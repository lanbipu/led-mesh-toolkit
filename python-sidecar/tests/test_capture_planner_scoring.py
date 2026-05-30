import numpy as np

from lmt_vba_sidecar.ipc import CabinetArray
from lmt_vba_sidecar.capture_planner.geometry import expand_screen
from lmt_vba_sidecar.capture_planner.visibility import intrinsics_from_fov, look_at_camera
from lmt_vba_sidecar.capture_planner.scoring import score_screen


def _flat_grid():
    cab = CabinetArray(cols=2, rows=2, cabinet_size_mm=[500.0, 500.0], absent_cells=[])
    return expand_screen(cab, "flat", sample_grid=(4, 4))


def _ring(geom, K, n=4, span_deg=40.0, dist=4000.0):
    cx = geom.total_width_mm / 2.0
    cy = geom.total_height_mm / 2.0
    target = np.array([cx, cy, 0.0])
    cams = []
    for a in np.deg2rad(np.linspace(-span_deg / 2, span_deg / 2, n)):
        pos = target + np.array([dist * np.sin(a), 0.0, dist * np.cos(a)])
        cams.append(look_at_camera(K, pos, target, (1920, 1080)))
    return cams


def test_scorer_is_exact_under_zero_noise():
    # No detection noise, no as-built deviation -> estimated poses are exact and
    # triangulation must return the truth. Sub-micron residual proves the
    # Monte-Carlo geometry (observe -> PnP -> triangulate) is wired correctly.
    geom = _flat_grid()
    K = intrinsics_from_fov((1920, 1080), hfov_deg=60.0)
    cams = _ring(geom, K, n=4, span_deg=40.0)
    report = score_screen(geom, cams, pixel_sigma=0.0, nominal_deviation_mm=0.0,
                          trials=3, seed=0)
    for cov in report.values():
        assert cov["reconstructable"] is True
        assert cov["p95_mm"] < 1e-3   # ~0 mm


def test_well_covered_wall_passes_with_small_residual():
    # A genuinely good capture: 6 cameras on a 50-deg arc at 2 m, mild noise.
    # (A flat wall is the weakest geometry for planar PnP, so cameras are kept
    # close and plentiful — far/sparse rigs legitimately exceed a 3 mm target.)
    geom = _flat_grid()
    K = intrinsics_from_fov((1920, 1080), hfov_deg=60.0)
    cams = _ring(geom, K, n=6, span_deg=50.0, dist=2000.0)
    report = score_screen(geom, cams, pixel_sigma=0.3, nominal_deviation_mm=0.5,
                          trials=12, seed=0, target_p95_residual_mm=3.0)
    for cov in report.values():
        assert cov["reconstructable"] is True
        assert cov["bridged"] is True
        assert cov["p95_mm"] < 3.0
        assert cov["pass"] is True


def test_under_observed_cabinet_is_flagged_not_scored():
    geom = _flat_grid()
    K = intrinsics_from_fov((1920, 1080), hfov_deg=60.0)
    # only ONE camera -> every cabinet has 1 covering view -> not reconstructable
    cams = _ring(geom, K, n=1, span_deg=0.0)
    report = score_screen(geom, cams, trials=5, seed=0, target_p95_residual_mm=3.0)
    for cov in report.values():
        assert cov["reconstructable"] is False
        assert cov["pass"] is False
        assert np.isnan(cov["p95_mm"])     # not scored, not an optimistic number


def test_strong_arc_far_end_not_optimistically_covered():
    # A strong wide arc with only frontal-ish cameras: the far ends must NOT all
    # pass (self-occlusion + grazing make them under-observed), proving the
    # planner is honest rather than optimistic about strong curves.
    from lmt_vba_sidecar.ipc import CabinetArray
    cab = CabinetArray(cols=10, rows=1, cabinet_size_mm=[500.0, 500.0], absent_cells=[])
    geom = expand_screen(cab, {"curved": {"radius_mm": 2200.0}}, sample_grid=(4, 4))
    K = intrinsics_from_fov((3840, 2160), hfov_deg=60.0)
    cams = _ring(geom, K, n=3, span_deg=30.0, dist=4000.0)
    report = score_screen(geom, cams, pixel_sigma=0.3, nominal_deviation_mm=1.0,
                          trials=6, seed=0, target_p95_residual_mm=3.0)
    assert not all(v["pass"] for v in report.values())   # far ends not all covered
