//! Project panel — shows the active `.fiarproj` bundle:
//!   * top-level metadata (name, author, description, source TIFF, …)
//!   * per-run list (with extraction inputs)
//!   * traceability/audit log

use eframe::egui;
use fiarfly_core::io::project::AuditAction;
use crate::state::{AppState, DialogRequest};

pub fn show(ui: &mut egui::Ui, state: &mut AppState) {
    egui::ScrollArea::vertical().show(ui, |ui| show_inner(ui, state));
}

fn show_inner(ui: &mut egui::Ui, state: &mut AppState) {
    ui.heading("Project");
    ui.add_space(6.0);

    // Top action row: works whether or not a project is open.
    ui.horizontal(|ui| {
        if ui.button("New Project…").clicked() {
            state.pending_dialog = Some(DialogRequest::NewProject);
        }
        if ui.button("Open Project…").clicked() {
            state.pending_dialog = Some(DialogRequest::OpenProject);
        }
        let has_project = state.project.is_some();
        let has_traces  = state.raw_f.is_some() && state.delta_f.is_some();
        let can_add_run = has_project && has_traces;
        if ui.add_enabled(can_add_run, egui::Button::new("Add current as run")).clicked() {
            state.pending_dialog = Some(DialogRequest::AddCurrentAsRun);
        }
        if !has_project {
            ui.label(egui::RichText::new("(no project open)").weak());
        } else if !has_traces {
            ui.label(egui::RichText::new("Extract traces first.").weak().small());
        }
    });

    ui.add_space(8.0);

    // Snapshot the project (clone to release the borrow on `state` so panels
    // below can use `state.log` / `state.project.as_mut`).
    let snapshot = state.project.as_ref()
        .map(|p| (p.path().display().to_string(), p.file().clone()));
    let Some((path_str, file)) = snapshot else {
        ui.label(
            egui::RichText::new(
                "A project bundles ROIs, traces, and per-run parameters from one or more \
                 workflows so you can resume or compare without re-loading the TIFF."
            )
            .small()
            .weak(),
        );
        return;
    };
    egui::CollapsingHeader::new("Metadata").default_open(true).show(ui, |ui| {
        egui::Grid::new("project_meta").num_columns(2).spacing([12.0, 2.0]).show(ui, |ui| {
            ui.label("Name:");          ui.label(&file.name); ui.end_row();
            ui.label("Bundle path:");
            ui.label(egui::RichText::new(&path_str).monospace().small());
            ui.end_row();
            if let Some(author) = &file.author {
                ui.label("Author:"); ui.label(author); ui.end_row();
            }
            ui.label("Created:");       ui.label(format_timestamp(file.created_at)); ui.end_row();
            ui.label("Last modified:"); ui.label(format_timestamp(file.modified_at)); ui.end_row();
            ui.label("Version:");       ui.label(&file.version); ui.end_row();
            if let Some(p) = &file.source_tiff_path {
                ui.label("Source TIFF:");
                ui.label(egui::RichText::new(p.display().to_string()).monospace().small());
                ui.end_row();
            }
            if let Some(fr) = file.frame_rate {
                ui.label("Frame rate:"); ui.label(format!("{fr:.2} Hz")); ui.end_row();
            }
            if let Some(sh) = file.image_shape {
                ui.label("Image:"); ui.label(format!("{} × {} px", sh[0], sh[1])); ui.end_row();
            }
            if let Some(m) = &file.modality {
                ui.label("Modality:"); ui.label(format!("{m:?}")); ui.end_row();
            }
            ui.label("Runs:"); ui.label(format!("{}", file.runs.len())); ui.end_row();
        });

        ui.add_space(4.0);
        ui.label("Description:");
        let resp = ui.add(
            egui::TextEdit::multiline(&mut state.project_description_buf)
                .desired_rows(2)
                .desired_width(f32::INFINITY),
        );
        if resp.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter) || i.key_pressed(egui::Key::Tab)) {
            // commit handled below
        }
        if ui.small_button("Save description").clicked() {
            if let Some(p) = state.project.as_mut() {
                p.file_mut().description = if state.project_description_buf.is_empty() {
                    None
                } else {
                    Some(state.project_description_buf.clone())
                };
                if let Err(e) = p.save() {
                    state.log(format!("Project save error: {e}"));
                } else {
                    state.log("Project description saved.");
                }
            }
        }
    });

    ui.add_space(8.0);

    // ── Run list ──────────────────────────────────────────────────────────
    egui::CollapsingHeader::new(format!("Runs ({})", file.runs.len()))
        .default_open(true)
        .show(ui, |ui| {
            if file.runs.is_empty() {
                ui.label(egui::RichText::new(
                    "No runs yet. Extract traces, then click \"Add current as run\"."
                ).weak().small());
            }
            for run in &file.runs {
                ui.group(|ui| {
                    ui.horizontal(|ui| {
                        ui.label(egui::RichText::new(&run.name).strong());
                        ui.label(egui::RichText::new(format!(
                            "{} ROIs × {} frames",
                            run.n_rois, run.n_frames
                        )).small());
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            ui.label(egui::RichText::new(format_timestamp(run.created_at))
                                .small().weak());
                        });
                    });
                    ui.label(egui::RichText::new(&run.id).small().weak().monospace());
                    let mut tags = Vec::new();
                    if run.has_neuropil { tags.push("neuropil"); }
                    if run.has_spikes   { tags.push("deconvolved"); }
                    if run.has_events   { tags.push("events"); }
                    if run.has_quality  { tags.push("quality"); }
                    if !tags.is_empty() {
                        ui.label(egui::RichText::new(format!("Includes: {}", tags.join(", ")))
                            .small().weak());
                    }
                    if let Some(p) = run.inputs.source_tiff_path.as_ref() {
                        ui.label(egui::RichText::new(format!("Source: {}", p.display()))
                            .small().weak().monospace());
                    }
                    if run.inputs.used_motion_corrected {
                        ui.label(egui::RichText::new("Motion-corrected input").small().weak());
                    }
                    if let Some(notes) = run.notes.as_deref() {
                        ui.label(egui::RichText::new(format!("Notes: {notes}")).small());
                    }
                    if let Some(v) = &run.fiarfly_version {
                        ui.label(egui::RichText::new(format!("fiarfly {v}")).small().weak());
                    }
                });
            }
        });

    ui.add_space(8.0);

    // ── Audit log ─────────────────────────────────────────────────────────
    egui::CollapsingHeader::new(format!("Traceability log ({})", file.audit_log.len()))
        .default_open(false)
        .show(ui, |ui| {
            if file.audit_log.is_empty() {
                ui.label(egui::RichText::new("No entries yet.").weak().small());
            }
            for entry in file.audit_log.iter().rev() {
                ui.horizontal_wrapped(|ui| {
                    ui.label(egui::RichText::new(format_timestamp(entry.timestamp))
                        .small().monospace().weak());
                    ui.label(egui::RichText::new(audit_label(&entry.action))
                        .small().strong().color(audit_color(&entry.action)));
                    ui.label(egui::RichText::new(&entry.description).small());
                    if let Some(rid) = &entry.run_id {
                        ui.label(egui::RichText::new(format!("(→ {rid})"))
                            .small().weak().monospace());
                    }
                });
            }
        });

    ui.add_space(8.0);

    // ── Run-naming row for next "Add as run" ──────────────────────────────
    if state.raw_f.is_some() {
        ui.horizontal(|ui| {
            ui.label("Next run name:");
            ui.add(egui::TextEdit::singleline(&mut state.next_run_name)
                .desired_width(220.0)
                .hint_text("e.g. baseline, stim_block_1"));
        });
    }
}

fn audit_label(a: &AuditAction) -> &'static str {
    match a {
        AuditAction::ProjectCreated   => "CREATE",
        AuditAction::ProjectOpened    => "OPEN",
        AuditAction::RunAdded         => "RUN",
        AuditAction::RoiSetUpdated    => "ROI",
        AuditAction::FrameLabelsEdited => "LABELS",
        AuditAction::SourceTiffSet    => "SRC",
        AuditAction::Note             => "NOTE",
    }
}

fn audit_color(a: &AuditAction) -> egui::Color32 {
    match a {
        AuditAction::ProjectCreated | AuditAction::ProjectOpened
            => egui::Color32::from_rgb(120, 200, 255),
        AuditAction::RunAdded     => egui::Color32::from_rgb(120, 220, 130),
        AuditAction::RoiSetUpdated | AuditAction::FrameLabelsEdited
            => egui::Color32::from_rgb(255, 200, 100),
        AuditAction::SourceTiffSet => egui::Color32::from_rgb(180, 180, 220),
        AuditAction::Note          => egui::Color32::from_rgb(180, 180, 180),
    }
}

/// Format a Unix-seconds timestamp as `YYYY-MM-DD HH:MM:SS` UTC.
fn format_timestamp(unix_secs: u64) -> String {
    // Civil date math: see https://howardhinnant.github.io/date_algorithms.html
    let secs = unix_secs as i64;
    let days = secs.div_euclid(86_400);
    let secs_of_day = secs.rem_euclid(86_400);
    let h = secs_of_day / 3600;
    let m = (secs_of_day % 3600) / 60;
    let s = secs_of_day % 60;

    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097) as u64;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146_096) / 365;
    let y = yoe as i64 + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = if month <= 2 { y + 1 } else { y };
    format!("{year:04}-{month:02}-{d:02} {h:02}:{m:02}:{s:02}")
}

#[cfg(test)]
mod tests {
    use super::format_timestamp;
    #[test]
    fn epoch_zero_renders() {
        assert_eq!(format_timestamp(0), "1970-01-01 00:00:00");
    }
    #[test]
    fn known_timestamp_renders() {
        // 2026-04-26 00:00:00 UTC = 1777161600
        assert_eq!(format_timestamp(1_777_161_600), "2026-04-26 00:00:00");
        // 2024-02-29 12:34:56 UTC = 1709210096 (leap-year sanity)
        assert_eq!(format_timestamp(1_709_210_096), "2024-02-29 12:34:56");
    }
}
