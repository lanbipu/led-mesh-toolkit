# SL Detection Frontend (靠闪不靠亮 + Screen ROI) Implementation Plan
> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (- [ ]) syntax for tracking.

**Goal:** Replace the structured-light decode frontend's brightness-based detection with a three-pass temporal-change + screen-ROI pipeline so the pipeline decodes correspondences under any brightness/textured static background plus off-screen moving objects, while the existing gray-background synthetic material still decodes 100%.

**Architecture:** All detection logic lives in the Python sidecar `sl_decode.py` (Pass 1 full-clip per-pixel temporal range → coarse screen ROI; Pass 2 ROI-restricted sentinel + plateau indexing; Pass 3 ROI-restricted Otsu anchor seeding + shape filter + per-dot relative bit reading + parity decode gate). The Rust layer (`lmt-cli` clap → `commands/visual.rs` → `lmt-app/visual.rs` → `adapter-visual-ba`) is pure transport: it parses a `--screen-roi X,Y,W,H` string (rejecting bad format as `INVALID_INPUT(2)` before the destructive gate) and an `--emit-debug-image` flag, injects them into the IPC JSON conditionally, and surfaces the actually-used ROI via a new optional `screen_roi` provenance field on `CorrespondenceFile`. The debug image is written to a deterministic `<out>.debug.png` (not via IPC), and dry-run lists it under `would_write`.

**Tech Stack:** Python 3.12 (numpy, OpenCV `cv2`), pydantic IPC models, pytest; Rust (clap, serde, schemars, tokio adapter, `assert_cmd` E2E).

---

## File Structure

| File | Created/Modified | Responsibility |
| --- | --- | --- |
| `python-sidecar/src/lmt_vba_sidecar/sl_decode.py` | Modified | Three-pass detection pipeline: `derive_screen_roi` (Pass 1), ROI-restricted `segment_code_region`/`index_plateaus` (Pass 2), Otsu-seeded + shape-filtered + per-dot-relative bit reading (Pass 3); write `<out>.debug.png` when requested; stamp used `screen_roi` into `corr.json`. |
| `python-sidecar/src/lmt_vba_sidecar/ipc.py` | Modified | `DecodeStructuredLightInput` gains `screen_roi: tuple[int,int,int,int] \| None` + `emit_debug_image: bool=False`; `CorrespondenceFile` gains optional `screen_roi` provenance field. |
| `python-sidecar/tests/test_sl_decode.py` | Modified | New pytest cases S1–S5 + id0 + roi-auto-vs-manual + naive-fails control, using synthetic frames built from `run_generate_structured_light`. |
| `python-sidecar/build_exe.sh` | Run (not edited) | Rebuild the vendored sidecar binary after Python changes. |
| `crates/lmt-cli/src/cli.rs` | Modified | `VisualCmd::DecodeStructuredLight` gains `--screen-roi <X,Y,W,H>` (`Option<String>`) + `--emit-debug-image` (bool flag). |
| `crates/lmt-cli/src/commands/visual.rs` | Modified | `decode_structured_light` handler: parse ROI string → `INVALID_INPUT(2)` BEFORE the destructive gate; thread `screen_roi`/`emit_debug_image`; dry-run `would_write` lists `<out>.debug.png` when `--emit-debug-image`. |
| `crates/lmt-app/src/visual.rs` | Modified | `run_decode_structured_light` signature gains `screen_roi: Option<[u32;4]>` + `emit_debug_image: bool`; threads them into `DecodeStructuredLightArgs`. |
| `crates/adapter-visual-ba/src/api.rs` | Modified | `DecodeStructuredLightArgs` gains the two fields; IPC JSON injects them conditionally (ROI only when `Some`, debug flag only when `true`). |
| `crates/adapter-visual-ba/src/ipc.rs` | Modified | `CorrespondenceFile` Rust mirror gains optional `screen_roi` field. |
| `crates/lmt-shared/src/manifest.rs` | Modified | decode op CLI string adds `[--screen-roi X,Y,W,H] [--emit-debug-image]`; exit_codes stay `[0,2,3,4,13,18]`. |
| `crates/lmt-cli/tests/cli_e2e.rs` | Modified | New cases: `decode_..._with_roi_and_debug_dry_run`, `decode_..._invalid_roi_format` (exit 2), `decode_..._happy` (real sidecar, gray-bg, S1). |
| `docs/agents-cli.md` | Modified | decode-structured-light row signature + description updated for the new flags and three-pass behavior. |

---

### Task 1: IPC models — `screen_roi` + `emit_debug_image` input, `screen_roi` provenance on `CorrespondenceFile`

**Files:**
- `python-sidecar/src/lmt_vba_sidecar/ipc.py` (`DecodeStructuredLightInput` L222-228, `CorrespondenceFile` L239-246)
- `python-sidecar/tests/test_ipc.py` (mirror existing model-validation style)

- [ ] **Step 1: Write the failing test** — append to `python-sidecar/tests/test_ipc.py`:
```python
from lmt_vba_sidecar.ipc import DecodeStructuredLightInput, CorrespondenceFile


def test_decode_input_accepts_screen_roi_and_emit_debug():
    cmd = DecodeStructuredLightInput.model_validate({
        "command": "decode_structured_light", "version": 1,
        "input_path": "frames", "sl_meta_path": "sl_meta.json",
        "output_path": "corr.json",
        "screen_roi": [10, 20, 300, 200], "emit_debug_image": True,
    })
    assert cmd.screen_roi == (10, 20, 300, 200)
    assert cmd.emit_debug_image is True


def test_decode_input_defaults_screen_roi_none_emit_false():
    cmd = DecodeStructuredLightInput.model_validate({
        "command": "decode_structured_light", "version": 1,
        "input_path": "frames", "sl_meta_path": "sl_meta.json",
        "output_path": "corr.json",
    })
    assert cmd.screen_roi is None
    assert cmd.emit_debug_image is False


def test_correspondence_file_screen_roi_optional():
    base = {
        "schema_version": 1, "screen_id": "MAIN",
        "sl_meta_sha256": "deadbeef",
        "screen_resolution": [960, 540],
        "camera_image_size": [960, 540],
        "source_input": "frames", "points": [],
    }
    assert CorrespondenceFile.model_validate(base).screen_roi is None
    with_roi = CorrespondenceFile.model_validate({**base, "screen_roi": [5, 6, 100, 80]})
    assert with_roi.screen_roi == (5, 6, 100, 80)
```

- [ ] **Step 2: Run it, expect FAIL** — `python-sidecar/.venv/bin/python -m pytest python-sidecar/tests/test_ipc.py -k "screen_roi or emit_debug" -q` (expect failures: `DecodeStructuredLightInput` has no `screen_roi`/`emit_debug_image`; `CorrespondenceFile` rejects/lacks `screen_roi`).

- [ ] **Step 3: Minimal implementation** — in `python-sidecar/src/lmt_vba_sidecar/ipc.py` edit `DecodeStructuredLightInput` (L222-228) to add the two fields after `sentinel_threshold`:
```python
class DecodeStructuredLightInput(BaseModel):
    command: Literal["decode_structured_light"]
    version: Literal[1]
    input_path: str           # a video file OR a directory of frame images
    sl_meta_path: str
    output_path: str
    sentinel_threshold: float = Field(gt=0.0, le=1.0, default=0.85)
    # None = auto: Pass-1 temporal-activity map derives the screen ROI. A manual
    # [x, y, w, h] overrides it (fallback when auto fails on hard scenes).
    screen_roi: tuple[int, int, int, int] | None = None
    # Write the Pass-3 seed binary mask to <output_path>.debug.png for eyeball QA.
    emit_debug_image: bool = False
```
and edit `CorrespondenceFile` (L239-246) to add the provenance field after `source_input`:
```python
class CorrespondenceFile(BaseModel):
    schema_version: Literal[1]
    screen_id: str
    sl_meta_sha256: str        # provenance: which pattern/meta produced this
    screen_resolution: PositiveIntPair
    camera_image_size: Annotated[list[int], Field(min_length=2, max_length=2)]
    source_input: str          # the decoded video/dir path
    # Detection provenance: the screen ROI actually used (auto-derived or manual).
    # Optional so old corr.json still validate; reconstruct ignores it.
    screen_roi: tuple[int, int, int, int] | None = None
    points: list[CorrespondencePoint]
```

- [ ] **Step 4: Run tests, expect PASS** — `python-sidecar/.venv/bin/python -m pytest python-sidecar/tests/test_ipc.py -q`.

- [ ] **Step 5: Commit** —
```bash
git add python-sidecar/src/lmt_vba_sidecar/ipc.py python-sidecar/tests/test_ipc.py
git commit -m "$(cat <<'EOF'
feat(sidecar): SL decode IPC gains screen_roi + emit_debug_image; corr.json screen_roi provenance

Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>
EOF
)"
```

---

### Task 2: Pass 1 — `derive_screen_roi` from full-clip per-pixel temporal range

**Files:**
- `python-sidecar/src/lmt_vba_sidecar/sl_decode.py` (new function `derive_screen_roi`, placed after `load_frames` L42)
- `python-sidecar/tests/test_sl_decode.py` (mirror the existing pure-helper style, e.g. `test_segment_excludes_sentinels` L14)

- [ ] **Step 1: Write the failing test** — append to `python-sidecar/tests/test_sl_decode.py` (after the existing pure-helper tests, before the `import json` block at L39):
```python
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
```

- [ ] **Step 2: Run it, expect FAIL** — `python-sidecar/.venv/bin/python -m pytest python-sidecar/tests/test_sl_decode.py -k derive_screen_roi -q` (expect `ImportError: cannot import name 'derive_screen_roi'`).

- [ ] **Step 3: Minimal implementation** — in `python-sidecar/src/lmt_vba_sidecar/sl_decode.py` add after `load_frames` (after L42):
```python
def derive_screen_roi(frames: list[np.ndarray]) -> tuple[int, int, int, int]:
    """Pass 1: per-pixel temporal range (max-min) over the whole clip -> screen ROI.

    The screen rectangle is swept by the white sentinel + blinking dots, so it is
    a SOLID high-activity region. Off-screen movers (person/car) are thin, sparse,
    non-solid blobs. We Otsu-threshold the activity map, keep the connected
    component whose bbox is most rectangle-filled (component area / bbox area),
    and return its bounding box. Brightness never enters the decision."""
    stack = np.stack(frames).astype(np.int16)
    activity = (stack.max(axis=0) - stack.min(axis=0)).astype(np.uint8)
    if int(activity.max()) == 0:
        raise ValueError("no temporal activity; nothing blinks (static clip?)")
    _t, mask = cv2.threshold(activity, 0, 255, cv2.THRESH_BINARY + cv2.THRESH_OTSU)
    n, _lbl, stats, _cent = cv2.connectedComponentsWithStats(mask, connectivity=8)
    best: tuple[int, int, int, int] | None = None
    best_fill = 0.0
    for i in range(1, n):
        x, y, w, h, area = (int(stats[i][c]) for c in range(5))
        if w < 4 or h < 4:
            continue
        fill = area / float(w * h)        # how solidly the bbox is filled
        if fill > best_fill:
            best_fill, best = fill, (x, y, w, h)
    if best is None or best_fill < 0.5:   # no solid rectangle -> only thin movers
        raise ValueError(
            "could not auto-derive a solid screen ROI from temporal activity; "
            "pass --screen-roi X,Y,W,H to specify it manually")
    return best
```

- [ ] **Step 4: Run tests, expect PASS** — `python-sidecar/.venv/bin/python -m pytest python-sidecar/tests/test_sl_decode.py -k derive_screen_roi -q`.

- [ ] **Step 5: Commit** —
```bash
git add python-sidecar/src/lmt_vba_sidecar/sl_decode.py python-sidecar/tests/test_sl_decode.py
git commit -m "$(cat <<'EOF'
feat(sidecar): SL decode Pass 1 — derive_screen_roi from temporal-range activity map

Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>
EOF
)"
```

---

### Task 3: Pass 2 — ROI-restricted sentinel segmentation + plateau indexing

**Files:**
- `python-sidecar/src/lmt_vba_sidecar/sl_decode.py` (`segment_code_region` L45-75, `index_plateaus` L78-100 — add a `roi` keyword that restricts the per-frame statistics to the ROI crop)
- `python-sidecar/tests/test_sl_decode.py` (extend the existing `test_segment_*` / `test_index_plateaus_*` style)

- [ ] **Step 1: Write the failing test** — append to `python-sidecar/tests/test_sl_decode.py`:
```python
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
```

- [ ] **Step 2: Run it, expect FAIL** — `python-sidecar/.venv/bin/python -m pytest python-sidecar/tests/test_sl_decode.py -k "roi_mean or roi_only" -q` (expect `TypeError: segment_code_region() got an unexpected keyword argument 'roi'`).

- [ ] **Step 3: Minimal implementation** — in `python-sidecar/src/lmt_vba_sidecar/sl_decode.py` change `segment_code_region` (L45) and `index_plateaus` (L78) to accept an optional `roi`, cropping each frame's statistic to it. Replace the signature/`mb` line of `segment_code_region`:
```python
def segment_code_region(frames: list[np.ndarray], *, sentinel_threshold: float,
                        roi: tuple[int, int, int, int] | None = None) -> tuple[int, int]:
```
and replace its mean line (L56):
```python
    def _crop(f: np.ndarray) -> np.ndarray:
        if roi is None:
            return f
        x, y, w, h = roi
        return f[y:y + h, x:x + w]
    mb = np.array([float(_crop(f).mean()) for f in frames])
```
Replace `index_plateaus` signature (L78):
```python
def index_plateaus(region: list[np.ndarray], *, expected: int,
                   roi: tuple[int, int, int, int] | None = None) -> list[int]:
```
and replace its `changed` computation (L92-94) to crop both frames before the abs-diff:
```python
    def _crop(f: np.ndarray) -> np.ndarray:
        if roi is None:
            return f
        x, y, w, h = roi
        return f[y:y + h, x:x + w]
    changed = np.array([0] + [
        int((np.abs(_crop(region[i]).astype(np.int16)
                    - _crop(region[i - 1]).astype(np.int16)) > 64).sum())
        for i in range(1, len(region))])
```
(The `len(region) == expected` 1:1 short-circuit at L90-91 stays; it already needs no ROI.)

- [ ] **Step 4: Run tests, expect PASS** — `python-sidecar/.venv/bin/python -m pytest python-sidecar/tests/test_sl_decode.py -k "segment or index_plateaus or roi" -q` (existing `test_segment_excludes_sentinels`, `test_segment_skips_full_held_sentinel_runs`, `test_index_plateaus_*` must still pass — they call without `roi`, exercising the default-None path).

- [ ] **Step 5: Commit** —
```bash
git add python-sidecar/src/lmt_vba_sidecar/sl_decode.py python-sidecar/tests/test_sl_decode.py
git commit -m "$(cat <<'EOF'
feat(sidecar): SL decode Pass 2 — ROI-restricted sentinel mean + plateau change-count

Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>
EOF
)"
```

---

### Task 4: Pass 3 — ROI-restricted Otsu seeding, shape/size filter, per-dot relative bit reading

**Files:**
- `python-sidecar/src/lmt_vba_sidecar/sl_decode.py` (replace `_centroids` L103-106 with `_seed_dots`; replace `_read_bit_at` L109-114 with a per-dot relative reader `_read_bits_relative`)
- `python-sidecar/tests/test_sl_decode.py`

- [ ] **Step 1: Write the failing test** — append to `python-sidecar/tests/test_sl_decode.py`:
```python
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
```

- [ ] **Step 2: Run it, expect FAIL** — `python-sidecar/.venv/bin/python -m pytest python-sidecar/tests/test_sl_decode.py -k "seed_dots or read_bits_relative" -q` (expect `ImportError`).

- [ ] **Step 3: Minimal implementation** — in `python-sidecar/src/lmt_vba_sidecar/sl_decode.py` replace `_centroids` (L103-106) and `_read_bit_at` (L109-114) with:
```python
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


def _read_bits_relative(code_frames: list[np.ndarray], x: float, y: float) -> list[int]:
    """Pass 3.3: read each code frame's on/off for the dot at (x,y) RELATIVE to
    that dot's own min/max across the code frames (not a global 128). Robustly
    handles dim/oblique dots whose lit level sits below the background."""
    ix, iy = int(round(x)), int(round(y))
    samples = []
    for f in code_frames:
        y0, y1 = max(0, iy - 1), min(f.shape[0], iy + 2)
        x0, x1 = max(0, ix - 1), min(f.shape[1], ix + 2)
        samples.append(float(f[y0:y1, x0:x1].mean()))
    lo, hi = min(samples), max(samples)
    if hi - lo < 1e-6:
        return [0] * len(samples)
    mid = (lo + hi) / 2.0
    return [1 if s > mid else 0 for s in samples]
```

- [ ] **Step 4: Run tests, expect PASS** — `python-sidecar/.venv/bin/python -m pytest python-sidecar/tests/test_sl_decode.py -k "seed_dots or read_bits_relative" -q`.

- [ ] **Step 5: Commit** —
```bash
git add python-sidecar/src/lmt_vba_sidecar/sl_decode.py python-sidecar/tests/test_sl_decode.py
git commit -m "$(cat <<'EOF'
feat(sidecar): SL decode Pass 3 — ROI Otsu seeding + shape filter + per-dot relative bit reading

Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>
EOF
)"
```

---

### Task 5: Wire the three passes into `run_decode_structured_light` (debug image + corr.json `screen_roi`)

**Files:**
- `python-sidecar/src/lmt_vba_sidecar/sl_decode.py` (`run_decode_structured_light` L117-175 — call Pass 1 (or manual override), pass `roi` into Pass 2/3, write `<out>.debug.png`, add `screen_roi` to the corr dict)
- `python-sidecar/tests/test_sl_decode.py` (extends the roundtrip-style tests at L58+)

- [ ] **Step 1: Write the failing test** — append to `python-sidecar/tests/test_sl_decode.py` (after the `_gen`/roundtrip helpers; reuses `_gen` from L45):
```python
def test_decode_gray_bg_regression(tmp_path):
    # S1: the existing gray-background synthetic material decodes 100% through
    # the new three-pass frontend (no legacy flag).
    sl = _gen(tmp_path)
    meta = json.loads((sl / "sl_meta.json").read_text())
    dec = DecodeStructuredLightInput.model_validate({
        "command": "decode_structured_light", "version": 1,
        "input_path": str(sl / "frames"), "sl_meta_path": str(sl / "sl_meta.json"),
        "output_path": str(tmp_path / "corr.json")})
    assert run_decode_structured_light(dec) == 0
    corr = json.loads((tmp_path / "corr.json").read_text())
    assert len(corr["points"]) == len(meta["dots"])
    assert 0 in {p["id"] for p in corr["points"]}          # id=0 recovered


def _composite_onto_bg(frames_dir, bg, tmp_path, out_name):
    """Overlay the generated frame dir onto a (h,w) background: where the frame
    has a lit dot/sentinel keep it; elsewhere show the background. Writes a new
    frame dir and returns it."""
    import pathlib
    src = sorted(pathlib.Path(frames_dir).glob("frame_*.png"))
    dst = tmp_path / out_name
    dst.mkdir()
    for i, f in enumerate(src):
        fr = cv2.imread(str(f), cv2.IMREAD_GRAYSCALE)
        comp = np.where(fr > 5, fr, bg).astype(np.uint8)
        cv2.imwrite(str(dst / f"frame_{i:04d}.png"), comp)
    return dst


def test_decode_bright_textured_bg(tmp_path):
    # S2: bright textured static background under the (black) dot frames.
    sl = _gen(tmp_path)
    meta = json.loads((sl / "sl_meta.json").read_text())
    h, w = meta["screen_resolution"][1], meta["screen_resolution"][0]
    rng = np.random.default_rng(7)
    bg = rng.integers(170, 256, size=(h, w), dtype=np.uint8)
    frames = _composite_onto_bg(sl / "frames", bg, tmp_path, "bright")
    dec = DecodeStructuredLightInput.model_validate({
        "command": "decode_structured_light", "version": 1,
        "input_path": str(frames), "sl_meta_path": str(sl / "sl_meta.json"),
        "output_path": str(tmp_path / "corr.json")})
    assert run_decode_structured_light(dec) == 0
    corr = json.loads((tmp_path / "corr.json").read_text())
    assert len(corr["points"]) >= int(0.99 * len(meta["dots"]))
    assert corr["screen_roi"] is not None                  # provenance stamped


def test_decode_bright_textured_bg_fails_with_naive(tmp_path):
    # Control: the OLD global-128 frontend would flood the bright background and
    # decode garbage. Assert the naive centroid pass produces FAR too many blobs.
    from lmt_vba_sidecar.sl_decode import load_frames
    sl = _gen(tmp_path)
    meta = json.loads((sl / "sl_meta.json").read_text())
    h, w = meta["screen_resolution"][1], meta["screen_resolution"][0]
    rng = np.random.default_rng(7)
    bg = rng.integers(170, 256, size=(h, w), dtype=np.uint8)
    frames_dir = _composite_onto_bg(sl / "frames", bg, tmp_path, "bright")
    anchor = load_frames(str(frames_dir))[1]               # all-on anchor frame
    _t, bw = cv2.threshold(anchor, 128, 255, cv2.THRESH_BINARY)  # the naive path
    n, _l, _s, _c = cv2.connectedComponentsWithStats(bw, connectivity=8)
    assert (n - 1) > 2 * len(meta["dots"])                 # naive floods


def test_decode_moving_object_outside_roi(tmp_path):
    # S3: a bright moving block OUTSIDE the screen ROI must not change the result.
    sl = _gen(tmp_path)
    meta = json.loads((sl / "sl_meta.json").read_text())
    h, w = meta["screen_resolution"][1], meta["screen_resolution"][0]
    # Screen content occupies the dot bbox; pad the canvas so the mover is clearly
    # off-screen. Here we just paint a sliding block in the top 20px band.
    src = sorted((sl / "frames").glob("frame_*.png"))
    dst = tmp_path / "mover"; dst.mkdir()
    for i, f in enumerate(src):
        fr = cv2.imread(str(f), cv2.IMREAD_GRAYSCALE)
        fr[0:18, (i * 30) % w:(i * 30) % w + 24] = 255     # off-screen mover
        cv2.imwrite(str(dst / f"frame_{i:04d}.png"), fr)
    dec = DecodeStructuredLightInput.model_validate({
        "command": "decode_structured_light", "version": 1,
        "input_path": str(dst), "sl_meta_path": str(sl / "sl_meta.json"),
        "output_path": str(tmp_path / "corr.json")})
    assert run_decode_structured_light(dec) == 0
    corr = json.loads((tmp_path / "corr.json").read_text())
    assert len(corr["points"]) == len(meta["dots"])        # mover ignored


def test_decode_dim_dots_below_bg(tmp_path):
    # S4: dots dimmer than the background still decode (criterion is change).
    sl = _gen(tmp_path)
    meta = json.loads((sl / "sl_meta.json").read_text())
    h, w = meta["screen_resolution"][1], meta["screen_resolution"][0]
    bg = np.full((h, w), 120, np.uint8)                    # background brighter
    src = sorted((sl / "frames").glob("frame_*.png"))
    dst = tmp_path / "dim"; dst.mkdir()
    for i, f in enumerate(src):
        fr = cv2.imread(str(f), cv2.IMREAD_GRAYSCALE)
        dim = np.where(fr > 5, 70, bg).astype(np.uint8)    # dots=70 < bg=120
        cv2.imwrite(str(dst / f"frame_{i:04d}.png"), dim)
    dec = DecodeStructuredLightInput.model_validate({
        "command": "decode_structured_light", "version": 1,
        "input_path": str(dst), "sl_meta_path": str(sl / "sl_meta.json"),
        "output_path": str(tmp_path / "corr.json")})
    assert run_decode_structured_light(dec) == 0
    corr = json.loads((tmp_path / "corr.json").read_text())
    assert len(corr["points"]) >= int(0.99 * len(meta["dots"]))


def test_decode_finds_id0(tmp_path):
    sl = _gen(tmp_path)
    dec = DecodeStructuredLightInput.model_validate({
        "command": "decode_structured_light", "version": 1,
        "input_path": str(sl / "frames"), "sl_meta_path": str(sl / "sl_meta.json"),
        "output_path": str(tmp_path / "corr.json")})
    assert run_decode_structured_light(dec) == 0
    corr = json.loads((tmp_path / "corr.json").read_text())
    assert 0 in {p["id"] for p in corr["points"]}


def test_roi_auto_vs_manual(tmp_path):
    # Auto-derived ROI and a generous manual ROI must decode the same dot set.
    sl = _gen(tmp_path)
    meta = json.loads((sl / "sl_meta.json").read_text())
    h, w = meta["screen_resolution"][1], meta["screen_resolution"][0]
    auto = DecodeStructuredLightInput.model_validate({
        "command": "decode_structured_light", "version": 1,
        "input_path": str(sl / "frames"), "sl_meta_path": str(sl / "sl_meta.json"),
        "output_path": str(tmp_path / "auto.json")})
    assert run_decode_structured_light(auto) == 0
    manual = DecodeStructuredLightInput.model_validate({
        "command": "decode_structured_light", "version": 1,
        "input_path": str(sl / "frames"), "sl_meta_path": str(sl / "sl_meta.json"),
        "output_path": str(tmp_path / "manual.json"),
        "screen_roi": [0, 0, w, h]})
    assert run_decode_structured_light(manual) == 0
    a = {p["id"] for p in json.loads((tmp_path / "auto.json").read_text())["points"]}
    m = {p["id"] for p in json.loads((tmp_path / "manual.json").read_text())["points"]}
    assert a == m
    assert json.loads((tmp_path / "manual.json").read_text())["screen_roi"] == [0, 0, w, h]


def test_decode_emit_debug_image_writes_png(tmp_path):
    sl = _gen(tmp_path)
    out = tmp_path / "corr.json"
    dec = DecodeStructuredLightInput.model_validate({
        "command": "decode_structured_light", "version": 1,
        "input_path": str(sl / "frames"), "sl_meta_path": str(sl / "sl_meta.json"),
        "output_path": str(out), "emit_debug_image": True})
    assert run_decode_structured_light(dec) == 0
    dbg = tmp_path / "corr.json.debug.png"
    assert dbg.is_file() and dbg.stat().st_size > 0
```

- [ ] **Step 2: Run it, expect FAIL** — `python-sidecar/.venv/bin/python -m pytest python-sidecar/tests/test_sl_decode.py -k "gray_bg_regression or bright_textured or moving_object or dim_dots or finds_id0 or roi_auto_vs_manual or emit_debug" -q` (expect failures: `corr["screen_roi"]` KeyError, no `.debug.png`, and naive-still-wired decode mis-counts on bright/dim backgrounds).

- [ ] **Step 3: Minimal implementation** — in `python-sidecar/src/lmt_vba_sidecar/sl_decode.py` rewrite the body of `run_decode_structured_light` from the `dot_radius_px` read through the corr write (current L117-169). Add `dot_radius_px` read, derive/override ROI, thread `roi` into Pass 2/3, seed via `_seed_dots`, read via `_read_bits_relative`, write the debug PNG, and stamp `screen_roi`:
```python
def run_decode_structured_light(cmd: DecodeStructuredLightInput) -> int:
    meta_path = pathlib.Path(cmd.sl_meta_path)
    meta = json.loads(meta_path.read_text())
    sl_meta_sha256 = hashlib.sha256(meta_path.read_bytes()).hexdigest()
    data_bits = int(meta["code"]["data_bits"])
    total_bits = int(meta["code"]["total_bits"])
    dot_radius_px = int(meta["dot_radius_px"])
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
        bits = _read_bits_relative(code_frames, x, y)
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
```
(Leave the trailing `ResultEvent` write at L171-175 unchanged — the adapter ignores it and reads the corr file.)

- [ ] **Step 4: Run tests, expect PASS** — `python-sidecar/.venv/bin/python -m pytest python-sidecar/tests/test_sl_decode.py -q` (ALL cases, including the pre-existing `test_roundtrip_*` and `test_correspondence_has_provenance` at L58-106, must pass — they exercise the auto-ROI default path).

- [ ] **Step 5: Commit** —
```bash
git add python-sidecar/src/lmt_vba_sidecar/sl_decode.py python-sidecar/tests/test_sl_decode.py
git commit -m "$(cat <<'EOF'
feat(sidecar): SL decode three-pass pipeline (靠闪不靠亮 + ROI) wired end to end

Pass1 temporal-range ROI, Pass2 ROI sentinel/plateau, Pass3 ROI Otsu seed +
relative bits. Stamps screen_roi into corr.json, writes <out>.debug.png.
S1 gray-bg regression + S2/S3/S4 robustness covered.

Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>
EOF
)"
```

---

### Task 6: Rebuild the vendored sidecar binary

**Files:**
- `python-sidecar/build_exe.sh` (run only; this produces `target/sidecar-vendor/darwin-arm64/lmt-vba-sidecar`)

- [ ] **Step 1: Run the full sidecar suite first** (confirm green before bundling) — `python-sidecar/.venv/bin/python -m pytest python-sidecar/tests -q`. Expect: all pass.

- [ ] **Step 2: Rebuild the binary** — `bash /Users/bip.lan/AIWorkspace/vp/led-mesh-toolkit/python-sidecar/build_exe.sh`. Expect final line `Built: .../target/sidecar-vendor/darwin-arm64/lmt-vba-sidecar`.

- [ ] **Step 3: Smoke-test the bundled binary decodes a freshly generated sequence** —
```bash
cd /Users/bip.lan/AIWorkspace/vp/led-mesh-toolkit && \
BIN=target/sidecar-vendor/darwin-arm64/lmt-vba-sidecar && \
T=$(mktemp -d) && \
printf '{"command":"generate_structured_light","version":1,"project":{"screen_id":"MAIN","cabinet_array":{"cols":1,"rows":1,"absent_cells":[],"cabinet_size_mm":[500,500]}},"output_dir":"%s/sl","screen_resolution":[960,540],"dot_spacing_px":160,"margin_px":80}' "$T" | "$BIN" generate_structured_light >/dev/null && \
printf '{"command":"decode_structured_light","version":1,"input_path":"%s/sl/frames","sl_meta_path":"%s/sl/sl_meta.json","output_path":"%s/corr.json","emit_debug_image":true}' "$T" "$T" "$T" | "$BIN" decode_structured_light >/dev/null && \
python3 -c "import json,sys; c=json.load(open('$T/corr.json')); assert c['screen_roi'] is not None and len(c['points'])>0; print('OK', len(c['points']), 'dots, roi', c['screen_roi'])" && \
test -s "$T/corr.json.debug.png" && echo "debug png OK"
```
Expect: `OK <N> dots, roi [...]` and `debug png OK`.

- [ ] **Step 4: (verification only — no separate test)** the smoke test in Step 3 is the gate.

- [ ] **Step 5: Commit** —
```bash
git add -A target/sidecar-vendor/darwin-arm64/lmt-vba-sidecar
git commit -m "$(cat <<'EOF'
build(sidecar): rebuild vendored binary with SL three-pass decode frontend

Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>
EOF
)"
```

---

### Task 7: CLI flags `--screen-roi` + `--emit-debug-image` (clap)

**Files:**
- `crates/lmt-cli/src/cli.rs` (`VisualCmd::DecodeStructuredLight` L350-363)

- [ ] **Step 1: Write the failing test** — append to `crates/lmt-cli/tests/cli_e2e.rs` (near the other decode tests ~L1900):
```rust
#[test]
fn decode_structured_light_help_lists_new_flags() {
    let assert = lmt()
        .args(["visual", "decode-structured-light", "--help"])
        .assert()
        .success();
    let out = String::from_utf8_lossy(&assert.get_output().stdout);
    assert!(out.contains("--screen-roi"), "help must document --screen-roi: {out}");
    assert!(out.contains("--emit-debug-image"), "help must document --emit-debug-image: {out}");
}
```

- [ ] **Step 2: Run it, expect FAIL** — `cargo test -p lmt-cli --test cli_e2e decode_structured_light_help_lists_new_flags` (expect FAIL: help lacks the flags).

- [ ] **Step 3: Minimal implementation** — in `crates/lmt-cli/src/cli.rs` edit the `DecodeStructuredLight` variant (after the `sentinel_threshold` field, before the closing `}` at L363):
```rust
        /// 全白哨兵帧判定阈值(整帧均值/255,范围 0–1)。不传=0.85。
        /// 屏幕没填满画面或背景非黑(如可视化器灰底)时调低(如 0.4)。
        #[arg(long)]
        sentinel_threshold: Option<f64>,
        /// 手动屏幕 ROI,格式 `X,Y,W,H`(像素)。不传=从全片时序活动图自动推导。
        /// 自动失败(只有屏外细长运动、无实心矩形)时用它兜底。
        #[arg(long)]
        screen_roi: Option<String>,
        /// 额外写出 `<out>.debug.png`:Pass 3 seed 的纯黑底+白点掩膜,供肉眼核对。
        #[arg(long)]
        emit_debug_image: bool,
    },
```

- [ ] **Step 4: Run tests, expect PASS** — `cargo test -p lmt-cli --test cli_e2e decode_structured_light_help_lists_new_flags`. (Note: this requires Task 8's handler change to compile — if the build errors on unused fields, do Task 8 in the same working state; the two tasks share one compile unit. Run after Task 8 lands.)

- [ ] **Step 5: Commit** — defer to Task 8 (same compile unit); commit both together.

---

### Task 8: `commands/visual.rs` handler — parse ROI (INVALID_INPUT before gate), thread flags, dry-run debug path

**Files:**
- `crates/lmt-cli/src/commands/visual.rs` (`run` dispatch L73-80; `decode_structured_light` L383-426)

- [ ] **Step 1: Write the failing test** — append to `crates/lmt-cli/tests/cli_e2e.rs`:
```rust
/// Bad --screen-roi format is rejected as invalid_input (exit 2) BEFORE the
/// destructive gate — mirrors reconstruct-structured-light's >=2-corr pre-check.
#[test]
fn decode_structured_light_invalid_roi_format() {
    let tmp = TempDir::new().unwrap();
    let meta = tmp.path().join("sl_meta.json");
    std::fs::write(&meta, "{}").unwrap();
    let assert = lmt().args(["--json", "visual", "decode-structured-light",
        tmp.path().to_str().unwrap(), meta.to_str().unwrap(),
        "--out", tmp.path().join("c.json").to_str().unwrap(),
        "--screen-roi", "10,20,oops"]).assert().failure();
    let out = assert.get_output();
    assert_eq!(out.status.code(), Some(2), "bad ROI must be exit 2");
    let env: Value = serde_json::from_str(std::str::from_utf8(&out.stderr).unwrap().trim_end()).unwrap();
    assert_eq!(env["error"]["code"], "invalid_input");
}

/// dry-run with --emit-debug-image lists BOTH the corr file and <out>.debug.png
/// under would_write, and writes nothing.
#[test]
fn decode_structured_light_with_roi_and_debug_dry_run() {
    let tmp = TempDir::new().unwrap();
    let meta = tmp.path().join("sl_meta.json");
    std::fs::write(&meta, "{}").unwrap();
    let out_path = tmp.path().join("c.json");
    let assert = lmt().args(["--json", "--dry-run", "visual", "decode-structured-light",
        tmp.path().to_str().unwrap(), meta.to_str().unwrap(),
        "--out", out_path.to_str().unwrap(),
        "--screen-roi", "10,20,300,200", "--emit-debug-image"]).assert().success();
    let env: Value = serde_json::from_slice(&assert.get_output().stdout).unwrap();
    assert_eq!(env["ok"], true);
    assert_eq!(env["data"]["dry_run"], true);
    let ww = env["data"]["would_write"].as_array().expect("would_write is a list");
    let joined: Vec<String> = ww.iter().map(|v| v.as_str().unwrap().to_string()).collect();
    assert!(joined.iter().any(|s| s.ends_with("c.json")), "lists corr: {joined:?}");
    assert!(joined.iter().any(|s| s.ends_with("c.json.debug.png")), "lists debug png: {joined:?}");
    assert!(!out_path.exists());
    assert!(!tmp.path().join("c.json.debug.png").exists());
}
```

- [ ] **Step 2: Run it, expect FAIL** — `cargo test -p lmt-cli --test cli_e2e -- decode_structured_light_invalid_roi_format decode_structured_light_with_roi_and_debug_dry_run` (expect compile error or FAIL: handler doesn't accept the new args / `would_write` is a bare string).

- [ ] **Step 3: Minimal implementation** — in `crates/lmt-cli/src/commands/visual.rs`:

(a) Update the dispatch arm (L73-80):
```rust
        VisualCmd::DecodeStructuredLight {
            input_path,
            sl_meta,
            out,
            sentinel_threshold,
            screen_roi,
            emit_debug_image,
        } => decode_structured_light(
            mode, &input_path, &sl_meta, &out, sentinel_threshold,
            screen_roi.as_deref(), emit_debug_image, yes, dry_run,
        ),
```

(b) Replace the `decode_structured_light` fn (L383-426). Add a ROI parser, validate before the gate, build `would_write` as a list, and thread the parsed values:
```rust
/// Parse a `X,Y,W,H` ROI string into four u32. Returns None on any malformed
/// part (mapped by the caller to INVALID_INPUT before the destructive gate).
fn parse_screen_roi(s: &str) -> Option<[u32; 4]> {
    let parts: Vec<&str> = s.split(',').collect();
    if parts.len() != 4 {
        return None;
    }
    let mut out = [0u32; 4];
    for (i, p) in parts.iter().enumerate() {
        out[i] = p.trim().parse::<u32>().ok()?;
    }
    Some(out)
}

#[allow(clippy::too_many_arguments)]
fn decode_structured_light(
    mode: Mode,
    input_path: &str,
    sl_meta: &str,
    out: &str,
    sentinel_threshold: Option<f64>,
    screen_roi: Option<&str>,
    emit_debug_image: bool,
    yes: bool,
    dry_run: bool,
) -> i32 {
    // Validate ROI format BEFORE the destructive gate, so --dry-run does not
    // falsely report success for a command that would always fail on execute
    // (mirrors reconstruct-structured-light's >=2-corr pre-check).
    let roi: Option<[u32; 4]> = match screen_roi {
        Some(s) => match parse_screen_roi(s) {
            Some(r) => Some(r),
            None => {
                return output::err(
                    mode,
                    ApiError::new(
                        error_codes::INVALID_INPUT,
                        "--screen-roi must be four comma-separated non-negative integers: X,Y,W,H",
                    ),
                );
            }
        },
        None => None,
    };

    let decision = match util::gate_destructive(yes, dry_run, "visual decode-structured-light") {
        Ok(d) => d,
        Err(e) => return output::err(mode, e),
    };

    match decision {
        DestructiveDecision::DryRun => {
            let mut would_write = vec![out.to_string()];
            if emit_debug_image {
                would_write.push(format!("{out}.debug.png"));
            }
            let payload = serde_json::json!({
                "dry_run": true,
                "would_write": would_write,
            });
            output::ok(mode, payload, |_| {
                let _ = writeln!(std::io::stdout(), "[dry-run] would decode → {out}");
            })
        }
        DestructiveDecision::Execute => {
            match lmt_app::visual::run_decode_structured_light(
                Path::new(input_path),
                Path::new(sl_meta),
                Path::new(out),
                sentinel_threshold,
                roi,
                emit_debug_image,
            ) {
                Ok(r) => output::ok(mode, r, |p| {
                    let _ = writeln!(
                        std::io::stdout(),
                        "decoded {} dots → {}",
                        p.n_dots_decoded,
                        p.output_path
                    );
                }),
                Err(e) => output::err(mode, ApiError::from(e)),
            }
        }
    }
}
```

- [ ] **Step 4: Run tests, expect PASS** — `cargo test -p lmt-cli --test cli_e2e -- decode_structured_light_invalid_roi_format decode_structured_light_with_roi_and_debug_dry_run decode_structured_light_help_lists_new_flags decode_structured_light_dry_run_writes_nothing decode_structured_light_refuses_without_yes` (all pass; this also covers Task 7's help test and confirms the pre-existing decode dry-run/refuse tests still pass — note `decode_structured_light_dry_run_writes_nothing` asserts `env["data"]["dry_run"]==true`, which still holds since `would_write` is now a list).

- [ ] **Step 5: Commit** (Tasks 7+8 together) —
```bash
git add crates/lmt-cli/src/cli.rs crates/lmt-cli/src/commands/visual.rs crates/lmt-cli/tests/cli_e2e.rs
git commit -m "$(cat <<'EOF'
feat(cli): decode-structured-light --screen-roi (validated pre-gate) + --emit-debug-image

ROI string parses to INVALID_INPUT(2) before the destructive gate; dry-run
would_write lists <out>.debug.png when --emit-debug-image.

Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>
EOF
)"
```

---

### Task 9: Thread `screen_roi` + `emit_debug_image` through lmt-app and the adapter IPC

**Files:**
- `crates/lmt-app/src/visual.rs` (`run_decode_structured_light` L537-559)
- `crates/adapter-visual-ba/src/api.rs` (`DecodeStructuredLightArgs` L480-490; `decode_structured_light` IPC builder L498-511)
- `crates/adapter-visual-ba/src/ipc.rs` (`CorrespondenceFile` L282-288)

- [ ] **Step 1: Write the failing test** — add a unit test to `crates/adapter-visual-ba/src/ipc.rs` (mirrors the file's mirror-struct intent; add at the bottom of the file inside a `#[cfg(test)] mod tests`):
```rust
#[cfg(test)]
mod corr_roi_tests {
    use super::CorrespondenceFile;

    #[test]
    fn correspondence_file_screen_roi_optional() {
        // Old corr.json without screen_roi still deserializes.
        let old = r#"{"schema_version":1,"screen_id":"MAIN","sl_meta_sha256":"x","points":[]}"#;
        let f: CorrespondenceFile = serde_json::from_str(old).unwrap();
        assert!(f.screen_roi.is_none());
        // New corr.json carries the used ROI.
        let new = r#"{"schema_version":1,"screen_id":"MAIN","sl_meta_sha256":"x","screen_roi":[1,2,3,4],"points":[]}"#;
        let f2: CorrespondenceFile = serde_json::from_str(new).unwrap();
        assert_eq!(f2.screen_roi, Some([1, 2, 3, 4]));
    }
}
```

- [ ] **Step 2: Run it, expect FAIL** — `cargo test -p adapter-visual-ba corr_roi_tests` (expect compile error: `CorrespondenceFile` has no `screen_roi` field).

- [ ] **Step 3: Minimal implementation** —

(a) `crates/adapter-visual-ba/src/ipc.rs` — add the field to `CorrespondenceFile` (L282-288):
```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CorrespondenceFile {
    pub schema_version: u32,
    pub screen_id: String,
    pub sl_meta_sha256: String,
    /// Detection provenance: the screen ROI actually used (mirrors Python).
    /// Optional so older corr.json without it still deserialize.
    #[serde(default)]
    pub screen_roi: Option<[u32; 4]>,
    pub points: Vec<CorrespondencePoint>,
}
```

(b) `crates/adapter-visual-ba/src/api.rs` — add the two fields to `DecodeStructuredLightArgs` (after `sentinel_threshold`, L487):
```rust
    pub sentinel_threshold: Option<f64>,
    /// None = sidecar auto-derives the ROI from the temporal-activity map.
    pub screen_roi: Option<[u32; 4]>,
    /// Write <output_path>.debug.png (Pass-3 seed mask) for eyeball QA.
    pub emit_debug_image: bool,
    pub progress_tx: Option<mpsc::Sender<Event>>,
    pub cancel: Option<oneshot::Receiver<()>>,
```
and inject conditionally in the IPC builder (after the `sentinel_threshold` block, L508-511):
```rust
    // Omit when None so the sidecar uses its default sentinel_threshold (0.85).
    if let Some(t) = args.sentinel_threshold {
        payload["sentinel_threshold"] = json!(t);
    }
    // ROI is sent only when manually overridden; otherwise the sidecar auto-derives.
    if let Some(roi) = args.screen_roi {
        payload["screen_roi"] = json!(roi);
    }
    // emit_debug_image defaults false on the sidecar; only send when explicitly on.
    if args.emit_debug_image {
        payload["emit_debug_image"] = json!(true);
    }
```

(c) `crates/lmt-app/src/visual.rs` — extend `run_decode_structured_light` (L537-559):
```rust
pub fn run_decode_structured_light(
    input_path: &Path,
    sl_meta_path: &Path,
    output_path: &Path,
    // None = sidecar default (0.85). Lower for non-black / partially-filled frames.
    sentinel_threshold: Option<f64>,
    // None = sidecar auto-derives the screen ROI from the temporal-activity map.
    screen_roi: Option<[u32; 4]>,
    // When true the sidecar also writes <output_path>.debug.png.
    emit_debug_image: bool,
) -> LmtResult<DecodeStructuredLightResult> {
    let args = DecodeStructuredLightArgs {
        input_path: input_path.display().to_string(),
        sl_meta_path: sl_meta_path.display().to_string(),
        output_path: output_path.display().to_string(),
        sentinel_threshold,
        screen_roi,
        emit_debug_image,
        progress_tx: None,
        cancel: None,
    };

    let out = rt()?.block_on(decode_structured_light(args)).map_err(map_vba_err)?;

    Ok(DecodeStructuredLightResult {
        output_path: out.output_path,
        n_dots_decoded: out.n_dots_decoded as usize,
    })
}
```

- [ ] **Step 4: Run tests, expect PASS** — `cargo test -p adapter-visual-ba corr_roi_tests && cargo build -p lmt-cli` (the lmt-cli build now compiles against the 6-arg `run_decode_structured_light` it already calls in Task 8).

- [ ] **Step 5: Commit** —
```bash
git add crates/lmt-app/src/visual.rs crates/adapter-visual-ba/src/api.rs crates/adapter-visual-ba/src/ipc.rs
git commit -m "$(cat <<'EOF'
feat(visual): thread screen_roi + emit_debug_image through lmt-app + adapter IPC

CorrespondenceFile mirror gains optional screen_roi (detection provenance).
ROI/debug flag injected into decode IPC JSON only when set.

Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>
EOF
)"
```

---

### Task 10: Real-sidecar decode happy-path E2E (gray-bg, S1 at the CLI seam)

**Files:**
- `crates/lmt-cli/tests/cli_e2e.rs` (uses `make_sidecar_wrapper` L903; mirrors `generate_structured_light_emits_tiff_seq` L1838)

- [ ] **Step 1: Write the failing test** — append to `crates/lmt-cli/tests/cli_e2e.rs`:
```rust
/// decode-structured-light happy path through the REAL sidecar: generate a
/// gray-background sequence, decode it, assert the envelope + corr.json carries
/// screen_roi provenance and the debug png lands at <out>.debug.png. This is the
/// CLI-seam check that the three-pass frontend still decodes the gray-bg material
/// (S1) end to end (per-pixel decode coverage is unit-tested in the sidecar).
#[cfg(unix)]
#[test]
fn decode_structured_light_happy_with_roi_provenance_and_debug() {
    let tmp = TempDir::new().unwrap();
    let wrapper = match make_sidecar_wrapper(tmp.path()) {
        Some(w) => w,
        None => {
            eprintln!("skipping decode_structured_light_happy: python-sidecar venv not found");
            return;
        }
    };
    let proj = tmp.path().join("proj");
    write_gp_project(&proj, 1, 1);
    // Generate the SL sequence (frames + sl_meta.json) via the real sidecar.
    lmt()
        .env("LMT_VBA_SIDECAR_PATH", &wrapper)
        .args(["--json", "--yes", "visual", "generate-structured-light",
            proj.to_str().unwrap(), "MAIN"])
        .assert()
        .success();
    let sl_dir = proj.join("patterns/MAIN/sl");
    let frames = sl_dir.join("frames");
    let meta = sl_dir.join("sl_meta.json");
    let out = tmp.path().join("corr.json");

    let assert = lmt()
        .env("LMT_VBA_SIDECAR_PATH", &wrapper)
        .args(["--json", "--yes", "visual", "decode-structured-light",
            frames.to_str().unwrap(), meta.to_str().unwrap(),
            "--out", out.to_str().unwrap(), "--emit-debug-image"])
        .assert()
        .success();
    let env: Value = serde_json::from_slice(&assert.get_output().stdout).unwrap();
    assert_eq!(env["ok"], true, "envelope ok: {env}");
    assert!(env["data"]["n_dots_decoded"].as_u64().unwrap() > 0);

    let corr: Value = serde_json::from_str(&std::fs::read_to_string(&out).unwrap()).unwrap();
    assert!(corr["screen_roi"].is_array(), "corr.json must stamp screen_roi: {corr}");
    assert!(out.with_extension("json.debug.png").is_file()
        || std::path::Path::new(&format!("{}.debug.png", out.display())).is_file(),
        "<out>.debug.png must exist");
}
```

- [ ] **Step 2: Run it, expect FAIL/SKIP→PASS** — `cargo test -p lmt-cli --test cli_e2e decode_structured_light_happy_with_roi_provenance_and_debug -- --nocapture`. Before Task 9's rebuild + this test compiles it should FAIL to compile; once the workspace builds and the venv is present it must PASS (or print the skip line if the venv is absent — acceptable, like the sibling generate test).

- [ ] **Step 3: Minimal implementation** — none beyond the test (the path is wired in Tasks 1–9). If the test reveals a real defect, fix it in the relevant Task's file (do not add new logic in the test).

- [ ] **Step 4: Run tests, expect PASS** — `cargo test -p lmt-cli --test cli_e2e -- decode_structured_light` (all decode E2E cases pass; the happy test passes when the sidecar venv exists).

- [ ] **Step 5: Commit** —
```bash
git add crates/lmt-cli/tests/cli_e2e.rs
git commit -m "$(cat <<'EOF'
test(cli): decode-structured-light real-sidecar happy E2E (gray-bg S1, ROI provenance, debug png)

Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>
EOF
)"
```

---

### Task 11: Update manifest CLI string + agents-cli.md

**Files:**
- `crates/lmt-shared/src/manifest.rs` (decode op L128-129; exit_codes stay `[0,2,3,4,13,18]`)
- `docs/agents-cli.md` (decode row L44)

- [ ] **Step 1: Write the failing test** — append to `crates/lmt-cli/tests/cli_e2e.rs`:
```rust
/// The manifest's decode-structured-light CLI string documents the new flags and
/// keeps the exit-code set unchanged (no new error codes per spec A.3).
#[test]
fn decode_structured_light_manifest_documents_new_flags() {
    let assert = lmt().args(["--json", "schema"]).assert().success();
    let env: Value = serde_json::from_slice(&assert.get_output().stdout).unwrap();
    let ops = env["data"]["operations"].as_array()
        .or_else(|| env["data"]["manifest"]["operations"].as_array())
        .expect("schema envelope must list operations");
    let decode = ops.iter()
        .find(|o| o["name"] == "visual.decode_structured_light")
        .expect("decode op present in manifest");
    let cli = decode["cli"].as_str().unwrap();
    assert!(cli.contains("--screen-roi"), "manifest CLI must mention --screen-roi: {cli}");
    assert!(cli.contains("--emit-debug-image"), "manifest CLI must mention --emit-debug-image: {cli}");
    let codes: Vec<i64> = decode["exit_codes"].as_array().unwrap()
        .iter().map(|c| c.as_i64().unwrap()).collect();
    assert_eq!(codes, vec![0, 2, 3, 4, 13, 18], "exit codes unchanged");
}
```
(Note: if `schema` envelope field names differ, adjust the `ops`/`cli`/`exit_codes` key lookups to match the actual `schema` JSON before running — inspect with `./target/debug/lmt --json schema | jq '.data | keys'`.)

- [ ] **Step 2: Run it, expect FAIL** — `cargo test -p lmt-cli --test cli_e2e decode_structured_light_manifest_documents_new_flags` (expect FAIL: CLI string lacks the flags).

- [ ] **Step 3: Minimal implementation** —

(a) `crates/lmt-shared/src/manifest.rs` — edit the decode op (L128-129). Update the description and CLI string; keep `&[0, 2, 3, 4, 13, 18]`:
```rust
        op("visual.decode_structured_light", "Decode a recorded structured-light capture (video or frame dir) into a provenance-stamped screen<->camera correspondence file. Three-pass temporal frontend (decide by CHANGE, not brightness): Pass1 per-pixel temporal range -> auto screen ROI (--screen-roi X,Y,W,H overrides; auto failure -> detection_failed); Pass2 ROI-restricted sentinel + plateau indexing; Pass3 ROI Otsu seeding (recovers id=0) + shape filter + per-dot relative bit reading + parity gate. corr.json records the used screen_roi (provenance). --emit-debug-image also writes <out>.debug.png (Pass-3 seed mask). --sentinel-threshold (default 0.85) now applies to the ROI mean. Works on any-brightness textured static backgrounds with off-screen movers. detection_failed(13) if ROI auto-derive fails / too few dots decode; decode_failed(18) if sentinels/plateaus don't parse",
           "lmt visual decode-structured-light <input> <sl_meta> --out <corr.json> [--sentinel-threshold F] [--screen-roi X,Y,W,H] [--emit-debug-image]", Destructive, true, false, false, Some("DecodeStructuredLightResult"), &[0, 2, 3, 4, 13, 18]),
```

(b) `docs/agents-cli.md` — replace the decode row (L44) signature and description:
```
| `lmt visual decode-structured-light <input> <sl_meta> --out <corr.json> [--sentinel-threshold F] [--screen-roi X,Y,W,H] [--emit-debug-image]` | destructive | Decode a recorded structured-light capture (video or frame directory) into a provenance-stamped screen↔camera correspondence file (`screen_id`, `sl_meta_sha256`, `camera_image_size`, `source_input`, `screen_roi`, points). Three-pass temporal frontend that decides by **change, not brightness** so it works on any-brightness textured static backgrounds with off-screen moving objects: Pass 1 per-pixel temporal range (max−min) → auto screen ROI (largest solid activity rectangle; `--screen-roi X,Y,W,H` overrides; auto-derive failure → `detection_failed`); Pass 2 ROI-restricted white-sentinel mean + plateau indexing; Pass 3 ROI Otsu seeding (recovers the all-off `id=0`) + dot shape/size filter + per-dot relative (own min/max) bit reading + binary+parity decode gate. `corr.json` records the `screen_roi` actually used (detection provenance; `reconstruct-structured-light` ignores it). `--emit-debug-image` additionally writes `<out>.debug.png` (a black-background white-dot seed mask) for eyeball QA. `--sentinel-threshold` (default 0.85) now applies to the ROI-region mean. `decode_failed` (18) if sentinels/plateaus don't parse; `detection_failed` (13) if the ROI can't be auto-derived or too few dots decode. |
```

- [ ] **Step 4: Run tests, expect PASS** — `cargo test -p lmt-cli --test cli_e2e decode_structured_light_manifest_documents_new_flags` and verify the schema dump: `cargo build -p lmt-cli && ./target/debug/lmt --json schema | jq '.data | .. | objects | select(.name? == "visual.decode_structured_light") | {cli, exit_codes}'`.

- [ ] **Step 5: Commit** —
```bash
git add crates/lmt-shared/src/manifest.rs docs/agents-cli.md crates/lmt-cli/tests/cli_e2e.rs
git commit -m "$(cat <<'EOF'
docs(cli): document decode-structured-light --screen-roi / --emit-debug-image + three-pass frontend

manifest CLI string + agents-cli.md row; exit codes unchanged [0,2,3,4,13,18].

Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>
EOF
)"
```

---

### Task 12: Full workspace self-check (CLAUDE.md contract gate)

**Files:** none (verification only).

- [ ] **Step 1: Full sidecar suite** — `python-sidecar/.venv/bin/python -m pytest python-sidecar/tests -q`. Expect: all pass (including S1 regression `test_decode_gray_bg_regression` and the pre-existing `test_roundtrip_*`).

- [ ] **Step 2: Full Rust workspace tests** — `cargo test --workspace`. Then run the venv-gated decode E2E explicitly: `LMT_VBA_SIDECAR_PATH=$(pwd)/python-sidecar/.venv/bin/lmt-vba-sidecar cargo test -p lmt-cli --test cli_e2e -- decode_structured_light`. Expect: all pass (or skip if the wrapper venv is absent, matching sibling tests).

- [ ] **Step 3: Schema dump sanity** — `./target/debug/lmt --json schema | jq '.ok'` returns `true`; confirm `DecodeStructuredLightResult` is still present and unchanged (the DTO did NOT gain `screen_roi`/debug fields — those live on `CorrespondenceFile`/disk per spec A.3): `./target/debug/lmt --json schema | jq '.data | .. | objects | select(.name? == "DecodeStructuredLightResult")'` shows only `output_path` + `n_dots_decoded`.

- [ ] **Step 4: Help + flag wiring** — `./target/debug/lmt visual decode-structured-light --help` lists `--screen-roi` and `--emit-debug-image`; `./target/debug/lmt --help` still registers the subcommand.

- [ ] **Step 5: Commit** — nothing to commit (verification gate). If any step fails, return to the owning task; do not paper over with a new commit here.
