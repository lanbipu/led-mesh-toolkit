"""Tests for screen_mapping: ScreenMapping model, charuco_corner_local_mm, preflight."""
import pytest
import numpy as np
from lmt_vba_sidecar.screen_mapping import ScreenMapping, ScreenMappingError


def _mapping():
    return ScreenMapping.model_validate({
        "screen_id": "S",
        "cabinets": [{
            "cabinet_id": "V000_R000",
            "resolution_px": [900, 510],
            "active_size_mm": [600, 340],
            "pixel_pitch_mm": [0.667, 0.667],
            "active_origin": "center",
            "input_rect_px": [0, 0, 900, 510],
            "rotation": 0,
            "mirror_x": False,
            "mirror_y": False,
        }],
        "expected_pattern_hash": "abc123",
    })


# ---------------------------------------------------------------------------
# charuco_corner_local_mm
# ---------------------------------------------------------------------------

def test_charuco_corner_local_mm_centered():
    """p0 and p63 must be symmetric about center (physical ChArUco convention)."""
    m = _mapping()
    p0 = m.charuco_corner_local_mm("V000_R000", 0, squares_x=9, squares_y=9, square_px=60)
    p_last = m.charuco_corner_local_mm("V000_R000", 63, squares_x=9, squares_y=9, square_px=60)
    assert np.allclose(p0[:2], -p_last[:2], atol=1e-6), (
        f"Not symmetric: p0={p0[:2]}, p_last={p_last[:2]}"
    )
    assert p0[2] == 0.0
    assert p_last[2] == 0.0


def test_charuco_corner_local_mm_z_is_zero():
    """z component is always 0 (flat screen plane)."""
    m = _mapping()
    for cid in [0, 15, 31, 63]:
        p = m.charuco_corner_local_mm("V000_R000", cid, squares_x=9, squares_y=9, square_px=60)
        assert p[2] == 0.0, f"z != 0 for charuco_id={cid}"


def test_charuco_corner_local_mm_pitch_based_spacing():
    """
    Pitch-based convention (v2): corner (r, c) sits at board-pixel
    ((c+1)*square_px, (r+1)*square_px); mm = (px - board_px/2) * pixel_pitch.
    For a 9x9 board at square_px=60 (board 540px) with pitch 0.667:
      corner (0,0) -> x = (60 - 270)*0.667 = -140.07
    """
    m = _mapping()
    p0 = m.charuco_corner_local_mm("V000_R000", 0, squares_x=9, squares_y=9, square_px=60)
    expected_x = (1 * 60 - 9 * 60 / 2) * 0.667  # -140.07
    expected_y = (1 * 60 - 9 * 60 / 2) * 0.667
    assert np.allclose(p0[:2], [expected_x, expected_y], atol=1e-6), (
        f"Pitch-based spacing mismatch: got {p0[:2]}, expected [{expected_x:.4f}, {expected_y:.4f}]"
    )


def test_charuco_corner_local_mm_unknown_cabinet():
    """Raises ScreenMappingError for unknown cabinet_id."""
    m = _mapping()
    with pytest.raises(ScreenMappingError):
        m.charuco_corner_local_mm("NONEXISTENT", 0, squares_x=9, squares_y=9, square_px=60)


def test_charuco_corner_local_mm_rotation_guard():
    """rotation != 0 must raise ScreenMappingError (not silently return wrong coords)."""
    data = {
        "screen_id": "S",
        "cabinets": [{
            "cabinet_id": "C90",
            "resolution_px": [510, 900],
            "active_size_mm": [340, 600],
            "pixel_pitch_mm": [0.667, 0.667],
            "active_origin": "center",
            "input_rect_px": [0, 0, 510, 900],
            "rotation": 90,
            "mirror_x": False,
            "mirror_y": False,
        }],
        "expected_pattern_hash": "abc",
    }
    m = ScreenMapping.model_validate(data)
    with pytest.raises(ScreenMappingError, match="rotation/mirror"):
        m.charuco_corner_local_mm("C90", 0, squares_x=9, squares_y=9, square_px=60)


def test_charuco_corner_local_mm_mirror_x_guard():
    """mirror_x=True must raise ScreenMappingError."""
    data = {
        "screen_id": "S",
        "cabinets": [{
            "cabinet_id": "CM",
            "resolution_px": [900, 510],
            "active_size_mm": [600, 340],
            "pixel_pitch_mm": [0.667, 0.667],
            "active_origin": "center",
            "input_rect_px": [0, 0, 900, 510],
            "rotation": 0,
            "mirror_x": True,
            "mirror_y": False,
        }],
        "expected_pattern_hash": "abc",
    }
    m = ScreenMapping.model_validate(data)
    with pytest.raises(ScreenMappingError, match="rotation/mirror"):
        m.charuco_corner_local_mm("CM", 0, squares_x=9, squares_y=9, square_px=60)


# ---------------------------------------------------------------------------
# preflight
# ---------------------------------------------------------------------------

def test_preflight_passes_on_correct_hash():
    """preflight returns None when hash matches."""
    m = _mapping()
    result = m.preflight(actual_pattern_hash="abc123")
    assert result is None


def test_preflight_rejects_pattern_hash_mismatch():
    """preflight raises ScreenMappingError on hash mismatch."""
    m = _mapping()
    with pytest.raises(ScreenMappingError):
        m.preflight(actual_pattern_hash="WRONG")


def test_construction_rejects_invalid_rotation():
    """rotation=45 is rejected at model construction, not at preflight."""
    # model_post_init rejects values not in {0,90,180,270}; pydantic v2 wraps the
    # raised ValueError in a ValidationError (which subclasses ValueError).
    with pytest.raises(ValueError):
        ScreenMapping.model_validate({
            "screen_id": "S",
            "cabinets": [{
                "cabinet_id": "C",
                "resolution_px": [900, 510],
                "active_size_mm": [600, 340],
                "pixel_pitch_mm": [0.667, 0.667],
                "active_origin": "center",
                "input_rect_px": [0, 0, 900, 510],
                "rotation": 45,
                "mirror_x": False,
                "mirror_y": False,
            }],
            "expected_pattern_hash": "abc",
        })


def test_preflight_image_size_check():
    """preflight with matching image_size passes; mismatched raises ScreenMappingError."""
    m = _mapping()
    # Matching: resolution_px=[900,510] → image_size=(900,510)
    m.preflight(actual_pattern_hash="abc123", image_size=(900, 510))
    # Mismatched
    with pytest.raises(ScreenMappingError):
        m.preflight(actual_pattern_hash="abc123", image_size=(800, 400))


# ---------------------------------------------------------------------------
# ScreenMappingCabinet validation
# ---------------------------------------------------------------------------

def test_cabinet_resolution_must_be_positive():
    """resolution_px must have positive values."""
    with pytest.raises(Exception):
        ScreenMapping.model_validate({
            "screen_id": "S",
            "cabinets": [{
                "cabinet_id": "C",
                "resolution_px": [0, 510],
                "active_size_mm": [600, 340],
                "pixel_pitch_mm": [0.667, 0.667],
                "active_origin": "center",
                "input_rect_px": [0, 0, 900, 510],
                "rotation": 0,
                "mirror_x": False,
                "mirror_y": False,
            }],
            "expected_pattern_hash": "abc",
        })
