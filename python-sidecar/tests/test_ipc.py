"""Round-trip tests for IPC pydantic models."""
from __future__ import annotations

import json

import pytest
from pydantic import ValidationError

from lmt_vba_sidecar.ipc import (
    CabinetArray,
    CoordinateFrame,
    ProgressEvent,
    ResultEvent,
    WarningEvent,
    ErrorEvent,
    MeasuredPoint,
    PointSource,
    Uncertainty,
    ReconstructInput,
)


def _valid_reconstruct_input() -> dict:
    return {
        "command": "reconstruct",
        "version": 1,
        "project": {
            "screen_id": "MAIN",
            "coordinate_frame": {"origin_world": [0, 0, 0], "basis": [[1, 0, 0], [0, 1, 0], [0, 0, 1]]},
            "cabinet_array": {"cols": 4, "rows": 4, "cabinet_size_mm": [500, 500]},
            "shape_prior": "flat",
            "frame_strategy": "nominal_anchoring",
            "frame_anchors": None,
        },
        "images": ["a.jpg"],
        "intrinsics": {
            "K": [[1000, 0, 960], [0, 1000, 540], [0, 0, 1]],
            "dist_coeffs": [0, 0, 0, 0, 0],
            "image_size": [1920, 1080],
        },
        "pattern_meta": {
            "aruco_dict": "DICT_6X6_1000",
            "markers_per_cabinet": 64,
            "checkerboard_inner_corners": 8,
            "cabinets": [{"col": 0, "row": 0, "aruco_id_start": 0, "aruco_id_end": 63}],
        },
    }


def test_progress_event_serializes() -> None:
    ev = ProgressEvent(event="progress", stage="detect_charuco", percent=0.3, message="3/10")
    assert json.loads(ev.model_dump_json()) == {
        "event": "progress",
        "stage": "detect_charuco",
        "percent": 0.3,
        "message": "3/10",
    }


def test_measured_point_visual_ba_source() -> None:
    p = MeasuredPoint(
        name="MAIN_V001_R001",
        position=[1.0, 2.0, 3.0],
        uncertainty=Uncertainty(covariance=[[1e-4, 0, 0], [0, 1e-4, 0], [0, 0, 1e-4]]),
        source=PointSource(visual_ba={"camera_count": 5}),
    )
    payload = json.loads(p.model_dump_json())
    assert payload["source"] == {"visual_ba": {"camera_count": 5}}
    assert payload["uncertainty"] == {"covariance": [[1e-4, 0, 0], [0, 1e-4, 0], [0, 0, 1e-4]]}


def test_reconstruct_input_validates_frame_strategy() -> None:
    raw = {
        "command": "reconstruct",
        "version": 1,
        "project": {
            "screen_id": "MAIN",
            "coordinate_frame": {"origin_world": [0, 0, 0], "basis": [[1, 0, 0], [0, 1, 0], [0, 0, 1]]},
            "cabinet_array": {"cols": 4, "rows": 4, "cabinet_size_mm": [500, 500]},
            "shape_prior": "flat",
            "frame_strategy": "nominal_anchoring",
            "frame_anchors": None,
        },
        "images": ["a.jpg"],
        "intrinsics": {
            "K": [[1000, 0, 960], [0, 1000, 540], [0, 0, 1]],
            "dist_coeffs": [0, 0, 0, 0, 0],
            "image_size": [1920, 1080],
        },
        "pattern_meta": {
            "aruco_dict": "DICT_6X6_1000",
            "markers_per_cabinet": 64,
            "checkerboard_inner_corners": 8,
            "cabinets": [{"col": 0, "row": 0, "aruco_id_start": 0, "aruco_id_end": 63}],
        },
    }
    parsed = ReconstructInput.model_validate(raw)
    assert parsed.project.frame_strategy == "nominal_anchoring"


def test_result_event_round_trips() -> None:
    raw = {
        "event": "result",
        "data": {
            "measured_points": [],
            "ba_stats": {"rms_reprojection_px": 0.5, "iterations": 10, "converged": True},
            "frame_strategy_used": "nominal_anchoring",
        },
    }
    parsed = ResultEvent.model_validate(raw)
    assert parsed.data.ba_stats.converged is True


def test_three_points_strategy_requires_exactly_3_anchors() -> None:
    raw = _valid_reconstruct_input()
    raw["project"]["frame_strategy"] = "three_points"
    raw["project"]["frame_anchors"] = [
        {"cabinet_col": 0, "cabinet_row": 0, "aruco_id": 0, "position_world": [0, 0, 0]},
    ]
    with pytest.raises(ValidationError, match="three_points requires exactly 3"):
        ReconstructInput.model_validate(raw)


def test_nominal_anchoring_strategy_forbids_anchors() -> None:
    raw = _valid_reconstruct_input()
    raw["project"]["frame_anchors"] = [
        {"cabinet_col": 0, "cabinet_row": 0, "aruco_id": 0, "position_world": [0, 0, 0]},
        {"cabinet_col": 1, "cabinet_row": 0, "aruco_id": 64, "position_world": [0.5, 0, 0]},
        {"cabinet_col": 0, "cabinet_row": 1, "aruco_id": 128, "position_world": [0, 0.5, 0]},
    ]
    with pytest.raises(ValidationError, match="nominal_anchoring forbids"):
        ReconstructInput.model_validate(raw)


def test_ragged_K_matrix_rejected() -> None:
    raw = _valid_reconstruct_input()
    raw["intrinsics"]["K"] = [[1], [2], [3]]
    with pytest.raises(ValidationError):
        ReconstructInput.model_validate(raw)


def test_negative_cabinet_size_rejected() -> None:
    raw = _valid_reconstruct_input()
    raw["project"]["cabinet_array"]["cabinet_size_mm"] = [-500, 0]
    with pytest.raises(ValidationError):
        ReconstructInput.model_validate(raw)


def test_unknown_shape_prior_rejected() -> None:
    raw = _valid_reconstruct_input()
    raw["project"]["shape_prior"] = {"bogus": {}}
    with pytest.raises(ValidationError):
        ReconstructInput.model_validate(raw)


def test_curved_shape_prior_accepted() -> None:
    raw = _valid_reconstruct_input()
    raw["project"]["shape_prior"] = {"curved": {"radius_mm": 5000.0}}
    parsed = ReconstructInput.model_validate(raw)
    assert parsed.project.shape_prior.curved.radius_mm == 5000.0


def test_zero_image_size_rejected() -> None:
    raw = _valid_reconstruct_input()
    raw["intrinsics"]["image_size"] = [0, 1080]
    with pytest.raises(ValidationError):
        ReconstructInput.model_validate(raw)
