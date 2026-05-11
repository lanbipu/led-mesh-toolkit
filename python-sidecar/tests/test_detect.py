"""ChArUco detection on rendered patterns."""
from __future__ import annotations

import pathlib

import cv2
import numpy as np

from lmt_vba_sidecar.detect import detect_charuco_observations
from lmt_vba_sidecar.ipc import (
    CabinetArray,
    GeneratePatternInput,
    GeneratePatternProject,
)
from lmt_vba_sidecar.pattern import run_generate_pattern


def test_detect_finds_markers_on_rendered_pattern(tmp_out: pathlib.Path) -> None:
    project = GeneratePatternProject(
        screen_id="MAIN",
        cabinet_array=CabinetArray(cols=1, rows=1, cabinet_size_mm=[500.0, 500.0]),
    )
    cmd = GeneratePatternInput(
        command="generate_pattern", version=1, project=project,
        output_dir=str(tmp_out / "patterns"), screen_resolution=[720, 720],
    )
    assert run_generate_pattern(cmd) == 0

    full_path = str(tmp_out / "patterns" / "full_screen.png")
    obs = detect_charuco_observations(image_paths=[full_path])
    assert full_path in obs
    assert len(obs[full_path]) > 0
    aruco_ids = {m["aruco_id"] for m in obs[full_path]}
    assert aruco_ids.issubset(set(range(1000)))


def test_detect_returns_empty_for_blank(tmp_out: pathlib.Path) -> None:
    p = tmp_out / "blank.png"
    cv2.imwrite(str(p), np.full((720, 720), 255, dtype=np.uint8))
    obs = detect_charuco_observations(image_paths=[str(p)])
    assert obs[str(p)] == []


def test_detect_handles_unreadable_path(tmp_out: pathlib.Path) -> None:
    """Missing/corrupt image: returns empty list, not exception."""
    fake = tmp_out / "nonexistent.png"
    obs = detect_charuco_observations(image_paths=[str(fake)])
    assert obs[str(fake)] == []
