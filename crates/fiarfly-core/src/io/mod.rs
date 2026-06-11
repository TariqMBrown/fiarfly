//! Image I/O — TIFF loading.
//!
//! See IMPLEMENTATION_GUIDE.md Phase 1 for full spec.

mod tiff;
pub mod project;
pub use tiff::{load_tiff, save_stack, TiffReader};
pub use project::{
    FrameLabelDef, NewRun, Project, ProjectFile, RunMetadata, PROJECT_VERSION,
};
