//! Shared application state.

use fiarfly_core::{
    ImageStack,
    io::Project,
    roi::RoiSet,
    motion::{MotionCorrectionParams, SpatialFilterParams},
    signal::{DeltaFParams, EventDetectionParams, OasisParams, OasisResult, EventResult, RoiQuality},
};
use ndarray::Array2;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Instant;
use eframe::egui;

/// A named frame range shown in the draggable overlay.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct FrameLabel {
    pub name: String,
    pub start: usize,
    pub end: usize,
}

/// Which signal is shown in the trace plot.
#[derive(Debug, Clone, PartialEq, Default)]
pub enum TraceMode {
    #[default]
    Raw,
    DeltaF,
}

#[derive(Debug, Clone, PartialEq, Default)]
pub enum ActivePanel {
    #[default]
    Import,
    MotionCorrection,
    RoiEditor,
    SignalViewer,
    Export,
    Project,
    Analysis,
    Statistics,
}

// Dialogs must be opened outside the egui paint closure: invoking
// `rfd::FileDialog::pick_file()` from within a panel's `show` closure on
// macOS intermittently yields a NULL `NSOpenPanel` and panics. Panel code
// sets `AppState::pending_dialog` and `FiarflyApp::update` drains it at
// the top of the next frame, safely outside any paint closure.
#[derive(Debug, Clone, PartialEq)]
pub enum DialogRequest {
    OpenTiff,
    ImportRoiSet,
    SaveRoiSet,
    LoadRoiSet,
    PickExportDir,
    SaveSessionFile,
    AddBatchFiles,
    PickBatchOutputDir,
    NewProject,
    OpenProject,
    /// Append the current AppState as a run inside the open project.
    AddCurrentAsRun,
}

#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub enum Modality {
    TwoPhotonInVivo,
    TwoPhotonInVitro,
    OnePhotonInVivo,
    OnePhotonWideField,
}

impl std::fmt::Display for Modality {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Modality::TwoPhotonInVivo    => write!(f, "Two-Photon In Vivo"),
            Modality::TwoPhotonInVitro   => write!(f, "Two-Photon In Vitro"),
            Modality::OnePhotonInVivo    => write!(f, "One-Photon In Vivo"),
            Modality::OnePhotonWideField => write!(f, "One-Photon Wide-field"),
        }
    }
}

impl From<fiarfly_core::roi::Modality> for Modality {
    fn from(m: fiarfly_core::roi::Modality) -> Self {
        match m {
            fiarfly_core::roi::Modality::TwoPhotonInVivo    => Modality::TwoPhotonInVivo,
            fiarfly_core::roi::Modality::TwoPhotonInVitro   => Modality::TwoPhotonInVitro,
            fiarfly_core::roi::Modality::OnePhotonInVivo    => Modality::OnePhotonInVivo,
            fiarfly_core::roi::Modality::OnePhotonWideField => Modality::OnePhotonWideField,
        }
    }
}

impl From<&Modality> for fiarfly_core::roi::Modality {
    fn from(m: &Modality) -> Self {
        match m {
            Modality::TwoPhotonInVivo    => fiarfly_core::roi::Modality::TwoPhotonInVivo,
            Modality::TwoPhotonInVitro   => fiarfly_core::roi::Modality::TwoPhotonInVitro,
            Modality::OnePhotonInVivo    => fiarfly_core::roi::Modality::OnePhotonInVivo,
            Modality::OnePhotonWideField => fiarfly_core::roi::Modality::OnePhotonWideField,
        }
    }
}

#[derive(Debug, Clone)]
pub struct PipelineParams {
    pub modality: Modality,
    pub motion: MotionCorrectionParams,
    pub delta_f: DeltaFParams,
    pub neuropil_r: f32,
    pub frame_rate: Option<f32>,
    pub export_path: Option<PathBuf>,
    /// Whether to apply spatial high-pass filter before motion correction (1P).
    pub apply_spatial_filter: bool,
    pub spatial_filter: SpatialFilterParams,
    pub oasis: OasisParams,
    pub event_detection: EventDetectionParams,
}

impl Default for PipelineParams {
    fn default() -> Self {
        Self {
            modality: Modality::TwoPhotonInVivo,
            motion: MotionCorrectionParams::default(),
            delta_f: DeltaFParams::default(),
            neuropil_r: 0.7,
            frame_rate: None,
            export_path: None,
            apply_spatial_filter: false,
            spatial_filter: SpatialFilterParams::default(),
            oasis: OasisParams::default(),
            event_detection: EventDetectionParams::default(),
        }
    }
}

/// Result delivered by a background worker through `done_rx`.
pub enum WorkerOutput {
    MotionCorrected(fiarfly_core::motion::CorrectionResult),
    TracesExtracted {
        raw: Array2<f32>,
        delta_f: Array2<f32>,
        neuropil_f: Option<Array2<f32>>,
    },
    ProjectionComputed {
        mean: fiarfly_core::Frame,
        max:  fiarfly_core::Frame,
    },
    /// One batch MC item finished — data already saved to disk.
    BatchItemDone {
        input: PathBuf,
        n_frames: usize,
        elapsed_secs: f64,
    },
    /// OASIS deconvolution finished.
    OasisDone(OasisResult),
    /// Event detection + quality metrics finished.
    AnalysisDone {
        events: EventResult,
        quality: Vec<RoiQuality>,
    },
}

/// A detached background worker communicates via two channels.
/// The thread is detached (JoinHandle dropped); results come through done_rx.
pub struct WorkerHandle {
    pub progress_rx: std::sync::mpsc::Receiver<f32>,
    pub done_rx: std::sync::mpsc::Receiver<Result<WorkerOutput, fiarfly_core::FiarflyError>>,
}

/// The complete application state.
pub struct AppState {
    // --- Data ---
    pub source_path: Option<PathBuf>,
    pub tiff_reader: Option<fiarfly_core::io::TiffReader>,
    /// Raw stack in memory (Arc so workers can share without cloning GBs).
    pub stack: Option<Arc<ImageStack>>,
    /// Motion-corrected stack.
    pub corrected: Option<Arc<ImageStack>>,
    pub roi_set: Option<RoiSet>,
    pub raw_f: Option<Array2<f32>>,
    pub delta_f: Option<Array2<f32>>,
    pub neuropil_f: Option<Array2<f32>>,
    pub shift_scores: Vec<f32>,

    // --- UI ---
    pub active_panel: ActivePanel,
    pub current_frame: usize,
    pub params: PipelineParams,
    pub worker: Option<WorkerHandle>,
    /// Wall-clock instant when the current worker was spawned (for elapsed timing).
    pub worker_start: Option<std::time::Instant>,
    pub progress: f32,
    pub log: Vec<String>,

    // --- Batch motion correction ---
    pub batch_queue: Vec<PathBuf>,
    pub batch_out_dir: Option<PathBuf>,
    pub batch_running: bool,

    // --- Analysis results ---
    pub oasis_result: Option<OasisResult>,
    pub event_result: Option<EventResult>,
    pub roi_quality: Option<Vec<RoiQuality>>,

    // --- Signal viewer display ---
    pub trace_mode: TraceMode,
    /// Custom per-ROI baseline override: [start_frame, end_frame].
    /// None = use rolling DeltaFParams baseline.
    pub dff_baseline_start: usize,
    pub dff_baseline_end: usize,
    /// Whether a custom fixed baseline window is active.
    pub dff_use_fixed_baseline: bool,
    /// ΔF/F computed from the fixed baseline (recomputed on demand).
    pub delta_f_fixed: Option<Array2<f32>>,

    // --- Frame label overlay ---
    pub frame_labels: Vec<FrameLabel>,
    pub show_label_overlay: bool,

    // --- Frame preview cache ---
    pub preview_texture: Option<egui::TextureHandle>,
    pub preview_frame_loaded: usize,

    // --- ROI editor canvas ---
    pub projection_mean: Option<fiarfly_core::Frame>,
    pub projection_max: Option<fiarfly_core::Frame>,
    pub roi_lut_min: f32,
    pub roi_lut_max: f32,
    pub roi_show_projection: bool,
    pub roi_texture: Option<egui::TextureHandle>,
    /// Cache key: encodes source + LUT settings. Empty means stale.
    pub roi_texture_key: String,

    // --- Playback ---
    pub playing: bool,
    pub playback_speed: f32,
    pub last_frame_time: Option<Instant>,

    // --- Session ---
    pub session_path: Option<PathBuf>,

    // --- Deferred native dialog (see DialogRequest for why). ---
    pub pending_dialog: Option<DialogRequest>,

    // --- Project (.fiarproj bundle). ---
    pub project: Option<Project>,
    /// Editable run-name buffer for the next "Add as run" action.
    pub next_run_name: String,
    /// Editable description buffer (mirrored to project.description on save).
    pub project_description_buf: String,

    // --- Analysis panel UI state. ---
    pub analysis: AnalysisUi,

    // --- Statistics panel UI state. ---
    pub stats_ui: StatsUi,
}

/// Persistent UI state for the Statistics panel.
#[derive(Debug, Clone)]
pub struct StatsUi {
    pub axis: StatsAxis,
    pub kind: StatsKind,
    pub correction: StatsCorrection,
    /// Pairwise comparison results, populated by Run.
    pub last_results: Vec<StatsRow>,
    /// Optional last-error message shown beneath the controls.
    pub last_error: Option<String>,
}

impl Default for StatsUi {
    fn default() -> Self {
        Self {
            axis: StatsAxis::AcrossWindows,
            kind: StatsKind::Auto,
            correction: StatsCorrection::None,
            last_results: Vec::new(),
            last_error: None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StatsAxis {
    /// Compare metric across windows for the same ROIs (paired).
    AcrossWindows,
    /// Compare metric across ROI groups inside a fixed window (unpaired).
    AcrossGroups,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StatsKind {
    Auto,
    Parametric,
    NonParametric,
}

impl StatsKind {
    pub fn to_core(self) -> fiarfly_core::stats::TestKind {
        use fiarfly_core::stats::TestKind as K;
        match self {
            StatsKind::Auto          => K::Auto,
            StatsKind::Parametric    => K::Parametric,
            StatsKind::NonParametric => K::NonParametric,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StatsCorrection {
    None,
    Bonferroni,
    BenjaminiHochberg,
}

impl StatsCorrection {
    pub fn to_core(self) -> fiarfly_core::stats::MultipleComparisonsCorrection {
        use fiarfly_core::stats::MultipleComparisonsCorrection as M;
        match self {
            StatsCorrection::None              => M::None,
            StatsCorrection::Bonferroni        => M::Bonferroni,
            StatsCorrection::BenjaminiHochberg => M::BenjaminiHochberg,
        }
    }
    pub fn label(self) -> &'static str {
        match self {
            StatsCorrection::None              => "None",
            StatsCorrection::Bonferroni        => "Bonferroni",
            StatsCorrection::BenjaminiHochberg => "Benjamini-Hochberg (FDR)",
        }
    }
}

/// One pairwise comparison row in the results table.
#[derive(Debug, Clone)]
pub struct StatsRow {
    pub label_a: String,
    pub label_b: String,
    pub n_a: usize,
    pub n_b: usize,
    pub test_name: String,
    pub statistic: f64,
    pub p_value: f64,
    pub p_adjusted: f64,
    pub effect_size: Option<f64>,
    pub effect_kind: Option<String>,
    pub mean_a: f64,
    pub mean_b: f64,
}

/// Persistent UI state for the Analysis panel.
#[derive(Debug, Clone)]
pub struct AnalysisUi {
    pub source: AnalysisSource,
    pub plot_kind: PlotKind,
    pub metric: AnalysisMetric,
    pub windows: Vec<AnalysisWindow>,
    pub use_seconds: bool,
    pub group_by_roi_group: bool,
    /// Cached summary table from the last "Compute" action.
    pub last_summary: Option<SummaryTable>,
}

impl Default for AnalysisUi {
    fn default() -> Self {
        Self {
            source: AnalysisSource::DeltaF,
            plot_kind: PlotKind::Line,
            metric: AnalysisMetric::DffWindowMean,
            windows: Vec::new(),
            use_seconds: true,
            group_by_roi_group: false,
            last_summary: None,
        }
    }
}

/// Which trace data to feed into the analysis.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AnalysisSource {
    DeltaF,
    RawF,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PlotKind {
    Line,
    Bar,
}

/// GUI-facing metric enum (1:1 with `fiarfly_core::analysis::Metric`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AnalysisMetric {
    DffAtFrame,
    DffWindowMean,
    DffWindowPeak,
    Auc,
    LatencyToPeak,
    RiseTime,
}

impl AnalysisMetric {
    pub fn label(self) -> &'static str {
        match self {
            AnalysisMetric::DffAtFrame     => "Value at frame",
            AnalysisMetric::DffWindowMean  => "Window mean",
            AnalysisMetric::DffWindowPeak  => "Window peak",
            AnalysisMetric::Auc            => "AUC (trapezoidal)",
            AnalysisMetric::LatencyToPeak  => "Latency to peak",
            AnalysisMetric::RiseTime       => "Rise time (10–90%)",
        }
    }
    pub fn to_core(self) -> fiarfly_core::analysis::Metric {
        use fiarfly_core::analysis::Metric as M;
        match self {
            AnalysisMetric::DffAtFrame    => M::DffAtFrame,
            AnalysisMetric::DffWindowMean => M::DffWindowMean,
            AnalysisMetric::DffWindowPeak => M::DffWindowPeak,
            AnalysisMetric::Auc           => M::Auc,
            AnalysisMetric::LatencyToPeak => M::LatencyToPeak,
            AnalysisMetric::RiseTime      => M::RiseTime,
        }
    }
}

/// One user-defined window for analysis.
#[derive(Debug, Clone)]
pub struct AnalysisWindow {
    pub name: String,
    /// Start in frames OR seconds depending on `AnalysisUi::use_seconds`.
    pub start: f32,
    pub end: f32,
}

/// Computed summary: one row per ROI per window.
#[derive(Debug, Clone)]
pub struct SummaryTable {
    pub metric_label: String,
    pub windows: Vec<String>,
    pub roi_labels: Vec<String>,
    pub roi_groups: Vec<Option<String>>,
    /// Shape: [n_windows][n_rois].
    pub values: Vec<Vec<f32>>,
}

impl Default for AppState {
    fn default() -> Self {
        Self {
            source_path: None,
            tiff_reader: None,
            stack: None,
            corrected: None,
            roi_set: None,
            raw_f: None,
            delta_f: None,
            neuropil_f: None,
            shift_scores: Vec::new(),
            active_panel: ActivePanel::default(),
            current_frame: 0,
            params: PipelineParams::default(),
            worker: None,
            worker_start: None,
            progress: 0.0,
            log: Vec::new(),
            batch_queue: Vec::new(),
            batch_out_dir: None,
            batch_running: false,
            oasis_result: None,
            event_result: None,
            roi_quality: None,
            trace_mode: TraceMode::Raw,
            dff_baseline_start: 0,
            dff_baseline_end: 30,
            dff_use_fixed_baseline: false,
            delta_f_fixed: None,
            frame_labels: Vec::new(),
            show_label_overlay: true,
            preview_texture: None,
            preview_frame_loaded: usize::MAX,
            projection_mean: None,
            projection_max: None,
            roi_lut_min: 0.0,
            roi_lut_max: 1.0,
            roi_show_projection: false,
            roi_texture: None,
            roi_texture_key: String::new(),
            playing: false,
            playback_speed: 1.0,
            last_frame_time: None,
            session_path: None,
            pending_dialog: None,
            project: None,
            next_run_name: String::new(),
            project_description_buf: String::new(),
            analysis: AnalysisUi::default(),
            stats_ui: StatsUi::default(),
        }
    }
}

impl AppState {
    pub fn log(&mut self, msg: impl Into<String>) {
        use std::time::{SystemTime, UNIX_EPOCH};
        let t = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| {
                let s = d.as_secs() % 86400;
                format!("{:02}:{:02}:{:02}", s / 3600, (s % 3600) / 60, s % 60)
            })
            .unwrap_or_else(|_| "??:??:??".into());
        self.log.push(format!("[{t}] {}", msg.into()));
    }

    pub fn n_frames(&self) -> usize {
        self.tiff_reader.as_ref().map(|r| r.num_frames).unwrap_or(0)
    }

    pub fn image_shape(&self) -> Option<(usize, usize)> {
        self.tiff_reader.as_ref().map(|r| (r.height, r.width))
    }

    /// Return the best available stack for extraction: corrected > raw.
    pub fn best_stack(&self) -> Option<Arc<ImageStack>> {
        self.corrected.clone().or_else(|| self.stack.clone())
    }
}
