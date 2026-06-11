# FIARfly — GUI Design Specification (v0.2)

> Layout specifications, widget inventory, and navigation map for `fiarfly-gui`. This document is the implementation reference for the egui panels.

For an end-user manual see [USER_GUIDE.md](../USER_GUIDE.md).

---

## Navigation Model

The application uses a **linear pipeline navigation** model with a persistent top menu bar plus a **Project tab** that anchors persistence. Any panel can be jumped to from the stepper. State is preserved when navigating back.

```
┌──────────────────────────────────────────────────────────────┐
│  File    View    Help                                         │  ← Menu bar
├──────────┬───────────────────────────────────────────────────┤
│          │                                                    │
│  STEPS   │             MAIN PANEL CONTENT                    │
│          │                                                    │
│  Project │   (open / save .fiarproj, view runs + audit log) │
│ 1 Import │                                                    │
│ 2 Motion │                                                    │
│ 3 ROI    │                                                    │
│ 4 Signal │                                                    │
│ 5 Anlys. │   (v0.2 — windows, metrics, line/bar plots)      │
│ 6 Stats  │   (v0.2 — paired/unpaired/multi tests, FDR)      │
│ 7 Export │                                                    │
│          │                                                    │
│  Log     │                                                    │
│  …       │                                                    │
├──────────┴───────────────────────────────────────────────────┤
│  STATUS BAR: [filename]  [frames × H × W]  [progress bar]   │
└──────────────────────────────────────────────────────────────┘
```

The left sidebar shows the pipeline steps as a vertical stepper, with a **rolling log** below it. The active panel is highlighted; the rest are buttons.

**Native dialog deferral:** all `rfd::FileDialog` calls go through a single `pending_dialog` channel drained at the top of `update()`, *outside* any egui paint closure. This avoids the macOS `+[NSOpenPanel openPanel]` NULL-return crash that bit the previous codebase and Calsight before it.

---

## Panel 1: Import

```
┌─────────────────────────────────────────────────────────────────────┐
│  IMPORT                                                             │
├─────────────────────────────────────────────────────────────────────┤
│                                                                     │
│  Source File                                                        │
│  ┌──────────────────────────────────────────────┐  [Open TIFF...]  │
│  │ /path/to/recording.tif                       │                  │
│  └──────────────────────────────────────────────┘                  │
│                                                                     │
│  Stack Info:  1000 frames  ×  512 px  ×  512 px  |  16-bit  │ 1.0 GB │
│                                                                     │
│  Modality     [ Two-Photon In Vivo ▼ ]                             │
│  Frame Rate   [ 30.0  ] Hz    (optional — enables time axis)       │
│                                                                     │
│  Load Mode                                                          │
│  ◉ Streaming  (large files, low RAM)                               │
│  ○ Load all into RAM  (fast playback, small files < 4 GB)          │
│                                                                     │
│  ┌───────────────────────────────────────────────────────────────┐ │
│  │                                                               │ │
│  │                  FRAME PREVIEW                                │ │
│  │                  (mean projection                             │ │
│  │                   or current frame)                           │ │
│  │                                                               │ │
│  └───────────────────────────────────────────────────────────────┘ │
│  Frame  [────●────────────────────] 0 / 999     [Mean] [Max] [Curr] │
│                                                                     │
│                                           [Continue to Motion →]   │
└─────────────────────────────────────────────────────────────────────┘
```

**Widget inventory:**
| Widget | Type | Behavior |
|---|---|---|
| Open TIFF button | Button | Opens native file dialog (rfd) |
| File path | TextEdit (read-only) | Displays selected path |
| Stack info | Label | Updated after file loads |
| Modality | ComboBox | Sets `AppState.params.modality` |
| Frame rate | TextEdit → f32 | Optional; enables time-axis in trace viewer |
| Load mode | Radio | Sets streaming vs. load-all |
| Frame preview | Image (egui Texture) | Updates on slider drag |
| Frame slider | Slider<usize> | Controls preview frame |
| Projection buttons | Toggle | Switch preview: mean / max / current |
| Continue button | Button | Enabled after file loaded; switches panel |

---

## Panel 2: Motion Correction

```
┌─────────────────────────────────────────────────────────────────────┐
│  MOTION CORRECTION                                                  │
├─────────────────────────────────────────────────────────────────────┤
│                                                                     │
│  Algorithm                                                          │
│  ◉ Rigid     ○ Non-Rigid                                           │
│                                                                     │
│  Parameters                                                         │
│  Max shift      [ 10  ] px                                         │
│  Template bin   [ 50  ] frames                                      │
│  ── Non-Rigid only ──────────────────────                          │
│  Grid size      [ 32  ] × [ 32  ] px   (grayed when Rigid)        │
│  Overlap        [ 8   ] px                                         │
│                                                                     │
│  [  Run Motion Correction  ]   ████████████░░░░░░░ 62%            │
│                                                                     │
│  ┌────────────────────────┐  ┌────────────────────────┐           │
│  │   BEFORE (raw)         │  │   AFTER (corrected)    │           │
│  │                        │  │                        │           │
│  │    [frame image]       │  │    [frame image]       │           │
│  │                        │  │                        │           │
│  └────────────────────────┘  └────────────────────────┘           │
│  Frame  [────────●──────────────────────] 247 / 999               │
│                                                                     │
│  Quality Metrics                                                    │
│  ┌─────────────────────────────────────────────────────────────┐   │
│  │  correlation  ▂▃▄▅▄▅▅▅▄▅▃▅▅▅▅▅▅▅▅▅  [plot, per frame]     │   │
│  │  ──── threshold (0.90) ──────────────────────────────────── │   │
│  └─────────────────────────────────────────────────────────────┘   │
│  Flagged frames: 3   (frames 12, 89, 341 — correlation < 0.90)     │
│                                                                     │
│  [← Back]                              [Continue to ROI Editor →]  │
└─────────────────────────────────────────────────────────────────────┘
```

**Widget inventory:**
| Widget | Type | Behavior |
|---|---|---|
| Algorithm radio | Radio | Rigid / Non-Rigid |
| Max shift | DragValue | `params.motion.max_shift` |
| Template bin | DragValue | `params.motion.bin_width` |
| Grid size | 2× DragValue | `params.motion.grid_size`; grayed if Rigid |
| Overlap | DragValue | `params.motion.overlap`; grayed if Rigid |
| Run button | Button | Spawns worker thread |
| Progress bar | ProgressBar | Driven by `AppState.progress` |
| Before/After images | 2× Image | Synced to frame slider |
| Frame slider | Slider<usize> | Controls both preview images |
| Quality plot | egui_plot::Plot | Per-frame correlation; horizontal threshold line |
| Flagged frames | Label | Lists frame indices below threshold |

---

## Panel 3: ROI Editor

```
┌─────────────────────────────────────────────────────────────────────┐
│  ROI EDITOR                                                         │
├───────────────────────────────────────────────────┬─────────────────┤
│  Tools: [Draw] [Select] [Move] [Delete]           │ ROI LIST        │
│  Frame: [──────●─────────────────] 0/999 [Mean]  │─────────────────│
│                                                   │ ● Cell 1   🎨 👁│
│  ┌────────────────────────────────────────────┐  │ ● Cell 2   🎨 👁│
│  │                                            │  │ ○ Cell 3   🎨 👁│
│  │                                            │  │                 │
│  │   [image with overlaid ROI polygons]       │  │ Label: [Cell 3] │
│  │                                            │  │ Group: [L2/3  ] │
│  │   ┌──────────────────┐                    │  │                 │
│  │   │    ROI (filled,  │                    │  │ [Delete ROI]    │
│  │   │    semi-trans.)  │                    │  │                 │
│  │   └──────────────────┘                    │  │─────────────────│
│  │                                            │  │ ROI SETS        │
│  │   ● ← vertex handle (drag to edit)        │  │                 │
│  │                                            │  │ [Save ROI Set]  │
│  │                                            │  │ [Load ROI Set]  │
│  └────────────────────────────────────────────┘  │ [Add Individual]│
│                                                   │ [Clear All]     │
│  Drawing:  Click=add vertex  DblClick=close       │                 │
│            Esc=cancel   RightClick=remove vertex  │ Opacity [████░] │
│                                                   │                 │
│  [← Back]              [Continue to Traces →]    │                 │
└───────────────────────────────────────────────────┴─────────────────┘
```

**Widget inventory:**
| Widget | Type | Behavior |
|---|---|---|
| Tool buttons | SegmentedButton | Draw / Select / Move / Delete modes |
| Frame slider | Slider | Change displayed frame |
| Projection toggle | Button | Mean / Max / Current |
| Canvas | Painter (custom) | Interactive ROI drawing area |
| ROI list | ScrollArea + rows | Each row: colored dot, label, eye toggle |
| Label field | TextEdit | Rename selected ROI inline |
| Group field | TextEdit | Assign ROI to a group |
| Delete ROI | Button | Remove selected ROI |
| Save/Load ROI Set | Button | `rfd` dialogs → `.rois.json` |
| Add Individual | Button | Load a single `.rois.json`, append |
| Clear All | Button | Confirmation dialog first |
| Opacity slider | Slider<f32> | ROI fill opacity (0.1–0.5) |

**ROI Drawing State Machine:**

```
IDLE
 │ LClick on canvas
 ▼
IN_PROGRESS(vertices: Vec<[f32;2]>)
 │ LClick → add vertex
 │ DblClick OR close-to-first-vertex → COMPLETE
 │ Esc → IDLE (discard)
 ▼
COMPLETE → add Roi to AppState.roi_set → IDLE
```

**Coordinate transform:**
- Canvas fills the panel's available rect minus sidebar.
- Image is drawn scaled to fill the canvas, preserving aspect ratio.
- `screen_to_image(pos)`: scale by `(img_w / canvas_w, img_h / canvas_h)`.
- All ROI vertices stored in image-pixel coordinates.

---

## Panel 4: Signal Viewer

```
┌─────────────────────────────────────────────────────────────────────┐
│  SIGNAL VIEWER                                                      │
├─────────────────────────────────────────────────────────────────────┤
│  [Extract Traces]  ████████████████████ Done (12 ROIs, 1000 frames)│
│                                                                     │
│  Display:  ◉ ΔF/F   ○ Raw F    Neuropil correction: [☑] (r=0.70)  │
│  [☑] Cell 1  [☑] Cell 2  [☑] Cell 3 ...  [All] [None]             │
│                                                                     │
│  ┌─────────────────────────────────────────────────────────────┐   │
│  │ ΔF/F                                                         │   │
│  │  2.0 │         ╭╮                                           │   │
│  │  1.5 │         ││  ╭╮                                       │   │
│  │  1.0 │    ╭╮   ││  ││                                       │   │
│  │  0.5 │────╯╰───╯╰──╯╰───────────────────────────────────── │   │
│  │  0.0 │                                                       │   │
│  │      └────────────────────────────────────────── frame      │   │
│  │          0         250         500         750      999      │   │
│  └─────────────────────────────────────────────────────────────┘   │
│                        [zoom/pan with scroll/drag]                 │
│                                                                     │
│  ┌──────────────────────┐  Playback:  [◀◀] [▶/⏸] [▶▶]  1×▼      │
│  │  Frame thumbnail     │  Frame: 247   Time: 8.23 s              │
│  │  with ROI overlay    │                                          │
│  └──────────────────────┘  [OASIS Deconvolution...]               │
│                                                                     │
│  [← Back]                              [Continue to Export →]      │
└─────────────────────────────────────────────────────────────────────┘
```

**Widget inventory:**
| Widget | Type | Behavior |
|---|---|---|
| Extract button | Button | Spawns extraction worker |
| Display toggle | Radio | ΔF/F vs. Raw F |
| Neuropil checkbox | Checkbox | Toggle neuropil correction |
| r slider | Slider<f32> | Neuropil factor 0.0–1.0 |
| ROI toggles | Checkboxes | Show/hide per-ROI trace |
| All / None | Buttons | Select/deselect all ROIs |
| Trace plot | egui_plot::Plot | Multi-line, color-matched to ROIs |
| Frame thumbnail | Image | Current frame with ROI overlay |
| Playback controls | Buttons | ◀◀ (first), ▶/⏸ (play/pause), ▶▶ (last) |
| Speed selector | ComboBox | 0.25×, 0.5×, 1×, 2× |
| Frame counter | Label | Current frame + time |
| OASIS button | Button | Opens deconvolution dialog |

**Playback implementation:**
- Track `last_frame_time: std::time::Instant`.
- In `update()`: if playing, check elapsed time vs. `1.0 / (frame_rate * speed)`, advance frame if needed.
- Request repaint after advancing: `ctx.request_repaint()`.

---

## Panel 5: Export

```
┌─────────────────────────────────────────────────────────────────────┐
│  EXPORT                                                             │
├─────────────────────────────────────────────────────────────────────┤
│                                                                     │
│  Output Directory                                                   │
│  ┌──────────────────────────────────────────┐  [Browse...]         │
│  │ /Users/researcher/data/results/          │                      │
│  └──────────────────────────────────────────┘                      │
│                                                                     │
│  Filename stem:  [recording_20240115]                               │
│                  → recording_20240115.parquet                       │
│                  → recording_20240115.csv                           │
│                                                                     │
│  Export Format                                                      │
│  [☑] Parquet  (recommended — typed, compressed)                    │
│  [☑] CSV      (for compatibility)                                   │
│                                                                     │
│  Include Columns                                                    │
│  [☑] raw_f            [☑] delta_f_over_f                          │
│  [☐] neuropil_f       [☐] deconvolved                              │
│  [☑] roi_label        [☑] time_s                                   │
│                                                                     │
│  Session File                                                       │
│  [Save session (.fiarfly)]  — saves ROIs, params, file reference  │
│                                                                     │
│  ┌──────────────────────────────────────────────────────────────┐  │
│  │  Preview (first 5 rows):                                     │  │
│  │  roi_id   frame_idx  time_s  raw_f  delta_f_over_f          │  │
│  │  roi_001  0          0.000   0.312  0.021                    │  │
│  │  roi_001  1          0.033   0.314  0.028                    │  │
│  │  ...                                                         │  │
│  └──────────────────────────────────────────────────────────────┘  │
│                                                                     │
│  [  Export  ]    ✓ Exported 12 ROIs × 1000 frames = 12,000 rows   │
│                  → /path/to/recording_20240115.parquet  [Open]     │
│                                                                     │
│  [← Back]                                                          │
└─────────────────────────────────────────────────────────────────────┘
```

**Widget inventory:**
| Widget | Type | Behavior |
|---|---|---|
| Directory field | TextEdit + Button | rfd folder picker |
| Filename stem | TextEdit | Editable; shows generated filenames below |
| Format checkboxes | Checkboxes | parquet / csv |
| Column checkboxes | Checkboxes | Which columns to include |
| Save session | Button | rfd save dialog → `.fiarfly` |
| Preview table | Grid | Shows first 5 rows of output |
| Export button | Button | Runs export, shows success message |
| Open button | Button | Opens output directory in Finder/Explorer |

---

## Top Menu Bar

```
File
├── Open TIFF...               Cmd/Ctrl+O
├── ─────────
├── New Project...             (create a .fiarproj bundle)
├── Open Project...            (folder picker; loads project.json)
├── Add current as run         (enabled when a project is open + traces exist)
├── ─────────
├── Import ROI Set...
├── ─────────
└── Quit                       Cmd/Ctrl+Q

View
├── Reset Zoom
└── Frame label overlay (toggle)

Help
└── About FIARfly
```

---

## Panel: Project (v0.2)

```
┌─────────────────────────────────────────────────────────────────────┐
│  PROJECT                                                            │
├─────────────────────────────────────────────────────────────────────┤
│  [New Project…]  [Open Project…]  [Add current as run]              │
│                                                                     │
│  ▼ Metadata                                                         │
│    Name:           study_a                                          │
│    Bundle path:    /data/study_a.fiarproj                           │
│    Author:         tariq@studio                                     │
│    Created:        2026-04-23 10:15:42                              │
│    Modified:       2026-04-26 14:08:11                              │
│    Version:        0.2.0                                            │
│    Source TIFF:    /data/recording_2026-04-23.tif                   │
│    Frame rate:     30.00 Hz                                         │
│    Image:          512 × 512 px                                     │
│    Modality:       TwoPhotonInVivo                                  │
│    Runs:           3                                                │
│    Description:    [editable multi-line]   [Save description]      │
│                                                                     │
│  ▼ Runs (3)                                                         │
│    ┌────────────────────────────────────────────────────────────┐  │
│    │ baseline-rolling-pct8       12 ROIs × 1800 frames           │  │
│    │ run_1714061720000_baseline_rolling_pct8                     │  │
│    │ Includes: neuropil, deconvolved                             │  │
│    │ Source: /data/recording_2026-04-23.tif  Motion-corrected    │  │
│    │ fiarfly 0.1.0                  2026-04-23 11:02:33          │  │
│    └────────────────────────────────────────────────────────────┘  │
│    ... (one card per run)                                          │
│                                                                     │
│  ▼ Traceability log (8)                                             │
│    2026-04-26 14:08:11  RUN     Run "fixed-baseline" added: 12 …   │
│    2026-04-26 13:55:01  OPEN    Project opened in GUI               │
│    2026-04-23 11:02:33  RUN     Run "baseline-rolling-pct8" added… │
│    2026-04-23 10:15:42  CREATE  Project "study_a" created.         │
│                                                                     │
│  Next run name: [_______________________]                          │
└─────────────────────────────────────────────────────────────────────┘
```

**Widget inventory:**
| Widget | Type | Behavior |
|---|---|---|
| New Project… | Button | Save dialog → `Project::create_new` |
| Open Project… | Button | Folder picker → `Project::open` |
| Add current as run | Button | Enabled iff project + traces present |
| Description | TextEdit (multiline) | Editable; Save Description button persists |
| Run cards | Group + Labels | One card per `RunMetadata` |
| Audit log | Reverse-chronological list | Color-coded action tag + description + run_id |
| Next run name | TextEdit | Buffer for the next "Add current as run" call |

**State:** `AppState::project: Option<Project>`, `next_run_name: String`, `project_description_buf: String`.

---

## Panel: Analysis (v0.2)

```
┌─────────────────────────────────────────────────────────────────────┐
│  ANALYSIS                                                           │
├─────────────────────────────────────────────────────────────────────┤
│  Source: [ΔF/F ▼]   Metric: [Window mean ▼]   Plot: ◉ Line ○ Bar  │
│  [Window units: seconds]   [☐ Aggregate by ROI group]              │
│                                                                     │
│  ▼ Windows (2)                                                      │
│    Add from label: [Baseline] [Stim 1] [Stim 2]                    │
│    [Stim 1____] from (s) [10.0] to (s) [15.0]   ✕                  │
│    [Stim 2____] from (s) [25.0] to (s) [35.0]   ✕                  │
│    [+ Add window]  [Clear]                                         │
│                                                                     │
│  [Compute]   Window mean · 2 windows · 12 ROIs                     │
│                                                                     │
│  ┌─────────────────────────────────────────────────────────────┐   │
│  │  [line plot of ΔF/F per ROI with vertical window guides]    │   │
│  └─────────────────────────────────────────────────────────────┘   │
│                                                                     │
│                                            [→ Statistics]          │
└─────────────────────────────────────────────────────────────────────┘
```

**Widget inventory:**
| Widget | Type | Behavior |
|---|---|---|
| Source | ComboBox | ΔF/F or Raw F |
| Metric | ComboBox | maps to `fiarfly_core::analysis::Metric` |
| Plot | Selectable label | Line / Bar |
| Units toggle | Selectable label | Frames vs Seconds (warns when seconds without frame_rate) |
| Aggregate by ROI group | Checkbox | Per-ROI vs per-group bars |
| Add from label | Buttons | One per `state.frame_labels` — auto-converts frames↔seconds |
| Window row | TextEdit + 2× DragValue | Editable name and bounds |
| Compute | Button | Runs `fiarfly_core::analysis::compute` per window, builds `SummaryTable` |
| Line plot | egui_plot::Plot | Per-ROI traces + vertical window guides |
| Bar plot | egui_plot::Plot | Grouped bars, one cluster per group, one bar per window |
| Summary table | Grid (collapsing) | mean per group × window |

**State:** `AppState::analysis: AnalysisUi { source, plot_kind, metric, windows, use_seconds, group_by_roi_group, last_summary }`.

---

## Panel: Statistics (v0.2)

```
┌─────────────────────────────────────────────────────────────────────┐
│  STATISTICS                                                         │
├─────────────────────────────────────────────────────────────────────┤
│  Source metric: Window mean (2 windows, 12 ROIs)                    │
│  Compare: ◉ across windows (paired)  ○ across ROI groups (unpaired)│
│  Test family: ◉ Auto  ○ Parametric  ○ Non-parametric               │
│  Multiple comparisons: [Bonferroni ▼]                               │
│                                                                     │
│  [Run tests]                                                        │
│                                                                     │
│  ▼ Results                                                          │
│   A          B         n   test         stat   mean A   mean B …   │
│  (omnibus)  2 samples  24  paired t      2.1   …                   │
│  Stim 1     Stim 2     12  paired t      3.4   0.31    0.62  …    │
│                                                                     │
│  ▼ Effect-size forest plot                                          │
│  ●─────────────────  Stim 1 vs Stim 2 (Cohen's dz = 0.92)          │
│              0                                                      │
└─────────────────────────────────────────────────────────────────────┘
```

**Widget inventory:**
| Widget | Type | Behavior |
|---|---|---|
| Compare | Selectable label | `StatsAxis::AcrossWindows` / `AcrossGroups` |
| Test family | Selectable label | maps to `fiarfly_core::stats::TestKind` |
| Multiple comparisons | ComboBox | None / Bonferroni / Benjamini-Hochberg |
| Run tests | Button | Builds samples from `SummaryTable`, calls `run_test` |
| Results table | Grid | A, B, n, test, stat, means, p, p_adj (color-coded), effect |
| Forest plot | egui_plot::Plot | Effect-size points + reference whiskers |

The omnibus test (ANOVA / Kruskal-Wallis) is automatically run when ≥3 samples are involved and prepended to the results as a single `(omnibus)` row, *not* included in the multiple-comparisons family.

**State:** `AppState::stats_ui: StatsUi { axis, kind, correction, last_results, last_error }`.

---

## Color Scheme and Visual Style

- **Theme:** Dark by default (better for viewing fluorescence images). Toggle available.
- **ROI colors:** Assigned automatically from a 12-color qualitative palette (ColorBrewer Set3), cycling if > 12 ROIs.
- **Accent color:** Teal (`#2DD4BF`) for active/selected states, progress bars, step indicators.
- **Warning color:** Amber (`#FBBF24`) for flagged frames, out-of-range parameters.
- **Error color:** Red (`#F87171`) for failed operations.
- **Fonts:** egui default (Hack) for the UI; monospace for the status log.

---

## Accessibility Notes

- All interactive elements have hover tooltips.
- Keyboard shortcuts documented in tooltips and menu labels.
- ROI colors are drawn with both fill color and a border with distinct line style (solid / dashed) to aid color-blind users.
- Font size adjustable via View > Preferences.

---

## Error and Status Display

A collapsible log panel at the bottom of the status bar shows timestamped messages:

```
[10:42:31] Loaded recording.tif: 1000 frames × 512×512, 16-bit
[10:43:05] Motion correction: 1000/1000 frames complete (rigid)
[10:43:05] Warning: 3 frames flagged (correlation < 0.90): frames 12, 89, 341
[10:44:18] Extracted traces: 12 ROIs × 1000 frames
```

Error messages appear as modal dialogs for critical errors (file not found, out of memory) and as log entries for warnings.
