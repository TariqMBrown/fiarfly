//! ΔF/F computation and neuropil correction.

use crate::FiarflyError;
use ndarray::{s, Array2};
use rayon::prelude::*;

/// Parameters for ΔF/F baseline estimation.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct DeltaFParams {
    /// Percentile of trace window used as F0 baseline (0–100). Default: 8.0.
    pub baseline_percentile: f32,
    /// Rolling window width in frames for percentile estimation. Default: 300.
    pub window_frames: usize,
}

impl Default for DeltaFParams {
    fn default() -> Self {
        Self {
            baseline_percentile: 8.0,
            window_frames: 300,
        }
    }
}

/// Compute ΔF/F = (F - F0) / F0 for each ROI using a rolling percentile baseline.
///
/// Input/output shape: `[n_rois, n_frames]`
pub fn compute_delta_f(
    raw: &Array2<f32>,
    params: &DeltaFParams,
) -> Result<Array2<f32>, FiarflyError> {
    let (n_rois, n_frames) = raw.dim();
    if n_frames == 0 {
        return Err(FiarflyError::InvalidParameter("Empty trace array".into()));
    }
    let half_w = params.window_frames / 2;
    let pct = params.baseline_percentile;

    // Parallelize over ROIs.
    let rows: Vec<Vec<f32>> = (0..n_rois)
        .into_par_iter()
        .map(|roi| {
            let trace = raw.slice(s![roi, ..]);
            (0..n_frames)
                .map(|t| {
                    let start = t.saturating_sub(half_w);
                    let end = (t + half_w + 1).min(n_frames);
                    let window: Vec<f32> = trace.slice(s![start..end]).to_vec();
                    let f0 = percentile_sorted(&window, pct);
                    if f0.abs() > 1e-10 {
                        (trace[t] - f0) / f0
                    } else {
                        0.0
                    }
                })
                .collect()
        })
        .collect();

    let mut result = Array2::zeros((n_rois, n_frames));
    for (roi, row) in rows.iter().enumerate() {
        for (t, &val) in row.iter().enumerate() {
            result[[roi, t]] = val;
        }
    }
    Ok(result)
}

/// Subtract scaled neuropil: F_corrected = soma - r * neuropil.
///
/// All arrays shape: `[n_rois, n_frames]`.
pub fn apply_neuropil_correction(
    soma: &Array2<f32>,
    neuropil: &Array2<f32>,
    r: f32,
) -> Result<Array2<f32>, FiarflyError> {
    if soma.shape() != neuropil.shape() {
        return Err(FiarflyError::DimensionMismatch(format!(
            "soma {:?} ≠ neuropil {:?}",
            soma.shape(),
            neuropil.shape()
        )));
    }
    Ok(soma - &(neuropil * r))
}

// --- helpers ---

fn percentile_sorted(data: &[f32], p: f32) -> f32 {
    if data.is_empty() {
        return 0.0;
    }
    let mut sorted = data.to_vec();
    sorted.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    let idx = ((p / 100.0) * (sorted.len() - 1) as f32).round() as usize;
    sorted[idx.min(sorted.len() - 1)]
}

// --- tests ---

#[cfg(test)]
mod tests {
    use super::*;
    use ndarray::array;

    #[test]
    fn delta_f_step_response() {
        // Flat baseline at 0.5, then a step up to 1.0 halfway through.
        let n = 100usize;
        let mut raw = Array2::<f32>::zeros((1, n));
        for t in 0..n {
            raw[[0, t]] = if t < n / 2 { 0.5 } else { 1.0 };
        }
        let params = DeltaFParams {
            baseline_percentile: 8.0,
            window_frames: 20,
        };
        let df = compute_delta_f(&raw, &params).unwrap();
        // Before step: ΔF/F ≈ 0.
        assert!(
            df[[0, 10]].abs() < 0.1,
            "expected ~0 before step, got {}",
            df[[0, 10]]
        );
        // At t=55 the rolling window [45, 66) spans both pre- and post-step frames,
        // so F0 is driven by the pre-step percentile (≈ 0.5) → ΔF/F ≈ 1.0.
        assert!(
            df[[0, 55]] > 0.5,
            "expected >0.5 near step, got {}",
            df[[0, 55]]
        );
    }

    #[test]
    fn neuropil_correction_subtracts() {
        let soma = Array2::from_elem((1, 4), 1.0f32);
        let neuropil = Array2::from_elem((1, 4), 0.5f32);
        let corrected = apply_neuropil_correction(&soma, &neuropil, 0.7).unwrap();
        let expected = 1.0 - 0.7 * 0.5;
        for &v in corrected.iter() {
            assert!((v - expected).abs() < 1e-5);
        }
    }
}
