//! Per-ROI metrics. All take a 2D `[n_rois, n_frames]` view and a resolved
//! frame range; all return one value per ROI.

use super::window::ResolvedRange;
use ndarray::{s, ArrayView2};

/// Identifier for which scalar metric to compute over a window.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Metric {
    /// Trace value at the window's start frame.
    DffAtFrame,
    /// Mean of the trace over the window.
    DffWindowMean,
    /// Maximum of the trace over the window.
    DffWindowPeak,
    /// Trapezoidal integral over the window. Units: ΔF/F · seconds when a
    /// frame_rate is provided, else ΔF/F · frames.
    Auc,
    /// Frames (or seconds if frame_rate is set) from window start to peak.
    LatencyToPeak,
    /// 10→90 % rise time (frames or seconds).
    RiseTime,
}

/// Single-frame ΔF/F at `frame_idx` for every ROI.
pub fn dff_at_frame(data: ArrayView2<f32>, frame_idx: usize) -> Vec<f32> {
    (0..data.nrows()).map(|r| data[[r, frame_idx]]).collect()
}

/// Mean of `data[roi, start..end]` for every ROI.
pub fn dff_window_mean(data: ArrayView2<f32>, range: ResolvedRange) -> Vec<f32> {
    let n = range.len() as f32;
    (0..data.nrows())
        .map(|r| {
            let row = data.slice(s![r, range.start..range.end]);
            row.iter().copied().fold(0.0_f32, |a, b| a + b) / n
        })
        .collect()
}

/// Maximum of `data[roi, start..end]` for every ROI.
pub fn dff_window_peak(data: ArrayView2<f32>, range: ResolvedRange) -> Vec<f32> {
    (0..data.nrows())
        .map(|r| {
            let row = data.slice(s![r, range.start..range.end]);
            row.iter().copied().fold(f32::NEG_INFINITY, f32::max)
        })
        .collect()
}

/// Trapezoidal area-under-curve over `[start, end)`.
///
/// If `frame_rate` is given, units are ΔF/F · seconds (dt = 1/fr).
/// Otherwise dt = 1, so units are ΔF/F · frames.
pub fn auc_trapezoid(
    data: ArrayView2<f32>,
    range: ResolvedRange,
    frame_rate: Option<f32>,
) -> Vec<f32> {
    let dt = frame_rate.map(|f| 1.0 / f).unwrap_or(1.0);
    (0..data.nrows())
        .map(|r| {
            let row = data.slice(s![r, range.start..range.end]);
            let n = row.len();
            if n < 2 {
                return 0.0;
            }
            let mut sum = 0.0_f32;
            for i in 0..n - 1 {
                sum += 0.5 * (row[i] + row[i + 1]);
            }
            sum * dt
        })
        .collect()
}

/// Time from the window's `start` to the per-ROI peak inside the window.
/// Returned in seconds if `frame_rate` is given, else in frames.
pub fn latency_to_peak_frames(
    data: ArrayView2<f32>,
    range: ResolvedRange,
    frame_rate: Option<f32>,
) -> Vec<f32> {
    let dt = frame_rate.map(|f| 1.0 / f).unwrap_or(1.0);
    (0..data.nrows())
        .map(|r| {
            let row = data.slice(s![r, range.start..range.end]);
            let mut best_i = 0_usize;
            let mut best_v = f32::NEG_INFINITY;
            for (i, &v) in row.iter().enumerate() {
                if v > best_v {
                    best_v = v;
                    best_i = i;
                }
            }
            best_i as f32 * dt
        })
        .collect()
}

/// Time taken to rise from `lo_frac` × peak to `hi_frac` × peak inside the
/// window. Default usage: 10 → 90 %. Returns 0.0 if the rise cannot be
/// resolved (e.g. peak at frame 0, monotonic decrease).
pub fn rise_time_frames(
    data: ArrayView2<f32>,
    range: ResolvedRange,
    frame_rate: Option<f32>,
    lo_frac: f32,
    hi_frac: f32,
) -> Vec<f32> {
    let dt = frame_rate.map(|f| 1.0 / f).unwrap_or(1.0);
    (0..data.nrows())
        .map(|r| {
            let row = data.slice(s![r, range.start..range.end]);
            let n = row.len();
            if n == 0 {
                return 0.0;
            }
            // Find peak.
            let mut peak_i = 0_usize;
            let mut peak_v = f32::NEG_INFINITY;
            for (i, &v) in row.iter().enumerate() {
                if v > peak_v {
                    peak_v = v;
                    peak_i = i;
                }
            }
            if peak_v <= 0.0 || peak_i == 0 {
                return 0.0;
            }
            let lo = peak_v * lo_frac;
            let hi = peak_v * hi_frac;
            // Walk forward from window start to peak.
            let mut lo_i = None;
            let mut hi_i = None;
            for i in 0..=peak_i {
                let v = row[i];
                if lo_i.is_none() && v >= lo {
                    lo_i = Some(i);
                }
                if hi_i.is_none() && v >= hi {
                    hi_i = Some(i);
                    break;
                }
            }
            match (lo_i, hi_i) {
                (Some(a), Some(b)) if b >= a => (b - a) as f32 * dt,
                _ => 0.0,
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use ndarray::arr2;

    fn rng(start: usize, end: usize) -> ResolvedRange {
        ResolvedRange { start, end }
    }

    #[test]
    fn window_mean_matches_hand_calc() {
        // 2 ROIs × 5 frames
        let a = arr2(&[[0.0_f32, 1.0, 2.0, 3.0, 4.0], [4.0, 3.0, 2.0, 1.0, 0.0]]);
        // Mean over frames 1..4 = (1+2+3)/3 = 2.0 for both
        let m = dff_window_mean(a.view(), rng(1, 4));
        assert!((m[0] - 2.0).abs() < 1e-6);
        assert!((m[1] - 2.0).abs() < 1e-6);
    }

    #[test]
    fn window_peak_picks_max() {
        let a = arr2(&[[0.0_f32, 0.5, 0.9, 0.3, 0.0], [-1.0, -0.5, 0.0, 0.5, 0.2]]);
        let p = dff_window_peak(a.view(), rng(0, 5));
        assert!((p[0] - 0.9).abs() < 1e-6);
        assert!((p[1] - 0.5).abs() < 1e-6);
    }

    #[test]
    fn auc_constant_signal_equals_height_times_width() {
        // f(t)=2 over 4 samples at 10 Hz → trapezoidal = 2 * (4-1)/10 = 0.6
        let a = arr2(&[[2.0_f32, 2.0, 2.0, 2.0]]);
        let auc = auc_trapezoid(a.view(), rng(0, 4), Some(10.0));
        assert!((auc[0] - 0.6).abs() < 1e-6);
    }

    #[test]
    fn auc_units_in_frames_without_frame_rate() {
        // f(t)=1 over 5 samples → trapezoidal = 4
        let a = arr2(&[[1.0_f32, 1.0, 1.0, 1.0, 1.0]]);
        let auc = auc_trapezoid(a.view(), rng(0, 5), None);
        assert!((auc[0] - 4.0).abs() < 1e-6);
    }

    #[test]
    fn auc_handles_short_window() {
        let a = arr2(&[[1.0_f32, 2.0]]);
        let auc = auc_trapezoid(a.view(), rng(0, 1), Some(10.0));
        assert_eq!(auc[0], 0.0); // single sample → 0
    }

    #[test]
    fn latency_to_peak_returns_seconds_with_frame_rate() {
        // Peak is at index 3 within window starting at 0.
        let a = arr2(&[[0.0_f32, 0.1, 0.2, 1.0, 0.3]]);
        let lat = latency_to_peak_frames(a.view(), rng(0, 5), Some(10.0));
        assert!((lat[0] - 0.3).abs() < 1e-6); // 3 frames / 10 Hz
    }

    #[test]
    fn rise_time_10_to_90_pct() {
        // Linear ramp 0..1 over 11 frames; peak=1 at i=10.
        // 10% of peak = 0.1 reached at i=1; 90% at i=9.
        // rise = 8 frames.
        let mut row: Vec<f32> = (0..11).map(|i| i as f32 / 10.0).collect();
        let _ = &mut row;
        let a = ndarray::Array2::from_shape_vec((1, 11), row).unwrap();
        let rt = rise_time_frames(a.view(), rng(0, 11), None, 0.1, 0.9);
        assert!((rt[0] - 8.0).abs() < 1e-6);
    }

    #[test]
    fn rise_time_zero_when_peak_at_start() {
        let a = arr2(&[[1.0_f32, 0.5, 0.0]]);
        let rt = rise_time_frames(a.view(), rng(0, 3), None, 0.1, 0.9);
        assert_eq!(rt[0], 0.0);
    }

    #[test]
    fn dff_at_frame_picks_column() {
        let a = arr2(&[[0.0_f32, 10.0, 20.0], [1.0, 11.0, 21.0]]);
        let v = dff_at_frame(a.view(), 1);
        assert_eq!(v, vec![10.0, 11.0]);
    }
}
