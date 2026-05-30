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
