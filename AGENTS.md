# AGENTS.md — FIARfly

Onboarding pointer for AI/coding agents working in this repo. Read this first, then
[CLAUDE.md](CLAUDE.md) for the working details. This file is intentionally short; it
points at the canonical docs rather than repeating them.

## What this project is

FIARfly is a **calcium imaging analysis** tool: a Rust core engine (`fiarfly-core`),
an egui desktop GUI (`fiarfly-gui`), and a Python wrapper (`fiarfly-py`, via maturin/
`uv`). The pipeline is linear: **Import (.tif/.tiff) → Motion correction → ROI →
Signal extraction (ΔF/F) → Analysis → Stats → Export**, plus a **Project** tab that
bundles results into `.fiarproj` directories.

Repo lives at `~/Documents/VsCode/fiarfly` (moved 2026-05-07 from `~/fiarfly`).

## Where to read what

| You need…                                  | Read |
| ------------------------------------------ | ---- |
| Day-to-day working notes, commands, gotchas | [CLAUDE.md](CLAUDE.md) |
| Crate/module layout, design rationale       | [ARCHITECTURE.md](ARCHITECTURE.md) |
| Project pitch, feature overview             | [README.md](README.md) |
| Build/toolchain setup                       | [SETUP.md](SETUP.md) |
| End-user manual                             | [USER_GUIDE.md](USER_GUIDE.md) |
| Step-by-step build-out history              | [IMPLEMENTATION_GUIDE.md](IMPLEMENTATION_GUIDE.md) |
| Science / algorithm reference               | [docs/PIPELINE_REFERENCE.md](docs/PIPELINE_REFERENCE.md) |
| egui panel/layout spec                      | [docs/GUI_DESIGN.md](docs/GUI_DESIGN.md) |
| Python API surface                          | [docs/API_DESIGN.md](docs/API_DESIGN.md) |
| Current code-audit status & handoff         | [docs/AUDIT_LOG.md](docs/AUDIT_LOG.md) |

## Rules of the road (the short version)

- **Keep `fiarfly-core` GUI-free and Python-free.** All compute lives there; the GUI
  and Python wrapper are thin shells. Display-only concerns (colormaps, brightness/
  contrast) stay in `fiarfly-gui`.
- **Don't block the UI thread.** The GUI uses a worker-thread + `mpsc` + `try_recv`
  polling architecture. Preserve it.
- **Rebuild the Python extension after Rust changes** the Python package uses:
  `./dev.sh build` (`maturin develop`), or Python runs against a stale `.so`.
- **Plan larger features in [ARCHITECTURE.md](ARCHITECTURE.md) before building** new
  modules/tabs — write the plan in the doc first, then execute.
- **The motion-correction hot path is tuned.** See the "Hot path / gotchas" section of
  CLAUDE.md before touching `motion/rigid.rs`.

## Verify before you call it done

```bash
cargo build --release
cargo test            # all tests should pass
cargo clippy          # known backlog of mechanical warnings; don't add new ones
cargo run -p fiarfly-gui --release   # smoke-test the GUI for UI changes
```
