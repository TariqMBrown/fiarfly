//! Calcium transient / event detection.
//!
//! Detects discrete calcium events in ΔF/F traces using threshold-based
//! peak finding.  For each detected event the algorithm reports:
//!   - onset frame (first frame crossing the threshold on the rising edge)
//!   - peak frame and amplitude
//!   - offset frame (last frame above threshold on the decay)
//!   - half-decay frame (first frame below peak/2 after the peak)
//!
//! Reference approach: comparable to the standard used in Suite2p and CaImAn
//! event detection; see also Greenberg & Bhatt 2022 (CaPTure toolbox).

use crate::FiarflyError;
use ndarray::{s, Array2};
use rayon::prelude::*;

// ─────────────────────────────────────────────────────────────────────────────
// Public types
// ─────────────────────────────────────────────────────────────────────────────

/// Parameters controlling event detection.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct EventDetectionParams {
    /// Minimum ΔF/F value to consider as an event threshold.
    /// Expressed as a multiple of the per-ROI baseline noise std.
    /// Typical range: 2–4.  Default: 2.5.
    pub threshold_std: f32,

    /// Minimum number of frames above threshold for a valid event.
    /// Prevents single-frame noise spikes from being reported.  Default: 2.
    pub min_duration_frames: usize,

    /// Minimum interval between event peaks (frames).
    /// Events with peaks closer together are merged into the larger one.
    /// Default: 5.
    pub min_interval_frames: usize,
}

impl Default for EventDetectionParams {
    fn default() -> Self {
        Self {
            threshold_std: 2.5,
            min_duration_frames: 2,
            min_interval_frames: 5,
        }
    }
}

/// A single detected calcium event for one ROI.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct CaEvent {
    /// Frame index of the event onset (threshold crossing, ascending).
    pub onset_frame: usize,
    /// Frame index of the peak.
    pub peak_frame: usize,
    /// Peak ΔF/F amplitude.
    pub amplitude: f32,
    /// Frame index of the event offset (last frame above threshold).
    pub offset_frame: usize,
    /// Duration in frames (offset − onset + 1).
    pub duration_frames: usize,
    /// Frame index where signal drops to ≤ 50 % of peak amplitude
    /// (relative to baseline), or `None` if not reached within the recording.
    pub half_decay_frame: Option<usize>,
}

/// Event detection results for all ROIs.
pub struct EventResult {
    /// Detected events per ROI, in frame order.
    pub events: Vec<Vec<CaEvent>>,
    /// Per-ROI baseline noise standard deviation used for thresholding.
    pub noise_std: Vec<f32>,
    /// Per-ROI threshold (absolute ΔF/F units).
    pub threshold: Vec<f32>,
}

impl EventResult {
    /// Total number of events across all ROIs.
    pub fn total_events(&self) -> usize {
        self.events.iter().map(|e| e.len()).sum()
    }

    /// Mean event frequency in events per frame for ROI `i`.
    /// Multiply by `frame_rate` to convert to Hz.
    pub fn event_rate(&self, roi: usize, n_frames: usize) -> f32 {
        if n_frames == 0 {
            return 0.0;
        }
        self.events
            .get(roi)
            .map(|e| e.len() as f32 / n_frames as f32)
            .unwrap_or(0.0)
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Public entry point
// ─────────────────────────────────────────────────────────────────────────────

/// Detect calcium transients in ΔF/F traces.
///
/// `delta_f` shape: `[n_rois, n_frames]`.
pub fn detect_events(
    delta_f: &Array2<f32>,
    params: &EventDetectionParams,
) -> Result<EventResult, FiarflyError> {
    let (n_rois, n_frames) = delta_f.dim();
    if n_frames == 0 {
        return Err(FiarflyError::InvalidParameter("Empty trace array".into()));
    }

    let results: Vec<(Vec<CaEvent>, f32, f32)> = (0..n_rois)
        .into_par_iter()
        .map(|i| {
            let trace: Vec<f32> = delta_f.slice(s![i, ..]).to_vec();
            detect_single_roi(&trace, params)
        })
        .collect();

    let mut events_out = Vec::with_capacity(n_rois);
    let mut noise_std = Vec::with_capacity(n_rois);
    let mut threshold = Vec::with_capacity(n_rois);

    for (evs, ns, thr) in results {
        events_out.push(evs);
        noise_std.push(ns);
        threshold.push(thr);
    }

    Ok(EventResult {
        events: events_out,
        noise_std,
        threshold,
    })
}

// ─────────────────────────────────────────────────────────────────────────────
// Per-ROI detection
// ─────────────────────────────────────────────────────────────────────────────

/// Returns (events, noise_std, threshold).
fn detect_single_roi(y: &[f32], params: &EventDetectionParams) -> (Vec<CaEvent>, f32, f32) {
    let n = y.len();
    let ns = noise_std_mad(y);
    let thr = params.threshold_std * ns;

    if n == 0 || thr <= 0.0 {
        return (vec![], ns, thr);
    }

    // ── Step 1: find all contiguous above-threshold regions ──────────────────
    let mut events: Vec<CaEvent> = Vec::new();
    let mut in_event = false;
    let mut onset = 0;

    let mut i = 0;
    while i < n {
        if !in_event && y[i] >= thr {
            in_event = true;
            onset = i;
        } else if in_event && (y[i] < thr || i == n - 1) {
            // Event ends at i-1 (or n-1 if we hit the end while above threshold).
            let offset = if y[i] >= thr { i } else { i - 1 };
            let duration = offset - onset + 1;

            if duration >= params.min_duration_frames {
                // Find peak within [onset, offset].
                let (peak_frame, &amplitude) = y[onset..=offset]
                    .iter()
                    .enumerate()
                    .max_by(|a, b| a.1.partial_cmp(b.1).unwrap_or(std::cmp::Ordering::Equal))
                    .map(|(k, v)| (onset + k, v))
                    .unwrap();

                let half_decay_frame = find_half_decay(y, peak_frame, amplitude);

                events.push(CaEvent {
                    onset_frame: onset,
                    peak_frame,
                    amplitude,
                    offset_frame: offset,
                    duration_frames: duration,
                    half_decay_frame,
                });
            }
            in_event = false;
        }
        i += 1;
    }

    // ── Step 2: merge events whose peaks are too close ───────────────────────
    let events = merge_nearby_events(events, params.min_interval_frames);

    (events, ns, thr)
}

/// Merge events whose peaks are within `min_interval` frames of each other.
/// When two events are merged, the one with the higher amplitude is kept.
fn merge_nearby_events(mut events: Vec<CaEvent>, min_interval: usize) -> Vec<CaEvent> {
    if events.len() < 2 {
        return events;
    }
    events.sort_by_key(|e| e.peak_frame);

    let mut merged: Vec<CaEvent> = Vec::with_capacity(events.len());
    for ev in events {
        if let Some(last) = merged.last_mut() {
            if ev.peak_frame - last.peak_frame < min_interval {
                // Keep the higher-amplitude event, expand onset/offset.
                if ev.amplitude > last.amplitude {
                    let new_onset = last.onset_frame.min(ev.onset_frame);
                    let new_offset = last.offset_frame.max(ev.offset_frame);
                    *last = ev;
                    last.onset_frame = new_onset;
                    last.offset_frame = new_offset;
                    last.duration_frames = last.offset_frame - last.onset_frame + 1;
                } else {
                    last.onset_frame = last.onset_frame.min(ev.onset_frame);
                    last.offset_frame = last.offset_frame.max(ev.offset_frame);
                    last.duration_frames = last.offset_frame - last.onset_frame + 1;
                }
                continue;
            }
        }
        merged.push(ev);
    }
    merged
}

/// Find the first frame after `peak_frame` where the signal drops to ≤ 50 %
/// of the peak amplitude.
fn find_half_decay(y: &[f32], peak_frame: usize, amplitude: f32) -> Option<usize> {
    let half = amplitude * 0.5;
    for t in (peak_frame + 1)..y.len() {
        if y[t] <= half {
            return Some(t);
        }
    }
    None
}

// ─────────────────────────────────────────────────────────────────────────────
// Noise estimation (shared helper — also used in quality.rs)
// ─────────────────────────────────────────────────────────────────────────────

/// Estimate per-trace noise std using median absolute deviation of first
/// differences.  Robust to calcium transients.
pub(crate) fn noise_std_mad(y: &[f32]) -> f32 {
    if y.len() < 2 {
        return 1.0;
    }
    let mut diffs: Vec<f32> = y.windows(2).map(|w| (w[1] - w[0]).abs()).collect();
    diffs.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    let median = diffs[diffs.len() / 2];
    (median * 1.4826 / std::f32::consts::SQRT_2).max(1e-6)
}

// ─────────────────────────────────────────────────────────────────────────────
// Tests
// ─────────────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use ndarray::Array2;

    fn flat_with_peaks(n: usize, peaks: &[(usize, f32)]) -> Vec<f32> {
        let mut y = vec![0.0_f32; n];
        for &(t, amp) in peaks {
            // Simple decaying transient
            for k in 0..20usize {
                if t + k < n {
                    y[t + k] += amp * 0.85_f32.powi(k as i32);
                }
            }
        }
        y
    }

    #[test]
    fn detects_two_events() {
        // Noise std ≈ 0 (flat background), two large transients.
        let y = flat_with_peaks(200, &[(30, 1.0), (130, 1.0)]);
        let arr = Array2::from_shape_vec((1, 200), y).unwrap();
        let params = EventDetectionParams {
            threshold_std: 0.5, // very low — noise is ~0
            min_duration_frames: 1,
            min_interval_frames: 5,
        };
        let result = detect_events(&arr, &params).unwrap();
        let events = &result.events[0];
        assert_eq!(events.len(), 2, "Expected 2 events, got {}", events.len());
        assert!(
            events[0].peak_frame >= 28 && events[0].peak_frame <= 32,
            "First peak frame {} not near 30",
            events[0].peak_frame
        );
        assert!(
            events[1].peak_frame >= 128 && events[1].peak_frame <= 132,
            "Second peak frame {} not near 130",
            events[1].peak_frame
        );
    }

    #[test]
    fn no_events_flat_trace() {
        let arr = Array2::<f32>::zeros((1, 100));
        let params = EventDetectionParams::default();
        let result = detect_events(&arr, &params).unwrap();
        assert_eq!(result.events[0].len(), 0);
    }

    #[test]
    fn event_amplitude_correct() {
        let mut y = vec![0.0_f32; 100];
        y[40] = 2.0;
        y[41] = 1.8;
        y[42] = 1.5;
        let arr = Array2::from_shape_vec((1, 100), y).unwrap();
        let params = EventDetectionParams {
            threshold_std: 0.5,
            min_duration_frames: 1,
            min_interval_frames: 5,
        };
        let result = detect_events(&arr, &params).unwrap();
        let events = &result.events[0];
        assert!(!events.is_empty());
        assert!(
            (events[0].amplitude - 2.0).abs() < 0.01,
            "Amplitude {} ≠ 2.0",
            events[0].amplitude
        );
    }
}
