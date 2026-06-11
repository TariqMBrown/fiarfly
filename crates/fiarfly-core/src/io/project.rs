//! Project bundle — persistent workflow state without the source TIFF.
//!
//! A `.fiarproj` is a directory bundle containing:
//!
//! ```text
//! mystudy.fiarproj/
//! ├── project.json         metadata, params, ROI polygons, frame labels, run index
//! └── runs/
//!     └── <run_id>/
//!         ├── run.json      per-run metadata + params snapshot
//!         ├── traces.parquet (raw_f, delta_f, neuropil_f, deconvolved)
//!         ├── events.parquet (optional)
//!         └── quality.parquet (optional)
//! ```
//!
//! The bundle deliberately does *not* store the source TIFF — only the
//! analysis outputs, ROIs, and parameters needed to resume or extend a
//! workflow. The path to the original TIFF is remembered so the GUI can
//! offer to re-load it for fresh ROI overlays.

use std::fs;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use ndarray::Array2;
use polars::prelude::*;
use serde::{Deserialize, Serialize};

use crate::export::{to_dataframe, write_parquet, ExportData};
use crate::motion::MotionCorrectionParams;
use crate::roi::{Modality, RoiSet};
use crate::signal::{CaEvent, DeltaFParams, OasisParams, OasisResult, RoiQuality};
use crate::FiarflyError;

pub const PROJECT_VERSION: &str = "0.2.0";
pub const PROJECT_FILE: &str = "project.json";
pub const RUNS_DIR: &str = "runs";
pub const RUN_FILE: &str = "run.json";
pub const TRACES_FILE: &str = "traces.parquet";
pub const EVENTS_FILE: &str = "events.parquet";
pub const QUALITY_FILE: &str = "quality.parquet";

// ─────────────────────────────────────────────────────────────────────────────
// Serializable types
// ─────────────────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct FrameLabelDef {
    pub name: String,
    pub start: usize,
    pub end: usize,
}

/// Discrete event category in the project's audit log.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum AuditAction {
    ProjectCreated,
    ProjectOpened,
    RunAdded,
    RoiSetUpdated,
    FrameLabelsEdited,
    SourceTiffSet,
    Note,
}

/// One entry in the project's traceability log. Append-only.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AuditEntry {
    /// Unix seconds since epoch.
    pub timestamp: u64,
    pub action: AuditAction,
    /// Human-readable description.
    pub description: String,
    /// Optional reference to a run id (for RunAdded etc.).
    pub run_id: Option<String>,
}

/// On-disk schema for `project.json`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProjectFile {
    pub version: String,
    pub name: String,
    /// Unix seconds since epoch.
    pub created_at: u64,
    pub modified_at: u64,
    /// Free-form project description / experiment notes.
    #[serde(default)]
    pub description: Option<String>,
    /// Originating user / workstation (best-effort, may be None).
    #[serde(default)]
    pub author: Option<String>,
    pub source_tiff_path: Option<PathBuf>,
    pub image_shape: Option<[usize; 2]>,
    pub frame_rate: Option<f32>,
    pub modality: Option<Modality>,
    #[serde(default)]
    pub frame_labels: Vec<FrameLabelDef>,
    pub roi_set: Option<RoiSet>,
    #[serde(default)]
    pub runs: Vec<RunMetadata>,
    /// Append-only traceability log: every project-level event in order.
    #[serde(default)]
    pub audit_log: Vec<AuditEntry>,
}

/// On-disk schema for `runs/<id>/run.json`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RunMetadata {
    pub id: String,
    pub name: String,
    pub created_at: u64,
    pub notes: Option<String>,
    pub motion_params: Option<MotionCorrectionParams>,
    pub delta_f_params: Option<DeltaFParams>,
    pub oasis_params: Option<OasisParams>,
    pub n_rois: usize,
    pub n_frames: usize,
    pub has_neuropil: bool,
    pub has_spikes: bool,
    pub has_events: bool,
    pub has_quality: bool,
    /// If a run has its own ROI set distinct from the project default,
    /// it is stored inline here.
    pub roi_set: Option<RoiSet>,
    /// Snapshot of the inputs that produced this run, for traceability.
    #[serde(default)]
    pub inputs: RunInputs,
    /// Tool version that produced this run.
    #[serde(default)]
    pub fiarfly_version: Option<String>,
}

/// Snapshot of where a run's data came from. Stored inside `RunMetadata`
/// so a reader can answer "how was this trace generated?" with no extra
/// state.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct RunInputs {
    /// Path to the source TIFF when the run was created (may be missing
    /// from this machine later — kept as text only).
    pub source_tiff_path: Option<PathBuf>,
    /// Whether the traces were extracted from motion-corrected data.
    pub used_motion_corrected: bool,
    /// Whether neuropil correction was applied.
    pub neuropil_correction_applied: bool,
    /// Frame rate used to compute time axis (Hz).
    pub frame_rate: Option<f32>,
    /// Modality at extraction time.
    pub modality: Option<Modality>,
}

// ─────────────────────────────────────────────────────────────────────────────
// Project handle (path + parsed file)
// ─────────────────────────────────────────────────────────────────────────────

#[derive(Debug)]
pub struct Project {
    path: PathBuf,
    file: ProjectFile,
}

impl Project {
    /// Create a new project bundle at `path` (must not already exist).
    pub fn create_new(
        path: impl Into<PathBuf>,
        name: impl Into<String>,
    ) -> Result<Self, FiarflyError> {
        let path = path.into();
        if path.exists() {
            return Err(FiarflyError::InvalidParameter(format!(
                "project path already exists: {}",
                path.display()
            )));
        }
        fs::create_dir_all(path.join(RUNS_DIR))?;
        let now = unix_secs_now();
        let name_str = name.into();
        let mut file = ProjectFile {
            version: PROJECT_VERSION.into(),
            name: name_str.clone(),
            created_at: now,
            modified_at: now,
            description: None,
            author: detect_author(),
            source_tiff_path: None,
            image_shape: None,
            frame_rate: None,
            modality: None,
            frame_labels: Vec::new(),
            roi_set: None,
            runs: Vec::new(),
            audit_log: Vec::new(),
        };
        file.audit_log.push(AuditEntry {
            timestamp: now,
            action: AuditAction::ProjectCreated,
            description: format!("Project \"{name_str}\" created."),
            run_id: None,
        });
        let project = Self { path, file };
        project.save()?;
        Ok(project)
    }

    /// Open an existing project bundle.
    pub fn open(path: impl Into<PathBuf>) -> Result<Self, FiarflyError> {
        let path = path.into();
        let json = fs::read_to_string(path.join(PROJECT_FILE))?;
        let file: ProjectFile = serde_json::from_str(&json)?;
        Ok(Self { path, file })
    }

    /// Persist the current `project.json`.
    pub fn save(&self) -> Result<(), FiarflyError> {
        fs::create_dir_all(self.path.join(RUNS_DIR))?;
        let json = serde_json::to_string_pretty(&self.file)?;
        fs::write(self.path.join(PROJECT_FILE), json)?;
        Ok(())
    }

    pub fn path(&self) -> &Path {
        &self.path
    }
    pub fn file(&self) -> &ProjectFile {
        &self.file
    }
    pub fn file_mut(&mut self) -> &mut ProjectFile {
        &mut self.file
    }

    /// Append an audit entry. Caller is responsible for `save()`.
    pub fn log_action(
        &mut self,
        action: AuditAction,
        description: impl Into<String>,
        run_id: Option<String>,
    ) {
        self.file.audit_log.push(AuditEntry {
            timestamp: unix_secs_now(),
            action,
            description: description.into(),
            run_id,
        });
        self.file.modified_at = unix_secs_now();
    }

    /// Append a new run: writes its directory, `run.json`, and parquet files,
    /// then updates and re-saves `project.json`. Returns the new run's id.
    pub fn add_run(&mut self, spec: NewRun<'_>) -> Result<String, FiarflyError> {
        let id = make_run_id(spec.name);
        let run_dir = self.path.join(RUNS_DIR).join(&id);
        fs::create_dir_all(&run_dir)?;

        let (n_rois, n_frames) = spec.raw_f.dim();

        let roi_set: &RoiSet = spec.roi_set.or(self.file.roi_set.as_ref()).ok_or_else(|| {
            FiarflyError::InvalidParameter(
                "ROI set required (in project or run spec) before adding a run".into(),
            )
        })?;

        // Traces parquet (reuses the existing tidy export path).
        let data = ExportData {
            roi_set,
            raw_f: spec.raw_f,
            delta_f: spec.delta_f,
            neuropil_f: spec.neuropil_f,
            deconvolved: spec.deconvolved,
            frame_rate: self.file.frame_rate,
            session_id: &id,
        };
        let mut df = to_dataframe(&data)?;
        write_parquet(&mut df, &run_dir.join(TRACES_FILE))?;

        if let Some(events) = spec.events {
            write_events_parquet(events, &run_dir.join(EVENTS_FILE))?;
        }
        if let Some(quality) = spec.quality {
            write_quality_parquet(roi_set, quality, &run_dir.join(QUALITY_FILE))?;
        }

        let meta = RunMetadata {
            id: id.clone(),
            name: spec.name.to_string(),
            created_at: unix_secs_now(),
            notes: spec.notes.map(|s| s.to_string()),
            motion_params: spec.motion_params.cloned(),
            delta_f_params: spec.delta_f_params.cloned(),
            oasis_params: spec.oasis_params.cloned(),
            n_rois,
            n_frames,
            has_neuropil: spec.neuropil_f.is_some(),
            has_spikes: spec.deconvolved.is_some(),
            has_events: spec.events.is_some(),
            has_quality: spec.quality.is_some(),
            roi_set: spec.roi_set.cloned(),
            inputs: spec.inputs,
            fiarfly_version: Some(env!("CARGO_PKG_VERSION").to_string()),
        };
        let run_json = serde_json::to_string_pretty(&meta)?;
        fs::write(run_dir.join(RUN_FILE), run_json)?;

        let summary = format!(
            "Run \"{}\" added: {} ROIs × {} frames{}{}",
            meta.name,
            n_rois,
            n_frames,
            if meta.has_neuropil { ", neuropil" } else { "" },
            if meta.has_spikes { ", deconvolved" } else { "" },
        );
        self.file.runs.push(meta);
        self.file.audit_log.push(AuditEntry {
            timestamp: unix_secs_now(),
            action: AuditAction::RunAdded,
            description: summary,
            run_id: Some(id.clone()),
        });
        self.file.modified_at = unix_secs_now();
        self.save()?;
        Ok(id)
    }

    /// Locate a run by id.
    pub fn run(&self, id: &str) -> Option<&RunMetadata> {
        self.file.runs.iter().find(|r| r.id == id)
    }

    /// Read the traces DataFrame for `id` back from disk.
    pub fn load_run_traces(&self, id: &str) -> Result<DataFrame, FiarflyError> {
        let path = self.path.join(RUNS_DIR).join(id).join(TRACES_FILE);
        read_parquet_file(&path)
    }

    /// Read the events DataFrame, if present.
    pub fn load_run_events(&self, id: &str) -> Result<Option<DataFrame>, FiarflyError> {
        let path = self.path.join(RUNS_DIR).join(id).join(EVENTS_FILE);
        if path.exists() {
            Ok(Some(read_parquet_file(&path)?))
        } else {
            Ok(None)
        }
    }

    /// Read the quality DataFrame, if present.
    pub fn load_run_quality(&self, id: &str) -> Result<Option<DataFrame>, FiarflyError> {
        let path = self.path.join(RUNS_DIR).join(id).join(QUALITY_FILE);
        if path.exists() {
            Ok(Some(read_parquet_file(&path)?))
        } else {
            Ok(None)
        }
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// New-run input bundle
// ─────────────────────────────────────────────────────────────────────────────

/// Inputs needed to write a new run into a project.
///
/// `roi_set` is optional — if `None` we use the project-level ROI set. At
/// least one must be present.
pub struct NewRun<'a> {
    pub name: &'a str,
    pub notes: Option<&'a str>,
    pub roi_set: Option<&'a RoiSet>,
    pub raw_f: &'a Array2<f32>,
    pub delta_f: &'a Array2<f32>,
    pub neuropil_f: Option<&'a Array2<f32>>,
    pub deconvolved: Option<&'a OasisResult>,
    pub events: Option<&'a [Vec<CaEvent>]>,
    pub quality: Option<&'a [RoiQuality]>,
    pub motion_params: Option<&'a MotionCorrectionParams>,
    pub delta_f_params: Option<&'a DeltaFParams>,
    pub oasis_params: Option<&'a OasisParams>,
    /// Snapshot of the inputs used to produce these traces.
    pub inputs: RunInputs,
}

// ─────────────────────────────────────────────────────────────────────────────
// Internals
// ─────────────────────────────────────────────────────────────────────────────

fn detect_author() -> Option<String> {
    let user = std::env::var("USER")
        .or_else(|_| std::env::var("USERNAME"))
        .ok()?;
    let host = std::env::var("HOSTNAME")
        .or_else(|_| std::env::var("COMPUTERNAME"))
        .ok();
    Some(match host {
        Some(h) if !h.is_empty() => format!("{user}@{h}"),
        _ => user,
    })
}

fn unix_secs_now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

fn make_run_id(name: &str) -> String {
    let ms = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis())
        .unwrap_or(0);
    let slug = sanitize_slug(name);
    if slug.is_empty() {
        format!("run_{ms}")
    } else {
        format!("run_{ms}_{slug}")
    }
}

fn sanitize_slug(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut last_underscore = false;
    for ch in s.chars() {
        if ch.is_ascii_alphanumeric() {
            out.push(ch.to_ascii_lowercase());
            last_underscore = false;
        } else if !last_underscore {
            out.push('_');
            last_underscore = true;
        }
    }
    out.trim_matches('_').to_string()
}

fn read_parquet_file(path: &Path) -> Result<DataFrame, FiarflyError> {
    let file = fs::File::open(path)?;
    ParquetReader::new(file)
        .finish()
        .map_err(|e| FiarflyError::Export(format!("parquet read {}: {e}", path.display())))
}

fn write_events_parquet(events_per_roi: &[Vec<CaEvent>], path: &Path) -> Result<(), FiarflyError> {
    let total: usize = events_per_roi.iter().map(|v| v.len()).sum();
    let mut roi_idx: Vec<u32> = Vec::with_capacity(total);
    let mut event_idx: Vec<u32> = Vec::with_capacity(total);
    let mut onset: Vec<u32> = Vec::with_capacity(total);
    let mut peak: Vec<u32> = Vec::with_capacity(total);
    let mut amp: Vec<f32> = Vec::with_capacity(total);
    let mut offset: Vec<u32> = Vec::with_capacity(total);
    let mut duration: Vec<u32> = Vec::with_capacity(total);
    let mut half_decay: Vec<Option<u32>> = Vec::with_capacity(total);

    for (ri, events) in events_per_roi.iter().enumerate() {
        for (ei, ev) in events.iter().enumerate() {
            roi_idx.push(ri as u32);
            event_idx.push(ei as u32);
            onset.push(ev.onset_frame as u32);
            peak.push(ev.peak_frame as u32);
            amp.push(ev.amplitude);
            offset.push(ev.offset_frame as u32);
            duration.push(ev.duration_frames as u32);
            half_decay.push(ev.half_decay_frame.map(|v| v as u32));
        }
    }

    let series: Vec<Series> = vec![
        Series::new("roi_idx", roi_idx),
        Series::new("event_idx", event_idx),
        Series::new("onset_frame", onset),
        Series::new("peak_frame", peak),
        Series::new("amplitude", amp),
        Series::new("offset_frame", offset),
        Series::new("duration_frames", duration),
        Series::new("half_decay_frame", half_decay),
    ];
    let mut df = DataFrame::new(series).map_err(|e| FiarflyError::Export(e.to_string()))?;
    write_parquet(&mut df, path)?;
    Ok(())
}

fn write_quality_parquet(
    roi_set: &RoiSet,
    quality: &[RoiQuality],
    path: &Path,
) -> Result<(), FiarflyError> {
    if quality.len() != roi_set.rois.len() {
        return Err(FiarflyError::DimensionMismatch(format!(
            "quality has {} entries but roi_set has {} ROIs",
            quality.len(),
            roi_set.rois.len()
        )));
    }

    let n = quality.len();
    let mut roi_id: Vec<String> = Vec::with_capacity(n);
    let mut snr: Vec<f32> = Vec::with_capacity(n);
    let mut skewness: Vec<f32> = Vec::with_capacity(n);
    let mut active_fraction: Vec<f32> = Vec::with_capacity(n);
    let mut peak_dff: Vec<f32> = Vec::with_capacity(n);
    let mut noise_std: Vec<f32> = Vec::with_capacity(n);
    let mut passes: Vec<bool> = Vec::with_capacity(n);

    for (roi, q) in roi_set.rois.iter().zip(quality.iter()) {
        roi_id.push(roi.id.clone());
        snr.push(q.snr);
        skewness.push(q.skewness);
        active_fraction.push(q.active_fraction);
        peak_dff.push(q.peak_dff);
        noise_std.push(q.noise_std);
        passes.push(q.passes);
    }

    let series: Vec<Series> = vec![
        Series::new("roi_id", roi_id),
        Series::new("snr", snr),
        Series::new("skewness", skewness),
        Series::new("active_fraction", active_fraction),
        Series::new("peak_dff", peak_dff),
        Series::new("noise_std", noise_std),
        Series::new("passes", passes),
    ];
    let mut df = DataFrame::new(series).map_err(|e| FiarflyError::Export(e.to_string()))?;
    write_parquet(&mut df, path)?;
    Ok(())
}

// ─────────────────────────────────────────────────────────────────────────────
// Tests
// ─────────────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::roi::Roi;
    use ndarray::arr2;

    fn tmp_path(stem: &str) -> PathBuf {
        let mut p = std::env::temp_dir();
        let pid = std::process::id();
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0);
        p.push(format!("fiarfly_test_{stem}_{pid}_{nanos}.fiarproj"));
        p
    }

    fn small_roi_set() -> RoiSet {
        let mut set = RoiSet::new("test", 16, 16);
        set.add_roi(Roi {
            id: String::new(),
            label: "A".into(),
            group: Some("g1".into()),
            color: [255, 0, 0],
            vertices: vec![[2.0, 2.0], [6.0, 2.0], [6.0, 6.0], [2.0, 6.0]],
        });
        set.add_roi(Roi {
            id: String::new(),
            label: "B".into(),
            group: Some("g2".into()),
            color: [0, 255, 0],
            vertices: vec![[8.0, 8.0], [12.0, 8.0], [12.0, 12.0], [8.0, 12.0]],
        });
        set
    }

    fn cleanup(p: &Path) {
        let _ = fs::remove_dir_all(p);
    }

    #[test]
    fn create_open_save_roundtrip() {
        let p = tmp_path("create_open");
        let mut proj = Project::create_new(&p, "study A").expect("create");
        proj.file_mut().frame_rate = Some(30.0);
        proj.file_mut().image_shape = Some([16, 16]);
        proj.file_mut().modality = Some(Modality::TwoPhotonInVivo);
        proj.file_mut().roi_set = Some(small_roi_set());
        proj.file_mut().frame_labels.push(FrameLabelDef {
            name: "Baseline".into(),
            start: 0,
            end: 49,
        });
        proj.save().expect("save");

        let reopened = Project::open(&p).expect("reopen");
        assert_eq!(reopened.file().name, "study A");
        assert_eq!(reopened.file().version, PROJECT_VERSION);
        assert_eq!(reopened.file().frame_rate, Some(30.0));
        assert_eq!(reopened.file().modality, Some(Modality::TwoPhotonInVivo));
        let labels = &reopened.file().frame_labels;
        assert_eq!(labels.len(), 1);
        assert_eq!(labels[0].name, "Baseline");
        let rs = reopened.file().roi_set.as_ref().expect("roi set");
        assert_eq!(rs.rois.len(), 2);
        assert_eq!(rs.rois[0].label, "A");

        cleanup(&p);
    }

    #[test]
    fn create_rejects_existing_path() {
        let p = tmp_path("exists");
        fs::create_dir_all(&p).unwrap();
        let err = Project::create_new(&p, "x").unwrap_err();
        assert!(matches!(err, FiarflyError::InvalidParameter(_)));
        cleanup(&p);
    }

    #[test]
    fn add_run_and_load_traces() {
        let p = tmp_path("addrun");
        let mut proj = Project::create_new(&p, "with run").expect("create");
        proj.file_mut().frame_rate = Some(10.0);
        proj.file_mut().roi_set = Some(small_roi_set());

        // 2 ROIs × 4 frames
        let raw_f = arr2(&[[1.0_f32, 2.0, 3.0, 4.0], [5.0, 6.0, 7.0, 8.0]]);
        let delta_f = arr2(&[[0.1_f32, 0.2, 0.3, 0.4], [0.5, 0.6, 0.7, 0.8]]);

        let id = proj
            .add_run(NewRun {
                name: "first",
                notes: Some("smoke run"),
                roi_set: None,
                raw_f: &raw_f,
                delta_f: &delta_f,
                neuropil_f: None,
                deconvolved: None,
                events: None,
                quality: None,
                motion_params: Some(&MotionCorrectionParams::default()),
                delta_f_params: Some(&DeltaFParams::default()),
                oasis_params: None,
                inputs: RunInputs {
                    source_tiff_path: Some(PathBuf::from("/data/recording.tif")),
                    used_motion_corrected: true,
                    neuropil_correction_applied: false,
                    frame_rate: Some(10.0),
                    modality: Some(Modality::TwoPhotonInVivo),
                },
            })
            .expect("add_run");

        assert_eq!(proj.file().runs.len(), 1);
        let meta = proj.run(&id).expect("run meta");
        assert_eq!(meta.n_rois, 2);
        assert_eq!(meta.n_frames, 4);
        assert!(!meta.has_neuropil);
        assert!(!meta.has_quality);
        assert_eq!(meta.notes.as_deref(), Some("smoke run"));

        // Round-trip through disk.
        let reopened = Project::open(&p).expect("reopen");
        assert_eq!(reopened.file().runs.len(), 1);
        let df = reopened.load_run_traces(&id).expect("load traces");
        assert_eq!(df.height(), 2 * 4);
        for col in ["roi_id", "frame_idx", "raw_f", "delta_f_over_f", "time_s"] {
            assert!(
                df.column(col).is_ok(),
                "missing column {col}; got {:?}",
                df.get_column_names()
            );
        }

        cleanup(&p);
    }

    #[test]
    fn add_run_without_roiset_fails() {
        let p = tmp_path("noroi");
        let mut proj = Project::create_new(&p, "no rois").expect("create");
        let raw_f = arr2(&[[1.0_f32, 2.0]]);
        let delta_f = arr2(&[[0.1_f32, 0.2]]);
        let err = proj
            .add_run(NewRun {
                name: "x",
                notes: None,
                roi_set: None,
                raw_f: &raw_f,
                delta_f: &delta_f,
                neuropil_f: None,
                deconvolved: None,
                events: None,
                quality: None,
                motion_params: None,
                delta_f_params: None,
                oasis_params: None,
                inputs: RunInputs::default(),
            })
            .unwrap_err();
        assert!(matches!(err, FiarflyError::InvalidParameter(_)));
        cleanup(&p);
    }

    #[test]
    fn audit_log_records_lifecycle_events() {
        let p = tmp_path("audit");
        let mut proj = Project::create_new(&p, "audited").expect("create");
        proj.file_mut().roi_set = Some(small_roi_set());
        proj.file_mut().frame_rate = Some(30.0);

        // Manual entry.
        proj.log_action(AuditAction::Note, "user clicked something", None);
        proj.save().expect("save");

        let raw_f = arr2(&[[1.0_f32, 2.0], [3.0, 4.0]]);
        let delta_f = arr2(&[[0.0_f32, 0.1], [0.0, 0.2]]);
        let id = proj
            .add_run(NewRun {
                name: "trace v1",
                notes: None,
                roi_set: None,
                raw_f: &raw_f,
                delta_f: &delta_f,
                neuropil_f: None,
                deconvolved: None,
                events: None,
                quality: None,
                motion_params: None,
                delta_f_params: None,
                oasis_params: None,
                inputs: RunInputs {
                    source_tiff_path: Some(PathBuf::from("/data/x.tif")),
                    used_motion_corrected: true,
                    neuropil_correction_applied: false,
                    frame_rate: Some(30.0),
                    modality: Some(Modality::OnePhotonInVivo),
                },
            })
            .expect("add_run");

        // Reopen and verify the audit log persisted in order.
        let reopened = Project::open(&p).expect("reopen");
        let log = &reopened.file().audit_log;
        assert!(log.len() >= 3, "expected >=3 entries, got {}", log.len());
        assert_eq!(log[0].action, AuditAction::ProjectCreated);
        assert_eq!(log[1].action, AuditAction::Note);
        assert_eq!(log.last().unwrap().action, AuditAction::RunAdded);
        assert_eq!(log.last().unwrap().run_id.as_deref(), Some(id.as_str()));
        // Timestamps are non-decreasing.
        for w in log.windows(2) {
            assert!(w[0].timestamp <= w[1].timestamp);
        }

        // Per-run inputs round-tripped.
        let run = reopened.run(&id).expect("run meta");
        assert_eq!(
            run.inputs.source_tiff_path.as_deref(),
            Some(Path::new("/data/x.tif"))
        );
        assert!(run.inputs.used_motion_corrected);
        assert_eq!(run.inputs.modality, Some(Modality::OnePhotonInVivo));
        assert!(run.fiarfly_version.is_some());

        cleanup(&p);
    }

    #[test]
    fn slug_sanitizer_handles_specials() {
        assert_eq!(sanitize_slug("Baseline 1"), "baseline_1");
        assert_eq!(sanitize_slug(" hello/world! "), "hello_world");
        assert_eq!(sanitize_slug(""), "");
        assert_eq!(sanitize_slug("///"), "");
    }
}
