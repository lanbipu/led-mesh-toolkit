# SL Decode — disguise 10-bit DPX Input Support — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Let `decode-structured-light` ingest a disguise feed export (`<name>.seq/` directory of 10-bit `.dpx` frames) directly, with no manual transcode.

**Architecture:** Add a pure-numpy SMPTE-268M DPX reader (`dpx.py::read_dpx_gray8`) scoped to disguise's exact variant (LE, 1920×1080-agnostic, RGB descriptor 50, 10-bit, Method-A packing, no RLE), downscale to 8-bit grayscale, and dispatch `.dpx` files inside `load_frames`. Downstream decode is unchanged (still uint8). The byte layout and unpack formula are verified pixel-exact against ffmpeg ground truth on a real disguise sample (`R=(w>>22)&0x3FF, G=(w>>12)&0x3FF, B=(w>>2)&0x3FF`, 2 pad bits at LSB).

**Tech Stack:** Python (numpy, opencv-contrib-python), Rust (clap CLI E2E via `assert_cmd`). No new runtime dependency; no system ffmpeg at runtime.

**Spec:** `docs/superpowers/specs/2026-05-30-sl-dpx-input-design.md`

---

## File Structure

| File | Change | Responsibility |
| --- | --- | --- |
| `python-sidecar/src/lmt_vba_sidecar/dpx.py` | **Create** | `read_dpx_gray8(path) -> (H,W) uint8`. Pure-numpy DPX parser, variant-guarded. |
| `python-sidecar/src/lmt_vba_sidecar/sl_decode.py` | Modify (26-42, 226) | Dispatch `.dpx` in `load_frames`; wrap load failure into a clean `decode_failed` envelope. |
| `python-sidecar/src/lmt_vba_sidecar/ipc.py` | Modify (225 comment) | DTO docstring mentions `.dpx`. |
| `python-sidecar/tests/_dpx_fixtures.py` | **Create** | Single source of truth for writing test DPX bytes; CLI converter `frames/ -> .dpx/`. Used by Python + Rust tests. |
| `python-sidecar/tests/test_dpx.py` | **Create** | Parser unit tests (round-trip, both endians, variant-guard raises, truncation, fixture-vs-real-sample header check). |
| `python-sidecar/tests/test_sl_decode.py` | Modify (append) | One end-to-end test decoding a `.dpx` frame directory. |
| `crates/lmt-cli/tests/cli_e2e.rs` | Modify (append) | `decode_structured_light_accepts_dpx_dir` E2E. |
| `docs/agents-cli.md` | Modify (line 44) | Note `.dpx` directory input. |

No new CLI subcommand, no new DTO field, no new error/exit code. CLI-contract "new command" checklist does not apply (input-format extension to an existing command).

### Test harness facts (verified)

- `python-sidecar/pyproject.toml` `[tool.pytest.ini_options]`: `pythonpath = ["src"]`, `testpaths = ["tests"]`, **default prepend import-mode** (no `import-mode` override), and `tests/` has no `__init__.py`. → A sibling helper `tests/_dpx_fixtures.py` is importable from any `tests/test_*.py` via `import _dpx_fixtures`. **No `conftest.py` is needed** (none exists today; do not add one).
- That same config sets `filterwarnings = ["error::DeprecationWarning", "error::PendingDeprecationWarning"]` → **any DeprecationWarning fails the test.** All code in this plan uses current APIs (`np.random.default_rng`, `np.frombuffer`, `np.stack`, `cv2.cvtColor`, `struct.unpack_from`) and triggers none — keep it that way if you adjust anything.

---

## Task 0: Feature branch

- [ ] **Step 1: Branch off main** (repo is on the default branch; never commit feature work to `main`)

Run:
```bash
cd /Users/bip.lan/AIWorkspace/vp/led-mesh-toolkit
git checkout -b feat/sl-dpx-input
git status
```
Expected: `On branch feat/sl-dpx-input`. The untracked `docs/superpowers/specs/2026-05-30-sl-dpx-input-design.md` and `docs/superpowers/plans/2026-05-30-sl-dpx-input.md` carry over.

- [ ] **Step 2: Commit the spec + plan**

```bash
git add docs/superpowers/specs/2026-05-30-sl-dpx-input-design.md docs/superpowers/plans/2026-05-30-sl-dpx-input.md
git commit -m "docs(sl): DPX input spec + plan"
```

---

## Task 1: Test DPX fixture writer (single source of truth)

This module defines the verified disguise DPX byte layout **once** so both the Python and Rust tests synthesize identical, real-shaped DPX without an 8 MB committed binary. It is test-only (lives under `tests/`, never imported by shipped code) and standalone (numpy + cv2 only, no `lmt_vba_sidecar` import) so the Rust test can run it as a script.

**Files:**
- Create: `python-sidecar/tests/_dpx_fixtures.py`

- [ ] **Step 1: Create the fixture writer**

```python
"""Test-only: synthesize disguise-style 10-bit Method-A DPX frames.

Single source of truth for the DPX byte layout exercised by the SL DPX tests
(verified against a real disguise sample: LE magic XPDS, data offset 8192,
descriptor 50=RGB, bit depth 10, packing 1=Method A, R/G/B at bits 22/12/2,
2 pad bits at LSB). NOT part of the shipped package. Standalone: numpy + cv2
only (no lmt_vba_sidecar import) so it also runs as a CLI converter.
"""
from __future__ import annotations

import struct
import sys
from pathlib import Path

import cv2
import numpy as np

DPX_HEADER_SIZE = 8192  # disguise: pixel data starts at 0x2000

_SRC_EXTS = (".png", ".jpg", ".jpeg", ".bmp", ".tif", ".tiff")


def write_dpx(path, gray_u8, *, endian: str = "<") -> None:
    """Write an (H, W) uint8 grayscale image as a 10-bit RGB Method-A DPX
    (R=G=B=gray<<2). `endian` is "<" (LE, disguise default) or ">" (BE)."""
    assert endian in ("<", ">"), endian
    g = np.asarray(gray_u8)
    assert g.dtype == np.uint8 and g.ndim == 2, "gray_u8 must be (H, W) uint8"
    h, w = int(g.shape[0]), int(g.shape[1])

    v10 = g.astype(np.uint32) << 2  # 8-bit -> top 8 of 10 bits
    word = (v10 << 22) | (v10 << 12) | (v10 << 2)  # R/G/B slots, pad bits = LSB 0
    pixels = word.astype(endian + "u4").tobytes()

    hdr = bytearray(DPX_HEADER_SIZE)
    hdr[0:4] = b"XPDS" if endian == "<" else b"SDPX"
    struct.pack_into(endian + "I", hdr, 4, DPX_HEADER_SIZE)              # image data offset
    hdr[8:12] = b"V1.0"
    struct.pack_into(endian + "I", hdr, 24, DPX_HEADER_SIZE + len(pixels))  # total size (honest)
    struct.pack_into(endian + "H", hdr, 768, 0)                         # orientation
    struct.pack_into(endian + "H", hdr, 770, 1)                         # number of image elements
    struct.pack_into(endian + "I", hdr, 772, w)                         # PixelsPerLine
    struct.pack_into(endian + "I", hdr, 776, h)                         # LinesPerElement
    hdr[800] = 50                                                       # descriptor = RGB
    hdr[803] = 10                                                       # bit depth
    struct.pack_into(endian + "H", hdr, 804, 1)                         # packing = Method A
    struct.pack_into(endian + "H", hdr, 806, 0)                         # encoding = none
    struct.pack_into(endian + "I", hdr, 808, DPX_HEADER_SIZE)           # element data offset

    Path(path).write_bytes(bytes(hdr) + pixels)


def convert_dir_to_dpx(src_dir, dst_dir) -> int:
    """Read every image in src_dir (sorted) as grayscale and write a matching
    frameNNNN.dpx into dst_dir. Returns the count written."""
    src, dst = Path(src_dir), Path(dst_dir)
    dst.mkdir(parents=True, exist_ok=True)
    files = sorted(f for f in src.iterdir() if f.suffix.lower() in _SRC_EXTS)
    for i, f in enumerate(files):
        g = cv2.imread(str(f), cv2.IMREAD_GRAYSCALE)
        if g is None:
            raise ValueError(f"could not read {f}")
        write_dpx(dst / f"frame{i:04d}.dpx", g)
    return len(files)


if __name__ == "__main__":
    n = convert_dir_to_dpx(sys.argv[1], sys.argv[2])
    print(n)
```

- [ ] **Step 2: Commit**

```bash
git add python-sidecar/tests/_dpx_fixtures.py
git commit -m "test(sl): DPX fixture writer (single source of truth)"
```

---

## Task 2: DPX reader `read_dpx_gray8`

**Files:**
- Create: `python-sidecar/src/lmt_vba_sidecar/dpx.py`
- Test: `python-sidecar/tests/test_dpx.py`

- [ ] **Step 1: Write the failing tests**

Create `python-sidecar/tests/test_dpx.py`:

```python
import struct

import numpy as np
import pytest

from _dpx_fixtures import write_dpx, DPX_HEADER_SIZE
from lmt_vba_sidecar.dpx import read_dpx_gray8


def test_roundtrip_recovers_grayscale_le(tmp_path):
    rng = np.random.default_rng(0)
    g = rng.integers(0, 256, size=(7, 11), dtype=np.uint8)
    p = tmp_path / "f.dpx"
    write_dpx(p, g)
    out = read_dpx_gray8(p)
    assert out.dtype == np.uint8 and out.shape == (7, 11)
    np.testing.assert_array_equal(out, g)


def test_roundtrip_big_endian(tmp_path):
    g = np.array([[0, 64, 128], [192, 255, 1]], np.uint8)
    p = tmp_path / "be.dpx"
    write_dpx(p, g, endian=">")
    np.testing.assert_array_equal(read_dpx_gray8(p), g)


def test_fixture_header_matches_real_disguise_layout(tmp_path):
    # Pin the fixture writer to the verified real-sample offsets so the tests
    # below decode disguise-shaped bytes, not an ad-hoc format.
    p = tmp_path / "h.dpx"
    write_dpx(p, np.zeros((4, 5), np.uint8))
    raw = p.read_bytes()
    assert raw[:4] == b"XPDS"
    assert struct.unpack_from("<I", raw, 4)[0] == DPX_HEADER_SIZE == 8192
    assert struct.unpack_from("<I", raw, 772)[0] == 5   # width
    assert struct.unpack_from("<I", raw, 776)[0] == 4   # height
    assert raw[800] == 50
    assert raw[803] == 10
    assert struct.unpack_from("<H", raw, 804)[0] == 1
    assert struct.unpack_from("<H", raw, 806)[0] == 0


def _make_bad(tmp_path, mutate):
    g = np.zeros((4, 5), np.uint8)
    p = tmp_path / "bad.dpx"
    write_dpx(p, g)
    raw = bytearray(p.read_bytes())
    mutate(raw)
    p.write_bytes(bytes(raw))
    return p


def test_raises_on_bad_magic(tmp_path):
    p = _make_bad(tmp_path, lambda r: r.__setitem__(slice(0, 4), b"FAKE"))
    with pytest.raises(ValueError, match="not a DPX"):
        read_dpx_gray8(p)


def test_raises_on_unsupported_bit_depth(tmp_path):
    p = _make_bad(tmp_path, lambda r: r.__setitem__(803, 12))
    with pytest.raises(ValueError, match="bit depth"):
        read_dpx_gray8(p)


def test_raises_on_unsupported_descriptor(tmp_path):
    p = _make_bad(tmp_path, lambda r: r.__setitem__(800, 6))  # 6 = luma
    with pytest.raises(ValueError, match="descriptor"):
        read_dpx_gray8(p)


def test_raises_on_unsupported_packing(tmp_path):
    p = _make_bad(tmp_path, lambda r: struct.pack_into("<H", r, 804, 0))
    with pytest.raises(ValueError, match="packing"):
        read_dpx_gray8(p)


def test_raises_on_rle_encoding(tmp_path):
    p = _make_bad(tmp_path, lambda r: struct.pack_into("<H", r, 806, 1))
    with pytest.raises(ValueError, match="RLE"):
        read_dpx_gray8(p)


def test_raises_on_truncated_pixels(tmp_path):
    g = np.zeros((4, 5), np.uint8)
    p = tmp_path / "trunc.dpx"
    write_dpx(p, g)
    raw = p.read_bytes()
    p.write_bytes(raw[:-8])  # drop two pixel words
    with pytest.raises(ValueError, match="truncated"):
        read_dpx_gray8(p)
```

- [ ] **Step 2: Run tests to verify they fail**

Run:
```bash
cd /Users/bip.lan/AIWorkspace/vp/led-mesh-toolkit/python-sidecar
.venv/bin/python -m pytest tests/test_dpx.py -v
```
Expected: collection/import error — `ModuleNotFoundError: No module named 'lmt_vba_sidecar.dpx'`.

- [ ] **Step 3: Write the reader**

Create `python-sidecar/src/lmt_vba_sidecar/dpx.py`:

```python
"""Read disguise 10-bit Method-A DPX frames -> 8-bit grayscale.

Pure numpy + cv2 (no extra deps, PyInstaller-safe, no runtime ffmpeg). Scoped to
the disguise variant verified against a real sample; raises ValueError on anything
else rather than silently misdecoding. Unpack formula verified pixel-exact vs
ffmpeg: R=(w>>22)&0x3FF, G=(w>>12)&0x3FF, B=(w>>2)&0x3FF (2 pad bits at LSB).
"""
from __future__ import annotations

import struct
from pathlib import Path

import cv2
import numpy as np

_DESCRIPTOR_RGB = 50
_PACKING_METHOD_A = 1
_ENCODING_NONE = 0
_BIT_DEPTH = 10
_MIN_HEADER = 812  # last field we read is element data offset at 808..812


def read_dpx_gray8(path) -> np.ndarray:
    """Return an (H, W) uint8 grayscale frame from a disguise 10-bit RGB Method-A
    DPX. Raises ValueError on any non-disguise variant or truncation."""
    raw = Path(path).read_bytes()
    if len(raw) < _MIN_HEADER:
        raise ValueError(f"{path}: file too small to be a DPX ({len(raw)} bytes)")

    magic = raw[:4]
    if magic == b"XPDS":
        end = "<"
    elif magic == b"SDPX":
        end = ">"
    else:
        raise ValueError(f"{path}: not a DPX (magic {magic!r})")

    data_off = struct.unpack_from(end + "I", raw, 4)[0]
    width = struct.unpack_from(end + "I", raw, 772)[0]
    height = struct.unpack_from(end + "I", raw, 776)[0]
    descriptor = raw[800]
    bit_depth = raw[803]
    packing = struct.unpack_from(end + "H", raw, 804)[0]
    encoding = struct.unpack_from(end + "H", raw, 806)[0]

    if bit_depth != _BIT_DEPTH:
        raise ValueError(f"{path}: unsupported DPX bit depth {bit_depth} (only 10 supported)")
    if descriptor != _DESCRIPTOR_RGB:
        raise ValueError(f"{path}: unsupported DPX descriptor {descriptor} (only 50=RGB supported)")
    if packing != _PACKING_METHOD_A:
        raise ValueError(f"{path}: unsupported DPX packing {packing} (only 1=Method A supported)")
    if encoding != _ENCODING_NONE:
        raise ValueError(f"{path}: RLE-encoded DPX not supported (encoding={encoding})")
    if width == 0 or height == 0:
        raise ValueError(f"{path}: bad DPX dimensions {width}x{height}")

    need = width * height * 4  # Method A: one 32-bit word per RGB pixel
    if len(raw) < data_off + need:
        raise ValueError(
            f"{path}: truncated DPX pixel data (need {data_off + need} bytes, have {len(raw)})"
        )

    words = np.frombuffer(raw[data_off:data_off + need], dtype=end + "u4").reshape(height, width)
    r8 = (((words >> 22) & 0x3FF) >> 2).astype(np.uint8)  # 10-bit -> 8-bit
    g8 = (((words >> 12) & 0x3FF) >> 2).astype(np.uint8)
    b8 = (((words >> 2) & 0x3FF) >> 2).astype(np.uint8)
    rgb8 = np.stack([r8, g8, b8], axis=-1)
    return cv2.cvtColor(rgb8, cv2.COLOR_RGB2GRAY)  # BT.601 luma, matches IMREAD_GRAYSCALE
```

- [ ] **Step 4: Run tests to verify they pass**

Run:
```bash
cd /Users/bip.lan/AIWorkspace/vp/led-mesh-toolkit/python-sidecar
.venv/bin/python -m pytest tests/test_dpx.py -v
```
Expected: all 9 tests PASS.

- [ ] **Step 5: Commit**

```bash
git add python-sidecar/src/lmt_vba_sidecar/dpx.py python-sidecar/tests/test_dpx.py
git commit -m "feat(sl): pure-numpy 10-bit DPX reader (disguise variant)"
```

---

## Task 3: Wire `.dpx` into `load_frames` + clean error envelope

**Files:**
- Modify: `python-sidecar/src/lmt_vba_sidecar/sl_decode.py` (imports, 26-42, 226)
- Test: `python-sidecar/tests/test_sl_decode.py` (append)

- [ ] **Step 1: Write the failing end-to-end test**

Append to `python-sidecar/tests/test_sl_decode.py`:

```python
def test_roundtrip_from_dpx_frame_dir(tmp_path):
    # disguise feed export = a directory of 10-bit .dpx frames. Decoding it must
    # match the PNG-directory happy path (every dot, including id=0, recovered).
    from _dpx_fixtures import convert_dir_to_dpx

    sl = _gen(tmp_path)
    meta = json.loads((sl / "sl_meta.json").read_text())
    dpx_dir = sl / "frames_dpx"
    n = convert_dir_to_dpx(sl / "frames", dpx_dir)
    assert n > 0

    dec = DecodeStructuredLightInput.model_validate({
        "command": "decode_structured_light", "version": 1,
        "input_path": str(dpx_dir), "sl_meta_path": str(sl / "sl_meta.json"),
        "output_path": str(tmp_path / "corr.json")})
    assert run_decode_structured_light(dec) == 0
    corr = json.loads((tmp_path / "corr.json").read_text())
    by_id = {p["id"]: p for p in corr["points"]}
    assert len(corr["points"]) == len(meta["dots"])
    assert 0 in by_id


def test_decode_bad_dpx_reports_decode_failed(tmp_path, capsys):
    # An unsupported/garbage .dpx must surface as a clean fatal decode_failed
    # envelope, not an internal_error traceback.
    from lmt_vba_sidecar.ipc import GenerateStructuredLightInput  # noqa: F401 (sl already generated below)

    sl = _gen(tmp_path)
    bad_dir = tmp_path / "bad_dpx"
    bad_dir.mkdir()
    (bad_dir / "frame0000.dpx").write_bytes(b"NOTADPX" + b"\x00" * 2000)

    dec = DecodeStructuredLightInput.model_validate({
        "command": "decode_structured_light", "version": 1,
        "input_path": str(bad_dir), "sl_meta_path": str(sl / "sl_meta.json"),
        "output_path": str(tmp_path / "corr.json")})
    assert run_decode_structured_light(dec) == 1
    events = [json.loads(l) for l in capsys.readouterr().out.splitlines() if l.strip()]
    err = [e for e in events if e.get("event") == "error"][-1]
    assert err["code"] == "decode_failed"
    assert err["fatal"] is True


def test_decode_missing_dpx_file_reports_decode_failed(tmp_path, capsys):
    # Regression guard for the dispatch we add in Step 4: a missing SINGLE .dpx
    # path is routed to read_dpx_gray8, whose read_bytes() raises FileNotFoundError
    # (an OSError, NOT a ValueError). The load wrapper must map it to a clean fatal
    # decode_failed, never let it escape to __main__.py's internal_error+traceback.
    sl = _gen(tmp_path)
    dec = DecodeStructuredLightInput.model_validate({
        "command": "decode_structured_light", "version": 1,
        "input_path": str(tmp_path / "nope.dpx"),
        "sl_meta_path": str(sl / "sl_meta.json"),
        "output_path": str(tmp_path / "corr.json")})
    assert run_decode_structured_light(dec) == 1
    events = [json.loads(l) for l in capsys.readouterr().out.splitlines() if l.strip()]
    err = [e for e in events if e.get("event") == "error"][-1]
    assert err["code"] == "decode_failed"
    assert err["fatal"] is True
```

- [ ] **Step 2: Run to verify failure**

Run:
```bash
cd /Users/bip.lan/AIWorkspace/vp/led-mesh-toolkit/python-sidecar
.venv/bin/python -m pytest tests/test_sl_decode.py::test_roundtrip_from_dpx_frame_dir tests/test_sl_decode.py::test_decode_bad_dpx_reports_decode_failed tests/test_sl_decode.py::test_decode_missing_dpx_file_reports_decode_failed -v
```
Expected, on pristine code, before any edit:
- `test_roundtrip_from_dpx_frame_dir` **FAILS** — `load_frames` filters `.dpx` out of the dir branch → `[]` → "no frames loaded".
- `test_decode_bad_dpx_reports_decode_failed` **PASSES already** — the bad `.dpx` is also filtered out of the dir branch → `[]` → "no frames loaded" → `decode_failed` (right code, coincidental path). After the fix it still passes, now via `read_dpx_gray8` raising `ValueError`. It is a contract guard, not a fail-first test — that is expected.
- `test_decode_missing_dpx_file_reports_decode_failed` **PASSES already** — a missing single `.dpx` currently falls through to `cv2.VideoCapture` (empty) → `[]` → `decode_failed`. **This is the subtle one:** Step 4's dispatch will *re-route* a missing `.dpx` to `read_dpx_gray8` → `FileNotFoundError`. If you add Step 4 but forget Step 5, this test flips to RED (the exception escapes → `internal_error`/test errors). Step 5's `(ValueError, OSError)` catch is what keeps it green. It is a regression guard for our own change.

Only `test_roundtrip_from_dpx_frame_dir` is a true fail-first test here; the other two are guards proving the `decode_failed` contract survives the dispatch change. Do not fabricate a red for them.

- [ ] **Step 3: Add the DPX import**

In `python-sidecar/src/lmt_vba_sidecar/sl_decode.py`, change the import block (lines 22-24):

Old:
```python
from lmt_vba_sidecar.io_utils import write_event
from lmt_vba_sidecar.ipc import DecodeStructuredLightInput, ErrorEvent
from lmt_vba_sidecar.sl_codec import decode_bits
```
New:
```python
from lmt_vba_sidecar.dpx import read_dpx_gray8
from lmt_vba_sidecar.io_utils import write_event
from lmt_vba_sidecar.ipc import DecodeStructuredLightInput, ErrorEvent
from lmt_vba_sidecar.sl_codec import decode_bits
```

- [ ] **Step 4: Dispatch `.dpx` in `load_frames`**

Replace lines 26-42:

Old:
```python
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
```
New:
```python
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
```

- [ ] **Step 5: Wrap the load so a bad DPX maps to `decode_failed`**

In `run_decode_structured_light`, replace line 226:

Old:
```python
    frames = load_frames(cmd.input_path)
```
New:
```python
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
```

- [ ] **Step 6: Run the new tests + full sl_decode suite to verify pass + no regression**

Run:
```bash
cd /Users/bip.lan/AIWorkspace/vp/led-mesh-toolkit/python-sidecar
.venv/bin/python -m pytest tests/test_sl_decode.py tests/test_dpx.py -v
```
Expected: all PASS (the two new tests + every pre-existing sl_decode test).

- [ ] **Step 6b: Sanity-check the regression guard actually guards**

Temporarily revert ONLY Step 5's `except` to `except ValueError as exc:` and run:
```bash
cd /Users/bip.lan/AIWorkspace/vp/led-mesh-toolkit/python-sidecar
.venv/bin/python -m pytest tests/test_sl_decode.py::test_decode_missing_dpx_file_reports_decode_failed -v
```
Expected: it ERRORS/FAILS (FileNotFoundError escapes → internal_error). Then restore `except (ValueError, OSError) as exc:` and confirm it PASSES again. This proves the OSError catch is load-bearing, not dead. (Skip the temporary revert if you trust the reasoning — it is purely a confidence check.)

- [ ] **Step 7: Commit**

```bash
git add python-sidecar/src/lmt_vba_sidecar/sl_decode.py python-sidecar/tests/test_sl_decode.py
git commit -m "feat(sl): load_frames reads .dpx dirs; bad/missing DPX -> decode_failed"
```

---

## Task 4: Rust CLI E2E — decode a `.dpx` directory

**Files:**
- Modify: `crates/lmt-cli/tests/cli_e2e.rs` (append after `decode_structured_light_happy_with_roi_provenance_and_debug`, i.e. after line 2011)

- [ ] **Step 1: Write the E2E test**

Insert this function immediately after the closing brace of `decode_structured_light_happy_with_roi_provenance_and_debug` (line 2011), before the `/// The manifest's...` doc comment at line 2013:

```rust
/// decode-structured-light accepts a disguise-style .dpx frame DIRECTORY through
/// the REAL sidecar: generate PNG frames, transcode them to 10-bit Method-A DPX
/// via the test fixture writer (single source of truth), then decode the .dpx
/// directory. Asserts the CLI seam loads .dpx end to end (n_dots_decoded > 0).
#[cfg(unix)]
#[test]
fn decode_structured_light_accepts_dpx_dir() {
    let tmp = TempDir::new().unwrap();
    let wrapper = match make_sidecar_wrapper(tmp.path()) {
        Some(w) => w,
        None => {
            eprintln!("skipping decode_structured_light_accepts_dpx_dir: python-sidecar venv not found");
            return;
        }
    };
    let proj = tmp.path().join("proj");
    write_gp_project(&proj, 1, 1);
    lmt()
        .env("LMT_VBA_SIDECAR_PATH", &wrapper)
        .args(["--json", "--yes", "visual", "generate-structured-light",
            proj.to_str().unwrap(), "MAIN"])
        .assert()
        .success();
    let sl_dir = proj.join("patterns/MAIN/sl");
    let frames = sl_dir.join("frames");
    let dpx_dir = sl_dir.join("frames_dpx");
    let meta = sl_dir.join("sl_meta.json");
    let out = tmp.path().join("corr.json");

    // Transcode PNG frames -> .dpx using the venv python + the fixture writer.
    let py = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../python-sidecar/.venv/bin/python");
    let fixtures = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../python-sidecar/tests/_dpx_fixtures.py");
    let status = std::process::Command::new(&py)
        .arg(&fixtures)
        .arg(&frames)
        .arg(&dpx_dir)
        .status()
        .expect("run _dpx_fixtures.py converter");
    assert!(status.success(), "DPX conversion failed");
    assert!(dpx_dir.join("frame0000.dpx").is_file(), "converter wrote .dpx frames");

    let assert = lmt()
        .env("LMT_VBA_SIDECAR_PATH", &wrapper)
        .args(["--json", "--yes", "visual", "decode-structured-light",
            dpx_dir.to_str().unwrap(), meta.to_str().unwrap(),
            "--out", out.to_str().unwrap()])
        .assert()
        .success();
    let env: Value = serde_json::from_slice(&assert.get_output().stdout).unwrap();
    assert_eq!(env["ok"], true, "envelope ok: {env}");
    assert!(env["data"]["n_dots_decoded"].as_u64().unwrap() > 0,
        "decoded > 0 dots from .dpx dir");
}
```

- [ ] **Step 2: Build the test binary to verify it compiles**

Run:
```bash
cd /Users/bip.lan/AIWorkspace/vp/led-mesh-toolkit
cargo test -p lmt-cli --test cli_e2e --no-run
```
Expected: compiles clean (no warnings about the new fn).

- [ ] **Step 3: Run the new E2E**

Run:
```bash
cargo test -p lmt-cli --test cli_e2e decode_structured_light_accepts_dpx_dir -- --nocapture
```
Expected: PASS (or a printed "skipping ... venv not found" no-op if the venv is absent — it is present here, so PASS with `n_dots_decoded > 0`).

- [ ] **Step 4: Commit**

```bash
git add crates/lmt-cli/tests/cli_e2e.rs
git commit -m "test(cli): decode-structured-light accepts .dpx frame directory (E2E)"
```

---

## Task 5: Docs — agents-cli.md + DTO docstring

**Files:**
- Modify: `docs/agents-cli.md` (line 44)
- Modify: `python-sidecar/src/lmt_vba_sidecar/ipc.py` (line 225)

- [ ] **Step 1: Update the decode-structured-light manifest row**

In `docs/agents-cli.md` line 44, change the opening description fragment:

Old:
```
Decode a recorded structured-light capture (video or frame directory) into a provenance-stamped screen↔camera correspondence file
```
New:
```
Decode a recorded structured-light capture (video, frame-image directory, or a disguise `.seq` directory of 10-bit `.dpx` frames — DPX is read by a built-in parser and downscaled to 8-bit, no transcode needed) into a provenance-stamped screen↔camera correspondence file
```

- [ ] **Step 2: Update the DTO docstring**

In `python-sidecar/src/lmt_vba_sidecar/ipc.py` line 225:

Old:
```python
    input_path: str           # a video file OR a directory of frame images
```
New:
```python
    input_path: str           # a video file OR a directory of frame images (PNG/JPG/BMP/TIFF or disguise 10-bit .dpx)
```

- [ ] **Step 3: Commit**

```bash
git add docs/agents-cli.md python-sidecar/src/lmt_vba_sidecar/ipc.py
git commit -m "docs(sl): document .dpx directory input for decode-structured-light"
```

---

## Task 6: Full self-check (project contract)

- [ ] **Step 1: Sidecar test suite (no regression)**

Run:
```bash
cd /Users/bip.lan/AIWorkspace/vp/led-mesh-toolkit/python-sidecar
.venv/bin/python -m pytest tests/ -q
```
Expected: all pass (new `test_dpx.py` + `test_sl_decode.py` additions + everything pre-existing).

- [ ] **Step 2: Workspace tests incl. CLI E2E**

Run:
```bash
cd /Users/bip.lan/AIWorkspace/vp/led-mesh-toolkit
cargo test --workspace
```
Expected: all pass. In particular `decode_structured_light_manifest_documents_new_flags` stays green — the DPX-variant error reuses `decode_failed` (18), so the asserted exit-code set `[0, 2, 3, 4, 13, 18]` is unchanged.

- [ ] **Step 3: CLI contract sanity (no schema/help drift)**

Run:
```bash
cd /Users/bip.lan/AIWorkspace/vp/led-mesh-toolkit
cargo build -p lmt-cli
./target/debug/lmt visual decode-structured-light --help
```
Expected: help still lists `<input>`/`<sl_meta>`/`--out`/`--sentinel-threshold`/`--screen-roi`/`--emit-debug-image` unchanged (no new flags — DPX is auto-detected by extension). No `lmt --json schema` change is expected (no DTO field added); skip it.

- [ ] **Step 4: Final commit (if any working-tree changes remain)**

```bash
git status
# only if anything is uncommitted:
git add -A && git commit -m "chore(sl): DPX input self-check"
```

---

## Self-Review (filled by plan author)

**Spec coverage:** §1/§2 → Tasks 2-3; §3 verified layout → Task 2 reader + Task 1 fixture; §4.1 reader → Task 2; §4.2 load_frames → Task 3; §4.3 error surfacing (incl. `OSError` for missing single `.dpx`) → Task 3 Step 5 + Step 6b guard (`decode_failed`); §5 non-goals → respected (no output writer, no 12/16-bit, no new flag/DTO); §6 CLI-contract → Task 4 (E2E) + Task 5 (docs); §7 testing → Tasks 1-4; §8 risk (variant guard) → Task 2 raise tests. No gaps.

**Placeholder scan:** none — every code/command step is concrete.

**Type/name consistency:** `read_dpx_gray8` (dpx.py) imported and called identically in sl_decode.py and test_dpx.py; `write_dpx` / `convert_dir_to_dpx` (`_dpx_fixtures.py`) used identically in test_dpx.py, test_sl_decode.py, and the Rust converter invocation; error code `decode_failed` matches the existing constant used at sl_decode.py:228 and the exit-code set in the manifest test.

**Open item carried from spec §8:** confirmed during planning — the reused fatal error code is `decode_failed` (sl_decode.py:228), exit 18, already in the manifest's asserted set. No new code.

**Adversarial-review fix (Codex, 2026-05-30):** the load wrapper catches `(ValueError, OSError)`, not just `ValueError`. A missing single `.dpx` path hits `read_bytes()` (→ `FileNotFoundError`, an `OSError`) before any `ValueError`; catching only `ValueError` would regress that input from a clean `decode_failed` to an `internal_error`+traceback. Covered by `test_decode_missing_dpx_file_reports_decode_failed` (Task 3 Step 1) + the Step 6b load-bearing-catch sanity check. (Review's other two findings — an unrelated uncommitted `export.rs` orientation change, and a stale sibling outlier-rejection doc — are out of scope for this plan; the latter got a SUPERSEDED banner, the former is left for the user to adjudicate.)
