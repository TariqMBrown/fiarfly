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

## Hot path / gotchas (see CODE_REVIEW_FINDINGS.md, fiarfly)

Motion correction (`motion/rigid.rs`) is the dominant compute cost. Its parallel
per-frame loop does redundant work:

- `cross_correlate_2d` recomputes `fft2(template)` per frame though the template is
  static — hoist it to the frequency domain once before the loop (~⅓ of FFT work).
- A new `FftPlanner` is built inside `.map()` per frame — use `rayon`'s
  `map_init(FftPlanner::new, …)` so each worker reuses one planner.
- Peak memory is ~2× stack size because `results` holds all shifted frames before
  copying into `corrected`; writing into the output via `axis_iter_mut` + parallel zip
  halves it (matters for multi-GB stacks).
- Progress only reports 0% and 100%; an atomic counter would give the GUI real progress.
