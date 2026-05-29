import numpy as np
import pytest
from lmt_vba_sidecar.sl_decode import load_frames, segment_code_region, index_plateaus


def _white(h=120, w=160):
    return np.full((h, w), 255, np.uint8)


def _g(v, h=120, w=160):
    return np.full((h, w), v, np.uint8)


def test_segment_excludes_sentinels():
    frames = [_white(), _g(10), _g(200), _g(10), _white()]
    assert segment_code_region(frames, sentinel_threshold=0.85) == (1, 4)


def test_segment_skips_full_held_sentinel_runs():
    # recorded/held video: each logical frame spans many camera frames, so the
    # white sentinels are CONTIGUOUS RUNS. Must skip both full runs, not just the
    # first/last bright frame (else index_plateaus sees extra white plateaus).
    frames = [_white(), _white(), _g(10), _g(200), _g(10), _white(), _white()]
    assert segment_code_region(frames, sentinel_threshold=0.85) == (2, 5)


def test_index_plateaus_counts_anchor_plus_code():
    # anchor + 1 code frame, captured 3x each
    region = [_g(180), _g(180), _g(180), _g(40), _g(40), _g(40)]
    reps = index_plateaus(region, expected=2)   # expected = total_bits + 1
    assert len(reps) == 2


def test_index_plateaus_raises_on_mismatch():
    with pytest.raises(ValueError):
        index_plateaus([_g(10), _g(200)], expected=5)


import json
from lmt_vba_sidecar.ipc import GenerateStructuredLightInput, DecodeStructuredLightInput
from lmt_vba_sidecar.structured_light import run_generate_structured_light
from lmt_vba_sidecar.sl_decode import run_decode_structured_light


def _gen(tmp_path):
    cmd = GenerateStructuredLightInput.model_validate({
        "command": "generate_structured_light", "version": 1,
        "project": {"screen_id": "MAIN",
                    "cabinet_array": {"cols": 1, "rows": 1, "absent_cells": [],
                                      "cabinet_size_mm": [500, 500]}},
        "output_dir": str(tmp_path / "sl"), "screen_resolution": [960, 540],
        "dot_spacing_px": 160, "margin_px": 80,
    })
    assert run_generate_structured_light(cmd) == 0
    return tmp_path / "sl"


def test_roundtrip_recovers_every_dot_including_id0(tmp_path):
    sl = _gen(tmp_path)
    meta = json.loads((sl / "sl_meta.json").read_text())
    dec = DecodeStructuredLightInput.model_validate({
        "command": "decode_structured_light", "version": 1,
        "input_path": str(sl / "frames"), "sl_meta_path": str(sl / "sl_meta.json"),
        "output_path": str(tmp_path / "corr.json")})
    assert run_decode_structured_light(dec) == 0
    corr = json.loads((tmp_path / "corr.json").read_text())
    by_id = {p["id"]: p for p in corr["points"]}
    assert len(corr["points"]) == len(meta["dots"])
    assert 0 in by_id                                  # id=0 must be recovered
    for d in meta["dots"]:
        p = by_id[d["id"]]
        assert abs(p["x"] - d["u"]) < 1.0 and abs(p["y"] - d["v"]) < 1.0


def test_roundtrip_from_held_video_sequence_mp4(tmp_path):
    # The advertised video path: decode the generated sequence.mp4 (each logical
    # frame held hold_repeat times -> sentinels span contiguous runs). Exercises
    # the held-sentinel segmentation + plateau indexing through a real (lossy
    # mp4v) codec end to end.
    sl = _gen(tmp_path)
    meta = json.loads((sl / "sl_meta.json").read_text())
    dec = DecodeStructuredLightInput.model_validate({
        "command": "decode_structured_light", "version": 1,
        "input_path": str(sl / "sequence.mp4"), "sl_meta_path": str(sl / "sl_meta.json"),
        "output_path": str(tmp_path / "corr.json")})
    assert run_decode_structured_light(dec) == 0
    corr = json.loads((tmp_path / "corr.json").read_text())
    by_id = {p["id"]: p for p in corr["points"]}
    assert len(corr["points"]) == len(meta["dots"])
    assert 0 in by_id                                  # id=0 recovered from the anchor


def test_correspondence_has_provenance(tmp_path):
    sl = _gen(tmp_path)
    dec = DecodeStructuredLightInput.model_validate({
        "command": "decode_structured_light", "version": 1,
        "input_path": str(sl / "frames"), "sl_meta_path": str(sl / "sl_meta.json"),
        "output_path": str(tmp_path / "corr.json")})
    run_decode_structured_light(dec)
    corr = json.loads((tmp_path / "corr.json").read_text())
    import hashlib
    expect_hash = hashlib.sha256((sl / "sl_meta.json").read_bytes()).hexdigest()
    assert corr["screen_id"] == "MAIN"
    assert corr["sl_meta_sha256"] == expect_hash
    assert corr["camera_image_size"] == [960, 540]
