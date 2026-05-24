import numpy as np
from lmt_vba_sidecar.ipc import SimulateInput
from lmt_vba_sidecar.simulate import build_scene


def _inp(seed=42, n=12, vis=1.0, pitch=0.0):
    return SimulateInput.model_validate({
        "command": "simulate", "version": 1,
        "scene": {"cabinet_array": {"cols": 2, "rows": 1, "cabinet_size_mm": [600, 340]},
                  "shape_prior": "flat", "inter_board_angle_deg": 10.0},
        "cameras": {"n_views": n, "distance_mm_range": [1500, 3000],
                    "yaw_deg_range": [-40, 40], "pitch_deg_range": [-20, 20]},
        "intrinsics": {"K": [[2000, 0, 960], [0, 2000, 540], [0, 0, 1]],
                       "dist_coeffs": [0, 0, 0, 0, 0], "image_size": [1920, 1080]},
        "noise": {"pixel_sigma": 0.0, "visibility_frac": vis, "pixel_pitch_error_frac": pitch},
        "seed": seed})


def test_scene_is_deterministic_per_seed():
    a = build_scene(_inp(seed=7, vis=0.8))
    b = build_scene(_inp(seed=7, vis=0.8))
    assert np.allclose(a.true_camera_poses[0][1], b.true_camera_poses[0][1])
    assert np.allclose(a.observations[0].pixel, b.observations[0].pixel)
    assert len(a.observations) == len(b.observations)
    c = build_scene(_inp(seed=99, vis=0.8))
    assert not np.allclose(a.true_camera_poses[0][1], c.true_camera_poses[0][1])


def test_zero_noise_observations_reproject_exactly():
    scene = build_scene(_inp(seed=1))
    K = scene.K
    for o in scene.observations[:50]:
        Rc, tc = scene.true_camera_poses[o.camera_idx]
        Rb, tb = scene.true_cabinet_poses[o.cabinet_idx]
        xw = Rb @ o.p_local + tb
        xc = Rc @ xw + tc
        p = K @ xc
        assert np.linalg.norm(p[:2] / p[2] - o.pixel) < 1e-6


def test_inter_board_angle_is_applied():
    scene = build_scene(_inp())
    n0 = scene.true_cabinet_poses[0][0] @ np.array([0, 0, 1.])
    n1 = scene.true_cabinet_poses[1][0] @ np.array([0, 0, 1.])
    ang = np.degrees(np.arccos(np.clip(n0 @ n1, -1, 1)))
    assert abs(ang - 10.0) < 1e-6


def test_two_by_two_grid_positions():
    inp = _inp()
    inp = inp.model_copy(update={"scene": inp.scene.model_copy(update={
        "cabinet_array": inp.scene.cabinet_array.model_copy(update={"rows": 2}),
        "inter_board_angle_deg": 0.0})})
    scene = build_scene(inp)
    # j=2 -> col=0,row=1 -> (0, ch, 0)
    assert np.allclose(scene.true_cabinet_poses[2][1], [0, 340, 0])
    assert np.allclose(scene.true_cabinet_poses[3][1], [600, 340, 0])
