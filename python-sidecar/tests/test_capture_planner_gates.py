import inspect

from lmt_vba_sidecar import reconstruct
from lmt_vba_sidecar.observability import check_observability
from lmt_vba_sidecar.capture_planner import gates


def test_gate_constants_mirror_reconstruct():
    # PnP corner floor and quality-view threshold are importable module
    # constants in reconstruct.py — assert exact mirror.
    assert gates.MIN_PNP_CORNERS == reconstruct.MIN_PNP_CORNERS
    assert gates.QUALITY_MIN_VIEWS == reconstruct.QUALITY_MIN_VIEWS


def test_gate_constants_mirror_check_observability_defaults():
    # min_views / min_points live as defaults on check_observability.
    sig = inspect.signature(check_observability)
    assert gates.MIN_VIEWS == sig.parameters["min_views"].default
    assert gates.MIN_POINTS_PER_CABINET == sig.parameters["min_points"].default
