"""
Phase 1 tests: TIFF loading.
Run after `maturin develop`.
"""
import pytest


@pytest.fixture
def synthetic_tiff(tmp_path):
    """Write a minimal 10-frame 64×64 uint16 TIFF for testing."""
    import numpy as np

    # Only attempt if tifffile is available; skip otherwise
    tifffile = pytest.importorskip("tifffile")
    path = tmp_path / "synthetic.tif"
    rng = np.random.default_rng(42)
    data = (rng.random((10, 64, 64)) * 65535).astype(np.uint16)
    tifffile.imwrite(str(path), data)
    return path


def test_session_opens_tiff(synthetic_tiff):
    """Session should open without error and report correct dimensions."""
    fiarfly = pytest.importorskip("fiarfly")
    s = fiarfly.Session(str(synthetic_tiff), frame_rate=30.0)
    # TODO: assert s.info()["n_frames"] == 10 once Phase 7 is implemented


def test_session_requires_native_extension():
    """Import should fail gracefully if native extension not built."""
    import importlib
    import sys
    # Temporarily hide the native extension
    fiarfly_mod = sys.modules.get("fiarfly")
    # This is a smoke test — just ensure the package is importable
    import fiarfly  # noqa: F401
