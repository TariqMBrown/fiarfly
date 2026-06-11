//! Top-level eframe::App implementation.

use eframe::egui;
use std::sync::Arc;
use crate::state::{ActivePanel, AppState, DialogRequest, WorkerOutput};
use crate::panels;

pub struct FiarflyApp {
    pub state: AppState,
}

impl FiarflyApp {
    pub fn new(_cc: &eframe::CreationContext<'_>) -> Self {
        Self { state: AppState::default() }
    }

    fn step_label(panel: &ActivePanel) -> &'static str {
        match panel {
            ActivePanel::Import           => "1. Import",
            ActivePanel::MotionCorrection => "2. Motion Correction",
            ActivePanel::RoiEditor        => "3. ROI Editor",
            ActivePanel::SignalViewer     => "4. Signal Viewer",
            ActivePanel::Export           => "5. Export",
            ActivePanel::Project          => "Project",
            ActivePanel::Analysis         => "Analysis",
            ActivePanel::Statistics       => "Statistics",
        }
    }

    /// Check for background worker completion and apply results to state.
    fn poll_worker(&mut self, ctx: &egui::Context) {
        let done = if let Some(worker) = &self.state.worker {
            while let Ok(p) = worker.progress_rx.try_recv() {
                self.state.progress = p;
            }
            match worker.done_rx.try_recv() {
                Ok(result) => Some(result),
                Err(std::sync::mpsc::TryRecvError::Empty) => {
                    ctx.request_repaint_after(std::time::Duration::from_millis(50));
                    None
                }
                Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                    Some(Err(fiarfly_core::FiarflyError::Export("Worker disconnected".into())))
                }
            }
        } else {
            None
        };

        if let Some(result) = done {
            self.state.worker      = None;
            self.state.worker_start = None;
            self.state.progress    = 1.0;
            match result {
                Ok(WorkerOutput::MotionCorrected(cr)) => {
                    let n = cr.corrected.dim().0;
                    let elapsed = self.state.worker_start
                        .map(|t| t.elapsed().as_secs_f64())
                        .unwrap_or(0.0);
                    self.state.shift_scores = cr.correlation_scores;
                    self.state.corrected = Some(Arc::new(cr.corrected));
                    self.state.log(format!(
                        "Motion correction complete — {n} frames in {elapsed:.1}s."
                    ));
                }
                Ok(WorkerOutput::TracesExtracted { raw, delta_f, neuropil_f }) => {
                    let n_rois: usize = raw.nrows();
                    let n_frames: usize = raw.ncols();
                    self.state.log(format!(
                        "Trace extraction complete — {n_rois} ROIs × {n_frames} frames."
                    ));
                    self.state.raw_f = Some(raw);
                    self.state.delta_f = Some(delta_f);
                    self.state.neuropil_f = neuropil_f;
                }
                Ok(WorkerOutput::ProjectionComputed { mean, max }) => {
                    let (h, w) = mean.dim();
                    self.state.projection_mean = Some(mean);
                    self.state.projection_max = Some(max);
                    self.state.roi_texture_key = String::new();
                    self.state.log(format!("Projection computed ({h}×{w})."));
                }
                Ok(WorkerOutput::OasisDone(result)) => {
                    let n_rois = result.spikes.nrows();
                    let g_mean: f32 = result.g_estimated.iter().sum::<f32>()
                        / result.g_estimated.len().max(1) as f32;
                    self.state.oasis_result = Some(result);
                    self.state.log(format!(
                        "OASIS deconvolution done — {n_rois} ROIs, mean g={g_mean:.3}."
                    ));
                }
                Ok(WorkerOutput::AnalysisDone { events, quality }) => {
                    let total = events.total_events();
                    let n_pass = quality.iter().filter(|q| q.passes).count();
                    let n_rois = quality.len();
                    self.state.event_result = Some(events);
                    self.state.roi_quality  = Some(quality);
                    self.state.log(format!(
                        "Analysis done — {total} events detected, {n_pass}/{n_rois} ROIs pass quality."
                    ));
                }
                Ok(WorkerOutput::BatchItemDone { input, n_frames, elapsed_secs }) => {
                    let fname = input.file_name()
                        .map(|n| n.to_string_lossy().to_string())
                        .unwrap_or_else(|| input.display().to_string());
                    self.state.log(format!(
                        "Batch MC done: {fname} — {n_frames} frames in {elapsed_secs:.1}s."
                    ));
                    // Pop next item from queue and spawn a new worker.
                    if let Some(next_path) = self.state.batch_queue.first().cloned() {
                        self.state.batch_queue.remove(0);
                        let out_dir = self.state.batch_out_dir.clone();
                        let params  = self.state.params.motion.clone();
                        panels::motion_correction::spawn_batch_item(
                            &mut self.state, next_path, out_dir, params,
                        );
                    } else {
                        self.state.batch_running = false;
                        self.state.log("Batch motion correction complete.");
                    }
                }
                Err(e) => {
                    self.state.log(format!("Worker error: {e}"));
                    // Stop batch on error.
                    self.state.batch_running = false;
                    self.state.batch_queue.clear();
                }
            }
        }
    }

    /// Advance current_frame when playback is active.
    fn tick_playback(&mut self, ctx: &egui::Context) {
        if !self.state.playing {
            return;
        }
        let n = self.state.n_frames();
        if n == 0 {
            self.state.playing = false;
            return;
        }
        let fps = self.state.params.frame_rate.unwrap_or(30.0);
        let interval = std::time::Duration::from_secs_f64(
            1.0 / (fps as f64 * self.state.playback_speed as f64),
        );
        let now = std::time::Instant::now();
        let should_advance = match self.state.last_frame_time {
            Some(t) => now.duration_since(t) >= interval,
            None => true,
        };
        if should_advance {
            self.state.last_frame_time = Some(now);
            let next = self.state.current_frame + 1;
            if next >= n {
                // Reached end — loop back to start.
                self.state.current_frame = 0;
            } else {
                self.state.current_frame = next;
            }
            // Invalidate preview caches so the new frame renders.
            self.state.preview_frame_loaded = usize::MAX;
            self.state.roi_texture_key = String::new();
        }
        ctx.request_repaint_after(interval);
    }

    fn draw_menu_bar(&mut self, ctx: &egui::Context) {
        egui::TopBottomPanel::top("menu_bar").show(ctx, |ui| {
            egui::menu::bar(ui, |ui| {
                ui.menu_button("File", |ui| {
                    if ui.button("Open TIFF…  Ctrl+O").clicked() {
                        self.state.pending_dialog = Some(DialogRequest::OpenTiff);
                        ui.close_menu();
                    }
                    ui.separator();
                    if ui.button("New Project…").clicked() {
                        self.state.pending_dialog = Some(DialogRequest::NewProject);
                        ui.close_menu();
                    }
                    if ui.button("Open Project…").clicked() {
                        self.state.pending_dialog = Some(DialogRequest::OpenProject);
                        ui.close_menu();
                    }
                    let can_add_run = self.state.project.is_some()
                        && self.state.raw_f.is_some()
                        && self.state.delta_f.is_some();
                    if ui.add_enabled(can_add_run,
                        egui::Button::new("Add current as run")).clicked()
                    {
                        self.state.pending_dialog = Some(DialogRequest::AddCurrentAsRun);
                        ui.close_menu();
                    }
                    ui.separator();
                    if ui.button("Import ROI Set…").clicked() {
                        self.state.pending_dialog = Some(DialogRequest::ImportRoiSet);
                        ui.close_menu();
                    }
                    ui.separator();
                    if ui.button("Quit  Ctrl+Q").clicked() {
                        ctx.send_viewport_cmd(egui::ViewportCommand::Close);
                    }
                });
                ui.menu_button("View", |ui| {
                    if ui.button("Reset Zoom").clicked() { ui.close_menu(); }
                    ui.checkbox(&mut self.state.show_label_overlay, "Frame label overlay");
                });
                ui.menu_button("Help", |ui| {
                    if ui.button("About FIARfly").clicked() { ui.close_menu(); }
                });
            });
        });
    }

    fn draw_status_bar(&mut self, ctx: &egui::Context) {
        egui::TopBottomPanel::bottom("status_bar").show(ctx, |ui| {
            ui.horizontal(|ui| {
                if let Some(path) = &self.state.source_path {
                    ui.label(path.file_name().unwrap_or_default().to_string_lossy().as_ref());
                    ui.separator();
                    if let Some((h, w)) = self.state.image_shape() {
                        ui.label(format!("{} frames × {h}×{w}", self.state.n_frames()));
                    }
                    ui.separator();
                }
                if self.state.worker.is_some() {
                    // Show elapsed time while worker is running.
                    if let Some(start) = self.state.worker_start {
                        let elapsed = start.elapsed().as_secs_f64();
                        ui.label(format!("{elapsed:.0}s"));
                    }
                    ui.add(egui::ProgressBar::new(self.state.progress)
                        .desired_width(200.0)
                        .animate(true));
                    if self.state.batch_running && !self.state.batch_queue.is_empty() {
                        ui.label(format!("{} remaining", self.state.batch_queue.len()));
                    }
                }
                // Live process memory (right-aligned with log message).
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    let rss = process_rss_bytes();
                    if rss > 0 {
                        ui.label(egui::RichText::new(format!("Mem: {}", format_bytes_short(rss)))
                            .small().weak());
                        ui.separator();
                    }
                    if let Some(msg) = self.state.log.last() {
                        ui.label(egui::RichText::new(msg).weak().small());
                    }
                });
            });
        });
    }

    fn draw_stepper(&mut self, ui: &mut egui::Ui) {
        let steps = [
            ActivePanel::Project,
            ActivePanel::Import,
            ActivePanel::MotionCorrection,
            ActivePanel::RoiEditor,
            ActivePanel::SignalViewer,
            ActivePanel::Analysis,
            ActivePanel::Statistics,
            ActivePanel::Export,
        ];
        for step in &steps {
            let is_active = &self.state.active_panel == step;
            let label = Self::step_label(step);
            if ui.add(egui::Button::new(label)
                .selected(is_active)
                .min_size(egui::vec2(160.0, 32.0)))
                .clicked()
            {
                self.state.active_panel = step.clone();
            }
        }
    }
}

impl FiarflyApp {
    /// Drain `AppState::pending_dialog` outside of any egui paint closure.
    ///
    /// Native file dialogs (`rfd` → `NSOpenPanel`) crash intermittently when
    /// invoked from inside an egui paint closure on macOS — `openPanel`
    /// returns nil and `objc2` panics. Running them here in `update` but
    /// before any panels/menus are shown avoids the bad context.
    fn process_pending_dialog(&mut self) {
        let Some(req) = self.state.pending_dialog.take() else { return };
        match req {
            DialogRequest::OpenTiff => {
                panels::import::open_tiff_dialog(&mut self.state);
            }
            DialogRequest::ImportRoiSet | DialogRequest::LoadRoiSet => {
                if let Some(path) = rfd::FileDialog::new()
                    .add_filter("ROI set", &["json"])
                    .pick_file()
                {
                    match fiarfly_core::roi::RoiSet::load(&path) {
                        Ok(loaded) => {
                            self.state.log(format!(
                                "Loaded {} ROIs from {}.",
                                loaded.rois.len(), path.display()
                            ));
                            self.state.roi_set = Some(loaded);
                        }
                        Err(e) => self.state.log(format!("ROI load error: {e}")),
                    }
                }
            }
            DialogRequest::SaveRoiSet => {
                if let Some(path) = rfd::FileDialog::new()
                    .add_filter("ROI set", &["rois.json"])
                    .save_file()
                {
                    if let Some(roi_set) = &self.state.roi_set {
                        match roi_set.save(&path) {
                            Ok(_)  => self.state.log(format!("Saved to {}.", path.display())),
                            Err(e) => self.state.log(format!("Save error: {e}")),
                        }
                    }
                }
            }
            DialogRequest::PickExportDir => {
                if let Some(dir) = rfd::FileDialog::new().pick_folder() {
                    self.state.params.export_path = Some(dir);
                }
            }
            DialogRequest::SaveSessionFile => {
                if let Some(path) = rfd::FileDialog::new()
                    .add_filter("FIARfly session", &["fiarfly"])
                    .save_file()
                {
                    self.state.log(format!(
                        "Session save not yet implemented → {}", path.display()
                    ));
                }
            }
            DialogRequest::AddBatchFiles => {
                if let Some(paths) = rfd::FileDialog::new()
                    .add_filter("TIFF", &["tif", "tiff"])
                    .pick_files()
                {
                    for p in paths {
                        if !self.state.batch_queue.contains(&p) {
                            self.state.batch_queue.push(p);
                        }
                    }
                }
            }
            DialogRequest::PickBatchOutputDir => {
                if let Some(dir) = rfd::FileDialog::new().pick_folder() {
                    self.state.batch_out_dir = Some(dir);
                }
            }
            DialogRequest::NewProject => self.handle_new_project(),
            DialogRequest::OpenProject => self.handle_open_project(),
            DialogRequest::AddCurrentAsRun => self.handle_add_current_as_run(),
        }
    }

    fn handle_new_project(&mut self) {
        let Some(path) = rfd::FileDialog::new()
            .add_filter("FIARfly project", &["fiarproj"])
            .set_file_name("untitled.fiarproj")
            .save_file()
        else { return };
        let name = path.file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("project")
            .to_string();
        match fiarfly_core::io::Project::create_new(&path, &name) {
            Ok(mut project) => {
                // Seed project metadata from the current AppState (if any).
                self.populate_project_from_state(&mut project);
                if let Err(e) = project.save() {
                    self.state.log(format!("Project save error: {e}"));
                }
                self.state.project_description_buf = project.file().description.clone().unwrap_or_default();
                self.state.project = Some(project);
                self.state.log(format!("Created project at {}", path.display()));
                self.state.active_panel = ActivePanel::Project;
            }
            Err(e) => self.state.log(format!("Project create error: {e}")),
        }
    }

    fn handle_open_project(&mut self) {
        let Some(path) = rfd::FileDialog::new().pick_folder() else { return };
        match fiarfly_core::io::Project::open(&path) {
            Ok(mut project) => {
                use fiarfly_core::io::project::AuditAction;
                self.apply_project_to_state(&project);
                project.log_action(AuditAction::ProjectOpened, "Project opened in GUI", None);
                if let Err(e) = project.save() {
                    self.state.log(format!("Project save error: {e}"));
                }
                self.state.log(format!(
                    "Opened project \"{}\" — {} run(s).",
                    project.file().name,
                    project.file().runs.len(),
                ));
                self.state.project_description_buf = project.file().description.clone().unwrap_or_default();
                self.state.project = Some(project);
                self.state.active_panel = ActivePanel::Project;
            }
            Err(e) => self.state.log(format!("Project open error: {e}")),
        }
    }

    fn handle_add_current_as_run(&mut self) {
        use fiarfly_core::io::project::{NewRun, RunInputs};
        let Some(project) = self.state.project.as_mut() else {
            self.state.log("No project open."); return;
        };
        let (Some(raw_f), Some(delta_f)) =
            (self.state.raw_f.as_ref(), self.state.delta_f.as_ref())
        else {
            self.state.log("No traces to save — extract first.");
            return;
        };

        // Sync any roi_set / frame_rate / labels into the project before persisting.
        if let Some(rs) = self.state.roi_set.as_ref() {
            project.file_mut().roi_set = Some(rs.clone());
        }
        if let Some(fr) = self.state.params.frame_rate {
            project.file_mut().frame_rate = Some(fr);
        }
        if let Some(p) = self.state.source_path.as_ref() {
            project.file_mut().source_tiff_path = Some(p.clone());
        }
        project.file_mut().modality = Some((&self.state.params.modality).into());

        let name_buf = self.state.next_run_name.trim();
        let default_name = format!("run_{}", project.file().runs.len() + 1);
        let name = if name_buf.is_empty() { default_name.as_str() } else { name_buf };

        let inputs = RunInputs {
            source_tiff_path: self.state.source_path.clone(),
            used_motion_corrected: self.state.corrected.is_some(),
            neuropil_correction_applied: self.state.neuropil_f.is_some(),
            frame_rate: self.state.params.frame_rate,
            modality: Some((&self.state.params.modality).into()),
        };

        let spec = NewRun {
            name,
            notes: None,
            roi_set: None, // use the project-level set
            raw_f,
            delta_f,
            neuropil_f: self.state.neuropil_f.as_ref(),
            deconvolved: self.state.oasis_result.as_ref(),
            events: None,
            quality: self.state.roi_quality.as_deref(),
            motion_params: Some(&self.state.params.motion),
            delta_f_params: Some(&self.state.params.delta_f),
            oasis_params: Some(&self.state.params.oasis),
            inputs,
        };

        match project.add_run(spec) {
            Ok(id) => {
                self.state.log(format!("Run added → {id}"));
                self.state.next_run_name.clear();
            }
            Err(e) => self.state.log(format!("Run add error: {e}")),
        }
    }

    /// Copy fields from the current AppState into the project file.
    /// Used when creating a new project, so the bundle inherits whatever
    /// the user already configured.
    fn populate_project_from_state(&self, project: &mut fiarfly_core::io::Project) {
        use fiarfly_core::io::project::FrameLabelDef;
        let f = project.file_mut();
        f.source_tiff_path = self.state.source_path.clone();
        f.frame_rate = self.state.params.frame_rate;
        f.image_shape = self.state.image_shape().map(|(h, w)| [h, w]);
        f.modality = Some((&self.state.params.modality).into());
        f.roi_set = self.state.roi_set.clone();
        f.frame_labels = self.state.frame_labels.iter()
            .map(|l| FrameLabelDef {
                name: l.name.clone(), start: l.start, end: l.end,
            })
            .collect();
    }

    /// Apply persisted project state into the live AppState.
    fn apply_project_to_state(&mut self, project: &fiarfly_core::io::Project) {
        use crate::state::FrameLabel;
        let f = project.file();
        if let Some(fr) = f.frame_rate { self.state.params.frame_rate = Some(fr); }
        if let Some(m)  = f.modality.clone() { self.state.params.modality = m.into(); }
        if let Some(rs) = f.roi_set.clone()  { self.state.roi_set = Some(rs); }
        if let Some(p)  = f.source_tiff_path.clone() { self.state.source_path = Some(p); }
        self.state.frame_labels = f.frame_labels.iter()
            .map(|l| FrameLabel { name: l.name.clone(), start: l.start, end: l.end })
            .collect();
    }
}

impl eframe::App for FiarflyApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        self.process_pending_dialog();
        self.poll_worker(ctx);
        self.tick_playback(ctx);
        self.draw_menu_bar(ctx);
        self.draw_status_bar(ctx);

        egui::SidePanel::left("stepper")
            .resizable(false)
            .exact_width(172.0)
            .show(ctx, |ui| {
                ui.add_space(16.0);
                ui.heading("Pipeline");
                ui.add_space(8.0);
                self.draw_stepper(ui);
                ui.add_space(16.0);
                ui.separator();
                ui.label(egui::RichText::new("Log").strong());
                egui::ScrollArea::vertical()
                    .stick_to_bottom(true)
                    .max_height(200.0)
                    .show(ui, |ui| {
                        for msg in &self.state.log {
                            ui.label(egui::RichText::new(msg).small().weak());
                        }
                    });
            });

        egui::CentralPanel::default().show(ctx, |ui| {
            match &self.state.active_panel.clone() {
                ActivePanel::Project          => panels::project::show(ui, &mut self.state),
                ActivePanel::Import           => panels::import::show(ui, &mut self.state),
                ActivePanel::MotionCorrection => panels::motion_correction::show(ui, &mut self.state),
                ActivePanel::RoiEditor        => panels::roi_editor::show(ui, &mut self.state),
                ActivePanel::SignalViewer     => panels::signal_viewer::show(ui, &mut self.state),
                ActivePanel::Analysis         => panels::analysis::show(ui, &mut self.state),
                ActivePanel::Statistics       => panels::stats::show(ui, &mut self.state),
                ActivePanel::Export           => panels::export::show(ui, &mut self.state),
            }
        });

        // Frame label overlay — floating draggable window, shown on all panels.
        if self.state.show_label_overlay && !self.state.frame_labels.is_empty() {
            let frame = self.state.current_frame;
            let active_label = self.state.frame_labels.iter()
                .find(|l| frame >= l.start && frame <= l.end)
                .map(|l| l.name.clone());

            if let Some(label_text) = active_label {
                egui::Window::new("##label_overlay")
                    .title_bar(false)
                    .resizable(false)
                    .movable(true)
                    .default_pos(egui::pos2(ctx.screen_rect().right() - 200.0, 60.0))
                    .show(ctx, |ui| {
                        ui.label(
                            egui::RichText::new(&label_text)
                                .strong()
                                .size(15.0)
                                .color(egui::Color32::from_rgb(100, 220, 255)),
                        );
                        ui.label(
                            egui::RichText::new(format!("frame {frame}"))
                                .small()
                                .weak(),
                        );
                    });
            }
        }
    }
}

fn format_bytes_short(bytes: u64) -> String {
    if bytes >= 1024 * 1024 * 1024 {
        format!("{:.1} GB", bytes as f64 / (1024.0 * 1024.0 * 1024.0))
    } else {
        format!("{} MB", bytes / (1024 * 1024))
    }
}

/// Return the current process's resident set size in bytes.
fn process_rss_bytes() -> u64 {
    #[cfg(target_os = "macos")]
    {
        use std::mem::MaybeUninit;
        let mut info = MaybeUninit::<libc::mach_task_basic_info_data_t>::zeroed();
        let mut count = (std::mem::size_of::<libc::mach_task_basic_info_data_t>()
            / std::mem::size_of::<libc::natural_t>()) as libc::mach_msg_type_number_t;
        #[allow(deprecated)]
        let ret = unsafe {
            libc::task_info(
                libc::mach_task_self(),
                libc::MACH_TASK_BASIC_INFO,
                info.as_mut_ptr().cast(),
                &mut count,
            )
        };
        if ret == libc::KERN_SUCCESS {
            unsafe { info.assume_init().resident_size as u64 }
        } else {
            0
        }
    }
    #[cfg(target_os = "linux")]
    {
        // Read from /proc/self/statm — field 1 is RSS in pages.
        std::fs::read_to_string("/proc/self/statm")
            .ok()
            .and_then(|s| s.split_whitespace().nth(1)?.parse::<u64>().ok())
            .map(|pages| pages * 4096)
            .unwrap_or(0)
    }
    #[cfg(not(any(target_os = "macos", target_os = "linux")))]
    { 0 }
}
