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


from lmt_vba_sidecar.ipc import CabinetArray
from lmt_vba_sidecar.capture_planner.geometry import expand_screen
from lmt_vba_sidecar.capture_planner.visibility import coverage_report, vis_count
from lmt_vba_sidecar.capture_planner import gates


def _single_flat_cabinet():
    cab = CabinetArray(cols=1, rows=1, cabinet_size_mm=[500.0, 500.0], absent_cells=[])
    return expand_screen(cab, "flat", sample_grid=(4, 4))


def _good_cam(K, pos, geom):
    # a camera that frontally sees the whole single cabinet
    return look_at_camera(K, pos, geom.cabinets[0].center_mm, (1920, 1080))


def test_guardrail_center_in_frame_but_points_clipped_is_not_covered():
    # Codex guardrail: the cabinet CENTER projects in-frame (the old
    # center-shortcut would PASS), but a tiny 64x64 frame with long focal length
    # clips every off-center sample point -> vis_count 0 -> NOT covered.
    geom = _single_flat_cabinet()
    cabg = geom.cabinets[0]
    K = np.array([[2000.0, 0.0, 32.0], [0.0, 2000.0, 32.0], [0.0, 0.0, 1.0]])
    cam = look_at_camera(K, [250.0, 250.0, 1000.0], cabg.center_mm, (64, 64))
    # the geometric center would have passed a center-only test:
    assert point_visible(cam, cabg.center_mm, cabg.normal) is True
    # but per-point gating sees < MIN_PNP_CORNERS sample points:
    assert vis_count(cam, cabg) < gates.MIN_PNP_CORNERS
    per_cab, _ = coverage_report(geom, [cam])
    assert per_cab[0].reconstructable is False


def test_one_view_not_reconstructable_even_if_all_points_visible():
    geom = _single_flat_cabinet()
    K = intrinsics_from_fov((1920, 1080), hfov_deg=50.0)
    cam = _good_cam(K, [250.0, 250.0, 3000.0], geom)
    assert vis_count(cam, geom.cabinets[0]) == 16   # sees all sample points
    per_cab, _ = coverage_report(geom, [cam])
    cov = per_cab[0]
    assert cov.total_observations >= gates.MIN_POINTS_PER_CABINET  # points gate OK
    assert len(cov.covering_cams) == 1
    assert cov.reconstructable is False              # ... but views gate fails


def test_two_views_reconstructable_but_low_observation():
    geom = _single_flat_cabinet()
    K = intrinsics_from_fov((1920, 1080), hfov_deg=50.0)
    cams = [
        _good_cam(K, [-1500.0, 250.0, 3000.0], geom),
        _good_cam(K, [2000.0, 250.0, 3000.0], geom),
    ]
    per_cab, _ = coverage_report(geom, cams)
    cov = per_cab[0]
    assert len(cov.covering_cams) == 2
    assert cov.reconstructable is True
    assert cov.low_observation is True               # 2 < QUALITY_MIN_VIEWS(4)


def test_four_views_not_low_observation():
    geom = _single_flat_cabinet()
    K = intrinsics_from_fov((1920, 1080), hfov_deg=50.0)
    cams = [
        _good_cam(K, [-1500.0, 250.0, 3000.0], geom),
        _good_cam(K, [600.0, 250.0, 3000.0], geom),
        _good_cam(K, [2000.0, 250.0, 3000.0], geom),
        _good_cam(K, [250.0, 1800.0, 3000.0], geom),
    ]
    per_cab, _ = coverage_report(geom, cams)
    cov = per_cab[0]
    assert len(cov.covering_cams) == 4
    assert cov.reconstructable is True
    assert cov.low_observation is False


from lmt_vba_sidecar.capture_planner.visibility import bridging_report


def test_bridging_single_camera_covering_two_adjacent_is_one_component():
    cab = CabinetArray(cols=2, rows=1, cabinet_size_mm=[500.0, 500.0], absent_cells=[])
    geom = expand_screen(cab, "flat", sample_grid=(4, 4))
    K = intrinsics_from_fov((1920, 1080), hfov_deg=70.0)
    # one camera centered on the 2-wide wall, far enough to cover both cabinets
    center = np.array([500.0, 250.0, 0.0])
    cam = look_at_camera(K, center + [0.0, 0.0, 4000.0], center, (1920, 1080))
    rep = bridging_report(geom, [cam])
    assert rep.broken_edges == []
    assert rep.n_components == 1


def test_bridging_disjoint_cameras_break_the_chain():
    cab = CabinetArray(cols=2, rows=1, cabinet_size_mm=[500.0, 500.0], absent_cells=[])
    geom = expand_screen(cab, "flat", sample_grid=(4, 4))
    K = intrinsics_from_fov((1920, 1080), hfov_deg=30.0)
    # two tight-FOV cameras close in, each frontal to ONE cabinet only; at 800mm
    # the other cabinet's nearest (seam) column projects off-sensor -> no shared cover
    left_c = np.array([250.0, 250.0, 0.0])
    right_c = np.array([750.0, 250.0, 0.0])
    cams = [
        look_at_camera(K, left_c + [0.0, 0.0, 800.0], left_c, (1920, 1080)),
        look_at_camera(K, right_c + [0.0, 0.0, 800.0], right_c, (1920, 1080)),
    ]
    rep = bridging_report(geom, cams)
    assert ((0, 0), (1, 0)) in rep.broken_edges or ((1, 0), (0, 0)) in rep.broken_edges
    assert rep.n_components == 2


from lmt_vba_sidecar.capture_planner.geometry import ArcOccluder
from lmt_vba_sidecar.capture_planner.visibility import point_visible as pv


def test_arc_occlusion_blocks_far_point_from_end_camera():
    radius = 2500.0
    width = 6000.0
    arc = ArcOccluder(cx=width / 2.0, cz=radius, radius=radius,
                      a_min=-width / (2 * radius), a_max=width / (2 * radius))
    a = width / (2 * radius)
    q = np.array([arc.cx + radius * np.sin(a), 250.0, radius - radius * np.cos(a)])
    n = np.array([np.sin(a), 0.0, np.cos(a)])
    K = intrinsics_from_fov((3840, 2160), hfov_deg=70.0)
    cam = look_at_camera(K, [-4000.0, 250.0, 3500.0], q, (3840, 2160))
    assert pv(cam, q, n, arc=arc) is False


def test_arc_occlusion_does_not_block_frontal_view():
    radius = 2500.0
    width = 6000.0
    arc = ArcOccluder(cx=width / 2.0, cz=radius, radius=radius,
                      a_min=-width / (2 * radius), a_max=width / (2 * radius))
    q = np.array([arc.cx, 250.0, 0.0])
    n = np.array([0.0, 0.0, 1.0])
    K = intrinsics_from_fov((3840, 2160), hfov_deg=70.0)
    cam = look_at_camera(K, [arc.cx, 250.0, 5000.0], q, (3840, 2160))
    assert pv(cam, q, n, arc=arc) is True
