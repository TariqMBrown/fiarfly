# Calcium Imaging Pipeline — Reference

> Science and algorithm reference for developers building FIARfly. Not a user manual.

---

## What is Calcium Imaging?

Calcium imaging uses fluorescent calcium indicators (GCaMP, RCaMP, jRCaMP, etc.) to indirectly measure neuronal activity. When a neuron fires, intracellular calcium ([Ca²⁺]) rises sharply, causing the fluorescent indicator to brighten. This brightness change — the calcium transient — is recorded as a video of the brain tissue.

### Why is analysis non-trivial?

1. **Motion artifacts** — the brain moves slightly during recording (heartbeat, respiration, animal movement). Uncorrected, this causes ROIs to drift across cells.
2. **Overlapping signals** — neurons are densely packed; signals from adjacent cells bleed into each other.
3. **Background fluorescence** — tissue scatters light; the neuropil (dendrites, axons, glial processes) produces diffuse fluorescence that contaminates somatic signals.
4. **Noise** — shot noise (photon statistics), dark current, read noise from the camera.
5. **Bleaching** — gradual loss of indicator fluorescence over the session (slow drift in baseline F).

---

## Imaging Modalities

### Two-Photon Microscopy (2P)
- Uses two near-infrared photons simultaneously absorbed → fluorescence only at focal point.
- High spatial resolution (~0.5–1 µm lateral), optical sectioning, low background.
- Scans point-by-point → bidirectional scanning artifacts ("sawtooth" distortion per line).
- Typical framerate: 15–60 Hz; typical FOV: 256×256 to 1024×1024 px.
- Background: low and spatially structured.
- Neuropil contamination: significant; neuropil correction mandatory.
- Used for: cortex, hippocampus, cerebellum imaging in vivo and acute slice (in vitro).

### One-Photon Miniature Microscope (1P Miniscope)
- Single photon, implanted gradient-index (GRIN) lens.
- Lower resolution (~2–3 µm), significant out-of-plane background.
- Used for: deep brain structures in freely-moving animals (in vivo only).
- Background: high, spatially varying — requires CNMF-E or ring subtraction.
- Neuropil correction: not standard; background subtraction replaces it.
- Typical framerate: 15–30 Hz.

### Wide-Field One-Photon (1P Wide-field)
- Epi-fluorescence; illuminates entire FOV simultaneously.
- Very low resolution at single-cell level; mainly used for mesoscale imaging.
- Background: high global component.
- Typical use: cortex-wide functional mapping in vivo or large dish in vitro.

### In Vivo vs. In Vitro

| Factor | In vivo | In vitro (acute slice / culture) |
|---|---|---|
| Motion | Significant (heartbeat, breath, movement) | Minimal to none |
| Baseline stability | Drifts with arousal state | Generally stable |
| Cell density | High (neuropil dense) | Variable |
| Recording duration | 10 min – hours | 5 min – 1 hr |
| Typical artifacts | Motion, photobleaching | Mechanical drift, pH changes |

---

## Standard Pipeline

```
Raw TIFF frames
      │
      ▼ 1. PREPROCESSING
      │  - Spatial downsampling (optional)
      │  - Temporal downsampling (optional)
      │  - Spatial filtering (high-pass for 1P)
      │
      ▼ 2. MOTION CORRECTION
      │  - Rigid: single [dy, dx] shift per frame
      │  - Non-rigid: displacement field per frame
      │
      ▼ 3. ROI DEFINITION
      │  - Manual (hand-drawn polygons) ← FIARfly focus
      │  - Automated (CNMF, PCA-ICA, CELLMax)
      │
      ▼ 4. SIGNAL EXTRACTION
      │  - Raw fluorescence F per ROI per frame
      │  - Neuropil correction (2P)
      │  - ΔF/F normalization
      │
      ▼ 5. DECONVOLUTION (optional)
      │  - Infer spike times from calcium transient shape
      │  - OASIS, MLSpike, etc.
      │
      ▼ 6. ANALYSIS (out of scope for FIARfly v0.1)
         - Event-triggered averaging
         - Tuning curves, decoding
         - Cross-session alignment
```

---

## Motion Correction Algorithms

### Rigid Motion Correction

Assumes the entire frame undergoes a single rigid translation each frame. Sufficient for in vitro, or in vivo recordings where brain movement is small (< 10 µm).

**Algorithm (NoRMCorre rigid):**

1. Initialize template `T` as mean of first `bin_width` frames.
2. For frame `f_t`:
   a. Compute 2D FFT: `F = FFT2(f_t)`, `G = FFT2(T)`.
   b. Normalized cross-correlation in frequency domain:
      `NCC = IFFT2( F * conj(G) / (|F| * |G| + ε) )`
   c. Find peak location `(dy, dx)` in NCC (restrict to within `max_shift`).
   d. Sub-pixel refinement: fit 1D Gaussian to peak row and column independently.
   e. Shift frame by `(-dy, -dx)` using bilinear interpolation.
   f. Update template: exponential moving average `T ← α * f_shifted + (1-α) * T`, where `α = 1/bin_width`.
3. Output: shifted frames + shift array `shifts[t] = [dy, dx]`.

**Key parameter:** `max_shift` — prevents false correlation matches. Set to ~10% of frame size. Too large: risk of wrong match. Too small: fails for large movements.

### Non-Rigid Motion Correction

Accounts for locally varying motion across the FOV (e.g., different brain regions moving independently). Essential for large FOV recordings or when rigid correction leaves residual motion artifacts.

**Algorithm (NoRMCorre non-rigid):**

1. Divide frame into overlapping patches of size `(grid_h + 2*overlap) × (grid_w + 2*overlap)`.
2. For each patch `p` independently, compute rigid shift `[dy_p, dx_p]` vs. template patch.
3. Assemble displacement field: interpolate patch-level shifts to per-pixel displacements using Gaussian-weighted blending. Constraint: field must be smooth to avoid tearing artifacts.
4. Apply displacement field: for each output pixel `(y, x)`, sample source frame at `(y + dy(y,x), x + dx(y,x))` using bilinear interpolation.
5. Update template.

**Key parameters:**
- `grid_size`: patch size. Smaller → more local correction but more noise. Typical: [32,32] to [64,64].
- `overlap`: patch overlap for smooth field. Typically 25–50% of grid_size.
- `max_shift`: per-patch maximum. Usually same as rigid.

**Quality assessment:**
- Per-frame correlation with template: `corr(frame, template)`. Plot to detect frames with failed correction.
- Crispness (variance of Laplacian): higher = sharper = better corrected.
- Flag frames where correlation drops below 0.9 × median.

---

## ROI Definition

### Manual ROI Drawing (FIARfly approach)

The user draws polygon boundaries around individual cells (somata) by clicking vertices on the motion-corrected mean image. This is the gold standard for accuracy when cell count is manageable (< ~200 ROIs).

**Best practices to communicate to users:**
- Draw on the **mean projection** of the corrected stack (brighter = more active cells).
- Also consider the **max projection** (shows even rarely-active cells).
- For 2P: draw around the soma only, not processes.
- For 1P miniscope: cells appear as bright rings; draw around the center.
- Neuropil ring should not overlap adjacent ROIs.

### Automated extraction (out of scope for v0.1, document for future)

| Algorithm | Best for | Notes |
|---|---|---|
| CNMF | 2P in vivo/vitro | Matrix factorization; handles overlap |
| CNMF-E | 1P miniscope | Extended background model |
| PCA-ICA | 1P, small datasets | Faster but less accurate |
| CELLMax | 1P miniscope | Successor to PCA-ICA |
| EXTRACT | Both | Robust to noise |

These can be added as optional pipeline steps via the Python API in later versions, using CaImAn as a backend.

---

## ΔF/F Normalization

Raw fluorescence `F(t)` varies across sessions due to expression levels, illumination, bleaching. Normalizing to ΔF/F makes signals comparable:

```
ΔF/F(t) = (F(t) - F0(t)) / F0(t)
```

Where `F0(t)` is the estimated baseline fluorescence (what F would be if the neuron were silent).

**Estimating F0:**
- **Percentile method** (recommended): `F0(t) = percentile(F(t-w/2 : t+w/2), p)` where `p` ≈ 8th percentile and `w` ≈ 300 frames. The low percentile approximates the minimum (baseline) while being robust to noise.
- **Exponential baseline**: fit an exponential decay to account for photobleaching — useful for long recordings.
- **Fixed window**: average of the first N frames (only valid if recording starts at baseline).

---

## Neuropil Correction (2-Photon)

In 2P data, the measured signal at a soma includes contamination from surrounding neuropil:

```
F_measured(t) = F_soma(t) + r * F_neuropil(t)
```

Where `r` is the neuropil contamination factor (typically 0.7, range 0.5–0.9).

Correction:
```
F_corrected(t) = F_measured(t) - r * F_neuropil(t)
```

`F_neuropil` is estimated from an annular ring around the soma (inner radius ≈ soma radius + 2 µm, outer radius ≈ soma radius + 15 µm), excluding pixels belonging to other ROIs.

**For 1P data:** Do not apply neuropil correction. Instead, subtract a global or local background component estimated from ROI-free regions.

---

## OASIS Deconvolution

Fluorescence transients have a characteristic shape dictated by the calcium indicator's kinetics. OASIS (Online Active Set method to Infer Spikes) deconvolves this to estimate the underlying spike train.

**Calcium model (AR(1)):**
```
c[t] = g * c[t-1] + s[t]
y[t] = c[t] + noise
```
Where:
- `c[t]` = true calcium signal
- `s[t] ≥ 0` = spike-triggered calcium event (0 if no spike)
- `g` = decay constant (related to indicator: GCaMP6f → g ≈ 0.95 at 30Hz)
- `y[t]` = observed ΔF/F

OASIS finds `c` and `s` that minimize: `||y - c||² + λ||s||₁` subject to `c ≥ 0`, `s ≥ 0`.

**Parameters:**
- `g`: auto-estimate from autocorrelation lag-1 of the trace.
- `sn`: noise std, auto-estimate from power spectral density of high-frequency content.
- `lambda`: sparsity penalty; 0 for no penalty (recommended for GCaMP).

---

## Key References

| Paper | What it describes |
|---|---|
| Pnevmatikakis & Giovannucci 2017, Neuron | NoRMCorre motion correction |
| Pnevmatikakis et al. 2016, Neuron | CNMF for 2P |
| Zhou et al. 2018, eLife (CaImAn paper) | Full CaImAn pipeline |
| Friedrich et al. 2017, PLoS Comput Biol | OASIS deconvolution |
| Giovannucci et al. 2019, eLife | CNMF-E for 1P miniscopes |
| Shuman et al. 2020, Frontiers Neural Circuits | EZcalcium (MATLAB pipeline) |
