//! Raw fluorescence extraction from ROI masks.

use crate::roi::RoiMask;
use crate::{FiarflyError, ImageStack};
use ndarray::{s, Array2};
use rayon::prelude::*;

/// For each ROI mask, compute the mean pixel intensity for every frame.
///
/// Returns an array of shape `[n_rois, n_frames]`.
/// Frames are processed in parallel via rayon.
pub fn extract_raw_fluorescence(
    stack: &ImageStack,
    masks: &[RoiMask],
) -> Result<Array2<f32>, FiarflyError> {
    if masks.is_empty() {
        return Err(FiarflyError::InvalidParameter(
            "No ROI masks provided".into(),
        ));
    }
    let (n_frames, height, width) = stack.dim();

    // Validate pixel indices.
    for mask in masks {
        for &[r, c] in &mask.pixels {
            if r >= height || c >= width {
                return Err(FiarflyError::DimensionMismatch(format!(
                    "ROI '{}' has pixel ({r},{c}) outside {}×{} image",
                    mask.roi_id, height, width
                )));
            }
        }
    }

    // Process frames in parallel; each produces a Vec<f32> of length n_rois.
    let rows: Vec<Vec<f32>> = (0..n_frames)
        .into_par_iter()
        .map(|i| {
            let frame = stack.slice(s![i, .., ..]);
            masks
                .iter()
                .map(|mask| {
                    if mask.pixels.is_empty() {
                        return 0.0f32;
                    }
                    let sum: f32 = mask.pixels.iter().map(|&[r, c]| frame[[r, c]]).sum();
                    sum / mask.pixels.len() as f32
                })
                .collect()
        })
        .collect();

    // Transpose: rows[frame][roi] → result[roi][frame].
    let n_rois = masks.len();
    let mut result = Array2::zeros((n_rois, n_frames));
    for (t, frame_vals) in rows.iter().enumerate() {
        for (roi_idx, &val) in frame_vals.iter().enumerate() {
            result[[roi_idx, t]] = val;
        }
    }
    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::roi::RoiMask;
    use ndarray::Array3;

    #[test]
    fn constant_frame_extracts_correct_mean() {
        // 5 frames of constant value 0.5; 4×4 ROI at top-left.
        let mut stack = Array3::<f32>::zeros([5, 16, 16]);
        stack.fill(0.5);

        let mut mask = RoiMask::new("roi_001");
        for r in 0..4usize {
            for c in 0..4usize {
                mask.pixels.push([r, c]);
            }
        }

        let raw = extract_raw_fluorescence(&stack, &[mask]).unwrap();
        assert_eq!(raw.dim(), (1, 5));
        for t in 0..5 {
            assert!((raw[[0, t]] - 0.5).abs() < 1e-5);
        }
    }
}
