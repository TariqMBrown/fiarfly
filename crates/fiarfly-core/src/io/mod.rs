//! Image I/O — TIFF loading.
//!
//! See IMPLEMENTATION_GUIDE.md Phase 1 for full spec.

pub mod project;
mod tiff;
pub use project::{FrameLabelDef, NewRun, Project, ProjectFile, RunMetadata, PROJECT_VERSION};
pub use tiff::{load_tiff, save_stack, TiffReader};
