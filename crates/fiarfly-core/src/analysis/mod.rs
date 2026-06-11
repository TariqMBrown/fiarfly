//! Analysis primitives — per-ROI metrics over user-defined frame windows.
//!
//! These are pure functions on `ArrayView2<f32>` (shape `[n_rois, n_frames]`)
//! plus a window specification. They have no GUI dependency and are the
//! computational core of the v0.2 Analysis panel and Stats panel.
//!
//! All metrics return one value per ROI (`Vec<f32>`).

pub mod window;
mod metric;

pub use window::{ResolvedRange, Window};
pub use metric::{
    auc_trapezoid, dff_at_frame, dff_window_mean, dff_window_peak,
    latency_to_peak_frames, rise_time_frames, Metric,
};

use ndarray::ArrayView2;
use crate::FiarflyError;

/// Compute one metric over one window for every ROI in `data`.
///
/// `data` shape: `[n_rois, n_frames]`. Returns a vector of length `n_rois`.
pub fn compute(
    data: ArrayView2<f32>,
    window: &Window,
    metric: Metric,
) -> Result<Vec<f32>, FiarflyError> {
    let (_n_rois, n_frames) = data.dim();
    let resolved = window.resolve(n_frames)?;
    Ok(match metric {
        Metric::DffAtFrame => {
            // For a single-frame metric we treat the window's start as the
            // frame of interest.
            (0..data.nrows()).map(|r| data[[r, resolved.start]]).collect()
        }
        Metric::DffWindowMean   => dff_window_mean(data, resolved),
        Metric::DffWindowPeak   => dff_window_peak(data, resolved),
        Metric::Auc             => auc_trapezoid(data, resolved, window.frame_rate),
        Metric::LatencyToPeak   => latency_to_peak_frames(data, resolved, window.frame_rate),
        Metric::RiseTime        => rise_time_frames(data, resolved, window.frame_rate, 0.1, 0.9),
    })
}

#[cfg(test)]
mod integration_tests {
    use super::*;
    use ndarray::Array2;

    /// End-to-end: 30 Hz, 60 s recording, two windows (10–15 s, 25–35 s).
    /// ROI 0: zero baseline + 1.0 step in 25–35 s window.
    /// ROI 1: 0.5 step in 10–15 s window only.
    /// Verifies that AUC and mean both pick the right window and that the
    /// values match the integral of the step.
    #[test]
    fn paired_window_use_case() {
        let fr = 30.0_f32;
        let n_frames = (60.0 * fr) as usize; // 1800
        let mut data = Array2::<f32>::zeros((2, n_frames));

        let f1_start = (10.0 * fr) as usize;
        let f1_end   = (15.0 * fr) as usize;
        let f2_start = (25.0 * fr) as usize;
        let f2_end   = (35.0 * fr) as usize;
        for f in f2_start..f2_end { data[[0, f]] = 1.0; }
        for f in f1_start..f1_end { data[[1, f]] = 0.5; }

        let w1 = Window::seconds(10.0, 15.0, fr);
        let w2 = Window::seconds(25.0, 35.0, fr);

        let mean1 = compute(data.view(), &w1, Metric::DffWindowMean).unwrap();
        let mean2 = compute(data.view(), &w2, Metric::DffWindowMean).unwrap();
        // ROI 0 active only in window 2; ROI 1 active only in window 1.
        assert!(mean1[0] < 0.01);
        assert!((mean1[1] - 0.5).abs() < 0.01);
        assert!((mean2[0] - 1.0).abs() < 0.01);
        assert!(mean2[1]  < 0.01);

        // AUC should approximately equal step_height × duration.
        let auc1 = compute(data.view(), &w1, Metric::Auc).unwrap();
        let auc2 = compute(data.view(), &w2, Metric::Auc).unwrap();
        // ROI 1 over 10–15s: 0.5 × 5s = 2.5 ΔF/F·s
        assert!((auc1[1] - 2.5).abs() < 0.1);
        // ROI 0 over 25–35s: 1.0 × 10s = 10.0
        assert!((auc2[0] - 10.0).abs() < 0.1);
    }

    #[test]
    fn compute_propagates_window_errors() {
        let data = Array2::<f32>::zeros((2, 10));
        let bad = Window::frames(20, 30);
        assert!(compute(data.view(), &bad, Metric::DffWindowMean).is_err());
    }
}
