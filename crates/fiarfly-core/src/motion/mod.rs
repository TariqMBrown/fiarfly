//! Motion correction — rigid and non-rigid (NoRMCorre-style).
//!
//! Implementation spec: IMPLEMENTATION_GUIDE.md §2

mod nonrigid;
mod preprocess;
mod rigid;

pub use nonrigid::correct_nonrigid;
pub use preprocess::{spatial_highpass, SpatialFilterParams};
pub use rigid::{correct_rigid, pearson_correlation};

use ndarray::Array4;
use serde::{Deserialize, Serialize};

/// Parameters for motion correction.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MotionCorrectionParams {
    pub mode: MotionMode,
    /// Maximum allowed shift in pixels (per axis). Default: 10.
    pub max_shift: usize,
    /// Patch dimensions for non-rigid correction [height, width]. Default: [32, 32].
    pub grid_size: [usize; 2],
    /// Overlap between patches in pixels. Default: 8.
    pub overlap: usize,
    /// Number of frames averaged per template update step. Default: 50.
    pub bin_width: usize,
}

impl Default for MotionCorrectionParams {
    fn default() -> Self {
        Self {
            mode: MotionMode::Rigid,
            max_shift: 10,
            grid_size: [32, 32],
            overlap: 8,
            bin_width: 50,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum MotionMode {
    Rigid,
    NonRigid,
}

/// Per-frame shift information.
#[derive(Debug, Clone)]
pub struct ShiftMap {
    /// Rigid shifts: one `[dy, dx]` per frame.
    pub shifts: Vec<[f64; 2]>,
    /// Non-rigid displacement field: `[frames, patches_y, patches_x, 2]`.
    /// None for rigid correction.
    pub field: Option<Array4<f32>>,
}

/// Output of a motion correction run.
#[derive(Debug)]
pub struct CorrectionResult {
    pub corrected: crate::ImageStack,
    pub shifts: ShiftMap,
    /// Pearson correlation of each corrected frame with the template (0..1).
    pub correlation_scores: Vec<f32>,
}
