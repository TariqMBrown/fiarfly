//! PyO3 Python bindings for fiarfly-core.
//!
//! Exposes the v0.2 surface — project bundles (PR2), analysis metrics
//! (PR4), and statistical tests (PR6) — to Python users.

// PyO3's `#[pyfunction]`/`#[pymethods]` macros emit a `.into()` on each
// function's `Result` to coerce the error into `PyErr`. Our functions already
// return `PyErr`, so clippy flags the macro-generated conversion against every
// signature. It's a false positive in expanded code we don't control.
#![allow(clippy::useless_conversion)]

use std::path::PathBuf;

use numpy::PyReadonlyArray2;
use pyo3::exceptions::{PyIOError, PyValueError};
use pyo3::prelude::*;
use pyo3::types::PyDict;

use fiarfly_core::analysis::{
    auc_trapezoid, dff_at_frame, dff_window_mean, dff_window_peak, latency_to_peak_frames,
    rise_time_frames, Window,
};
use fiarfly_core::io::{Project, ProjectFile, RunMetadata};
use fiarfly_core::stats::{
    benjamini_hochberg as bh_core, bonferroni as bonferroni_core, kruskal_wallis,
    mann_whitney_u, one_way_anova, paired_t_test, run_test, welch_t_test,
    wilcoxon_signed_rank, Comparison, TestKind, TestResult,
};
use fiarfly_core::FiarflyError;

// ─────────────────────────────────────────────────────────────────────────
// Module entry
// ─────────────────────────────────────────────────────────────────────────

#[pymodule]
fn fiarfly(m: &Bound<'_, PyModule>) -> PyResult<()> {
    // Project bundle (PR2 / PR3).
    m.add_class::<PyProject>()?;
    m.add_class::<PyRunMetadata>()?;

    // Analysis metrics (PR4).
    m.add_function(wrap_pyfunction!(window_mean, m)?)?;
    m.add_function(wrap_pyfunction!(window_peak, m)?)?;
    m.add_function(wrap_pyfunction!(window_value_at, m)?)?;
    m.add_function(wrap_pyfunction!(auc, m)?)?;
    m.add_function(wrap_pyfunction!(latency_to_peak, m)?)?;
    m.add_function(wrap_pyfunction!(rise_time, m)?)?;

    // Statistics (PR6).
    m.add_class::<PyTestResult>()?;
    m.add_function(wrap_pyfunction!(welch_t, m)?)?;
    m.add_function(wrap_pyfunction!(paired_t, m)?)?;
    m.add_function(wrap_pyfunction!(mann_whitney, m)?)?;
    m.add_function(wrap_pyfunction!(wilcoxon, m)?)?;
    m.add_function(wrap_pyfunction!(anova, m)?)?;
    m.add_function(wrap_pyfunction!(kruskal, m)?)?;
    m.add_function(wrap_pyfunction!(test_compare, m)?)?;
    m.add_function(wrap_pyfunction!(bonferroni, m)?)?;
    m.add_function(wrap_pyfunction!(benjamini_hochberg, m)?)?;

    m.add("__version__", env!("CARGO_PKG_VERSION"))?;
    Ok(())
}

// ─────────────────────────────────────────────────────────────────────────
// Helpers
// ─────────────────────────────────────────────────────────────────────────

fn map_err(e: FiarflyError) -> PyErr {
    match e {
        FiarflyError::Io(io)              => PyIOError::new_err(io.to_string()),
        FiarflyError::Serialization(se)   => PyValueError::new_err(se.to_string()),
        other                             => PyValueError::new_err(other.to_string()),
    }
}

/// Resolve a (start, end) frame range against the data length, building
/// either a frame- or seconds-based core `Window`.
fn build_window(
    start: f64,
    end: f64,
    frame_rate: Option<f32>,
    seconds: bool,
) -> Window {
    if seconds {
        // frame_rate validated downstream by Window::resolve.
        Window::seconds(start as f32, end as f32, frame_rate.unwrap_or(0.0))
    } else {
        Window::frames(start as usize, end as usize)
    }
}

fn resolve_window(
    n_frames: usize,
    start: f64,
    end: f64,
    frame_rate: Option<f32>,
    seconds: bool,
) -> PyResult<fiarfly_core::analysis::ResolvedRange> {
    let w = build_window(start, end, frame_rate, seconds);
    w.resolve(n_frames).map_err(map_err)
}

// ─────────────────────────────────────────────────────────────────────────
// Project bindings (PR2 / PR3)
// ─────────────────────────────────────────────────────────────────────────

/// Lightweight read-only handle to a `.fiarproj` directory bundle.
///
/// Use `Project.open(path)` for an existing bundle. Call `runs` to get
/// the list of run summaries, and `load_run_traces(run_id)` to read a
/// run's parquet back as a list of dict rows.
#[pyclass(name = "Project")]
pub struct PyProject {
    inner: Project,
}

#[pymethods]
impl PyProject {
    #[staticmethod]
    fn open(path: &str) -> PyResult<Self> {
        let inner = Project::open(PathBuf::from(path)).map_err(map_err)?;
        Ok(Self { inner })
    }

    #[staticmethod]
    fn create(path: &str, name: &str) -> PyResult<Self> {
        let inner = Project::create_new(PathBuf::from(path), name).map_err(map_err)?;
        Ok(Self { inner })
    }

    fn save(&self) -> PyResult<()> {
        self.inner.save().map_err(map_err)
    }

    #[getter]
    fn name(&self) -> &str {
        &self.inner.file().name
    }
    #[getter]
    fn version(&self) -> &str {
        &self.inner.file().version
    }
    #[getter]
    fn path(&self) -> String {
        self.inner.path().display().to_string()
    }
    #[getter]
    fn frame_rate(&self) -> Option<f32> {
        self.inner.file().frame_rate
    }
    #[getter]
    fn description(&self) -> Option<String> {
        self.inner.file().description.clone()
    }
    #[getter]
    fn author(&self) -> Option<String> {
        self.inner.file().author.clone()
    }
    #[getter]
    fn created_at(&self) -> u64 {
        self.inner.file().created_at
    }
    #[getter]
    fn modified_at(&self) -> u64 {
        self.inner.file().modified_at
    }
    #[getter]
    fn source_tiff_path(&self) -> Option<String> {
        self.inner
            .file()
            .source_tiff_path
            .as_ref()
            .map(|p| p.display().to_string())
    }

    /// One dict per run (id, name, n_rois, n_frames, created_at, …).
    #[getter]
    fn runs(&self, py: Python<'_>) -> PyResult<Vec<Py<PyRunMetadata>>> {
        self.inner
            .file()
            .runs
            .iter()
            .map(|r| Py::new(py, PyRunMetadata::from(r)))
            .collect()
    }

    /// Audit log as a list of dicts (timestamp, action, description, run_id).
    fn audit_log<'py>(&self, py: Python<'py>) -> PyResult<Vec<Bound<'py, PyDict>>> {
        let mut out = Vec::with_capacity(self.inner.file().audit_log.len());
        for e in &self.inner.file().audit_log {
            let d = PyDict::new_bound(py);
            d.set_item("timestamp", e.timestamp)?;
            d.set_item("action", format!("{:?}", e.action))?;
            d.set_item("description", &e.description)?;
            d.set_item("run_id", e.run_id.clone())?;
            out.push(d);
        }
        Ok(out)
    }

    /// Read the traces parquet for `run_id` as a list of dict rows.
    /// (Python users typically convert this to a pandas/polars DataFrame.)
    fn load_run_traces<'py>(
        &self,
        py: Python<'py>,
        run_id: &str,
    ) -> PyResult<Vec<Bound<'py, PyDict>>> {
        let df = self.inner.load_run_traces(run_id).map_err(map_err)?;
        dataframe_to_dict_rows(py, &df)
    }

    fn load_run_events<'py>(
        &self,
        py: Python<'py>,
        run_id: &str,
    ) -> PyResult<Option<Vec<Bound<'py, PyDict>>>> {
        match self.inner.load_run_events(run_id).map_err(map_err)? {
            Some(df) => Ok(Some(dataframe_to_dict_rows(py, &df)?)),
            None => Ok(None),
        }
    }

    fn load_run_quality<'py>(
        &self,
        py: Python<'py>,
        run_id: &str,
    ) -> PyResult<Option<Vec<Bound<'py, PyDict>>>> {
        match self.inner.load_run_quality(run_id).map_err(map_err)? {
            Some(df) => Ok(Some(dataframe_to_dict_rows(py, &df)?)),
            None => Ok(None),
        }
    }

    fn __repr__(&self) -> String {
        let f: &ProjectFile = self.inner.file();
        format!(
            "<Project name=\"{}\" runs={} version={}>",
            f.name,
            f.runs.len(),
            f.version
        )
    }
}

#[pyclass(name = "RunMetadata")]
#[derive(Clone)]
pub struct PyRunMetadata {
    #[pyo3(get)] pub id: String,
    #[pyo3(get)] pub name: String,
    #[pyo3(get)] pub created_at: u64,
    #[pyo3(get)] pub n_rois: usize,
    #[pyo3(get)] pub n_frames: usize,
    #[pyo3(get)] pub has_neuropil: bool,
    #[pyo3(get)] pub has_spikes: bool,
    #[pyo3(get)] pub has_events: bool,
    #[pyo3(get)] pub has_quality: bool,
    #[pyo3(get)] pub notes: Option<String>,
    #[pyo3(get)] pub fiarfly_version: Option<String>,
    #[pyo3(get)] pub source_tiff_path: Option<String>,
    #[pyo3(get)] pub used_motion_corrected: bool,
    #[pyo3(get)] pub neuropil_correction_applied: bool,
    #[pyo3(get)] pub frame_rate: Option<f32>,
}

impl From<&RunMetadata> for PyRunMetadata {
    fn from(r: &RunMetadata) -> Self {
        Self {
            id: r.id.clone(),
            name: r.name.clone(),
            created_at: r.created_at,
            n_rois: r.n_rois,
            n_frames: r.n_frames,
            has_neuropil: r.has_neuropil,
            has_spikes: r.has_spikes,
            has_events: r.has_events,
            has_quality: r.has_quality,
            notes: r.notes.clone(),
            fiarfly_version: r.fiarfly_version.clone(),
            source_tiff_path: r
                .inputs
                .source_tiff_path
                .as_ref()
                .map(|p| p.display().to_string()),
            used_motion_corrected: r.inputs.used_motion_corrected,
            neuropil_correction_applied: r.inputs.neuropil_correction_applied,
            frame_rate: r.inputs.frame_rate,
        }
    }
}

#[pymethods]
impl PyRunMetadata {
    fn __repr__(&self) -> String {
        format!(
            "<RunMetadata id={} name=\"{}\" rois={} frames={}>",
            self.id, self.name, self.n_rois, self.n_frames
        )
    }
}

fn dataframe_to_dict_rows<'py>(
    py: Python<'py>,
    df: &polars::prelude::DataFrame,
) -> PyResult<Vec<Bound<'py, PyDict>>> {
    use polars::prelude::AnyValue;
    let names: Vec<String> = df
        .get_column_names()
        .iter()
        .map(|s: &&str| s.to_string())
        .collect();
    let h = df.height();
    let mut rows = Vec::with_capacity(h);
    for row_idx in 0..h {
        let d = PyDict::new_bound(py);
        for (col_idx, name) in names.iter().enumerate() {
            let v = df.get_columns()[col_idx].get(row_idx).map_err(|e| {
                PyValueError::new_err(format!("DataFrame access error: {e}"))
            })?;
            let py_val: PyObject = match v {
                AnyValue::Null         => py.None(),
                AnyValue::Boolean(b)   => b.into_py(py),
                AnyValue::String(s)    => s.into_py(py),
                AnyValue::Int8(i)      => i.into_py(py),
                AnyValue::Int16(i)     => i.into_py(py),
                AnyValue::Int32(i)     => i.into_py(py),
                AnyValue::Int64(i)     => i.into_py(py),
                AnyValue::UInt8(i)     => i.into_py(py),
                AnyValue::UInt16(i)    => i.into_py(py),
                AnyValue::UInt32(i)    => i.into_py(py),
                AnyValue::UInt64(i)    => i.into_py(py),
                AnyValue::Float32(f)   => f.into_py(py),
                AnyValue::Float64(f)   => f.into_py(py),
                other                  => format!("{other}").into_py(py),
            };
            d.set_item(name, py_val)?;
        }
        rows.push(d);
    }
    Ok(rows)
}

// ─────────────────────────────────────────────────────────────────────────
// Analysis bindings (PR4)
// ─────────────────────────────────────────────────────────────────────────

/// Mean of `data[:, start:end]` per row.
///
/// `data` is a 2-D float32 array of shape `(n_rois, n_frames)`.
/// If `seconds=True`, `start` / `end` are in seconds and `frame_rate`
/// must be set; otherwise they are frame indices.
#[pyfunction]
#[pyo3(signature = (data, start, end, frame_rate=None, seconds=false))]
fn window_mean(
    data: PyReadonlyArray2<f32>,
    start: f64,
    end: f64,
    frame_rate: Option<f32>,
    seconds: bool,
) -> PyResult<Vec<f32>> {
    let arr = data.as_array();
    let r = resolve_window(arr.ncols(), start, end, frame_rate, seconds)?;
    Ok(dff_window_mean(arr, r))
}

#[pyfunction]
#[pyo3(signature = (data, start, end, frame_rate=None, seconds=false))]
fn window_peak(
    data: PyReadonlyArray2<f32>,
    start: f64,
    end: f64,
    frame_rate: Option<f32>,
    seconds: bool,
) -> PyResult<Vec<f32>> {
    let arr = data.as_array();
    let r = resolve_window(arr.ncols(), start, end, frame_rate, seconds)?;
    Ok(dff_window_peak(arr, r))
}

#[pyfunction]
fn window_value_at(data: PyReadonlyArray2<f32>, frame: usize) -> PyResult<Vec<f32>> {
    let arr = data.as_array();
    if frame >= arr.ncols() {
        return Err(PyValueError::new_err(format!(
            "frame {frame} out of range (ncols={})",
            arr.ncols()
        )));
    }
    Ok(dff_at_frame(arr, frame))
}

/// Trapezoidal AUC. Units: ΔF/F·seconds when frame_rate is given,
/// ΔF/F·frames otherwise.
#[pyfunction]
#[pyo3(signature = (data, start, end, frame_rate=None, seconds=false))]
fn auc(
    data: PyReadonlyArray2<f32>,
    start: f64,
    end: f64,
    frame_rate: Option<f32>,
    seconds: bool,
) -> PyResult<Vec<f32>> {
    let arr = data.as_array();
    let r = resolve_window(arr.ncols(), start, end, frame_rate, seconds)?;
    Ok(auc_trapezoid(arr, r, frame_rate))
}

#[pyfunction]
#[pyo3(signature = (data, start, end, frame_rate=None, seconds=false))]
fn latency_to_peak(
    data: PyReadonlyArray2<f32>,
    start: f64,
    end: f64,
    frame_rate: Option<f32>,
    seconds: bool,
) -> PyResult<Vec<f32>> {
    let arr = data.as_array();
    let r = resolve_window(arr.ncols(), start, end, frame_rate, seconds)?;
    Ok(latency_to_peak_frames(arr, r, frame_rate))
}

/// 10–90% rise time (frames or seconds, matching `frame_rate`).
#[pyfunction]
#[pyo3(signature = (data, start, end, frame_rate=None, seconds=false, lo_frac=0.1, hi_frac=0.9))]
fn rise_time(
    data: PyReadonlyArray2<f32>,
    start: f64,
    end: f64,
    frame_rate: Option<f32>,
    seconds: bool,
    lo_frac: f32,
    hi_frac: f32,
) -> PyResult<Vec<f32>> {
    let arr = data.as_array();
    let r = resolve_window(arr.ncols(), start, end, frame_rate, seconds)?;
    Ok(rise_time_frames(arr, r, frame_rate, lo_frac, hi_frac))
}

// ─────────────────────────────────────────────────────────────────────────
// Stats bindings (PR6)
// ─────────────────────────────────────────────────────────────────────────

/// Result of a single statistical test.
#[pyclass(name = "TestResult")]
#[derive(Clone)]
pub struct PyTestResult {
    #[pyo3(get)] pub test_name: String,
    #[pyo3(get)] pub statistic: f64,
    #[pyo3(get)] pub p_value: f64,
    #[pyo3(get)] pub n: Vec<usize>,
    #[pyo3(get)] pub effect_size: Option<f64>,
    #[pyo3(get)] pub effect_kind: Option<String>,
    #[pyo3(get)] pub note: Option<String>,
}

impl From<TestResult> for PyTestResult {
    fn from(r: TestResult) -> Self {
        Self {
            test_name: r.test_name.to_string(),
            statistic: r.statistic,
            p_value: r.p_value,
            n: r.n,
            effect_size: r.effect_size,
            effect_kind: r.effect_kind.map(|s| s.to_string()),
            note: r.note,
        }
    }
}

#[pymethods]
impl PyTestResult {
    fn __repr__(&self) -> String {
        format!(
            "<TestResult {} stat={:.4} p={:.4e} n={:?}>",
            self.test_name, self.statistic, self.p_value, self.n
        )
    }
}

#[pyfunction]
fn welch_t(a: Vec<f32>, b: Vec<f32>) -> PyResult<PyTestResult> {
    welch_t_test(&a, &b).map(Into::into).map_err(map_err)
}

#[pyfunction]
fn paired_t(a: Vec<f32>, b: Vec<f32>) -> PyResult<PyTestResult> {
    paired_t_test(&a, &b).map(Into::into).map_err(map_err)
}

#[pyfunction]
fn mann_whitney(a: Vec<f32>, b: Vec<f32>) -> PyResult<PyTestResult> {
    mann_whitney_u(&a, &b).map(Into::into).map_err(map_err)
}

#[pyfunction]
fn wilcoxon(a: Vec<f32>, b: Vec<f32>) -> PyResult<PyTestResult> {
    wilcoxon_signed_rank(&a, &b).map(Into::into).map_err(map_err)
}

#[pyfunction]
fn anova(groups: Vec<Vec<f32>>) -> PyResult<PyTestResult> {
    let refs: Vec<&[f32]> = groups.iter().map(|g| g.as_slice()).collect();
    one_way_anova(&refs).map(Into::into).map_err(map_err)
}

#[pyfunction]
fn kruskal(groups: Vec<Vec<f32>>) -> PyResult<PyTestResult> {
    let refs: Vec<&[f32]> = groups.iter().map(|g| g.as_slice()).collect();
    kruskal_wallis(&refs).map(Into::into).map_err(map_err)
}

/// High-level dispatcher: pass groups + comparison kind + test family.
///
/// `comparison`: "paired" | "unpaired" | "multi"
/// `kind`:       "auto"   | "parametric" | "nonparametric"
#[pyfunction]
#[pyo3(signature = (groups, comparison="auto", kind="auto"))]
fn test_compare(
    groups: Vec<Vec<f32>>,
    comparison: &str,
    kind: &str,
) -> PyResult<PyTestResult> {
    let comp = match comparison {
        "paired"            => Comparison::PairedTwo,
        "unpaired" | "auto" => Comparison::UnpairedTwo,
        "multi"             => Comparison::Multi,
        other => return Err(PyValueError::new_err(format!(
            "comparison must be 'paired' | 'unpaired' | 'multi', got {other:?}"
        ))),
    };
    let test_kind = match kind {
        "auto"          => TestKind::Auto,
        "parametric"    => TestKind::Parametric,
        "nonparametric" => TestKind::NonParametric,
        other => return Err(PyValueError::new_err(format!(
            "kind must be 'auto' | 'parametric' | 'nonparametric', got {other:?}"
        ))),
    };
    let refs: Vec<&[f32]> = groups.iter().map(|g| g.as_slice()).collect();
    run_test(&refs, comp, test_kind).map(Into::into).map_err(map_err)
}

#[pyfunction]
fn bonferroni(p_values: Vec<f64>) -> Vec<f64> {
    bonferroni_core(&p_values)
}

#[pyfunction]
fn benjamini_hochberg(p_values: Vec<f64>) -> Vec<f64> {
    bh_core(&p_values)
}
