import cv2
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


from lmt_vba_sidecar.sl_decode import derive_screen_roi


def test_derive_screen_roi_finds_blinking_rect_ignoring_static_bright_bg():
    # Static bright textured background (range==0) + a blinking rect in the
    # middle (range high). ROI must be the rect, not the whole frame.
    rng = np.random.default_rng(0)
    bg = rng.integers(180, 256, size=(120, 160), dtype=np.uint8)  # bright, static
    frames = []
    for k in range(8):
        f = bg.copy()
        if k % 2 == 0:                       # rect blinks on even frames
            f[40:90, 50:130] = 255
        else:
            f[40:90, 50:130] = 20
        frames.append(f)
    x, y, w, h = derive_screen_roi(frames)
    assert 45 <= x <= 55 and 35 <= y <= 45      # near rect top-left (50,40)
    assert 70 <= w <= 90 and 45 <= h <= 60      # near rect 80x50
    assert (x, y, w, h) != (0, 0, 160, 120)     # not the whole frame


def test_derive_screen_roi_rejects_only_thin_offscreen_motion():
    # Only a thin, non-solid moving streak (an off-screen person/car) and no
    # screen activity -> no solid rect -> raise (caller maps to detection_failed).
    frames = []
    for k in range(8):
        f = np.full((120, 160), 200, np.uint8)
        f[10:14, (10 + k * 8):(14 + k * 8)] = 255   # thin sliding streak
        frames.append(f)
    with pytest.raises(ValueError):
        derive_screen_roi(frames)


def test_segment_uses_roi_mean_not_whole_frame():
    # Whole-frame mean is always bright (lit background), so a global mean would
    # never see the sentinel. Inside the ROI the sentinel run is the only bright
    # thing -> segmentation must use the ROI crop.
    def frame(roi_val):
        f = np.full((120, 160), 240, np.uint8)   # bright everywhere (background)
        f[40:90, 50:130] = roi_val               # ROI content
        return f
    roi = (50, 40, 80, 50)
    frames = [frame(255), frame(10), frame(200), frame(10), frame(255)]
    assert segment_code_region(frames, sentinel_threshold=0.85, roi=roi) == (1, 4)


def test_index_plateaus_changed_pixels_counted_in_roi_only():
    # Off-ROI churn must not create phantom plateau boundaries: only ROI changes
    # split the region. anchor + 1 code frame, held 3x each, with off-ROI noise.
    rng = np.random.default_rng(1)
    def frame(roi_val):
        f = rng.integers(0, 256, size=(120, 160), dtype=np.uint8)  # off-ROI noise
        f[40:90, 50:130] = roi_val
        return f
    roi = (50, 40, 80, 50)
    region = [frame(180), frame(180), frame(180), frame(40), frame(40), frame(40)]
    reps = index_plateaus(region, expected=2, roi=roi)
    assert len(reps) == 2


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


from lmt_vba_sidecar.sl_decode import _seed_dots, _read_bits_relative


def test_seed_dots_otsu_finds_dots_in_bright_roi():
    # Anchor with two lit dots over a bright (200) ROI background; global-128
    # would flood, Otsu must isolate the two dots.
    anchor = np.full((120, 160), 200, np.uint8)
    cv2.circle(anchor, (70, 60), 6, 255, -1)
    cv2.circle(anchor, (110, 60), 6, 255, -1)
    roi = (50, 40, 80, 50)
    seeds = _seed_dots(anchor, roi=roi, dot_radius_px=6)
    assert len(seeds) == 2
    xs = sorted(round(x) for (x, _y) in seeds)
    assert abs(xs[0] - 70) <= 2 and abs(xs[1] - 110) <= 2


def test_seed_dots_filters_oversized_blob():
    anchor = np.full((120, 160), 30, np.uint8)
    cv2.circle(anchor, (70, 60), 6, 255, -1)        # a real dot
    anchor[55:90, 95:130] = 255                     # a big non-dot block
    roi = (50, 40, 80, 50)
    seeds = _seed_dots(anchor, roi=roi, dot_radius_px=6)
    assert len(seeds) == 1


def test_read_bits_relative_uses_own_min_max_not_global_128():
    # A DIM dot: lit ~90, off ~20 (both below the global-128 brightness threshold).
    # Relative reading (own min/max) must still read [1, 0].
    lit = np.full((120, 160), 20, np.uint8)
    cv2.circle(lit, (70, 60), 6, 90, -1)
    off = np.full((120, 160), 20, np.uint8)
    code_frames = [lit, off]
    bits = _read_bits_relative(code_frames, 70.0, 60.0)
    assert bits == [1, 0]
