"""Run a method end-to-end on a synthetic Scene and return gauge-invariant
metrics. 'charuco' = model-constrained BA; 'free_point' = legacy ba.py for
baseline comparison.

Phase 0 note: both methods receive near-truth initialisation (true camera
poses + true cabinet translations + identity cabinet rotations) so that the
comparison isolates the accuracy difference of the optimisation model itself,
not initialisation quality.
"""
from __future__ import annotations
import numpy as np
from lmt_vba_sidecar.model_constrained_ba import model_constrained_ba
from lmt_vba_sidecar.evaluate import gauge_invariant_metrics
from lmt_vba_sidecar.observability import check_observability
from lmt_vba_sidecar import ba as legacy_ba


def reconstruct_cabinet_geometry(
    R: np.ndarray,
    t: np.ndarray,
    corners_local: np.ndarray,
) -> tuple[np.ndarray, np.ndarray, tuple[float, float], np.ndarray]:
    """Derive cabinet center, normal, size, and world corners from pose + local corners.

    Args:
        R: (3,3) rotation matrix (board frame -> world).
        t: (3,) translation (world position of board origin).
        corners_local: (M,3) corner positions in board-local mm coords.

    Returns:
        center: (3,) world centroid of corners.
        normal: (3,) unit board normal (rotated local +Z).
        size: (width_mm, height_mm) as x-span and y-span of local corners.
        world_corners: (M,3) corners in world frame.

    Note on size: it is read directly from the known local corner model, NOT
    from the estimated (R, t). The model-constrained BA treats the emitter
    surface dimensions as a fixed (known) constraint, so size does not change
    with pose — for the 'charuco' method true/est size are identical and the
    size error is structurally 0. A non-zero size error only arises when the
    pixel-pitch / panel-size input itself is wrong (covered by Task 3.1).
    """
    world = (corners_local @ R.T) + t
    center = world.mean(0)
    normal = R @ np.array([0.0, 0.0, 1.0])
    w = float(corners_local[:, 0].ptp())
    h = float(corners_local[:, 1].ptp())
    return center, normal, (w, h), world


def run_method(scene, method: str) -> dict:
    """Run a reconstruction method on a Scene and return gauge-invariant metrics.

    Args:
        scene: Scene from simulate.build_scene.
        method: 'charuco' (model-constrained BA) or 'free_point' (legacy BA).

    Returns:
        dict with keys from gauge_invariant_metrics:
          max_size_error_mm, rms_size_error_mm,
          max_distance_error_mm, max_angle_error_deg
    """
    check_observability(scene.observations, scene.n_cabinets, min_views=2, min_points=8)

    if method == "charuco":
        est_c, est_n, est_s = _charuco_geometry(scene)
    elif method == "free_point":
        est_c, est_n, est_s = _free_point_geometry(scene)
    else:
        raise ValueError(f"unknown method {method!r}")

    # Build ground-truth geometry from true poses
    true_c, true_n, true_s = {}, {}, {}
    for j in range(scene.n_cabinets):
        R, t = scene.true_cabinet_poses[j]
        c, n, s, _ = reconstruct_cabinet_geometry(R, t, scene.cabinet_corners_local[j])
        true_c[j], true_n[j], true_s[j] = c, n, s

    return gauge_invariant_metrics(true_c, true_n, true_s, est_c, est_n, est_s)


def _charuco_geometry(scene):
    """Model-constrained BA: cabinet pose is parameterised as SE(3) over the
    known metric local corners. Root cabinet gauge is fixed at I,0."""
    # Phase 0 near-truth init: use true camera poses; reset cabinet rotations
    # to identity so the BA can correct any residual pose error freely.
    init_cams = scene.true_camera_poses
    init_cabs = {
        j: (np.eye(3), scene.true_cabinet_poses[j][1].copy())
        for j in range(scene.n_cabinets)
    }
    res = model_constrained_ba(
        K=scene.K,
        observations=scene.observations,
        n_cameras=scene.n_cameras,
        n_cabinets=scene.n_cabinets,
        root_cabinet_idx=0,
        init_cameras=init_cams,
        init_cabinets=init_cabs,
    )
    est_c, est_n, est_s = {}, {}, {}
    for j in range(scene.n_cabinets):
        R, t = res.cabinet_poses[j]
        c, n, s, _ = reconstruct_cabinet_geometry(R, t, scene.cabinet_corners_local[j])
        est_c[j], est_n[j], est_s[j] = c, n, s
    return est_c, est_n, est_s


def _free_point_geometry(scene):
    """Legacy free-point BA baseline: each (cabinet, corner) is an independent
    free 3D point. Cabinet center = centroid, normal = smallest PCA singular
    vector (plane normal), size = principal-axis span.

    This deliberately ignores the known metric board model — the resulting
    accuracy is structurally lower than the model-constrained method, which
    is the whole point of the comparison.
    """
    # Map (cabinet_idx, rounded_local_coord) -> sequential point index
    pt_index: dict = {}
    init_pts: list[np.ndarray] = []
    for j in range(scene.n_cabinets):
        Rb, tb = scene.true_cabinet_poses[j]   # near-truth init only
        for p in scene.cabinet_corners_local[j]:
            key = (j, tuple(np.round(p, 6).tolist()))
            if key not in pt_index:
                pt_index[key] = len(init_pts)
                init_pts.append(Rb @ p + tb)

    init_points = np.array(init_pts, float)

    # Remap Observation objects to (cam_i, pt_i, pixel) tuples for legacy API
    obs_legacy = [
        (
            o.camera_idx,
            pt_index[(o.cabinet_idx, tuple(np.round(o.p_local, 6).tolist()))],
            o.pixel,
        )
        for o in scene.observations
    ]

    res = legacy_ba.bundle_adjust(
        K=scene.K,
        dist_coeffs=np.zeros(5),
        initial_points=init_points,
        initial_cam_poses=list(scene.true_camera_poses),
        observations=obs_legacy,
        compute_covariance=False,
    )

    est_c, est_n, est_s = {}, {}, {}
    for j in range(scene.n_cabinets):
        idxs = [
            pt_index[(j, tuple(np.round(p, 6).tolist()))]
            for p in scene.cabinet_corners_local[j]
        ]
        pts = res.points[idxs]
        c = pts.mean(0)
        _, _, vt = np.linalg.svd(pts - c)
        # Smallest singular vector is the plane normal. Its sign is arbitrary
        # (SVD does not fix orientation), so disambiguate against a fixed
        # reference; otherwise two cabinets can pick opposite-sign normals and
        # the pairwise normal angle flips ~180 deg (garbage angle error).
        normal = vt[2]
        reference = np.array([0.0, 0.0, 1.0])
        if normal @ reference < 0:
            normal = -normal
        # Project onto first two principal axes to measure extent
        proj = (pts - c) @ vt[:2].T
        est_c[j] = c
        est_n[j] = normal
        est_s[j] = (float(proj[:, 0].ptp()), float(proj[:, 1].ptp()))

    return est_c, est_n, est_s
