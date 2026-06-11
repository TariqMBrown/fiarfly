//! Per-ROI quality metrics for calcium imaging traces.
//!
//! Computed from raw fluorescence or ΔF/F traces.  These metrics are used to
//! distinguish genuine neural signals from neuropil, motion artefacts, or
//! out-of-focus cells without requiring manual inspection.
//!
//! Metrics implemented:
//!   - SNR          : signal-to-noise ratio (peak / noise std)
//!   - skewness     : right-skewed distributions indicate genuine Ca transients
//!   - active_frames: fraction of frames exceeding 2× noise std
//!   - peak_dff     : 99th-percentile ΔF/F (robust peak amplitude)
//!
//! Reference: Stringer & Pachitariu 2019 (Suite2p); Chen et al. 2013 (GCaMP6).

use ndarray::{s, Array2};
use rayon::prelude::*;
use crate::FiarflyError;
use super::events::noise_std_mad;

// ─────────────────────────────────────────────────────────────────────────────
// Public types
// ─────────────────────────────────────────────────────────────────────────────

/// Quality metrics for a single ROI.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct RoiQuality {
    /// Signal-to-noise ratio: 99th-percentile ΔF/F / noise std.
    /// Values > 3 are typically considered acceptable; > 5 are good.
    pub snr: f32,

    /// Pearson skewness: (mean − mode_approx) / std.
    /// Positive skewness (> ~0.5) indicates right-tailed distributions
    /// consistent with sparse calcium transients.  Neuropil / background
    /// tends to be Gaussian (skewness ≈ 0) or left-skewed.
    pub skewness: f32,

    /// Fraction of frames where ΔF/F > 2× noise std.
    /// Useful as a sanity check: very high fractions (> 0.5) may indicate
    /// a contaminated or always-active ROI.
    pub active_fraction: f32,

    /// Robust peak ΔF/F: 99th percentile of the trace.
    pub peak_dff: f32,

    /// Baseline noise standard deviation (MAD-based).
    pub noise_std: f32,

    /// Simple pass/fail: `true` if snr > `threshold_snr` AND skewness > 0.
    pub passes: bool,
}

/// Parameters for quality scoring.
#[derive(Debug, Clone)]
pub struct QualityParams {
    /// Minimum SNR to `passes = true`.  Default: 3.0.
    pub threshold_snr: f32,
}

impl Default for QualityParams {
    fn default() -> Self {
        Self { threshold_snr: 3.0 }
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Public entry point
// ─────────────────────────────────────────────────────────────────────────────

/// Compute quality metrics for each ROI.
///
/// `delta_f` shape: `[n_rois, n_frames]`.
pub fn compute_quality(
    delta_f: &Array2<f32>,
    params: &QualityParams,
) -> Result<Vec<RoiQuality>, FiarflyError> {
    let (n_rois, n_frames) = delta_f.dim();
    if n_frames == 0 {
        return Err(FiarflyError::InvalidParameter("Empty trace array".into()));
    }

    let metrics: Vec<RoiQuality> = (0..n_rois)
        .into_par_iter()
        .map(|i| {
            let trace: Vec<f32> = delta_f.slice(s![i, ..]).to_vec();
            quality_single(&trace, params)
        })
        .collect();

    Ok(metrics)
}

// ─────────────────────────────────────────────────────────────────────────────
// Per-ROI computation
// ─────────────────────────────────────────────────────────────────────────────

fn quality_single(y: &[f32], params: &QualityParams) -> RoiQuality {
    let n = y.len();

    if n == 0 {
        return RoiQuality {
            snr: 0.0, skewness: 0.0, active_fraction: 0.0,
            peak_dff: 0.0, noise_std: 0.0, passes: false,
        };
    }

    let ns = noise_std_mad(y);

    // 99th-percentile peak (robust against single-frame artefacts).
    let peak = percentile99(y);

    // SNR
    let snr = if ns > 1e-10 { peak / ns } else { 0.0 };

    // Skewness: (mean − mode_approx) / std.
    // We use the Pearson median skewness: 3 × (mean − median) / std.
    let mean: f32 = y.iter().sum::<f32>() / n as f32;
    let var: f32  = y.iter().map(|&v| (v - mean).powi(2)).sum::<f32>() / n as f32;
    let std_dev   = var.sqrt().max(1e-10);
    let median    = median_of(y);
    let skewness  = 3.0 * (mean - median) / std_dev;

    // Active fraction.
    let active_threshold = 2.0 * ns;
    let active_frames    = y.iter().filter(|&&v| v > active_threshold).count();
    let active_fraction  = active_frames as f32 / n as f32;

    let passes = snr >= params.threshold_snr && skewness > 0.0;

    RoiQuality { snr, skewness, active_fraction, peak_dff: peak, noise_std: ns, passes }
}

// ─────────────────────────────────────────────────────────────────────────────
// Helpers
// ─────────────────────────────────────────────────────────────────────────────

fn percentile99(y: &[f32]) -> f32 {
    if y.is_empty() { return 0.0; }
    let mut sorted = y.to_vec();
    sorted.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    let idx = ((0.99 * (sorted.len() - 1) as f32) as usize).min(sorted.len() - 1);
    sorted[idx]
}

fn median_of(y: &[f32]) -> f32 {
    if y.is_empty() { return 0.0; }
    let mut sorted = y.to_vec();
    sorted.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    let mid = sorted.len() / 2;
    if sorted.len() % 2 == 0 {
        (sorted[mid - 1] + sorted[mid]) * 0.5
    } else {
        sorted[mid]
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Tests
// ─────────────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use ndarray::Array2;

    fn make_spiky_trace(n: usize, spike_frames: &[usize]) -> Vec<f32> {
        let mut y = vec![0.0_f32; n];
        for &t in spike_frames {
            for k in 0..15usize {
                if t + k < n {
                    y[t + k] += 3.0 * 0.85_f32.powi(k as i32);
                }
            }
        }
        y
    }

    #[test]
    fn spiky_roi_high_snr_positive_skewness() {
        let y = make_spiky_trace(300, &[50, 150, 250]);
        let arr = Array2::from_shape_vec((1, 300), y).unwrap();
        let q = compute_quality(&arr, &QualityParams::default()).unwrap();
        assert!(q[0].snr > 3.0, "SNR {} should be > 3", q[0].snr);
        assert!(q[0].skewness > 0.0, "Skewness {} should be positive", q[0].skewness);
        assert!(q[0].passes);
    }

    #[test]
    fn flat_roi_fails() {
        let arr = Array2::<f32>::zeros((1, 200));
        let q = compute_quality(&arr, &QualityParams::default()).unwrap();
        assert!(!q[0].passes, "Flat trace should fail quality check");
    }

    #[test]
    fn snr_scales_with_amplitude() {
        // Use the make_spiky_trace helper so the 99th-percentile captures real signal.
        let y_low  = make_spiky_trace(300, &[50, 150, 250]);
        // Scale up by 5× for the "high" ROI.
        let y_high: Vec<f32> = y_low.iter().map(|&v| v * 5.0).collect();
        let arr = Array2::from_shape_vec((2, 300), [y_low, y_high].concat()).unwrap();
        let q = compute_quality(&arr, &QualityParams::default()).unwrap();
        assert!(q[1].snr > q[0].snr, "Higher amplitude should yield higher SNR");
    }
}
