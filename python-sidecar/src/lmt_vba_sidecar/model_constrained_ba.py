"""Model-constrained bundle adjustment.

State = per-camera SE3 (rvec,t) + per-NON-root cabinet SE3 (rvec,t).
Root cabinet (gauge) is fixed at R=I,t=0 so the world frame equals the
root cabinet's active-surface frame. Observations carry the known local
mm coordinate of each detected corner. Scale is fixed by these metric
local coords — no anchors, no total station.
"""
from __future__ import annotations
from dataclasses import dataclass
import cv2
import numpy as np
from scipy.optimize import least_squares
from scipy.sparse import lil_matrix


MAX_COVARIANCE_PARAMS = 2400  # cap dense pinv at ~2400 params (~46MB matrix)


@dataclass
class Observation:
    camera_idx: int
    cabinet_idx: int
    p_local: np.ndarray  # (3,) mm
    pixel: np.ndarray     # (2,)


@dataclass
class BAResult:
    camera_poses: list[tuple[np.ndarray, np.ndarray]]
    cabinet_poses: dict[int, tuple[np.ndarray, np.ndarray]]  # idx -> (R,t); 含 root=I,0
    rms_reprojection_px: float
    iterations: int
    converged: bool
    cabinet_covariances: dict[int, np.ndarray]


def _nonroot_cabinets(n_cabinets: int, root: int) -> list[int]:
    return [j for j in range(n_cabinets) if j != root]


def _pack(cams, cabs, nonroot):
    parts = []
    for R, t in cams:
        rvec, _ = cv2.Rodrigues(R)
        parts.append(np.concatenate([rvec.ravel(), t]))
    for j in nonroot:
        R, t = cabs[j]
        rvec, _ = cv2.Rodrigues(R)
        parts.append(np.concatenate([rvec.ravel(), t]))
    return np.concatenate(parts)


def _unpack(x, n_cams, nonroot):
    cams = []
    for i in range(n_cams):
        seg = x[i*6:i*6+6]
        R, _ = cv2.Rodrigues(seg[:3])
        cams.append((R, seg[3:6].copy()))
    cabs = {}
    base = n_cams*6
    for k, j in enumerate(nonroot):
        seg = x[base+k*6: base+k*6+6]
        R, _ = cv2.Rodrigues(seg[:3])
        cabs[j] = (R, seg[3:6].copy())
    return cams, cabs


def _residuals(x, n_cams, nonroot, root, K, obs):
    cams, cabs = _unpack(x, n_cams, nonroot)
    res = np.zeros(len(obs)*2)
    for k, o in enumerate(obs):
        Rc, tc = cams[o.camera_idx]
        if o.cabinet_idx == root:
            Rb, tb = np.eye(3), np.zeros(3)
        else:
            Rb, tb = cabs[o.cabinet_idx]
        xw = Rb @ o.p_local + tb
        xc = Rc @ xw + tc
        p = K @ xc
        res[k*2:k*2+2] = p[:2]/p[2] - o.pixel
    return res


def _sparsity(n_cams, nonroot, root, obs):
    n = n_cams*6 + len(nonroot)*6
    A = lil_matrix((len(obs)*2, n), dtype=int)
    nonroot_pos = {j: k for k, j in enumerate(nonroot)}
    base = n_cams*6
    for k, o in enumerate(obs):
        A[k*2:k*2+2, o.camera_idx*6:o.camera_idx*6+6] = 1
        if o.cabinet_idx != root:
            c = base + nonroot_pos[o.cabinet_idx]*6
            A[k*2:k*2+2, c:c+6] = 1
    return A


def model_constrained_ba(*, K, observations, n_cameras, n_cabinets,
                         root_cabinet_idx, init_cameras, init_cabinets,
                         loss="huber", f_scale=2.0, max_nfev=200,
                         compute_covariance=True) -> BAResult:
    nonroot = _nonroot_cabinets(n_cabinets, root_cabinet_idx)
    cabs0 = dict(init_cabinets)
    for j in nonroot:
        cabs0.setdefault(j, (np.eye(3), np.zeros(3)))
    x0 = _pack(init_cameras, cabs0, nonroot)
    sp = _sparsity(n_cameras, nonroot, root_cabinet_idx, observations)
    sol = least_squares(
        _residuals, x0, jac_sparsity=sp, method="trf",
        loss=loss, f_scale=f_scale, max_nfev=max_nfev, verbose=0,
        args=(n_cameras, nonroot, root_cabinet_idx, K, observations),
    )
    cams, cabs = _unpack(sol.x, n_cameras, nonroot)
    cabs[root_cabinet_idx] = (np.eye(3), np.zeros(3))
    rms = float(np.sqrt((sol.fun**2).reshape(-1, 2).sum(axis=1).mean()))
    covs: dict[int, np.ndarray] = {}
    n_params = n_cameras*6 + len(nonroot)*6
    if compute_covariance and sol.jac is not None and n_params <= MAX_COVARIANCE_PARAMS:
        try:
            J = sol.jac.toarray() if hasattr(sol.jac, "toarray") else np.asarray(sol.jac)
            dof = max(1, J.shape[0]-J.shape[1])
            cov = np.linalg.pinv(J.T @ J) * float((sol.fun**2).sum()/dof)
            base = n_cameras*6
            for k, j in enumerate(nonroot):
                a = base + k*6 + 3  # translation block
                # translation 3×3 sub-block only (rotation covariance dropped)
                covs[j] = cov[a:a+3, a:a+3]
        except np.linalg.LinAlgError:
            pass
    return BAResult(camera_poses=cams, cabinet_poses=cabs,
                    rms_reprojection_px=rms, iterations=int(sol.nfev),
                    converged=bool(sol.success), cabinet_covariances=covs)
