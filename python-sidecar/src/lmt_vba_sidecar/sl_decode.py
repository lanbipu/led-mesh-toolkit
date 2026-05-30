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

from lmt_vba_sidecar.dpx import read_dpx_gray8
from lmt_vba_sidecar.io_utils import write_event
from lmt_vba_sidecar.ipc import DecodeStructuredLightInput, ErrorEvent
from lmt_vba_sidecar.sl_codec import decode_bits

_IMG_EXTS = (".png", ".jpg", ".jpeg", ".bmp", ".tif", ".tiff")
_DPX_EXT = ".dpx"


def _read_frame_file(f: pathlib.Path) -> np.ndarray:
    # cv2 cannot decode 10-bit DPX (returns None); route .dpx through our parser.
    if f.suffix.lower() == _DPX_EXT:
        return read_dpx_gray8(f)
    return cv2.imread(str(f), cv2.IMREAD_GRAYSCALE)


def load_frames(input_path: str) -> list[np.ndarray]:
    p = pathlib.Path(input_path)
    if p.is_dir():
        files = sorted(
            f for f in p.iterdir()
            if f.suffix.lower() in _IMG_EXTS or f.suffix.lower() == _DPX_EXT
        )
        return [_read_frame_file(f) for f in files]
    if p.suffix.lower() == _DPX_EXT:          # single .dpx = one frame (still format)
        return [read_dpx_gray8(p)]
    cap = cv2.VideoCapture(str(p))
    frames: list[np.ndarray] = []
    while True:
        ok, fr = cap.read()
        if not ok:
            break
        frames.append(cv2.cvtColor(fr, cv2.COLOR_BGR2GRAY))
    cap.release()
    return frames


def derive_screen_roi(frames: list[np.ndarray]) -> tuple[int, int, int, int]:
    """Pass 1: per-pixel temporal range (max-min) over the whole clip -> screen ROI.

    The screen rectangle is swept by the white sentinel + blinking dots, so it is
    a SOLID high-activity region. Off-screen movers (person/car) are thin, sparse,
    non-solid blobs. We threshold the activity map at a fixed fraction of its peak,
    keep the largest solid (well-filled) component, and return its bounding box.
    Brightness never enters the decision.

    Known limitation: the threshold is a fraction of the GLOBAL peak temporal
    range, so a DIM/oblique screen filmed alongside a BRIGHTER off-screen MOVING
    object (whose motion dominates the peak) can fall below the cut and be missed.
    For that case pass an explicit --screen-roi X,Y,W,H (manual override)."""
    stack = np.stack(frames).astype(np.int16)
    activity = (stack.max(axis=0) - stack.min(axis=0)).astype(np.uint8)
    a_max = int(activity.max())
    if a_max == 0:
        raise ValueError("no temporal activity; nothing blinks (static clip?)")
    # Threshold at a fixed fraction of peak activity, NOT Otsu: when the screen
    # fills the camera frame (the canonical case), the whole activity map is
    # uniformly high (unimodal), and Otsu carves that uniform region into specks.
    # "Changed by > half the peak range" cleanly marks the swept screen region.
    mask = (activity > max(1, a_max // 2)).astype(np.uint8) * 255
    n, _lbl, stats, _cent = cv2.connectedComponentsWithStats(mask, connectivity=8)
    # A screen ROI is BOTH solid (bbox well-filled) AND large; off-screen movers
    # are tiny specks that are individually "100% filled" but cover a negligible
    # fraction of the frame. Keep the largest-area solid component and reject if
    # it is still too small to be a screen.
    frame_area = float(activity.shape[0] * activity.shape[1])
    best: tuple[int, int, int, int] | None = None
    best_area = 0
    for i in range(1, n):
        x, y, w, h, area = (int(stats[i][c]) for c in range(5))
        if w < 4 or h < 4:
            continue
        fill = area / float(w * h)        # how solidly the bbox is filled
        if fill < 0.5:                    # not a solid rectangle
            continue
        if area > best_area:
            best_area, best = area, (x, y, w, h)
    if best is None or best_area < 0.01 * frame_area:   # only thin/small movers
        raise ValueError(
            "could not auto-derive a solid screen ROI from temporal activity; "
            "pass --screen-roi X,Y,W,H to specify it manually")
    return best


def segment_code_region(frames: list[np.ndarray], *, sentinel_threshold: float,
                        roi: tuple[int, int, int, int] | None = None) -> tuple[int, int]:
    """Code region = ONE cycle: the frames between the first white-sentinel RUN
    and the NEXT one. Robust to three real-world capture shapes:
      - single playthrough: [sentinel, code, sentinel] -> the region between them.
      - LOOPED capture: [sentinel, cycle, sentinel, cycle, ...] (e.g. disguise
        looping the .seq); we take the FIRST complete inter-sentinel cycle rather
        than spanning every loop (which would make index_plateaus see N*cycles).
      - a recording that STARTS mid-cycle (missed the opening sentinel): as long
        as it contains >= 2 sentinel runs, the first complete cycle is recovered.
    Each sentinel spans a CONTIGUOUS bright run (held frames, or two adjacent
    loop-boundary whites), so we work in runs, not single bright frames."""
    def _crop(f: np.ndarray) -> np.ndarray:
        if roi is None:
            return f
        x, y, w, h = roi
        return f[y:y + h, x:x + w]
    mb = np.array([float(_crop(f).mean()) for f in frames])
    bright = mb > sentinel_threshold * 255.0
    runs: list[tuple[int, int]] = []          # contiguous bright runs = sentinels
    i, n = 0, len(frames)
    while i < n:
        if bright[i]:
            j = i
            while j < n and bright[j]:
                j += 1
            runs.append((i, j))
            i = j
        else:
            i += 1
    if len(runs) < 2:
        raise ValueError("could not find two white sentinel frames")
    s = runs[0][1]      # first frame after the first sentinel run
    e = runs[1][0]      # first frame of the next sentinel run (exclusive end)
    if s >= e:
        raise ValueError("no code region between white sentinels")
    return s, e


def index_plateaus(region: list[np.ndarray], *, expected: int,
                   roi: tuple[int, int, int, int] | None = None) -> list[int]:
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
    def _crop(f: np.ndarray) -> np.ndarray:
        if roi is None:
            return f
        x, y, w, h = roi
        return f[y:y + h, x:x + w]
    changed = np.array([0] + [
        int((np.abs(_crop(region[i]).astype(np.int16)
                    - _crop(region[i - 1]).astype(np.int16)) > 64).sum())
        for i in range(1, len(region))])
    thr = max(1, int(changed.max()) // 4)
    bounds = [0] + [i for i in range(1, len(region)) if changed[i] > thr] + [len(region)]
    segs = [(bounds[k], bounds[k + 1]) for k in range(len(bounds) - 1) if bounds[k + 1] > bounds[k]]
    if len(segs) != expected:
        raise ValueError(f"expected {expected} plateaus (anchor + code), found {len(segs)}")
    return [(a + b) // 2 for (a, b) in segs]


def _seed_dots(anchor: np.ndarray, *, roi: tuple[int, int, int, int],
               dot_radius_px: int) -> list[tuple[float, float]]:
    """Pass 3.1-3.2: Otsu-threshold the all-on anchor WITHIN the ROI (so id=0 is
    seeded too), keep round components sized like a dot. Returns frame-coords
    sub-pixel centroids. Adaptive threshold (not global 128) catches dim/oblique
    dots; the ROI excludes off-screen bright clutter."""
    x, y, w, h = roi
    crop = anchor[y:y + h, x:x + w]
    _t, bw = cv2.threshold(crop, 0, 255, cv2.THRESH_BINARY + cv2.THRESH_OTSU)
    n, _lbl, stats, cent = cv2.connectedComponentsWithStats(bw, connectivity=8)
    r = float(dot_radius_px)
    area_lo, area_hi = 0.25 * np.pi * r * r, 9.0 * np.pi * r * r
    side_hi = 6.0 * r
    out: list[tuple[float, float]] = []
    for i in range(1, n):
        cw, ch, area = int(stats[i][2]), int(stats[i][3]), int(stats[i][4])
        if not (area_lo <= area <= area_hi):
            continue
        if cw > side_hi or ch > side_hi:        # reject big/elongated blobs
            continue
        out.append((float(cent[i][0]) + x, float(cent[i][1]) + y))
    return out


def _read_bits_relative(code_frames: list[np.ndarray], x: float, y: float,
                        anchor: np.ndarray) -> list[int]:
    """Pass 3.3: read each code frame's on/off for the dot at (x,y) RELATIVE to
    that dot's own min/max across the code frames (not a global 128). Robustly
    handles dim/oblique dots whose lit level sits below the background.

    The all-on `anchor` supplies this dot's per-dot LIT reference level. When a
    dot's samples are CONSTANT across the code region the code frames alone can't
    tell on from off, so we compare the constant level against the anchor-lit
    level: a dot still ~as bright as in the anchor is constant-LIT (all ones), a
    dark one is constant-OFF (all zeros). This keeps an all-ones codeword (the max
    id when data_bits is odd — lit in every code frame) decoding to its true id
    instead of silently collapsing to a duplicate id=0; id=0 (off every code
    frame) still reads all zeros."""
    ix, iy = int(round(x)), int(round(y))

    def _patch(f: np.ndarray) -> float:
        y0, y1 = max(0, iy - 1), min(f.shape[0], iy + 2)
        x0, x1 = max(0, ix - 1), min(f.shape[1], ix + 2)
        return float(f[y0:y1, x0:x1].mean())

    samples = [_patch(f) for f in code_frames]
    lo, hi = min(samples), max(samples)
    if hi - lo < 1e-6:
        on_level = _patch(anchor)
        return [1 if lo > on_level * 0.5 else 0] * len(samples)
    mid = (lo + hi) / 2.0
    return [1 if s > mid else 0 for s in samples]


def run_decode_structured_light(cmd: DecodeStructuredLightInput) -> int:
    meta_path = pathlib.Path(cmd.sl_meta_path)
    meta = json.loads(meta_path.read_text())
    sl_meta_sha256 = hashlib.sha256(meta_path.read_bytes()).hexdigest()
    data_bits = int(meta["code"]["data_bits"])
    total_bits = int(meta["code"]["total_bits"])
    dot_radius_px = int(meta["dot_radius_px"])
    uv_by_id = {int(d["id"]): (float(d["u"]), float(d["v"])) for d in meta["dots"]}

    try:
        frames = load_frames(cmd.input_path)
    except (ValueError, OSError) as exc:
        # ValueError = unsupported/corrupt DPX variant (read_dpx_gray8 guards);
        # OSError/FileNotFoundError = a missing single .dpx path hits read_bytes()
        # before any ValueError. Both must surface as a clean fatal decode_failed,
        # never escape to __main__.py's internal_error+traceback fallback.
        write_event(ErrorEvent(event="error", code="decode_failed",
            message=f"failed to read frames: {exc}", fatal=True))
        return 1
    if not frames:
        write_event(ErrorEvent(event="error", code="decode_failed",
            message="no frames loaded from input", fatal=True))
        return 1
    if len(frames) < total_bits + 3:
        write_event(ErrorEvent(event="error", code="decode_failed",
            message=f"only {len(frames)} frames; need >= {total_bits + 3}", fatal=True))
        return 1
    cam_h, cam_w = frames[0].shape[:2]

    # Pass 1: ROI (manual override wins; else auto from temporal activity).
    try:
        if cmd.screen_roi is not None:
            roi = tuple(int(v) for v in cmd.screen_roi)
        else:
            roi = derive_screen_roi(frames)
    except ValueError as exc:
        write_event(ErrorEvent(event="error", code="detection_failed",
            message=str(exc), fatal=True))
        return 1

    # Pass 2: sentinel segmentation + plateau indexing, restricted to the ROI.
    try:
        s, e = segment_code_region(frames, sentinel_threshold=cmd.sentinel_threshold, roi=roi)
        reps = index_plateaus(frames[s:e], expected=total_bits + 1, roi=roi)
    except ValueError as exc:
        write_event(ErrorEvent(event="error", code="decode_failed", message=str(exc), fatal=True))
        return 1

    anchor = frames[s + reps[0]]
    code_frames = [frames[s + r] for r in reps[1:]]      # total_bits frames

    # Pass 3: seed in ROI (Otsu), filter by shape, read per-dot relative, decode.
    seeds = _seed_dots(anchor, roi=roi, dot_radius_px=dot_radius_px)
    if cmd.emit_debug_image:
        dbg = np.zeros((cam_h, cam_w), dtype=np.uint8)
        for (sx, sy) in seeds:
            cv2.circle(dbg, (int(round(sx)), int(round(sy))), dot_radius_px, 255, -1)
        cv2.imwrite(f"{cmd.output_path}.debug.png", dbg)

    points = []
    for (x, y) in seeds:
        bits = _read_bits_relative(code_frames, x, y, anchor)
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
        "screen_roi": [int(v) for v in roi],
        "points": points,
    }
    pathlib.Path(cmd.output_path).write_text(json.dumps(corr, indent=2))

    from lmt_vba_sidecar.ipc import BaStats, ResultData, ResultEvent
    write_event(ResultEvent(event="result", data=ResultData(
        measured_points=[], ba_stats=BaStats(rms_reprojection_px=0.0, iterations=0, converged=True),
        frame_strategy_used="nominal_anchoring", procrustes_align_rms_m=0.0)))
    return 0
