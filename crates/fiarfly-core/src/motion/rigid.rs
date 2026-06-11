//! Rigid motion correction via FFT cross-correlation (NoRMCorre).
//!
//! Each frame is corrected against a static initial template using
//! full-image FFT cross-correlation. All frames are processed in parallel
//! with rayon via `map_init`, so each worker reuses a single `FftPlanner`
//! across the frames it handles. The template's FFT is static, so it is
//! computed once up front and reused for every frame.

use super::{CorrectionResult, MotionCorrectionParams, ShiftMap};
use crate::{FiarflyError, Frame, ImageStack};
use ndarray::s;
use rayon::prelude::*;
use rustfft::{num_complex::Complex, FftPlanner};

pub fn correct_rigid(
    stack: &ImageStack,
    params: &MotionCorrectionParams,
    progress: Option<&std::sync::mpsc::Sender<f32>>,
) -> Result<CorrectionResult, FiarflyError> {
    let (n_frames, height, width) = stack.dim();
    if n_frames == 0 {
        return Err(FiarflyError::InvalidParameter("Empty stack".into()));
    }

    if let Some(tx) = progress {
        let _ = tx.send(0.0);
    }

    // Build initial template from first bin_width frames.
    let bin = params.bin_width.min(n_frames);
    let mut template = Frame::zeros((height, width));
    for i in 0..bin {
        template += &stack.slice(s![i, .., ..]);
    }
    template /= bin as f32;

    let max_shift = params.max_shift;

    // The template is static, so transform it once and reuse the spectrum
    // for every frame's cross-correlation (saves one fft2 per frame).
    let t_freq = {
        let mut planner = FftPlanner::<f32>::new();
        fft2(&template, &mut planner)
    };

    // Process all frames in parallel against the static template. `map_init`
    // gives each rayon worker one FftPlanner that it reuses across frames,
    // rather than rebuilding (and re-caching plans) on every iteration.
    let results: Vec<(Frame, [f64; 2], f32)> = (0..n_frames)
        .into_par_iter()
        .map_init(FftPlanner::<f32>::new, |planner, i| {
            let frame = stack.slice(s![i, .., ..]).to_owned();
            let ncc = cross_correlate_2d_freq(&frame, &t_freq, planner);
            let (dy, dx) = find_peak_with_subpixel(&ncc, height, width, max_shift);
            let shifted = shift_frame(&frame, dy, dx);
            let score = pearson_correlation(shifted.view(), template.view());
            (shifted, [dy, dx], score)
        })
        .collect();

    if let Some(tx) = progress {
        let _ = tx.send(1.0);
    }

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
        shifts: ShiftMap {
            shifts,
            field: None,
        },
        correlation_scores: scores,
    })
}

/// Shift a 2D frame by fractional `[dy, dx]` using bilinear interpolation.
/// Out-of-bounds pixels are filled with the frame mean.
pub fn shift_frame(frame: &Frame, dy: f64, dx: f64) -> Frame {
    let (height, width) = frame.dim();
    let fill = frame.mean().unwrap_or(0.0);
    let mut out = Frame::from_elem((height, width), fill);

    for oy in 0..height {
        for ox in 0..width {
            let sy = oy as f64 - dy;
            let sx = ox as f64 - dx;
            if sy < 0.0 || sx < 0.0 || sy >= (height - 1) as f64 || sx >= (width - 1) as f64 {
                continue;
            }
            let y0 = sy.floor() as usize;
            let x0 = sx.floor() as usize;
            let fy = sy.fract();
            let fx = sx.fract();
            let v = (1.0 - fy) * (1.0 - fx) * frame[[y0, x0]] as f64
                + (1.0 - fy) * fx * frame[[y0, x0 + 1]] as f64
                + fy * (1.0 - fx) * frame[[y0 + 1, x0]] as f64
                + fy * fx * frame[[y0 + 1, x0 + 1]] as f64;
            out[[oy, ox]] = v as f32;
        }
    }
    out
}

/// 2D cross-correlation via FFT: returns NCC as a flat row-major Vec.
/// Peak location (py, px) in the output corresponds to shift (dy, dx).
pub fn cross_correlate_2d(
    frame: &Frame,
    template: &Frame,
    planner: &mut FftPlanner<f32>,
) -> Vec<f32> {
    let t_freq = fft2(template, planner);
    cross_correlate_2d_freq(frame, &t_freq, planner)
}

/// Like [`cross_correlate_2d`] but takes the template already in the frequency
/// domain, so a static template can be transformed once and reused per frame.
pub fn cross_correlate_2d_freq(
    frame: &Frame,
    t_freq: &[Complex<f32>],
    planner: &mut FftPlanner<f32>,
) -> Vec<f32> {
    let (h, w) = frame.dim();
    let mut f_freq = fft2(frame, planner);
    // F * conj(T) in frequency domain = cross-correlation in spatial domain.
    for (f, t) in f_freq.iter_mut().zip(t_freq) {
        *f *= t.conj();
    }
    ifft2(&mut f_freq, h, w, planner)
}

// --- FFT internals ---

fn fft2(frame: &Frame, planner: &mut FftPlanner<f32>) -> Vec<Complex<f32>> {
    let (h, w) = frame.dim();
    let mut buf: Vec<Complex<f32>> = frame.iter().map(|&x| Complex::new(x, 0.0)).collect();

    // Row-wise forward FFT.
    let row_fft = planner.plan_fft_forward(w);
    for row in buf.chunks_mut(w) {
        row_fft.process(row);
    }

    // Column-wise forward FFT.
    let col_fft = planner.plan_fft_forward(h);
    let mut col = vec![Complex::new(0.0f32, 0.0); h];
    for c in 0..w {
        for r in 0..h {
            col[r] = buf[r * w + c];
        }
        col_fft.process(&mut col);
        for r in 0..h {
            buf[r * w + c] = col[r];
        }
    }
    buf
}

fn ifft2(
    buf: &mut [Complex<f32>],
    h: usize,
    w: usize,
    planner: &mut FftPlanner<f32>,
) -> Vec<f32> {
    // Column-wise inverse FFT.
    let col_ifft = planner.plan_fft_inverse(h);
    let mut col = vec![Complex::new(0.0f32, 0.0); h];
    for c in 0..w {
        for r in 0..h {
            col[r] = buf[r * w + c];
        }
        col_ifft.process(&mut col);
        for r in 0..h {
            buf[r * w + c] = col[r];
        }
    }

    // Row-wise inverse FFT.
    let row_ifft = planner.plan_fft_inverse(w);
    for row in buf.chunks_mut(w) {
        row_ifft.process(row);
    }

    let scale = 1.0 / (h * w) as f32;
    buf.iter().map(|c| c.re * scale).collect()
}

// --- Peak detection ---

/// Find cross-correlation peak within `max_shift` pixels of the origin (with wrap),
/// then refine to sub-pixel precision via Gaussian fit.
pub fn find_peak_with_subpixel(ncc: &[f32], h: usize, w: usize, max_shift: usize) -> (f64, f64) {
    let mut best = f32::NEG_INFINITY;
    let mut py = 0usize;
    let mut px = 0usize;

    for r in 0..h {
        let ay = if r <= h / 2 { r } else { h - r }; // unsigned distance from 0
        if ay > max_shift {
            continue;
        }
        for c in 0..w {
            let ax = if c <= w / 2 { c } else { w - c };
            if ax > max_shift {
                continue;
            }
            let v = ncc[r * w + c];
            if v > best {
                best = v;
                py = r;
                px = c;
            }
        }
    }

    // Convert to signed shift.
    let dy = if py <= h / 2 {
        py as f64
    } else {
        py as f64 - h as f64
    };
    let dx = if px <= w / 2 {
        px as f64
    } else {
        px as f64 - w as f64
    };

    // Gaussian sub-pixel refinement.
    let slog = |v: f32| if v > 1e-10 { v.ln() as f64 } else { -23.0_f64 };

    let sub_y = if py > 0 && py < h - 1 {
        let vm = slog(ncc[(py - 1) * w + px]);
        let v0 = slog(ncc[py * w + px]);
        let vp = slog(ncc[(py + 1) * w + px]);
        let d = vm - 2.0 * v0 + vp;
        if d.abs() > 1e-10 {
            (0.5 * (vm - vp) / d).clamp(-1.0, 1.0)
        } else {
            0.0
        }
    } else {
        0.0
    };

    let sub_x = if px > 0 && px < w - 1 {
        let vm = slog(ncc[py * w + px - 1]);
        let v0 = slog(ncc[py * w + px]);
        let vp = slog(ncc[py * w + px + 1]);
        let d = vm - 2.0 * v0 + vp;
        if d.abs() > 1e-10 {
            (0.5 * (vm - vp) / d).clamp(-1.0, 1.0)
        } else {
            0.0
        }
    } else {
        0.0
    };

    (dy + sub_y, dx + sub_x)
}

// --- Quality metric ---

pub fn pearson_correlation(a: ndarray::ArrayView2<f32>, b: ndarray::ArrayView2<f32>) -> f32 {
    let n = a.len();
    if n == 0 {
        return 0.0;
    }
    let ma = a.iter().sum::<f32>() / n as f32;
    let mb = b.iter().sum::<f32>() / n as f32;
    let (mut cov, mut va, mut vb) = (0.0f64, 0.0f64, 0.0f64);
    for (&av, &bv) in a.iter().zip(b.iter()) {
        let da = (av - ma) as f64;
        let db = (bv - mb) as f64;
        cov += da * db;
        va += da * da;
        vb += db * db;
    }
    let d = (va * vb).sqrt();
    if d > 1e-15 {
        (cov / d) as f32
    } else {
        0.0
    }
}

// --- Tests ---

#[cfg(test)]
mod tests {
    use super::*;
    use ndarray::Array3;

    #[test]
    fn shift_frame_zero_shift_is_identity() {
        let frame = Frame::from_shape_fn((8, 8), |(r, c)| (r + c) as f32 * 0.1);
        let shifted = shift_frame(&frame, 0.0, 0.0);
        for ((r, c), &v) in frame.indexed_iter() {
            // Interior pixels should be reproduced exactly.
            if r > 0 && r < 7 && c > 0 && c < 7 {
                assert!((shifted[[r, c]] - v).abs() < 1e-5, "mismatch at ({r},{c})");
            }
        }
    }

    #[test]
    fn shift_frame_integer_shift() {
        // Uniform frame — shift by any amount should give same mean.
        let frame = Frame::from_elem((16, 16), 0.5_f32);
        let shifted = shift_frame(&frame, 3.0, 4.0);
        let mean: f32 = shifted.iter().sum::<f32>() / shifted.len() as f32;
        assert!((mean - 0.5).abs() < 0.05);
    }

    #[test]
    fn rigid_correction_smoke() {
        // Stack with known constant shift of 2 pixels in y.
        let n = 5;
        let (h, w) = (32usize, 32usize);
        let mut stack = Array3::<f32>::zeros([n, h, w]);
        // Reference pattern: bright square in upper-left.
        for i in 0..n {
            let offset = if i == 0 { 0i64 } else { 2i64 };
            for r in 2..8usize {
                let sr = (r as i64 + offset) as usize;
                if sr < h {
                    for c in 2..8usize {
                        stack[[i, sr, c]] = 1.0;
                    }
                }
            }
        }
        let params = MotionCorrectionParams {
            mode: super::super::MotionMode::Rigid,
            max_shift: 5,
            bin_width: 1,
            ..Default::default()
        };
        let result = correct_rigid(&stack, &params, None).unwrap();
        assert_eq!(result.corrected.dim(), (n, h, w));
        // Shifts for frames 1..n should be approximately [-2, 0].
        for i in 1..n {
            let [dy, _dx] = result.shifts.shifts[i];
            assert!(
                dy.abs() > 0.5,
                "Expected nonzero shift for frame {i}, got {dy}"
            );
        }
    }
}
