"""VP-QSP reconstruct integration: detect -> Observation -> shared BA.

Uses a FAITHFUL (proper / det+1) synthetic correspondence by projecting
marker_local_mm through known camera poses (exactly what a real LED-panel capture
produces). The detector is validated separately on rendered images
(test_vpqsp_detect); here we monkeypatch it to isolate the reconstruct wiring +
the model-constrained BA, and assert the known scene geometry is recovered.
"""
from __future__ import annotations

import json

import cv2
import numpy as np
import pytest

import lmt_vba_sidecar.vpqsp_detect as vpqsp_detect
from lmt_vba_sidecar.ipc import ReconstructInput, VpqspPatternMeta
from lmt_vba_sidecar.reconstruct import pattern_hash, run_reconstruct
from lmt_vba_sidecar.vpqsp_layout import choose_marker_grid, marker_local_mm

_ACTIVE = 600.0
_RES = (630, 630)
_IMG = (1920, 1080)
_DIST_MM = 700.0
_ANGLE_DEG = 10.0
_K = np.array([[2400.0, 0, _IMG[0] / 2], [0, 2400.0, _IMG[1] / 2], [0, 0, 1]], float)


def _camera_poses():
    center = np.array([_DIST_MM / 2.0, 0.0, 0.0])
    out = []
    for yaw in (-25, -12, 0, 12, 25):
        for pit in (-12, 0, 12):
            y, p = np.deg2rad(yaw), np.deg2rad(pit)
            cp = center + 2200.0 * np.array([np.sin(y) * np.cos(p), np.sin(p), -np.cos(y) * np.cos(p)])
            fwd = center - cp; fwd /= np.linalg.norm(fwd)
            up = np.array([0.0, -1.0, 0.0])
            right = np.cross(up, fwd); right /= np.linalg.norm(right)
            up2 = np.cross(fwd, right)
            Rc = np.stack([right, up2, fwd])
            out.append((Rc, -Rc @ cp))
    return out


def _build_capture(tmp_path, *, screen_id_code=0, cab1_view_limit=None):
    """Write meta/screen_mapping/intrinsics/manifest; return (paths, proper detections)."""
    mx, my, mpx = choose_marker_grid(_RES)
    pitch = (_ACTIVE / _RES[0], _ACTIVE / _RES[1])
    R0, T0 = np.eye(3), np.zeros(3)
    R1, _ = cv2.Rodrigues(np.array([0.0, np.deg2rad(_ANGLE_DEG), 0.0]))
    T1 = np.array([_DIST_MM, 0.0, 0.0])
    cab_defs = [((0, 0), R0, T0), ((1, 0), R1, T1)]

    cap = tmp_path / "capture"
    cap.mkdir()
    rng = np.random.default_rng(0)
    detections: dict[str, list] = {}
    views = []
    for vi, (Rc, tc) in enumerate(_camera_poses()):
        path = str(cap / f"cam_{vi:03d}.png")
        views.append({"view_id": f"cam_{vi:03d}", "images": [f"cam_{vi:03d}.png"]})
        obs = []
        for (cr, Rb, Tb) in cab_defs:
            if cr == (1, 0) and cab1_view_limit is not None and vi >= cab1_view_limit:
                continue
            for lid in range(mx * my):
                pl = marker_local_mm(lid, markers_x=mx, markers_y=my, resolution_px=_RES, pixel_pitch_mm=pitch)
                cam = Rc @ (Rb @ pl + Tb) + tc
                if cam[2] <= 0:
                    continue
                uv = _K @ cam
                uv = uv[:2] / uv[2] + rng.normal(0, 0.2, 2)
                obs.append({"cabinet": cr, "screen_id": screen_id_code, "local_id": lid,
                            "corner_px": [float(uv[0]), float(uv[1])]})
        detections[path] = obs

    def _meta_cab(col, row):
        return {"col": col, "row": row, "resolution_px": list(_RES), "markers_x": mx,
                "markers_y": my, "marker_px": mpx, "pixel_pitch_mm": list(pitch)}

    meta = VpqspPatternMeta.model_validate(
        {"schema_version": "vpqsp.v1", "screen_id_code": screen_id_code,
         "cabinets": [_meta_cab(0, 0), _meta_cab(1, 0)]})
    (cap / "pattern_meta.json").write_text(meta.model_dump_json())
    (cap / "intrinsics.json").write_text(json.dumps(
        {"K": _K.tolist(), "dist_coeffs": [0, 0, 0, 0, 0], "image_size": list(_IMG)}))

    def _sm_cab(cid):
        return {"cabinet_id": cid, "resolution_px": list(_RES), "active_size_mm": [_ACTIVE, _ACTIVE],
                "pixel_pitch_mm": list(pitch), "active_origin": "center",
                "input_rect_px": [0, 0, _RES[0], _RES[1]], "rotation": 0,
                "mirror_x": False, "mirror_y": False}

    (cap / "screen_mapping.json").write_text(json.dumps(
        {"screen_id": "S", "cabinets": [_sm_cab("V000_R000"), _sm_cab("V001_R000")],
         "expected_pattern_hash": pattern_hash(meta)}))
    (cap / "capture.json").write_text(json.dumps(
        {"method": "vpqsp", "intrinsics": "intrinsics.json", "pattern_meta": "pattern_meta.json",
         "screen_mapping": "screen_mapping.json", "views": views}))
    return {
        "capture": str(cap / "capture.json"),
        "screen_mapping": str(cap / "screen_mapping.json"),
        "pose_report": str(tmp_path / "pose.json"),
    }, detections


def _patch_detector(monkeypatch, detections):
    def fake(paths, *, screen_id_code=None, config=None):
        return {p: [o for o in detections.get(p, [])
                    if screen_id_code is None or o["screen_id"] == screen_id_code]
                for p in paths}
    monkeypatch.setattr(vpqsp_detect, "detect_vpqsp_markers", fake)


def _input(paths) -> ReconstructInput:
    return ReconstructInput.model_validate({
        "command": "reconstruct", "version": 1,
        "project": {"screen_id": "S",
                    "cabinet_array": {"cols": 2, "rows": 1, "cabinet_size_mm": [600, 600]},
                    "shape_prior": "flat"},
        "capture_manifest_path": paths["capture"],
        "screen_mapping_path": paths["screen_mapping"],
        "pose_report_path": paths["pose_report"],
    })


def _result(out: str) -> dict:
    for line in out.splitlines():
        line = line.strip()
        if line and json.loads(line).get("event") == "result":
            return json.loads(line)
    raise AssertionError("no result event")


def _error(out: str) -> dict:
    for line in out.splitlines():
        line = line.strip()
        if line and json.loads(line).get("event") == "error":
            return json.loads(line)
    raise AssertionError("no error event")


def test_vpqsp_reconstruct_recovers_known_geometry(tmp_path, capsys, monkeypatch):
    paths, dets = _build_capture(tmp_path)
    _patch_detector(monkeypatch, dets)
    assert run_reconstruct(_input(paths)) == 0
    data = _result(capsys.readouterr().out)["data"]
    assert data["ba_stats"]["rms_reprojection_px"] < 1.0
    names = {mp["name"] for mp in data["measured_points"]}
    assert names == {"MAIN_V000_R000", "MAIN_V001_R000"}  # MeasuredPoint prefix is fixed "MAIN_"

    pose = json.loads((tmp_path / "pose.json").read_text())
    norm = {c["cabinet_id"]: np.array(c["normal"]) for c in pose["cabinet_poses"]}
    pos = {c["cabinet_id"]: np.array(c["position_mm"]) for c in pose["cabinet_poses"]}
    angle = np.degrees(np.arccos(np.clip(norm["V000_R000"] @ norm["V001_R000"], -1, 1)))
    dist = np.linalg.norm(pos["V001_R000"] - pos["V000_R000"])
    assert abs(angle - _ANGLE_DEG) < 0.5   # inter-cabinet angle recovered
    assert abs(dist - _DIST_MM) < 10.0     # inter-cabinet distance recovered (mm scale)


def test_vpqsp_reconstruct_no_detections_is_detection_failed(tmp_path, capsys, monkeypatch):
    paths, _ = _build_capture(tmp_path)
    _patch_detector(monkeypatch, {})  # detector finds nothing
    assert run_reconstruct(_input(paths)) == 1
    assert _error(capsys.readouterr().out)["code"] == "detection_failed"


def test_vpqsp_reconstruct_single_view_cabinet_is_observability_failed(tmp_path, capsys, monkeypatch):
    # Cabinet (1,0) seen in only ONE view -> under-determined -> observability_failed.
    paths, dets = _build_capture(tmp_path, cab1_view_limit=1)
    _patch_detector(monkeypatch, dets)
    assert run_reconstruct(_input(paths)) == 1
    assert _error(capsys.readouterr().out)["code"] == "observability_failed"


def test_vpqsp_reconstruct_scale_mismatch_is_invalid_input(tmp_path, capsys, monkeypatch):
    # screen_mapping.active_size_mm (pose-report corner scale) diverges >1% from the
    # vpqsp pattern_meta resolution_px*pixel_pitch_mm (BA scale) -> fail loud, not a
    # silent metric mismatch. (Guards the integration reviewer's high finding.)
    paths, dets = _build_capture(tmp_path)
    _patch_detector(monkeypatch, dets)
    sm_path = tmp_path / "capture" / "screen_mapping.json"
    sm = json.loads(sm_path.read_text())
    # Keep screen_mapping INTERNALLY consistent (res*pitch==active, so its own
    # model validator passes) but 10% larger than the vpqsp pattern_meta scale —
    # the cross-source divergence only the new VP-QSP preflight catches. The
    # pattern_hash (over vpqsp_meta) is unchanged, so preflight hash still passes.
    big = _ACTIVE * 1.1
    sm["cabinets"][1]["active_size_mm"] = [big, big]
    sm["cabinets"][1]["pixel_pitch_mm"] = [big / _RES[0], big / _RES[1]]
    sm_path.write_text(json.dumps(sm))
    assert run_reconstruct(_input(paths)) == 1
    err = _error(capsys.readouterr().out)
    assert err["code"] == "invalid_input"
    assert "scale mismatch" in err["message"]


def test_vpqsp_reconstruct_rotation_is_invalid_input(tmp_path, capsys, monkeypatch):
    # A rotated/mirrored cabinet is not yet supported in VP-QSP local-mm mapping;
    # the charuco path fails loud here, so VP-QSP must too (no silent ignore).
    paths, dets = _build_capture(tmp_path)
    _patch_detector(monkeypatch, dets)
    sm_path = tmp_path / "capture" / "screen_mapping.json"
    sm = json.loads(sm_path.read_text())
    sm["cabinets"][0]["rotation"] = 90
    sm_path.write_text(json.dumps(sm))
    assert run_reconstruct(_input(paths)) == 1
    assert _error(capsys.readouterr().out)["code"] == "invalid_input"


def test_vpqsp_reconstruct_screen_id_filter(tmp_path, capsys, monkeypatch):
    # Markers encode screen 4; meta declares screen 4 -> kept. A meta declaring a
    # different screen would filter everything out (multi-screen Volume isolation).
    paths, dets = _build_capture(tmp_path, screen_id_code=4)
    _patch_detector(monkeypatch, dets)
    assert run_reconstruct(_input(paths)) == 0
    assert _result(capsys.readouterr().out)["data"]["ba_stats"]["rms_reprojection_px"] < 1.0
