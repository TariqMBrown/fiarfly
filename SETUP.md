# FIARfly — Installation & Running Guide

This guide covers everything from a fresh machine to a working FIARfly installation.

---

## Recommended System Specifications

FIARfly loads entire TIFF stacks into memory as 32-bit floats and performs compute-intensive operations (FFT-based motion correction, signal extraction). The requirements scale with your data size.

| | Minimum | Recommended | Heavy workloads |
|---|---|---|---|
| **RAM** | 8 GB | 16 GB | 32–64 GB |
| **CPU** | 4 cores | 8+ cores | 16+ cores |
| **Storage** | SSD (any) | NVMe SSD | NVMe SSD |
| **OS** | macOS 12+, Linux (glibc 2.31+), Windows 10+ | Same | Same |

### Memory rule of thumb

A TIFF stack uses **frames x height x width x 4 bytes** in memory. Motion correction roughly triples this (raw + corrected + FFT workspace). Examples:

| Recording | Stack size | Peak during MC |
|---|---|---|
| 1,000 frames x 512x512 | 1.0 GB | ~2.5 GB |
| 5,000 frames x 512x512 | 5.0 GB | ~12.5 GB |
| 10,000 frames x 512x512 | 10.0 GB | ~25 GB |
| 5,000 frames x 1024x1024 | 20.0 GB | ~50 GB |

The Import panel in FIARfly shows these estimates for your specific file after loading. If peak memory exceeds ~80% of your system RAM, expect slowdowns from swap. Non-rigid motion correction uses more memory than rigid due to per-patch FFT buffers.

### CPU scaling

Motion correction and signal extraction use all available cores via Rayon. More cores = proportionally faster processing. Single-threaded performance matters for TIFF I/O (decoding is sequential).

---

## Prerequisites

### 1. Rust toolchain

Rust is required to build both the GUI and the Python extension.

```bash
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
```

Follow the on-screen prompts (choose the default installation).  
Then add Rust to your current shell session:

```bash
export PATH="$HOME/.cargo/bin:$PATH"
```

To make this permanent, add it to your `~/.zshrc`:

```bash
echo 'export PATH="$HOME/.cargo/bin:$PATH"' >> ~/.zshrc
source ~/.zshrc
```

Verify:
```bash
cargo --version   # e.g. cargo 1.78.0
```

---

### 2. Xcode Command Line Tools (macOS only)

Required for the C linker used during compilation.

```bash
xcode-select --install
```

If already installed you will see: `command line tools are already installed`.

---

### 3. Python 3.11 (Python package only)

Only needed if you want to use the Python API. The GUI has no Python dependency.

Check if you already have it:
```bash
python3.11 --version
```

If not, install via the official installer at https://www.python.org/downloads/ and select **Python 3.11**.

> **Conda users:** deactivate your conda environment before working with FIARfly's Python package to avoid conflicts:
> ```bash
> conda deactivate
> ```

---

## First-time Installation

### Clone the repository

```bash
git clone <repository-url> fiarfly
cd fiarfly
```

---

### GUI only (no Python required)

Build and run in one command:

```bash
cargo run --release -p fiarfly-gui
```

The first build downloads and compiles all Rust dependencies — this takes a few minutes. Subsequent builds are incremental and much faster.

---

### Python package (optional)

The Python package lets you run the same analysis pipeline from scripts and Jupyter notebooks.

**Step 1 — Install uv** (fast Python package manager):

```bash
curl -LsSf https://astral.sh/uv/install.sh | sh -s -- --no-modify-path
export PATH="$HOME/.local/bin:$PATH"
```

To make the PATH change permanent:
```bash
echo 'export PATH="$HOME/.local/bin:$PATH"' >> ~/.zshrc
source ~/.zshrc
```

**Step 2 — Create the environment and install dependencies:**

```bash
uv sync
```

**Step 3 — Build the Rust extension for Python:**

```bash
uv run maturin develop
```

For an optimised build (slower to compile, faster at runtime):
```bash
uv run maturin develop --release
```

**Step 4 — Verify:**
```bash
uv run python -c "import fiarfly; print(fiarfly.__version__)"
# 0.1.0
```

---

## Running FIARfly

### GUI

```bash
cd /path/to/fiarfly
cargo run --release -p fiarfly-gui
```

To run without the `--release` flag (faster compile, slower runtime — useful during development):
```bash
cargo run -p fiarfly-gui
```

### Python API (v0.2)

The Python package wraps `fiarfly-core`'s project, analysis, and statistics modules. The heavy ingest steps (TIFF load, motion correction, ROI drawing) are best done in the desktop GUI; save the result as a `.fiarproj` bundle, then drive analysis programmatically:

```python
import fiarfly
import numpy as np
import polars as pl

# Open a saved workflow.
proj = fiarfly.Project.open("/data/study_a.fiarproj")
print(proj.name, len(proj.runs))

# Read a run's traces.
rows = proj.load_run_traces(proj.runs[0].id)
df = pl.DataFrame(rows)

# Compute window metrics.
delta_f = (df.pivot(index="frame_idx", on="roi_id", values="delta_f_over_f")
             .drop("frame_idx").to_numpy().T.astype("float32"))
auc_stim1 = fiarfly.auc(delta_f, 10.0, 15.0, frame_rate=proj.frame_rate, seconds=True)

# Run a paired test.
res = fiarfly.paired_t([float(x) for x in auc_stim1], [...])
print(res.test_name, res.p_value, res.effect_size)
```

See [docs/API_DESIGN.md](docs/API_DESIGN.md) for the full Python API and [USER_GUIDE.md](USER_GUIDE.md) for a walkthrough of the desktop application.

Run scripts via uv to ensure the correct environment is used:
```bash
uv run python my_analysis.py
```

### Tests

```bash
uv run pytest
```

### Jupyter notebook

```bash
uv run jupyter lab
```

---

## After Rust code changes

If you change any Rust code and want the Python package to pick it up:

```bash
uv run maturin develop
```

The GUI always recompiles from source when you run `cargo run`.

---

## Shortcuts via dev.sh

The included `dev.sh` script wraps the common commands:

| Command | What it does |
|---|---|
| `./dev.sh` | First-time setup: installs uv, creates `.venv`, builds extension |
| `./dev.sh build` | Rebuild Rust extension (debug) |
| `./dev.sh build-release` | Rebuild Rust extension (optimised) |
| `./dev.sh test` | Run pytest |
| `./dev.sh notebook` | Launch JupyterLab |

---

## Troubleshooting

### `zsh: command not found: cargo`
Rust is installed but not on your PATH. Run:
```bash
export PATH="$HOME/.cargo/bin:$PATH"
```

### `zsh: command not found: uv`
uv is installed but not on your PATH. Run:
```bash
export PATH="$HOME/.local/bin:$PATH"
```

### `Both VIRTUAL_ENV and CONDA_PREFIX are set`
You are inside a conda environment. Deactivate it first:
```bash
conda deactivate
```

### `error: linking with 'cc' failed` (fiarfly-py)
The Python headers are missing or the Python version is wrong. Ensure Python 3.11 is installed and that `python3.11 --version` works. Then retry `uv run maturin develop`.

### `Undefined symbols: _PyBaseObject_Type, _PyBool_Type, …` (cargo build of fiarfly-py)
You're invoking `cargo build -p fiarfly-py` directly. PyO3 extension modules don't link against libpython at build time — Python provides those symbols at import time. The repo includes [`crates/fiarfly-py/build.rs`](crates/fiarfly-py/build.rs) that injects `-undefined dynamic_lookup` on macOS so plain `cargo build` works; if you still see this error, make sure the build script is present and re-run `cargo clean -p fiarfly-py && cargo build -p fiarfly-py`. The recommended path remains `uv run maturin develop` / `./dev.sh build`.

### Permission errors when running `dev.sh`
uv's installer tries to update shell config files. If those files are not writable:
```bash
chmod u+rw ~/.zshrc ~/.bash_profile
```
Then re-run, or install uv manually with `--no-modify-path` as shown above.

---

## Environment summary

| Tool | Where | How to install |
|---|---|---|
| Rust / cargo | `~/.cargo/bin/` | `rustup` installer |
| uv | `~/.local/bin/` | `astral.sh/uv` installer |
| Python 3.11 | system | python.org or Homebrew |
| Xcode CLI | system | `xcode-select --install` |
