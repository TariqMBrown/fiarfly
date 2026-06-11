//! Export panel — write results to Parquet and CSV.

use eframe::egui;
use fiarfly_core::export::{ExportData, to_dataframe, write_csv, write_parquet};
use crate::state::{ActivePanel, AppState, DialogRequest};

pub fn show(ui: &mut egui::Ui, state: &mut AppState) {
    ui.heading("Export");
    ui.add_space(8.0);

    // Output directory selector.
    ui.horizontal(|ui| {
        let dir_str = state.params.export_path.as_deref()
            .and_then(|p| p.to_str())
            .unwrap_or("(no directory chosen)");
        ui.add(egui::TextEdit::singleline(&mut dir_str.to_owned())
            .desired_width(360.0).interactive(false));
        if ui.button("Browse…").clicked() {
            state.pending_dialog = Some(DialogRequest::PickExportDir);
        }
    });

    ui.add_space(8.0);

    // Filename stem.
    let stem = state.source_path.as_ref()
        .and_then(|p| p.file_stem())
        .and_then(|s| s.to_str())
        .unwrap_or("results")
        .to_owned();
    ui.horizontal(|ui| {
        ui.label("Output files:");
        ui.label(egui::RichText::new(format!("{stem}.parquet  ·  {stem}.csv")).monospace());
    });

    ui.add_space(8.0);

    // Data availability summary.
    egui::Grid::new("export_status").num_columns(2).show(ui, |ui| {
        let check = |ok: bool| if ok { "✓" } else { "✗" };
        ui.label("Stack loaded:");
        ui.label(check(state.best_stack().is_some()));
        ui.end_row();
        ui.label("ROIs defined:");
        ui.label(check(state.roi_set.as_ref().map(|r| !r.rois.is_empty()).unwrap_or(false)));
        ui.end_row();
        ui.label("Traces extracted:");
        ui.label(check(state.raw_f.is_some()));
        ui.end_row();
        ui.label("ΔF/F computed:");
        ui.label(check(state.delta_f.is_some()));
        ui.end_row();
    });

    ui.add_space(12.0);

    let can_export = state.raw_f.is_some()
        && state.delta_f.is_some()
        && state.roi_set.is_some()
        && state.params.export_path.is_some();

    if ui.add_enabled(can_export, egui::Button::new("Export  (.parquet + .csv)")).clicked() {
        run_export(state, &stem);
    }

    if !can_export {
        ui.label(egui::RichText::new(
            "Complete Import → Motion Correction → ROI Editor → Signal Viewer before exporting."
        ).weak().small());
    }

    ui.add_space(8.0);

    if ui.button("Save session (.fiarfly)…").clicked() {
        state.pending_dialog = Some(DialogRequest::SaveSessionFile);
    }

    ui.add_space(16.0);
    if ui.button("← Back").clicked() { state.active_panel = ActivePanel::SignalViewer; }
}

fn run_export(state: &mut AppState, stem: &str) {
    let export_dir = match &state.params.export_path {
        Some(d) => d.clone(),
        None => return,
    };
    let roi_set = match &state.roi_set {
        Some(r) => r,
        None => return,
    };
    let raw_f  = match &state.raw_f  { Some(v) => v, None => return };
    let delta_f = match &state.delta_f { Some(v) => v, None => return };

    let data = ExportData {
        roi_set,
        raw_f,
        delta_f,
        neuropil_f: state.neuropil_f.as_ref(),
        deconvolved: None,
        frame_rate: state.params.frame_rate,
        session_id: stem,
    };

    match to_dataframe(&data) {
        Err(e) => { state.log(format!("Export error: {e}")); }
        Ok(mut df) => {
            let parquet_path = export_dir.join(format!("{stem}.parquet"));
            let csv_path     = export_dir.join(format!("{stem}.csv"));

            if let Err(e) = write_parquet(&mut df, &parquet_path) {
                state.log(format!("Parquet write error: {e}"));
                return;
            }
            if let Err(e) = write_csv(&mut df.clone(), &csv_path) {
                state.log(format!("CSV write error: {e}"));
                return;
            }
            state.log(format!(
                "Exported {} rows → {parquet_path:?}  +  {csv_path:?}",
                df.height()
            ));
        }
    }
}
