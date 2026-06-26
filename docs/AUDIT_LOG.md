# FIARfly — Audit & Handoff Log

> Running log of the code-audit / hardening pass on FIARfly, kept so a future agent
> can pick up mid-stream without re-deriving context. Newest entries at the top.
> For project orientation start at [AGENTS.md](../AGENTS.md) → [CLAUDE.md](../CLAUDE.md).

## How this audit works

- **Goal:** review the codebase pass-by-pass for correctness, performance, and UX, and
  fix issues as they're found — not a one-shot rewrite.
- **Finding labels:** issues are tagged `FF-<n>` (e.g. `FF-1`) and referenced in commit
  messages. There is no separate tracker; the labels live in git history and here.
- **Scope guardrails:** display/UX changes stay in `fiarfly-gui`; anything that alters
  computed pixel values, traces, or ΔF/F belongs in `fiarfly-core` and must be called
  out explicitly. Keep `fiarfly-core` GUI-free.
- **Definition of done for a change:** `cargo build --release`, `cargo test`, and
  `cargo clippy` clean (no *new* warnings), plus a GUI smoke-test for UI work.

## Status at last handoff (2026-06-26)

### Done & committed

- **FF-1 / FF-2 / FF-5 — rigid motion-correction speedup + fmt/clippy across core**
  (commit `b6587e8`). The hot path in `motion/rigid.rs` was tuned; the optimizations
  are documented in CLAUDE.md → "Hot path / gotchas" and **must be preserved**:
  - static template FFT hoisted to the frequency domain once (`t_freq`), reused per
    frame via `cross_correlate_2d_freq`;
  - each rayon worker reuses one `FftPlanner` via `map_init`;
  - corrected frames written in place with `axis_iter_mut` (peak memory ≈ one stack,
    not 2×);
  - progress reported via an `AtomicUsize` through the `mpsc` channel.

### In flight — UNCOMMITTED working-tree changes

These were being made when the session ended (`git status` shows them modified/new).
A future agent should review, test, and commit them (likely as the next `FF-` finding).

- **New `crates/fiarfly-gui/src/colormap.rs`** — display-only ImageJ-style pseudo-color
  LUTs (Grays/Fire/Ice/Green/Magenta/Viridis/Inferno). Maps a normalized intensity to
  RGB. Lives in the GUI crate so `fiarfly-core` stays display-free.
- **`state.rs`** — added `display_colormap`, `preview_brightness`, `preview_contrast`
  fields, and a `reset_derived()` method that clears everything derived from a
  recording's pixels (stacks, ROIs, traces, MC scores, deconv/event/quality results,
  caches, projections, textures) while **preserving** user settings and any open
  `.fiarproj`. Called on new-TIFF load and the "Reset workspace" button — this fixed a
  stale-state bug where panels kept showing the previous file's results.
- **`panels/import.rs`** — LUT selector + **brightness/contrast sliders** on the Import
  preview (display-only). Transform layers on top of the existing per-frame
  normalization, then feeds the colormap:

  ```text
  t0 = (v - frame_min) / frame_range          # existing per-frame stretch
  t  = (t0 - 0.5) * contrast + 0.5 + brightness
  display = colormap(clamp(t, 0..1))
  ```

  Defaults (brightness 0, contrast 1×) reproduce prior output exactly. Moving a slider
  or Reset invalidates the cached preview texture (`preview_frame_loaded = usize::MAX`).
- **`panels/roi_editor.rs`** — applies the selected LUT to the ROI-editor canvas; keeps
  its own existing min/max LUT sliders.
- **`main.rs`** — wires in the new `colormap` module.
- **`ARCHITECTURE.md`** — documents that display-only LUT helpers live in the GUI crate.

Last verification on these changes: GUI compiled clean (only the pre-existing
core `t`-index warning), tests passed.

## Known follow-ups / open items

- **Per-frame vs. stack-wide normalization (open design choice).** Import-preview
  brightness/contrast are currently relative to *each frame's* min/max, so during
  playback the baseline can shift frame-to-frame. True ImageJ B&C behavior would compute
  the stack min/max once on load and stretch against that fixed range. Decide and, if
  desired, switch the preview to a fixed stack-wide range.
- **Clippy backlog.** There is a standing set of mostly-mechanical warnings
  (`cargo clippy --fix` clears most). Don't add new ones; chipping at the backlog is
  welcome but separate from feature work.
- **Pre-existing core warning:** `the loop variable 't' is used to index 'y'` in
  `fiarfly-core` — benign and unrelated to GUI work, but worth a proper fix.

## Commit checklist for the in-flight work

1. `cargo build --release && cargo test` — all pass.
2. `cargo clippy -p fiarfly-gui` — no new warnings.
3. `cargo run -p fiarfly-gui --release` — eyeball the Import preview: LUT menu,
   brightness/contrast sliders, Reset, and new-file load clearing stale panels.
4. Commit with an `FF-<n>` label describing the LUT + B&C + stale-state fix.
