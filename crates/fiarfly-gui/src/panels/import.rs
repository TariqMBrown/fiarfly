//! Import panel — load TIFF files, set modality, define frame labels.

use eframe::egui;
use crate::state::{ActivePanel, AppState, DialogRequest, FrameLabel, Modality};

pub fn open_tiff_dialog(state: &mut AppState) {
    if let Some(path) = rfd::FileDialog::new()
        .add_filter("TIFF", &["tif", "tiff"])
        .pick_file()
    {
        match fiarfly_core::io::TiffReader::open(&path) {
            Ok(reader) => {
                state.log(format!(
                    "Loaded {}: {} frames × {}×{}",
                    path.file_name().unwrap_or_default().to_string_lossy(),
                    reader.num_frames, reader.height, reader.width
                ));
                state.source_path = Some(path);
                state.tiff_reader = Some(reader);
                state.current_frame = 0;
                // Invalidate any cached texture so the new file is shown.
                state.preview_texture = None;
                state.preview_frame_loaded = usize::MAX;
            }
            Err(e) => state.log(format!("Error loading TIFF: {e}")),
        }
    }
}

pub fn show(ui: &mut egui::Ui, state: &mut AppState) {
    // Wrap everything in a scroll area so nothing goes off-screen.
    egui::ScrollArea::vertical().show(ui, |ui| {
        ui.heading("Import");
        ui.add_space(8.0);

        // Source file row.
        ui.horizontal(|ui| {
            let path_str = state.source_path
                .as_deref()
                .and_then(|p| p.to_str())
                .unwrap_or("(no file selected)");
            ui.add(egui::TextEdit::singleline(&mut path_str.to_owned())
                .desired_width(360.0)
                .interactive(false));
            if ui.button("Open TIFF…").clicked() {
                // Deferred: see DialogRequest doc for why.
                state.pending_dialog = Some(DialogRequest::OpenTiff);
            }
        });

        // Stack metadata + resource estimates.
        if let Some(reader) = &state.tiff_reader {
            ui.label(format!(
                "Stack: {} frames  ×  {} px  ×  {} px",
                reader.num_frames, reader.height, reader.width
            ));

            // Memory & compute estimates.
            let n = reader.num_frames as f64;
            let h = reader.height as f64;
            let w = reader.width as f64;
            let pixels = n * h * w;
            let stack_bytes = pixels * 4.0; // f32
            let mc_peak_bytes = stack_bytes * 2.5; // raw + corrected + FFT workspace
            let total_peak_bytes = stack_bytes + mc_peak_bytes;

            ui.add_space(4.0);
            egui::CollapsingHeader::new("Resource Estimates").show(ui, |ui| {
                egui::Grid::new("resource_estimates").num_columns(2).spacing([12.0, 2.0]).show(ui, |ui| {
                    ui.label("Stack in memory:");
                    ui.label(format_bytes(stack_bytes));
                    ui.end_row();

                    ui.label("Motion correction peak:");
                    ui.label(format!("~{}", format_bytes(mc_peak_bytes)));
                    ui.end_row();

                    ui.label("Total peak (estimated):");
                    let peak_label = format!("~{}", format_bytes(total_peak_bytes));
                    let sys_mem = sys_memory_bytes();
                    if sys_mem > 0 && total_peak_bytes > sys_mem as f64 * 0.8 {
                        ui.label(egui::RichText::new(peak_label)
                            .color(egui::Color32::from_rgb(255, 100, 100)));
                        ui.end_row();
                        ui.label("");
                        ui.label(egui::RichText::new(
                            "Warning: may exceed available RAM"
                        ).small().color(egui::Color32::from_rgb(255, 100, 100)));
                    } else {
                        ui.label(peak_label);
                    }
                    ui.end_row();

                    if sys_mem > 0 {
                        ui.label("System RAM:");
                        ui.label(format_bytes(sys_mem as f64));
                        ui.end_row();
                    }

                    if let Some(fr) = state.params.frame_rate {
                        let duration = n / fr as f64;
                        ui.label("Recording duration:");
                        ui.label(format!("{duration:.1} s"));
                        ui.end_row();
                    }
                });
            });
        }

        ui.add_space(8.0);

        // Modality selector.
        ui.horizontal(|ui| {
            ui.label("Modality:");
            egui::ComboBox::from_id_salt("modality")
                .selected_text(state.params.modality.to_string())
                .show_ui(ui, |ui| {
                    ui.selectable_value(&mut state.params.modality, Modality::TwoPhotonInVivo,    "Two-Photon In Vivo");
                    ui.selectable_value(&mut state.params.modality, Modality::TwoPhotonInVitro,   "Two-Photon In Vitro");
                    ui.selectable_value(&mut state.params.modality, Modality::OnePhotonInVivo,    "One-Photon In Vivo");
                    ui.selectable_value(&mut state.params.modality, Modality::OnePhotonWideField, "One-Photon Wide-field");
                });
        });

        // Frame rate.
        ui.horizontal(|ui| {
            ui.label("Frame rate (Hz):");
            let mut fr_str = state.params.frame_rate
                .map(|f| f.to_string())
                .unwrap_or_default();
            if ui.add(egui::TextEdit::singleline(&mut fr_str).desired_width(80.0)).changed() {
                state.params.frame_rate = fr_str.parse::<f32>().ok();
            }
            ui.label("(optional)");
        });

        ui.add_space(8.0);

        // Frame slider — placed BEFORE the image so it's always visible.
        let has_reader = state.tiff_reader.is_some();
        if has_reader {
            let n = state.n_frames().saturating_sub(1);
            let old_frame = state.current_frame;
            ui.add(egui::Slider::new(&mut state.current_frame, 0..=n)
                .text("Frame")
                .clamping(egui::SliderClamping::Always));
            if state.current_frame != old_frame {
                // Invalidate texture so it reloads.
                state.preview_frame_loaded = usize::MAX;
                // Stop playback if user manually scrubs.
                if state.playing {
                    state.playing = false;
                    state.last_frame_time = None;
                }
            }

            // Playback controls.
            ui.horizontal(|ui| {
                // Play / Pause button.
                let icon = if state.playing { "⏸" } else { "▶" };
                if ui.button(icon).on_hover_text(if state.playing { "Pause" } else { "Play" }).clicked() {
                    state.playing = !state.playing;
                    if !state.playing {
                        state.last_frame_time = None;
                    }
                }

                // Speed selector.
                ui.label("Speed:");
                egui::ComboBox::from_id_salt("playback_speed")
                    .width(55.0)
                    .selected_text(format!("{}×", state.playback_speed))
                    .show_ui(ui, |ui| {
                        for &s in &[0.25_f32, 0.5, 1.0, 2.0, 4.0] {
                            ui.selectable_value(&mut state.playback_speed, s, format!("{s}×"));
                        }
                    });

                // Frame counter.
                ui.label(format!("{} / {}", state.current_frame, n));
                if let Some(fr) = state.params.frame_rate {
                    let time = state.current_frame as f32 / fr;
                    ui.label(format!("{time:.2} s"));
                }
            });
        }

        ui.add_space(4.0);

        // Frame preview — load texture on demand, cache by frame index.
        let available_w = ui.available_width().min(700.0);
        let preview_h   = (available_w * 0.6).min(380.0);

        if has_reader {
            let frame_idx = state.current_frame;
            let need_load = state.preview_frame_loaded != frame_idx
                || state.preview_texture.is_none();

            if need_load {
                if let Some(reader) = &state.tiff_reader {
                    match reader.get_frame(frame_idx) {
                        Ok(frame) => {
                            let (h, w) = frame.dim();
                            // Normalise to 0-255 with per-frame min/max contrast.
                            let min = frame.iter().cloned().fold(f32::INFINITY, f32::min);
                            let max = frame.iter().cloned().fold(f32::NEG_INFINITY, f32::max);
                            let range = (max - min).max(1e-6);
                            let pixels: Vec<u8> = frame.iter()
                                .map(|&v| ((v - min) / range * 255.0) as u8)
                                .collect();
                            let color_img = egui::ColorImage::from_gray([w, h], &pixels);
                            let tex = ui.ctx().load_texture(
                                "frame_preview",
                                color_img,
                                egui::TextureOptions::LINEAR,
                            );
                            state.preview_texture = Some(tex);
                            state.preview_frame_loaded = frame_idx;
                        }
                        Err(e) => {
                            state.log(format!("Frame load error: {e}"));
                        }
                    }
                }
            }

            if let Some(tex) = &state.preview_texture {
                let size = egui::vec2(available_w, preview_h);
                ui.add(egui::Image::new(tex).fit_to_exact_size(size));
            }
        } else {
            ui.add_sized(
                egui::vec2(available_w, preview_h),
                egui::Label::new(
                    egui::RichText::new("Open a TIFF file to see a preview.")
                        .weak()
                ),
            );
        }

        ui.add_space(12.0);

        // ── Frame label editor ────────────────────────────────────────────────
        egui::CollapsingHeader::new("Frame Labels  (shown in overlay)").show(ui, |ui| {
            ui.label(egui::RichText::new(
                "Define named ranges (e.g. Baseline 0–124, Stim 1 125–744). \
                 The draggable overlay shows the label for the current frame."
            ).small().weak());
            ui.add_space(4.0);

            let n_frames = state.n_frames();
            let mut to_remove: Option<usize> = None;

            for (i, label) in state.frame_labels.iter_mut().enumerate() {
                ui.horizontal(|ui| {
                    ui.add(egui::TextEdit::singleline(&mut label.name).desired_width(110.0));
                    ui.label("frames");
                    ui.add(egui::DragValue::new(&mut label.start)
                        .range(0..=n_frames.saturating_sub(1)));
                    ui.label("–");
                    ui.add(egui::DragValue::new(&mut label.end)
                        .range(label.start..=n_frames.saturating_sub(1)));
                    if ui.add(egui::Button::new(
                        egui::RichText::new("✕").color(egui::Color32::from_rgb(210, 60, 60))
                    ).small()).clicked() {
                        to_remove = Some(i);
                    }
                });
            }
            if let Some(i) = to_remove {
                state.frame_labels.remove(i);
            }

            if ui.button("+ Add label").clicked() {
                let start = state.frame_labels.last()
                    .map(|l| l.end + 1).unwrap_or(0);
                state.frame_labels.push(FrameLabel {
                    name: format!("Epoch {}", state.frame_labels.len() + 1),
                    start,
                    end: n_frames.saturating_sub(1),
                });
            }

            ui.horizontal(|ui| {
                ui.checkbox(&mut state.show_label_overlay, "Show overlay");
                if ui.small_button("Clear all").clicked() {
                    state.frame_labels.clear();
                }
            });
        });

        ui.add_space(8.0);

        // Navigation.
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            if ui.add_enabled(has_reader, egui::Button::new("Continue to Motion →")).clicked() {
                state.active_panel = ActivePanel::MotionCorrection;
            }
        });
    });
}

fn format_bytes(bytes: f64) -> String {
    if bytes >= 1024.0 * 1024.0 * 1024.0 {
        format!("{:.1} GB", bytes / (1024.0 * 1024.0 * 1024.0))
    } else if bytes >= 1024.0 * 1024.0 {
        format!("{:.0} MB", bytes / (1024.0 * 1024.0))
    } else {
        format!("{:.0} KB", bytes / 1024.0)
    }
}

/// Return total physical memory in bytes, or 0 if unavailable.
fn sys_memory_bytes() -> u64 {
    #[cfg(target_os = "macos")]
    {
        use std::mem::MaybeUninit;
        let mut size = MaybeUninit::<u64>::uninit();
        let mut len = std::mem::size_of::<u64>();
        let name = c"hw.memsize";
        let ret = unsafe {
            libc::sysctlbyname(
                name.as_ptr(),
                size.as_mut_ptr().cast(),
                &mut len,
                std::ptr::null_mut(),
                0,
            )
        };
        if ret == 0 { unsafe { size.assume_init() } } else { 0 }
    }
    #[cfg(target_os = "linux")]
    {
        unsafe {
            let info = MaybeUninit::<libc::sysinfo>::uninit();
            let mut info = info.assume_init();
            if libc::sysinfo(&mut info) == 0 {
                info.totalram * info.mem_unit as u64
            } else {
                0
            }
        }
    }
    #[cfg(not(any(target_os = "macos", target_os = "linux")))]
    { 0 }
}
