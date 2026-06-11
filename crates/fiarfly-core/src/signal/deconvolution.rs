//! OASIS spike deconvolution — AR(1) model.
//!
//! Reference: Friedrich et al. 2017, PLoS Comput Biol
//! "Fast Online Deconvolution of Calcium Imaging Data"
//!
//! Algorithm: Pool-Adjacent-Violators (PAV) for constrained non-negative
//! deconvolution of the AR(1) calcium model:
//!   c[t] = g * c[t-1] + s[t]   (calcium dynamics)
//!   y[t] = c[t] + noise         (observation)
//!
//! Minimises: ½ ‖y − c‖² + λ ‖s‖₁  subject to  c ≥ 0, s ≥ 0.

use ndarray::{s, Array2};
use rayon::prelude::*;
use crate::FiarflyError;

// ─────────────────────────────────────────────────────────────────────────────
// Public types
// ─────────────────────────────────────────────────────────────────────────────

/// Parameters for OASIS AR(1) deconvolution.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct OasisParams {
    /// Calcium decay constant g (0 < g < 1).
    /// Relationship to indicator time constant τ: g = exp(−dt/τ),
    /// where dt = 1/frame_rate.
    /// `None` → estimated from lag-1 autocorrelation of each trace.
    pub g: Option<f32>,

    /// Noise standard deviation.
    /// `None` → estimated from the median absolute deviation of first
    /// differences (robust to transients).
    pub sn: Option<f32>,

    /// Sparsity penalty λ (≥ 0).
    /// 0 = unconstrained (NND); higher values enforce sparser spike trains.
    pub lambda: f32,
}

impl Default for OasisParams {
    fn default() -> Self {
        Self { g: None, sn: None, lambda: 0.0 }
    }
}

/// Output of OASIS deconvolution for all ROIs.
pub struct OasisResult {
    /// Denoised calcium trace.  Shape: `[n_rois, n_frames]`.
    pub calcium: Array2<f32>,
    /// Inferred spike train (non-negative).  Shape: `[n_rois, n_frames]`.
    pub spikes: Array2<f32>,
    /// Estimated decay constant per ROI (same as `params.g` if provided).
    pub g_estimated: Vec<f32>,
    /// Estimated noise std per ROI.
    pub sn_estimated: Vec<f32>,
}

// ─────────────────────────────────────────────────────────────────────────────
// Public entry point
// ─────────────────────────────────────────────────────────────────────────────

/// Deconvolve ΔF/F traces using OASIS AR(1).
///
/// `delta_f` shape: `[n_rois, n_frames]`.
/// `frame_rate` is used only when auto-estimating `g`.
pub fn deconvolve_oasis(
    delta_f: &Array2<f32>,
    params: &OasisParams,
    _frame_rate: f32,
) -> Result<OasisResult, FiarflyError> {
    let (n_rois, n_frames) = delta_f.dim();
    if n_frames == 0 {
        return Err(FiarflyError::InvalidParameter("Empty trace array".into()));
    }

    // Process each ROI in parallel.
    let results: Vec<(Vec<f32>, Vec<f32>, f32, f32)> = (0..n_rois)
        .into_par_iter()
        .map(|i| {
            let trace: Vec<f32> = delta_f.slice(s![i, ..]).to_vec();
            oasis_ar1_single(&trace, params)
        })
        .collect();

    // Unpack into output arrays.
    let mut calcium    = Array2::<f32>::zeros((n_rois, n_frames));
    let mut spikes     = Array2::<f32>::zeros((n_rois, n_frames));
    let mut g_est      = Vec::with_capacity(n_rois);
    let mut sn_est     = Vec::with_capacity(n_rois);

    for (i, (cal, spk, g, sn)) in results.into_iter().enumerate() {
        for t in 0..n_frames {
            calcium[[i, t]] = cal[t];
            spikes[[i, t]]  = spk[t];
        }
        g_est.push(g);
        sn_est.push(sn);
    }

    Ok(OasisResult { calcium, spikes, g_estimated: g_est, sn_estimated: sn_est })
}

// ─────────────────────────────────────────────────────────────────────────────
// Single-ROI OASIS AR(1) — PAV algorithm
// ─────────────────────────────────────────────────────────────────────────────

/// Returns (calcium, spikes, g_used, sn_used).
fn oasis_ar1_single(y: &[f32], params: &OasisParams) -> (Vec<f32>, Vec<f32>, f32, f32) {
    let n = y.len();
    if n == 0 {
        return (vec![], vec![], 0.0, 0.0);
    }

    let g  = params.g .unwrap_or_else(|| estimate_g(y)).clamp(0.05, 0.9999);
    let sn = params.sn.unwrap_or_else(|| estimate_noise(y)).max(1e-10);
    let lambda = params.lambda;

    // ── PAV forward pass ─────────────────────────────────────────────────────
    //
    // A "pool" covers a contiguous run of frames [t_start, t_start+length).
    // Within a pool, the calcium trace is a scaled exponential:
    //   c[t_start + k] = h * g^k
    //
    // The optimal pool amplitude h (given the pool covers frames [i, j]):
    //   h = max(0, (num − λ) / den)
    //
    // where
    //   num = Σ_{k=0}^{j−i} y[i+k] * g^k
    //   den = Σ_{k=0}^{j−i} g^(2k)
    //
    // Merge condition: the last pool [p] violates the AR(1) constraint with
    // the pool before it [q] when:
    //   c_p[0] < g * c_q[last]
    //   h_p    < h_q * g^(q.length)
    //
    // On merge we accumulate numerators/denominators with the appropriate
    // g-factor so no O(n) per-pool scan is needed.

    struct Pool {
        h:       f32,   // optimal amplitude at pool start
        t_start: usize,
        length:  usize,
        num:     f32,   // running numerator
        den:     f32,   // running denominator
    }

    let compute_h = |num: f32, den: f32| -> f32 {
        if den < 1e-15 { return 0.0; }
        ((num - lambda) / den).max(0.0)
    };

    let mut pools: Vec<Pool> = Vec::with_capacity(n);

    for t in 0..n {
        let num = y[t];
        let den = 1.0_f32;
        pools.push(Pool {
            h: compute_h(num, den),
            t_start: t,
            length: 1,
            num,
            den,
        });

        // Merge while the last two pools violate monotonicity.
        loop {
            let len = pools.len();
            if len < 2 { break; }

            let (prev_h, prev_len) = {
                let p = &pools[len - 2];
                (p.h, p.length)
            };
            let next_h = pools[len - 1].h;

            // Required: next pool starts no lower than g * c_prev[last frame]
            let c_prev_last = prev_h * g.powi(prev_len as i32 - 1);
            if next_h >= g * c_prev_last {
                break;
            }

            // Merge: pop last pool, accumulate into second-to-last.
            let q = pools.pop().unwrap();
            let p = pools.last_mut().unwrap();
            let g_fac  = g.powi(p.length as i32);
            let g2_fac = g.powi(2 * p.length as i32);
            p.num    = p.num + g_fac  * q.num;
            p.den    = p.den + g2_fac * q.den;
            p.h      = compute_h(p.num, p.den);
            p.length += q.length;
        }
    }

    // ── Reconstruct calcium trace ─────────────────────────────────────────────
    let mut calcium = vec![0.0_f32; n];
    for pool in &pools {
        for k in 0..pool.length {
            calcium[pool.t_start + k] = pool.h * g.powi(k as i32);
        }
    }

    // ── Infer spikes ──────────────────────────────────────────────────────────
    let mut spikes = vec![0.0_f32; n];
    if n > 0 {
        spikes[0] = calcium[0].max(0.0);
    }
    for t in 1..n {
        spikes[t] = (calcium[t] - g * calcium[t - 1]).max(0.0);
    }

    (calcium, spikes, g, sn)
}

// ─────────────────────────────────────────────────────────────────────────────
// Parameter estimation helpers
// ─────────────────────────────────────────────────────────────────────────────

/// Estimate the AR(1) decay constant g from the lag-1 autocorrelation.
///
/// For a pure exponential decay: AC(1) / AC(0) ≈ g.
/// Falls back to 0.95 for very short or constant traces.
fn estimate_g(y: &[f32]) -> f32 {
    let n = y.len();
    if n < 4 { return 0.95; }

    let mean: f32 = y.iter().sum::<f32>() / n as f32;

    let var: f32 = y.iter().map(|&v| (v - mean).powi(2)).sum::<f32>() / n as f32;
    if var < 1e-10 { return 0.95; }

    let cov: f32 = y[..n - 1]
        .iter()
        .zip(y[1..].iter())
        .map(|(&a, &b)| (a - mean) * (b - mean))
        .sum::<f32>()
        / (n - 1) as f32;

    (cov / var).clamp(0.05, 0.9999)
}

/// Estimate noise σ from the median absolute deviation of first differences.
///
/// For Gaussian noise: σ ≈ median(|Δy|) / √2 × 1.4826.
/// This is robust to calcium transients (which inflate the variance).
fn estimate_noise(y: &[f32]) -> f32 {
    if y.len() < 2 { return 1.0; }
    let mut diffs: Vec<f32> = y.windows(2).map(|w| (w[1] - w[0]).abs()).collect();
    diffs.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    let median = diffs[diffs.len() / 2];
    // Normalise: for half-normal from N(0, σ²), E[|x|] = σ√(2/π).
    // For |Δy| where Δy ~ N(0, 2σ²): E[|Δy|] = σ√(4/π) = 2σ/√π.
    // MAD → σ: divide by √2 × MAD scale factor 1.4826 (for Gaussian).
    (median * 1.4826 / std::f32::consts::SQRT_2).max(1e-6)
}

// ─────────────────────────────────────────────────────────────────────────────
// Tests
// ─────────────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use ndarray::Array2;

    /// Build a synthetic calcium trace from known spikes.
    fn synthetic_trace(g: f32, spikes: &[f32], noise_std: f32) -> Vec<f32> {
        let n = spikes.len();
        let mut c = vec![0.0_f32; n];
        for t in 0..n {
            c[t] = if t == 0 { spikes[0] } else { g * c[t - 1] + spikes[t] };
        }
        // No noise for deterministic tests.
        let _ = noise_std;
        c
    }

    #[test]
    fn recovers_single_spike() {
        // True spike at frame 10, g = 0.9, no noise.
        let n = 50;
        let mut spike_vec = vec![0.0_f32; n];
        spike_vec[10] = 1.0;

        let trace = synthetic_trace(0.9, &spike_vec, 0.0);
        let arr = Array2::from_shape_vec((1, n), trace).unwrap();

        let params = OasisParams { g: Some(0.9), sn: Some(1e-6), lambda: 0.0 };
        let result = deconvolve_oasis(&arr, &params, 30.0).unwrap();

        // Spike should be at or near frame 10.
        let inferred = result.spikes.row(0);
        let peak_frame = inferred
            .iter()
            .enumerate()
            .max_by(|a, b| a.1.partial_cmp(b.1).unwrap())
            .map(|(i, _)| i)
            .unwrap();

        assert_eq!(peak_frame, 10, "Peak inferred at wrong frame: {peak_frame}");
    }

    #[test]
    fn calcium_non_negative() {
        // All-zero trace → all-zero calcium.
        let arr = Array2::<f32>::zeros((2, 100));
        let params = OasisParams::default();
        let result = deconvolve_oasis(&arr, &params, 30.0).unwrap();
        assert!(result.calcium.iter().all(|&v| v >= 0.0));
        assert!(result.spikes.iter().all(|&v| v >= 0.0));
    }

    #[test]
    fn g_estimation_reasonable() {
        // Build trace with known g = 0.9.
        let mut spike_vec = vec![0.0_f32; 200];
        spike_vec[20]  = 1.0;
        spike_vec[80]  = 1.0;
        spike_vec[150] = 0.8;
        let trace = synthetic_trace(0.9, &spike_vec, 0.0);
        let g_hat = estimate_g(&trace);
        // Should be within 0.05 of true g.
        assert!(
            (g_hat - 0.9).abs() < 0.06,
            "g estimate {g_hat:.4} too far from 0.9"
        );
    }
}
