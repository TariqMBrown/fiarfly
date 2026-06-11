"""
fiarfly — Python interface to the FIARfly calcium imaging analysis engine.

The core computation runs in Rust (fiarfly-core) and is exposed here via
PyO3 bindings (the native `fiarfly` extension built by maturin).

Public surface (v0.2):

  Project bundles
    - `fiarfly.Project.open(path)` / `Project.create(path, name)`
    - `Project.runs`, `Project.audit_log()`, `Project.load_run_traces(id)`

  Analysis metrics (operate on a 2-D float32 array `[n_rois, n_frames]`)
    - `fiarfly.window_mean(data, start, end, ...)`
    - `fiarfly.window_peak(data, start, end, ...)`
    - `fiarfly.window_value_at(data, frame)`
    - `fiarfly.auc(data, start, end, frame_rate=...)`
    - `fiarfly.latency_to_peak(data, start, end, frame_rate=...)`
    - `fiarfly.rise_time(data, start, end, frame_rate=..., lo_frac=0.1, hi_frac=0.9)`

  Statistics
    - `fiarfly.welch_t(a, b)` / `fiarfly.paired_t(a, b)`
    - `fiarfly.mann_whitney(a, b)` / `fiarfly.wilcoxon(a, b)`
    - `fiarfly.anova(groups)` / `fiarfly.kruskal(groups)`
    - `fiarfly.test_compare(groups, comparison=..., kind=...)`
    - `fiarfly.bonferroni(ps)` / `fiarfly.benjamini_hochberg(ps)`
    - `fiarfly.TestResult`
"""

# Native PyO3 extension surface. After `maturin develop`, this import
# pulls in the Rust-defined classes / functions.
try:
    from fiarfly.fiarfly import (  # type: ignore[import]
        Project,
        RunMetadata,
        TestResult,
        window_mean,
        window_peak,
        window_value_at,
        auc,
        latency_to_peak,
        rise_time,
        welch_t,
        paired_t,
        mann_whitney,
        wilcoxon,
        anova,
        kruskal,
        test_compare,
        bonferroni,
        benjamini_hochberg,
        __version__,
    )
    _NATIVE_AVAILABLE = True
except ImportError:
    # Allow `import fiarfly` to succeed in source checkouts where the
    # native extension hasn't been built yet (e.g. doc generation).
    _NATIVE_AVAILABLE = False
    __version__ = "0.2.0"

__all__ = [
    "Project",
    "RunMetadata",
    "TestResult",
    "window_mean",
    "window_peak",
    "window_value_at",
    "auc",
    "latency_to_peak",
    "rise_time",
    "welch_t",
    "paired_t",
    "mann_whitney",
    "wilcoxon",
    "anova",
    "kruskal",
    "test_compare",
    "bonferroni",
    "benjamini_hochberg",
]
