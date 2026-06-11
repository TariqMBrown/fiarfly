"""
Python-side RoiSet wrapper.

Full spec: docs/API_DESIGN.md §fiarfly.RoiSet
"""
from __future__ import annotations
import os
import polars as pl

try:
    from fiarfly import PyRoiSet  # type: ignore[import]
    _NATIVE_AVAILABLE = True
except ImportError:
    _NATIVE_AVAILABLE = False
    PyRoiSet = None  # type: ignore[assignment,misc]


class RoiSet:
    """A collection of polygon ROIs loaded from a .rois.json file."""

    def __init__(self, _inner: object) -> None:
        self._inner = _inner

    @classmethod
    def from_file(cls, path: str | os.PathLike) -> "RoiSet":
        if not _NATIVE_AVAILABLE:
            raise RuntimeError("fiarfly native extension not built. Run `maturin develop`.")
        return cls(PyRoiSet.from_file(str(path)))

    def to_dataframe(self) -> pl.DataFrame:
        """Return ROI metadata as a Polars DataFrame."""
        raise NotImplementedError("Phase 7")
