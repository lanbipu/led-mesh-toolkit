"""End-to-end reconstruct tests for the model-constrained (zero total-station)
pipeline.

The synthetic_charuco_capture fixture (tests/conftest.py) renders two real
ChArUco boards at a known distance / inter-board angle, captured from many
views. reconstruct must recover that geometry from screen-mapping local mm
alone (no anchors, no world datum)."""
from __future__ import annotations

import io
import json
import pathlib

import numpy as np

from lmt_vba_sidecar.ipc import ReconstructInput
from lmt_vba_sidecar.reconstruct import run_reconstruct


def _build_input(paths: dict) -> ReconstructInput:
    return ReconstructInput.model_validate(
        {
            "command": "reconstruct",
            "version": 1,
            "project": {
                "screen_id": "S",
                # cabinet_size_mm is only the nominal BA-init seed grid. This
                # 2x1 horizontal layout uses only the x spacing for init, so
                # the height (340) has no geometric effect here; the actual
                # panel size / corners come from screen_mapping's SQUARE active
                # surface (600x600 — square because a ChArUco board PNG must
                # fill its canvas with no letterbox to keep the local-mm chain
                # exact). BA still recovers the true 700mm / 10deg regardless.
                "cabinet_array": {"cols": 2, "rows": 1, "cabinet_size_mm": [600, 340]},
                "shape_prior": "flat",
            },
            "capture_manifest_path": paths["capture"],
            "screen_mapping_path": paths["screen_mapping"],
            "pose_report_path": paths["pose_report"],
        }
    )


def test_reconstruct_writes_pose_report_and_matches_known_geometry(
    synthetic_charuco_capture, capsys,
):
    paths = synthetic_charuco_capture
    rc = run_reconstruct(_build_input(paths))
    assert rc == 0

    rep = json.loads(open(paths["pose_report"]).read())
    assert rep["schema_version"] == "visual_pose_report.v1"

    # --- gauge frame invariants (the "zero total station" design center) ---
    assert rep["frame"]["gauge_strategy"] == "fix_root_cabinet"
    assert rep["frame"]["root_cabinet"] == [0, 0]

    poses = {p["cabinet_id"]: p for p in rep["cabinet_poses"]}
    c0 = np.array(poses["V000_R000"]["position_mm"])
    c1 = np.array(poses["V001_R000"]["position_mm"])

    # Root cabinet is the gauge: fixed at origin with identity rotation.
    assert np.allclose(c0, [0.0, 0.0, 0.0], atol=1e-6)
    assert np.allclose(
        np.array(poses["V000_R000"]["rotation_matrix"]), np.eye(3), atol=1e-6
    )

    # --- recovered geometry matches known truth ---
    assert abs(np.linalg.norm(c1 - c0) - 700.0) < 5.0
    n0 = np.array(poses["V000_R000"]["normal"])
    n1 = np.array(poses["V001_R000"]["normal"])
    ang = np.degrees(np.arccos(np.clip(n0 @ n1, -1, 1)))
    assert abs(ang - 10.0) < 0.5

    # --- measured_points: count / names / mm->m conversion ---
    result = json.loads(
        [ln for ln in capsys.readouterr().out.splitlines() if ln.strip()][-1]
    )
    assert result["event"] == "result"
    mps = result["data"]["measured_points"]
    assert len(mps) == 2
    by_name = {m["name"]: m for m in mps}
    assert set(by_name) == {"MAIN_V000_R000", "MAIN_V001_R000"}
    # Positions are in METERS: root at origin, second cabinet ~0.7m in x.
    p0 = np.array(by_name["MAIN_V000_R000"]["position"])
    p1 = np.array(by_name["MAIN_V001_R000"]["position"])
    assert np.allclose(p0, [0.0, 0.0, 0.0], atol=1e-6)
    assert abs(p1[0] - 0.7) < 0.005


def test_reconstruct_structured_light_method_rejected(synthetic_charuco_capture):
    """The capture manifest method gates the pipeline: structured-light is not
    implemented and must fail closed with the invalid_input envelope."""
    paths = synthetic_charuco_capture
    cap_path = pathlib.Path(paths["capture"])
    manifest = json.loads(cap_path.read_text())
    manifest["method"] = "structured-light"
    # Structured-light views need a frames list (charuco needs images); supply
    # a minimal frames entry so manifest loading reaches the method gate.
    for view in manifest["views"]:
        view["frames"] = [{"path": view["images"][0]}]
        view.pop("images", None)
    sl_path = cap_path.with_name("capture_sl.json")
    sl_path.write_text(json.dumps(manifest))

    inp = _build_input({**paths, "capture": str(sl_path)})

    import contextlib

    buf = io.StringIO()
    with contextlib.redirect_stdout(buf):
        rc = run_reconstruct(inp)
    assert rc == 1
    last = json.loads([ln for ln in buf.getvalue().splitlines() if ln.strip()][-1])
    assert last["event"] == "error"
    assert last["code"] == "invalid_input"
