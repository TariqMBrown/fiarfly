# FIARfly — Implementation Guide

> **Primary reference for any developer or AI picking up this project.**
> Describes what is built, what remains, and exactly where each thing lives.
> Read `ARCHITECTURE.md` for system design context first.

---

## Current Build Status (v0.2)

### v0.2 expansion (PR1–PR8, complete)
| PR | Component | Status | Notes |
|---|---|---|---|
| PR1 | Crash hardening | ✅ Complete | `pending_dialog` deferral fixed `+[NSOpenPanel openPanel]` NULL crash; LUT sliders coupled + defensive sort fixed `f32::clamp` panic. |
| PR2 | `fiarfly-core::io::project` | ✅ Complete | `.fiarproj` directory bundle: `project.json` + per-run parquets. |
| PR3 | GUI Project tab + audit log | ✅ Complete | `panels::project`, File menu items, traceability log with action/timestamp/run_id; metadata + run inputs round-trip. |
| PR4 | `fiarfly-core::analysis` | ✅ Complete | `Window` + 6 metrics (mean / peak / AUC / value-at / latency / rise-time). |
| PR5 | GUI Analysis panel | ✅ Complete | Source/metric/plot selectors, frame-label window shortcuts, line + grouped-bar plots, summary table. |
| PR6 | `fiarfly-core::stats` | ✅ Complete | Welch / paired t / Wilcoxon / Mann-Whitney / one-way ANOVA / Kruskal-Wallis; Bonferroni + BH FDR; effect sizes. |
| PR7 | GUI Statistics panel | ✅ Complete | Across-windows / across-groups, omnibus + pairwise, color-coded p_adj table, forest plot. |
| PR8 | Python bindings | ✅ Complete | PyO3 wrappers for Project / analysis / stats. `build.rs` injects `dynamic_lookup` on macOS. |

### Pre-v0.2 baseline (still in place)
| Component | Status |
|---|---|
| Workspace scaffold | ✅ |
| TIFF I/O (U8/U16/U32/F32/F64, streaming) | ✅ |
| Rigid + non-rigid motion correction | ✅ |
| ROI polygon rasterizer + JSON serialization | ✅ |
| Signal extraction + rolling-percentile ΔF/F + neuropil correction | ✅ |
| OASIS deconvolution + event detection + quality scoring | ✅ |
| Polars export (parquet + csv) | ✅ |
| GUI: Import / Motion / ROI / Signal / Export | ✅ |

### Test counts (workspace, all green)
- **62** tests in `fiarfly-core` — covers `io::project`, `analysis::*`, `stats::*`, plus pre-v0.2 modules.
- **2** tests in `fiarfly-gui` — timestamp formatter for the audit log.
- `cargo test --workspace`: 64 passed / 0 failed.

---

## Repository Layout (actual, current state)

```
fiarfly/
├── Cargo.toml                        # workspace: fiarfly-core, fiarfly-gui, fiarfly-py
├── pyproject.toml                    # Python package root (maturin + uv)
├── README.md
├── ARCHITECTURE.md                   # System design, data flow, tech decisions
├── IMPLEMENTATION_GUIDE.md          # This file
├── docs/
│   ├── PIPELINE_REFERENCE.md        # Calcium imaging science + algorithm reference
│   ├── GUI_DESIGN.md                # ASCII wireframes for all 5 panels
│   └── API_DESIGN.md                # Python API specification
├── crates/
│   ├── fiarfly-core/                  # Pure computation — no GUI, no Python deps
│   │   ├── Cargo.toml
│   │   └── src/
│   │       ├── lib.rs               # FiarflyError, ImageStack, Frame types
│   │       ├── io/
│   │       │   ├── mod.rs
│   │       │   └── tiff.rs          # ✅ TiffReader: open, get_frame, load_all
│   │       ├── motion/
│   │       │   ├── mod.rs           # MotionCorrectionParams, ShiftMap, CorrectionResult
│   │       │   ├── rigid.rs         # ✅ correct_rigid, shift_frame, cross_correlate_2d
│   │       │   └── nonrigid.rs      # ✅ correct_nonrigid, patch-based displacement
│   │       ├── roi/
│   │       │   ├── mod.rs           # Modality enum + defaults
│   │       │   ├── polygon.rs       # ✅ Roi, RoiSet — rasterize, neuropil, serialize
│   │       │   └── mask.rs          # ✅ RoiMask struct
│   │       ├── signal/
│   │       │   ├── mod.rs
│   │       │   ├── extraction.rs    # ✅ extract_raw_fluorescence
│   │       │   ├── delta_f.rs       # ✅ compute_delta_f, apply_neuropil_correction
│   │       │   └── deconvolution.rs # 🔲 OasisParams/OasisResult types only; body unimplemented
│   │       └── export/
│   │           ├── mod.rs
│   │           └── dataframe.rs     # ✅ to_dataframe, write_parquet, write_csv
│   ├── fiarfly-gui/                   # egui desktop application
│   │   ├── Cargo.toml               # deps: fiarfly-core, eframe, egui, egui_plot, rfd, ndarray
│   │   └── src/
│   │       ├── main.rs              # eframe::run_native entry point
│   │       ├── app.rs               # FiarflyApp, poll_worker, menu bar, status bar
│   │       ├── state.rs             # AppState, PipelineParams, WorkerHandle, WorkerOutput
│   │       └── panels/
│   │           ├── mod.rs
│   │           ├── import.rs        # ✅ open_tiff_dialog, file info, modality
│   │           ├── motion_correction.rs  # ✅ spawn_motion_correction (background thread)
│   │           ├── roi_editor.rs    # ✅ Layout + ROI list; 🔲 polygon drawing interaction
│   │           ├── signal_viewer.rs # ✅ spawn_extraction, trace plot, playback
│   │           └── export.rs        # ✅ run_export → parquet + csv
│   └── fiarfly-py/                    # PyO3 bindings
│       ├── Cargo.toml
│       └── src/lib.rs               # 🔲 PySession, PyRoiSet skeletons only
├── python/
│   ├── pyproject.toml
│   ├── fiarfly/
│   │   ├── __init__.py
│   │   ├── session.py               # Python-side Session wrapper
│   │   └── roi.py                   # Python-side RoiSet wrapper
│   └── tests/
│       └── test_io.py
└── .github/
    └── workflows/
        └── ci.yml
```

---

## Prerequisites

| Tool | Version | Purpose |
|---|---|---|
| `rustup` + `cargo` | stable ≥ 1.78 | Rust toolchain |
| Xcode CLI tools | current | macOS linker (`sudo xcodebuild -license`) |
| `uv` | ≥ 0.4 | Python package manager |
| Python | 3.11+ | Python runtime |
| `maturin` | ≥ 1.5 | Build PyO3 wheels |

**One-time macOS setup:**
```bash
sudo xcodebuild -license   # accept Xcode license (required for cc linker)
```

**Build and run the GUI:**
```bash
cd ~/fiarfly
cargo run --release -p fiarfly-gui
```

---

## Architecture in One Diagram

```
┌─────────────────────────────────────────────────────┐
│  fiarfly-gui (egui)          fiarfly Python package      │
│  Desktop app               Jupyter / scripts         │
└────────────┬───────────────────────┬────────────────┘
             │ direct Rust calls     │ PyO3 FFI
             ▼                       ▼
      ┌──────────────────────────────────────┐
      │              fiarfly-py                │
      │        (maturin wheel — 🔲 TODO)     │
      └─────────────────┬────────────────────┘
                        ▼
      ┌──────────────────────────────────────┐
      │           fiarfly-core                 │
      │  io · motion · roi · signal · export │
      └──────────────────────────────────────┘
```

---

## Key Types (fiarfly-core)

```rust
// Central array types (lib.rs)
type ImageStack = ndarray::Array3<f32>;  // [frames, height, width]
type Frame      = ndarray::Array2<f32>;  // [height, width]

// Error type (lib.rs)
enum FiarflyError { Io, Tiff, UnsupportedFormat, DimensionMismatch,
                     InvalidParameter, Serialization, Export }

// Motion (motion/mod.rs)
struct MotionCorrectionParams { mode, max_shift, grid_size, overlap, bin_width }
enum   MotionMode              { Rigid, NonRigid }
struct CorrectionResult        { corrected: ImageStack, shifts: ShiftMap,
                                 correlation_scores: Vec<f32> }

// ROI (roi/polygon.rs, roi/mask.rs)
struct Roi     { id, label, group, color, vertices: Vec<[f32;2]> }
struct RoiSet  { name, image_shape, rois: Vec<Roi> }
struct RoiMask { roi_id, pixels: Vec<[usize;2]> }

// Signal (signal/)
struct DeltaFParams  { baseline_percentile: f32, window_frames: usize }
struct OasisParams   { g, sn, lambda }   // deconvolution — not yet implemented
struct OasisResult   { calcium, spikes, g_estimated }

// Export (export/dataframe.rs)
struct ExportData<'a> { roi_set, raw_f, delta_f, neuropil_f, deconvolved,
                        frame_rate, session_id }
```

---

## Key Types (fiarfly-gui)

```rust
// state.rs
enum ActivePanel  { Import, MotionCorrection, RoiEditor, SignalViewer, Export }
enum Modality     { TwoPhotonInVivo, TwoPhotonInVitro, OnePhotonInVivo, OnePhotonWideField }
struct AppState   { source_path, tiff_reader, stack: Arc<ImageStack>,
                    corrected: Arc<ImageStack>, roi_set, raw_f, delta_f,
                    neuropil_f, shift_scores, active_panel, current_frame,
                    params: PipelineParams, worker: Option<WorkerHandle>,
                    progress, log }
struct WorkerHandle { progress_rx: Receiver<f32>,
                      done_rx: Receiver<Result<WorkerOutput, FiarflyError>> }
enum WorkerOutput { MotionCorrected(CorrectionResult),
                    TracesExtracted { raw, delta_f, neuropil_f } }
```

---

## Data Flow (end-to-end)

```
User opens .tif
  → TiffReader::open()            [io/tiff.rs]
  → TiffReader::load_all()        → ImageStack [frames×H×W, f32 0..1]
  → stored in AppState::stack

User clicks "Run Motion Correction"
  → spawn_motion_correction()     [panels/motion_correction.rs]
  → background thread:
      correct_rigid() or correct_nonrigid()   [motion/rigid.rs, nonrigid.rs]
      ← CorrectionResult { corrected, shifts, correlation_scores }
  → stored in AppState::corrected, AppState::shift_scores

User draws ROIs (polygon editor)
  → vertices stored in AppState::roi_set (RoiSet)
  → can save/load as .rois.json   [roi/polygon.rs RoiSet::save/load]

User clicks "Extract Traces"
  → spawn_extraction()            [panels/signal_viewer.rs]
  → background thread:
      Roi::to_mask() for each ROI → Vec<RoiMask>    [roi/polygon.rs]
      extract_raw_fluorescence()  → Array2<f32> [n_rois × n_frames]  [signal/extraction.rs]
      compute_delta_f()           → Array2<f32> [n_rois × n_frames]  [signal/delta_f.rs]
  → stored in AppState::raw_f, AppState::delta_f

User clicks "Export"
  → run_export()                  [panels/export.rs]
  → to_dataframe()                [export/dataframe.rs]
  → write_parquet() + write_csv()
  → output: stem.parquet, stem.csv
```

---

## Output DataFrame Schema

One row per (ROI × frame). Long/tidy format.

| Column | Type | Always present |
|---|---|---|
| roi_id | String | ✅ |
| roi_label | String | ✅ |
| roi_group | String (nullable) | ✅ |
| session_id | String | ✅ |
| frame_idx | UInt32 | ✅ |
| time_s | Float32 (nullable) | only if frame_rate set |
| raw_f | Float32 | ✅ |
| delta_f_over_f | Float32 | ✅ |
| neuropil_f | Float32 | only if neuropil extracted |
| deconvolved | Float32 | only if OASIS run (🔲 not yet) |

---

## Background Worker Pattern (GUI)

Long operations run off the GUI thread. The pattern used throughout:

```rust
// In a panel's "Run" button handler:
let (progress_tx, progress_rx) = std::sync::mpsc::channel::<f32>();
let (done_tx, done_rx)         = std::sync::mpsc::channel();

std::thread::spawn(move || {
    let result = (|| {
        let _ = progress_tx.send(0.0);
        // ... do work, periodically send progress_tx.send(0.0..1.0) ...
        Ok(WorkerOutput::SomeVariant(data))
    })();
    let _ = done_tx.send(result);
});

state.worker = Some(WorkerHandle { progress_rx, done_rx });
```

```rust
// In app.rs FiarflyApp::poll_worker() — called every frame:
if let Some(worker) = &state.worker {
    while let Ok(p) = worker.progress_rx.try_recv() { state.progress = p; }
    if let Ok(result) = worker.done_rx.try_recv() {
        state.worker = None;
        // apply result to state
    }
}
// ctx.request_repaint_after(50ms) keeps the loop alive during work
```

---

## What Remains To Build

### Phase 6 (partial) — ROI polygon drawing interaction
**File:** `crates/fiarfly-gui/src/panels/roi_editor.rs`

The layout, sidebar, save/load buttons, and ROI list are all present. What's missing is the actual interactive drawing on the canvas. The `DrawState` struct and `DrawMode` enum exist. Need to implement:

1. **Canvas interaction** — wire the egui `Response` mouse events to `DrawState.in_progress`:
   - `response.clicked()` → add vertex to `in_progress`
   - Double-click or click near first vertex (< 8px) → close polygon, create `Roi`
   - `Escape` key → discard `in_progress`
2. **Render ROIs** — use `egui::Painter` to draw filled polygons over the frame texture:
   ```rust
   painter.add(egui::Shape::convex_polygon(screen_verts, fill_color, stroke));
   ```
3. **Frame texture upload** — convert `Frame` (Array2<f32>) to `egui::ColorImage`:
   ```rust
   let pixels: Vec<u8> = frame.iter().map(|&v| (v * 255.0) as u8).collect();
   let img = egui::ColorImage::from_gray([w, h], &pixels);
   let texture = ctx.load_texture("frame", img, Default::default());
   ui.image(&texture);
   ```
4. **Coordinate transform** — convert egui screen pos → image pixel coords:
   ```rust
   fn screen_to_image(pos: Pos2, img_rect: Rect, img_w: f32, img_h: f32) -> [f32; 2] {
       let rel = (pos - img_rect.min) / img_rect.size();
       [rel.x * img_w, rel.y * img_h]
   }
   ```

See `docs/GUI_DESIGN.md §"Panel 3: ROI Editor"` for the full drawing state machine spec.

### Phase 7 — Python bindings (fiarfly-py)

**File:** `crates/fiarfly-py/src/lib.rs`

Skeletons exist for `PySession` and `PyRoiSet`. Each `#[new]` and method returns `PyNotImplementedError`. To implement:

1. `PySession::new` → call `TiffReader::open`
2. `PySession::motion_correct` → call `correct_rigid` / `correct_nonrigid`
3. `PySession::load_rois` → call `RoiSet::load`
4. `PySession::extract_traces` → rasterize ROIs, call extraction + delta_f, return via Arrow IPC

Key pattern for returning a Polars DataFrame to Python:
```rust
// Rust side: serialize to Arrow IPC bytes
let mut buf = Vec::new();
let mut writer = polars::io::ipc::IpcWriter::new(&mut buf);
writer.finish(&mut df)?;
// Return bytes to Python; Python reconstructs with polars.read_ipc(BytesIO(buf))
```

### Phase 4 (deferred) — OASIS deconvolution

**File:** `crates/fiarfly-core/src/signal/deconvolution.rs`

Types and signature are in place. Implement the AR(1) OASIS algorithm:
- Model: `c[t] = g * c[t-1] + s[t]`, `y[t] = c[t] + noise`
- Minimize `||y - c||² + λ||s||₁` subject to `c ≥ 0`, `s ≥ 0`
- Key subroutine: pool-adjacent-violators (PAV)
- Auto-estimate `g` from lag-1 autocorrelation; `sn` from high-freq PSD

Reference: Friedrich et al. 2017 PLoS Comput Biol. Python ref impl: https://github.com/j-friedrich/OASIS

---

## Polars API Notes (version 0.42)

These caused compile errors and were fixed. Keep in mind for future work:

- `Series::new` requires `use polars::prelude::NamedFrom;` in scope
- `CsvWriter::finish()` takes `&mut DataFrame`
- `ParquetWriter::finish()` takes `&mut DataFrame`
- Type inference on `Array2::nrows()` / `ncols()` sometimes fails through trait indirection — annotate the result as `usize` explicitly

---

## Modality Defaults

| Modality | Neuropil correction | Baseline window | Background model |
|---|---|---|---|
| TwoPhotonInVivo | Yes (r=0.7) | 300 frames | Low, structured |
| TwoPhotonInVitro | Yes (r=0.7) | 100 frames | Low, structured |
| OnePhotonInVivo | No | 300 frames | High — ring subtraction |
| OnePhotonWideField | No | 200 frames | High — global subtraction |

Set via `AppState.params.modality`. Signal extraction currently does not auto-switch neuropil on/off based on modality — the GUI checkbox controls it manually. A future improvement is to auto-default the checkbox based on modality selection.

---

## Testing

Run all unit tests:
```bash
cargo test --workspace
```

Key tests that already exist:
- `io/tiff.rs` — synthetic TIFF round-trip (requires `tifffile` Python package in dev deps, or write with tiff crate directly)
- `motion/rigid.rs` — `shift_frame_zero_shift_is_identity`, `rigid_correction_smoke`
- `roi/polygon.rs` — `rasterize_square_pixel_count`, `point_in_polygon`, `roi_set_round_trip`
- `signal/extraction.rs` — `constant_frame_extracts_correct_mean`
- `signal/delta_f.rs` — `delta_f_step_response`, `neuropil_correction_subtracts`

---

## Session File Format (.fiarfly)

Not yet implemented but designed. Will be JSON:
```json
{
  "version": "0.1.0",
  "source_file": "/path/to/recording.tif",
  "modality": "TwoPhotonInVivo",
  "frame_rate": 30.0,
  "motion_correction": {
    "mode": "Rigid",
    "max_shift": 10,
    "grid_size": [32, 32],
    "overlap": 8,
    "bin_width": 50
  },
  "roi_set_path": "recording_rois.rois.json"
}
```

Save/load hooks exist in `panels/export.rs` and `app.rs` menu but write `state.log("not yet implemented")`.
