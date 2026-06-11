//! Motion correction panel — single-file and batch processing.

use eframe::egui;
use egui_plot::{Line, Plot, PlotPoints};
use fiarfly_core::motion::{MotionCorrectionParams, MotionMode};
use crate::state::{ActivePanel, AppState, DialogRequest, WorkerHandle, WorkerOutput};
use std::path::PathBuf;

pub fn show(ui: &mut egui::Ui, state: &mut AppState) {
    egui::ScrollArea::vertical().show(ui, |ui| { show_inner(ui, state); });
}

fn show_inner(ui: &mut egui::Ui, state: &mut AppState) {
    ui.heading("Motion Correction");
    ui.add_space(8.0);

    // ── Algorithm ────────────────────────────────────────────────────────────
    ui.horizontal(|ui| {
        ui.label("Algorithm:");
        ui.radio_value(&mut state.params.motion.mode, MotionMode::Rigid,    "Rigid");
        ui.radio_value(&mut state.params.motion.mode, MotionMode::NonRigid, "Non-Rigid");
    });
    if state.params.motion.mode == MotionMode::NonRigid {
        ui.label(egui::RichText::new(
            "ℹ Non-rigid: patch-based correction. Slower but handles local motion."
        ).small().weak());
    }

    ui.add_space(8.0);
    ui.label(egui::RichText::new("Parameters").strong());

    egui::Grid::new("mc_params").num_columns(3).spacing([8.0, 4.0]).show(ui, |ui| {
        ui.label("Max shift (px):");
        ui.add(egui::DragValue::new(&mut state.params.motion.max_shift).range(1..=100));
        help_tip(ui,
            "Maximum allowed displacement (in pixels) that the algorithm will \
             search for between frames. Set this to roughly the largest expected \
             motion in your recording. Too small → motion not fully corrected. \
             Too large → risk of mis-registration to noise peaks. \
             Typical values: 10–30 px for in vivo 2-photon.");
        ui.end_row();

        ui.label("Template bin (frames):");
        ui.add(egui::DragValue::new(&mut state.params.motion.bin_width).range(1..=500));
        help_tip(ui,
            "Number of frames averaged together to build the reference template \
             used for cross-correlation. Larger bins produce a less noisy \
             template, reducing sensitivity to shot noise, but may blur fast \
             structural changes. Good starting point: 50–200 frames \
             (≈ a few seconds of data).");
        ui.end_row();

        let nonrigid = state.params.motion.mode == MotionMode::NonRigid;
        ui.add_enabled(nonrigid, egui::Label::new("Grid size [H × W] (px):"));
        ui.horizontal(|ui| {
            ui.add_enabled(nonrigid, egui::DragValue::new(&mut state.params.motion.grid_size[0]).range(8..=128));
            ui.add_enabled(nonrigid, egui::Label::new("×"));
            ui.add_enabled(nonrigid, egui::DragValue::new(&mut state.params.motion.grid_size[1]).range(8..=128));
        });
        help_tip(ui,
            "Patch size for non-rigid (piecewise) correction. The image is \
             divided into overlapping tiles of this size; each tile is \
             registered independently, then the displacement field is \
             interpolated across tile boundaries. Smaller patches capture \
             local motion better but increase compute time and are noisier \
             in low-SNR regions. Typical: 64×64 px.");
        ui.end_row();
    });

    ui.add_space(12.0);

    // ── 1-Photon spatial background removal ──────────────────────────────────
    egui::CollapsingHeader::new("1-Photon Pre-processing (Spatial High-pass Filter)").show(ui, |ui| {
        ui.label(egui::RichText::new(
            "Subtracts a spatially-blurred version of each frame to remove out-of-focus \
             background — strongly recommended for miniscope / 1-photon recordings. \
             Applied before motion correction."
        ).small().weak());
        ui.add_space(4.0);

        ui.checkbox(&mut state.params.apply_spatial_filter, "Enable spatial high-pass filter");

        if state.params.apply_spatial_filter {
            ui.add_space(4.0);
            egui::Grid::new("sf_params").num_columns(3).spacing([8.0, 4.0]).show(ui, |ui| {
                ui.label("Background sigma (px):");
                ui.add(
                    egui::DragValue::new(&mut state.params.spatial_filter.sigma)
                        .range(5.0..=200.0)
                        .speed(1.0),
                );
                help_tip(ui,
                    "Gaussian sigma for the blurring used to estimate background. \
                     Should be larger than a cell diameter and smaller than the \
                     spatial scale of background variation. Typical: 20–60 px for \
                     miniscope data. Larger σ preserves more low-frequency signal.");
                ui.end_row();

                ui.label("Clip negatives:");
                ui.checkbox(&mut state.params.spatial_filter.clip_negative, "");
                help_tip(ui,
                    "Clip values below zero to 0 after background subtraction. \
                     Recommended: on. Negative values after subtraction represent \
                     photon shot noise and can cause artefacts in downstream steps.");
                ui.end_row();
            });
        }
    });

    ui.add_space(8.0);

    // ── Single-file run ───────────────────────────────────────────────────────
    ui.label(egui::RichText::new("Single File").strong());
    ui.horizontal(|ui| {
        let can_run = state.tiff_reader.is_some() && state.worker.is_none();
        let label = if state.corrected.is_some() {
            "Re-run Motion Correction"
        } else {
            "Run Motion Correction"
        };
        if ui.add_enabled(can_run, egui::Button::new(label)).clicked() {
            spawn_motion_correction(state);
        }
        if state.worker.is_some() && !state.batch_running {
            if let Some(start) = state.worker_start {
                ui.label(format!("{:.0}s", start.elapsed().as_secs_f64()));
            }
            ui.add(egui::ProgressBar::new(state.progress)
                .desired_width(180.0)
                .animate(true)
                .text(format!("{:.0}%", state.progress * 100.0)));
        }
        if let Some(corrected) = &state.corrected {
            let n = corrected.dim().0;
            ui.label(egui::RichText::new(format!("✓ {n} frames corrected")).small().weak());
        }
    });

    // ── Quality plot ─────────────────────────────────────────────────────────
    if !state.shift_scores.is_empty() {
        ui.add_space(8.0);
        ui.label(egui::RichText::new("Quality: per-frame correlation with template").strong());

        let scores = &state.shift_scores;
        let mean_score = scores.iter().sum::<f32>() / scores.len() as f32;
        let min_score  = scores.iter().cloned().fold(f32::INFINITY, f32::min);
        let max_score  = scores.iter().cloned().fold(f32::NEG_INFINITY, f32::max);
        let variance   = scores.iter().map(|&s| (s - mean_score).powi(2)).sum::<f32>()
            / scores.len() as f32;
        let std_dev    = variance.sqrt();
        let threshold  = (mean_score - 2.0 * std_dev) as f64;
        let n_bad      = scores.iter().filter(|&&s| (s as f64) < threshold).count();

        ui.label(egui::RichText::new(format!(
            "Correlation  min: {min_score:.3}  mean: {mean_score:.3}  max: {max_score:.3}"
        )).small());

        Plot::new("quality_plot")
            .height(120.0)
            .allow_drag(false)
            .allow_zoom(false)
            .show(ui, |plot_ui| {
                let points: PlotPoints = scores.iter().enumerate()
                    .map(|(i, &s)| [i as f64, s as f64])
                    .collect();
                plot_ui.line(Line::new(points).name("correlation"));
                let thr_pts: PlotPoints = vec![
                    [0.0, threshold],
                    [scores.len() as f64, threshold],
                ].into();
                plot_ui.line(
                    Line::new(thr_pts)
                        .name(format!("threshold (mean−2σ = {threshold:.3})"))
                        .color(egui::Color32::RED),
                );
            });

        if n_bad > 0 {
            ui.label(egui::RichText::new(
                format!("⚠ {n_bad} frames below adaptive threshold.")
            ).color(egui::Color32::YELLOW));
        } else {
            ui.label(egui::RichText::new("✓ No outlier frames detected.").color(egui::Color32::GREEN));
        }
    }

    ui.add_space(16.0);
    ui.separator();

    // ── Batch processing ──────────────────────────────────────────────────────
    ui.label(egui::RichText::new("Batch Processing").strong());
    ui.add_space(4.0);
    ui.label(egui::RichText::new(
        "Correct multiple TIFF files. Each output is saved as <stem>_corrected.tiff \
         in the chosen output directory."
    ).small().weak());
    ui.add_space(6.0);

    ui.horizontal(|ui| {
        if ui.button("Add Files…").clicked() {
            state.pending_dialog = Some(DialogRequest::AddBatchFiles);
        }

        let out_label = state.batch_out_dir.as_deref()
            .and_then(|p| p.to_str())
            .unwrap_or("(same as input)");
        ui.label("Output dir:");
        ui.add(egui::TextEdit::singleline(&mut out_label.to_owned())
            .desired_width(200.0)
            .interactive(false));
        if ui.small_button("Choose…").clicked() {
            state.pending_dialog = Some(DialogRequest::PickBatchOutputDir);
        }
        if ui.small_button("Clear").clicked() {
            state.batch_out_dir = None;
        }
    });

    // Queue list
    if !state.batch_queue.is_empty() {
        let queue_h = (state.batch_queue.len() as f32 * 18.0 + 8.0).min(120.0);
        egui::ScrollArea::vertical()
            .id_salt("batch_list")
            .max_height(queue_h)
            .show(ui, |ui| {
                let mut remove = None;
                for (i, p) in state.batch_queue.iter().enumerate() {
                    ui.horizontal(|ui| {
                        ui.label(egui::RichText::new(
                            p.file_name().unwrap_or_default().to_string_lossy().as_ref()
                        ).small());
                        if ui.add(egui::Button::new(
                            egui::RichText::new("✕").color(egui::Color32::from_rgb(210, 60, 60))
                        ).small()).clicked() {
                            remove = Some(i);
                        }
                    });
                }
                if let Some(i) = remove {
                    state.batch_queue.remove(i);
                }
            });

        ui.horizontal(|ui| {
            let can_run = state.worker.is_none() && !state.batch_queue.is_empty();
            if ui.add_enabled(can_run, egui::Button::new(
                format!("▶ Run Batch ({} files)", state.batch_queue.len())
            )).clicked() {
                start_batch(state);
            }
            if ui.add_enabled(
                !state.batch_queue.is_empty(), egui::Button::new("Clear Queue")
            ).clicked() {
                state.batch_queue.clear();
            }
            if state.batch_running {
                if let Some(start) = state.worker_start {
                    ui.label(format!("{:.0}s", start.elapsed().as_secs_f64()));
                }
                ui.add(egui::ProgressBar::new(state.progress)
                    .desired_width(140.0)
                    .animate(true));
            }
        });
    }

    ui.add_space(16.0);

    // ── Navigation ────────────────────────────────────────────────────────────
    ui.horizontal(|ui| {
        if ui.button("← Back").clicked() { state.active_panel = ActivePanel::Import; }
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            if ui.button("Continue to ROI Editor →").clicked() {
                state.active_panel = ActivePanel::RoiEditor;
            }
        });
    });
}

// ─────────────────────────────────────────────────────────────────────────────
// Helpers
// ─────────────────────────────────────────────────────────────────────────────

fn help_tip(ui: &mut egui::Ui, text: &str) {
    ui.add(
        egui::Label::new(
            egui::RichText::new("❓").size(11.0).color(egui::Color32::from_rgb(130, 170, 230))
        )
        .sense(egui::Sense::hover()),
    )
    .on_hover_text(egui::RichText::new(text).size(12.0));
}

// ─────────────────────────────────────────────────────────────────────────────
// Single-file worker
// ─────────────────────────────────────────────────────────────────────────────

fn spawn_motion_correction(state: &mut AppState) {
    let path = match &state.source_path {
        Some(p) => p.clone(),
        None => { state.log("No file loaded."); return; }
    };
    let params            = state.params.motion.clone();
    let apply_sf          = state.params.apply_spatial_filter;
    let sf_params         = state.params.spatial_filter.clone();
    let mode_label        = format!("{:?}", params.mode);
    let (progress_tx, progress_rx) = std::sync::mpsc::channel::<f32>();
    let (done_tx, done_rx)         = std::sync::mpsc::channel();

    std::thread::spawn(move || {
        let result: Result<WorkerOutput, fiarfly_core::FiarflyError> = (|| {
            let _ = progress_tx.send(0.0);
            let reader = fiarfly_core::io::TiffReader::open(&path)?;
            let stack  = reader.load_all()?;

            // Optional 1P spatial high-pass filter before motion correction.
            let stack = if apply_sf {
                fiarfly_core::motion::spatial_highpass(&stack, &sf_params)?
            } else {
                stack
            };

            let cr = match params.mode {
                MotionMode::Rigid    =>
                    fiarfly_core::motion::correct_rigid(&stack, &params, Some(&progress_tx))?,
                MotionMode::NonRigid =>
                    fiarfly_core::motion::correct_nonrigid(&stack, &params, Some(&progress_tx))?,
            };
            Ok(WorkerOutput::MotionCorrected(cr))
        })();
        let _ = done_tx.send(result);
    });

    state.worker       = Some(WorkerHandle { progress_rx, done_rx });
    state.worker_start = Some(std::time::Instant::now());
    state.progress     = 0.0;
    state.log(format!("Motion correction ({mode_label}) started…"));
}

// ─────────────────────────────────────────────────────────────────────────────
// Batch worker helpers (also called from app.rs when chaining queue items)
// ─────────────────────────────────────────────────────────────────────────────

fn start_batch(state: &mut AppState) {
    if state.batch_queue.is_empty() { return; }
    let first = state.batch_queue.remove(0);
    let out_dir = state.batch_out_dir.clone();
    let params  = state.params.motion.clone();
    state.batch_running = true;
    spawn_batch_item(state, first, out_dir, params);
}

pub fn spawn_batch_item(
    state: &mut AppState,
    input: PathBuf,
    out_dir: Option<PathBuf>,
    params: MotionCorrectionParams,
) {
    let (progress_tx, progress_rx) = std::sync::mpsc::channel::<f32>();
    let (done_tx, done_rx)         = std::sync::mpsc::channel();

    let input_for_log = input.clone();
    std::thread::spawn(move || {
        let t0 = std::time::Instant::now();
        let result: Result<WorkerOutput, fiarfly_core::FiarflyError> = (|| {
            let _ = progress_tx.send(0.0);
            let reader = fiarfly_core::io::TiffReader::open(&input)?;
            let stack  = reader.load_all()?;
            let cr = match params.mode {
                MotionMode::Rigid    =>
                    fiarfly_core::motion::correct_rigid(&stack, &params, Some(&progress_tx))?,
                MotionMode::NonRigid =>
                    fiarfly_core::motion::correct_nonrigid(&stack, &params, Some(&progress_tx))?,
            };
            let n_frames = cr.corrected.dim().0;

            // Build output path.
            let stem = input.file_stem()
                .map(|s| format!("{}_corrected", s.to_string_lossy()))
                .unwrap_or_else(|| "corrected".into());
            let out_path = if let Some(dir) = out_dir {
                dir.join(format!("{stem}.tiff"))
            } else {
                input.with_file_name(format!("{stem}.tiff"))
            };
            fiarfly_core::io::save_stack(&cr.corrected, &out_path)?;
            let elapsed_secs = t0.elapsed().as_secs_f64();
            Ok(WorkerOutput::BatchItemDone { input, n_frames, elapsed_secs })
        })();
        let _ = done_tx.send(result);
    });

    let fname = input_for_log.file_name()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_else(|| input_for_log.to_string_lossy().to_string());
    state.worker       = Some(WorkerHandle { progress_rx, done_rx });
    state.worker_start = Some(std::time::Instant::now());
    state.progress     = 0.0;
    state.log(format!("Batch MC: processing {fname}…"));
}
