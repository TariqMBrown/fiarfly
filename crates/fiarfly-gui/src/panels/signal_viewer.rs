//! Signal viewer panel — trace extraction, deconvolution, and event detection.

use eframe::egui;
use egui_plot::{Line, Plot, PlotPoints};
use crate::state::{ActivePanel, AppState, TraceMode, WorkerHandle, WorkerOutput};

pub fn show(ui: &mut egui::Ui, state: &mut AppState) {
    egui::ScrollArea::vertical().show(ui, |ui| { show_inner(ui, state); });
}

fn show_inner(ui: &mut egui::Ui, state: &mut AppState) {
    ui.heading("Signal Viewer");
    ui.add_space(8.0);

    // ── Extract button ──────────────────────────────────────────────────────
    ui.horizontal(|ui| {
        let has_stack = state.best_stack().is_some();
        let has_rois  = state.roi_set.as_ref().map(|r| !r.rois.is_empty()).unwrap_or(false);
        let can_run   = has_stack && has_rois && state.worker.is_none();

        let label = if state.raw_f.is_some() { "Re-extract Traces" } else { "Extract Traces" };
        if ui.add_enabled(can_run, egui::Button::new(label)).clicked() {
            spawn_extraction(state);
        }

        if state.worker.is_some() {
            ui.add(egui::ProgressBar::new(state.progress)
                .desired_width(200.0)
                .animate(true));
        }

        if let Some(raw) = &state.raw_f {
            let n_rois: usize   = raw.nrows();
            let n_frames: usize = raw.ncols();
            ui.label(format!("✓  {n_rois} ROIs × {n_frames} frames"));
        } else if !has_stack {
            ui.label(egui::RichText::new("⚠ Load a file first.").weak());
        } else if !has_rois {
            ui.label(egui::RichText::new("⚠ Draw ROIs first.").weak());
        }
    });

    ui.add_space(8.0);

    // ── Display mode toggle ─────────────────────────────────────────────────
    ui.horizontal(|ui| {
        ui.label("Show:");
        let was = state.trace_mode.clone();
        ui.radio_value(&mut state.trace_mode, TraceMode::Raw,    "Raw F");
        ui.radio_value(&mut state.trace_mode, TraceMode::DeltaF, "ΔF/F");
        if state.trace_mode != was
            && state.trace_mode == TraceMode::DeltaF && state.dff_use_fixed_baseline {
                recompute_fixed_dff(state);
            }
    });

    // ── ΔF/F baseline options ───────────────────────────────────────────────
    if state.trace_mode == TraceMode::DeltaF {
        ui.horizontal(|ui| {
            ui.checkbox(&mut state.dff_use_fixed_baseline, "Fixed baseline window");
            help_tip(ui, "When checked, ΔF/F is computed using F0 = mean fluorescence \
                over the specified frame range (e.g. your baseline epoch).\n\
                When unchecked, the rolling-percentile baseline from the \
                Signal Parameters section is used instead.");
        });

        if state.dff_use_fixed_baseline {
            let n      = state.n_frames().saturating_sub(1);
            let fr     = state.params.frame_rate;
            let old_s  = state.dff_baseline_start;
            let old_e  = state.dff_baseline_end;

            ui.horizontal(|ui| {
                ui.label("Baseline:");
                ui.add(egui::DragValue::new(&mut state.dff_baseline_start)
                    .range(0..=state.dff_baseline_end).prefix("frame "));
                ui.label("–");
                ui.add(egui::DragValue::new(&mut state.dff_baseline_end)
                    .range(state.dff_baseline_start..=n).prefix("frame "));
                if let Some(f) = fr {
                    let t0 = state.dff_baseline_start as f32 / f;
                    let t1 = state.dff_baseline_end   as f32 / f;
                    ui.label(egui::RichText::new(format!("({:.2}s – {:.2}s)", t0, t1)).small().weak());
                }
            });

            if state.dff_baseline_start != old_s || state.dff_baseline_end != old_e {
                recompute_fixed_dff(state);
            }
        }
    }

    ui.add_space(8.0);

    // ── Signal parameters ───────────────────────────────────────────────────
    egui::CollapsingHeader::new("Signal Parameters (rolling ΔF/F)").show(ui, |ui| {
        egui::Grid::new("sig_params").num_columns(3).spacing([8.0, 4.0]).show(ui, |ui| {
            ui.label("Baseline percentile:");
            ui.add(egui::DragValue::new(&mut state.params.delta_f.baseline_percentile)
                .range(1.0..=50.0).speed(0.1));
            help_tip(ui, "The Nth percentile of fluorescence within the rolling window \
                 used as F0. Lower values (e.g. 8) pick near-minimum baseline \
                 and are robust to transient activity. Higher values bias toward \
                 average fluorescence. Typical range: 5–15.");
            ui.end_row();

            ui.label("Baseline window (frames):");
            ui.add(egui::DragValue::new(&mut state.params.delta_f.window_frames)
                .range(10..=2000));
            help_tip(ui, "Half-width of the sliding window (in frames) used to estimate \
                 the local baseline F0. Longer windows (e.g. 300–600 frames) \
                 track slow drift; shorter windows follow faster baseline changes \
                 but may suppress slow signals.");
            ui.end_row();

            ui.label("Neuropil r:");
            ui.add(egui::DragValue::new(&mut state.params.neuropil_r)
                .range(0.0..=1.0).speed(0.01));
            help_tip(ui, "Neuropil contamination coefficient. The corrected signal is \
                 F_soma − r × F_neuropil. Typical: 0.7 for 2-photon in vivo. \
                 Set to 0 to skip neuropil subtraction.");
            ui.end_row();
        });
    });

    ui.add_space(8.0);

    // ── Trace plot ──────────────────────────────────────────────────────────
    let display_data: Option<&ndarray::Array2<f32>> = match state.trace_mode {
        TraceMode::Raw    => state.raw_f.as_ref(),
        TraceMode::DeltaF => {
            if state.dff_use_fixed_baseline {
                state.delta_f_fixed.as_ref().or(state.delta_f.as_ref())
            } else {
                state.delta_f.as_ref().or(state.raw_f.as_ref())
            }
        }
    };

    let y_label = match state.trace_mode {
        TraceMode::Raw    => "F (raw)",
        TraceMode::DeltaF => "ΔF/F",
    };
    let plot_id = match state.trace_mode {
        TraceMode::Raw    => "trace_plot_raw",
        TraceMode::DeltaF => "trace_plot_dff",
    };

    Plot::new(plot_id)
        .height(220.0)
        .legend(egui_plot::Legend::default())
        .x_axis_label(if state.params.frame_rate.is_some() { "Time (s)" } else { "Frame" })
        .y_axis_label(y_label)
        .show(ui, |plot_ui| {
            if let (Some(df), Some(roi_set)) = (display_data, &state.roi_set) {
                let fr = state.params.frame_rate;
                for (i, roi) in roi_set.rois.iter().enumerate() {
                    if i >= df.nrows() { break; }
                    let row = df.row(i);
                    let pts: PlotPoints = row.iter().enumerate()
                        .map(|(t, &v)| {
                            let x = fr.map(|f| t as f64 / f as f64).unwrap_or(t as f64);
                            [x, v as f64]
                        })
                        .collect();
                    let color = egui::Color32::from_rgb(roi.color[0], roi.color[1], roi.color[2]);
                    plot_ui.line(Line::new(pts).name(&roi.label).color(color));
                }
            } else {
                let pts: PlotPoints = (0..200)
                    .map(|i| { let x = i as f64 * 0.1; [x, (x * 0.5).sin() * 0.3] })
                    .collect();
                plot_ui.line(Line::new(pts).name("(no data)").color(egui::Color32::GRAY));
            }
        });

    ui.add_space(8.0);

    // ── OASIS Spike Deconvolution ───────────────────────────────────────────
    egui::CollapsingHeader::new("Spike Deconvolution (OASIS AR1)").show(ui, |ui| {
        ui.label(egui::RichText::new(
            "Infers spike trains from ΔF/F using the OASIS AR(1) algorithm \
             (Friedrich et al. 2017). Outputs a denoised calcium trace and \
             a non-negative spike probability trace."
        ).small().weak());
        ui.add_space(4.0);

        egui::Grid::new("oasis_params").num_columns(3).spacing([8.0, 4.0]).show(ui, |ui| {
            ui.label("Decay constant g:");
            let mut use_auto_g = state.params.oasis.g.is_none();
            if ui.checkbox(&mut use_auto_g, "Auto").changed() {
                state.params.oasis.g = if use_auto_g { None } else { Some(0.95) };
            }
            if !use_auto_g {
                if let Some(g) = &mut state.params.oasis.g {
                    ui.add(egui::DragValue::new(g).range(0.05..=0.999).speed(0.001));
                } else {
                    ui.label("—");
                }
            } else {
                ui.label(egui::RichText::new("(estimated per ROI)").weak().small());
            }
            help_tip(ui, "Calcium decay constant g ∈ (0, 1). Related to indicator time \
                constant τ by g = exp(−dt/τ). Auto-estimation uses the lag-1 \
                autocorrelation of each trace.");
            ui.end_row();

            ui.label("Sparsity λ:");
            ui.add(egui::DragValue::new(&mut state.params.oasis.lambda)
                .range(0.0..=10.0).speed(0.01));
            help_tip(ui, "L1 sparsity penalty. 0 = non-negative deconvolution (NND, \
                recommended). Increase to enforce sparser spike trains.");
            ui.end_row();
        });

        ui.add_space(4.0);

        ui.horizontal(|ui| {
            let has_dff   = state.delta_f.is_some();
            let can_run   = has_dff && state.worker.is_none();
            let label = if state.oasis_result.is_some() {
                "Re-run OASIS"
            } else {
                "Run OASIS Deconvolution"
            };
            if ui.add_enabled(can_run, egui::Button::new(label)).clicked() {
                spawn_oasis(state);
            }
            if !has_dff {
                ui.label(egui::RichText::new("⚠ Extract traces first.").weak());
            }
            if let Some(oasis) = &state.oasis_result {
                let n = oasis.spikes.nrows();
                ui.label(egui::RichText::new(format!("✓ {n} ROIs deconvolved")).weak().small());
            }
        });

        // Show inferred spike trains if available.
        if state.oasis_result.is_some() {
            if let (Some(oasis), Some(roi_set)) = (&state.oasis_result, &state.roi_set) {
                Plot::new("spike_plot")
                    .height(100.0)
                    .legend(egui_plot::Legend::default())
                    .x_axis_label(if state.params.frame_rate.is_some() { "Time (s)" } else { "Frame" })
                    .y_axis_label("Spikes")
                    .show(ui, |plot_ui| {
                        let fr = state.params.frame_rate;
                        for (i, roi) in roi_set.rois.iter().enumerate() {
                            if i >= oasis.spikes.nrows() { break; }
                            let row = oasis.spikes.row(i);
                            // Offset each ROI by i for visibility.
                            let pts: PlotPoints = row.iter().enumerate()
                                .map(|(t, &v)| {
                                    let x = fr.map(|f| t as f64 / f as f64).unwrap_or(t as f64);
                                    [x, v as f64 + i as f64 * 0.1]
                                })
                                .collect();
                            let color = egui::Color32::from_rgb(
                                roi.color[0], roi.color[1], roi.color[2]);
                            plot_ui.line(Line::new(pts).name(&roi.label).color(color));
                        }
                    });
            }
        }
    });

    ui.add_space(8.0);

    // ── Event Detection ─────────────────────────────────────────────────────
    egui::CollapsingHeader::new("Event Detection & Quality Metrics").show(ui, |ui| {
        ui.label(egui::RichText::new(
            "Detects calcium transients and computes per-ROI quality scores \
             (SNR, skewness, active fraction)."
        ).small().weak());
        ui.add_space(4.0);

        egui::Grid::new("event_params").num_columns(3).spacing([8.0, 4.0]).show(ui, |ui| {
            ui.label("Threshold (× noise std):");
            ui.add(egui::DragValue::new(&mut state.params.event_detection.threshold_std)
                .range(0.5..=10.0).speed(0.1));
            help_tip(ui, "Detection threshold expressed as a multiple of per-ROI baseline \
                noise std. Events must exceed this threshold to be reported. \
                Typical: 2.5–3.5.");
            ui.end_row();

            ui.label("Min duration (frames):");
            ui.add(egui::DragValue::new(&mut state.params.event_detection.min_duration_frames)
                .range(1..=50));
            help_tip(ui, "Minimum number of consecutive above-threshold frames for a \
                valid event. Prevents single-frame noise spikes.");
            ui.end_row();

            ui.label("Min event interval (frames):");
            ui.add(egui::DragValue::new(&mut state.params.event_detection.min_interval_frames)
                .range(1..=200));
            help_tip(ui, "Events with peaks closer than this are merged into the \
                higher-amplitude one.");
            ui.end_row();
        });

        ui.add_space(4.0);

        ui.horizontal(|ui| {
            let has_dff = state.delta_f.is_some();
            let can_run = has_dff && state.worker.is_none();
            let label = if state.event_result.is_some() {
                "Re-run Analysis"
            } else {
                "Run Event Detection + Quality"
            };
            if ui.add_enabled(can_run, egui::Button::new(label)).clicked() {
                spawn_analysis(state);
            }
            if !has_dff {
                ui.label(egui::RichText::new("⚠ Extract traces first.").weak());
            }
        });

        // Event summary table.
        if let (Some(events), Some(quality), Some(roi_set)) = (
            &state.event_result,
            &state.roi_quality,
            &state.roi_set,
        ) {
            ui.add_space(6.0);
            ui.label(egui::RichText::new("Per-ROI Results").strong());

            let fr = state.params.frame_rate;
            let n_frames = state.raw_f.as_ref().map(|r| r.ncols()).unwrap_or(1);

            egui::ScrollArea::vertical()
                .id_salt("quality_table")
                .max_height(180.0)
                .show(ui, |ui| {
                    egui::Grid::new("quality_grid")
                        .num_columns(7)
                        .spacing([8.0, 3.0])
                        .striped(true)
                        .show(ui, |ui| {
                            // Header
                            ui.label(egui::RichText::new("ROI").strong());
                            ui.label(egui::RichText::new("Events").strong());
                            ui.label(egui::RichText::new("Rate (Hz)").strong());
                            ui.label(egui::RichText::new("SNR").strong());
                            ui.label(egui::RichText::new("Skewness").strong());
                            ui.label(egui::RichText::new("Active%").strong());
                            ui.label(egui::RichText::new("Pass").strong());
                            ui.end_row();

                            for (i, roi) in roi_set.rois.iter().enumerate() {
                                let n_events = events.events.get(i)
                                    .map(|e| e.len()).unwrap_or(0);
                                let rate_hz = fr.map(|f| {
                                    events.event_rate(i, n_frames) * f
                                });
                                let q = quality.get(i);

                                let color = egui::Color32::from_rgb(
                                    roi.color[0], roi.color[1], roi.color[2]);
                                ui.label(egui::RichText::new(&roi.label).color(color));
                                ui.label(format!("{n_events}"));
                                ui.label(match rate_hz {
                                    Some(r) => format!("{r:.3}"),
                                    None    => "—".into(),
                                });
                                ui.label(q.map(|q| format!("{:.2}", q.snr))
                                    .unwrap_or_else(|| "—".into()));
                                ui.label(q.map(|q| format!("{:.2}", q.skewness))
                                    .unwrap_or_else(|| "—".into()));
                                ui.label(q.map(|q| format!("{:.1}%", q.active_fraction * 100.0))
                                    .unwrap_or_else(|| "—".into()));

                                let pass_text = match q {
                                    Some(q) if q.passes => {
                                        egui::RichText::new("✓").color(egui::Color32::GREEN)
                                    }
                                    Some(_) => {
                                        egui::RichText::new("✗").color(egui::Color32::RED)
                                    }
                                    None => egui::RichText::new("—"),
                                };
                                ui.label(pass_text);
                                ui.end_row();
                            }
                        });
                });

            let total_events: usize = events.total_events();
            let n_pass = quality.iter().filter(|q| q.passes).count();
            ui.label(egui::RichText::new(format!(
                "Total: {total_events} events · {n_pass}/{} ROIs pass quality (SNR > 3, skewness > 0)",
                quality.len()
            )).small().weak());
        }
    });

    ui.add_space(8.0);

    // ── ROI color editor ────────────────────────────────────────────────────
    if let Some(roi_set) = &mut state.roi_set {
        if !roi_set.rois.is_empty() {
            egui::CollapsingHeader::new("ROI Colors").show(ui, |ui| {
                ui.label(egui::RichText::new(
                    "Changes sync to the ROI editor canvas automatically."
                ).small().weak());
                ui.add_space(4.0);
                let mut changed = false;
                for roi in roi_set.rois.iter_mut() {
                    ui.horizontal(|ui| {
                        let mut rgba = egui::Color32::from_rgb(
                            roi.color[0], roi.color[1], roi.color[2],
                        );
                        if ui.color_edit_button_srgba(&mut rgba).changed() {
                            roi.color = [rgba.r(), rgba.g(), rgba.b()];
                            changed = true;
                        }
                        ui.label(&roi.label);
                    });
                }
                if changed {
                    state.roi_texture_key = String::new();
                }
            });
        }
    }

    ui.add_space(8.0);

    // ── Playback controls ───────────────────────────────────────────────────
    ui.horizontal(|ui| {
        let n = state.n_frames().saturating_sub(1);
        if ui.button("⏮").clicked() { state.current_frame = 0; }
        if ui.button("⏭").clicked() { state.current_frame = n; }
        ui.add(egui::Slider::new(&mut state.current_frame, 0..=n).text("Frame"));
        if let Some(fr) = state.params.frame_rate {
            ui.label(format!("{:.2} s", state.current_frame as f32 / fr));
        }
    });

    ui.add_space(16.0);

    ui.horizontal(|ui| {
        if ui.button("← Back").clicked() { state.active_panel = ActivePanel::RoiEditor; }
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            let can_export = state.delta_f.is_some() || state.raw_f.is_some();
            if ui.add_enabled(can_export, egui::Button::new("Continue to Export →")).clicked() {
                state.active_panel = ActivePanel::Export;
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

fn recompute_fixed_dff(state: &mut AppState) {
    let raw = match &state.raw_f { Some(r) => r, None => return };
    let start = state.dff_baseline_start.min(raw.ncols().saturating_sub(1));
    let end   = state.dff_baseline_end.min(raw.ncols().saturating_sub(1)).max(start);
    let n_rois   = raw.nrows();
    let n_frames = raw.ncols();
    let mut out = ndarray::Array2::<f32>::zeros((n_rois, n_frames));
    for i in 0..n_rois {
        let row = raw.row(i);
        let window = &row.as_slice().unwrap()[start..=end];
        let f0: f32 = window.iter().sum::<f32>() / window.len() as f32;
        let f0 = f0.max(1e-6);
        for t in 0..n_frames { out[[i, t]] = (row[t] - f0) / f0; }
    }
    state.delta_f_fixed = Some(out);
}

// ─────────────────────────────────────────────────────────────────────────────
// Background workers
// ─────────────────────────────────────────────────────────────────────────────

fn spawn_extraction(state: &mut AppState) {
    let stack = match state.best_stack() {
        Some(s) => s,
        None => { state.log("No image stack available."); return; }
    };
    let roi_set = match &state.roi_set {
        Some(r) => r.clone(),
        None => { state.log("No ROIs defined."); return; }
    };
    let n_rois_label = roi_set.rois.len();
    let (height, width) = match state.image_shape() {
        Some(hw) => hw,
        None => { state.log("Image dimensions unknown."); return; }
    };
    let delta_f_params = state.params.delta_f.clone();
    let (progress_tx, progress_rx) = std::sync::mpsc::channel::<f32>();
    let (done_tx, done_rx)         = std::sync::mpsc::channel();

    std::thread::spawn(move || {
        let result: Result<WorkerOutput, fiarfly_core::FiarflyError> = (|| {
            let _ = progress_tx.send(0.1);
            let masks: Vec<fiarfly_core::roi::RoiMask> = roi_set.rois.iter()
                .map(|roi| roi.to_mask(height, width))
                .collect();
            let _ = progress_tx.send(0.2);
            let raw     = fiarfly_core::signal::extract_raw_fluorescence(&stack, &masks)?;
            let _ = progress_tx.send(0.7);
            let delta_f = fiarfly_core::signal::compute_delta_f(&raw, &delta_f_params)?;
            let _ = progress_tx.send(1.0);
            Ok(WorkerOutput::TracesExtracted { raw, delta_f, neuropil_f: None })
        })();
        let _ = done_tx.send(result);
    });

    state.worker   = Some(WorkerHandle { progress_rx, done_rx });
    state.progress = 0.0;
    state.delta_f_fixed = None;
    state.log(format!("Extracting traces for {n_rois_label} ROIs…"));
}

fn spawn_oasis(state: &mut AppState) {
    let delta_f = match &state.delta_f {
        Some(d) => d.clone(),
        None => { state.log("No ΔF/F traces available."); return; }
    };
    let params     = state.params.oasis.clone();
    let frame_rate = state.params.frame_rate.unwrap_or(30.0);
    let n_rois     = delta_f.nrows();

    let (progress_tx, progress_rx) = std::sync::mpsc::channel::<f32>();
    let (done_tx, done_rx)         = std::sync::mpsc::channel();

    std::thread::spawn(move || {
        let _ = progress_tx.send(0.1);
        let result: Result<WorkerOutput, fiarfly_core::FiarflyError> = (|| {
            let oasis = fiarfly_core::signal::deconvolve_oasis(&delta_f, &params, frame_rate)?;
            let _ = progress_tx.send(1.0);
            Ok(WorkerOutput::OasisDone(oasis))
        })();
        let _ = done_tx.send(result);
    });

    state.worker   = Some(WorkerHandle { progress_rx, done_rx });
    state.progress = 0.0;
    state.log(format!("Running OASIS deconvolution for {n_rois} ROIs…"));
}

fn spawn_analysis(state: &mut AppState) {
    let delta_f = match &state.delta_f {
        Some(d) => d.clone(),
        None => { state.log("No ΔF/F traces available."); return; }
    };
    let event_params   = state.params.event_detection.clone();
    let quality_params = fiarfly_core::signal::QualityParams::default();
    let n_rois         = delta_f.nrows();

    let (progress_tx, progress_rx) = std::sync::mpsc::channel::<f32>();
    let (done_tx, done_rx)         = std::sync::mpsc::channel();

    std::thread::spawn(move || {
        let _ = progress_tx.send(0.2);
        let result: Result<WorkerOutput, fiarfly_core::FiarflyError> = (|| {
            let events  = fiarfly_core::signal::detect_events(&delta_f, &event_params)?;
            let _ = progress_tx.send(0.7);
            let quality = fiarfly_core::signal::compute_quality(&delta_f, &quality_params)?;
            let _ = progress_tx.send(1.0);
            Ok(WorkerOutput::AnalysisDone { events, quality })
        })();
        let _ = done_tx.send(result);
    });

    state.worker   = Some(WorkerHandle { progress_rx, done_rx });
    state.progress = 0.0;
    state.log(format!("Running event detection + quality for {n_rois} ROIs…"));
}
