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


def test_solver_falls_back_to_radial2_on_distortion_free_data():
    # Distortion-free truth: k3/tangential are unobservable (~0 < stddev) -> radial2,
    # even with allow_full_distortion=True. (This is the correct fallback, not a bug.)
    obj, img = _well_object_image_points(noise=0.0)
    res = solve_sl_intrinsics(obj, img, IMG, max_rms_px=1.5, allow_full_distortion=True)
    assert res.distortion_model == "radial2"
