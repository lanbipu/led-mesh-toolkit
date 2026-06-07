"""VP-QSP pattern generation: artifacts, meta, full_screen detect, capacity."""
from __future__ import annotations

import json

from lmt_vba_sidecar.ipc import GeneratePatternInput
from lmt_vba_sidecar.pattern import run_generate_pattern
from lmt_vba_sidecar.vpqsp_detect import detect_vpqsp_markers


def _result_event(out: str) -> dict:
    for line in out.splitlines():
        line = line.strip()
        if line and json.loads(line).get("event") == "result":
            return json.loads(line)
    raise AssertionError("no result event")


def _error_event(out: str) -> dict | None:
    for line in out.splitlines():
        line = line.strip()
        if line and json.loads(line).get("event") == "error":
            return json.loads(line)
    return None


def _cmd(out_dir, *, cols=2, rows=1, res=(1280, 640), screen_id_code=3) -> GeneratePatternInput:
    return GeneratePatternInput.model_validate({
        "command": "generate_pattern", "version": 1,
        "project": {"screen_id": "MAIN",
                    "cabinet_array": {"cols": cols, "rows": rows, "cabinet_size_mm": [600, 600]}},
        "output_dir": str(out_dir), "screen_resolution": list(res),
        "method": "vpqsp", "screen_id_code": screen_id_code,
    })


def test_generate_vpqsp_emits_artifacts(tmp_path, capsys):
    out = tmp_path / "pattern"
    assert run_generate_pattern(_cmd(out)) == 0
    _result_event(capsys.readouterr().out)
    assert (out / "full_screen.png").exists()
    assert (out / "pattern_meta.json").exists()
    assert {p.name for p in (out / "cabinets").iterdir()} == {"V000_R000.png", "V001_R000.png"}
    meta = json.loads((out / "pattern_meta.json").read_text())
    assert meta["schema_version"] == "vpqsp.v1"
    assert meta["screen_id_code"] == 3
    assert len(meta["cabinets"]) == 2
    cab = meta["cabinets"][0]
    assert {"col", "row", "resolution_px", "markers_x", "markers_y", "marker_px", "pixel_pitch_mm"} <= set(cab)


def test_generated_full_screen_decodes_back(tmp_path, capsys):
    out = tmp_path / "pattern"
    assert run_generate_pattern(_cmd(out, screen_id_code=5)) == 0
    capsys.readouterr()
    meta = json.loads((out / "pattern_meta.json").read_text())
    per_cab = meta["cabinets"][0]["markers_x"] * meta["cabinets"][0]["markers_y"]
    obs = detect_vpqsp_markers([str(out / "full_screen.png")], screen_id_code=5)[str(out / "full_screen.png")]
    cabinets = sorted({tuple(o["cabinet"]) for o in obs})
    assert cabinets == [(0, 0), (1, 0)]
    assert len(obs) == 2 * per_cab  # every marker recovered from the assembled screen


def test_tiny_cabinet_below_marker_floor_is_invalid_input(tmp_path, capsys):
    # A cabinet too small to host >= MIN_MARKERS_PER_CABINET markers must fail loud
    # at generation, not silently ship a pattern that breaks reconstruct.
    out = tmp_path / "pattern"
    # 120x120 per cabinet -> marker grid collapses below the observability floor.
    rc = run_generate_pattern(_cmd(out, cols=2, rows=1, res=(240, 120)))
    assert rc == 1
    err = _error_event(capsys.readouterr().out)
    assert err is not None and err["code"] == "invalid_input"
    assert "VP-QSP markers" in err["message"]


def test_wide_cabinet_caps_markers_not_crashes(tmp_path, capsys):
    # A wide cabinet whose marker grid would exceed the 6-bit local_id capacity
    # must generate cleanly (grid capped at 64), NOT crash with an encode overflow
    # surfacing as internal_error.
    out = tmp_path / "pattern"
    rc = run_generate_pattern(_cmd(out, cols=1, rows=1, res=(1920, 360), screen_id_code=0))
    assert rc == 0
    capsys.readouterr()
    meta = json.loads((out / "pattern_meta.json").read_text())
    cab = meta["cabinets"][0]
    assert cab["markers_x"] * cab["markers_y"] <= 64
    # And it round-trips: every marker decodes (no invalid local_id slipped through).
    obs = detect_vpqsp_markers([str(out / "full_screen.png")], screen_id_code=0)[str(out / "full_screen.png")]
    assert 8 <= len(obs) <= 64


def test_grid_beyond_address_space_is_invalid_input(tmp_path, capsys):
    # >128 columns exceeds the marker's 7-bit cab_col field → clean invalid_input,
    # not an encode-time crash (internal_error).
    out = tmp_path / "pattern"
    rc = run_generate_pattern(_cmd(out, cols=130, rows=1, res=(130 * 256, 256)))
    assert rc == 1
    err = _error_event(capsys.readouterr().out)
    assert err is not None and err["code"] == "invalid_input"
    assert "address space" in err["message"]


def test_no_aruco_capacity_ceiling(tmp_path, capsys):
    # 30 cabinets would overflow ChArUco's 1000-marker dictionary (~13 cap);
    # VP-QSP generates them without complaint.
    out = tmp_path / "pattern"
    rc = run_generate_pattern(_cmd(out, cols=6, rows=5, res=(6 * 640, 5 * 640)))
    assert rc == 0
    capsys.readouterr()
    meta = json.loads((out / "pattern_meta.json").read_text())
    assert len(meta["cabinets"]) == 30
