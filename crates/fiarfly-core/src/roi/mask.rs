//! Precomputed pixel mask for fast signal extraction.

/// A rasterized ROI mask: a list of [row, col] pixel indices.
#[derive(Debug, Clone)]
pub struct RoiMask {
    pub roi_id: String,
    /// Pixel indices as [row, col] (image coordinates).
    pub pixels: Vec<[usize; 2]>,
}

impl RoiMask {
    pub fn new(roi_id: impl Into<String>) -> Self {
        Self { roi_id: roi_id.into(), pixels: Vec::new() }
    }

    pub fn n_pixels(&self) -> usize {
        self.pixels.len()
    }
}
