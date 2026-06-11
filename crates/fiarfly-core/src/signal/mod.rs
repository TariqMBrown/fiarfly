//! Signal extraction and processing.
//!
//! Implementation spec: IMPLEMENTATION_GUIDE.md §4

mod deconvolution;
mod delta_f;
mod events;
mod extraction;
mod quality;

pub use deconvolution::{deconvolve_oasis, OasisParams, OasisResult};
pub use delta_f::{apply_neuropil_correction, compute_delta_f, DeltaFParams};
pub use events::{detect_events, CaEvent, EventDetectionParams, EventResult};
pub use extraction::extract_raw_fluorescence;
pub use quality::{compute_quality, QualityParams, RoiQuality};
