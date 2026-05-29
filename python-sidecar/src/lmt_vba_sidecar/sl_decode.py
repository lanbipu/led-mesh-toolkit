"""Structured-light decode: recorded capture -> provenance-stamped correspondences.

  1. load frames (video via VideoCapture, or a directory of images)
  2. segment the code region using the bright full-screen white sentinels
  3. index plateaus (each held frame = one plateau); plateau[0] = all-on anchor,
     plateau[1..] = the total_bits code frames
  4. seed every dot location from the anchor (so the all-off id=0 is found too)
  5. read each seeded dot's on/off across code plateaus -> binary+parity -> id
  6. write a CorrespondenceFile with provenance (screen_id, sl_meta_sha256, ...)
All identity decisions are black/white (gamma-immune); the anchor removes any
dependence on a dot being lit in some code frame, and on any screen corner.
"""
from __future__ import annotations

import hashlib
import json
import pathlib

import cv2
import numpy as np

from lmt_vba_sidecar.io_utils import write_event
from lmt_vba_sidecar.ipc import DecodeStructuredLightInput, ErrorEvent
from lmt_vba_sidecar.sl_codec import decode_bits

_IMG_EXTS = (".png", ".jpg", ".jpeg", ".bmp", ".tif", ".tiff")


def load_frames(input_path: str) -> list[np.ndarray]:
    p = pathlib.Path(input_path)
    if p.is_dir():
        files = sorted(f for f in p.iterdir() if f.suffix.lower() in _IMG_EXTS)
        return [cv2.imread(str(f), cv2.IMREAD_GRAYSCALE) for f in files]
    cap = cv2.VideoCapture(str(p))
    frames: list[np.ndarray] = []
    while True:
        ok, fr = cap.read()
        if not ok:
            break
        frames.append(cv2.cvtColor(fr, cv2.COLOR_BGR2GRAY))
    cap.release()
    return frames


def segment_code_region(frames: list[np.ndarray], *, sentinel_threshold: float) -> tuple[int, int]:
    """Code region = frames strictly between the opening and closing white
    sentinel RUNS. A recorded capture (and the held-frame sequence.mp4) repeats
    each logical frame over many camera frames, so each sentinel spans a
    CONTIGUOUS RUN — skip the whole opening run and stop before the whole closing
    run, not just the first/last bright frame (else index_plateaus sees the
    leftover sentinel frames as extra white plateaus and the decode fails)."""
    mb = np.array([float(f.mean()) for f in frames])
    bright = mb > sentinel_threshold * 255.0
    idx = np.where(bright)[0]
    if idx.size < 2:
        raise ValueError("could not find two white sentinel frames")
    s = int(idx[0])
    while s < len(frames) and bright[s]:
        s += 1                       # first frame after the opening sentinel run
    e = int(idx[-1])
    while e >= 0 and bright[e]:
        e -= 1
    e += 1                           # first frame of the closing run (exclusive end)
    if s >= e:
        raise ValueError("no code region between white sentinels")
    return s, e


def index_plateaus(region: list[np.ndarray], *, expected: int) -> list[int]:
    """Split into `expected` plateaus; return the middle index of each. Raises if
    the count != expected. `expected` == total_bits + 1 (anchor + code frames).

    Two input shapes:
      - canonical frames dir (1:1, no playback holds): len(region) == expected,
        so each frame is its own plateau.
      - recorded video (each logical frame held over many camera frames): group
        by CHANGED-PIXEL COUNT, not global mean — a sparse dot pattern barely
        moves the global mean, but a transition flips many dot pixels at once."""
    if not region:
        raise ValueError("empty code region")
    if len(region) == expected:
        return list(range(len(region)))
    changed = np.array([0] + [
        int((np.abs(region[i].astype(np.int16) - region[i - 1].astype(np.int16)) > 64).sum())
        for i in range(1, len(region))])
    thr = max(1, int(changed.max()) // 4)
    bounds = [0] + [i for i in range(1, len(region)) if changed[i] > thr] + [len(region)]
    segs = [(bounds[k], bounds[k + 1]) for k in range(len(bounds) - 1) if bounds[k + 1] > bounds[k]]
    if len(segs) != expected:
        raise ValueError(f"expected {expected} plateaus (anchor + code), found {len(segs)}")
    return [(a + b) // 2 for (a, b) in segs]


def _centroids(frame: np.ndarray) -> list[tuple[float, float]]:
    _, bw = cv2.threshold(frame, 128, 255, cv2.THRESH_BINARY)
    n, _l, _s, cent = cv2.connectedComponentsWithStats(bw, connectivity=8)
    return [(float(cent[i][0]), float(cent[i][1])) for i in range(1, n)]


def _read_bit_at(frame: np.ndarray, x: float, y: float) -> int:
    """1 if the dot at (x,y) is lit in this frame (sample a small patch)."""
    ix, iy = int(round(x)), int(round(y))
    y0, y1 = max(0, iy - 1), min(frame.shape[0], iy + 2)
    x0, x1 = max(0, ix - 1), min(frame.shape[1], ix + 2)
    return 1 if float(frame[y0:y1, x0:x1].mean()) > 128.0 else 0


def run_decode_structured_light(cmd: DecodeStructuredLightInput) -> int:
    meta_path = pathlib.Path(cmd.sl_meta_path)
    meta = json.loads(meta_path.read_text())
    sl_meta_sha256 = hashlib.sha256(meta_path.read_bytes()).hexdigest()
    data_bits = int(meta["code"]["data_bits"])
    total_bits = int(meta["code"]["total_bits"])
    uv_by_id = {int(d["id"]): (float(d["u"]), float(d["v"])) for d in meta["dots"]}

    frames = load_frames(cmd.input_path)
    if not frames:
        write_event(ErrorEvent(event="error", code="decode_failed",
            message="no frames loaded from input", fatal=True))
        return 1
    if len(frames) < total_bits + 3:
        write_event(ErrorEvent(event="error", code="decode_failed",
            message=f"only {len(frames)} frames; need >= {total_bits + 3}", fatal=True))
        return 1
    cam_h, cam_w = frames[0].shape[:2]
    try:
        s, e = segment_code_region(frames, sentinel_threshold=cmd.sentinel_threshold)
        reps = index_plateaus(frames[s:e], expected=total_bits + 1)
    except ValueError as exc:
        write_event(ErrorEvent(event="error", code="decode_failed", message=str(exc), fatal=True))
        return 1

    anchor = frames[s + reps[0]]
    code_frames = [frames[s + r] for r in reps[1:]]      # total_bits frames
    seeds = _centroids(anchor)                            # every dot, incl id=0

    points = []
    for (x, y) in seeds:
        bits = [_read_bit_at(f, x, y) for f in code_frames]
        dot_id = decode_bits(bits, data_bits)
        if dot_id is None or dot_id not in uv_by_id:
            continue
        u, v = uv_by_id[dot_id]
        points.append({"id": dot_id, "u": u, "v": v, "x": x, "y": y})

    if len(points) < max(4, len(uv_by_id) // 10):
        write_event(ErrorEvent(event="error", code="detection_failed",
            message=f"decoded only {len(points)} of {len(uv_by_id)} dots", fatal=True))
        return 1

    corr = {
        "schema_version": 1,
        "screen_id": meta["screen_id"],
        "sl_meta_sha256": sl_meta_sha256,
        "screen_resolution": meta["screen_resolution"],
        "camera_image_size": [int(cam_w), int(cam_h)],
        "source_input": cmd.input_path,
        "points": points,
    }
    pathlib.Path(cmd.output_path).write_text(json.dumps(corr, indent=2))

    from lmt_vba_sidecar.ipc import BaStats, ResultData, ResultEvent
    write_event(ResultEvent(event="result", data=ResultData(
        measured_points=[], ba_stats=BaStats(rms_reprojection_px=0.0, iterations=0, converged=True),
        frame_strategy_used="nominal_anchoring", procrustes_align_rms_m=0.0)))
    return 0
