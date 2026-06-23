//! Statistics panel — pairwise tests over the Analysis panel's summary
//! table. Operates on the cached `SummaryTable` so the user computes
//! metrics once and runs tests across either windows (paired) or ROI
//! groups (unpaired).

use std::collections::BTreeMap;

use eframe::egui;
use egui_plot::{Line, Plot, PlotPoints, Points};
use fiarfly_core::stats::{adjust, run_test, Comparison, MultipleComparisonsCorrection};

use crate::state::{
    ActivePanel, AppState, StatsAxis, StatsCorrection, StatsKind, StatsRow, SummaryTable,
};

pub fn show(ui: &mut egui::Ui, state: &mut AppState) {
    egui::ScrollArea::vertical().show(ui, |ui| show_inner(ui, state));
}

fn show_inner(ui: &mut egui::Ui, state: &mut AppState) {
    ui.heading("Statistics");
    ui.add_space(6.0);

    // Inputs come from the Analysis panel's last summary.
    let Some(summary) = state.analysis.last_summary.clone() else {
        ui.label(
            egui::RichText::new(
                "No analysis summary available. Open the Analysis panel, define windows, \
                 and click Compute first.",
            )
            .weak(),
        );
        if ui.button("→ Go to Analysis").clicked() {
            state.active_panel = ActivePanel::Analysis;
        }
        return;
    };

    ui.label(
        egui::RichText::new(format!(
            "Source metric: {} ({} window{}, {} ROIs).",
            summary.metric_label,
            summary.windows.len(),
            if summary.windows.len() == 1 { "" } else { "s" },
            summary.roi_labels.len(),
        ))
        .small()
        .weak(),
    );

    ui.add_space(6.0);

    // ── Selectors ─────────────────────────────────────────────────────────
    ui.horizontal(|ui| {
        ui.label("Compare:");
        ui.selectable_value(&mut state.stats_ui.axis, StatsAxis::AcrossWindows,
            "across windows (paired)");
        ui.selectable_value(&mut state.stats_ui.axis, StatsAxis::AcrossGroups,
            "across ROI groups (unpaired)");
    });

    ui.horizontal(|ui| {
        ui.label("Test family:");
        ui.selectable_value(&mut state.stats_ui.kind, StatsKind::Auto,          "Auto");
        ui.selectable_value(&mut state.stats_ui.kind, StatsKind::Parametric,    "Parametric (t / ANOVA)");
        ui.selectable_value(&mut state.stats_ui.kind, StatsKind::NonParametric, "Non-parametric (rank)");
    });

    ui.horizontal(|ui| {
        ui.label("Multiple comparisons:");
        egui::ComboBox::from_id_salt("stats_correction")
            .selected_text(state.stats_ui.correction.label())
            .show_ui(ui, |ui| {
                for c in [
                    StatsCorrection::None,
                    StatsCorrection::Bonferroni,
                    StatsCorrection::BenjaminiHochberg,
                ] {
                    ui.selectable_value(&mut state.stats_ui.correction, c, c.label());
                }
            });
    });

    ui.add_space(8.0);

    // ── Run button ────────────────────────────────────────────────────────
    if ui.button("Run tests").clicked() {
        match run(&summary, &state.stats_ui) {
            Ok(rows) => {
                state.stats_ui.last_results = rows;
                state.stats_ui.last_error = None;
            }
            Err(msg) => {
                state.stats_ui.last_results.clear();
                state.stats_ui.last_error = Some(msg.clone());
                state.log(format!("Stats error: {msg}"));
            }
        }
    }
    if let Some(err) = &state.stats_ui.last_error {
        ui.colored_label(egui::Color32::from_rgb(220, 100, 100), err);
    }

    ui.add_space(8.0);

    // ── Results table + forest plot ───────────────────────────────────────
    if state.stats_ui.last_results.is_empty() {
        ui.label(egui::RichText::new("No results yet.").weak().small());
    } else {
        draw_results_table(ui, &state.stats_ui.last_results);
        ui.add_space(10.0);
        draw_forest_plot(ui, &state.stats_ui.last_results);
    }
}

// ──────────────────────────────────────────────────────────────────────────
// Compute
// ──────────────────────────────────────────────────────────────────────────

fn run(summary: &SummaryTable, ui: &crate::state::StatsUi) -> Result<Vec<StatsRow>, String> {
    let comparisons = build_comparisons(summary, ui.axis)?;

    if comparisons.len() < 2 {
        return Err(
            "Need at least two comparison samples (e.g. ≥2 windows, or ≥2 ROI groups)."
                .into(),
        );
    }

    // If user has ≥3 samples, also run the omnibus test (ANOVA / KW) and
    // then list pairwise tests for interpretability.
    let (axis_kind, omnibus) = match summary_omnibus(&comparisons, ui) {
        Some(r) => (Some(r.test_name.to_string()), Some(r)),
        None    => (None, None),
    };
    let _ = axis_kind;

    let mut rows: Vec<StatsRow> = Vec::new();

    if let Some(om) = omnibus {
        rows.push(StatsRow {
            label_a: "(omnibus)".into(),
            label_b: format!("{} samples", comparisons.len()),
            n_a: om.n.iter().sum(),
            n_b: 0,
            test_name: om.test_name.to_string(),
            statistic: om.statistic,
            p_value: om.p_value,
            p_adjusted: om.p_value,
            effect_size: om.effect_size,
            effect_kind: om.effect_kind.map(|s| s.to_string()),
            mean_a: 0.0,
            mean_b: 0.0,
        });
    }

    // All unique pairs.
    let test_kind = ui.kind.to_core();
    let comparison_type = match ui.axis {
        StatsAxis::AcrossWindows => Comparison::PairedTwo,
        StatsAxis::AcrossGroups  => Comparison::UnpairedTwo,
    };
    for i in 0..comparisons.len() {
        for j in (i + 1)..comparisons.len() {
            let (la, va) = (&comparisons[i].0, &comparisons[i].1);
            let (lb, vb) = (&comparisons[j].0, &comparisons[j].1);
            // Paired needs equal length; if not, fall back to unpaired so
            // we still produce a row.
            let mut comp = comparison_type;
            if matches!(comp, Comparison::PairedTwo) && va.len() != vb.len() {
                comp = Comparison::UnpairedTwo;
            }
            let res = run_test(&[va.as_slice(), vb.as_slice()], comp, test_kind)
                .map_err(|e| format!("{la} vs {lb}: {e}"))?;
            rows.push(StatsRow {
                label_a: la.clone(),
                label_b: lb.clone(),
                n_a: va.len(),
                n_b: vb.len(),
                test_name: res.test_name.to_string(),
                statistic: res.statistic,
                p_value: res.p_value,
                p_adjusted: res.p_value,
                effect_size: res.effect_size,
                effect_kind: res.effect_kind.map(|s| s.to_string()),
                mean_a: mean_f32(va),
                mean_b: mean_f32(vb),
            });
        }
    }

    apply_correction(&mut rows, ui.correction.to_core());
    Ok(rows)
}

/// Build the per-comparison sample vectors from the analysis summary.
///
/// AcrossWindows → one sample per window, paired across ROIs.
/// AcrossGroups  → one sample per ROI-group, taking the *first* window's
///                 values (typical "did groups differ during stim 1?" use).
fn build_comparisons(
    summary: &SummaryTable,
    axis: StatsAxis,
) -> Result<Vec<(String, Vec<f32>)>, String> {
    match axis {
        StatsAxis::AcrossWindows => {
            if summary.windows.len() < 2 {
                return Err("AcrossWindows needs at least 2 windows.".into());
            }
            Ok(summary
                .windows
                .iter()
                .zip(summary.values.iter())
                .map(|(w, v)| (w.clone(), v.clone()))
                .collect())
        }
        StatsAxis::AcrossGroups => {
            // Use the first window's values, partitioned by roi.group.
            let mut by_group: BTreeMap<String, Vec<f32>> = BTreeMap::new();
            let values = summary.values.first().ok_or("No windows")?;
            for (i, g) in summary.roi_groups.iter().enumerate() {
                let key = g.clone().unwrap_or_else(|| "(ungrouped)".into());
                by_group.entry(key).or_default().push(values[i]);
            }
            let groups: Vec<(String, Vec<f32>)> = by_group.into_iter().collect();
            if groups.len() < 2 {
                return Err(
                    "AcrossGroups needs ≥ 2 ROI groups. Tag ROIs with groups in the ROI Editor."
                        .into(),
                );
            }
            Ok(groups)
        }
    }
}

/// Run the omnibus test (ANOVA or Kruskal-Wallis) when there are ≥3
/// samples. Returns None for 2-sample situations (the pairwise covers it).
fn summary_omnibus(
    comparisons: &[(String, Vec<f32>)],
    ui: &crate::state::StatsUi,
) -> Option<fiarfly_core::stats::TestResult> {
    if comparisons.len() < 3 {
        return None;
    }
    let refs: Vec<&[f32]> = comparisons.iter().map(|(_, v)| v.as_slice()).collect();
    run_test(&refs, Comparison::Multi, ui.kind.to_core()).ok()
}

fn apply_correction(rows: &mut [StatsRow], method: MultipleComparisonsCorrection) {
    if matches!(method, MultipleComparisonsCorrection::None) {
        for r in rows.iter_mut() {
            r.p_adjusted = r.p_value;
        }
        return;
    }
    // Don't include the omnibus row in the family-wise adjustment.
    let pairwise_idxs: Vec<usize> = rows
        .iter()
        .enumerate()
        .filter(|(_, r)| r.label_a != "(omnibus)")
        .map(|(i, _)| i)
        .collect();
    let raw_ps: Vec<f64> = pairwise_idxs.iter().map(|&i| rows[i].p_value).collect();
    let adj = adjust(&raw_ps, method);
    for (k, idx) in pairwise_idxs.iter().enumerate() {
        rows[*idx].p_adjusted = adj[k];
    }
}

fn mean_f32(v: &[f32]) -> f64 {
    if v.is_empty() {
        0.0
    } else {
        v.iter().map(|&x| x as f64).sum::<f64>() / v.len() as f64
    }
}

// ──────────────────────────────────────────────────────────────────────────
// Results table
// ──────────────────────────────────────────────────────────────────────────

fn draw_results_table(ui: &mut egui::Ui, rows: &[StatsRow]) {
    ui.label(egui::RichText::new("Results").strong());
    egui::Grid::new("stats_results_grid")
        .num_columns(10)
        .striped(true)
        .show(ui, |ui| {
            for h in [
                "A", "B", "n", "test", "stat", "mean A", "mean B", "p", "p_adj", "effect",
            ] {
                ui.label(egui::RichText::new(h).strong());
            }
            ui.end_row();
            for r in rows {
                ui.label(&r.label_a);
                ui.label(&r.label_b);
                ui.label(if r.n_b == 0 {
                    format!("{}", r.n_a)
                } else {
                    format!("{} / {}", r.n_a, r.n_b)
                });
                ui.label(&r.test_name);
                ui.label(format!("{:.3}", r.statistic));
                ui.label(format!("{:.3}", r.mean_a));
                ui.label(format!("{:.3}", r.mean_b));
                ui.label(p_value_display(r.p_value));
                ui.colored_label(
                    p_color(r.p_adjusted),
                    p_value_display(r.p_adjusted),
                );
                ui.label(match (&r.effect_kind, r.effect_size) {
                    (Some(k), Some(v)) => format!("{:.3} ({})", v, k),
                    _ => "—".into(),
                });
                ui.end_row();
            }
        });
}

fn p_value_display(p: f64) -> String {
    if p < 1e-4 {
        format!("{:.2e}", p)
    } else {
        format!("{:.4}", p)
    }
}

fn p_color(p: f64) -> egui::Color32 {
    if p < 0.001 {
        egui::Color32::from_rgb(140, 230, 140)
    } else if p < 0.01 {
        egui::Color32::from_rgb(200, 230, 130)
    } else if p < 0.05 {
        egui::Color32::from_rgb(230, 220, 130)
    } else {
        egui::Color32::from_rgb(180, 180, 180)
    }
}

// ──────────────────────────────────────────────────────────────────────────
// Forest plot (effect sizes, with horizontal CI bars where derivable)
// ──────────────────────────────────────────────────────────────────────────

fn draw_forest_plot(ui: &mut egui::Ui, rows: &[StatsRow]) {
    ui.label(egui::RichText::new("Effect-size forest plot").strong());
    let pairwise: Vec<&StatsRow> = rows.iter().filter(|r| r.label_a != "(omnibus)").collect();
    if pairwise.is_empty() {
        ui.label(egui::RichText::new("No pairwise rows to plot.").weak().small());
        return;
    }

    Plot::new("stats_forest")
        .height(40.0_f32.max(28.0 * pairwise.len() as f32 + 50.0))
        .x_axis_label("effect size")
        .y_axis_label("comparison")
        .show_axes([true, false])
        .show(ui, |plot_ui| {
            // Vertical zero line.
            let max_y = pairwise.len() as f64 + 0.5;
            plot_ui.line(
                Line::new(PlotPoints::from(vec![[0.0, -0.5], [0.0, max_y]]))
                    .color(egui::Color32::from_gray(140))
                    .name("0"),
            );
            for (i, r) in pairwise.iter().enumerate() {
                let y = (pairwise.len() - 1 - i) as f64;
                let e = r.effect_size.unwrap_or(0.0);
                // Approximate CI: ±0.5 in standardized-effect units.
                // (Real CI by test family is a follow-up.)
                let half = 0.5_f64;
                let color = effect_color(e);
                plot_ui.line(
                    Line::new(PlotPoints::from(vec![[e - half, y], [e + half, y]]))
                        .color(color)
                        .width(2.0)
                        .name(format!("{} vs {}", r.label_a, r.label_b)),
                );
                plot_ui.points(
                    Points::new(PlotPoints::from(vec![[e, y]]))
                        .radius(4.0)
                        .color(color)
                        .name(format!("{} vs {}", r.label_a, r.label_b)),
                );
            }
        });

    ui.label(
        egui::RichText::new(
            "(Whiskers shown at ±0.5 effect units as a visual reference; \
             test-family-specific 95% CIs are a follow-up.)",
        )
        .small()
        .weak(),
    );
}

fn effect_color(e: f64) -> egui::Color32 {
    let mag = e.abs();
    if mag >= 0.8 {
        egui::Color32::from_rgb(140, 230, 140)
    } else if mag >= 0.5 {
        egui::Color32::from_rgb(230, 220, 130)
    } else if mag >= 0.2 {
        egui::Color32::from_rgb(220, 180, 130)
    } else {
        egui::Color32::from_rgb(180, 180, 180)
    }
}
