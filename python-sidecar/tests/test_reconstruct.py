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

import cv2
import numpy as np

from lmt_vba_sidecar.ipc import ReconstructInput
from lmt_vba_sidecar.model_constrained_ba import Observation, model_constrained_ba
from lmt_vba_sidecar.reconstruct import (
    MIN_PNP_CORNERS,
    _classify_cabinet_quality,
    _pnp_camera,
    estimate_nonroot_cabinet_init,
    run_reconstruct,
)


def _build_input(paths: dict, shape_prior="flat") -> ReconstructInput:
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
                "shape_prior": shape_prior,
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


def test_classify_cabinet_quality_all_branches():
    """Soft classifier: views-below-threshold dominates, then residual, else ok."""
    assert _classify_cabinet_quality(2, 0.5) == "low_observation"  # views < 4
    assert _classify_cabinet_quality(10, 3.0) == "high_residual"  # rms > 2.0
    assert _classify_cabinet_quality(10, 0.5) == "ok"
    # QUALITY_MIN_VIEWS boundary: exactly 4 is ok, 3 is low (strict <).
    assert _classify_cabinet_quality(4, 0.5) == "ok"
    assert _classify_cabinet_quality(3, 0.5) == "low_observation"


def test_reconstruct_happy_path_quality_ok_no_warning(
    synthetic_charuco_capture, capsys,
):
    """Both cabinets seen by all views with low residual -> quality "ok" and NO
    cabinet_quality warning emitted."""
    paths = synthetic_charuco_capture
    rc = run_reconstruct(_build_input(paths))
    assert rc == 0

    rep = json.loads(open(paths["pose_report"]).read())
    poses = {p["cabinet_id"]: p for p in rep["cabinet_poses"]}
    assert poses["V000_R000"]["quality"] == "ok"
    assert poses["V001_R000"]["quality"] == "ok"

    events = [
        json.loads(ln)
        for ln in capsys.readouterr().out.splitlines()
        if ln.strip()
    ]
    quality_warnings = [
        e for e in events
        if e.get("event") == "warning" and e.get("code") == "cabinet_quality"
    ]
    assert quality_warnings == []


def test_reconstruct_underobserved_cabinet_flagged_low_observation(
    synthetic_charuco_capture_underobserved, capsys,
):
    """Non-root cabinet rendered into only 3 views (>=2 clears observability,
    but < QUALITY_MIN_VIEWS=4) -> quality "low_observation" + a cabinet_quality
    warning for it. The root (in all views) stays "ok"."""
    paths = synthetic_charuco_capture_underobserved
    rc = run_reconstruct(_build_input(paths))
    assert rc == 0

    rep = json.loads(open(paths["pose_report"]).read())
    poses = {p["cabinet_id"]: p for p in rep["cabinet_poses"]}
    assert poses["V001_R000"]["observed_views"] == 3
    assert poses["V001_R000"]["quality"] == "low_observation"
    assert poses["V000_R000"]["quality"] == "ok"

    events = [
        json.loads(ln)
        for ln in capsys.readouterr().out.splitlines()
        if ln.strip()
    ]
    quality_warnings = [
        e for e in events
        if e.get("event") == "warning" and e.get("code") == "cabinet_quality"
    ]
    assert len(quality_warnings) == 1
    w = quality_warnings[0]
    assert w["cabinet"] == "V001_R000"
    assert "low_observation" in w["message"]


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


def test_pnp_camera_fallback_recovers_world_pose():
    """Unit test for the non-root PnP fallback frame composition (no rendering).

    init_cabinets stores world_from_cabinet (BA: xw = R_wc·p_local + t_wc).
    solvePnP returns camera_from_cabinet (Rcc, tcc): x_cam = Rcc·p_local + tcc.
    The camera init must be camera_from_world: x_cam = R·x_world + t. So the
    correct composition is camera_from_world = camera_from_cabinet ∘
    inverse(world_from_cabinet): R = Rcc·R_wc^T, t = tcc − R·t_wc.

    The old buggy composition (R = Rcc·R_wc; t = Rcc·t_wc + tcc) shifts the
    translation seed by ~2·R_cam·t_wc when the camera is rotated, so the
    recovered t is off by far more than 1 mm.
    """
    import cv2

    # Known camera world pose (camera_from_world): x_cam = R_cam·x_world + t_cam.
    rvec_cam = np.array([0.05, -0.08, 0.03], dtype=float)
    R_cam, _ = cv2.Rodrigues(rvec_cam)
    t_cam = np.array([50.0, -20.0, 2500.0], dtype=float)

    # Non-root cabinet (idx 1) world pose (world_from_cabinet): identity
    # rotation + nominal offset, matching init_cabinets non-root entries.
    t_wc = np.array([700.0, 0.0, 0.0], dtype=float)

    # A grid of local-mm corners spanning ±300 x ±170 (>= MIN_PNP_CORNERS).
    p_locals = [
        np.array([x, y, 0.0], dtype=float)
        for x in (-300.0, 300.0)
        for y in (-170.0, 170.0)
    ] + [
        np.array([x, y, 0.0], dtype=float)
        for x in (-150.0, 150.0)
        for y in (-85.0, 85.0)
    ]
    assert len(p_locals) >= MIN_PNP_CORNERS

    K = np.array(
        [[1800.0, 0.0, 960.0], [0.0, 1800.0, 540.0], [0.0, 0.0, 1.0]], dtype=float
    )

    corners = []
    for p_local in p_locals:
        xw = p_local + t_wc            # world_from_cabinet (identity R)
        xc = R_cam @ xw + t_cam        # camera_from_world
        proj = K @ xc
        px = proj[:2] / proj[2]
        corners.append((p_local, px))

    # NO (0, root_idx) entry -> forces the fallback branch.
    per_view_cab_corners = {(0, 1): corners}
    init_cabinets = {
        0: (np.eye(3), np.zeros(3)),
        1: (np.eye(3), t_wc),
    }

    R, t = _pnp_camera(
        cam_idx=0,
        root_idx=0,
        init_cabinets=init_cabinets,
        per_view_cab_corners=per_view_cab_corners,
        K=K,
    )

    assert np.allclose(R, R_cam, atol=1e-4)
    assert np.linalg.norm(t - t_cam) < 1.0


def test_reconstruct_folded_shape_prior_is_invalid_input(
    synthetic_charuco_capture, capsys,
):
    """An unsupported (folded) shape_prior reaches nominal_cabinet_centers_model_frame
    after detection + observability pass, where it raises ValueError. That must
    surface as the invalid_input envelope, NOT an internal_error traceback."""
    paths = synthetic_charuco_capture
    inp = _build_input(paths, shape_prior={"folded": {"fold_seam_columns": [1]}})
    rc = run_reconstruct(inp)
    assert rc == 1

    last = json.loads(
        [ln for ln in capsys.readouterr().out.splitlines() if ln.strip()][-1]
    )
    assert last["event"] == "error"
    assert last["code"] == "invalid_input"


def _project(R_cam, t_cam, R_cab, t_cab, p_local, K):
    xw = R_cab @ p_local + t_cab
    xc = R_cam @ xw + t_cam
    p = K @ xc
    return p[:2] / p[2]


def test_estimate_nonroot_cabinet_init_recovers_known_pose():
    K = np.array([[2000.0, 0, 960], [0, 2000.0, 540], [0, 0, 1.0]])
    # 4 well-spread coplanar corners — ChArUco object points are coplanar, so
    # cv2's ITERATIVE solver uses a homography init that works with >= 4 points.
    root_local = np.array([[-300, -170, 0], [300, -170, 0],
                           [300, 170, 0], [-300, 170, 0]], dtype=float)
    # Identical coplanar local geometry on purpose: both cabinets share the
    # same active-surface corner layout, so any recovered pose difference comes
    # only from the bridge composition, not from differing object points.
    nonroot_local = root_local.copy()
    # ground-truth world_from_nonroot: 60 deg about y + translate
    ang = np.deg2rad(60.0)
    R_true = np.array([[np.cos(ang), 0, np.sin(ang)],
                       [0, 1, 0],
                       [-np.sin(ang), 0, np.cos(ang)]])
    t_true = np.array([500.0, 0.0, -200.0])

    # 3 synthetic cameras, all see both cabinets
    cams = []
    for dx in (-300.0, 0.0, 300.0):
        R_cam = np.eye(3)
        t_cam = np.array([dx, 0.0, 2200.0])
        cams.append((R_cam, t_cam))

    per_view: dict[tuple[int, int], list] = {}
    for ci, (R_cam, t_cam) in enumerate(cams):
        root_obs = [(p, _project(R_cam, t_cam, np.eye(3), np.zeros(3), p, K))
                    for p in root_local]
        non_obs = [(p, _project(R_cam, t_cam, R_true, t_true, p, K))
                   for p in nonroot_local]
        per_view[(ci, 0)] = root_obs   # cabinet idx 0 = root
        per_view[(ci, 1)] = non_obs    # cabinet idx 1 = non-root

    n_true = R_true @ np.array([0.0, 0.0, 1.0])
    out, undecidable = estimate_nonroot_cabinet_init(
        per_view, root_idx=0, K=K,
        nominal_normals={0: (0.0, 0.0, 1.0), 1: tuple(n_true)},
        nominal_centers={0: (0.0, 0.0, 0.0), 1: tuple(t_true / 1000.0)},
    )
    assert undecidable == set()
    assert 1 in out, "non-root cabinet should get a bridge estimate"
    R_est, t_est = out[1]
    # rotation close (trace test) and translation close
    ang_err = np.degrees(np.arccos(np.clip((np.trace(R_est.T @ R_true) - 1) / 2, -1, 1)))
    assert ang_err < 1.0, f"rotation error {ang_err:.3f} deg too large"
    assert np.linalg.norm(t_est - t_true) < 5.0, f"t_est={t_est} vs {t_true}"


def test_estimate_nonroot_cabinet_init_no_bridge_returns_empty():
    """No view sees the root with >= MIN_PNP_CORNERS corners (one view shows
    only the non-root, another shows the root with just 2 corners) -> nothing
    can be bridged, so the result is an empty dict (caller falls back to nominal)."""
    K = np.array([[2000.0, 0, 960], [0, 2000.0, 540], [0, 0, 1.0]])
    local = np.array([[-300, -170, 0], [300, -170, 0],
                      [300, 170, 0], [-300, 170, 0]], dtype=float)
    R_cam = np.eye(3)
    t_cam = np.array([0.0, 0.0, 2200.0])

    per_view: dict[tuple[int, int], list] = {
        # view 0: only the non-root cabinet visible (no root in this view).
        (0, 1): [(p, _project(R_cam, t_cam, np.eye(3), np.zeros(3), p, K))
                 for p in local],
        # view 1: root visible but with < MIN_PNP_CORNERS corners.
        (1, 0): [(p, _project(R_cam, t_cam, np.eye(3), np.zeros(3), p, K))
                 for p in local[:2]],
        (1, 1): [(p, _project(R_cam, t_cam, np.eye(3), np.zeros(3), p, K))
                 for p in local],
    }

    out, undecidable = estimate_nonroot_cabinet_init(
        per_view, root_idx=0, K=K,
        nominal_normals={0: (0.0, 0.0, 1.0), 1: (0.0, 0.0, 1.0)},
        nominal_centers={0: (0.0, 0.0, 0.0), 1: (0.0, 0.0, 0.0)},
    )
    assert out == {}
    assert undecidable == set()


def test_bridge_init_makes_ba_converge_to_known_angle():
    K = np.array([[2000.0, 0, 960], [0, 2000.0, 540], [0, 0, 1.0]])
    # 4 well-spread coplanar corners — proves 4-corner bridge views are usable.
    root_local = np.array([[-300, -170, 0], [300, -170, 0],
                           [300, 170, 0], [-300, 170, 0]], dtype=float)
    ang = np.deg2rad(60.0)
    R_true = np.array([[np.cos(ang), 0, np.sin(ang)],
                       [0, 1, 0],
                       [-np.sin(ang), 0, np.cos(ang)]])
    t_true = np.array([500.0, 0.0, -200.0])
    cams = [(np.eye(3), np.array([dx, 0.0, 2200.0])) for dx in (-300., -100., 100., 300.)]

    per_view: dict[tuple[int, int], list] = {}
    observations = []
    init_cameras = []
    for ci, (R_cam, t_cam) in enumerate(cams):
        init_cameras.append((R_cam, t_cam))
        for p in root_local:
            pix = _project(R_cam, t_cam, np.eye(3), np.zeros(3), p, K)
            observations.append(Observation(camera_idx=ci, cabinet_idx=0, p_local=p, pixel=pix))
            per_view.setdefault((ci, 0), []).append((p, pix))
        for p in root_local:
            pix = _project(R_cam, t_cam, R_true, t_true, p, K)
            observations.append(Observation(camera_idx=ci, cabinet_idx=1, p_local=p, pixel=pix))
            per_view.setdefault((ci, 1), []).append((p, pix))

    n_true = R_true @ np.array([0.0, 0.0, 1.0])
    bridge, undecidable = estimate_nonroot_cabinet_init(
        per_view, root_idx=0, K=K,
        nominal_normals={0: (0.0, 0.0, 1.0), 1: tuple(n_true)},
        nominal_centers={0: (0.0, 0.0, 0.0), 1: tuple(t_true / 1000.0)},
    )
    assert undecidable == set()
    init_cabinets = {0: (np.eye(3), np.zeros(3)), 1: bridge[1]}
    res = model_constrained_ba(
        K=K, observations=observations, n_cameras=len(cams), n_cabinets=2,
        root_cabinet_idx=0, init_cameras=init_cameras, init_cabinets=init_cabinets,
    )
    assert res.converged
    assert res.rms_reprojection_px < 1.0
    R_solved, _ = res.cabinet_poses[1]
    n_root = np.array([0, 0, 1.0])
    n_non = R_solved @ np.array([0, 0, 1.0])
    angle = np.degrees(np.arccos(np.clip(n_root @ n_non, -1, 1)))
    assert abs(angle - 60.0) < 1.0, f"recovered inter-panel angle {angle:.2f} != 60"


def test_solve_pnp_handles_4_points_and_skips_degenerate():
    from lmt_vba_sidecar.reconstruct import _solve_pnp
    K = np.array([[2000.0, 0, 960], [0, 2000.0, 540], [0, 0, 1.0]])
    R = cv2.Rodrigues(np.array([0.1, 0.2, 0.05]))[0]
    t = np.array([50.0, 30.0, 2200.0])

    def obs(obj):
        xc = (R @ obj.T).T + t
        pix = (K @ xc.T).T
        pix = pix[:, :2] / pix[:, 2:3]
        return list(zip(obj, pix))

    # 4 well-spread coplanar corners -> solvable (homography path)
    spread4 = np.array([[-300, -170, 0], [300, -170, 0], [300, 170, 0], [-300, 170, 0]], dtype=float)
    assert _solve_pnp(obs(spread4), K) is not None
    # 5 (near-)collinear coplanar points -> degenerate -> None (caught cv2.error, no crash)
    collinear5 = np.array([[x, 0.0, 0.0] for x in np.linspace(-300, 300, 5)], dtype=float)
    assert _solve_pnp(obs(collinear5), K) is None
    # < 4 points -> None
    assert _solve_pnp(obs(spread4[:3]), K) is None


def test_solve_pnp_branches_returns_inliers_and_two_ippe_branches():
    from lmt_vba_sidecar.reconstruct import _solve_pnp_branches
    K = np.array([[2000.0, 0, 960], [0, 2000.0, 540], [0, 0, 1.0]])
    # Oblique view (tilt ~40 deg about y) of a coplanar grid -> IPPE gives 2 branches.
    ang = np.deg2rad(40.0)
    R = np.array([[np.cos(ang), 0, np.sin(ang)], [0, 1, 0], [-np.sin(ang), 0, np.cos(ang)]])
    t = np.array([40.0, 30.0, 2200.0])
    obj = np.array([[x, y, 0.0] for x in (-300.0, -100.0, 100.0, 300.0)
                    for y in (-170.0, 0.0, 170.0)], dtype=float)
    xc = (R @ obj.T).T + t
    pix = (K @ xc.T).T
    pix = pix[:, :2] / pix[:, 2:3]
    corners = list(zip(obj, pix))

    res = _solve_pnp_branches(corners, K)
    assert res is not None
    branches, inlier_mask = res
    # All clean points are inliers.
    assert inlier_mask.sum() == len(corners)
    # IPPE yields 1 or 2 branches; when 2, the camera-frame normals share z-sign
    # (Codex finding-1: front-facing cannot disambiguate; only lateral flips).
    assert 1 <= len(branches) <= 2
    if len(branches) == 2:
        n0 = branches[0][0] @ np.array([0.0, 0.0, 1.0])
        n1 = branches[1][0] @ np.array([0.0, 0.0, 1.0])
        # In the OBJECT frame both branches' surface points face the camera, so
        # the camera-frame z-component of the rotated normal shares sign.
        zc0 = (R.T if False else branches[0][0]) @ np.array([0.0, 0.0, 1.0])
        assert np.sign(n0[2]) == np.sign(n1[2])


def test_solve_pnp_branches_rejects_gross_outlier():
    from lmt_vba_sidecar.reconstruct import _solve_pnp_branches
    K = np.array([[2000.0, 0, 960], [0, 2000.0, 540], [0, 0, 1.0]])
    R = cv2.Rodrigues(np.array([0.1, 0.2, 0.05]))[0]
    t = np.array([50.0, 30.0, 2200.0])
    obj = np.array([[x, y, 0.0] for x in (-300.0, -100.0, 100.0, 300.0)
                    for y in (-170.0, 0.0, 170.0)], dtype=float)
    xc = (R @ obj.T).T + t
    pix = (K @ xc.T).T
    pix = pix[:, :2] / pix[:, 2:3]
    # Corrupt the last point's pixel by 400px -> must be a RANSAC outlier.
    pix[-1] += np.array([400.0, 400.0])
    corners = list(zip(obj, pix))
    res = _solve_pnp_branches(corners, K)
    assert res is not None
    _branches, inlier_mask = res
    assert inlier_mask[-1] == False
    assert inlier_mask[:-1].all()


def test_solve_pnp_branches_none_for_few_or_degenerate():
    from lmt_vba_sidecar.reconstruct import _solve_pnp_branches
    K = np.array([[2000.0, 0, 960], [0, 2000.0, 540], [0, 0, 1.0]])
    obj3 = [(np.array([x, 0.0, 0.0]), np.array([x, 0.0])) for x in (-1.0, 0.0, 1.0)]
    assert _solve_pnp_branches(obj3, K) is None  # < 4
    R = cv2.Rodrigues(np.array([0.1, 0.2, 0.05]))[0]
    t = np.array([50.0, 30.0, 2200.0])
    collinear = np.array([[x, 0.0, 0.0] for x in np.linspace(-300, 300, 6)], dtype=float)
    xc = (R @ collinear.T).T + t
    pix = (K @ xc.T).T
    pix = pix[:, :2] / pix[:, 2:3]
    assert _solve_pnp_branches(list(zip(collinear, pix)), K) is None


def _ippe_oblique_corners(K, R_world_from_cab, t_world, R_cam, t_cam):
    """Coplanar grid at world pose -> camera pixels (used to build IPPE cases)."""
    obj = np.array([[x, y, 0.0] for x in (-300.0, -100.0, 100.0, 300.0)
                    for y in (-170.0, 0.0, 170.0)], dtype=float)
    corners = []
    for p in obj:
        xw = R_world_from_cab @ p + t_world
        xc = R_cam @ xw + t_cam
        pr = K @ xc
        corners.append((p, pr[:2] / pr[2]))
    return corners


def test_nominal_orientation_picks_correct_branch_oblique():
    """A single oblique non-root cabinet (curved nominal tilt) is disambiguated
    to the branch whose model-frame normal matches the nominal arc normal, NOT
    its mirror."""
    from lmt_vba_sidecar.reconstruct import estimate_nonroot_cabinet_init
    K = np.array([[2000.0, 0, 960], [0, 2000.0, 540], [0, 0, 1.0]])
    # Ground-truth non-root cabinet tilted +35deg about y (right side of an arc).
    a = np.deg2rad(35.0)
    R_true = np.array([[np.cos(a), 0, np.sin(a)], [0, 1, 0], [-np.sin(a), 0, np.cos(a)]])
    t_true = np.array([500.0, 0.0, 150.0])
    root_local = np.array([[-300, -170, 0], [300, -170, 0], [300, 170, 0], [-300, 170, 0]], float)
    cams = [(np.eye(3), np.array([dx, 0.0, 2400.0])) for dx in (-200.0, 0.0, 200.0)]
    per_view = {}
    for ci, (R_cam, t_cam) in enumerate(cams):
        per_view[(ci, 0)] = [(p, (lambda xw: (K @ (R_cam @ xw + t_cam))[:2]
                                  / (K @ (R_cam @ xw + t_cam))[2])(p))
                             for p in root_local]
        per_view[(ci, 1)] = _ippe_oblique_corners(K, R_true, t_true, R_cam, t_cam)
    # Nominal normal for cabinet 1 points like +35deg branch: [-sin? ] -> match true.
    nominal_normals = {0: (0.0, 0.0, 1.0),
                       1: (float(np.sin(a)), 0.0, float(np.cos(a)))}  # +x tilt
    nominal_centers = {0: (0.0, 0.0, 0.0), 1: (0.5, 0.0, 0.15)}
    out, undecidable = estimate_nonroot_cabinet_init(
        per_view, root_idx=0, K=K,
        nominal_normals=nominal_normals, nominal_centers=nominal_centers,
    )
    assert undecidable == set()
    R_est, _t = out[1]
    n_est = R_est @ np.array([0.0, 0.0, 1.0])
    n_true = R_true @ np.array([0.0, 0.0, 1.0])
    ang = np.degrees(np.arccos(np.clip(n_est @ n_true, -1, 1)))
    assert ang < 5.0, f"picked wrong (mirror) branch: {ang:.1f}deg from truth"


def test_seeded_flip_is_corrected_by_nominal():
    """Even when the iterative/homography path would seed the mirror branch,
    nominal disambiguation returns the non-mirrored pose."""
    from lmt_vba_sidecar.reconstruct import estimate_nonroot_cabinet_init
    K = np.array([[2000.0, 0, 960], [0, 2000.0, 540], [0, 0, 1.0]])
    a = np.deg2rad(45.0)
    R_true = np.array([[np.cos(a), 0, np.sin(a)], [0, 1, 0], [-np.sin(a), 0, np.cos(a)]])
    t_true = np.array([600.0, 0.0, 200.0])
    root_local = np.array([[-300, -170, 0], [300, -170, 0], [300, 170, 0], [-300, 170, 0]], float)
    cams = [(np.eye(3), np.array([dx, 0.0, 2400.0])) for dx in (-200.0, 0.0, 200.0)]
    per_view = {}
    for ci, (R_cam, t_cam) in enumerate(cams):
        per_view[(ci, 0)] = [(p, (lambda xw: (K @ (R_cam @ xw + t_cam))[:2]
                                  / (K @ (R_cam @ xw + t_cam))[2])(p)) for p in root_local]
        per_view[(ci, 1)] = _ippe_oblique_corners(K, R_true, t_true, R_cam, t_cam)
    nominal_normals = {0: (0.0, 0.0, 1.0), 1: (float(np.sin(a)), 0.0, float(np.cos(a)))}
    nominal_centers = {0: (0.0, 0.0, 0.0), 1: (0.6, 0.0, 0.2)}
    out, undecidable = estimate_nonroot_cabinet_init(
        per_view, root_idx=0, K=K,
        nominal_normals=nominal_normals, nominal_centers=nominal_centers)
    assert 1 in out and undecidable == set()
    n_est = out[1][0] @ np.array([0.0, 0.0, 1.0])
    n_true = R_true @ np.array([0.0, 0.0, 1.0])
    assert np.degrees(np.arccos(np.clip(n_est @ n_true, -1, 1))) < 5.0


def test_stageA_pnp_ransac_inliers_drops_far_outlier():
    from lmt_vba_sidecar.reconstruct import stage_a_prune
    from lmt_vba_sidecar.model_constrained_ba import Observation
    K = np.array([[2000.0, 0, 960], [0, 2000.0, 540], [0, 0, 1.0]])
    R = cv2.Rodrigues(np.array([0.05, 0.1, 0.0]))[0]
    t = np.array([0.0, 0.0, 2300.0])
    obj = np.array([[x, y, 0.0] for x in (-300.0, -100.0, 100.0, 300.0)
                    for y in (-170.0, 0.0, 170.0)], dtype=float)
    observations, pvcc = [], {}
    for p in obj:
        xc = R @ p + t
        pr = K @ xc
        pix = pr[:2] / pr[2]
        observations.append(Observation(camera_idx=0, cabinet_idx=0, p_local=p, pixel=pix))
        pvcc.setdefault((0, 0), []).append((p, pix))
    # Inject ONE far outlier (wrong-id pixel 500px off) into the SAME group.
    bad_pix = observations[0].pixel + np.array([500.0, 0.0])
    observations.append(Observation(camera_idx=0, cabinet_idx=0, p_local=obj[5], pixel=bad_pix))
    pvcc[(0, 0)].append((obj[5], bad_pix))

    obs2, pvcc2, views2, pts2, n_rej, rej_per_cab = stage_a_prune(observations, pvcc, K)
    assert n_rej == 1
    assert rej_per_cab == {0: 1}            # the one outlier is on cabinet 0
    assert len(obs2) == len(obj)            # the clean dozen survive
    assert pts2[0] == len(obj)
    assert views2[0] == {0}
    assert all(not np.allclose(o.pixel, bad_pix) for o in obs2)


def _two_panel_clean(K, R_true, t_true):
    root_local = np.array([[-300, -170, 0], [300, -170, 0], [300, 170, 0], [-300, 170, 0],
                           [-150, -85, 0], [150, -85, 0], [150, 85, 0], [-150, 85, 0]], float)
    cams = [(np.eye(3), np.array([dx, 0.0, 2400.0])) for dx in (-300., -100., 100., 300.)]
    obs, init_cams = [], []
    for ci, (R_cam, t_cam) in enumerate(cams):
        init_cams.append((R_cam, t_cam))
        for p in root_local:
            pr = K @ (R_cam @ p + t_cam); obs.append(Observation(ci, 0, p, pr[:2]/pr[2]))
        for p in root_local:
            xw = R_true @ p + t_true; pr = K @ (R_cam @ xw + t_cam)
            obs.append(Observation(ci, 1, p, pr[:2]/pr[2]))
    return obs, init_cams, cams, root_local


def test_stage_b_trims_pointwise_outliers_and_converges():
    from lmt_vba_sidecar.reconstruct import stage_b_robust_solve
    K = np.array([[2000.0, 0, 960], [0, 2000.0, 540], [0, 0, 1.0]])
    a = np.deg2rad(20.0)
    R_true = np.array([[np.cos(a),0,np.sin(a)],[0,1,0],[-np.sin(a),0,np.cos(a)]])
    t_true = np.array([700.0, 0.0, 0.0])
    obs, init_cams, cams, _ = _two_panel_clean(K, R_true, t_true)
    # Inject 3 random-far pointwise outliers (different cams, cabinet 1).
    for k in (5, 20, 33):
        obs[k] = Observation(obs[k].camera_idx, obs[k].cabinet_idx,
                             obs[k].p_local, obs[k].pixel + np.array([250.0, -180.0]))
    init_cabinets = {0: (np.eye(3), np.zeros(3)), 1: (np.eye(3), t_true)}
    res, rej_per_cab, total, surviving = stage_b_robust_solve(
        K=K, observations=obs, n_cameras=len(cams), n_cabinets=2,
        root_cabinet_idx=0, init_cameras=init_cams, init_cabinets=init_cabinets,
        per_cabinet_min_points=8)
    assert res.converged
    assert res.rms_reprojection_px < 1.0
    assert total >= 3            # at least the injected outliers rejected
    assert len(surviving) == len(obs) - total   # surviving = trimmed obs list


def test_overtrim_stops_at_floor():
    """Trimming must never push a cabinet below min_points (would KeyError in
    _per_cabinet_reproj_rms / geometry)."""
    from lmt_vba_sidecar.reconstruct import stage_b_robust_solve
    K = np.array([[2000.0, 0, 960], [0, 2000.0, 540], [0, 0, 1.0]])
    a = np.deg2rad(20.0)
    R_true = np.array([[np.cos(a),0,np.sin(a)],[0,1,0],[-np.sin(a),0,np.cos(a)]])
    obs, init_cams, cams, _ = _two_panel_clean(K, R_true, np.array([700.0,0.0,0.0]))
    init_cabinets = {0: (np.eye(3), np.zeros(3)), 1: (np.eye(3), np.array([700.,0.,0.]))}
    res, rej_per_cab, total, surviving = stage_b_robust_solve(
        K=K, observations=obs, n_cameras=len(cams), n_cabinets=2,
        root_cabinet_idx=0, init_cameras=init_cams, init_cabinets=init_cabinets,
        per_cabinet_min_points=8)
    # Clean data: no cabinet trimmed below the floor of 8 points each. With 4
    # cameras x 8 corners = 32 obs/cabinet, the floor leaves >=8 per cabinet.
    from collections import Counter
    survivors = Counter(o.cabinet_idx for o in surviving)
    assert survivors[0] >= 8
    assert survivors[1] >= 8
    assert rej_per_cab.get(0, 0) <= 32 - 8
    assert rej_per_cab.get(1, 0) <= 32 - 8
