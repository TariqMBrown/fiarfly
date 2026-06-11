//! Spatial preprocessing for calcium imaging stacks.
//!
//! One-photon (miniscope) recordings lack optical sectioning, leading to
//! strong out-of-focus background fluorescence that contaminates cell signals.
//! A simple and effective remedy is a **spatial high-pass filter**:
//!   filtered[t, y, x] = raw[t, y, x] − blur(raw[t, y, x], σ)
//!
//! This subtracts the slowly-varying background while preserving sharp soma
//! signals.  Sigma (σ) should be larger than a cell diameter and smaller than
//! the spatial scale of background variation; 20–50 pixels is typical for
//! miniscope data (≈ 3–8 µm/px).
//!
//! Implementation: separable box-filter approximation to a Gaussian (3 passes
//! of a box filter with width w ≈ σ√12 gives σ_box ≈ σ to < 3 % error).
//! No external crates required.

use ndarray::{s, Array2};
use rayon::prelude::*;
use crate::{FiarflyError, ImageStack};

// ─────────────────────────────────────────────────────────────────────────────
// Public types
// ─────────────────────────────────────────────────────────────────────────────

/// Parameters for spatial high-pass filtering.
#[derive(Debug, Clone)]
pub struct SpatialFilterParams {
    /// Gaussian sigma for the blurring step (pixels).
    /// Typical range for 1-photon miniscope data: 20–60.  Default: 30.
    pub sigma: f32,

    /// After subtraction, clip negative values to zero.
    /// Recommended: `true` — negative values after background subtraction
    /// indicate photon shot noise; clamping prevents artefacts downstream.
    pub clip_negative: bool,
}

impl Default for SpatialFilterParams {
    fn default() -> Self {
        Self { sigma: 30.0, clip_negative: true }
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Public entry point
// ─────────────────────────────────────────────────────────────────────────────

/// Apply spatial high-pass filter to every frame of the stack.
///
/// Returns a new stack with the same shape; the input is not modified.
/// Processes frames in parallel via rayon.
pub fn spatial_highpass(
    stack: &ImageStack,
    params: &SpatialFilterParams,
) -> Result<ImageStack, FiarflyError> {
    let (n_frames, height, width) = stack.dim();
    if height == 0 || width == 0 {
        return Err(FiarflyError::InvalidParameter("Empty stack".into()));
    }
    if params.sigma <= 0.0 {
        return Err(FiarflyError::InvalidParameter(
            "sigma must be > 0".into(),
        ));
    }

    // Gaussian approximated by 3 passes of box filter.
    // Box width: w = round(σ × √12 / 3) rounded to nearest odd integer.
    let box_width = gaussian_to_box_width(params.sigma);

    // Process frames in parallel.
    let frames: Vec<Array2<f32>> = (0..n_frames)
        .into_par_iter()
        .map(|t| {
            let frame = stack.slice(s![t, .., ..]).to_owned();
            let background = triple_box_blur_2d(&frame, box_width);

            if params.clip_negative {
                Array2::from_shape_fn((height, width), |(r, c)| {
                    (frame[[r, c]] - background[[r, c]]).max(0.0)
                })
            } else {
                Array2::from_shape_fn((height, width), |(r, c)| {
                    frame[[r, c]] - background[[r, c]]
                })
            }
        })
        .collect();

    // Assemble result stack.
    let mut out = ImageStack::zeros((n_frames, height, width));
    for (t, frame) in frames.into_iter().enumerate() {
        out.slice_mut(s![t, .., ..]).assign(&frame);
    }
    Ok(out)
}

// ─────────────────────────────────────────────────────────────────────────────
// Box-filter Gaussian approximation
// ─────────────────────────────────────────────────────────────────────────────

/// Convert Gaussian sigma to a box-filter width that approximates it.
/// Three passes of a box filter of width `w` approximate a Gaussian with
/// σ² = w² / 12 × 3 = w² / 4, so w = 2σ (rounded up to the next odd integer).
fn gaussian_to_box_width(sigma: f32) -> usize {
    let w = (sigma * 2.0).round() as usize;
    if w % 2 == 0 { w + 1 } else { w }.max(3)
}

/// Apply three passes of a 1D box filter in each spatial dimension
/// (separable 2D convolution).
fn triple_box_blur_2d(frame: &Array2<f32>, w: usize) -> Array2<f32> {
    let mut buf = frame.to_owned();
    for _ in 0..3 {
        buf = box_blur_rows(&buf, w);
        buf = box_blur_cols(&buf, w);
    }
    buf
}

/// Box blur along rows (axis 1 = x direction).
fn box_blur_rows(frame: &Array2<f32>, w: usize) -> Array2<f32> {
    let (h, width) = frame.dim();
    let half = w / 2;
    let mut out = Array2::<f32>::zeros((h, width));

    for r in 0..h {
        let row = frame.row(r);
        // Sliding sum.
        let mut sum: f32 = 0.0;
        let mut count: usize = 0;

        for c in 0..width {
            // Add new right element.
            let right = c + half;
            if right < width {
                sum   += row[right];
                count += 1;
            }
            // Remove left element that's now out of window.
            if c >= half + 1 {
                let left = c - half - 1;
                sum   -= row[left];
                count -= 1;
            }
            out[[r, c]] = if count > 0 { sum / count as f32 } else { row[c] };
        }
    }
    out
}

/// Box blur along columns (axis 0 = y direction).
fn box_blur_cols(frame: &Array2<f32>, w: usize) -> Array2<f32> {
    let (h, width) = frame.dim();
    let half = w / 2;
    let mut out = Array2::<f32>::zeros((h, width));

    for c in 0..width {
        let mut sum: f32 = 0.0;
        let mut count: usize = 0;

        for r in 0..h {
            let bottom = r + half;
            if bottom < h {
                sum   += frame[[bottom, c]];
                count += 1;
            }
            if r >= half + 1 {
                let top = r - half - 1;
                sum   -= frame[[top, c]];
                count -= 1;
            }
            out[[r, c]] = if count > 0 { sum / count as f32 } else { frame[[r, c]] };
        }
    }
    out
}

// ─────────────────────────────────────────────────────────────────────────────
// Tests
// ─────────────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use ndarray::Array3;

    #[test]
    fn constant_background_removed() {
        // Uniform frame — after subtraction should be all zeros (or near zero).
        let mut stack = Array3::<f32>::zeros([3, 64, 64]);
        stack.fill(100.0);
        let params = SpatialFilterParams { sigma: 10.0, clip_negative: false };
        let out = spatial_highpass(&stack, &params).unwrap();
        for &v in out.iter() {
            assert!(v.abs() < 1.0, "Expected ~0 after removing flat background, got {v}");
        }
    }

    #[test]
    fn sharp_spot_preserved() {
        // Flat background at 100 + a bright 3×3 spot.  After high-pass,
        // the background should be removed and the spot should remain visible.
        let h = 64usize;
        let w = 64usize;
        let n = 1usize;
        let mut stack = Array3::<f32>::from_elem([n, h, w], 50.0_f32);
        // Add a bright spot in the centre.
        for r in 30..34 { for c in 30..34 { stack[[0, r, c]] = 200.0; } }

        let params = SpatialFilterParams { sigma: 15.0, clip_negative: true };
        let out = spatial_highpass(&stack, &params).unwrap();

        let spot_val   = out[[0, 31, 31]];
        let bg_val     = out[[0, 5, 5]];
        assert!(
            spot_val > bg_val + 10.0,
            "Spot ({spot_val:.2}) should be much brighter than background ({bg_val:.2})"
        );
    }

    #[test]
    fn clip_negative_works() {
        let mut stack = Array3::<f32>::zeros([1, 16, 16]);
        stack.fill(1.0);
        let params_clip = SpatialFilterParams { sigma: 5.0, clip_negative: true };
        let out = spatial_highpass(&stack, &params_clip).unwrap();
        assert!(out.iter().all(|&v| v >= 0.0), "All values should be ≥ 0 when clipping");
    }
}
