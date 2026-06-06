# SL Decode — disguise 10-bit DPX Input Support — Spec

**Status:** Draft · **Date:** 2026-05-30 · **Owner:** vision/SL pipeline

> Goal in one line: let `decode-structured-light` ingest a disguise feed export
> (`<name>.seq/` directory of 10-bit `.dpx` frames) directly, with no manual
> transcode step, by adding a pure-numpy DPX reader to the sidecar frame loader.

---

## 1. Problem / Motivation

disguise records/exports SL captures as a `.seq` **directory of 10-bit DPX frames**
(`frameNNNN.dpx`). The sidecar's frame loader `load_frames`
(`python-sidecar/src/lmt_vba_sidecar/sl_decode.py`) cannot read these:

- `_IMG_EXTS` whitelist has no `.dpx`, so DPX files are silently filtered out of the
  directory branch → empty frame list.
- Even if `.dpx` were whitelisted, the directory branch uses
  `cv2.imread(str(f), cv2.IMREAD_GRAYSCALE)`, and **this OpenCV wheel (4.11) has no DPX
  decoder** — `cv2.imread` returns `None` for all flags (verified). The sidecar venv
  has no other image library (numpy/scipy/opencv/pydantic only), and is PyInstaller-packaged
  for offline single-file distribution, so we cannot assume system `ffmpeg`/imageio/OIIO
  at runtime.

Net effect: the one standard disguise output format requires a manual transcode every
time before it can enter the decode pipeline.

## 2. Current State (verified)

`load_frames` (`sl_decode.py:29-42`):

```python
_IMG_EXTS = (".png", ".jpg", ".jpeg", ".bmp", ".tif", ".tiff")

def load_frames(input_path: str) -> list[np.ndarray]:
    p = pathlib.Path(input_path)
    if p.is_dir():
        files = sorted(f for f in p.iterdir() if f.suffix.lower() in _IMG_EXTS)
        return [cv2.imread(str(f), cv2.IMREAD_GRAYSCALE) for f in files]   # 8-bit uint8
    cap = cv2.VideoCapture(str(p))                                          # video branch
    ...  # COLOR_BGR2GRAY -> uint8
```

- Returns **uint8 grayscale** (the `IMREAD_GRAYSCALE` flag forces 8-bit even for 16-bit
  inputs). The whole decode pipeline downstream assumes uint8 0–255 in a few hardcoded
  places (`derive_screen_roi` `.astype(np.uint8)`; `segment_code_region` sentinel
  `mb > sentinel_threshold * 255.0`; `_seed_dots` Otsu needs `CV_8UC1`). The actual bit
  reader `_read_bits_relative` is per-dot relative (bit-depth agnostic).
- Single call site: `run_decode_structured_light(...)` → `frames = load_frames(cmd.input_path)`
  (`sl_decode.py:226`). `input_path` (a dir or a file) comes via stdin JSON
  (`DecodeStructuredLightInput.input_path`, `ipc.py`). No argparse flags; the Rust CLI
  passes `input_path` through unchanged (`crates/lmt-cli` → `lmt-app/src/visual.rs` →
  `adapter-visual-ba` payload `"input_path"`).

**Decision (from brainstorming):** downscale DPX to 8-bit at load time so **no downstream
decode code changes** — lowest risk to the just-stabilized SL core. 10-bit precision is
deliberately not preserved (the relative per-dot bit reader does not benefit materially).

## 3. Verified DPX layout (real disguise sample)

Confirmed against a real sample pulled from disguise output
(`...feed/track 1 lanpc feeds head 2_00000.seq/frame0001.dpx`, 8,302,592 bytes):

| Field | Offset | Value |
|---|---|---|
| magic / endianness | 0 | `XPDS` → **little-endian** (`SDPX` = big-endian) |
| image data offset | 4 (u32) | **8192** (0x2000) |
| version | 8 | "V1.0" |
| total file size | 24 (u32) | **1664 — BOGUS, do not trust** (real = 8.3 MB) |
| PixelsPerLine (W) | 772 (u32) | **1920** |
| LinesPerElement (H) | 776 (u32) | **1080** |
| descriptor | 800 (u8) | **50 = RGB** |
| bit depth | 803 (u8) | **10** |
| packing | 804 (u16) | **1 = Method A** (pad to 32-bit) |
| encoding | 806 (u16) | **0 = none** (no RLE) |
| element data offset | 808 (u32) | 8192 (agrees with @4) |

Size check: `8192 + 1920×1080×4 = 8,302,592` → exact. **One 32-bit word per RGB pixel.**

**Unpack formula — verified pixel-exact against ffmpeg ground truth (maxdiff R=0 G=0 B=0):**

```
word = little-endian uint32 per pixel
R = (word >> 22) & 0x3FF
G = (word >> 12) & 0x3FF
B = (word >>  2) & 0x3FF     # 2 padding bits at LSB
```

(Padding-at-MSB and channel-reversed candidates were tested and rejected — they differed
by hundreds of codes.) The bit formula is endian-independent once the word is read in the
file's endianness.

Sequence convention: frames named `frameNNNN.dpx`, **4-digit, starting at 0000** (this
sample: 143 frames). `sorted()` on the directory handles ordering.

## 4. Design

### 4.1 New module `python-sidecar/src/lmt_vba_sidecar/dpx.py`

Single-purpose, dependency-light (numpy + cv2 only), independently testable.

```
def read_dpx_gray8(path) -> np.ndarray:   # returns (H, W) uint8
```

Steps:
1. Read whole file into bytes.
2. magic @0 → endianness; raise `ValueError` if not `XPDS`/`SDPX`.
3. Read W@772, H@776, data_offset@4 (file endianness); descriptor@800, bit_depth@803,
   packing@804, encoding@806.
4. **Variant guard** (scoped to disguise's variant — explicit error, no silent garbage):
   raise `ValueError` naming the offending field unless `bit_depth==10`, `descriptor==50`,
   `packing==1`, `encoding==0`.
5. Slice `pixels = buf[data_offset : data_offset + W*H*4]`; raise if shorter (truncated).
   **Never use total_file_size@24** (proven bogus) — size is computed from W×H.
6. `words = np.frombuffer(pixels, dtype=end+"u4").reshape(H, W)`; unpack R/G/B per §3.
7. Reduce to uint8 RGB (`>> 2` per channel) then `cv2.cvtColor(rgb_u8, cv2.COLOR_RGB2GRAY)`
   → byte-identical luma to the existing `cv2.IMREAD_GRAYSCALE` path (BT.601). Return uint8.

### 4.2 `load_frames` integration (only production change in `sl_decode.py`)

- Directory branch: accept `.dpx` in addition to `_IMG_EXTS`; dispatch per file —
  `.dpx` → `read_dpx_gray8`, others → `cv2.imread(..., IMREAD_GRAYSCALE)`.
- Single-file branch: if path suffix is `.dpx`, return `[read_dpx_gray8(p)]` (DPX is a
  single-frame still format — one file = one frame).
- Video branch unchanged. Return type stays `list[np.ndarray]` uint8 → **zero downstream change**.

### 4.3 Error surfacing

`run_decode_structured_light` wraps the load so a failed read becomes a clean **fatal
`ErrorEvent`** with code `decode_failed` (the existing input-load constant at
`sl_decode.py:228`, exit 18), reusing the path already used for "no frames loaded from
input". **No new error_code/exit_code category** → no three-place sync; the manifest's
asserted exit-code set `[0,2,3,4,13,18]` is unchanged.

The wrapper catches **both `ValueError` and `OSError`**, not just `ValueError`:
- `ValueError` — `read_dpx_gray8` rejecting an unsupported/corrupt variant (bad magic,
  wrong bit depth/descriptor/packing, RLE, truncated pixels).
- `OSError`/`FileNotFoundError` — a **missing single `.dpx` path** hits `Path(path).read_bytes()`
  inside `read_dpx_gray8` *before* any `ValueError` is raised. Without the `OSError` catch
  this escapes to `__main__.py`'s `internal_error`+traceback fallback, breaking the clean
  `decode_failed` contract. (Caught by Codex adversarial review, 2026-05-30.)

## 5. Scope / Non-goals (YAGNI)

- **No DPX output.** Generate side keeps emitting the TIFF `.seq`; this is input-only.
- **No general DPX.** Only disguise's 10-bit / RGB / Method-A / no-RLE variant. Any other
  bit depth, descriptor, packing (0/2), or RLE → explicit `ValueError`, not best-effort.
- **No 10-bit preservation** (per brainstorming decision) — downscaled to 8-bit.
- **No new CLI subcommand or DTO field.** `input_path` already accepts a dir/file; this is
  an input-format extension to an existing command. The CLI-contract "new command" checklist
  does not apply (see §6).
- **No system-ffmpeg / new runtime dependency.** Reader is pure numpy; ffmpeg is used only
  as a one-time verification oracle during development, never at runtime.

## 6. CLI-contract impact (per project CLAUDE.md)

This enhances an existing command, so most of the new-command checklist is N/A. Triggered items:

- **CLI E2E** — add a `.dpx`-directory happy-path case (see §7).
- **AGENTS doc** — `docs/agents-cli.md`: note that `decode-structured-light` `input_path`
  accepts a `.dpx` frame directory (10-bit, internally downscaled to 8-bit). No error-code
  table change.
- **Sidecar DTO docstring** — `ipc.py` `DecodeStructuredLightInput.input_path` mentions `.dpx`.
- No DTO/schema change → no `schema::dump_all()` / JsonSchema work.
- No new error code → no `error_codes`/`exit_codes`/doc three-place sync.

## 7. Testing

Single source of truth for test DPX bytes: **`python-sidecar/tests/_dpx_fixtures.py`** —
`write_dpx(path, gray_u8)` writes a minimal valid Method-A LE DPX (correct load-bearing
header offsets; `R=G=B=gray<<2` words), plus a `__main__` that converts a frame directory
→ a `.dpx` directory (reads source frames via cv2). Used by both the Python and Rust tests
so the byte layout is defined once.

- **`python-sidecar/tests/test_dpx.py`** (parser unit):
  - write→read round-trip recovers the grayscale; correct shape/dtype.
  - both endiannesses read identically.
  - raises on each guarded field: bad magic, `bit_depth!=10`, `descriptor!=50`,
    `packing!=1`, `encoding!=0`, and truncated pixel data.
- **`python-sidecar/tests/test_sl_decode.py`** (end-to-end, hermetic, in-process):
  - generate SL frames (existing `_gen` helper), emit them as a `.dpx` dir via
    `_dpx_fixtures.write_dpx`, run `run_decode_structured_light` on the `.dpx` dir, assert
    dots recovered (mirrors the existing PNG happy-path). Proves the `.dpx` dispatch +
    full decode.
- **`crates/lmt-cli/tests/cli_e2e.rs`** (`decode_structured_light_accepts_dpx_dir`):
  - generate frames via the existing flow, convert to `.dpx` with
    `<venv>/bin/python tests/_dpx_fixtures.py <frames> <dpx_out>`, run
    `decode-structured-light <dpx_dir> <sl_meta> --out ... --yes`, assert envelope `ok` +
    `n_dots_decoded > 0`. Gated on venv presence like the existing happy-path test.
  - No system-ffmpeg dependency anywhere in tests.

## 8. Risks / open items

- **Variant coverage.** Verified against ONE real frame (1920×1080, 10-bit, Method A, LE,
  RGB). If a different disguise project emits a different bit depth/packing, the reader
  raises a clear error rather than misdecoding — acceptable and explicit. Add variants only
  when a real sample of that variant appears.
- **Resolution generality.** W/H are read from the header (not hardcoded), so other
  resolutions work as long as the variant matches.
- **Confirmed:** the reused fatal error-code constant is `decode_failed` (`sl_decode.py:228`,
  exit 18), already in the manifest's asserted set — no new code. The load wrapper catches
  `(ValueError, OSError)` so missing/corrupt `.dpx` both map to it (see §4.3).
