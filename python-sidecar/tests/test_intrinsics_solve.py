import cv2
import numpy as np
from lmt_vba_sidecar.intrinsics_solve import solve_sl_intrinsics, IntrinsicsRefused
from lmt_vba_sidecar.nominal import nominal_dot_positions_world
from lmt_vba_sidecar.sl_feasibility import look_at_pose, project_point

K_TRUE = np.array([[3000.0, 0.0, 2000.0], [0.0, 3000.0, 1500.0], [0.0, 0.0, 1.0]])
IMG = (4000, 3000)


def _well_object_image_points(noise=0.0, seed=0):
    """6 oblique multi-distance poses of a 3x3 curved wall (the gate-passing envelope).
    Returns (object_points, image_points) lists of float32 arrays, one per pose."""
    from test_calibrate_sl import _well_meta, _wall_center, _well_poses  # reuse builders
    meta, proj, cab, shape = _well_meta()
    world = nominal_dot_positions_world(meta, cab, shape)
    poses = _well_poses(_wall_center(meta, cab, shape))
    rng = np.random.default_rng(seed)
    obj, img = [], []
    ids = sorted(world.keys())
    for (R, t) in poses:
        o = np.array([world[i] for i in ids], dtype=np.float32)
        p = np.array([project_point(K_TRUE, R, t, world[i]) + rng.normal(0, noise, 2)
                      for i in ids], dtype=np.float32)
        obj.append(o)
        img.append(p)
    return obj, img


def test_solver_recovers_focal_noise_free():
    obj, img = _well_object_image_points(noise=0.0)
    res = solve_sl_intrinsics(obj, img, IMG, max_rms_px=1.5)
    assert abs(res.K[0, 0] - 3000.0) / 3000.0 < 0.01
    assert abs(res.K[0, 2] - 2000.0) < 1.5
    assert res.distortion_model in ("radial2", "full")
    assert res.focal_stddev_px[0] >= 0.0 and res.pp_stddev_px[0] >= 0.0


# Codex #6 fix: project_point (sl_feasibility) is DISTORTION-FREE pinhole, so a "full"
# model can NEVER be accepted from _well_object_image_points (k3/p1/p2 ~ 0 < their stddev
# -> always falls back to radial2). The full-distortion POSITIVE case must synthesize
# image points THROUGH a known non-zero distortion with cv2.projectPoints.
DIST_TRUE = np.array([-0.12, 0.04, 0.0008, -0.0006, 0.02])   # [k1,k2,p1,p2,k3]


def _well_object_image_points_distorted(dist_true, seed=0):
    """Same 6-oblique-pose well-conditioned geometry as _well_object_image_points,
    but pixels are projected through dist_true so k3/tangential become observable."""
    from test_calibrate_sl import _well_meta, _wall_center, _well_poses
    meta, proj, cab, shape = _well_meta()
    world = nominal_dot_positions_world(meta, cab, shape)
    poses = _well_poses(_wall_center(meta, cab, shape))
    ids = sorted(world.keys())
    obj, img = [], []
    for (R, t) in poses:
        o = np.array([world[i] for i in ids], dtype=np.float32)
        rvec, _ = cv2.Rodrigues(R.astype(np.float64))
        proj_pts, _ = cv2.projectPoints(o.reshape(-1, 1, 3), rvec, t.reshape(3, 1).astype(np.float64),
                                        K_TRUE, dist_true)
        obj.append(o)
        img.append(proj_pts.reshape(-1, 2).astype(np.float32))
    return obj, img


def test_solver_solves_full_distortion_when_well_conditioned():
    # Distortion is REAL here, so k3 + tangential are observable -> "full".
    obj, img = _well_object_image_points_distorted(DIST_TRUE)
    res = solve_sl_intrinsics(obj, img, IMG, max_rms_px=1.5, allow_full_distortion=True)
    assert res.distortion_model == "full"
    assert len(res.dist.flatten()) >= 5  # [k1,k2,p1,p2,k3]
    assert abs(res.dist.flatten()[4] - DIST_TRUE[4]) < 0.01   # k3 recovered


def test_solver_keeps_full_for_k3_only_centered_sensor():
    # Review finding: a lens with observable k3 but a CENTERED sensor (p1=p2=0, the
    # common case) must NOT fall back to radial2 and drop k3. k3_ok is True, tan_ok is
    # False -> accept full via the OR gate, recovering k3.
    obj, img = _well_object_image_points_distorted(np.array([-0.12, 0.04, 0.0, 0.0, 0.02]))
    res = solve_sl_intrinsics(obj, img, IMG, max_rms_px=1.5, allow_full_distortion=True)
    assert res.distortion_model == "full"
    assert abs(res.dist.flatten()[4] - 0.02) < 0.01   # k3 recovered, not discarded


def test_solver_falls_back_to_radial2_on_distortion_free_data():
    # Distortion-free truth: k3/tangential are unobservable (~0 < stddev) -> radial2,
    # even with allow_full_distortion=True. (This is the correct fallback, not a bug.)
    obj, img = _well_object_image_points(noise=0.0)
    res = solve_sl_intrinsics(obj, img, IMG, max_rms_px=1.5, allow_full_distortion=True)
    assert res.distortion_model == "radial2"


def test_radial2_fallback_K_matches_pure_radial_solve():
    # Codex P1 regression: when allow_full_distortion=True falls back to radial2, the
    # returned K must be the RADIAL solve's K — not the full probe's (cv2 mutates the
    # guess in place). It must equal a pure radial solve byte-for-byte.
    obj, img = _well_object_image_points(noise=0.0)
    radial = solve_sl_intrinsics(obj, img, IMG, max_rms_px=1.5, allow_full_distortion=False)
    fallback = solve_sl_intrinsics(obj, img, IMG, max_rms_px=1.5, allow_full_distortion=True)
    assert fallback.distortion_model == "radial2"
    assert np.allclose(fallback.K, radial.K)
    assert np.allclose(fallback.dist, radial.dist)


# --- Task 3: anti-absorption cross-check ---
from lmt_vba_sidecar.intrinsics_solve import crosscheck_intrinsics, IntrinsicsResult


def _res(K, dist=None, coplanar=0.3):
    return IntrinsicsResult(K=np.asarray(K, float),
                            dist=np.zeros(5) if dist is None else np.asarray(dist, float),
                            rms=0.2, focal_stddev_px=(1.0, 1.0), pp_stddev_px=(0.5, 0.5),
                            distortion_model="radial2", coplanar_ratio=coplanar, rvecs=[])


ANCHOR_K = np.array([[3000.0, 0, 2000.0], [0, 3000.0, 1500.0], [0, 0, 1.0]])


def test_crosscheck_refuses_when_anchor_disagrees_on_aspect():
    # Anisotropic absorption (class b): fx/fy ratio drifted ~2% vs a square-pixel anchor.
    res = _res([[3060.0, 0, 2000.0], [0, 3000.0, 1500.0], [0, 0, 1]])
    refusal = crosscheck_intrinsics(res, anchor_K=ANCHOR_K, anchor_dist=np.zeros(5))
    assert refusal is not None and refusal.code == "observability_failed"
    assert "aspect" in refusal.message.lower() or "anchor" in refusal.message.lower()


def test_crosscheck_refuses_when_anchor_disagrees_on_distortion():
    # Smooth nonlinear remap (class c): focal & aspect UNCHANGED, but distortion drifted.
    # A focal+aspect-only check would MISS this; the distortion-magnitude term catches it.
    res = _res(ANCHOR_K, dist=[-0.15, 0.05, 0.0, 0.0, 0.03])   # nonzero k1,k2,k3 vs anchor 0
    refusal = crosscheck_intrinsics(res, anchor_K=ANCHOR_K, anchor_dist=np.zeros(5))
    assert refusal is not None and refusal.code == "observability_failed"
    assert "distortion" in refusal.message.lower()


def test_crosscheck_refuses_opposite_sign_distortion():
    # Review finding: barrel (k1<0) vs pincushion (k1>0) of EQUAL magnitude must not
    # difference to zero — the signed radial displacement catches the sign flip.
    res = _res(ANCHOR_K, dist=[-0.12, 0.0, 0.0, 0.0, 0.0])     # barrel
    refusal = crosscheck_intrinsics(res, anchor_K=ANCHOR_K,
                                    anchor_dist=[0.12, 0.0, 0.0, 0.0, 0.0])  # pincushion, |k1| equal
    assert refusal is not None and refusal.code == "observability_failed"
    assert "distortion" in refusal.message.lower()


def test_crosscheck_refuses_when_anchor_disagrees_on_tangential():
    # Codex P2: screen shear/decentering absorbed into p1/p2 while focal, aspect and
    # RADIAL distortion all match the anchor. A radial-only check would PASS this; the
    # tangential displacement term must catch it.
    res = _res(ANCHOR_K, dist=[-0.12, 0.04, 0.003, -0.002, 0.02])   # nonzero p1,p2
    refusal = crosscheck_intrinsics(res, anchor_K=ANCHOR_K,
                                    anchor_dist=[-0.12, 0.04, 0.0, 0.0, 0.02])  # same radial, p1=p2=0
    assert refusal is not None and refusal.code == "observability_failed"
    assert "tangential" in refusal.message.lower()


def test_crosscheck_refuses_malformed_anchor_shape():
    # Codex P2: a 1-D / non-3x3 anchor K passes np.array(...) but 2-D indexing would throw
    # IndexError (escaping as internal_error). It must be the advertised invalid_input.
    res = _res(ANCHOR_K, dist=[-0.12, 0.04, 0, 0, 0.02])
    refusal = crosscheck_intrinsics(res, anchor_K=[3000.0, 0.0, 2000.0], anchor_dist=np.zeros(5))
    assert refusal is not None and refusal.code == "invalid_input"


def test_crosscheck_refuses_nonfinite_anchor():
    # Codex P2: a NaN in the anchor K makes every `> threshold` comparison False and would
    # SILENTLY pass the guard (disabling anti-absorption). It must be rejected as invalid_input.
    bad_K = np.array([[np.nan, 0, 2000.0], [0, 3000.0, 1500.0], [0, 0, 1.0]])
    res = _res([[3300.0, 0, 2000.0], [0, 3000.0, 1500.0], [0, 0, 1]])   # 10% focal drift
    refusal = crosscheck_intrinsics(res, anchor_K=bad_K, anchor_dist=np.zeros(5))
    assert refusal is not None and refusal.code == "invalid_input"


def test_crosscheck_passes_when_anchor_agrees():
    res = _res([[3005.0, 0, 2000.0], [0, 3004.0, 1500.0], [0, 0, 1]], dist=[-0.12, 0.04, 0, 0, 0.02])
    assert crosscheck_intrinsics(res, anchor_K=ANCHOR_K, anchor_dist=[-0.12, 0.04, 0, 0, 0.02]) is None


def test_crosscheck_refuses_flat_wall_without_anchor():
    res = _res(np.eye(3), coplanar=1e-5)  # coplanar (flat wall), no anchor
    refusal = crosscheck_intrinsics(res, anchor_K=None, anchor_dist=None)
    assert refusal is not None and refusal.code == "observability_failed"
    assert "flat wall" in refusal.message.lower() or "anchor" in refusal.message.lower()


def test_crosscheck_warns_curved_wall_without_anchor():
    res = _res(np.eye(3), coplanar=0.3)   # non-coplanar (curved), no anchor
    assert crosscheck_intrinsics(res, anchor_K=None, anchor_dist=None) is None  # caller warns
