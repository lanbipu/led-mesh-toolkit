import pytest
from lmt_vba_sidecar.ipc import CorrespondenceFile
from lmt_vba_sidecar.sl_reconstruct import validate_sl_provenance


def _corr(screen_id="MAIN", sha="abc"):
    return CorrespondenceFile.model_validate({
        "schema_version": 1, "screen_id": screen_id, "sl_meta_sha256": sha,
        "screen_resolution": [960, 540], "camera_image_size": [4000, 3000],
        "source_input": "/cap/p.mp4",
        "points": [{"id": 0, "u": 1.0, "v": 2.0, "x": 3.0, "y": 4.0}]})


def test_provenance_accepts_consistent_set():
    validate_sl_provenance([_corr(), _corr()], expected_sha="abc", expected_screen_id="MAIN")


def test_provenance_rejects_mixed_screen_id():
    with pytest.raises(ValueError, match="screen_id"):
        validate_sl_provenance([_corr(screen_id="MAIN"), _corr(screen_id="FLOOR")],
                               expected_sha="abc", expected_screen_id="MAIN")


def test_provenance_rejects_sha_mismatch_vs_meta():
    with pytest.raises(ValueError, match="sl_meta_sha256"):
        validate_sl_provenance([_corr(sha="abc")], expected_sha="DIFFERENT",
                               expected_screen_id="MAIN")


def test_provenance_rejects_screen_id_not_matching_project():
    with pytest.raises(ValueError, match="project"):
        validate_sl_provenance([_corr(screen_id="MAIN")], expected_sha="abc",
                               expected_screen_id="FLOOR")


import json, hashlib, pathlib
import numpy as np
from lmt_vba_sidecar.ipc import GenerateStructuredLightInput, ReconstructStructuredLightInput
from lmt_vba_sidecar.structured_light import run_generate_structured_light
from lmt_vba_sidecar.sl_geometry import sl_local_mm
from lmt_vba_sidecar.sl_feasibility import look_at_pose, project_point
from lmt_vba_sidecar.sl_reconstruct import run_reconstruct_structured_light


def _gen_two_cabinet_meta(tmp_path):
    cmd = GenerateStructuredLightInput.model_validate({
        "command": "generate_structured_light", "version": 1,
        "project": {"screen_id": "MAIN",
                    "cabinet_array": {"cols": 2, "rows": 1, "absent_cells": [],
                                      "cabinet_size_mm": [500, 500]}},
        "output_dir": str(tmp_path / "sl"), "screen_resolution": [960, 480],
        "dot_spacing_px": 80, "margin_px": 60})
    assert run_generate_structured_light(cmd) == 0
    return tmp_path / "sl" / "sl_meta.json"


def _write_intrinsics(tmp_path, f=3000.0, cx=2000.0, cy=1500.0, w=4000, h=3000):
    p = tmp_path / "intr.json"
    p.write_text(json.dumps({"K": [[f, 0, cx], [0, f, cy], [0, 0, 1]],
                             "dist_coeffs": [0, 0, 0, 0, 0], "image_size": [w, h]}))
    return p, np.array([[f, 0, cx], [0, f, cy], [0, 0, 1]], float)


def test_synthetic_sl_reconstruction_recovers_cabinet_offset_mm(tmp_path):
    """Synthetic perfect correspondences (+0.1px noise) for a 2-cabinet wall with
    a KNOWN deviation on cabinet 1; recovered pose must place it within mm of
    the true (deviated) position. This is the Phase-3 gating test."""
    meta_path = _gen_two_cabinet_meta(tmp_path)
    meta = json.loads(meta_path.read_text())
    intr_path, K = _write_intrinsics(tmp_path)
    rect_by_cr = {(c["col"], c["row"]): c["input_rect_px"] for c in meta["cabinets"]}
    pitch_by_cr = {(c["col"], c["row"]): c["pixel_pitch_mm"] for c in meta["cabinets"]}
    cab_by_id = {d["id"]: tuple(d["cabinet"]) for d in meta["dots"]}

    # True world: root (0,0) frame = world; cabinet (1,0) nominally +500mm x,
    # plus a KNOWN 4mm deviation (3mm x, 2mm y, 1mm z) we expect to recover.
    nominal_offset = np.array([500.0, 0.0, 0.0])
    deviation = np.array([3.0, 2.0, 1.0])
    cab_world_t = {(0, 0): np.zeros(3), (1, 0): nominal_offset + deviation}

    truth_world = {}
    for d in meta["dots"]:
        cr = cab_by_id[d["id"]]
        p_local = sl_local_mm(tuple(rect_by_cr[cr]), d["u"], d["v"],
                              pitch_by_cr[cr][0], pitch_by_cr[cr][1])
        truth_world[d["id"]] = p_local + cab_world_t[cr]   # identity cabinet rotation

    sha = hashlib.sha256(meta_path.read_bytes()).hexdigest()
    poses = [look_at_pose(np.array([px, 0.0, -3500.0]), np.array([250.0, 0.0, 0.0]))
             for px in (-1200.0, -400.0, 400.0, 1200.0)]
    rng = np.random.default_rng(0)
    corr_paths = []
    for vi, (R, t) in enumerate(poses):
        pts = []
        for d in meta["dots"]:
            p = project_point(K, R, t, truth_world[d["id"]]) + rng.normal(0, 0.1, 2)
            pts.append({"id": d["id"], "u": d["u"], "v": d["v"],
                        "x": float(p[0]), "y": float(p[1])})
        cp = tmp_path / f"corr_{vi}.json"
        cp.write_text(json.dumps({
            "schema_version": 1, "screen_id": "MAIN", "sl_meta_sha256": sha,
            "screen_resolution": meta["screen_resolution"], "camera_image_size": [4000, 3000],
            "source_input": f"/cap/pose{vi}.mp4", "points": pts}))
        corr_paths.append(str(cp))

    report_path = tmp_path / "report.json"
    cmd = ReconstructStructuredLightInput.model_validate({
        "command": "reconstruct_structured_light", "version": 1,
        "project": {"screen_id": "MAIN",
                    "cabinet_array": {"cols": 2, "rows": 1, "absent_cells": [],
                                      "cabinet_size_mm": [500, 500]}},
        "correspondence_paths": corr_paths, "sl_meta_path": str(meta_path),
        "intrinsics_path": str(intr_path), "pose_report_path": str(report_path)})
    assert run_reconstruct_structured_light(cmd) == 0

    report = json.loads(report_path.read_text())
    by_id = {c["cabinet_id"]: c for c in report["cabinet_poses"]}
    true_center_10 = cab_world_t[(1, 0)]
    got = np.array(by_id["V001_R000"]["position_mm"])
    assert np.linalg.norm(got - true_center_10) < 5.0   # mm (conservative; BA + 0.1px noise)

    # Finding-2 guard: correspondence (u,v) must be IGNORED (canonical (u,v) comes
    # from sl_meta). Corrupt every corr point's u,v to garbage, reconstruct again ->
    # identical cabinet pose. If p_local trusted corr (u,v), this would diverge/fail.
    for cp in corr_paths:
        d = json.loads(pathlib.Path(cp).read_text())
        for p in d["points"]:
            p["u"], p["v"] = 0.0, 0.0
        pathlib.Path(cp).write_text(json.dumps(d))
    report2 = tmp_path / "report2.json"
    assert run_reconstruct_structured_light(cmd.model_copy(update={"pose_report_path": str(report2)})) == 0
    got2 = np.array({c["cabinet_id"]: c for c in json.loads(report2.read_text())["cabinet_poses"]}
                    ["V001_R000"]["position_mm"])
    np.testing.assert_allclose(got, got2, atol=1e-6)


def _valid_corr(tmp_path, sha, n=2, screen_res=(960, 480)):
    """n minimal corr files that pass provenance (shared screen_id MAIN + sha)."""
    paths = []
    for i in range(n):
        cp = tmp_path / f"vc{i}.json"
        cp.write_text(json.dumps({
            "schema_version": 1, "screen_id": "MAIN", "sl_meta_sha256": sha,
            "screen_resolution": list(screen_res), "camera_image_size": [4000, 3000],
            "source_input": "x", "points": [{"id": 0, "u": 1, "v": 1, "x": 1, "y": 1}]}))
        paths.append(str(cp))
    return paths


def test_run_rejects_malformed_sl_meta(tmp_path):
    bad = tmp_path / "bad_meta.json"
    bad.write_text('{"schema_version": 1, "screen_id": "MAIN"}')   # missing required fields
    intr_path, _ = _write_intrinsics(tmp_path)
    cmd = ReconstructStructuredLightInput.model_validate({
        "command": "reconstruct_structured_light", "version": 1,
        "project": {"screen_id": "MAIN",
                    "cabinet_array": {"cols": 2, "rows": 1, "absent_cells": [],
                                      "cabinet_size_mm": [500, 500]}},
        "correspondence_paths": _valid_corr(tmp_path, "x"),
        "sl_meta_path": str(bad), "intrinsics_path": str(intr_path)})
    assert run_reconstruct_structured_light(cmd) == 1     # invalid_input, not a traceback


def test_run_rejects_meta_project_cabinet_mismatch(tmp_path):
    # sl_meta present = {(0,0),(1,0)}; project declares (1,0) ABSENT -> {(0,0)}.
    meta_path = _gen_two_cabinet_meta(tmp_path)
    sha = hashlib.sha256(meta_path.read_bytes()).hexdigest()
    intr_path, _ = _write_intrinsics(tmp_path)
    cmd = ReconstructStructuredLightInput.model_validate({
        "command": "reconstruct_structured_light", "version": 1,
        "project": {"screen_id": "MAIN",
                    "cabinet_array": {"cols": 2, "rows": 1, "absent_cells": [[1, 0]],
                                      "cabinet_size_mm": [500, 500]}},
        "correspondence_paths": _valid_corr(tmp_path, sha),
        "sl_meta_path": str(meta_path), "intrinsics_path": str(intr_path)})
    assert run_reconstruct_structured_light(cmd) == 1     # cabinet-set mismatch -> invalid_input


def test_run_rejects_provenance_mismatch(tmp_path):
    meta_path = _gen_two_cabinet_meta(tmp_path)
    intr_path, _ = _write_intrinsics(tmp_path)
    # two corr files with DIFFERENT sha -> invalid_input (return 1)
    for vi, sha in enumerate(("aaa", "bbb")):
        (tmp_path / f"c{vi}.json").write_text(json.dumps({
            "schema_version": 1, "screen_id": "MAIN", "sl_meta_sha256": sha,
            "screen_resolution": [960, 480], "camera_image_size": [4000, 3000],
            "source_input": "x", "points": [{"id": 0, "u": 1, "v": 1, "x": 1, "y": 1}]}))
    cmd = ReconstructStructuredLightInput.model_validate({
        "command": "reconstruct_structured_light", "version": 1,
        "project": {"screen_id": "MAIN",
                    "cabinet_array": {"cols": 2, "rows": 1, "absent_cells": [],
                                      "cabinet_size_mm": [500, 500]}},
        "correspondence_paths": [str(tmp_path / "c0.json"), str(tmp_path / "c1.json")],
        "sl_meta_path": str(meta_path), "intrinsics_path": str(intr_path)})
    assert run_reconstruct_structured_light(cmd) == 1
