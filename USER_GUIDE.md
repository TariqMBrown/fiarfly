# FIARfly — User Guide (v0.2)

A panel-by-panel manual for the desktop application, plus the Python API and worked examples.

For installation see [SETUP.md](SETUP.md). For algorithm and science reference see [docs/PIPELINE_REFERENCE.md](docs/PIPELINE_REFERENCE.md).

---

## Table of contents

1. [Big picture](#big-picture)
2. [Launching the app](#launching-the-app)
3. [The interface at a glance](#the-interface-at-a-glance)
4. [Panel 1 — Import](#panel-1--import)
5. [Panel 2 — Motion Correction](#panel-2--motion-correction)
6. [Panel 3 — ROI Editor](#panel-3--roi-editor)
7. [Panel 4 — Signal Viewer](#panel-4--signal-viewer)
8. [Panel 5 — Analysis](#panel-5--analysis)
9. [Panel 6 — Statistics](#panel-6--statistics)
10. [Panel 7 — Export](#panel-7--export)
11. [The Project tab — `.fiarproj` bundles](#the-project-tab--fiarproj-bundles)
12. [Frame labels (epochs)](#frame-labels-epochs)
13. [Worked examples](#worked-examples)
14. [Python API](#python-api)
15. [Files FIARfly creates](#files-fiarfly-creates)
16. [Keyboard, mouse, and gestures](#keyboard-mouse-and-gestures)
17. [Troubleshooting](#troubleshooting)

---

## Big picture

FIARfly takes you from a calcium-imaging TIFF stack to publication-ready summary statistics in one session, without scripting:

```
TIFF  →  motion correction  →  ROI drawing  →  ΔF/F traces  →  custom analysis  →  statistics  →  export
```

Anything you do can be saved into a **`.fiarproj` project bundle** — a directory holding ROIs, extracted traces, parameters, and an append-only **traceability log** of every change. You can re-open the bundle later, add new runs (e.g. re-extract with different parameters, or analyse a follow-up condition), and visually compare them — all without re-loading the TIFF.

---

## Launching the app

```bash
cd /path/to/fiarfly
cargo run --release -p fiarfly-gui
```

The first launch builds everything; expect a few minutes. Subsequent launches are nearly instant.

---

## The interface at a glance

```
┌─────────────────────────────────────────────────────────────────────┐
│  File   View   Help                                                  │  ← menu bar
├──────────────┬──────────────────────────────────────────────────────┤
│   Project    │                                                       │
│ 1. Import    │                                                       │
│ 2. Motion    │                                                       │
│ 3. ROI       │              ACTIVE PANEL CONTENT                    │
│ 4. Signal    │                                                       │
│ 5. Analysis  │                                                       │
│ 6. Stats     │                                                       │
│ 7. Export    │                                                       │
│              │                                                       │
│  Log         │                                                       │
│  …           │                                                       │
├──────────────┴──────────────────────────────────────────────────────┤
│  recording.tif · 1000 frames × 512×512   Mem: 2.1 GB · "Loaded …"   │  ← status bar
└─────────────────────────────────────────────────────────────────────┘
```

* **Left sidebar** — the pipeline stepper. Click any step to jump to that panel; the active panel is highlighted. Below the stepper is a **rolling log** showing recent actions and warnings.
* **Right side** — the active panel.
* **Status bar (bottom)** — current source file, dimensions, in-flight worker progress, the most recent log message, and live process memory usage.
* **Menu bar (top)** — File, View, Help. Most actions are also reachable from buttons inside the panels themselves.

A floating **frame-label overlay** appears in the corner whenever you have at least one frame label defined and the current frame falls inside one of them. It's draggable and can be toggled from `View → Frame label overlay`.

---

## Panel 1 — Import

This is where every workflow starts.

### Open a TIFF
* Click **Open TIFF…** (or `File → Open TIFF…`). A native file dialog appears; select a `.tif` / `.tiff` stack.
* The Stack info line shows `<frames> × <height> × <width> px` once loaded.
* The **Resource Estimates** collapsing section shows the in-memory cost of the stack and the projected peak during motion correction. If that peak exceeds ~80% of system RAM you'll see a red warning.

### Set modality
Pick the modality that matches your recording. This sets sensible defaults further down the pipeline (neuropil correction, baseline percentile, rolling window):

| Modality | Use for |
|---|---|
| Two-Photon In Vivo | Cortex / hippocampus 2P imaging in live animal |
| Two-Photon In Vitro | 2P in slice |
| One-Photon In Vivo | Miniscope (GRIN-lens) recordings |
| One-Photon Wide-field | Wide-field epifluorescence |

### Set frame rate (recommended)
Enter the recording frame rate in Hz. This:
* Enables a `time_s` column in exports.
* Lets the Analysis and Stats panels work in seconds instead of frames.
* Is required to estimate OASIS decay (`g`) automatically.

### Frame preview & playback
* The **Frame slider** and `▶/⏸` button scrub or play through the stack at 0.25× / 0.5× / 1× / 2× / 4× speed. Speed is relative to your set frame rate (or 30 Hz if not set).
* Image contrast is per-frame min/max — fine for navigation, not for measurement.

### Frame Labels (epochs)
Open the **Frame Labels** collapsing section to define named frame ranges (e.g. *Baseline 0–124*, *Stim 1 125–744*). These labels:
* Light up the floating overlay during playback.
* Are exported with the project.
* Auto-populate the **Window picker** in the Analysis panel — one click per epoch.

### Continue
Click **Continue to Motion →** to proceed.

---

## Panel 2 — Motion Correction

Brain motion makes ROIs drift across cells. Run motion correction before drawing ROIs.

### Algorithm
* **Rigid** — a single (dy, dx) shift per frame. Fast, robust for small motion. Default for 2P.
* **Non-rigid** — patch-by-patch shifts (NoRMCorre-style). Better for warping / breathing artifacts but uses more RAM.

### Parameters
| Parameter | Meaning | Typical |
|---|---|---|
| Max shift (px) | Largest allowed translation per frame | 10 |
| Template bin (frames) | Frames averaged to refine the alignment template | 50 |
| Grid size (px) | Patch size, non-rigid only | 32 × 32 |
| Overlap (px) | Patch overlap, non-rigid only | 8 |

### 1-photon spatial filter
Toggle **Apply spatial high-pass before MC** for 1P data — strips out the spatially varying background that otherwise dominates the alignment cost. Filter sigma is exposed in the same row.

### Run
Click **Run Motion Correction**. A worker thread does the work; the progress bar in the status bar shows ETA. The corrected stack replaces the raw stack in memory.

The **Quality plot** at the bottom shows the per-frame Pearson correlation between the corrected frame and the alignment template. Any frames below 0.90 are flagged in the log — inspect them visually before trusting the alignment.

### Batch mode
The **Batch motion correction** section lets you queue multiple TIFFs and a single output directory. Each result is written to disk as it completes, freeing RAM for the next file. Useful for processing a day's recordings overnight.

---

## Panel 3 — ROI Editor

Where you tell FIARfly *what* you're measuring.

### Canvas
The big left-hand area is the image canvas with your stack drawn on top of a dark background. Above it:

* **Frame slider** — scrub the underlying image.
* **Show projection** toggle — show the mean projection (computed in the background) instead of a single frame; cells stand out far more clearly.
* **LUT Min / LUT Max sliders** — clip the display range. The two sliders are coupled so Min can't exceed Max. Click ⟳ to reset.
* **Zoom buttons + scroll-wheel zoom** — scroll on the canvas to zoom around the cursor; click `−` / `+` for fixed steps. **Rotate** lets you rotate the canvas (handy for rotated FOVs); the ↻ button rotates by +90°.

### Drawing ROIs
* Pick the **Draw** tool, then left-click on the canvas to drop polygon vertices.
* Double-click — or click near the first vertex — to **close** the polygon.
* Press `Esc` to cancel an in-progress shape.
* Right-click removes the most recent vertex while drawing.

### Stamping repeated shapes (circles / squares)
For images with many similar features (e.g. roughly circular cell bodies), use a stamp tool instead of redrawing each one:

* **● Circle** — sets a fixed radius (in image pixels) plus a vertex count (6–64). Click the centre of each cell and a circle ROI is added instantly.
* **■ Square** — sets a fixed half-side (in image pixels). Click the centre and a square ROI is added.

A yellow live preview at the cursor shows exactly what will be stamped. After stamping, switch to **Select** and drag any vertex handle to reshape — circles can be deformed into ellipses, squares into rectangles or trapezoids — without leaving the editor.

The stamp size is a single global value, so once you've dialled it in for one cell you can rapid-fire-click the rest.

### Editing
* **Select** tool — click a polygon to highlight it. Once selected, every vertex shows as a draggable handle: hover over a handle (it turns yellow), then click-and-drag to move that vertex. Use this to refine stamped circles/squares or to nudge hand-drawn ROIs.
* **Pan** tool — left-drag to pan; middle/right-drag pans in any mode.
* **Delete** tool — click a polygon to remove it.
* The right-hand sidebar shows the ROI list. Each row has:
  * The ROI's color
  * **Label** — rename inline
  * **Group** — tag the ROI for later analysis (e.g. `layer_2/3`, `layer_5`, `responder`, `non-responder`). **This is what powers the "Aggregate by ROI group" feature in Analysis and the across-groups comparison in Stats.**
  * Eye toggle — show/hide individual ROIs
  * `✕` — delete

### Saving / loading ROI sets
* **Save ROI Set…** — writes a `.rois.json` file with all polygons, labels, groups, and colors.
* **Load ROI Set…** — replaces the current set.
* **Import ROI Set…** in `File →` menu appends instead.

> **Tip:** Save your ROI set as soon as you've drawn them. Even if you save the whole project later, having a stand-alone `.rois.json` is the safe checkpoint.

### Continue
**Continue to Signal →** when ready.

---

## Panel 4 — Signal Viewer

Extracts the per-ROI fluorescence trace and shows the result.

### Extract Traces
Click the button. Background workers compute, in order:

1. Per-ROI raw fluorescence over time (mean of the pixels inside each polygon).
2. Per-ROI **neuropil** fluorescence (annular ring around each polygon, excluding other polygons). For 2P only by default.
3. Neuropil-corrected `F` (`F_soma − r * F_neuropil`).
4. **ΔF/F** using a rolling-percentile baseline.

Status switches to `✓  N ROIs × N frames` when done.

### Display options
* **Raw F** vs **ΔF/F** radio.
* **Fixed baseline window** — when checked, F0 is the *mean* fluorescence over a frame range you specify. Use this when you have a clean baseline epoch (e.g. pre-stimulus) and want all transients normalised to it. Otherwise the rolling-percentile baseline (configurable in the Signal Parameters section) is used.
* **ROI checkboxes** — show / hide individual traces; **All / None** clear or enable everything.

### Plot
* Multi-line plot, one color per ROI matching the ROI Editor canvas colors.
* Zoom and pan with scroll wheel and click-drag.
* Hover lines for cursor coordinates.

### OASIS deconvolution (optional)
**Run OASIS** infers spike trains from ΔF/F via the constrained AR(1) model from Friedrich 2017. Parameters:
* `g` — calcium decay constant (auto-estimated from each trace's autocorrelation if left blank).
* `sn` — noise std (auto via MAD).
* `lambda` — sparsity penalty (0 = unconstrained).

Adds a `deconvolved` trace overlay and unlocks the `deconvolved` column in exports.

### Event detection + quality
**Run Analysis** detects calcium transients and computes per-ROI quality metrics:
* `snr`, `skewness`, `active_fraction`, `peak_dff`, `noise_std`, `passes`.
The pass / fail flag uses the SNR threshold under Quality settings.

### Continue
**Continue to Export →** for the classic linear flow, or jump to **Analysis** via the stepper.

---

## Panel 5 — Analysis

Custom visualisations and per-ROI metrics. Inputs come from the Signal Viewer (or any open project run).

### What it computes
Six metrics, each producing one value per ROI:

| Metric | Definition |
|---|---|
| **Window mean** | Mean of the trace over the window. |
| **Window peak** | Max of the trace over the window. |
| **Value at frame** | Trace value at the window's start frame (single-sample). |
| **AUC (trapezoidal)** | Area under the curve. Units: ΔF/F·s if frame_rate set, else ΔF/F·frames. |
| **Latency to peak** | Time from window start to per-ROI peak (s or frames). |
| **Rise time (10–90%)** | Time to rise from 10% to 90% of the within-window peak. |

### Setting up windows
* Toggle **Window units** (frames vs seconds). Seconds requires a frame rate to be set in Import.
* Click **+ Add window** to add an entry, then drag the start/end values; rename the window.
* If you defined frame labels in Import, you'll see one **shortcut button per label** (`Add from label: Baseline | Stim 1 | …`) — one click adds a window pre-populated to that epoch's range, auto-converted between frames and seconds.

### Source / metric / plot
Top-row selectors:
* **Source** — ΔF/F or Raw F.
* **Metric** — one of the six above.
* **Plot** — Line or Bar.
* **Aggregate by ROI group** — when checked, bars are grouped by the `group` field on each ROI (set in the ROI Editor) instead of one bar per ROI.

### Compute
Click **Compute**. Results are cached on the panel as a **summary table** (rows = ROIs, columns = windows). It also drives the **Stats** panel — there is no need to re-run anything in Stats once you've computed here.

### Plot views
* **Line plot** — full traces over the recording, vertical white guides at every window boundary, auto-coloured by ROI. Legend uses your ROI labels.
* **Bar plot** — one cluster per group (per ROI by default; per ROI-group when "Aggregate by ROI group" is on), one bar per window inside each cluster. Color encodes the window. Below the plot, the **Summary table** collapsing section shows the underlying mean ± n.

### Continue
**→ Statistics** routes to the Stats panel.

---

## Panel 6 — Statistics

Hypothesis testing on the Analysis panel's summary. **Run Compute in Analysis first.**

### Selectors

| Control | Options |
|---|---|
| **Compare** | *across windows (paired)* — same ROIs at two windows; *across ROI groups (unpaired)* — different ROI groups at one window. |
| **Test family** | *Auto*, *Parametric* (t / Welch / ANOVA), *Non-parametric* (Wilcoxon / Mann-Whitney / Kruskal-Wallis). |
| **Multiple comparisons** | *None*, *Bonferroni*, *Benjamini-Hochberg (FDR)*. |

### Run
Click **Run tests**. The panel computes:

1. **Omnibus test** when ≥ 3 samples are involved (ANOVA or Kruskal-Wallis), prepended as a single row labelled `(omnibus)`.
2. **All unique pairwise tests** between the relevant samples.

If you picked Paired but two compared windows have unequal lengths, FIARfly silently downgrades to Unpaired (and labels the test accordingly) so you still get a row.

### Results table
Columns:
* **A**, **B** — names of the compared windows (or groups).
* **n** — sample sizes (one number for paired, two for unpaired).
* **test** — concrete test name (`paired t`, `Welch t`, `Wilcoxon signed-rank`, `Mann-Whitney U`, `one-way ANOVA`, `Kruskal-Wallis H`).
* **stat** — test statistic.
* **mean A**, **mean B** — group means for context.
* **p** — raw p-value, scientific notation below 1e-4.
* **p_adj** — p-value after multiple-comparisons correction. Color-coded:
  * **green** p < 0.001
  * **yellow-green** p < 0.01
  * **yellow** p < 0.05
  * **grey** p ≥ 0.05
* **effect** — effect size + kind (`Cohen's d`, `dz`, `η²`, `rank-biserial`).

The omnibus row is excluded from the multiple-comparisons family — only pairwise rows are adjusted.

### Forest plot
One row per pairwise comparison; each row plots the effect-size estimate with reference whiskers at ±0.5 standardised effect units. Color encodes magnitude (Cohen-d-style: 0.2 / 0.5 / 0.8). A vertical zero line is drawn for reference. Test-family-specific 95% CIs are a follow-up.

### Reading a typical comparison

> *"Did ΔF/F differ between 10–15 s (Stim 1) and 25–35 s (Stim 2) in the same cells?"*

1. In Analysis: pick **Window mean** of **ΔF/F**, add the two windows from frame labels, click **Compute**.
2. In Statistics: pick **across windows (paired)** + **Auto** + **Bonferroni**, click **Run tests**.
3. Read the row: `Stim 1 vs Stim 2 — paired t — t=2.3 — p=0.018 — p_adj=0.018 — 0.92 (Cohen's dz)`.
4. The forest plot puts a yellow whisker just past the zero line.

> *"Did layer 2/3 differ from layer 5 during Stim 1?"*

1. In ROI Editor, group your cells (`L2/3`, `L5`).
2. In Analysis: same metric + Stim 1 window, click **Compute**.
3. In Statistics: pick **across ROI groups (unpaired)**, click **Run tests**.
4. Result row: `L2/3 vs L5 — Welch t — …`.

---

## Panel 7 — Export

Writes results to disk in tidy long format.

### Where
* Pick an **output directory** (Browse… → folder picker).
* The **Filename stem** defaults to the source TIFF's stem; edit if you like.
* Outputs are `<stem>.parquet` and `<stem>.csv`. Parquet is preferred (typed, compressed, fast); CSV is for compatibility.

### Schema
One row per (ROI × frame). Columns:

```
roi_id, roi_label, roi_group, session_id,
frame_idx, time_s,                           # time_s only when frame_rate is set
raw_f, delta_f_over_f,
neuropil_f,                                   # if neuropil correction was applied
deconvolved                                   # if OASIS was run
```

### Project save
**Save session (.fiarfly)…** is a legacy shortcut; the recommended persistence path in v0.2 is to **save the entire workflow into a `.fiarproj` bundle** from the Project tab — see below.

---

## The Project tab — `.fiarproj` bundles

A **project** captures everything except the TIFF: ROIs, traces, frame labels, parameters, audit trail. Use it to:

* Resume work on a recording weeks later, with no need to re-decode the source TIFF.
* Store multiple **runs** (re-extractions) of the same recording with different parameters.
* Compare conditions visually within one project.
* Archive results in a single browsable directory.

### File format
A `.fiarproj` is a directory:

```
mystudy.fiarproj/
├── project.json              metadata, params, ROI polygons, frame labels, run index, audit log
└── runs/
    ├── run_<unix_ms>_<slug>/
    │   ├── run.json           per-run metadata + params snapshot + inputs snapshot
    │   ├── traces.parquet     tidy raw_f / delta_f / neuropil_f / deconvolved
    │   ├── events.parquet     optional, when event detection was run
    │   └── quality.parquet    optional, when quality scoring was run
    └── …
```

* `project.json` is human-readable JSON. Diff-friendly; safe to commit to git.
* `traces.parquet` is the same long-format table as the Export panel writes — readable from Python, R, or Polars without any FIARfly dependency.
* No TIFF is stored. The original path is remembered so the GUI can offer to re-load it.

### Creating a project
1. With a workflow set up (TIFF loaded, ROIs drawn, traces extracted), open the **Project** tab.
2. Click **New Project…**, choose a save location and filename (extension `.fiarproj` is added automatically).
3. The current AppState is seeded into the project (frame rate, modality, ROI set, frame labels, source TIFF path).
4. The **traceability log** receives a `CREATE` entry.

### Adding a run
1. With traces extracted, click **Add current as run**.
2. Enter a run name (e.g. `baseline-rolling-pct8` or `stim1-fixed-baseline`) in the *Next run name* field; if blank, FIARfly assigns `run_<N>`.
3. The run directory is created; `traces.parquet` is written via the same `to_dataframe` path the Export panel uses; `events.parquet` and `quality.parquet` are written if those analyses were performed.
4. The audit log gains a `RUN` entry referencing the run id.

### Opening a project
1. **File → Open Project…** or the Project tab's **Open Project…** button.
2. The project's metadata is poured back into the live AppState (frame rate, modality, ROI set, source TIFF path, frame labels).
3. The audit log gains an `OPEN` entry.
4. From here you can extract new traces (e.g. with different parameters) and **Add current as run** to capture the comparison.

### What you see in the panel

```
Project
├── Metadata
│   name · path · author · created · modified · version · source TIFF
│   frame rate · image · modality · run count · description (editable)
├── Runs (N)
│   one card per run: name · ROIs × frames · created at · run id · tags
│   (neuropil / deconvolved / events / quality) · source TIFF · notes ·
│   fiarfly version
└── Traceability log (M)
    reverse-chronological audit trail with color-coded action tags:
    CREATE  OPEN  RUN  ROI  LABELS  SRC  NOTE
```

### The traceability / audit log
Every project-level event is recorded with:
* **timestamp** (UTC, displayed as `YYYY-MM-DD HH:MM:SS`)
* **action** — one of `ProjectCreated`, `ProjectOpened`, `RunAdded`, `RoiSetUpdated`, `FrameLabelsEdited`, `SourceTiffSet`, `Note`
* **description** — human-readable string
* **run_id** — when the entry refers to a specific run

The log is append-only on disk; deleting an entry would require manually editing `project.json`. This is deliberate — the log is your evidence trail when you need to know how a trace was generated.

---

## Frame labels (epochs)

Frame labels are named frame ranges that flow through the whole app:

* **Defined** in the Import panel (Frame Labels collapsing section).
* **Visible** as a draggable corner overlay during playback.
* **Saved** in `project.json`.
* **Reusable** as one-click window presets in the Analysis panel.

Use them to encode your experimental structure (`Baseline 0–124`, `Stim 1 125–744`, `Inter-stim 745–874`, …) once, then re-use everywhere.

---

## Worked examples

### A. *"Compare ΔF/F at 10–15 s vs 25–35 s in the same cells"*

1. **Import** TIFF, set frame rate `30 Hz`, add labels `Stim1: 300–449`, `Stim2: 750–1049`.
2. **Motion** — rigid, defaults.
3. **ROI Editor** — draw ~10 cells, set group `responders` on the ones that visibly transient.
4. **Signal Viewer** — Extract Traces, ΔF/F + neuropil correction.
5. **Analysis** — Source `ΔF/F`, Metric `Window mean`, Plot `Bar`, *units: seconds*. Click `Add from label: Stim1`, then `Stim2`. Click **Compute**.
6. **Statistics** — *across windows (paired)*, *Auto*, *Bonferroni*. Click **Run tests**.
7. Read row `Stim1 vs Stim2`. Save the workflow into a `.fiarproj` from the Project tab.

### B. *"Compare AUC during Stim 1 between layer 2/3 and layer 5"*

1. Same Import / Motion / Signal as above.
2. In **ROI Editor**, set group `L2/3` on superficial cells and `L5` on deep ones.
3. **Analysis** — Metric `AUC`, single window from `Stim1` label, plot `Bar`, **Aggregate by ROI group** ON, click **Compute**. The bar plot now has two bars (`L2/3`, `L5`).
4. **Statistics** — *across ROI groups (unpaired)*, click **Run tests** → Welch t and effect size.

### C. *"Re-extract the same recording with a different ΔF/F baseline window and compare"*

1. Open the existing `.fiarproj`; the ROIs and labels return automatically.
2. **Signal Viewer** — switch to Fixed baseline window, set `0–124` (your `Baseline` epoch), Re-extract Traces.
3. **Project tab** — type `fixed-baseline-0-124` into *Next run name*, click **Add current as run**.
4. The project now has two runs (the original + the new one). The audit log records both.

### D. *"Open three projects, see how cells responded across animals"*

> Cross-project comparison is on the v0.2 stretch list (PR scope is single-project). Today, open one project at a time and export `traces.parquet` per project; merge and compare in pandas/R/Polars.

---

## Python API

After `uv run maturin develop`, the `fiarfly` package exposes the same v0.2 surface to Python:

### Project bundles

```python
import fiarfly

proj = fiarfly.Project.open("/data/study_a.fiarproj")
print(proj.name, proj.author, proj.frame_rate)
print([r.name for r in proj.runs])

# Traceability log
for entry in proj.audit_log():
    print(entry["timestamp"], entry["action"], entry["description"])

# Read a run's traces back. Returns list[dict] for portability; pipe to your DF lib.
rows = proj.load_run_traces(proj.runs[0].id)

import polars as pl
df = pl.DataFrame(rows)
print(df.head())

# Or with pandas:
# import pandas as pd; df = pd.DataFrame(rows)

# Create a new project programmatically
new = fiarfly.Project.create("/data/auto_study.fiarproj", name="auto_study")
new.save()
```

### Analysis metrics

All take a 2-D float32 numpy array `[n_rois, n_frames]`. With `seconds=True` you give `start` / `end` in seconds; the metric returns time-axis values in seconds when `frame_rate` is set.

```python
import numpy as np
import fiarfly

# Synthetic delta_f: 5 ROIs × 60 s @ 30 Hz
fs = 30.0
delta_f = np.zeros((5, int(60 * fs)), dtype="float32")
delta_f[0, 300:450] = 1.0     # ROI 0 active during 10–15 s
delta_f[1, 750:1050] = 0.7    # ROI 1 active during 25–35 s

mean_stim1 = fiarfly.window_mean(delta_f, 10.0, 15.0, frame_rate=fs, seconds=True)
mean_stim2 = fiarfly.window_mean(delta_f, 25.0, 35.0, frame_rate=fs, seconds=True)
auc_stim1  = fiarfly.auc      (delta_f, 10.0, 15.0, frame_rate=fs, seconds=True)
peak_stim1 = fiarfly.window_peak(delta_f, 10.0, 15.0, frame_rate=fs, seconds=True)
lat_stim1  = fiarfly.latency_to_peak(delta_f, 10.0, 15.0, frame_rate=fs, seconds=True)
rt_stim1   = fiarfly.rise_time(delta_f, 10.0, 15.0, frame_rate=fs, seconds=True)
```

### Statistics

```python
import fiarfly

a = [10.0, 11.0, 12.0, 9.0, 13.0]
b = [8.0, 9.5, 10.5, 7.0, 11.0]

# Specific tests
res = fiarfly.welch_t(a, b)         # → TestResult
res = fiarfly.paired_t(a, b)
res = fiarfly.mann_whitney(a, b)
res = fiarfly.wilcoxon(a, b)
res = fiarfly.anova([a, b, [12.0, 13.0, 14.0, 11.0, 15.0]])
res = fiarfly.kruskal([a, b, [12.0, 13.0, 14.0, 11.0, 15.0]])

# High-level dispatcher matching the GUI's Auto / Parametric / Non-parametric
res = fiarfly.test_compare(
    [a, b],
    comparison="paired",       # "paired" | "unpaired" | "multi"
    kind="auto",               # "auto" | "parametric" | "nonparametric"
)

print(res.test_name, res.statistic, res.p_value, res.effect_size, res.effect_kind)

# Multiple-comparisons correction
fiarfly.bonferroni([0.01, 0.04, 0.5])
fiarfly.benjamini_hochberg([0.01, 0.04, 0.03, 0.005])
```

### `TestResult` fields

| Field | Type | Notes |
|---|---|---|
| `test_name` | str | concrete test name (`Welch t`, `Mann-Whitney U`, …) |
| `statistic` | float | t / U / W / F / H |
| `p_value` | float | two-sided |
| `n` | list[int] | per-group sample sizes (length 1 for paired) |
| `effect_size` | float \| None | test-appropriate point estimate |
| `effect_kind` | str \| None | `Cohen's d` / `Cohen's dz` / `η²` / `rank-biserial` / `η²H` |
| `note` | str \| None | optional diagnostic (e.g. zero-variance fallback) |

---

## Files FIARfly creates

| Extension | Where | Format | Contains |
|---|---|---|---|
| `.tif` / `.tiff` | input | TIFF | source recording (read-only) |
| `.rois.json` | export from ROI Editor | JSON | polygon vertices, labels, groups, colors |
| `.parquet` | Export panel; per-run inside `.fiarproj` | Apache Parquet | tidy long-format traces |
| `.csv` | Export panel | CSV | same as `.parquet`, for compatibility |
| `.fiarfly` | legacy session save | JSON | superseded by `.fiarproj` |
| `.fiarproj/` | Project tab | directory bundle | `project.json` + `runs/<id>/...parquet` + audit log |

---

## Keyboard, mouse, and gestures

### Global
* `Ctrl/Cmd+O` — Open TIFF…
* `Ctrl/Cmd+Q` — Quit

### ROI canvas (Panel 3)
* Scroll wheel — zoom around the cursor
* Middle-drag / right-drag — pan in any mode
* In **Pan** mode, left-drag pans
* In **Select** mode, left-drag on a vertex handle reshapes that vertex
* In **Stamp** modes (Circle / Square), left-click stamps a shape at the cursor
* `Esc` — cancel in-progress polygon
* Double-click (Draw mode) — close current polygon
* Right-click while drawing — remove the last vertex placed

### Plots (everywhere)
* Scroll — zoom
* Right-click + drag — pan
* Double-click — auto-fit

---

## Troubleshooting

### The app crashes when I click "Open TIFF…"
Fixed in v0.2 (PR1). Make sure you've rebuilt after pulling: `cargo run --release -p fiarfly-gui`. The previous `+[NSOpenPanel openPanel]` NULL-return bug on macOS was resolved by deferring native dialogs to outside the egui paint pass.

### "min > max" panic in the ROI Editor
Also fixed in v0.2 (PR1). The LUT Min and Max sliders are now coupled so they cannot cross.

### "No analysis summary available" in the Stats panel
Open the Analysis panel first, define ≥ 1 window, click **Compute**. The Stats panel then reads its inputs from that summary. Click **→ Go to Analysis** to navigate.

### "AcrossGroups needs ≥ 2 ROI groups"
Tag your ROIs with the **Group** field in the ROI Editor (e.g. `L2/3`, `L5`). The Stats panel partitions ROIs by that field when comparing groups.

### "Project create error: path already exists"
`Project::create_new` refuses to overwrite. Either pick a different filename or delete the existing `.fiarproj` directory first.

### A project I saved on another machine won't open the source TIFF
The project remembers the source TIFF's *path*, but the file may not exist on this machine. The project itself opens fine and you can analyse the cached traces — only motion correction and re-rendering the canvas need the TIFF.

### Python import fails after a Rust change
After editing Rust, rebuild the extension: `uv run maturin develop` (or `./dev.sh build`).

### Crash report appeared in `~/Documents/FIARfly Crash Reports`
Open the latest `.log` file — the first line is the panic message and the call stack shows the source location. File a bug; include both the message and the first ~30 lines of the trace.

---

For algorithm and science background see [docs/PIPELINE_REFERENCE.md](docs/PIPELINE_REFERENCE.md). For internal architecture see [ARCHITECTURE.md](ARCHITECTURE.md).
