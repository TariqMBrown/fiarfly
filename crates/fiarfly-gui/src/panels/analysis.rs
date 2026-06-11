//! Analysis panel — custom visualizations of extracted traces.
//!
//! Lets the user pick a source (ΔF/F or raw F), one or more frame/time
//! windows, a metric (mean / peak / AUC / latency / rise-time), and a plot
//! type (line over time, or bar of the metric per ROI / per group across
//! windows). Computation is delegated to `fiarfly_core::analysis`.

use eframe::egui;
use egui_plot::{Bar, BarChart, Line, Plot, PlotPoints};
use fiarfly_core::analysis::{compute, Window};
use ndarray::Array2;

use crate::state::{
    ActivePanel, AnalysisMetric, AnalysisSource, AnalysisWindow, AppState, PlotKind, SummaryTable,
};

pub fn show(ui: &mut egui::Ui, state: &mut AppState) {
    egui::ScrollArea::vertical().show(ui, |ui| show_inner(ui, state));
}

fn show_inner(ui: &mut egui::Ui, state: &mut AppState) {
    ui.heading("Analysis");
    ui.add_space(6.0);

    let has_data = state.raw_f.is_some() && state.delta_f.is_some();
    if !has_data {
        ui.label(egui::RichText::new(
            "No traces loaded. Extract traces first (Signal Viewer) \
             or open a project run.",
        ).weak());
        return;
    }

    // ── Source / metric / plot selectors ─────────────────────────────────
    ui.horizontal(|ui| {
        ui.label("Source:");
        egui::ComboBox::from_id_salt("ana_source")
            .selected_text(match state.analysis.source {
                AnalysisSource::DeltaF => "ΔF/F",
                AnalysisSource::RawF   => "Raw F",
            })
            .show_ui(ui, |ui| {
                ui.selectable_value(&mut state.analysis.source, AnalysisSource::DeltaF, "ΔF/F");
                ui.selectable_value(&mut state.analysis.source, AnalysisSource::RawF,   "Raw F");
            });

        ui.separator();
        ui.label("Metric:");
        egui::ComboBox::from_id_salt("ana_metric")
            .selected_text(state.analysis.metric.label())
            .show_ui(ui, |ui| {
                for m in [
                    AnalysisMetric::DffWindowMean,
                    AnalysisMetric::DffWindowPeak,
                    AnalysisMetric::Auc,
                    AnalysisMetric::DffAtFrame,
                    AnalysisMetric::LatencyToPeak,
                    AnalysisMetric::RiseTime,
                ] {
                    ui.selectable_value(&mut state.analysis.metric, m, m.label());
                }
            });

        ui.separator();
        ui.label("Plot:");
        ui.selectable_value(&mut state.analysis.plot_kind, PlotKind::Line, "Line");
        ui.selectable_value(&mut state.analysis.plot_kind, PlotKind::Bar, "Bar");
    });

    ui.add_space(4.0);
    ui.horizontal(|ui| {
        let unit_label = if state.analysis.use_seconds && state.params.frame_rate.is_some() {
            "Window units: seconds"
        } else {
            "Window units: frames"
        };
        if ui
            .selectable_label(state.analysis.use_seconds, unit_label)
            .on_hover_text("Toggle between frames and seconds for window inputs.")
            .clicked()
        {
            state.analysis.use_seconds = !state.analysis.use_seconds;
        }
        if state.analysis.use_seconds && state.params.frame_rate.is_none() {
            ui.label(
                egui::RichText::new("⚠ Set a frame rate in Import to use seconds.")
                    .small()
                    .color(egui::Color32::from_rgb(220, 160, 80)),
            );
        }
        ui.checkbox(
            &mut state.analysis.group_by_roi_group,
            "Aggregate by ROI group",
        );
    });

    ui.add_space(8.0);

    // ── Window editor ────────────────────────────────────────────────────
    egui::CollapsingHeader::new(format!("Windows ({})", state.analysis.windows.len()))
        .default_open(true)
        .show(ui, |ui| {
            // "Add from frame label" shortcut — populates a window from the
            // user's existing frame labels.
            if !state.frame_labels.is_empty() {
                ui.horizontal(|ui| {
                    ui.label("Add from label:");
                    for label in state.frame_labels.clone().iter() {
                        if ui.small_button(&label.name).clicked() {
                            state.analysis.windows.push(label_to_window(
                                label,
                                state.analysis.use_seconds,
                                state.params.frame_rate,
                            ));
                        }
                    }
                });
            }

            let mut to_remove: Option<usize> = None;
            for (i, w) in state.analysis.windows.iter_mut().enumerate() {
                ui.horizontal(|ui| {
                    ui.add(
                        egui::TextEdit::singleline(&mut w.name)
                            .desired_width(100.0)
                            .hint_text(format!("window {}", i + 1)),
                    );
                    ui.label(if state.analysis.use_seconds { "from (s)" } else { "from (frame)" });
                    ui.add(egui::DragValue::new(&mut w.start).speed(0.1).range(0.0..=f32::MAX));
                    ui.label(if state.analysis.use_seconds { "to (s)" } else { "to (frame)" });
                    ui.add(egui::DragValue::new(&mut w.end).speed(0.1).range(0.0..=f32::MAX));
                    if ui
                        .add(egui::Button::new(
                            egui::RichText::new("✕").color(egui::Color32::from_rgb(210, 60, 60)),
                        ).small())
                        .clicked()
                    {
                        to_remove = Some(i);
                    }
                });
            }
            if let Some(i) = to_remove {
                state.analysis.windows.remove(i);
            }

            ui.horizontal(|ui| {
                if ui.button("+ Add window").clicked() {
                    let n = state.analysis.windows.len();
                    let default_end = if state.analysis.use_seconds { 5.0 } else { 100.0 };
                    state.analysis.windows.push(AnalysisWindow {
                        name: format!("Window {}", n + 1),
                        start: 0.0,
                        end: default_end,
                    });
                }
                if ui.button("Clear").clicked() {
                    state.analysis.windows.clear();
                    state.analysis.last_summary = None;
                }
            });
        });

    ui.add_space(8.0);

    // ── Compute button + status ──────────────────────────────────────────
    ui.horizontal(|ui| {
        let can_compute = !state.analysis.windows.is_empty();
        if ui.add_enabled(can_compute, egui::Button::new("Compute")).clicked() {
            match run_compute(state) {
                Ok(summary) => {
                    state.analysis.last_summary = Some(summary);
                }
                Err(msg) => {
                    state.log(format!("Analysis error: {msg}"));
                }
            }
        }
        if let Some(s) = &state.analysis.last_summary {
            ui.label(
                egui::RichText::new(format!(
                    "{} · {} window(s) · {} ROIs",
                    s.metric_label,
                    s.windows.len(),
                    s.roi_labels.len()
                ))
                .small()
                .weak(),
            );
        }
    });

    ui.add_space(8.0);

    // ── Plot ─────────────────────────────────────────────────────────────
    match state.analysis.plot_kind {
        PlotKind::Line => draw_line_plot(ui, state),
        PlotKind::Bar  => draw_bar_plot(ui, state),
    }

    ui.add_space(12.0);

    // ── Navigation ───────────────────────────────────────────────────────
    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
        if ui.button("→ Statistics").clicked() {
            state.active_panel = ActivePanel::Statistics;
        }
    });
}

// ──────────────────────────────────────────────────────────────────────────
// Compute
// ──────────────────────────────────────────────────────────────────────────

fn run_compute(state: &mut AppState) -> Result<SummaryTable, String> {
    let source: &Array2<f32> = match state.analysis.source {
        AnalysisSource::DeltaF => state.delta_f.as_ref().ok_or("no ΔF/F".to_string())?,
        AnalysisSource::RawF   => state.raw_f.as_ref().ok_or("no raw F".to_string())?,
    };
    let roi_set = state.roi_set.as_ref().ok_or("no ROI set".to_string())?;
    let metric = state.analysis.metric.to_core();
    let frame_rate = state.params.frame_rate;

    let mut window_names = Vec::new();
    let mut values: Vec<Vec<f32>> = Vec::new();
    for w in &state.analysis.windows {
        let core_window = build_core_window(w, state.analysis.use_seconds, frame_rate)?;
        let v = compute(source.view(), &core_window, metric)
            .map_err(|e| format!("'{}' failed: {e}", w.name))?;
        window_names.push(w.name.clone());
        values.push(v);
    }

    let roi_labels: Vec<String> = roi_set.rois.iter().map(|r| r.label.clone()).collect();
    let roi_groups: Vec<Option<String>> = roi_set.rois.iter().map(|r| r.group.clone()).collect();
    Ok(SummaryTable {
        metric_label: state.analysis.metric.label().to_string(),
        windows: window_names,
        roi_labels,
        roi_groups,
        values,
    })
}

fn build_core_window(
    w: &AnalysisWindow,
    use_seconds: bool,
    frame_rate: Option<f32>,
) -> Result<Window, String> {
    if w.end <= w.start {
        return Err(format!("'{}' end must be > start", w.name));
    }
    if use_seconds {
        let fr = frame_rate.ok_or_else(|| {
            "frame rate not set — switch units to frames or set rate in Import".to_string()
        })?;
        Ok(Window::seconds(w.start, w.end, fr))
    } else {
        Ok(Window::frames(w.start as usize, w.end as usize))
    }
}

fn label_to_window(
    label: &crate::state::FrameLabel,
    use_seconds: bool,
    frame_rate: Option<f32>,
) -> AnalysisWindow {
    if use_seconds {
        let fr = frame_rate.unwrap_or(1.0);
        AnalysisWindow {
            name: label.name.clone(),
            start: label.start as f32 / fr,
            end: (label.end as f32 + 1.0) / fr,
        }
    } else {
        AnalysisWindow {
            name: label.name.clone(),
            start: label.start as f32,
            end: (label.end + 1) as f32,
        }
    }
}

// ──────────────────────────────────────────────────────────────────────────
// Plotting
// ──────────────────────────────────────────────────────────────────────────

const ROI_COLORS: &[egui::Color32] = &[
    egui::Color32::from_rgb(255, 100, 100),
    egui::Color32::from_rgb(100, 220, 100),
    egui::Color32::from_rgb(100, 180, 255),
    egui::Color32::from_rgb(255, 200, 50),
    egui::Color32::from_rgb(220, 100, 255),
    egui::Color32::from_rgb(255, 150, 50),
    egui::Color32::from_rgb(100, 230, 200),
    egui::Color32::from_rgb(255, 120, 200),
];

fn draw_line_plot(ui: &mut egui::Ui, state: &AppState) {
    let Some(source) = (match state.analysis.source {
        AnalysisSource::DeltaF => state.delta_f.as_ref(),
        AnalysisSource::RawF   => state.raw_f.as_ref(),
    }) else { return; };
    let (n_rois, n_frames) = source.dim();
    let fr = state.params.frame_rate;
    let x_label = if fr.is_some() { "time (s)" } else { "frame" };

    let plot = Plot::new("ana_line_plot")
        .height(320.0)
        .show_axes([true, true])
        .x_axis_label(x_label)
        .y_axis_label(match state.analysis.source {
            AnalysisSource::DeltaF => "ΔF/F",
            AnalysisSource::RawF   => "F",
        })
        .legend(egui_plot::Legend::default());

    plot.show(ui, |plot_ui| {
        for r in 0..n_rois {
            let color = ROI_COLORS[r % ROI_COLORS.len()];
            let label = state.roi_set.as_ref()
                .and_then(|s| s.rois.get(r))
                .map(|r| r.label.clone())
                .unwrap_or_else(|| format!("ROI {}", r + 1));
            let pts: PlotPoints = (0..n_frames)
                .map(|t| {
                    let x = match fr {
                        Some(f) => t as f64 / f as f64,
                        None    => t as f64,
                    };
                    [x, source[[r, t]] as f64]
                })
                .collect();
            plot_ui.line(Line::new(pts).color(color).name(label));
        }

        // Shade each window with a vertical guide pair.
        for w in &state.analysis.windows {
            let (x0, x1) = if state.analysis.use_seconds && fr.is_some() {
                (w.start as f64, w.end as f64)
            } else {
                let fr = fr.unwrap_or(1.0) as f64;
                if state.analysis.use_seconds {
                    (w.start as f64, w.end as f64)
                } else {
                    (w.start as f64 / if fr > 0.0 { fr } else { 1.0 },
                     w.end   as f64 / if fr > 0.0 { fr } else { 1.0 })
                }
            };
            let _ = (x0, x1, w);
            // Use a thin vertical line at each boundary; egui_plot doesn't
            // support filled rectangles in PlotItem cleanly across versions.
            plot_ui.line(
                Line::new(PlotPoints::from(vec![[x0, plot_ui.plot_bounds().min()[1]],
                                                [x0, plot_ui.plot_bounds().max()[1]]]))
                    .color(egui::Color32::from_rgba_unmultiplied(255, 255, 255, 50))
                    .name(format!("{} ◀", w.name)),
            );
            plot_ui.line(
                Line::new(PlotPoints::from(vec![[x1, plot_ui.plot_bounds().min()[1]],
                                                [x1, plot_ui.plot_bounds().max()[1]]]))
                    .color(egui::Color32::from_rgba_unmultiplied(255, 255, 255, 50))
                    .name(format!("{} ▶", w.name)),
            );
        }
    });
}

fn draw_bar_plot(ui: &mut egui::Ui, state: &AppState) {
    let Some(summary) = &state.analysis.last_summary else {
        ui.label(
            egui::RichText::new("Click \"Compute\" to render the bar chart.")
                .weak().small(),
        );
        return;
    };

    let groups: Vec<(String, Vec<usize>)> = if state.analysis.group_by_roi_group {
        group_indices_by_roi_group(summary)
    } else {
        // One "group" per ROI.
        summary
            .roi_labels
            .iter()
            .enumerate()
            .map(|(i, lbl)| (lbl.clone(), vec![i]))
            .collect()
    };

    let n_windows = summary.windows.len();
    let n_groups  = groups.len();
    if n_windows == 0 || n_groups == 0 {
        ui.label("Nothing to plot.");
        return;
    }

    Plot::new("ana_bar_plot")
        .height(320.0)
        .x_axis_label("group")
        .y_axis_label(&summary.metric_label)
        .legend(egui_plot::Legend::default())
        .show(ui, |plot_ui| {
            // Bars are grouped by window, with one bar per group inside each
            // window. X positions: window_idx * (n_groups + 1) + group_idx.
            for (wi, win_name) in summary.windows.iter().enumerate() {
                let mut bars: Vec<Bar> = Vec::with_capacity(n_groups);
                for (gi, (_, idxs)) in groups.iter().enumerate() {
                    let mean = if idxs.is_empty() {
                        0.0
                    } else {
                        let sum: f32 = idxs.iter().map(|i| summary.values[wi][*i]).sum();
                        sum / idxs.len() as f32
                    };
                    let x = (gi as f64) * (n_windows as f64 + 1.0) + wi as f64;
                    bars.push(
                        Bar::new(x, mean as f64)
                            .width(0.85)
                            .fill(ROI_COLORS[wi % ROI_COLORS.len()])
                            .name(format!("{} — {}", win_name, groups[gi].0)),
                    );
                }
                plot_ui.bar_chart(BarChart::new(bars).name(win_name));
            }
        });

    ui.add_space(6.0);
    draw_summary_table(ui, summary, &groups);
}

fn group_indices_by_roi_group(summary: &SummaryTable) -> Vec<(String, Vec<usize>)> {
    use std::collections::BTreeMap;
    let mut map: BTreeMap<String, Vec<usize>> = BTreeMap::new();
    for (i, g) in summary.roi_groups.iter().enumerate() {
        let key = g.clone().unwrap_or_else(|| "(ungrouped)".to_string());
        map.entry(key).or_default().push(i);
    }
    map.into_iter().collect()
}

fn draw_summary_table(
    ui: &mut egui::Ui,
    summary: &SummaryTable,
    groups: &[(String, Vec<usize>)],
) {
    egui::CollapsingHeader::new("Summary table")
        .default_open(false)
        .show(ui, |ui| {
            egui::Grid::new("ana_summary_table")
                .num_columns(summary.windows.len() + 2)
                .striped(true)
                .show(ui, |ui| {
                    ui.label(egui::RichText::new("group").strong());
                    ui.label(egui::RichText::new("n").strong());
                    for w in &summary.windows {
                        ui.label(egui::RichText::new(w).strong());
                    }
                    ui.end_row();

                    for (name, idxs) in groups {
                        ui.label(name);
                        ui.label(format!("{}", idxs.len()));
                        for wi in 0..summary.windows.len() {
                            let mean = if idxs.is_empty() {
                                0.0
                            } else {
                                let sum: f32 = idxs.iter().map(|i| summary.values[wi][*i]).sum();
                                sum / idxs.len() as f32
                            };
                            ui.label(format!("{mean:.4}"));
                        }
                        ui.end_row();
                    }
                });
        });
}
