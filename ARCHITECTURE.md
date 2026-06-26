# FIARfly — Architecture

## System Overview

FIARfly is organized as a Rust workspace with three crates and a Python package layer on top.

```
┌─────────────────────────────────────────────────────────────────┐
│                        USER INTERFACES                          │
│                                                                 │
│   ┌─────────────────────────┐   ┌─────────────────────────┐   │
│   │    fiarfly-gui (egui)     │   │  fiarfly Python package   │   │
│   │  Desktop GUI application│   │  Jupyter / scripts      │   │
│   └────────────┬────────────┘   └────────────┬────────────┘   │
│                │ direct Rust calls            │ PyO3 FFI       │
│                ▼                              ▼                 │
│   ┌─────────────────────────────────────────────────────────┐  │
│   │                    fiarfly-py                             │  │
│   │           PyO3 bindings / maturin wheel                 │  │
│   └─────────────────────────┬───────────────────────────────┘  │
│                             │                                   │
│                             ▼                                   │
│   ┌─────────────────────────────────────────────────────────┐  │
│   │                    fiarfly-core                           │  │
│   │  ┌──────┐ ┌──────────┐ ┌─────┐ ┌────────┐ ┌────────┐  │  │
│   │  │  io  │ │  motion  │ │ roi │ │ signal │ │ export │  │  │
│   │  └──────┘ └──────────┘ └─────┘ └────────┘ └────────┘  │  │
│   └─────────────────────────────────────────────────────────┘  │
└─────────────────────────────────────────────────────────────────┘
```

---

## Crate Responsibilities

### `fiarfly-core`
The pure computation library. Has **no GUI dependencies, no Python dependencies**. This is the unit-testable heart of the project.

| Module | Responsibility |
|---|---|
| `io` | Load multi-page TIFF stacks into `ndarray::Array3<f32>` (frames × height × width). Lazy/chunked loading for large files. |
| `motion` | Rigid and non-rigid motion correction. FFT-based patch registration. Returns corrected frames + shift map. |
| `roi` | ROI data structures (polygons, masks), draw/hit-test, serialization to/from JSON. |
| `signal` | Extract per-ROI fluorescence, compute ΔF/F, neuropil correction, optional OASIS deconvolution. |
| `export` | Write results to Polars DataFrames (via the `polars` Rust crate), serialize to `.parquet` and `.csv`. |

### `fiarfly-gui`
The egui desktop application. Thin orchestration layer that calls `fiarfly-core`. Manages application state, panels, and rendering.

Display-only helpers live alongside the panels — e.g. `colormap.rs` provides ImageJ-style pseudo-color LUTs (Grays/Fire/Ice/Green/Magenta/Viridis/Inferno) mapping a normalized intensity to RGB. The selected LUT (`AppState::display_colormap`) is applied when uploading the raw-fluorescence textures in the Import preview and ROI-editor canvas; it never touches computed values, so it stays in the GUI crate, not `fiarfly-core`.

### `fiarfly-py`
PyO3 bindings compiled by `maturin`. Exposes `fiarfly-core` types and functions to Python. The Python-side `fiarfly` package wraps these bindings with Pythonic sugar.

---

## Data Flow (end-to-end)

```
.tiff file on disk
       │
       ▼ fiarfly_core::io::load_tiff()
Array3<f32>  [frames × height × width]
       │
       ▼ fiarfly_core::motion::correct()
Array3<f32>  [motion-corrected] + ShiftMap
       │
       ├──── displayed in GUI (egui texture upload)
       │
       ▼ fiarfly_core::roi::apply_rois()
Vec<RoiMask>
       │
       ▼ fiarfly_core::signal::extract()
DataFrame  [frame_idx, roi_id, raw_f, delta_f_over_f, deconvolved?]
       │
       ├──── plotted in trace viewer (egui_plot)
       │
       ▼ fiarfly_core::export::write()
.parquet / .csv
```

---

## Key Technology Decisions

### GUI: `egui` + `eframe`
- **Why not Tauri?** Tauri requires a web frontend. For a scientific imaging tool with direct pixel-level image manipulation, staying in Rust avoids the JS/WebAssembly image-buffer bridge overhead and simplifies the rendering pipeline.
- **Why not `iced`?** `iced` is excellent but its retained-mode model adds complexity for interactive ROI drawing (where we need per-frame hit testing and drag state). `egui`'s immediate mode is simpler for this.
- **Tradeoff:** `egui` UIs can look less polished by default. This is acceptable for a research tool.

### Image Array: `ndarray`
- Provides N-dimensional typed arrays with slicing/broadcasting akin to NumPy.
- Integrates with `rayon` for parallel frame processing via `ndarray::parallel`.
- Clean interop with PyO3 via `numpy` crate for zero-copy array sharing.

### FFT: `rustfft`
- Pure Rust, no C dependency.
- Used in motion correction for template matching and in signal processing.

### Python Bindings: PyO3 + maturin
- `maturin develop` builds a `.so` / `.pyd` in-place for development.
- `maturin build --release` produces a wheel for distribution.
- The Python `fiarfly` package imports the native extension and adds type hints, docstrings, and convenience wrappers.

### Package Management: `uv`
- All Python dependencies declared in `pyproject.toml`.
- Developers run `uv sync` to get a reproducible environment.
- No `requirements.txt` — `uv.lock` is the ground truth.

### Export: Polars (Rust crate)
- Using the Rust Polars crate directly in `fiarfly-core` means export works from both the GUI and the Python bindings without requiring a Python runtime.
- Outputs `.parquet` (preferred, typed, compressed) and `.csv` (for compatibility).
- Each row in the output DataFrame corresponds to one (frame, ROI) pair.

---

## Concurrency Model

| Operation | Strategy |
|---|---|
| TIFF loading | Single-threaded I/O → Rayon parallel decode per frame |
| Motion correction | Rayon `par_iter` over frames (each frame independent) |
| ROI extraction | Rayon `par_iter` over (frame × ROI) pairs |
| GUI rendering | Single-threaded egui event loop |
| Background computation | `std::thread` + channels, results polled by GUI event loop |

Long-running operations (motion correction on large stacks) run on a background thread pool. The GUI shows a progress bar updated via a `std::sync::mpsc` channel.

---

## Session File Format

ROIs, correction parameters, and pipeline settings are saved in a `.fiarfly` session file (JSON under the hood via `serde_json`):

```json
{
  "version": "0.1.0",
  "source_file": "recording.tif",
  "motion_correction": {
    "mode": "nonrigid",
    "grid_size": [32, 32],
    "max_shift": 10,
    "overlap": 8
  },
  "rois": [
    {
      "id": "roi_001",
      "label": "Cell 1",
      "polygon": [[x1,y1], [x2,y2], "..."],
      "group": "layer_2/3"
    }
  ],
  "modality": "two_photon"
}
```

---

## Supported Imaging Modalities

| Modality | Background model | Preferred extraction |
|---|---|---|
| 2-photon (2P) | Low, structured | CNMF / simple mask |
| 1-photon (miniscope / wide-field) | High, spatially varying | CNMF-E style; background subtracted ΔF/F |

The `modality` field in the session file switches defaults (background estimation, spatial filter sigma, etc.). Users can override any parameter.

---

## Directory Structure (full)

```
fiarfly/
├── Cargo.toml                        # [workspace] members
├── pyproject.toml                    # Python package (uv)
├── README.md
├── ARCHITECTURE.md                   # This file
├── IMPLEMENTATION_GUIDE.md
├── docs/
│   ├── PIPELINE_REFERENCE.md
│   ├── GUI_DESIGN.md
│   └── API_DESIGN.md
├── crates/
│   ├── fiarfly-core/
│   │   ├── Cargo.toml
│   │   └── src/
│   │       ├── lib.rs
│   │       ├── io/
│   │       │   ├── mod.rs
│   │       │   ├── tiff.rs
│   │       │   └── project.rs        # v0.2 — project bundle read/write
│   │       ├── motion/
│   │       │   ├── mod.rs
│   │       │   ├── rigid.rs
│   │       │   └── nonrigid.rs
│   │       ├── roi/
│   │       │   ├── mod.rs
│   │       │   ├── polygon.rs
│   │       │   └── mask.rs
│   │       ├── signal/
│   │       │   ├── mod.rs
│   │       │   ├── extraction.rs
│   │       │   ├── delta_f.rs
│   │       │   └── deconvolution.rs
│   │       └── export/
│   │           ├── mod.rs
│   │           └── dataframe.rs
│   ├── fiarfly-gui/
│   │   ├── Cargo.toml
│   │   └── src/
│   │       ├── main.rs
│   │       ├── app.rs           # AppState, top-level eframe::App impl
│   │       ├── state.rs         # Shared application state types
│   │       └── panels/
│   │           ├── mod.rs
│   │           ├── import.rs
│   │           ├── motion_correction.rs
│   │           ├── roi_editor.rs
│   │           ├── signal_viewer.rs
│   │           ├── analysis.rs         # v0.2 — custom viz (line/bar, ΔF/F, AUC)
│   │           ├── stats.rs            # v0.2 — group/timepoint comparisons
│   │           └── export.rs
│   └── fiarfly-py/
│       ├── Cargo.toml
│       └── src/
│           └── lib.rs
├── python/
│   ├── pyproject.toml
│   ├── uv.lock
│   ├── fiarfly/
│   │   ├── __init__.py
│   │   ├── session.py
│   │   ├── roi.py
│   │   └── py.typed
│   └── tests/
│       ├── test_io.py
│       ├── test_motion.py
│       └── test_signal.py
└── .github/
    └── workflows/
        └── ci.yml
```

---

## Expansion Plan — v0.2 (Projects, Analysis, Statistics)

This section plans the next major release before implementation begins. Phase 0 is a hotfix phase for two recurring crashes; Phases 1–3 add the new capabilities.

### Phase 0 — Crash hardening (carries over from Calsight)

| # | Crash | Root cause | Fix |
|---|---|---|---|
| 0.1 | `unexpected NULL returned from +[NSOpenPanel openPanel]` at [import.rs:9](crates/fiarfly-gui/src/panels/import.rs#L9) | `rfd::FileDialog::pick_file()` is invoked synchronously from inside an egui paint closure. On macOS this dispatches `run_on_main` while winit's own autoreleasepool is already active on the event loop — intermittently `NSOpenPanel` returns nil. | (a) Set a `AppState::pending_dialog: Option<DialogRequest>` flag inside the closure; run the picker in `FiarflyApp::update()` **after** egui's frame closure returns, then apply results next frame. (b) Alternative: switch to `rfd::AsyncFileDialog`, poll `Future` across frames. Pick (a) — simpler, keeps sync flow. |
| 0.2 | `f32::clamp` panic `min > max` at [roi_editor.rs:225](crates/fiarfly-gui/src/panels/roi_editor.rs#L225) (and calci-gui `roi_editor.rs:207` — same bug inherited) | LUT min/max sliders are independent; user can drive `lut_min > lut_max`. The existing `(max-min).max(1e-6)` guard only protects the division, not the `clamp()` call. | Normalize locally: `let (lo, hi) = (lut_min.min(lut_max), (lut_min.max(lut_max)).max(lut_min.min(lut_max) + 1e-6));` before constructing `to_u8`. Additionally, constrain the UI: `roi_lut_max` DragValue `range = (roi_lut_min + eps)..=..`, and vice versa. |
| 0.3 | Defensive audit | Same `f32::clamp(min,max)` pattern is likely repeated elsewhere (signal plot y-axis, motion shift limits). | `rg "\.clamp\(" crates/fiarfly-gui` and harden every case where both bounds come from user input. |

### Phase 1 — Projects: workflow persistence without the TIFF

#### Goal
After a workflow ends, save everything *except* the raw TIFF (ROIs, traces, events, deconv, frame labels, params) so the user can reopen the project, continue analysis, add new workflows/runs, or compare visually — without paying the cost to re-decode gigabytes of TIFF.

#### File format: `.fiarproj` directory bundle

```
mystudy.fiarproj/
├── project.json              # metadata, params, ROI polygons, frame labels
├── runs/
│   ├── 2026-04-23T10-15_baseline/
│   │   ├── run.json              # run-level metadata + params snapshot
│   │   ├── traces.parquet         # tidy: roi_id, frame_idx, time_s, raw_f, delta_f, neuropil_f
│   │   ├── events.parquet         # optional
│   │   ├── spikes.parquet         # optional (OASIS)
│   │   └── quality.parquet        # optional ROI quality metrics
│   └── 2026-04-23T14-02_rerun/
│       └── ...
└── thumbnail.png             # mean projection for the picker preview
```

- Directory bundle (not single file): lets `runs/` grow, keeps Parquet readable by Python/R without parsing a custom container.
- `project.json` is human-readable and diff-friendly.
- Per-run parquet files mirror the existing `export::dataframe::to_dataframe` output → we **reuse** that code path.
- No TIFF is stored. `project.json` remembers `source_tiff_path`; if still present we offer to reload for re-rendering ROI overlays, otherwise analysis proceeds trace-only.

#### Code changes

- **`fiarfly-core::io::project`** (new module):
  - `Project::save(path, &AppState) -> Result<()>`
  - `Project::open(path) -> Result<Project>`
  - `Project::add_run(&mut self, Run)`
  - Serde types: `ProjectFile`, `RunMetadata`.
- **`fiarfly-gui::state::AppState`** additions:
  - `project: Option<Project>`
  - `active_run_id: Option<String>`
- **`fiarfly-gui::app`**: new menu bar with `File → New Project / Open Project… / Save / Save As… / Add Run to Project`. Respect Phase 0.1's deferred-dialog pattern.
- **Recent projects**: small `recents.json` under platform config dir (`directories::ProjectDirs`).

#### UX entry points

- Landing screen gets two primary actions: "New workflow (load TIFF…)" and "Open project…".
- When a project is open, the Import panel becomes a *Runs* overview showing all runs with their metrics; double-click to enter.

### Phase 2 — Analysis panel: custom visualizations

New panel [`analysis.rs`](crates/fiarfly-gui/src/panels/analysis.rs). Operates on the active run's traces (`raw_f`, `delta_f`, `neuropil_f`, optionally `spikes`), ROI groups, and frame labels.

#### Metrics (the "what")

| Metric | Definition | Input |
|---|---|---|
| `dff_at_frame` | ΔF/F at a single frame | frame index |
| `dff_window_mean` | mean ΔF/F over [start, end] | frame range or label |
| `dff_window_peak` | max ΔF/F over [start, end] | frame range or label |
| `auc` | trapezoidal integral of ΔF/F over [start, end] | frame range or label |
| `latency_to_peak` | frames (or sec) from window start to peak | frame range |
| `rise_time` | 10→90% rise within window | frame range |

All implemented in `fiarfly-core::analysis` (new module), pure functions on `ArrayView2<f32>` + a window spec. Unit-testable without GUI.

#### Plots (the "how")

- **Line plot** — trace overlay per ROI or per group; mean±SEM band; optional event raster overlay.
- **Bar plot** — per-ROI or per-group summary of one metric over one window. Individual points scattered over bars.
- **Grouped bar plot** — metric across multiple windows, grouped by ROI group (this directly serves the "10–15 s vs 25–35 s" use case).
- **Heatmap** — ROI × frame ΔF/F with labeled window guides (stretch goal).

Uses `egui_plot`. Export a PNG via `egui_plot::PlotResponse` screenshot, and export the underlying summary table as CSV/parquet.

#### Window picker UX

- Preset dropdown populated from `state.frame_labels` ("Baseline", "Stim 1", …) — this is why labels already exist in state.
- Or custom: two number inputs in frames or seconds (seconds if `frame_rate` is set).
- Multiple windows can be added to a single chart (primary/secondary/...).

### Phase 3 — Statistics panel: comparisons

New panel [`stats.rs`](crates/fiarfly-gui/src/panels/stats.rs). Depends on Phase 2 (reuses metric computation).

#### Comparison types

| Design | Test (parametric) | Test (non-parametric) | Use case |
|---|---|---|---|
| Two windows, same ROIs | paired t-test | Wilcoxon signed-rank | "Is ΔF/F larger at 25–35 s than at 10–15 s in the same cells?" |
| Two groups, same window | Welch's t-test | Mann-Whitney U | "Do layer 2/3 cells differ from layer 5 during Stim 1?" |
| ≥3 windows/groups | one-way repeated-measures or one-way ANOVA | Friedman / Kruskal-Wallis | Multi-condition comparison |
| Post-hoc | Tukey HSD | Dunn's test | Which pair drove the ANOVA hit |

#### Multiple comparisons

- Bonferroni (default, conservative)
- Benjamini-Hochberg FDR (toggle)

#### Diagnostics

- Shapiro-Wilk normality per group
- Levene equal-variance
- Recommend parametric vs. non-parametric with a short rationale string shown in the UI.

#### Effect sizes

- Cohen's d (between), Cohen's dz (paired), rank-biserial r for non-parametric.

#### Crate dependency

Add [`statrs`](https://crates.io/crates/statrs) (already pure Rust) to `fiarfly-core` for distributions and basic tests. Implement Wilcoxon, Mann-Whitney, Friedman, Dunn's ourselves (small code). Put it all in a new `fiarfly-core::stats` module; the GUI consumes a tidy `StatResult` struct.

#### Output

- Results table with per-comparison row: test, statistic, df, p, p_adj, effect_size, n.
- Forest plot of effect sizes with 95% CI.
- "Export report" → CSV of the table + PNG of the forest plot.

### Phase 4 — Cross-project comparison (stretch)

Allow `File → Open additional project for comparison`; stats panel gains a "dataset" axis so you can ask: ΔF/F at Stim 1 in project A vs project B, group-matched.

### Concurrency & state model

- Analysis and stats computations are fast (ms–s); run synchronously in-panel unless a metric is triggered on all ROIs × all frames, in which case reuse the existing `WorkerHandle` pattern in [state.rs](crates/fiarfly-gui/src/state.rs).
- Projects are lightweight (< 100 MB typical); load synchronously with a spinner.

### Reuse and backward compatibility

- Existing `.fiarfly` session JSON is upgraded to `project.json` on load (version bump 0.1 → 0.2).
- Python `fiarfly` package gets `Project.open()` / `Project.save()` bindings via PyO3.

### Build order (suggested PR sequence)

1. **PR1** — Phase 0 crash fixes (small, ships immediately).
2. **PR2** — `fiarfly-core::io::project` types + roundtrip tests, no GUI.
3. **PR3** — GUI menu + project open/save flow; Runs picker; backward-compat session upgrade.
4. **PR4** — `fiarfly-core::analysis` metrics + unit tests.
5. **PR5** — Analysis panel UI (line + bar plots, window picker).
6. **PR6** — `fiarfly-core::stats` tests + effect sizes.
7. **PR7** — Statistics panel UI.
8. **PR8** — Python bindings for project, analysis, stats.

### Open design questions (resolve before PR2)

1. **Project bundle vs. single file?** Bundle recommended; confirm before committing schema.
2. **ROI polygon storage**: keep in `project.json` (current) or move to `rois.parquet`? JSON is fine until N > ~500 ROIs.
3. **Do we store the mean/max projections?** Yes → `projections.npy` or `projections.parquet`; needed to re-render the ROI canvas without the TIFF.
4. **Seconds vs. frames as the canonical analysis-time axis?** Seconds if `frame_rate` set, else frames; metrics always accept either and convert internally.
