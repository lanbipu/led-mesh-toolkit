import numpy as np
import pytest
from lmt_vba_sidecar.sl_feasibility import (
    project_point, triangulate_multiview, look_at_pose, solve_pnp_pose,
)


def _K(f=3000.0, cx=1920.0, cy=1080.0):
    return np.array([[f, 0, cx], [0, f, cy], [0, 0, 1]], float)


def test_project_then_triangulate_is_exact_without_noise():
    K = _K()
    X = np.array([100.0, -50.0, 0.0])
    poses = [look_at_pose(np.array([-1500.0, 0.0, -4000.0])),
             look_at_pose(np.array([1500.0, 0.0, -4000.0]))]
    pts = [project_point(K, R, t, X) for (R, t) in poses]
    Xhat = triangulate_multiview(K, poses, pts)
    assert np.linalg.norm(Xhat - X) < 1e-6


def test_triangulate_requires_two_views():
    K = _K()
    with pytest.raises(ValueError):
        triangulate_multiview(K, [look_at_pose(np.array([0.0, 0.0, -4000.0]))],
                              [np.array([1920.0, 1080.0])])


def test_solve_pnp_recovers_true_pose_without_noise():
    K = _K()
    R_true, t_true = look_at_pose(np.array([800.0, 0.0, -4000.0]))
    # a non-trivial (slightly curved) object so PnP is well-posed
    obj = np.array([[x, y, 5.0 * np.cos(x / 500.0)]
                    for y in (-600, 0, 600) for x in (-900, -300, 300, 900)], float)
    img = np.array([project_point(K, R_true, t_true, X) for X in obj])
    R_est, t_est = solve_pnp_pose(K, obj, img)
    assert np.linalg.norm(R_est - R_true) < 1e-3
    assert np.linalg.norm(t_est - t_true) < 1e-2
