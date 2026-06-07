"""VP-QSP marker detection across an image set.

Ported from vpcal's VP-QCP detector (pure cv2 + numpy, no domain coupling) and
adapted to lmt's detection seam. Pipeline per image:

  optional normal−inverted differencing → Otsu threshold + morph-close →
  external contours → 4-vertex convex quad candidates → perspective rectify to a
  canonical 7x7 panel → cell sampling → 4-rotation orientation gate → 32-bit
  decode + CRC-8 check → diagonal-intersection-seeded Gaussian centroid.

Output matches the ChArUco detect seam shape so reconstruct's Observation
assembly is near-identical:

  {"path": [{"cabinet": (col, row), "screen_id": int, "local_id": int,
             "corner_px": [x, y]}]}

The Gaussian centroid is the load-bearing sub-pixel measurement fed to BA.
"""

from __future__ import annotations

from dataclasses import dataclass

import cv2
import numpy as np
from numpy.typing import NDArray

from lmt_vba_sidecar.vpqsp_codec import (
    GRID,
    _MARGIN_FRAC,
    VpqspMarkerId,
    cellgrid_to_code,
    decode_marker,
    orientation_ok,
)

_CANON_CELL_PX = 12  # canonical pixels per cell for rectified sampling (7*12 = 84)


@dataclass
class VpqspDetectorConfig:
    min_area_px: float = 200.0
    max_area_frac: float = 0.25  # reject quads larger than this fraction of the image


def _order_corners(pts: NDArray[np.float64]) -> NDArray[np.float64]:
    """Order 4 points as TL, TR, BR, BL (image convention, y down)."""
    pts = pts.reshape(4, 2).astype(np.float64)
    s = pts.sum(axis=1)
    d = np.diff(pts, axis=1).ravel()
    return np.array(
        [pts[np.argmin(s)], pts[np.argmin(d)], pts[np.argmax(s)], pts[np.argmax(d)]],
        dtype=np.float64,
    )


def _threshold(gray: NDArray[np.uint8]) -> NDArray[np.uint8]:
    blur = cv2.GaussianBlur(gray, (3, 3), 0)
    _, th = cv2.threshold(blur, 0, 255, cv2.THRESH_BINARY + cv2.THRESH_OTSU)
    return cv2.morphologyEx(th, cv2.MORPH_CLOSE, np.ones((3, 3), np.uint8))


def _sample_cellgrid(rect: NDArray[np.uint8]) -> NDArray[np.int_]:
    """Sample the GRID×GRID cell value grid from a rectified marker panel image."""
    n = GRID * _CANON_CELL_PX
    margin = int(round(n * _MARGIN_FRAC))
    panel = rect[margin : n - margin, margin : n - margin]
    ps = panel.shape[0]
    cell = ps / GRID
    vals = np.zeros((GRID, GRID), dtype=np.float64)
    for r in range(GRID):
        for c in range(GRID):
            y0 = int(round(r * cell + cell * 0.25))
            y1 = int(round(r * cell + cell * 0.75))
            x0 = int(round(c * cell + cell * 0.25))
            x1 = int(round(c * cell + cell * 0.75))
            vals[r, c] = panel[y0:y1, x0:x1].mean()
    thresh = (vals.max() + vals.min()) / 2.0
    return (vals > thresh).astype(int)


def _decode_quad(gray: NDArray[np.uint8], corners: NDArray[np.float64]) -> VpqspMarkerId | None:
    """Rectify a quad and decode its marker id over the 4 orientations."""
    n = GRID * _CANON_CELL_PX
    dst = np.array([[0, 0], [n - 1, 0], [n - 1, n - 1], [0, n - 1]], dtype=np.float32)
    H = cv2.getPerspectiveTransform(corners.astype(np.float32), dst)
    rect = cv2.warpPerspective(gray, H, (n, n))
    grid = _sample_cellgrid(rect)
    for k in range(4):
        g = np.rot90(grid, k)
        if orientation_ok(g):
            marker = decode_marker(cellgrid_to_code(g))
            if marker is not None:
                return marker
    return None


def _diagonal_intersection(corners: NDArray[np.float64]) -> NDArray[np.float64]:
    """Intersection of the quad diagonals = projection of the square's centre."""
    tl, tr, br, bl = corners
    p, r = tl, br - tl
    q, s = tr, bl - tr
    denom = r[0] * s[1] - r[1] * s[0]
    if abs(denom) < 1e-12:
        return corners.mean(axis=0)
    t = ((q[0] - p[0]) * s[1] - (q[1] - p[1]) * s[0]) / denom
    return p + t * r


def _subpixel_center(
    gray: NDArray[np.uint8], corners: NDArray[np.float64]
) -> tuple[float, float]:
    """Intensity-weighted centroid of the central locator dot.

    Seeds at the quad's diagonal intersection, then refines the centroid in a
    window just under one cell so neighbouring bright code cells stay out of the
    integration region. The connected-component mask isolates the dot blob so
    nearby code cells inside the window cannot bias the centroid.
    """
    center = _diagonal_intersection(corners)
    side = np.mean([np.linalg.norm(corners[i] - corners[(i + 1) % 4]) for i in range(4)])
    cell = side * (1.0 - 2.0 * _MARGIN_FRAC) / GRID
    radius = max(3, int(round(cell)))
    cx, cy = float(center[0]), float(center[1])
    for _ in range(3):
        cx0, cy0 = int(round(cx)), int(round(cy))
        x0 = max(0, cx0 - radius)
        x1 = min(gray.shape[1], cx0 + radius + 1)
        y0 = max(0, cy0 - radius)
        y1 = min(gray.shape[0], cy0 + radius + 1)
        win = gray[y0:y1, x0:x1].astype(np.float64)
        if win.size == 0:
            break
        bg = float(np.median(win))
        peak = float(win.max())
        if peak <= bg:
            break
        mask = (win > bg + 0.3 * (peak - bg)).astype(np.uint8)
        _n_lbl, labels = cv2.connectedComponents(mask)
        seed = labels[
            min(int(round(cy)) - y0, win.shape[0] - 1),
            min(int(round(cx)) - x0, win.shape[1] - 1),
        ]
        if seed == 0:
            seed = labels[labels.shape[0] // 2, labels.shape[1] // 2]
        blob = labels == seed
        weights = np.where(blob, win - bg, 0.0)
        total = weights.sum()
        if total <= 0:
            break
        ys, xs = np.mgrid[y0:y1, x0:x1]
        cx = float((xs * weights).sum() / total)
        cy = float((ys * weights).sum() / total)
    return cx, cy


def detect_markers_image(
    image: NDArray[np.uint8],
    *,
    inverted: NDArray[np.uint8] | None = None,
    config: VpqspDetectorConfig | None = None,
) -> list[tuple[VpqspMarkerId, float, float]]:
    """Detect + decode VP-QSP markers in one image.

    `inverted` (optional) enables normal−inverted differencing for ambient
    cancellation. Returns one (marker_id, u, v) per CRC-valid decoded marker; the
    (u, v) is the sub-pixel Gaussian centroid.
    """
    cfg = config or VpqspDetectorConfig()
    gray = image if image.ndim == 2 else cv2.cvtColor(image, cv2.COLOR_BGR2GRAY)
    gray = gray.astype(np.uint8)
    if inverted is not None:
        inv = inverted if inverted.ndim == 2 else cv2.cvtColor(inverted, cv2.COLOR_BGR2GRAY)
        detect_src = cv2.subtract(gray, inv.astype(np.uint8))
    else:
        detect_src = gray

    binary = _threshold(detect_src)
    contours, _ = cv2.findContours(binary, cv2.RETR_EXTERNAL, cv2.CHAIN_APPROX_SIMPLE)
    img_area = gray.shape[0] * gray.shape[1]
    out: list[tuple[VpqspMarkerId, float, float]] = []
    for cnt in contours:
        area = cv2.contourArea(cnt)
        if area < cfg.min_area_px or area > cfg.max_area_frac * img_area:
            continue
        peri = cv2.arcLength(cnt, True)
        approx = None
        for eps in (0.02, 0.03, 0.04, 0.05):
            cand = cv2.approxPolyDP(cnt, eps * peri, True)
            if len(cand) == 4 and cv2.isContourConvex(cand):
                approx = cand
                break
        if approx is None:
            continue
        corners = _order_corners(approx.reshape(4, 2).astype(np.float64))
        marker = _decode_quad(gray, corners)
        if marker is None:
            continue
        u, v = _subpixel_center(gray, corners)
        out.append((marker, u, v))
    return out


def detect_vpqsp_markers(
    image_paths: list[str],
    *,
    screen_id_code: int | None = None,
    config: VpqspDetectorConfig | None = None,
) -> dict[str, list[dict]]:
    """Detect VP-QSP markers across an image set → per-image observation lists.

    Each observation: {"cabinet": (col, row), "screen_id": int, "local_id": int,
    "corner_px": [x, y]}. When `screen_id_code` is set, markers decoded to a
    different screen are dropped (multi-screen Volume disambiguation); None keeps
    all. Unreadable images yield an empty list (not an exception), matching
    detect_charuco_corners' tolerance.
    """
    out: dict[str, list[dict]] = {}
    for path in image_paths:
        img = cv2.imread(path, cv2.IMREAD_GRAYSCALE)
        if img is None:
            out[path] = []
            continue
        observations: list[dict] = []
        for marker, u, v in detect_markers_image(img, config=config):
            if screen_id_code is not None and marker.screen_id != screen_id_code:
                continue
            observations.append({
                "cabinet": (marker.col, marker.row),
                "screen_id": marker.screen_id,
                "local_id": marker.local_id,
                "corner_px": [float(u), float(v)],
            })
        out[path] = observations
    return out
