//! Non-rigid motion correction via patch-based FFT registration (NoRMCorre).
//!
//! The FOV is divided into a grid of overlapping patches.  Each patch is
//! independently aligned to the corresponding template patch (same algorithm
//! as rigid).  Patch shifts are bilinearly interpolated to produce a
//! per-pixel displacement field which is then applied to the frame.
//!
//! All frames are processed in parallel; each worker thread creates its own
//! FftPlanner.

use rustfft::FftPlanner;
use ndarray::{s, Array3};
use rayon::prelude::*;
use crate::{FiarflyError, Frame, ImageStack};
use super::{CorrectionResult, MotionCorrectionParams, ShiftMap};
use super::rigid::{cross_correlate_2d, find_peak_with_subpixel, pearson_correlation};

pub fn correct_nonrigid(
    stack: &ImageStack,
    params: &MotionCorrectionParams,
    progress: Option<&std::sync::mpsc::Sender<f32>>,
) -> Result<CorrectionResult, FiarflyError> {
    let (n_frames, height, width) = stack.dim();
    if n_frames == 0 {
        return Err(FiarflyError::InvalidParameter("Empty stack".into()));
    }

    if let Some(tx) = progress { let _ = tx.send(0.0); }

    let [gh, gw] = params.grid_size;
    if gh == 0 || gw == 0 {
        return Err(FiarflyError::InvalidParameter("grid_size must be > 0".into()));
    }

    // Number of patches in each dimension.
    let n_ph = height.div_ceil(gh);
    let n_pw = width.div_ceil(gw);

    // Build initial template from first bin_width frames.
    let bin = params.bin_width.min(n_frames);
    let mut template = Frame::zeros((height, width));
    for i in 0..bin {
        template += &stack.slice(s![i, .., ..]);
    }
    template /= bin as f32;

    let max_shift = params.max_shift;

    // Process all frames in parallel.
    let results: Vec<(Frame, [f64; 2], f32)> = (0..n_frames)
        .into_par_iter()
        .map(|i| {
            let frame = stack.slice(s![i, .., ..]).to_owned();
            let mut planner = FftPlanner::<f32>::new();

            // Compute per-patch shifts.
            let mut patch_shifts = Array3::<f64>::zeros([n_ph, n_pw, 2]);
            for ph in 0..n_ph {
                for pw in 0..n_pw {
                    let y0 = (ph * gh).min(height.saturating_sub(gh));
                    let y1 = (y0 + gh).min(height);
                    let x0 = (pw * gw).min(width.saturating_sub(gw));
                    let x1 = (x0 + gw).min(width);

                    let fp = frame.slice(s![y0..y1, x0..x1]).to_owned();
                    let tp = template.slice(s![y0..y1, x0..x1]).to_owned();

                    if fp.len() >= 4 {
                        let (ph_h, ph_w) = fp.dim();
                        let ncc = cross_correlate_2d(&fp, &tp, &mut planner);
                        let (dy, dx) = find_peak_with_subpixel(&ncc, ph_h, ph_w, max_shift);
                        patch_shifts[[ph, pw, 0]] = dy;
                        patch_shifts[[ph, pw, 1]] = dx;
                    }
                }
            }

            // Bilinearly interpolate patch shifts to per-pixel displacements.
            let disp = interpolate_displacement(&patch_shifts, height, width, gh, gw, n_ph, n_pw);

            // Apply displacement field.
            let shifted = apply_displacement(&frame, &disp);
            let score = pearson_correlation(shifted.view(), template.view());

            // Store mean shift for the ShiftMap.
            let mean_dy = disp.slice(s![.., .., 0]).mean().unwrap_or(0.0) as f64;
            let mean_dx = disp.slice(s![.., .., 1]).mean().unwrap_or(0.0) as f64;

            (shifted, [mean_dy, mean_dx], score)
        })
        .collect();

    if let Some(tx) = progress { let _ = tx.send(1.0); }

    let mut corrected = ImageStack::zeros([n_frames, height, width]);
    let mut shifts = Vec::with_capacity(n_frames);
    let mut scores = Vec::with_capacity(n_frames);
    for (i, (shifted_frame, shift, score)) in results.into_iter().enumerate() {
        corrected.slice_mut(s![i, .., ..]).assign(&shifted_frame);
        shifts.push(shift);
        scores.push(score);
    }

    Ok(CorrectionResult {
        corrected,
        shifts: ShiftMap { shifts, field: None },
        correlation_scores: scores,
    })
}

/// Bilinearly interpolate patch-level shifts `[n_ph, n_pw, 2]` to a
/// per-pixel displacement field `[height, width, 2]`.
fn interpolate_displacement(
    patch_shifts: &Array3<f64>,
    height: usize,
    width: usize,
    gh: usize,
    gw: usize,
    n_ph: usize,
    n_pw: usize,
) -> Array3<f32> {
    let mut field = Array3::<f32>::zeros([height, width, 2]);

    for y in 0..height {
        for x in 0..width {
            // Position in patch-grid coordinates.
            let py_f = y as f64 / gh as f64;
            let px_f = x as f64 / gw as f64;

            let py0 = (py_f.floor() as usize).min(n_ph.saturating_sub(1));
            let px0 = (px_f.floor() as usize).min(n_pw.saturating_sub(1));
            let py1 = (py0 + 1).min(n_ph.saturating_sub(1));
            let px1 = (px0 + 1).min(n_pw.saturating_sub(1));

            let fy = py_f - py0 as f64;
            let fx = px_f - px0 as f64;

            for d in 0..2usize {
                let v = (1.0 - fy) * (1.0 - fx) * patch_shifts[[py0, px0, d]]
                      + (1.0 - fy) *        fx  * patch_shifts[[py0, px1, d]]
                      +        fy  * (1.0 - fx) * patch_shifts[[py1, px0, d]]
                      +        fy  *        fx  * patch_shifts[[py1, px1, d]];
                field[[y, x, d]] = v as f32;
            }
        }
    }
    field
}

/// Apply a per-pixel displacement field `[height, width, 2]` to a frame via
/// bilinear interpolation.  Out-of-bounds samples use the frame mean.
fn apply_displacement(frame: &Frame, disp: &Array3<f32>) -> Frame {
    let (h, w) = frame.dim();
    let fill = frame.mean().unwrap_or(0.0);
    let mut out = Frame::from_elem((h, w), fill);

    for y in 0..h {
        for x in 0..w {
            let dy = disp[[y, x, 0]] as f64;
            let dx = disp[[y, x, 1]] as f64;
            let sy = y as f64 - dy;
            let sx = x as f64 - dx;
            if sy < 0.0 || sx < 0.0 || sy >= (h - 1) as f64 || sx >= (w - 1) as f64 {
                continue;
            }
            let y0 = sy.floor() as usize;
            let x0 = sx.floor() as usize;
            let fy = sy.fract();
            let fx = sx.fract();
            let v = (1.0 - fy) * (1.0 - fx) * frame[[y0,     x0    ]] as f64
                  + (1.0 - fy) *        fx  * frame[[y0,     x0 + 1]] as f64
                  +        fy  * (1.0 - fx) * frame[[y0 + 1, x0    ]] as f64
                  +        fy  *        fx  * frame[[y0 + 1, x0 + 1]] as f64;
            out[[y, x]] = v as f32;
        }
    }
    out
}
