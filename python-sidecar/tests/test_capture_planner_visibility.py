import numpy as np

from lmt_vba_sidecar.capture_planner.visibility import (
    Camera,
    intrinsics_from_fov,
    look_at_camera,
    point_visible,
)

FLAT_NORMAL = np.array([0.0, 0.0, 1.0])
WALL_PT = np.array([250.0, 250.0, 0.0])   # a point on a flat wall facing +z


def test_intrinsics_from_fov_horizontal():
    K = intrinsics_from_fov((1920, 1080), hfov_deg=90.0)
    # f = (W/2)/tan(45deg) = 960
    assert np.isclose(K[0, 0], 960.0)
    assert np.isclose(K[1, 1], 960.0)
    assert np.isclose(K[0, 2], 960.0)
    assert np.isclose(K[1, 2], 540.0)


def test_point_visible_frontal_true():
    K = intrinsics_from_fov((1920, 1080), hfov_deg=50.0)
    cam = look_at_camera(K, [250.0, 250.0, 3000.0], WALL_PT, (1920, 1080))
    assert point_visible(cam, WALL_PT, FLAT_NORMAL) is True


def test_point_visible_behind_camera_false_cheirality():
    K = intrinsics_from_fov((1920, 1080), hfov_deg=50.0)
    cam = look_at_camera(K, [250.0, 250.0, 3000.0], WALL_PT, (1920, 1080))
    behind = np.array([250.0, 250.0, 6000.0])  # past the camera, +z further out
    assert point_visible(cam, behind, FLAT_NORMAL) is False


def test_point_visible_out_of_frame_false():
    K = intrinsics_from_fov((1920, 1080), hfov_deg=50.0)
    cam = look_at_camera(K, [250.0, 250.0, 3000.0], WALL_PT, (1920, 1080))
    far_side = np.array([9000.0, 250.0, 0.0])   # way off to the right, off-sensor
    assert point_visible(cam, far_side, FLAT_NORMAL) is False


def test_point_visible_grazing_incidence_false():
    K = intrinsics_from_fov((1920, 1080), hfov_deg=50.0)
    # camera almost in the wall plane -> ~88deg incidence on a +z normal
    cam = look_at_camera(K, [5250.0, 250.0, 100.0], WALL_PT, (1920, 1080))
    assert point_visible(cam, WALL_PT, FLAT_NORMAL, incidence_max_deg=60.0) is False
