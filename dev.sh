#!/usr/bin/env bash
# dev.sh — bootstrap the FIARfly Python development environment with uv.
#
# Usage:
#   ./dev.sh          # first-time setup: installs uv, creates .venv, builds extension
#   ./dev.sh build    # rebuild the Rust extension only (after Rust changes)
#   ./dev.sh test     # run the Python test suite
#   ./dev.sh notebook # launch JupyterLab

set -euo pipefail

COMMAND="${1:-setup}"

# ── 1. Ensure uv is available ─────────────────────────────────────────────────
if ! command -v uv &>/dev/null; then
    echo "uv not found — installing via official installer…"
    curl -LsSf https://astral.sh/uv/install.sh | sh
    # Add to PATH for the remainder of this script.
    export PATH="$HOME/.local/bin:$PATH"
fi

echo "uv $(uv --version)"

# ── 2. Dispatch commands ──────────────────────────────────────────────────────
case "$COMMAND" in

setup)
    echo ""
    echo "==> Creating virtual environment and installing dependencies…"
    uv sync --extra dev

    echo ""
    echo "==> Building Rust extension (debug build for development)…"
    uv run maturin develop

    echo ""
    echo "Done! Activate the environment with:"
    echo "  source .venv/bin/activate"
    echo ""
    echo "Or prefix any command with 'uv run', e.g.:"
    echo "  uv run python -c 'import fiarfly; print(fiarfly.__version__)'"
    echo "  uv run pytest"
    ;;

build)
    echo "==> Rebuilding Rust extension…"
    uv run maturin develop
    ;;

build-release)
    echo "==> Rebuilding Rust extension (release / optimised)…"
    uv run maturin develop --release
    ;;

test)
    echo "==> Running Python test suite…"
    uv run pytest "${@:2}"
    ;;

notebook)
    echo "==> Launching JupyterLab…"
    uv run jupyter lab
    ;;

*)
    echo "Usage: $0 [setup|build|build-release|test|notebook]"
    exit 1
    ;;

esac
