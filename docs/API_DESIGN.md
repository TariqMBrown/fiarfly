# FIARfly — Python API Reference (v0.2)

The `fiarfly` Python package is a thin PyO3 wrapper over `fiarfly-core`.
It exposes:

* **Project bundles** — read / write `.fiarproj` directories.
* **Analysis metrics** — per-ROI window metrics matching the GUI's Analysis panel.
* **Statistics** — t-tests, Wilcoxon, Mann-Whitney, ANOVA, Kruskal-Wallis, FDR.

The high-level legacy `Session` class is **not** part of v0.2 — the recommended workflow is to drive the desktop GUI for the heavy ingest/motion-correct/ROI-drawing steps, save a `.fiarproj` bundle, then use Python to extend the analysis or generate figures.

---

## Installation

After cloning:

```bash
conda deactivate          # if applicable
uv sync                   # creates .venv with deps
uv run maturin develop    # builds the Rust extension into .venv
uv run python -c "import fiarfly; print(fiarfly.__version__)"
# 0.1.0
```

Use `./dev.sh build` afterwards whenever you change Rust code.

For full setup including system requirements and troubleshooting see [SETUP.md](../SETUP.md).

---

## Quick start

```python
import fiarfly
import numpy as np
import polars as pl

# 1. Open a saved workflow.
proj = fiarfly.Project.open("/data/study_a.fiarproj")
print(f"{proj.name}: {len(proj.runs)} runs, by {proj.author}")

# 2. Read a run's traces back as a DataFrame.
rows = proj.load_run_traces(proj.runs[0].id)
traces = pl.DataFrame(rows)

# 3. Reshape to [n_rois, n_frames] for window metrics.
delta_f = (
    traces
    .pivot(index="frame_idx", on="roi_id", values="delta_f_over_f")
    .drop("frame_idx")
    .to_numpy()
    .T.astype("float32")
)

# 4. Compute window metrics.
auc_stim1 = fiarfly.auc(delta_f, 10.0, 15.0, frame_rate=proj.frame_rate, seconds=True)
auc_stim2 = fiarfly.auc(delta_f, 25.0, 35.0, frame_rate=proj.frame_rate, seconds=True)

# 5. Run a paired test.
res = fiarfly.paired_t(auc_stim1.tolist(), auc_stim2.tolist())
print(res.test_name, res.p_value, res.effect_size)
```

---

## Module surface

```python
fiarfly.__version__

# Project bundles
fiarfly.Project              # class
fiarfly.RunMetadata          # value object

# Analysis metrics (operate on a 2-D float32 array [n_rois, n_frames])
fiarfly.window_mean(data, start, end, frame_rate=None, seconds=False) -> list[float]
fiarfly.window_peak(data, start, end, frame_rate=None, seconds=False) -> list[float]
fiarfly.window_value_at(data, frame: int)                              -> list[float]
fiarfly.auc            (data, start, end, frame_rate=None, seconds=False) -> list[float]
fiarfly.latency_to_peak(data, start, end, frame_rate=None, seconds=False) -> list[float]
fiarfly.rise_time      (data, start, end, frame_rate=None, seconds=False,
                        lo_frac=0.1, hi_frac=0.9)                       -> list[float]

# Statistics
fiarfly.TestResult           # value object
fiarfly.welch_t(a, b)        -> TestResult
fiarfly.paired_t(a, b)       -> TestResult
fiarfly.mann_whitney(a, b)   -> TestResult
fiarfly.wilcoxon(a, b)       -> TestResult
fiarfly.anova(groups)        -> TestResult
fiarfly.kruskal(groups)      -> TestResult
fiarfly.test_compare(groups, comparison, kind) -> TestResult

# Multiple-comparisons correction
fiarfly.bonferroni(p_values: list[float])         -> list[float]
fiarfly.benjamini_hochberg(p_values: list[float]) -> list[float]
```

---

## `Project`

A handle to a `.fiarproj` directory bundle. Construct via classmethods.

### `Project.open(path: str) -> Project`

Open an existing bundle. Reads `project.json`; lazy on the per-run parquets.

### `Project.create(path: str, name: str) -> Project`

Create a brand-new bundle. The path must not exist yet (`Project::create_new` refuses to overwrite). Records a `ProjectCreated` audit entry and writes an initial `project.json`.

### `Project.save() -> None`

Persist the current `project.json` (in-memory edits made via the GUI auto-save; in Python you must call this explicitly after mutating the project).

### Read-only properties

| Property | Type | Description |
|---|---|---|
| `name` | `str` | Project name |
| `version` | `str` | Schema version, e.g. `"0.2.0"` |
| `path` | `str` | Bundle directory path |
| `frame_rate` | `float \| None` | Recording frame rate in Hz, if set |
| `description` | `str \| None` | Free-form description |
| `author` | `str \| None` | Auto-detected `$USER@$HOSTNAME` at create time |
| `created_at` | `int` | Unix seconds |
| `modified_at` | `int` | Unix seconds |
| `source_tiff_path` | `str \| None` | Original recording path |
| `runs` | `list[RunMetadata]` | One per saved run |

### `Project.audit_log() -> list[dict]`

Append-only traceability log. Each entry is a dict with keys:

| Key | Type | Notes |
|---|---|---|
| `timestamp` | `int` | Unix seconds |
| `action` | `str` | `"ProjectCreated"`, `"ProjectOpened"`, `"RunAdded"`, `"RoiSetUpdated"`, `"FrameLabelsEdited"`, `"SourceTiffSet"`, `"Note"` |
| `description` | `str` | Human-readable line |
| `run_id` | `str \| None` | Reference to a run, when applicable |

### `Project.load_run_traces(run_id: str) -> list[dict]`

Read the run's `traces.parquet`. Returns one dict per (ROI × frame) row with keys:

```
roi_id, roi_label, roi_group, session_id,
frame_idx, time_s,                           # time_s only when frame_rate set
raw_f, delta_f_over_f,
neuropil_f,                                   # if neuropil correction was applied
deconvolved                                   # if OASIS was run
```

Pipe into your DataFrame library of choice:

```python
import polars as pl
df = pl.DataFrame(proj.load_run_traces(run_id))

# or pandas
import pandas as pd
df = pd.DataFrame(proj.load_run_traces(run_id))
```

### `Project.load_run_events(run_id: str) -> list[dict] | None`

Returns `None` if the run did not include event detection. Otherwise a list of dict rows with columns:

```
roi_idx, event_idx, onset_frame, peak_frame, amplitude,
offset_frame, duration_frames, half_decay_frame
```

### `Project.load_run_quality(run_id: str) -> list[dict] | None`

`None` if quality scoring was not run. Otherwise a list of dict rows:

```
roi_id, snr, skewness, active_fraction, peak_dff, noise_std, passes
```

---

## `RunMetadata`

Read-only value object for a single run.

| Field | Type | Description |
|---|---|---|
| `id` | `str` | Run id, `run_<unix_ms>_<slug>` — chronologically sortable |
| `name` | `str` | Human-readable run name |
| `created_at` | `int` | Unix seconds |
| `n_rois` | `int` | Number of ROIs in this run |
| `n_frames` | `int` | Frames extracted |
| `has_neuropil` | `bool` | Whether `neuropil_f` is present |
| `has_spikes` | `bool` | Whether OASIS deconvolution was run |
| `has_events` | `bool` | Whether `events.parquet` was written |
| `has_quality` | `bool` | Whether `quality.parquet` was written |
| `notes` | `str \| None` | Free-form notes |
| `fiarfly_version` | `str \| None` | Tool version that produced the run |
| `source_tiff_path` | `str \| None` | TIFF used at extraction time |
| `used_motion_corrected` | `bool` | Traces came from corrected stack |
| `neuropil_correction_applied` | `bool` | Whether neuropil correction was on |
| `frame_rate` | `float \| None` | Recording frame rate at extraction time |

---

## Analysis metrics

All metrics share the same signature and operate on a 2-D float32 numpy array shaped `[n_rois, n_frames]`. Each returns a list of `n_rois` floats.

### Common arguments

| Argument | Type | Notes |
|---|---|---|
| `data` | `numpy.ndarray[float32]`, shape `(n_rois, n_frames)` | The trace matrix |
| `start`, `end` | `float` | Window bounds (frames or seconds, see `seconds=`) |
| `frame_rate` | `float \| None` | Required when `seconds=True`; controls time-axis units in `auc`, `latency_to_peak`, `rise_time` |
| `seconds` | `bool` (default `False`) | When `True`, `start`/`end` are seconds |

### `window_mean`

Mean of the trace over `[start, end)`.

```python
fiarfly.window_mean(data, 100, 200)                                # frames 100..200
fiarfly.window_mean(data, 10.0, 15.0, frame_rate=30, seconds=True)  # 10–15 s @ 30 Hz
```

### `window_peak`

Maximum of the trace over the window.

### `window_value_at(data, frame)`

Single-sample value at a fixed frame index (no window).

### `auc(data, start, end, ...)`

Trapezoidal AUC. Units:
* with `frame_rate`: ΔF/F · seconds
* without: ΔF/F · frames

### `latency_to_peak(data, start, end, ...)`

Time (frames or seconds) from window start to the per-ROI peak inside the window.

### `rise_time(data, start, end, ..., lo_frac=0.1, hi_frac=0.9)`

Time taken for the trace to rise from `lo_frac × peak` to `hi_frac × peak` within the window. Returns `0.0` when the rise cannot be resolved (peak at frame 0, monotonic decrease, etc.).

---

## Statistics

All tests return a `TestResult`.

### `TestResult`

| Field | Type | Notes |
|---|---|---|
| `test_name` | `str` | `"paired t"`, `"Welch t"`, `"Wilcoxon signed-rank"`, `"Mann-Whitney U"`, `"one-way ANOVA"`, `"Kruskal-Wallis H"` |
| `statistic` | `float` | t / U / W / F / H |
| `p_value` | `float` | two-sided |
| `n` | `list[int]` | sample sizes; length 1 for paired |
| `effect_size` | `float \| None` | test-appropriate; `None` only for malformed inputs |
| `effect_kind` | `str \| None` | `"Cohen's d"`, `"Cohen's dz"`, `"η²"`, `"η²H"`, `"rank-biserial"` |
| `note` | `str \| None` | populated for edge cases (zero-variance, etc.) |

### Specific tests

```python
res = fiarfly.paired_t(a, b)        # equal-length lists; uses dz effect size
res = fiarfly.welch_t(a, b)         # unequal variance / sizes; Cohen's d
res = fiarfly.wilcoxon(a, b)        # paired non-parametric (signed-rank)
res = fiarfly.mann_whitney(a, b)    # unpaired non-parametric
res = fiarfly.anova([g1, g2, g3])   # one-way; η²
res = fiarfly.kruskal([g1, g2, g3]) # rank-based ANOVA analog; η²H
```

### High-level dispatcher

`test_compare(groups, comparison, kind)` lets the runtime pick the test from the same `(comparison, kind)` axes the GUI exposes:

```python
res = fiarfly.test_compare(
    [a, b, c],
    comparison="multi",          # "paired" | "unpaired" | "multi"
    kind="auto",                 # "auto" | "parametric" | "nonparametric"
)
```

Validation rules match the GUI: `paired` requires two equal-length samples; `unpaired` requires two; `multi` requires ≥ 3.

### Multiple-comparisons correction

```python
fiarfly.bonferroni([0.01, 0.04, 0.5])              # → scaled, capped at 1
fiarfly.benjamini_hochberg([0.01, 0.04, 0.03, 0.005])
```

The Benjamini-Hochberg implementation matches R's `p.adjust(method="BH")` and statsmodels' `multipletests`. Both functions preserve input order and clamp at 1.

---

## Worked end-to-end example

Compute, visualise, and test "ΔF/F window mean at 10–15 s vs 25–35 s in the same cells" from a saved project, in pure Python:

```python
import fiarfly
import numpy as np
import polars as pl
import matplotlib.pyplot as plt

proj = fiarfly.Project.open("/data/study_a.fiarproj")
fr = proj.frame_rate or 30.0

# Pick the run we want.
run = proj.runs[0]
df = pl.DataFrame(proj.load_run_traces(run.id))

# Reshape to [n_rois, n_frames] float32 in ROI order.
delta_f = (
    df.pivot(index="frame_idx", on="roi_id", values="delta_f_over_f")
      .drop("frame_idx")
      .to_numpy()
      .T.astype("float32")
)

# Window means.
m1 = fiarfly.window_mean(delta_f, 10.0, 15.0, frame_rate=fr, seconds=True)
m2 = fiarfly.window_mean(delta_f, 25.0, 35.0, frame_rate=fr, seconds=True)

# Test.
res = fiarfly.paired_t(m1, m2)
print(f"{res.test_name}: t={res.statistic:.3f}, p={res.p_value:.4g}, "
      f"effect={res.effect_size:.2f} ({res.effect_kind})")

# Plot ROI-by-ROI.
fig, ax = plt.subplots()
ax.plot(["10–15 s", "25–35 s"], np.array([m1, m2]), "-o", alpha=0.6)
ax.set_ylabel("ΔF/F window mean")
ax.set_title(f"{proj.name} / {run.name}")
plt.show()
```

---

## Extending the package

If you want to expose more of `fiarfly-core` to Python:

1. Add the wrapper in `crates/fiarfly-py/src/lib.rs` using PyO3.
2. Re-export from `python/fiarfly/__init__.py`.
3. Run `uv run maturin develop` (or `./dev.sh build`) and add a Python test under `python/tests/`.
4. The `[tool.maturin]` config in `pyproject.toml` already points at `crates/fiarfly-py/Cargo.toml` and uses `python/` as the package source.

---

## See also

* [USER_GUIDE.md](../USER_GUIDE.md) — end-user manual covering every panel.
* [SETUP.md](../SETUP.md) — installation and run instructions.
* [ARCHITECTURE.md](../ARCHITECTURE.md) — internal architecture.
* [docs/PIPELINE_REFERENCE.md](PIPELINE_REFERENCE.md) — calcium imaging algorithm reference.
