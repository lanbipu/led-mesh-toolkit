# Precision Mode — Subpixel Centroid (L2) Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Upgrade structured-light dot localization from a binary Otsu component centroid to an intensity-weighted centroid, so each emitted correspondence `(x,y)` is more sub-pixel accurate — but ONLY make it the default if it beats Otsu on field-like fixtures (LED bloom, saturation flat-top, glare, merged components), with Otsu retained as the per-component fallback.

**Architecture:** Single-file change in `sl_decode._seed_dots`. The Otsu binary mask still does component discovery + shape filtering (unchanged); only the centroid *coordinate* of each accepted component changes. `corr.json` format is unchanged (`{id,u,v,x,y}`) — bit-decode is unaffected (it rounds the seed to integer anyway), so this is decode-safe. This change affects BOTH quick and precision modes (shared decode); precision does NOT depend on it (precision accuracy comes from L1 intrinsics). Promotion is gated on field-like fixtures, not clean synthetic, per spec §A.2 + Codex finding 2.

**Tech Stack:** Python sidecar (`lmt_vba_sidecar.sl_decode`, OpenCV, NumPy, pytest).

**Source spec:** `docs/superpowers/specs/2026-06-04-precision-mode-design.md` §A.2 (L2). **Branch:** `feat/precision-mode`.

**Scope note:** Plan 2 of 3. Independent of Plan 1 (intrinsics) and Plan 3 (planner/compare-known). No CLI/DTO/schema change.

---

## File Structure

| File | Responsibility | Change |
|---|---|---|
| `python-sidecar/src/lmt_vba_sidecar/sl_decode.py` | dot seeding/decoding | `_seed_dots`: capture label image, intensity-weighted centroid + Otsu fallback |
| `python-sidecar/tests/test_sl_decode.py` | decode unit tests | subpixel-truth test + field-like fixtures gate |

**Key current code** (`sl_decode.py:177-198`, verbatim): `_seed_dots` Otsu-thresholds `crop = anchor[y:y+h, x:x+w]` (line 184) → `n, _lbl, stats, cent = cv2.connectedComponentsWithStats(bw, connectivity=8)` (line 186, `_lbl` discarded) → shape-filters components (188-196) → `out.append((float(cent[i][0]) + x, float(cent[i][1]) + y))` (line 197, the binary centroid). The seed flows UNCHANGED into the emitted point `{"x": x, "y": y}` (`sl_decode.py:297`) and is rounded to int only for bit sampling (`:215`), so a centroid change cannot alter which id decodes.

---

## Task 1: Subpixel-truth test (TDD red) — weighted centroid beats Otsu on a known fractional center

**Files:**
- Test: `python-sidecar/tests/test_sl_decode.py`

- [ ] **Step 1: Write the failing test** — render a single anti-aliased dot whose intensity centroid is at a KNOWN fractional position; assert the seed is within 0.3px (Otsu's binary centroid lands on a coarser integer-ish grid and will miss this tolerance until weighting is added).

```python
# append to tests/test_sl_decode.py (after the existing _seed_dots tests, ~line 193)
def _draw_soft_dot(img, cx, cy, radius, peak=255):
    """Anti-aliased filled disc: pixel value scales with coverage of the disc,
    so the INTENSITY centroid sits at the true (fractional) (cx, cy)."""
    h, w = img.shape
    yy, xx = np.mgrid[0:h, 0:w]
    d = np.sqrt((xx - cx) ** 2 + (yy - cy) ** 2)
    cov = np.clip(radius + 0.5 - d, 0.0, 1.0)        # soft edge
    img[:] = np.maximum(img, (cov * peak).astype(np.uint8))


def _otsu_centroid_baseline(anchor, roi, dot_radius_px):
    """The pre-change Otsu binary component centroid, for the SAME accepted component —
    the baseline the weighted centroid must beat. (Shared with Task 3.)"""
    x, y, w, h = roi
    crop = anchor[y:y + h, x:x + w]
    _t, bw = cv2.threshold(crop, 0, 255, cv2.THRESH_BINARY + cv2.THRESH_OTSU)
    n, _lbl, stats, cent = cv2.connectedComponentsWithStats(bw, connectivity=8)
    r = float(dot_radius_px)
    out = []
    for i in range(1, n):
        cw, ch, area = int(stats[i][2]), int(stats[i][3]), int(stats[i][4])
        if 0.25 * np.pi * r * r <= area <= 9.0 * np.pi * r * r and cw <= 6 * r and ch <= 6 * r:
            out.append((float(cent[i][0]) + x, float(cent[i][1]) + y))
    return out


def test_seed_dots_weighted_centroid_beats_otsu_baseline():
    # Codex #5 fix: a SYMMETRIC soft dot's Otsu centroid is already ~0.08px accurate, so a
    # "Otsu misses 0.3px" RED is false. Instead assert the weighted centroid is STRICTLY
    # better than the Otsu baseline. Before Task 2, _seed_dots RETURNS the Otsu centroid, so
    # weighted_err == otsu_err and the strict `<` is guaranteed to FAIL (sound RED). Use a
    # glare-biased fixture where the global-Otsu binary shape is skewed but the dot's
    # intensity profile is symmetric, so the photometric (weighted) centroid is better.
    anchor = np.full((120, 160), 20, np.uint8)
    yy, xx = np.mgrid[0:120, 0:160]
    anchor = np.clip(anchor.astype(np.int16) + ((xx - 50) * 1.6).clip(0, 150), 0, 255).astype(np.uint8)  # glare ramp
    true_x, true_y = 70.37, 60.62
    _draw_soft_dot(anchor, true_x, true_y, radius=5)
    roi = (50, 40, 80, 50)
    seeds = _seed_dots(anchor, roi=roi, dot_radius_px=5)
    assert len(seeds) >= 1
    def err(pts):
        return min(np.hypot(sx - true_x, sy - true_y) for (sx, sy) in pts)
    w_err = err(seeds)
    o_err = err(_otsu_centroid_baseline(anchor, roi, dot_radius_px=5))
    assert w_err < o_err, f"weighted {w_err:.3f} not strictly better than otsu {o_err:.3f}"
```

- [ ] **Step 2: Run to verify it fails (and CONFIRM it is RED, not already-green)**

Run: `cd python-sidecar && .venv/bin/python -m pytest tests/test_sl_decode.py::test_seed_dots_weighted_centroid_beats_otsu_baseline -v`
Expected: FAIL — before Task 2, `_seed_dots` returns the Otsu centroid, so `w_err == o_err` and the strict `<` fails. **You MUST see this fail before Task 2** (Codex #5: a symmetric clean-dot test is already green and proves nothing). If it does NOT fail, the fixture is not discriminating — increase the glare ramp / shrink the dot until the Otsu baseline is measurably biased.

- [ ] **Step 3: Commit the failing test** (kept red until Task 2)

```bash
git add python-sidecar/tests/test_sl_decode.py
git commit -m "test(sl): subpixel-truth test for weighted dot centroid (red)"
```

---

## Task 2: Intensity-weighted centroid + Otsu fallback in `_seed_dots`

**Files:**
- Modify: `python-sidecar/src/lmt_vba_sidecar/sl_decode.py:177-198`

- [ ] **Step 1: Implement** — capture the label image (`lbl`), and for each accepted component compute the intensity-weighted centroid over its pixels, falling back to the Otsu `cent[i]` when the weight sum degenerates.

```python
# sl_decode.py — modified _seed_dots (line 186 captures lbl; line 197 swap)
def _seed_dots(anchor: np.ndarray, *, roi: tuple[int, int, int, int],
               dot_radius_px: int) -> list[tuple[float, float]]:
    x, y, w, h = roi
    crop = anchor[y:y + h, x:x + w]
    _t, bw = cv2.threshold(crop, 0, 255, cv2.THRESH_BINARY + cv2.THRESH_OTSU)
    n, lbl, stats, cent = cv2.connectedComponentsWithStats(bw, connectivity=8)
    r = float(dot_radius_px)
    area_lo, area_hi = 0.25 * np.pi * r * r, 9.0 * np.pi * r * r
    side_hi = 6.0 * r
    cropf = crop.astype(np.float64)
    floor = float(_t)                                  # Otsu threshold = background floor
    out: list[tuple[float, float]] = []
    for i in range(1, n):
        cw, ch, area = int(stats[i][2]), int(stats[i][3]), int(stats[i][4])
        if not (area_lo <= area <= area_hi):
            continue
        if cw > side_hi or ch > side_hi:
            continue
        cx, cy = _weighted_centroid(cropf, lbl, i, floor, fallback=(cent[i][0], cent[i][1]))
        out.append((cx + x, cy + y))
    return out


def _weighted_centroid(cropf: np.ndarray, lbl: np.ndarray, i: int, floor: float,
                       *, fallback: tuple[float, float]) -> tuple[float, float]:
    """Intensity-weighted centroid over component i's pixels (weight = intensity
    above the Otsu floor). Falls back to the binary centroid when weights vanish
    (e.g. a saturation flat-top where all weights are equal -> still fine, but a
    zero-weight region degenerates)."""
    ys, xs = np.nonzero(lbl == i)
    wgt = cropf[ys, xs] - floor
    wgt[wgt < 0] = 0.0
    s = wgt.sum()
    if s <= 1e-6:
        return float(fallback[0]), float(fallback[1])
    return float((wgt * xs).sum() / s), float((wgt * ys).sum() / s)
```

- [ ] **Step 2: Run the Task-1 test to verify it passes**

Run: `cd python-sidecar && .venv/bin/python -m pytest tests/test_sl_decode.py::test_seed_dots_weighted_centroid_beats_otsu_baseline -v`
Expected: PASS (weighted centroid now strictly beats the Otsu baseline on the glare fixture).

- [ ] **Step 3: Run the full decode suite (regression — existing `_seed_dots` + roundtrip tests must stay green)**

Run: `cd python-sidecar && .venv/bin/python -m pytest tests/test_sl_decode.py -v`
Expected: PASS — `test_seed_dots_otsu_finds_dots_in_bright_roi`, `test_seed_dots_filters_oversized_blob`, and the roundtrip tests still pass (component discovery + shape filter unchanged; only centroid coordinate moved, well within their tolerances).

- [ ] **Step 4: Commit**

```bash
git add python-sidecar/src/lmt_vba_sidecar/sl_decode.py
git commit -m "feat(sl): intensity-weighted dot centroid with Otsu fallback"
```

---

## Task 3: Field-like fixtures gate (the promotion decision, spec §A.2 + Codex finding 2)

**Files:**
- Test: `python-sidecar/tests/test_sl_decode.py`

- [ ] **Step 1: Write the gate test** — for each field-degradation, assert the weighted centroid error ≤ the Otsu binary centroid error against the known truth. This is the promotion gate: if any field fixture shows weighted WORSE, the test fails and the default should not flip (keep Otsu). Compute both centroids inside the test by calling the seeding twice is not possible (one function), so assert the weighted result (now default) error vs an explicitly-computed Otsu baseline.

```python
# append to tests/test_sl_decode.py
# (_otsu_centroid_baseline is defined in Task 1 — reuse it.)
@pytest.mark.parametrize("degrade", ["bloom", "saturation", "glare", "merged"])
def test_seed_dots_weighted_not_worse_than_otsu_on_field(degrade):
    anchor = np.full((120, 160), 30, np.uint8)
    true_x, true_y = 70.37, 60.62
    _draw_soft_dot(anchor, true_x, true_y, radius=6)
    if degrade == "bloom":
        anchor = cv2.GaussianBlur(anchor, (0, 0), 1.5)
    elif degrade == "saturation":
        _draw_soft_dot(anchor, true_x, true_y, radius=8, peak=255)   # wider flat-top plateau
    elif degrade == "glare":
        yy, xx = np.mgrid[0:120, 0:160]
        anchor = np.clip(anchor.astype(np.int16) + ((xx - 50) * 1.2).clip(0, 120), 0, 255).astype(np.uint8)
    elif degrade == "merged":
        _draw_soft_dot(anchor, true_x + 7, true_y, radius=6)         # a near neighbor (may merge)
    roi = (50, 40, 80, 50)
    weighted = _seed_dots(anchor, roi=roi, dot_radius_px=6)
    otsu = _otsu_centroid_baseline(anchor, roi, dot_radius_px=6)
    # For the merged case the component may split into 1 or 2; assert on the seed nearest truth.
    def nearest_err(seeds):
        if not seeds:
            return 1e9
        return min(np.hypot(sx - true_x, sy - true_y) for (sx, sy) in seeds)
    w_err, o_err = nearest_err(weighted), nearest_err(otsu)
    assert w_err <= o_err + 1e-6, f"{degrade}: weighted {w_err:.3f} worse than otsu {o_err:.3f}"
```

- [ ] **Step 2: Run the gate**

Run: `cd python-sidecar && .venv/bin/python -m pytest tests/test_sl_decode.py -k field -v`
Expected: PASS for all four `degrade` cases. **If any case fails** (weighted worse on, e.g., saturation flat-top), that is the documented signal to NOT flip the default — revert `_seed_dots` to return `cent[i]` and ship the weighted path behind an explicit experimental code path instead (per spec §A.2: "达不到 → 保持 Otsu 默认"). Record which fixture failed in the commit message.

- [ ] **Step 3: End-to-end-through-decode gate (Codex #3 fix)** — the prior plan's "run test_sl_reconstruct.py" is NOT end-to-end: that suite builds corr `(x,y)` directly and never runs `decode`, so it cannot show the centroid change helps real decoded positions. Replace it with a test that runs the FULL `generate → field-degrade frames → decode` pipeline and compares the **decoded dot-position error** (which is exactly the per-correspondence input the BA minimizes — lower decoded error ⇒ ≤ pose error) for weighted-default vs an Otsu baseline (monkeypatched fallback). This gates pose without a camera renderer.

```python
# append to tests/test_sl_decode.py
import lmt_vba_sidecar.sl_decode as sl_decode
from lmt_vba_sidecar.ipc import DecodeStructuredLightInput

def _decode_position_err(frames_dir, meta_path, out_path):
    """Run decode and return the mean |decoded (x,y) - true (u,v)| over matched dots."""
    meta = json.loads(meta_path.read_text())
    uv = {d["id"]: (d["u"], d["v"]) for d in meta["dots"]}
    rc = sl_decode.run_decode_structured_light(DecodeStructuredLightInput.model_validate({
        "command": "decode_structured_light", "version": 1,
        "input_path": str(frames_dir), "sl_meta_path": str(meta_path), "output_path": str(out_path)}))
    assert rc == 0
    pts = json.loads(out_path.read_text())["points"]
    errs = [np.hypot(p["x"] - uv[p["id"]][0], p["y"] - uv[p["id"]][1]) for p in pts if p["id"] in uv]
    return float(np.mean(errs)), len(pts)

def test_weighted_decode_position_not_worse_than_otsu_under_field(tmp_path, monkeypatch):
    # Generate screen frames (existing helper), then apply a glare ramp to every frame to
    # simulate field capture, decode twice (weighted default vs Otsu fallback), compare.
    frames_dir = _gen(tmp_path)                          # existing helper -> sl/frames + sl_meta
    meta_path = tmp_path / "sl" / "sl_meta.json"
    yy, xx = None, None
    for f in sorted(frames_dir.iterdir()):
        img = cv2.imread(str(f), cv2.IMREAD_GRAYSCALE)
        if yy is None:
            yy, xx = np.mgrid[0:img.shape[0], 0:img.shape[1]]
        glared = np.clip(img.astype(np.int16) + ((xx - img.shape[1] // 2) * 0.05).clip(0, 40), 0, 255)
        cv2.imwrite(str(f), glared.astype(np.uint8))
    w_err, w_n = _decode_position_err(frames_dir, meta_path, tmp_path / "w.json")
    # Otsu baseline: force _weighted_centroid to return its Otsu fallback.
    monkeypatch.setattr(sl_decode, "_weighted_centroid",
                        lambda cropf, lbl, i, floor, *, fallback: (float(fallback[0]), float(fallback[1])))
    o_err, o_n = _decode_position_err(frames_dir, meta_path, tmp_path / "o.json")
    assert w_n >= o_n                                    # decode rate not worse
    assert w_err <= o_err + 1e-6, f"weighted decoded-pos err {w_err:.3f} > otsu {o_err:.3f}"
```

(If `_gen` returns the sl dir rather than the frames dir, adjust the `frames_dir`/`meta_path` paths to match the helper at `test_sl_decode.py:107-117`. Verify `DecodeStructuredLightInput`'s field names against `ipc.py` before running — `input_path`/`sl_meta_path`/`output_path` per the decode IPC.)

- [ ] **Step 4: Run + commit** — the field-fixtures centroid gate AND the end-to-end-through-decode gate together are the promotion decision: weighted must be ≤ Otsu on BOTH. If either fails, keep Otsu as default (do not flip), per spec §A.2.

Run: `cd python-sidecar && .venv/bin/python -m pytest tests/test_sl_decode.py -k "field or decode_position" -v`
Expected: PASS.

```bash
git add python-sidecar/tests/test_sl_decode.py
git commit -m "test(sl): field-fixtures + end-to-end-through-decode gate for weighted centroid"
```

---

## Self-Review

**Spec coverage:** §A.2 weighted centroid with Otsu fallback (Task 2 ✓), field-like + **end-to-end-through-decode** pose-proxy gate (Task 3 ✓ — Codex #3 fixed: the centroid gate PLUS a generate→field-degrade→decode test comparing decoded-position error weighted-vs-Otsu, which is the per-correspondence input the BA minimizes, so it gates pose without a camera renderer), no CLI/DTO change (✓), no permanent flag with Otsu retained in-code as fallback (✓ — `_weighted_centroid` falls back to `cent[i]` per-component, and the change only ships if BOTH gates pass). **Codex #5 fixed:** the RED test asserts weighted STRICTLY beats the Otsu baseline (guaranteed-RED when behavior==baseline), not a symmetric-dot tolerance that is already green.

**Placeholder scan:** no TBD; real test + impl code in every step; `_draw_soft_dot`/`_weighted_centroid`/`_otsu_centroid_baseline` fully defined.

**Type consistency:** `_seed_dots` signature unchanged (`-> list[tuple[float, float]]`); `_weighted_centroid(cropf, lbl, i, floor, *, fallback)` consistent between Task 2 definition and its call site; `lbl` (renamed from `_lbl`) used consistently.
