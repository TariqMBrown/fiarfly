# CLAUDE.md — FIARfly

A user-friendly **calcium imaging analysis** tool: a Rust core engine for speed and
memory safety, an egui desktop GUI, and a Python wrapper (via maturin/`uv`) exposing
the same pipeline for scripting and notebooks. See [README.md](README.md),
[ARCHITECTURE.md](ARCHITECTURE.md), and [IMPLEMENTATION_GUIDE.md](IMPLEMENTATION_GUIDE.md).

Repo lives at `~/Documents/VsCode/fiarfly` (moved 2026-05-07 from `~/fiarfly`).

## Pipeline

Linear: **Import (.tif/.tiff) → Motion correction → ROI → Signal extraction (ΔF/F)
→ Analysis → Stats → Export**, plus a **Project** tab that bundles ROIs, traces, and
per-run metadata into `.fiarproj` directories with an append-only traceability log.

## Workspace

Cargo workspace, three crates:

```
crates/fiarfly-core/src/      # the engine — no GUI deps
  io/        tiff.rs, project.rs (.fiarproj bundles)
  motion/    rigid.rs, nonrigid.rs (NoRMCorre-style FFT registration), preprocess.rs
  roi/       polygon.rs, mask.rs
  signal/    extraction.rs, delta_f.rs, deconvolution.rs (OASIS), events.rs, quality.rs
  analysis/  metric.rs (mean/peak/AUC/latency/rise-time), window.rs
  stats/     parametric, nonparametric, effect sizes, correction (Bonferroni/BH)
  export/    dataframe.rs (Polars → parquet/csv)
crates/fiarfly-gui/src/       # egui app; panels/ mirror the pipeline tabs
crates/fiarfly-py/            # PyO3 bindings, built with maturin
python/fiarfly/, python/tests/
```

Key deps: `ndarray` (rayon feature), `rustfft`, `rayon`, `polars`, `statrs`, `tiff`.

## Commands

```bash
# Rust
cargo build --release
cargo test                      # 68 tests
cargo clippy                    # currently ~83 mostly-mechanical warnings

# GUI
cargo run -p fiarfly-gui --release

# Python (uv-managed) — dev.sh wraps these
./dev.sh                        # first-time: uv sync + maturin develop
./dev.sh build                  # rebuild Rust extension after Rust changes
./dev.sh test                   # python/tests via pytest
./dev.sh notebook               # JupyterLab
```

After editing Rust that the Python package uses, rebuild with `./dev.sh build`
(`maturin develop`) or the Python side runs against a stale extension.

## Conventions

- Keep `fiarfly-core` GUI-free — all compute lives there; the GUI and Python wrapper
  are thin shells over it. The GUI uses a worker-thread + `mpsc` + `try_recv` polling
  architecture to keep the UI responsive — preserve that, don't block the UI thread.
- `release` profile keeps `debug = 1` (symbol names) for readable crash backtraces.
- `cargo clippy --fix` clears most of the mechanical warning backlog.

## Hot path / gotchas

Motion correction (`motion/rigid.rs`) is the dominant compute cost. Its parallel
per-frame loop has been tuned; the optimizations below are all in place — keep them
when touching the loop:

- The static template's FFT is hoisted to the frequency domain once before the loop
  (`t_freq`) and reused per frame via `cross_correlate_2d_freq` (~⅓ of FFT work saved).
- Each rayon worker reuses one `FftPlanner` via `map_init` rather than rebuilding it
  per frame.
- Corrected frames are written straight into the output stack with `axis_iter_mut`
  (indexed parallel), so peak memory is ~one stack instead of ~2× (no second
  full-size `results` buffer). Matters for multi-GB stacks.
- Progress is an `AtomicUsize` counter sent through the `mpsc` channel, giving the GUI
  real incremental progress. Each worker clones the sender (it's `!Sync`).
