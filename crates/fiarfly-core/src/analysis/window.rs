//! Window specification — defines the `[start, end)` frame range over which
//! a metric is computed. Accepts either explicit frames or a time range
//! (seconds + frame_rate).

use crate::FiarflyError;

/// Half-open frame range `[start, end)` resolved at compute time.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ResolvedRange {
    pub start: usize,
    pub end: usize,
}

impl ResolvedRange {
    pub fn len(&self) -> usize {
        self.end - self.start
    }
    pub fn is_empty(&self) -> bool {
        self.end <= self.start
    }
}

/// User-facing window specification.
///
/// `frame_rate` is required when bounds are given in seconds. It is also
/// used by the time-axis-sensitive metrics (AUC, latency, rise-time) so
/// they can return values in seconds rather than frames.
#[derive(Debug, Clone)]
pub struct Window {
    pub bounds: WindowBounds,
    pub frame_rate: Option<f32>,
}

#[derive(Debug, Clone)]
pub enum WindowBounds {
    /// `[start_frame, end_frame_exclusive)`.
    Frames { start: usize, end: usize },
    /// `[start_secs, end_secs)`. Requires `frame_rate`.
    Seconds { start: f32, end: f32 },
}

impl Window {
    pub fn frames(start: usize, end: usize) -> Self {
        Self {
            bounds: WindowBounds::Frames { start, end },
            frame_rate: None,
        }
    }
    pub fn seconds(start: f32, end: f32, frame_rate: f32) -> Self {
        Self {
            bounds: WindowBounds::Seconds { start, end },
            frame_rate: Some(frame_rate),
        }
    }

    /// Validate against the actual trace length and return resolved frames.
    pub fn resolve(&self, n_frames: usize) -> Result<ResolvedRange, FiarflyError> {
        if n_frames == 0 {
            return Err(FiarflyError::InvalidParameter("empty trace".into()));
        }
        let (start, end) = match &self.bounds {
            WindowBounds::Frames { start, end } => (*start, *end),
            WindowBounds::Seconds { start, end } => {
                let fr = self.frame_rate.ok_or_else(|| {
                    FiarflyError::InvalidParameter(
                        "frame_rate required for seconds-based window".into(),
                    )
                })?;
                if fr <= 0.0 {
                    return Err(FiarflyError::InvalidParameter(
                        "frame_rate must be positive".into(),
                    ));
                }
                let s = (start * fr).round().max(0.0) as usize;
                let e = (end * fr).round().max(0.0) as usize;
                (s, e)
            }
        };
        if end <= start {
            return Err(FiarflyError::InvalidParameter(format!(
                "window end ({end}) must be > start ({start})"
            )));
        }
        let end = end.min(n_frames);
        if start >= n_frames {
            return Err(FiarflyError::InvalidParameter(format!(
                "window start ({start}) is past trace end ({n_frames})"
            )));
        }
        Ok(ResolvedRange { start, end })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn frames_window_resolves() {
        let w = Window::frames(10, 20);
        let r = w.resolve(100).unwrap();
        assert_eq!(r.start, 10);
        assert_eq!(r.end, 20);
        assert_eq!(r.len(), 10);
    }

    #[test]
    fn seconds_window_resolves_with_frame_rate() {
        // 10–15 seconds at 30 Hz = frames 300–450
        let w = Window::seconds(10.0, 15.0, 30.0);
        let r = w.resolve(1000).unwrap();
        assert_eq!(r.start, 300);
        assert_eq!(r.end, 450);
    }

    #[test]
    fn seconds_window_requires_frame_rate() {
        let w = Window {
            bounds: WindowBounds::Seconds {
                start: 0.0,
                end: 1.0,
            },
            frame_rate: None,
        };
        assert!(w.resolve(100).is_err());
    }

    #[test]
    fn empty_or_inverted_window_errors() {
        assert!(Window::frames(20, 20).resolve(100).is_err());
        assert!(Window::frames(20, 10).resolve(100).is_err());
    }

    #[test]
    fn end_clamped_to_trace_length() {
        let r = Window::frames(50, 200).resolve(100).unwrap();
        assert_eq!(r.start, 50);
        assert_eq!(r.end, 100);
    }

    #[test]
    fn start_past_end_errors() {
        assert!(Window::frames(150, 200).resolve(100).is_err());
    }

    #[test]
    fn empty_trace_errors() {
        assert!(Window::frames(0, 1).resolve(0).is_err());
    }
}
