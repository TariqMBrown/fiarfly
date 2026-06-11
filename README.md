# FIARfly

**A user-friendly calcium imaging analysis tool built in Rust with a Python interface.**

FIARfly provides a fast, cross-platform desktop application for processing, visualising, and statistically analysing calcium imaging data. The core engine is written in Rust for performance and memory safety; a Python wrapper (managed with `uv`) exposes the same pipeline programmatically for scripting and notebook workflows.

---

## Key Features

| Feature | Details |
|---|---|
| Input formats | `.tif` / `.tiff` (single-plane and multi-page stacks) |
| Motion correction | Rigid and non-rigid (NoRMCorre-style FFT patch registration) |
| ROI tools | Hand-drawn polygon ROIs, save/load individual or full mask sets, ROI groups |
| Imaging modalities | 1-photon (miniscope / wide-field) and 2-photon; in vivo and in vitro |
| Signal extraction | ΔF/F traces per ROI with optional neuropil correction |
| Deconvolution | OASIS spike inference (optional) |
| Visualisation | Movie playback, trace viewer, ROI overlay, custom line/bar plots |
| **Projects** | `.fiarproj` directory bundles store ROIs, traces, and per-run metadata across many workflows. **No need to re-load the TIFF.** Includes an append-only **traceability log** of every change. |
| **Custom analysis** | Per-ROI metrics (mean / peak / AUC / latency / 10–90% rise time) over arbitrary windows in frames or seconds; line and grouped-bar plots. |
| **Statistics** | Paired / unpaired / multi-group tests (t / Welch / Wilcoxon / Mann-Whitney / ANOVA / Kruskal-Wallis), effect sizes (Cohen's d/dz, η², rank-biserial), Bonferroni and Benjamini-Hochberg FDR. |
| Export | Polars DataFrames → `.parquet` and `.csv`; project bundles for archival |
| Python API | Full pipeline + analysis + stats accessible via the `fiarfly` Python package |
| Package management | `uv` for all Python dependencies |

---

## Workflow

FIARfly is organised as a **linear pipeline** plus a **Project tab** that stores results across runs:

```
        ┌─────────────┐
        │   Project   │ ◄─── open / save .fiarproj bundles, view runs + audit log
        └──────┬──────┘
               │
   ┌───────────┼─────────────────────────────────────────────────────────────────┐
   ▼           ▼                                                                 ▼
1. Import → 2. Motion → 3. ROI → 4. Signal Viewer → 5. Analysis → 6. Stats → 7. Export
```

A typical session:
1. **Import** a TIFF, set modality and frame rate, define frame labels (e.g. Baseline / Stim 1 / Stim 2).
2. **Motion-correct** with rigid or non-rigid registration; inspect the per-frame correlation plot.
3. **ROI Editor** — draw polygons, assign groups (e.g. `layer_2/3`, `layer_5`), save as a `.rois.json`.
4. **Signal Viewer** — extract traces, view ΔF/F overlays, optionally run OASIS deconvolution + event detection.
5. **Analysis** — pick a metric (window mean, AUC, etc.), define one or more windows in frames or seconds, render line or bar plots.
6. **Statistics** — run paired/unpaired/multi-group tests across windows or ROI groups; apply multiple-comparisons correction.
7. **Export** to `.parquet` / `.csv`, or **save the entire workflow into a `.fiarproj` bundle** to resume later or compare against new conditions without re-loading the TIFF.

---

## Technology Stack

| Layer | Technology | Rationale |
|---|---|---|
| GUI framework | [`egui`](https://github.com/emilk/egui) + [`eframe`](https://github.com/emilk/egui/tree/master/crates/eframe) | Pure Rust, immediate-mode, excellent for scientific apps |
| Plotting | [`egui_plot`](https://github.com/emilk/egui/tree/master/crates/egui_plot) | Native egui integration |
| Image I/O | [`tiff`](https://crates.io/crates/tiff) crate | Pure Rust TIFF support |
| Array math | [`ndarray`](https://crates.io/crates/ndarray) | NumPy-equivalent for Rust |
| FFT | [`rustfft`](https://crates.io/crates/rustfft) | Fast Fourier transforms for motion correction |
| Parallelism | [`rayon`](https://crates.io/crates/rayon) | Data parallelism across frames |
| Statistics | [`statrs`](https://crates.io/crates/statrs) | Distributions for p-values |
| Python bindings | [`pyo3`](https://pyo3.rs) + [`maturin`](https://www.maturin.rs) | Ergonomic Rust↔Python FFI |
| Python runtime | [`uv`](https://github.com/astral-sh/uv) | Fast, modern Python package management |
| DataFrames | [`polars`](https://pola.rs) (Rust crate) | Native DataFrames without Python dependency |
| Serialisation | [`serde`](https://serde.rs) + [`serde_json`](https://crates.io/crates/serde_json) | ROI, project, audit log on disk |

---

## Project Status

> **v0.2 — Projects, Analysis, Statistics**

See [`ARCHITECTURE.md`](ARCHITECTURE.md) for the full v0.2 expansion plan and [`IMPLEMENTATION_GUIDE.md`](IMPLEMENTATION_GUIDE.md) for the build roadmap.

---

## Repository Layout

```
fiarfly/
├── README.md                    # This file
├── ARCHITECTURE.md              # System design, technical decisions, expansion plan
├── IMPLEMENTATION_GUIDE.md      # Step-by-step build guide
├── SETUP.md                     # Installation and run instructions
├── USER_GUIDE.md                # End-user manual: every panel and feature
├── Cargo.toml                   # Rust workspace root
├── pyproject.toml               # Python package root (uv-managed)
├── docs/
│   ├── PIPELINE_REFERENCE.md   # Calcium imaging science + algorithm reference
│   ├── GUI_DESIGN.md           # UI layout specifications and navigation map
│   └── API_DESIGN.md           # Python API design
├── crates/
│   ├── fiarfly-core/             # Core Rust library (no GUI, no Python deps)
│   │   └── src/
│   │       ├── analysis/        # v0.2 — metrics over windows
│   │       ├── stats/           # v0.2 — t-tests, Mann-Whitney, ANOVA, FDR
│   │       ├── io/              # TIFF + .fiarproj read/write
│   │       ├── motion/          # rigid + non-rigid correction
│   │       ├── roi/             # polygon + mask types
│   │       ├── signal/          # ΔF/F, neuropil, OASIS, events, quality
│   │       └── export/          # Parquet / CSV via Polars
│   ├── fiarfly-gui/              # egui desktop application (7 panels)
│   └── fiarfly-py/              # PyO3 Python bindings (built with maturin)
├── python/
│   ├── fiarfly/                  # Python package (wraps fiarfly-py)
│   └── tests/
└── .github/
    └── workflows/
        └── ci.yml
```

---

## Quick Start

See **[SETUP.md](SETUP.md)** for full installation instructions and **[USER_GUIDE.md](USER_GUIDE.md)** for the panel-by-panel walkthrough.

### Run the GUI
```bash
cargo run --release -p fiarfly-gui
```

### Set up the Python package
```bash
conda deactivate          # if using conda
uv sync                   # create .venv and install dependencies
uv run maturin develop    # build the Rust extension
```

### Use the Python API (v0.2)
```python
import fiarfly
import numpy as np

# Open a saved project bundle (no TIFF required) and inspect it.
proj = fiarfly.Project.open("/data/study_a.fiarproj")
print(proj.name, len(proj.runs), proj.author)

# Read a run's traces back as dict-rows; pipe into your DataFrame library of choice.
rows = proj.load_run_traces(proj.runs[0].id)
print(rows[0])  # {'roi_id': 'roi_001', 'frame_idx': 0, 'time_s': 0.0, 'raw_f': ..., ...}

# Compute the same window metrics the Analysis panel uses.
delta_f = np.random.rand(10, 1800).astype("float32")  # 10 ROIs, 60 s @ 30 Hz
mean_dff = fiarfly.window_mean(delta_f, start=10.0, end=15.0, frame_rate=30.0, seconds=True)
auc      = fiarfly.auc       (delta_f, start=25.0, end=35.0, frame_rate=30.0, seconds=True)

# Run the same statistical tests the Stats panel uses.
res = fiarfly.welch_t(mean_dff[:5].tolist(), mean_dff[5:].tolist())
print(res.test_name, res.statistic, res.p_value, res.effect_size)
adj = fiarfly.benjamini_hochberg([0.001, 0.01, 0.03, 0.05])
```

---

## Contributing

See [`IMPLEMENTATION_GUIDE.md`](IMPLEMENTATION_GUIDE.md) for the build roadmap and [`ARCHITECTURE.md`](ARCHITECTURE.md) for system design.

---

## License

MIT — see `LICENSE` (to be added).
