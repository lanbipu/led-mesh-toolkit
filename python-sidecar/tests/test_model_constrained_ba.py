import numpy as np
import cv2
from lmt_vba_sidecar.model_constrained_ba import model_constrained_ba, Observation

def _project(K, R_cam, t_cam, R_cab, t_cab, p_local):
    xw = R_cab @ p_local + t_cab
    xc = R_cam @ xw + t_cam
    p = K @ xc
    return p[:2] / p[2]

def test_zero_noise_recovers_two_boards_exactly():
    K = np.array([[2000.,0,960],[0,2000,540],[0,0,1]])
    R0, t0 = np.eye(3), np.zeros(3)
    R1, _ = cv2.Rodrigues(np.array([0., np.deg2rad(15), 0.]))
    t1 = np.array([700., 0., 0.])
    corners = np.array([[-300,-170,0],[300,-170,0],[300,170,0],[-300,170,0]], float)
    boards = [(R0, t0), (R1, t1)]
    cams = []
    for i in range(5):
        rvec = np.array([0.05*i, 0.1*i, 0.0])
        Rc, _ = cv2.Rodrigues(rvec)
        tc = np.array([50.*i, -20.*i, 2500.])
        cams.append((Rc, tc))
    obs = []
    for ci,(Rc,tc) in enumerate(cams):
        for bj,(Rb,tb) in enumerate(boards):
            for p in corners:
                px = _project(K, Rc, tc, Rb, tb, p)
                obs.append(Observation(camera_idx=ci, cabinet_idx=bj,
                                       p_local=p.copy(), pixel=px.copy()))
    init_cams = [(Rc, tc) for Rc, tc in cams]
    init_boards = {1: (np.eye(3), np.array([700.,0,0]))}
    result = model_constrained_ba(
        K=K, observations=obs, n_cameras=5, n_cabinets=2,
        root_cabinet_idx=0, init_cameras=init_cams, init_cabinets=init_boards,
        loss="linear",
    )
    assert result.converged
    assert np.linalg.norm(result.cabinet_poses[1][1] - t1) < 0.05
    n_est = result.cabinet_poses[1][0] @ np.array([0,0,1.])
    n_true = R1 @ np.array([0,0,1.])
    ang = np.degrees(np.arccos(np.clip(n_est @ n_true, -1, 1)))
    assert ang < 0.05
    assert result.rms_reprojection_px < 1e-3
