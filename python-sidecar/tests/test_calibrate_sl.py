# python-sidecar/tests/test_calibrate_sl.py
import json
import numpy as np
import pytest

from lmt_vba_sidecar.ipc import (
    CabinetArray, CabinetRect, CodeSpec, SequenceSpec, ReconstructProject,
    ShapePriorCurved, ShapePriorCurvedBody, StructuredLightDot, StructuredLightMeta,
    CalibrateStructuredLightInput,
)
from lmt_vba_sidecar.nominal import nominal_dot_positions_world
from lmt_vba_sidecar.sl_feasibility import look_at_pose, project_point
from lmt_vba_sidecar.calibrate_sl import run_calibrate_structured_light

K_TRUE = np.array([[3000.0, 0.0, 2000.0], [0.0, 3000.0, 1500.0], [0.0, 0.0, 1.0]])
IMG = (4000, 3000)


def _curved_meta(cols=4, radius_mm=4000.0, grid=4):
    cab = CabinetArray(cols=cols, rows=1, cabinet_size_mm=[500.0, 500.0])
    shape = ShapePriorCurved(curved=ShapePriorCurvedBody(radius_mm=radius_mm))
    rects, dots, did = [], [], 0
    px = 540
    for c in range(cols):
        rects.append(CabinetRect(col=c, row=0, input_rect_px=[c*px, 0, px, px], pixel_pitch_mm=[500.0/px, 500.0/px]))
        for i in range(grid):
            for j in range(grid):
                u = c*px + (i + 0.5) * px / grid
                v = (j + 0.5) * px / grid
                dots.append(StructuredLightDot(id=did, u=float(u), v=float(v), cabinet=[c, 0])); did += 1
    meta = StructuredLightMeta(
        schema_version=1, screen_id="MAIN", screen_resolution=[cols*px, px], dot_radius_px=4,
        code=CodeSpec(data_bits=8, total_bits=9), sequence=SequenceSpec(n_code_frames=9, hold_ms=100, fps=30),
        cabinets=rects, dots=dots,
    )
    proj = ReconstructProject(screen_id="MAIN", cabinet_array=cab, shape_prior=shape)
    return meta, proj, cab, shape


def _write_corr(tmp, meta, world, poses, sha="sha-test", noise=0.0, seed=0):
    rng = np.random.default_rng(seed)
    paths = []
    for vi, (R, t) in enumerate(poses):
        pts = []
        for d in meta.dots:
            p = project_point(K_TRUE, R, t, world[d.id]) + rng.normal(0, noise, 2)
            pts.append({"id": d.id, "u": d.u, "v": d.v, "x": float(p[0]), "y": float(p[1])})
        cp = tmp / f"corr_{vi}.json"
        cp.write_text(json.dumps({
            "schema_version": 1, "screen_id": "MAIN", "sl_meta_sha256": sha,
            "screen_resolution": meta.screen_resolution, "camera_image_size": list(IMG),
            "source_input": f"/cap/pose{vi}.mp4", "points": pts,
        }))
        paths.append(str(cp))
    return paths


def _ring_poses(n=4, dist_m=6.0):
    # Cameras on a shallow arc in front of the wall (meters; world is meters).
    poses = []
    for k in range(n):
        x = -1.0 + 2.0 * k / max(1, n - 1)
        poses.append(look_at_pose(np.array([x, 0.0, -dist_m]), np.array([1.0, 0.0, 0.0])))
    return poses


def _run(tmp, meta, proj, paths):
    import hashlib
    meta_path = tmp / "sl_meta.json"
    meta_path.write_text(meta.model_dump_json())
    sha = hashlib.sha256(meta_path.read_bytes()).hexdigest()
    for p in paths:
        d = json.loads(open(p).read()); d["sl_meta_sha256"] = sha; open(p, "w").write(json.dumps(d))
    out = tmp / "sl_intrinsics.json"
    cmd = CalibrateStructuredLightInput(
        command="calibrate_structured_light", version=1, project=proj,
        correspondence_paths=paths, sl_meta_path=str(meta_path), output_path=str(out),
    )
    rc = run_calibrate_structured_light(cmd)
    return rc, out


def test_recovers_K_noise_free(tmp_path):
    meta, proj, cab, shape = _curved_meta()
    world = nominal_dot_positions_world(meta, cab, shape)
    paths = _write_corr(tmp_path, meta, world, _ring_poses(4), noise=0.0)
    rc, out = _run(tmp_path, meta, proj, paths)
    assert rc == 0
    intr = json.loads(out.read_text())
    K = np.array(intr["K"])
    assert abs(K[0, 0] - 3000.0) / 3000.0 < 0.01
    assert abs(K[0, 2] - 2000.0) < 1.5
    assert abs(K[1, 2] - 1500.0) < 1.5
    assert intr["calibration_method"] == "structured_light_nominal"
    assert intr["frames_used"] == 4


def test_recovers_K_with_noise_within_budget(tmp_path):
    meta, proj, cab, shape = _curved_meta()
    world = nominal_dot_positions_world(meta, cab, shape)
    paths = _write_corr(tmp_path, meta, world, _ring_poses(4), noise=0.3)
    rc, out = _run(tmp_path, meta, proj, paths)
    assert rc == 0
    K = np.array(json.loads(out.read_text())["K"])
    assert abs(K[0, 0] - 3000.0) / 3000.0 < 0.02


def test_structured_deviation_within_budget_or_refused(tmp_path):
    meta, proj, cab, shape = _curved_meta(radius_mm=4000.0)
    dev_shape = ShapePriorCurved(curved=ShapePriorCurvedBody(radius_mm=4080.0))
    truth_world = nominal_dot_positions_world(meta, cab, dev_shape)
    paths = _write_corr(tmp_path, meta, truth_world, _ring_poses(4), noise=0.3)
    rc, out = _run(tmp_path, meta, proj, paths)
    if rc == 0:
        K = np.array(json.loads(out.read_text())["K"])
        assert abs(K[0, 0] - 3000.0) / 3000.0 < 0.02, "absorbed deviation blew the focal budget without refusing"
    else:
        assert rc == 1


def test_near_flat_single_pose_refused(tmp_path):
    cab = CabinetArray(cols=1, rows=1, cabinet_size_mm=[500.0, 500.0])
    px = 540
    rect = CabinetRect(col=0, row=0, input_rect_px=[0, 0, px, px], pixel_pitch_mm=[500.0/px, 500.0/px])
    dots, did = [], 0
    for i in range(6):
        for j in range(6):
            dots.append(StructuredLightDot(id=did, u=(i+0.5)*px/6, v=(j+0.5)*px/6, cabinet=[0, 0])); did += 1
    meta = StructuredLightMeta(schema_version=1, screen_id="MAIN", screen_resolution=[px, px], dot_radius_px=4,
        code=CodeSpec(data_bits=8, total_bits=9), sequence=SequenceSpec(n_code_frames=9, hold_ms=100, fps=30),
        cabinets=[rect], dots=dots)
    proj = ReconstructProject(screen_id="MAIN", cabinet_array=cab, shape_prior="flat")
    world = nominal_dot_positions_world(meta, cab, "flat")
    paths = _write_corr(tmp_path, meta, world, _ring_poses(1), noise=0.0)
    rc, _ = _run(tmp_path, meta, proj, paths)
    assert rc == 1


def test_near_duplicate_poses_refused(tmp_path):
    meta, proj, cab, shape = _curved_meta()
    world = nominal_dot_positions_world(meta, cab, shape)
    dup = [look_at_pose(np.array([0.0 + 1e-3*k, 0.0, -6.0]), np.array([1.0, 0.0, 0.0])) for k in range(3)]
    paths = _write_corr(tmp_path, meta, world, dup, noise=0.1)
    rc, _ = _run(tmp_path, meta, proj, paths)
    assert rc == 1
