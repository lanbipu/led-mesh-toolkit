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
