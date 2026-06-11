//! ROI management — polygons, masks, serialization.
//!
//! Implementation spec: IMPLEMENTATION_GUIDE.md §3

mod mask;
mod polygon;

pub use mask::RoiMask;
pub use polygon::{Roi, RoiSet};

use serde::{Deserialize, Serialize};

/// Imaging modality — affects default pipeline parameters.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum Modality {
    TwoPhotonInVivo,
    TwoPhotonInVitro,
    OnePhotonInVivo,
    OnePhotonWideField,
}

impl Modality {
    /// Whether neuropil correction is recommended for this modality.
    pub fn use_neuropil_correction(&self) -> bool {
        matches!(self, Modality::TwoPhotonInVivo | Modality::TwoPhotonInVitro)
    }

    /// Recommended baseline percentile for ΔF/F.
    pub fn baseline_percentile(&self) -> f32 {
        8.0
    }

    /// Recommended rolling window for baseline estimation (frames).
    pub fn baseline_window(&self) -> usize {
        match self {
            Modality::TwoPhotonInVitro => 100,
            Modality::OnePhotonWideField => 200,
            _ => 300,
        }
    }
}
