//! fiarfly-core — pure computation library for FIARfly.
//!
//! No GUI dependencies, no Python dependencies.
//! All modules are independently unit-testable.

pub mod analysis;
pub mod export;
pub mod io;
pub mod motion;
pub mod roi;
pub mod signal;
pub mod stats;

/// Canonical 3D image stack type: [frames, height, width].
pub type ImageStack = ndarray::Array3<f32>;

/// A single 2D frame: [height, width].
pub type Frame = ndarray::Array2<f32>;

/// Top-level error type for all fiarfly-core operations.
#[derive(thiserror::Error, Debug)]
pub enum FiarflyError {
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),

    #[error("TIFF decode error: {0}")]
    Tiff(#[from] tiff::TiffError),

    #[error("Unsupported format: {0}")]
    UnsupportedFormat(String),

    #[error("Dimension mismatch: {0}")]
    DimensionMismatch(String),

    #[error("Invalid parameter: {0}")]
    InvalidParameter(String),

    #[error("Serialization error: {0}")]
    Serialization(#[from] serde_json::Error),

    #[error("Export error: {0}")]
    Export(String),
}
