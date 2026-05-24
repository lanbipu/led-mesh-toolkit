import numpy as np
from lmt_vba_sidecar.evaluate import (
    gauge_invariant_metrics, se3_aligned_holdout_rms, umeyama_no_scale,
)


def test_umeyama_recovers_known_rigid():
    rng = np.random.default_rng(0)
    src = rng.normal(size=(20, 3)) * 100
    R, _ = np.linalg.qr(rng.normal(size=(3, 3)))
    if np.linalg.det(R) < 0:
        R[:, 0] *= -1
    t = np.array([10., -5., 3.])
    dst = (src @ R.T) + t
    R_est, t_est = umeyama_no_scale(src, dst)
    assert np.allclose(R_est, R, atol=1e-8)
    assert np.allclose(t_est, t, atol=1e-8)


def test_gauge_invariant_metrics_zero_when_perfect():
    true_centers = {0: np.zeros(3), 1: np.array([700., 0, 0])}
    true_normals = {0: np.array([0, 0, 1.]), 1: np.array([0, 0, 1.])}
    true_sizes = {0: (600., 340.), 1: (600., 340.)}
    m = gauge_invariant_metrics(true_centers, true_normals, true_sizes,
                                true_centers, true_normals, true_sizes)
    assert m["max_distance_error_mm"] < 1e-9
    assert m["max_angle_error_deg"] < 1e-9
    assert m["max_size_error_mm"] < 1e-9
    assert m["rms_size_error_mm"] < 1e-9


def test_se3_holdout_rms_zero_when_perfect():
    rng = np.random.default_rng(3)
    true_pts = rng.normal(size=(20, 3)) * 100
    R, _ = np.linalg.qr(rng.normal(size=(3, 3)))
    if np.linalg.det(R) < 0:
        R[:, 0] *= -1
    t = np.array([4., -2., 9.])
    est_pts = (true_pts - t) @ R  # est mapped so umeyama(est->true) recovers exactly
    align_idx = np.arange(10)
    score_idx = np.arange(10, 20)
    out = se3_aligned_holdout_rms(true_pts, est_pts, align_idx, score_idx)
    assert out["rms_mm"] < 1e-9
    assert out["p95_mm"] < 1e-9
    assert out["max_mm"] < 1e-9
