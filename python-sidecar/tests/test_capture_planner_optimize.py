import numpy as np

from lmt_vba_sidecar.ipc import CabinetArray
from lmt_vba_sidecar.capture_planner.geometry import expand_screen
from lmt_vba_sidecar.capture_planner.visibility import intrinsics_from_fov
from lmt_vba_sidecar.capture_planner.seed import Shell
from lmt_vba_sidecar.capture_planner.optimize import candidate_cameras


def _wall(cols, rows):
    cab = CabinetArray(cols=cols, rows=rows, cabinet_size_mm=[500.0, 500.0], absent_cells=[])
    return expand_screen(cab, "flat", sample_grid=(4, 4))


def test_candidates_lie_within_the_shell():
    K = intrinsics_from_fov((1920, 1080), hfov_deg=60.0)
    shell = Shell(2000.0, 6000.0, 400.0, 2400.0)
    geom = _wall(2, 2)
    cams = candidate_cameras(geom, K, (1920, 1080), shell,
                             n_standoff=2, n_height=3, n_azimuth=5)
    assert len(cams) == 2 * 3 * 5
    cx = geom.total_width_mm / 2.0
    for cam in cams:
        pos = -cam.R.T @ cam.t            # camera center in world
        assert 400.0 - 1e-6 <= pos[1] <= 2400.0 + 1e-6      # height in shell
        standoff = np.linalg.norm([pos[0] - cx, pos[2]])    # radial dist in x-z
        assert 2000.0 - 1.0 <= standoff <= 6000.0 + 1.0     # standoff in shell
        assert pos[2] > 0                                    # in front of the wall


from lmt_vba_sidecar.capture_planner.seed import seed_cameras
from lmt_vba_sidecar.capture_planner.optimize import optimize, _score


def _score_kwargs():
    return dict(pixel_sigma=0.2, nominal_deviation_mm=0.5, trials=6,
               seed=0, target_p95_residual_mm=4.0)


def test_score_deficit_scales_with_min_views():
    # The view-deficit term (the greedy's tie-break that bootstraps a cabinet toward
    # min_views) must grow when min_views rises. The `failing` term stays pass-based —
    # `pass` already respects min_views via coverage_report's reconstructable gate (Task 2),
    # so only the deficit is parameterized here. (Include "pass" so failing is computable.)
    report = {(0, 0): {"n_views": 2, "pass": True}, (1, 0): {"n_views": 4, "pass": True}}
    _f2, deficit2 = _score(report, 2, min_views=2)
    _f3, deficit3 = _score(report, 2, min_views=3)
    assert deficit2 == 0            # both cabinets meet 2 views
    assert deficit3 == 1            # cabinet (0,0) is 1 view short of 3


def test_optimize_covers_a_reachable_flat_wall():
    K = intrinsics_from_fov((1920, 1080), hfov_deg=60.0)
    shell = Shell(2000.0, 4000.0, 400.0, 2200.0)
    geom = _wall(2, 2)
    seed = [s.camera for s in seed_cameras(geom, K, (1920, 1080), shell, n_fan=5)]
    result = optimize(geom, K, (1920, 1080), shell, seed_cams=seed,
                      max_stations=16, n_standoff=2, n_height=3, n_azimuth=5,
                      score_kwargs=_score_kwargs())
    assert result.unreachable == []
    assert all(v["pass"] for v in result.report.values())
    assert len(result.cameras) >= len(seed)        # warm-started, add-only


def test_optimize_reports_unreachable_when_shell_too_tight():
    K = intrinsics_from_fov((1920, 1080), hfov_deg=60.0)
    # a degenerate shell collapsed to a single near-frontal pencil: no two views
    # can ever form a baseline -> nothing reconstructable -> all unreachable.
    shell = Shell(3000.0, 3000.0, 1249.0, 1251.0)
    geom = _wall(2, 2)
    result = optimize(geom, K, (1920, 1080), shell, seed_cams=[],
                      max_stations=4, n_standoff=1, n_height=1, n_azimuth=1,
                      score_kwargs=_score_kwargs())
    assert len(result.unreachable) > 0
    assert not all(v["pass"] for v in result.report.values())


def test_optimize_adds_cameras_to_a_single_camera_start():
    # one frontal camera alone -> every cabinet has 1 view -> all fail. The
    # greedy MUST add cameras (exercise the add path) and converge.
    K = intrinsics_from_fov((1920, 1080), hfov_deg=60.0)
    shell = Shell(2000.0, 4000.0, 400.0, 2200.0)
    geom = _wall(2, 2)
    cx, cy = geom.total_width_mm / 2.0, geom.total_height_mm / 2.0
    from lmt_vba_sidecar.capture_planner.visibility import look_at_camera
    lone = look_at_camera(K, [cx, cy, 2500.0], [cx, cy, 0.0], (1920, 1080))
    result = optimize(geom, K, (1920, 1080), shell, seed_cams=[lone],
                      max_stations=16, n_standoff=2, n_height=3, n_azimuth=5,
                      score_kwargs=_score_kwargs())
    assert len(result.cameras) > 1                  # greedy added at least one
    assert result.unreachable == []
    assert all(v["pass"] for v in result.report.values())


def test_optimize_bootstraps_two_views_from_empty_seed():
    # From an EMPTY seed, no single added camera makes any cabinet pass (each
    # cabinet needs 2 views). The old binary "failing count" objective would
    # dead-stop after round 1 (failing unchanged) and report everything
    # unreachable; the view-deficit objective must bootstrap to 2 views and pass.
    K = intrinsics_from_fov((1920, 1080), hfov_deg=60.0)
    shell = Shell(2000.0, 4000.0, 400.0, 2200.0)
    geom = _wall(2, 2)
    result = optimize(geom, K, (1920, 1080), shell, seed_cams=[],
                      max_stations=16, n_standoff=2, n_height=3, n_azimuth=5,
                      score_kwargs=_score_kwargs())
    assert len(result.cameras) >= 2
    assert result.unreachable == []
    assert all(v["pass"] for v in result.report.values())


def test_optimize_never_returns_duplicate_poses():
    # Selected candidates are removed from the pool, so no two chosen cameras
    # share the same pose (duplicate poses have no baseline yet would be counted
    # as independent views).
    K = intrinsics_from_fov((1920, 1080), hfov_deg=60.0)
    shell = Shell(2000.0, 4000.0, 400.0, 2200.0)
    geom = _wall(2, 2)
    result = optimize(geom, K, (1920, 1080), shell, seed_cams=[],
                      max_stations=16, n_standoff=2, n_height=3, n_azimuth=5,
                      score_kwargs=_score_kwargs())
    keys = {(c.R.tobytes(), c.t.tobytes()) for c in result.cameras}
    assert len(keys) == len(result.cameras), "duplicate camera pose in plan"


def test_optimize_respects_max_stations_below_seed_count():
    # max_stations smaller than the recipe seed (7 = 5 fan + top + bottom) must
    # still cap the returned plan at the budget.
    from lmt_vba_sidecar.capture_planner.seed import seed_cameras
    K = intrinsics_from_fov((1920, 1080), hfov_deg=60.0)
    shell = Shell(2000.0, 4000.0, 400.0, 2200.0)
    geom = _wall(2, 2)
    seed = [s.camera for s in seed_cameras(geom, K, (1920, 1080), shell, n_fan=5)]
    assert len(seed) == 7
    result = optimize(geom, K, (1920, 1080), shell, seed_cams=seed,
                      max_stations=3, n_standoff=2, n_height=3, n_azimuth=5,
                      score_kwargs=_score_kwargs())
    assert len(result.cameras) <= 3
