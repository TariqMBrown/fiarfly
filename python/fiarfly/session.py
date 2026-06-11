"""
High-level Session class — Pythonic wrapper around the PyO3 PySession.

Full API spec: docs/API_DESIGN.md §fiarfly.Session
"""

from __future__ import annotations

import os
from typing import TYPE_CHECKING

import polars as pl

# The native extension is built by maturin (fiarfly-py crate).
# It will not be importable until `maturin develop` has been run.
try:
    from fiarfly import PySession  # type: ignore[import]
    _NATIVE_AVAILABLE = True
except ImportError:
    _NATIVE_AVAILABLE = False
    PySession = None  # type: ignore[assignment,misc]


class Session:
    """
    A FIARfly analysis session tied to a single TIFF recording.

    Parameters
    ----------
    path : str | os.PathLike
        Path to a .tif or .tiff file.
    modality : str
        One of: "two_photon_in_vivo", "two_photon_in_vitro",
                "one_photon_in_vivo", "one_photon_widefield"
    frame_rate : float, optional
        Recording frame rate in Hz. Enables the ``time_s`` column in output.
    """

    def __init__(
        self,
        path: str | os.PathLike,
        modality: str = "two_photon_in_vivo",
        frame_rate: float | None = None,
    ) -> None:
        if not _NATIVE_AVAILABLE:
            raise RuntimeError(
                "fiarfly native extension not found. "
                "Run `maturin develop` in the repo root to build it."
            )
        self._inner = PySession(str(path), modality, frame_rate)

    def motion_correct(
        self,
        mode: str = "rigid",
        max_shift: int = 10,
        grid_size: tuple[int, int] = (32, 32),
        overlap: int = 8,
        bin_width: int = 50,
        n_jobs: int = -1,
    ) -> "Session":
        """
        Run motion correction in-place.

        Parameters
        ----------
        mode : str
            "rigid" or "nonrigid"
        max_shift : int
            Maximum allowed shift in pixels.
        grid_size : (int, int)
            Patch size for non-rigid correction [height, width].
        overlap : int
            Patch overlap for non-rigid correction.
        bin_width : int
            Frames averaged per template update step.
        n_jobs : int
            CPU threads to use. -1 = all available.

        Returns
        -------
        self — for method chaining.
        """
        self._inner.motion_correct(mode)
        return self

    def load_rois(self, path: str | os.PathLike, append: bool = False) -> "Session":
        """
        Load ROIs from a .rois.json file saved by the GUI.

        Parameters
        ----------
        path : str | Path
        append : bool
            Append to existing ROIs if True; replace if False.

        Returns
        -------
        self
        """
        self._inner.load_rois(str(path))
        return self

    def extract_traces(
        self,
        delta_f: bool = True,
        baseline_percentile: float = 8.0,
        baseline_window: int = 300,
        neuropil_correction: bool = True,
        neuropil_r: float = 0.7,
        neuropil_inner_radius: float = 3.0,
        neuropil_outer_radius: float = 18.0,
        deconvolve: bool = False,
        oasis_g: float | None = None,
        oasis_sn: float | None = None,
    ) -> pl.DataFrame:
        """
        Extract fluorescence traces from all loaded ROIs.

        Returns a tidy Polars DataFrame (one row per ROI per frame).
        See docs/API_DESIGN.md for the full column schema.
        """
        return self._inner.extract_traces()  # type: ignore[return-value]

    def save(self, path: str | os.PathLike) -> None:
        """Save session state to a .fiarfly JSON file."""
        # TODO (Phase 7): implement
        raise NotImplementedError("Phase 7")

    @classmethod
    def load(cls, path: str | os.PathLike) -> "Session":
        """Load a previously saved session."""
        raise NotImplementedError("Phase 7")
