# Per-Cabinet Pitch-Matched ChArUco + Deterministic Marker Routing — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Generate a ChArUco board sized to each cabinet's *own* pixel pitch and resolution (supporting non-square cabinets), and route every detected marker to its cabinet/corner via a deterministic O(1) name+ID scheme borrowed from UE's `LedWallCalibration`, replacing the current uniform-square board + per-cabinet full-image re-scan.

**Architecture:** `generate-pattern` gains an optional `--screen-mapping` input so per-cabinet geometry comes from the *same* `screen_mapping.json` that `reconstruct` already consumes (single source of truth). For each cabinet the generator picks `(squares_x, squares_y)` to match that cabinet's pixel aspect at a detectable square-pixel size, renders the board at exact integer square pixels (no letterbox, no stretch), and records per-cabinet `squares_x/squares_y/square_px` + `pixel_pitch_mm` + ArUco ID block in `pattern_meta.json` (schema **v2**). Local-mm becomes pitch-based (`x = (corner_px − board_w_px/2)·pitch_x`), so the corner coordinates are exact even for non-square boards. Detection switches to one `detectMarkers` pass per image plus a precomputed `marker_id → cabinet` map, then per-cabinet `interpolateCornersCharuco` on the already-detected markers.

**Tech Stack:** Python sidecar (`opencv-contrib-python` aruco, pydantic v2, pytest), Rust adapter (`adapter-visual-ba`, `lmt-cli` clap, serde), the project's CLI contract (`docs/agents-cli.md`, `contract-manifest.json`).

---

## Scope

One cohesive subsystem: the ChArUco **generation + routing** path. It deliberately does **not** touch the BA kernel, gauge fixing, or the pose-report format. It is testable end-to-end on the existing monitor-bench fixture.

Out of scope (note for the engineer, do not implement here):
- Structured light (`graycode`) — separate gated effort (spec §16).
- Selectable ArUco dictionaries (4X4…7X7) — a clean follow-up; this plan keeps `DICT_6X6_1000` and only makes the capacity check per-cabinet-variable.
- Rotation/mirror local-mm support — still raised as unsupported (unchanged).

## Backward-compatibility contract

- `generate-pattern` **without** `--screen-mapping` keeps the current uniform-grid path. With the board-shape chooser pinned to `DEFAULT_SQUARES_SHORT = 9` (see DD2), a **square cabinet whose side divides evenly by 9** reproduces the legacy 9×9 / 40-marker board *bit-for-bit* (e.g. 1080→`square_px=120`, 9×9). A non-square cabinet, or a side that does not divide evenly, will get a different square count — that is the intended new behavior, not a regression. So: the uniform path still runs and stays under the 25-cabinet ceiling for square cabinets, but PNG bytes and the pattern hash may differ for non-square / non-evenly-dividing cabinets. Any existing test that hard-codes the old 9×9 layout for such a cabinet must be updated (Task 5.2).
- `pattern_meta.json` schema version goes 1 → 2. `reconstruct`/`detect` read v2. A v1 file (no per-cabinet `squares_x`) is rejected with `invalid_input` telling the user to re-generate. (We do not migrate v1 in place — the pattern must be re-generated and re-displayed anyway.)
- Changing `PatternMeta` changes the pattern hash (`reconstruct.pattern_hash`). This is expected; `screen_mapping.expected_pattern_hash` must be refreshed (the bench template `docs/poc/monitor-bench-report-template.md` §3 already documents how).

## File Structure

| File | Responsibility | Change |
|---|---|---|
| `python-sidecar/src/lmt_vba_sidecar/ipc.py` | IPC pydantic DTOs | Extend `PatternMetaCabinet` (+`squares_x/squares_y/square_px/pixel_pitch_mm`), add `schema_version` to `PatternMeta`, add optional `screen_mapping_path` to `GeneratePatternInput` |
| `python-sidecar/src/lmt_vba_sidecar/board_layout.py` | **NEW** — pure functions: choose per-cabinet `(squares_x, squares_y, square_px)`; build/route `marker_id ↔ cabinet`; canonical names | Create |
| `python-sidecar/src/lmt_vba_sidecar/pattern.py` | Generation | Per-cabinet board build from screen_mapping; non-square boards; variable ID blocks; write v2 meta; per-cabinet capacity check |
| `python-sidecar/src/lmt_vba_sidecar/screen_mapping.py` | local-mm + preflight | New pitch-based `charuco_corner_local_mm` honoring per-cabinet `squares_x/squares_y/square_px/pitch` |
| `python-sidecar/src/lmt_vba_sidecar/detect.py` | Detection | Single `detectMarkers` pass + `marker_id→cabinet` routing + per-cabinet `interpolateCornersCharuco` |
| `python-sidecar/src/lmt_vba_sidecar/reconstruct.py` | Orchestration | Pass per-cabinet `squares_x/squares_y` into detect + local-mm; build routing map |
| `crates/adapter-visual-ba/src/ipc.rs` | Rust mirror of `PatternMeta` | Add the v2 fields (serde) |
| `crates/adapter-visual-ba/src/api.rs` | `GeneratePatternArgs` | Add optional `screen_mapping_path`; forward to payload |
| `crates/lmt-cli/src/cli.rs` + `commands/visual.rs` | CLI surface | Add `--screen-mapping` to `generate-pattern` |
| `crates/lmt-cli/tests/cli_e2e.rs` | CLI E2E | happy + invalid-input cases |
| `docs/agents-cli.md` | CLI contract doc | Update `generate-pattern` row + flag |
| `docs/poc/monitor-bench-report-template.md` | Bench protocol | Note `--screen-mapping` in the generate step |

---

## Design Decisions (read before any task)

**DD1 — Per-cabinet geometry source = `screen_mapping.json`.** `generate-pattern` gains optional `--screen-mapping`. When present, per-cabinet `resolution_px` / `active_size_mm` / `pixel_pitch_mm` / `input_rect_px` come from there (reuse `ScreenMappingCabinet`). When absent → current uniform behavior. This is the *single source of truth* used by both generate and reconstruct — no new project.yaml fields.

**DD1a — Exact coverage when `--screen-mapping` is given (no silent fallback).** *(Codex review fix.)* Single-source-of-truth means: if `screen_mapping` is supplied, it must describe **exactly** the set of present (non-absent) grid cabinets — no more, no less. A present cabinet missing from `screen_mapping`, or a `cabinet_id` in `screen_mapping` that does not correspond to a present grid cell (stale/misspelled id), is a hard `invalid_input` error — **not** a per-cabinet fall back to uniform `project.yaml` dimensions. Uniform geometry is used **only** when `--screen-mapping` is absent entirely (then *every* cabinet is uniform). Rationale: a partial/typo'd mapping that silently mixes mapped + uniform cabinets generates a pattern that cannot be reconstructed cleanly (reconstruct's `_cabinet` lookup expects every detected cabinet present in the mapping) and corrupts metric scale on the fallback cabinets.

**DD2 — Per-cabinet board shape (the core of requirement #1).** The square *count* is anchored to the cabinet's **short side** via `DEFAULT_SQUARES_SHORT = 9` (matching the legacy 9×9 board), and the long side scales by aspect ratio. This is deliberate: a naïve "pack as many `MIN_SQUARE_PX` squares as fit" rule would put ~18×18 squares on a 1080px cabinet (162 markers/cabinet), which **slashes the 1000-marker dictionary ceiling from 25 cabinets to ~6** and breaks back-compat. Anchoring to the short side keeps a square cabinet at the legacy count and only widens the long axis for non-square cabinets:
```
w_px, h_px = resolution_px
short      = min(w_px, h_px)
square_px  = max(MIN_SQUARE_PX, short // DEFAULT_SQUARES_SHORT)   # integer px/cell
squares_x  = max(2, w_px // square_px)
squares_y  = max(2, h_px // square_px)
board_w_px = squares_x * square_px      # ≤ W_px (board centered in cell, black margin allowed)
board_h_px = squares_y * square_px      # ≤ H_px
```
Worked examples: `1080×1080 → square_px=120, 9×9` (= legacy, 40 markers); `1920×1080 → square_px=120, 16×9` (72 markers, non-square); `960×540 → square_px=60, 16×9`. `squares_x` may differ from `squares_y` (non-square board — confirmed valid; UE's board is 5×7, `CameraCalibrationCharucoBoard.h:51-55`). Each *cell* stays a true square (`squareLength` single value in `cv2.aruco.CharucoBoard`), rendered at exactly `square_px` pixels → no letterbox-of-a-square-into-a-rectangle, no stretch.

**DD3 — Pitch-based local-mm (the payoff of requirement #1).** Corner at inner index `(r, c)` sits at board-pixel `((c+1)·square_px, (r+1)·square_px)`. With the board centered and the cabinet's own pitch:
```
x_mm = ((c+1)·square_px − board_w_px/2) · pitch_x
y_mm = ((r+1)·square_px − board_h_px/2) · pitch_y
```
This uses **the cabinet's own pixel pitch directly**, so corner mm is exact for any per-cabinet size/pitch and any non-square board. (The old `active_size/(inner+1)` formula assumed a square board and a single `inner`.)

**DD4 — Deterministic ID + naming + O(1) routing (requirement #2).** Cabinets are ordered row-major (`row` outer, `col` inner — matches current `pattern.py` loop). Each cabinet gets a **contiguous** ArUco ID block of `markers_per_board(squares_x, squares_y)` markers, allocated sequentially (no gaps → no wasted dictionary capacity, important given the 1000-marker ceiling). A `marker_id → (col,row)` map is built once (O(total markers ≤ 1000) build, O(1) lookup) — this is the "fast find" rule. Canonical names (borrowing UE's `<Dict>-<Id>-<Corner>` idea, adapted to lmt's grid):
- cabinet: `V{col:03d}_R{row:03d}` (unchanged, already used by pose report)
- ChArUco corner / measured point: `{screen_id}_V{col:03d}_R{row:03d}_C{charuco_id:03d}`
- the `marker_id→cabinet` map and per-cabinet ID block live in `pattern_meta.json` so detection and any external tool can reverse-route a marker without re-scanning.

**DD5 — Detection: one scan, then route (requirement #2 performance).** Replace the per-cabinet `detectBoard` loop (O(N_cab) full-image scans) with: one `cv2.aruco.detectMarkers(img, DICT_6X6_1000)` per image → bucket detected markers into cabinets via the `marker_id→cabinet` map → per cabinet call `cv2.aruco.interpolateCornersCharuco(bucket_corners, bucket_ids, img, board)` to recover sub-pixel checkerboard corners. One scan + cheap per-cabinet interpolation; keeps ChArUco sub-pixel precision.

**DD6 — Screen assembly honors `input_rect_px` (the display artifact must match the mapping).** *(Codex review fix.)* `full_screen.png` is the single-framebuffer "Disguise drop-in" (`pattern.py` module docstring) that physically drives the wall/monitors. Each cabinet board must be pasted at the **exact rectangle that cabinet occupies in the feed**, i.e. its `input_rect_px = [x, y, w, h]`, not at a derived uniform `col·cw / row·ch` cell. With unequal cabinets (different `resolution_px`, different per-row/col counts, or a physical gap between two monitors), a uniform grid places the wrong board pixels under a cabinet → that monitor displays a garbled/foreign board → the camera captures unusable data for it. So:
- Each cabinet spec carries a **placement rect** `(x, y, w, h)`. In `--screen-mapping` mode it is the cabinet's `input_rect_px`; in uniform mode it is `(col·cw, row·ch, cw, ch)` with `cw = sw//cols, ch = sh//rows`.
- The board PNG (`board_w_px × board_h_px`, which may be smaller than the rect) is **centered inside its placement rect**.
- Validation: each board must fit inside its rect (`board_w_px ≤ w and board_h_px ≤ h`, else `invalid_input`), and every rect must fit inside `screen_resolution` (`x+w ≤ sw and y+h ≤ sh`, else `invalid_input`). The even-divisibility check (`sw % cols`, `sh % rows`) applies **only in uniform mode** — in mapped mode the rects define placement and divisibility is irrelevant.

**Constants** (in `board_layout.py`): `DEFAULT_SQUARES_SHORT = 9` (legacy 9×9 board ⇒ 40 markers/cabinet ⇒ 25-cabinet ceiling preserved for square cabinets). `MIN_SQUARE_PX = 60` is a **detectability floor**, not the target (a 6×6 marker at 0.7 ratio ⇒ ~6 px/bit ⇒ comfortably detectable); it only kicks in for very-low-resolution cabinets where `short // 9 < 60`. `MARKER_LENGTH_RATIO = 0.7` (matches current `pattern.py:81`).

---

## Phase 0 — Schema v2 (DTOs first, both languages)

### Task 0.1: Extend `PatternMetaCabinet` + `PatternMeta` (Python)

**Files:**
- Modify: `python-sidecar/src/lmt_vba_sidecar/ipc.py:72-83`
- Test: `python-sidecar/tests/test_ipc_pattern_meta_v2.py` (create)

- [ ] **Step 1: Write the failing test**

```python
# python-sidecar/tests/test_ipc_pattern_meta_v2.py
from lmt_vba_sidecar.ipc import PatternMeta, PatternMetaCabinet


def test_pattern_meta_v2_roundtrip_with_per_cabinet_geometry():
    meta = PatternMeta(
        schema_version=2,
        aruco_dict="DICT_6X6_1000",
        cabinets=[
            PatternMetaCabinet(
                col=0, row=0, aruco_id_start=0, aruco_id_end=39,
                squares_x=9, squares_y=9, square_px=120,
                pixel_pitch_mm=[0.2778, 0.2778],
            )
        ],
    )
    dumped = meta.model_dump_json()
    again = PatternMeta.model_validate_json(dumped)
    cab = again.cabinets[0]
    assert again.schema_version == 2
    assert (cab.squares_x, cab.squares_y, cab.square_px) == (9, 9, 120)
    assert cab.pixel_pitch_mm == [0.2778, 0.2778]
    assert cab.markers == (9 * 9) // 2  # derived helper


def test_pattern_meta_rejects_v1_missing_squares():
    import pytest
    from pydantic import ValidationError
    with pytest.raises(ValidationError):
        PatternMeta.model_validate_json(
            '{"schema_version":2,"aruco_dict":"DICT_6X6_1000",'
            '"cabinets":[{"col":0,"row":0,"aruco_id_start":0,"aruco_id_end":39}]}'
        )
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cd python-sidecar && .venv/bin/python -m pytest tests/test_ipc_pattern_meta_v2.py -v`
Expected: FAIL — `PatternMeta` has no `schema_version`, `PatternMetaCabinet` has no `squares_x`.

- [ ] **Step 3: Implement the DTO changes**

Replace `ipc.py:72-83` with:

```python
class PatternMetaCabinet(BaseModel):
    col: int
    row: int
    aruco_id_start: int
    aruco_id_end: int
    # v2: per-cabinet board geometry (pitch-matched generation)
    squares_x: int = Field(ge=2)
    squares_y: int = Field(ge=2)
    square_px: int = Field(gt=0)
    pixel_pitch_mm: PositiveSizePair  # [pitch_x, pitch_y]

    @property
    def markers(self) -> int:
        """Markers this board consumes (alternating cells of squares_x×squares_y)."""
        return (self.squares_x * self.squares_y) // 2

    @property
    def inner_x(self) -> int:
        return self.squares_x - 1

    @property
    def inner_y(self) -> int:
        return self.squares_y - 1


class PatternMeta(BaseModel):
    schema_version: Literal[2]
    aruco_dict: str
    cabinets: list[PatternMetaCabinet]
```

Note: `markers_per_cabinet` and `checkerboard_inner_corners` (old global fields) are removed — they were uniform and are now per-cabinet. `PositiveSizePair` is already imported in `ipc.py`.

- [ ] **Step 4: Run test to verify it passes**

Run: `cd python-sidecar && .venv/bin/python -m pytest tests/test_ipc_pattern_meta_v2.py -v`
Expected: PASS (both tests).

- [ ] **Step 5: Commit**

```bash
git add python-sidecar/src/lmt_vba_sidecar/ipc.py python-sidecar/tests/test_ipc_pattern_meta_v2.py
git commit -m "feat(vba): PatternMeta v2 with per-cabinet board geometry"
```

### Task 0.2: Add optional `screen_mapping_path` to `GeneratePatternInput` (Python)

**Files:**
- Modify: `python-sidecar/src/lmt_vba_sidecar/ipc.py:118-124`
- Test: `python-sidecar/tests/test_ipc_generate_pattern_input.py` (create)

- [ ] **Step 1: Write the failing test**

```python
# python-sidecar/tests/test_ipc_generate_pattern_input.py
from lmt_vba_sidecar.ipc import GeneratePatternInput


def test_generate_pattern_input_optional_screen_mapping():
    base = {
        "command": "generate_pattern", "version": 1,
        "project": {"screen_id": "BENCH",
                    "cabinet_array": {"cols": 1, "rows": 2, "cabinet_size_mm": [300.0, 300.0]}},
        "output_dir": "/tmp/out", "screen_resolution": [1080, 2160],
    }
    assert GeneratePatternInput.model_validate(base).screen_mapping_path is None
    with_sm = {**base, "screen_mapping_path": "/tmp/screen_mapping.json"}
    assert GeneratePatternInput.model_validate(with_sm).screen_mapping_path == "/tmp/screen_mapping.json"
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cd python-sidecar && .venv/bin/python -m pytest tests/test_ipc_generate_pattern_input.py -v`
Expected: FAIL — `screen_mapping_path` unknown / not stored.

- [ ] **Step 3: Implement**

In `ipc.py`, add one field to `GeneratePatternInput` (after `screen_resolution`):

```python
class GeneratePatternInput(BaseModel):
    command: Literal["generate_pattern"]
    version: Literal[1]
    project: GeneratePatternProject
    output_dir: str
    screen_resolution: PositiveIntPair
    # When set, per-cabinet board geometry (size/pitch) is read from this
    # screen_mapping.json; when None, uniform grid generation is used.
    screen_mapping_path: str | None = None
```

- [ ] **Step 4: Run test to verify it passes**

Run: `cd python-sidecar && .venv/bin/python -m pytest tests/test_ipc_generate_pattern_input.py -v`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add python-sidecar/src/lmt_vba_sidecar/ipc.py python-sidecar/tests/test_ipc_generate_pattern_input.py
git commit -m "feat(vba): generate_pattern accepts optional screen_mapping_path"
```

---

## Phase 1 — `board_layout.py` (pure logic: shape + routing + names)

### Task 1.1: Per-cabinet board shape chooser

**Files:**
- Create: `python-sidecar/src/lmt_vba_sidecar/board_layout.py`
- Test: `python-sidecar/tests/test_board_layout_shape.py` (create)

- [ ] **Step 1: Write the failing test**

```python
# python-sidecar/tests/test_board_layout_shape.py
from lmt_vba_sidecar.board_layout import (
    choose_board_shape, markers_per_board, MIN_SQUARE_PX, DEFAULT_SQUARES_SHORT,
)


def test_square_cabinet_reproduces_legacy_9x9_40_markers():
    # Back-compat anchor: a 1080px square cabinet must reproduce the legacy board.
    sx, sy, spx = choose_board_shape(resolution_px=(1080, 1080))
    assert (sx, sy, spx) == (9, 9, 120)
    assert markers_per_board(sx, sy) == 40  # legacy markers_per_cabinet
    assert spx >= MIN_SQUARE_PX
    assert sx * spx <= 1080 and sy * spx <= 1080


def test_widescreen_cabinet_gives_more_columns_than_rows():
    # 1920x1080 short side = 1080 -> square_px=120 -> 16x9 (non-square)
    sx, sy, spx = choose_board_shape(resolution_px=(1920, 1080))
    assert (sx, sy) == (16, 9)
    assert sx > sy  # non-square board fills the 16:9 region
    assert sx * spx <= 1920 and sy * spx <= 1080


def test_short_side_anchors_square_count():
    # Anchored to DEFAULT_SQUARES_SHORT on the short side (not packed to MIN_SQUARE_PX).
    sx, sy, spx = choose_board_shape(resolution_px=(960, 540))
    assert min(sx, sy) == DEFAULT_SQUARES_SHORT or spx == MIN_SQUARE_PX
    assert (sx, sy, spx) == (16, 9, 60)


def test_minimum_two_squares_each_axis():
    sx, sy, spx = choose_board_shape(resolution_px=(130, 130))
    assert sx >= 2 and sy >= 2
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cd python-sidecar && .venv/bin/python -m pytest tests/test_board_layout_shape.py -v`
Expected: FAIL — module `board_layout` does not exist.

- [ ] **Step 3: Implement**

```python
# python-sidecar/src/lmt_vba_sidecar/board_layout.py
"""Pure helpers for per-cabinet ChArUco board layout, marker-id routing,
and canonical naming. No OpenCV, no IO — easy to unit test."""
from __future__ import annotations

DEFAULT_SQUARES_SHORT = 9   # squares on the cabinet's SHORT side (legacy 9x9 -> 40 markers)
MIN_SQUARE_PX = 60          # detectability FLOOR (6x6 marker @0.7 ratio ~6 px/bit), not the target
MARKER_LENGTH_RATIO = 0.7   # matches pattern.py board construction


def choose_board_shape(
    *,
    resolution_px: tuple[int, int],
    squares_short: int = DEFAULT_SQUARES_SHORT,
) -> tuple[int, int, int]:
    """Pick (squares_x, squares_y, square_px) for one cabinet.

    Square COUNT is anchored to the short side (`squares_short`) so a square
    cabinet reproduces the legacy 9x9/40-marker board and the 1000-marker
    dictionary ceiling (25 cabinets) is preserved. The long side scales by
    aspect ratio, so a non-square cabinet yields squares_x != squares_y. Each
    cell renders at an integer `square_px`, so cells stay perfectly square (no
    stretch). `MIN_SQUARE_PX` is only a detectability floor for tiny cabinets.
    """
    w_px, h_px = resolution_px
    short = min(w_px, h_px)
    square_px = max(MIN_SQUARE_PX, short // squares_short)
    squares_x = max(2, w_px // square_px)
    squares_y = max(2, h_px // square_px)
    return squares_x, squares_y, square_px


def markers_per_board(squares_x: int, squares_y: int) -> int:
    """ArUco markers on a squares_x × squares_y ChArUco board (alternating cells)."""
    return (squares_x * squares_y) // 2
```

- [ ] **Step 4: Run test to verify it passes**

Run: `cd python-sidecar && .venv/bin/python -m pytest tests/test_board_layout_shape.py -v`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add python-sidecar/src/lmt_vba_sidecar/board_layout.py python-sidecar/tests/test_board_layout_shape.py
git commit -m "feat(vba): per-cabinet board shape chooser (non-square aware)"
```

### Task 1.2: Marker-id routing map + canonical names

**Files:**
- Modify: `python-sidecar/src/lmt_vba_sidecar/board_layout.py`
- Test: `python-sidecar/tests/test_board_layout_routing.py` (create)

- [ ] **Step 1: Write the failing test**

```python
# python-sidecar/tests/test_board_layout_routing.py
from lmt_vba_sidecar.board_layout import (
    build_marker_routing, cabinet_name, corner_name,
)


def test_routing_maps_marker_id_to_cabinet_block():
    # two cabinets: block0 = ids 0..39 (cab (0,0)), block1 = 40..71 (cab (0,1))
    blocks = [
        {"col": 0, "row": 0, "aruco_id_start": 0, "aruco_id_end": 39},
        {"col": 0, "row": 1, "aruco_id_start": 40, "aruco_id_end": 71},
    ]
    route = build_marker_routing(blocks)
    assert route[0] == (0, 0)
    assert route[39] == (0, 0)
    assert route[40] == (0, 1)
    assert route[71] == (0, 1)
    assert 72 not in route  # outside any block


def test_canonical_names():
    assert cabinet_name(0, 0) == "V000_R000"
    assert cabinet_name(12, 5) == "V012_R005"
    assert corner_name("BENCH", 0, 0, 12) == "BENCH_V000_R000_C012"
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cd python-sidecar && .venv/bin/python -m pytest tests/test_board_layout_routing.py -v`
Expected: FAIL — functions not defined.

- [ ] **Step 3: Implement (append to `board_layout.py`)**

```python
def cabinet_name(col: int, row: int) -> str:
    return f"V{col:03d}_R{row:03d}"


def corner_name(screen_id: str, col: int, row: int, charuco_id: int) -> str:
    """Canonical measured-point / corner name. Reverse-routable by a tool."""
    return f"{screen_id}_{cabinet_name(col, row)}_C{charuco_id:03d}"


def build_marker_routing(blocks: list[dict]) -> dict[int, tuple[int, int]]:
    """Build marker_id -> (col, row). O(total markers) build, O(1) lookup.

    `blocks` items: {"col", "row", "aruco_id_start", "aruco_id_end"} (inclusive).
    """
    route: dict[int, tuple[int, int]] = {}
    for b in blocks:
        for marker_id in range(b["aruco_id_start"], b["aruco_id_end"] + 1):
            route[marker_id] = (b["col"], b["row"])
    return route
```

- [ ] **Step 4: Run test to verify it passes**

Run: `cd python-sidecar && .venv/bin/python -m pytest tests/test_board_layout_routing.py -v`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add python-sidecar/src/lmt_vba_sidecar/board_layout.py python-sidecar/tests/test_board_layout_routing.py
git commit -m "feat(vba): deterministic marker->cabinet routing + canonical names"
```

---

## Phase 2 — Generation (`pattern.py`)

### Task 2.1: Generate one cabinet board from explicit shape

**Files:**
- Modify: `python-sidecar/src/lmt_vba_sidecar/pattern.py:49-87` (`generate_cabinet_png`)
- Test: `python-sidecar/tests/test_pattern_per_cabinet.py` (create)

- [ ] **Step 1: Write the failing test**

```python
# python-sidecar/tests/test_pattern_per_cabinet.py
import pathlib
import cv2
from lmt_vba_sidecar.pattern import generate_cabinet_png
from lmt_vba_sidecar.board_layout import markers_per_board


def test_generate_non_square_cabinet_png(tmp_path: pathlib.Path):
    out = tmp_path / "V000_R000.png"
    next_id = generate_cabinet_png(
        out_path=out, aruco_id_start=0,
        squares_x=16, squares_y=9, square_px=60,
    )
    assert next_id == markers_per_board(16, 9)
    img = cv2.imread(str(out), cv2.IMREAD_GRAYSCALE)
    assert img is not None
    # board canvas is squares*square_px (cells square, no stretch)
    assert img.shape == (9 * 60, 16 * 60)  # (height, width)


def test_id_overflow_raises(tmp_path: pathlib.Path):
    import pytest
    with pytest.raises(ValueError):
        generate_cabinet_png(
            out_path=tmp_path / "x.png", aruco_id_start=990,
            squares_x=9, squares_y=9, square_px=60,  # needs 40 > 10 left
        )
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cd python-sidecar && .venv/bin/python -m pytest tests/test_pattern_per_cabinet.py -v`
Expected: FAIL — `generate_cabinet_png` does not accept `squares_x/squares_y/square_px`.

- [ ] **Step 3: Implement**

Replace `generate_cabinet_png` (`pattern.py:49-87`) with:

```python
def generate_cabinet_png(
    *,
    out_path: pathlib.Path,
    aruco_id_start: int,
    squares_x: int,
    squares_y: int,
    square_px: int,
    aruco_dict_name: str = DEFAULT_ARUCO_DICT,
) -> int:
    """Render one cabinet's ChArUco PNG at exact integer square pixels.

    Canvas = (squares_x*square_px, squares_y*square_px); cells stay square.
    Returns the next free ArUco ID (caller assigns blocks sequentially).
    """
    if aruco_dict_name != DEFAULT_ARUCO_DICT:
        raise ValueError(f"only {DEFAULT_ARUCO_DICT} supported")
    from lmt_vba_sidecar.board_layout import markers_per_board
    aruco_dict = _aruco_dict()
    n_markers = markers_per_board(squares_x, squares_y)
    if aruco_id_start + n_markers > ARUCO_DICT_CAPACITY:
        raise ValueError(
            f"ArUco ID range {aruco_id_start}..{aruco_id_start + n_markers} "
            f"overflows {DEFAULT_ARUCO_DICT} ({ARUCO_DICT_CAPACITY} markers)"
        )
    sub_dict = cv2.aruco.Dictionary(
        aruco_dict.bytesList[aruco_id_start:aruco_id_start + n_markers],
        aruco_dict.markerSize,
    )
    board = cv2.aruco.CharucoBoard(
        size=(squares_x, squares_y),
        squareLength=1.0,
        markerLength=0.7,
        dictionary=sub_dict,
    )
    img = board.generateImage(
        (squares_x * square_px, squares_y * square_px),
        marginSize=0, borderBits=1,
    )
    out_path.parent.mkdir(parents=True, exist_ok=True)
    cv2.imwrite(str(out_path), img)
    return aruco_id_start + n_markers
```

Delete the now-unused `_markers_per_board` (`pattern.py:39-46`) and its uses (replaced by `board_layout.markers_per_board`). `ARUCO_DICT_CAPACITY` already exists at `pattern.py:116`; move its definition above `generate_cabinet_png` (top of file with `DEFAULT_ARUCO_DICT`).

- [ ] **Step 4: Run test to verify it passes**

Run: `cd python-sidecar && .venv/bin/python -m pytest tests/test_pattern_per_cabinet.py -v`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add python-sidecar/src/lmt_vba_sidecar/pattern.py python-sidecar/tests/test_pattern_per_cabinet.py
git commit -m "feat(vba): generate_cabinet_png takes explicit (squares_x,squares_y,square_px)"
```

### Task 2.2: Per-cabinet spec resolution (screen_mapping exact-coverage, or uniform fallback)

**Files:**
- Modify: `python-sidecar/src/lmt_vba_sidecar/pattern.py` (new helper `_resolve_cabinet_specs`)
- Test: `python-sidecar/tests/test_pattern_specs.py` (create)

Each spec dict carries `"col","row","resolution_px":(w,h),"pixel_pitch_mm":(px,py),"input_rect_px":(x,y,w,h)` — the `input_rect_px` is the placement rect for assembly (DD6). In `--screen-mapping` mode coverage is **exact** (DD1a): a missing or extra/misspelled cabinet raises `ValueError` (the caller maps it to `invalid_input`).

- [ ] **Step 1: Write the failing test**

```python
# python-sidecar/tests/test_pattern_specs.py
import pytest
from lmt_vba_sidecar.pattern import _resolve_cabinet_specs
from lmt_vba_sidecar.screen_mapping import ScreenMapping, ScreenMappingCabinet


def _cab(cid, res, size, pitch, rect):
    return ScreenMappingCabinet(
        cabinet_id=cid, resolution_px=res, active_size_mm=size, pixel_pitch_mm=pitch,
        active_origin="center", input_rect_px=rect,
        rotation=0, mirror_x=False, mirror_y=False)


def test_uniform_fallback_when_no_screen_mapping():
    specs = _resolve_cabinet_specs(
        cols=2, rows=1, absent=set(),
        screen_resolution=(2160, 1080), screen_mapping=None,
        cabinet_size_mm=[300.0, 300.0],
    )
    assert {(s["col"], s["row"]) for s in specs} == {(0, 0), (1, 0)}
    s = specs[0]
    assert s["resolution_px"] == (1080, 1080)  # 2160/2 x 1080/1
    assert s["pixel_pitch_mm"] == (300.0 / 1080, 300.0 / 1080)
    # uniform placement rect = (col*cw, row*ch, cw, ch)
    assert s["input_rect_px"] == (0, 0, 1080, 1080)
    assert specs[1]["input_rect_px"] == (1080, 0, 1080, 1080)


def test_screen_mapping_drives_per_cabinet_geometry_and_rects():
    sm = ScreenMapping(
        screen_id="BENCH", expected_pattern_hash="x",
        cabinets=[
            _cab("V000_R000", [1920, 1080], [600.0, 337.5], [0.3125, 0.3125], [0, 0, 1920, 1080]),
            _cab("V000_R001", [1080, 1080], [300.0, 300.0], [0.2778, 0.2778], [0, 1080, 1080, 1080]),
        ],
    )
    specs = _resolve_cabinet_specs(
        cols=1, rows=2, absent=set(),
        screen_resolution=(1920, 2160), screen_mapping=sm, cabinet_size_mm=[300.0, 300.0],
    )
    by_cr = {(s["col"], s["row"]): s for s in specs}
    assert by_cr[(0, 0)]["resolution_px"] == (1920, 1080)
    assert by_cr[(0, 1)]["resolution_px"] == (1080, 1080)
    # different pitch per cabinet flows through
    assert by_cr[(0, 0)]["pixel_pitch_mm"] != by_cr[(0, 1)]["pixel_pitch_mm"]
    # placement rects come straight from input_rect_px (NOT a uniform grid)
    assert by_cr[(0, 0)]["input_rect_px"] == (0, 0, 1920, 1080)
    assert by_cr[(0, 1)]["input_rect_px"] == (0, 1080, 1080, 1080)


def test_missing_cabinet_in_mapping_is_rejected():
    sm = ScreenMapping(screen_id="BENCH", expected_pattern_hash="x",
        cabinets=[_cab("V000_R000", [1080, 1080], [300.0, 300.0], [0.2778, 0.2778], [0, 0, 1080, 1080])])
    with pytest.raises(ValueError, match="V000_R001"):
        _resolve_cabinet_specs(cols=1, rows=2, absent=set(),
            screen_resolution=(1080, 2160), screen_mapping=sm, cabinet_size_mm=[300.0, 300.0])


def test_extra_or_misspelled_cabinet_in_mapping_is_rejected():
    sm = ScreenMapping(screen_id="BENCH", expected_pattern_hash="x",
        cabinets=[
            _cab("V000_R000", [1080, 1080], [300.0, 300.0], [0.2778, 0.2778], [0, 0, 1080, 1080]),
            _cab("V000_R009", [1080, 1080], [300.0, 300.0], [0.2778, 0.2778], [0, 1080, 1080, 1080]),
        ])
    with pytest.raises(ValueError, match="V000_R009"):
        _resolve_cabinet_specs(cols=1, rows=1, absent=set(),
            screen_resolution=(1080, 1080), screen_mapping=sm, cabinet_size_mm=[300.0, 300.0])
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cd python-sidecar && .venv/bin/python -m pytest tests/test_pattern_specs.py -v`
Expected: FAIL — `_resolve_cabinet_specs` not defined.

- [ ] **Step 3: Implement (add to `pattern.py`)**

```python
def _resolve_cabinet_specs(
    *, cols: int, rows: int, absent: set,
    screen_resolution: tuple[int, int],
    screen_mapping,  # ScreenMapping | None
    cabinet_size_mm: list[float],
) -> list[dict]:
    """Return per-cabinet specs in row-major order.

    Each: {"col","row","resolution_px":(w,h),"pixel_pitch_mm":(px,py),
           "input_rect_px":(x,y,w,h)}.

    --screen-mapping mode (screen_mapping is not None): per-cabinet geometry +
    placement rect come from screen_mapping, with EXACT coverage (DD1a) — every
    present grid cabinet must be in the mapping and every mapping cabinet_id must
    be a present grid cell, else ValueError (caller -> invalid_input).

    Uniform mode (screen_mapping is None): geometry from screen_resolution / grid,
    uniform cabinet_size_mm, placement rect = (col*cw, row*ch, cw, ch).
    """
    from lmt_vba_sidecar.board_layout import cabinet_name
    sw, sh = screen_resolution
    present = [(col, row) for row in range(rows) for col in range(cols)
               if (col, row) not in absent]

    if screen_mapping is not None:
        sm_by_name = {c.cabinet_id: c for c in screen_mapping.cabinets}
        present_names = {cabinet_name(col, row) for (col, row) in present}
        missing = sorted(present_names - set(sm_by_name))
        if missing:
            raise ValueError(
                f"screen_mapping is missing {len(missing)} present cabinet(s): "
                f"{missing}. With --screen-mapping every present cabinet must be "
                f"described (single source of truth).")
        extra = sorted(set(sm_by_name) - present_names)
        if extra:
            raise ValueError(
                f"screen_mapping has {len(extra)} cabinet id(s) that are not "
                f"present grid cells (stale/misspelled or absent): {extra}.")
        specs: list[dict] = []
        for (col, row) in present:
            cab = sm_by_name[cabinet_name(col, row)]
            x, y, w, h = cab.input_rect_px
            specs.append({
                "col": col, "row": row,
                "resolution_px": (cab.resolution_px[0], cab.resolution_px[1]),
                "pixel_pitch_mm": (cab.pixel_pitch_mm[0], cab.pixel_pitch_mm[1]),
                "input_rect_px": (x, y, w, h),
            })
        return specs

    # Uniform mode.
    uni_w, uni_h = sw // cols, sh // rows
    uni_pitch = (cabinet_size_mm[0] / uni_w, cabinet_size_mm[1] / uni_h)
    return [{
        "col": col, "row": row,
        "resolution_px": (uni_w, uni_h),
        "pixel_pitch_mm": uni_pitch,
        "input_rect_px": (col * uni_w, row * uni_h, uni_w, uni_h),
    } for (col, row) in present]
```

- [ ] **Step 4: Run test to verify it passes**

Run: `cd python-sidecar && .venv/bin/python -m pytest tests/test_pattern_specs.py -v`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add python-sidecar/src/lmt_vba_sidecar/pattern.py python-sidecar/tests/test_pattern_specs.py
git commit -m "feat(vba): per-cabinet specs w/ input_rect + exact screen_mapping coverage"
```

### Task 2.3: Rewire `run_generate_pattern` to per-cabinet + v2 meta

**Files:**
- Modify: `python-sidecar/src/lmt_vba_sidecar/pattern.py:142-245` (`run_generate_pattern`, `_preflight_capacity`, `_assemble_screen`)
- Test: `python-sidecar/tests/test_generate_pattern_e2e.py` (create)

- [ ] **Step 1: Write the failing test**

```python
# python-sidecar/tests/test_generate_pattern_e2e.py
import json, pathlib
import cv2
import numpy as np
from lmt_vba_sidecar.ipc import GeneratePatternInput
from lmt_vba_sidecar.pattern import run_generate_pattern


def test_uniform_generation_writes_v2_meta(tmp_path: pathlib.Path):
    out = tmp_path / "patterns" / "BENCH"
    cmd = GeneratePatternInput.model_validate({
        "command": "generate_pattern", "version": 1,
        "project": {"screen_id": "BENCH",
                    "cabinet_array": {"cols": 1, "rows": 2, "cabinet_size_mm": [300.0, 300.0]}},
        "output_dir": str(out), "screen_resolution": [1080, 2160],
    })
    assert run_generate_pattern(cmd) == 0
    meta = json.loads((out / "pattern_meta.json").read_text())
    assert meta["schema_version"] == 2
    assert len(meta["cabinets"]) == 2
    c0 = meta["cabinets"][0]
    assert {"col", "row", "aruco_id_start", "aruco_id_end",
            "squares_x", "squares_y", "square_px", "pixel_pitch_mm"} <= set(c0)
    # square cabinet reproduces legacy 9x9/40 markers
    assert (c0["squares_x"], c0["squares_y"]) == (9, 9)
    assert c0["aruco_id_end"] - c0["aruco_id_start"] + 1 == 40
    # contiguous, non-overlapping id blocks
    assert meta["cabinets"][1]["aruco_id_start"] == meta["cabinets"][0]["aruco_id_end"] + 1
    assert (out / "cabinets" / "V000_R000.png").exists()
    assert (out / "full_screen.png").exists()


def _write_screen_mapping(path, cabs):
    # cabs: list of (cid, res, size, pitch, rect)
    path.write_text(json.dumps({
        "screen_id": "BENCH", "expected_pattern_hash": "x",
        "cabinets": [{
            "cabinet_id": cid, "resolution_px": res, "active_size_mm": size,
            "pixel_pitch_mm": pitch, "active_origin": "center",
            "input_rect_px": rect, "rotation": 0, "mirror_x": False, "mirror_y": False,
        } for (cid, res, size, pitch, rect) in cabs]}))


def test_screen_mapping_unequal_cabinets_assemble_at_input_rect(tmp_path: pathlib.Path):
    # Two UNEQUAL cabinets stacked with a 40px gap: wide 1280x720 on top,
    # square 720x720 below. Boards must land at their input_rect_px, NOT a uniform grid.
    out = tmp_path / "patterns" / "BENCH"
    sm = tmp_path / "screen_mapping.json"
    _write_screen_mapping(sm, [
        ("V000_R000", [1280, 720], [400.0, 225.0], [0.3125, 0.3125], [0, 0, 1280, 720]),
        ("V000_R001", [720, 720], [225.0, 225.0], [0.3125, 0.3125], [0, 760, 720, 720]),
    ])
    cmd = GeneratePatternInput.model_validate({
        "command": "generate_pattern", "version": 1,
        "project": {"screen_id": "BENCH",
                    "cabinet_array": {"cols": 1, "rows": 2, "cabinet_size_mm": [300.0, 300.0]}},
        "output_dir": str(out), "screen_resolution": [1280, 1480],
        "screen_mapping_path": str(sm),
    })
    assert run_generate_pattern(cmd) == 0
    meta = json.loads((out / "pattern_meta.json").read_text())
    by = {(c["col"], c["row"]): c for c in meta["cabinets"]}
    assert (by[(0, 0)]["squares_x"], by[(0, 0)]["squares_y"]) == (16, 9)   # 1280x720 wide
    assert (by[(0, 1)]["squares_x"], by[(0, 1)]["squares_y"]) == (9, 9)    # 720x720 square
    full = cv2.imread(str(out / "full_screen.png"), cv2.IMREAD_GRAYSCALE)
    assert full.shape == (1480, 1280)
    # The 40px gap row (y in [720,760)) is untouched background (all white) — proof
    # the lower board was placed at its input_rect y=760, not at a uniform y=740 cell.
    assert (full[730:758, :] == 255).all()


def test_screen_mapping_missing_cabinet_is_invalid_input(tmp_path: pathlib.Path, capsys):
    out = tmp_path / "patterns" / "BENCH"
    sm = tmp_path / "screen_mapping.json"
    _write_screen_mapping(sm, [  # grid is 1x2 but only one cabinet described
        ("V000_R000", [720, 720], [225.0, 225.0], [0.3125, 0.3125], [0, 0, 720, 720]),
    ])
    cmd = GeneratePatternInput.model_validate({
        "command": "generate_pattern", "version": 1,
        "project": {"screen_id": "BENCH",
                    "cabinet_array": {"cols": 1, "rows": 2, "cabinet_size_mm": [225.0, 225.0]}},
        "output_dir": str(out), "screen_resolution": [720, 1440],
        "screen_mapping_path": str(sm),
    })
    assert run_generate_pattern(cmd) == 1
    cap = capsys.readouterr()
    err = cap.out + cap.err
    assert "invalid_input" in err and "V000_R001" in err
    assert not out.exists()  # nothing published on failure
```

> Note on `capsys`: the sidecar writes envelopes via `write_event` to stdout/stderr; the existing tests use the same capture pattern — match whichever stream `io_utils.write_event` uses (check `test_*` that already assert on `code":"invalid_input"`). If those tests read the event objects instead, mirror that.

- [ ] **Step 2: Run test to verify it fails**

Run: `cd python-sidecar && .venv/bin/python -m pytest tests/test_generate_pattern_e2e.py -v`
Expected: FAIL — meta has no `schema_version`/per-cabinet geometry; `generate_cabinet_png` signature changed; assembly ignores `input_rect_px`; missing-cabinet does not error.

- [ ] **Step 3: Implement**

Rewrite the body of `run_generate_pattern` (`pattern.py:142-245`). Key changes (keep the atomic-staging swap block `:165-234` and the final `ResultEvent` `:236-244` unchanged):

```python
def run_generate_pattern(cmd: GeneratePatternInput) -> int:
    out_dir = pathlib.Path(cmd.output_dir)
    cols = cmd.project.cabinet_array.cols
    rows = cmd.project.cabinet_array.rows
    absent = set(tuple(c) for c in cmd.project.cabinet_array.absent_cells)
    sw, sh = cmd.screen_resolution

    # Optional per-cabinet geometry from screen_mapping (single source of truth).
    screen_mapping = None
    if cmd.screen_mapping_path is not None:
        from lmt_vba_sidecar.screen_mapping import ScreenMapping
        screen_mapping = ScreenMapping.model_validate_json(
            pathlib.Path(cmd.screen_mapping_path).read_text())

    # Even-divisibility only constrains the UNIFORM path; in --screen-mapping
    # mode the per-cabinet input_rect_px defines placement (DD6), so divisibility
    # is irrelevant.
    if screen_mapping is None and (sw % cols != 0 or sh % rows != 0):
        write_event(ErrorEvent(event="error", code="invalid_input",
            message=f"screen_resolution {sw}x{sh} must divide evenly by grid {cols}x{rows}",
            fatal=True))
        return 1

    # Resolve specs; exact-coverage / bad-mapping errors -> invalid_input (DD1a).
    try:
        specs = _resolve_cabinet_specs(
            cols=cols, rows=rows, absent=absent,
            screen_resolution=(sw, sh), screen_mapping=screen_mapping,
            cabinet_size_mm=list(cmd.project.cabinet_array.cabinet_size_mm),
        )
    except ValueError as exc:
        write_event(ErrorEvent(event="error", code="invalid_input",
            message=str(exc), fatal=True))
        return 1

    # Choose per-cabinet board shape and run the capacity check on the real total.
    from lmt_vba_sidecar.board_layout import choose_board_shape, markers_per_board
    for s in specs:
        s["squares_x"], s["squares_y"], s["square_px"] = choose_board_shape(
            resolution_px=s["resolution_px"])
    total_markers = sum(markers_per_board(s["squares_x"], s["squares_y"]) for s in specs)
    if total_markers > ARUCO_DICT_CAPACITY:
        write_event(ErrorEvent(event="error", code="invalid_input",
            message=(f"grid needs {total_markers} ArUco IDs across {len(specs)} cabinets, "
                     f"exceeds {DEFAULT_ARUCO_DICT} capacity ({ARUCO_DICT_CAPACITY}); "
                     f"use fewer/larger cabinets or structured light"),
            fatal=True))
        return 1

    # DD6: every board must fit inside its placement rect, and every rect inside
    # the screen. Catches a board wider than its input_rect or rects that spill
    # past screen_resolution before any file is written.
    for s in specs:
        bw, bh = s["squares_x"] * s["square_px"], s["squares_y"] * s["square_px"]
        rx, ry, rw, rh = s["input_rect_px"]
        if bw > rw or bh > rh:
            write_event(ErrorEvent(event="error", code="invalid_input",
                message=(f"cabinet V{s['col']:03d}_R{s['row']:03d} board {bw}x{bh}px "
                         f"does not fit its input_rect {rw}x{rh}px"), fatal=True))
            return 1
        if rx < 0 or ry < 0 or rx + rw > sw or ry + rh > sh:
            write_event(ErrorEvent(event="error", code="invalid_input",
                message=(f"cabinet V{s['col']:03d}_R{s['row']:03d} input_rect "
                         f"[{rx},{ry},{rw},{rh}] spills past screen {sw}x{sh}"), fatal=True))
            return 1

    out_dir.parent.mkdir(parents=True, exist_ok=True)
    staging = pathlib.Path(tempfile.mkdtemp(
        prefix=f".{out_dir.name}-staging-", dir=str(out_dir.parent)))
    cabinets_dir = staging / "cabinets"
    cabinets_dir.mkdir(parents=True)

    try:
        cabinets_meta: list[PatternMetaCabinet] = []
        next_id = 0
        total = len(specs)
        for i, s in enumerate(specs):
            col, row = s["col"], s["row"]
            tile = cabinets_dir / f"V{col:03d}_R{row:03d}.png"
            id_start = next_id
            next_id = generate_cabinet_png(
                out_path=tile, aruco_id_start=id_start,
                squares_x=s["squares_x"], squares_y=s["squares_y"], square_px=s["square_px"])
            cabinets_meta.append(PatternMetaCabinet(
                col=col, row=row, aruco_id_start=id_start, aruco_id_end=next_id - 1,
                squares_x=s["squares_x"], squares_y=s["squares_y"], square_px=s["square_px"],
                pixel_pitch_mm=[s["pixel_pitch_mm"][0], s["pixel_pitch_mm"][1]]))
            write_event(ProgressEvent(event="progress", stage="output",
                percent=(i + 1) / total, message=f"cabinet V{col:03d}_R{row:03d}"))

        _assemble_screen(out_path=staging / "full_screen.png", cabinets_dir=cabinets_dir,
            specs=specs, screen_resolution=(sw, sh))

        meta = PatternMeta(schema_version=2, aruco_dict=DEFAULT_ARUCO_DICT, cabinets=cabinets_meta)
        (staging / "pattern_meta.json").write_text(meta.model_dump_json(indent=2))

        # ... (keep the existing atomic publish block pattern.py:217-231 verbatim) ...
    except Exception:
        shutil.rmtree(staging, ignore_errors=True)
        raise

    write_event(ResultEvent(event="result", data=ResultData(
        measured_points=[], ba_stats=BaStats(rms_reprojection_px=0.0, iterations=0, converged=True),
        frame_strategy_used="nominal_anchoring", procrustes_align_rms_m=0.0)))
    return 0
```

Also update `_assemble_screen` (`pattern.py:90-113`) to place each cabinet board **centered inside its `input_rect_px`** (DD6), not in a derived uniform grid. The board canvas (`squares*square_px`) may be smaller than the rect → center it and leave `ABSENT_CELL_FILL` margin:

```python
def _assemble_screen(*, out_path, cabinets_dir, specs, screen_resolution) -> None:
    sw, sh = screen_resolution
    full = np.full((sh, sw), ABSENT_CELL_FILL, dtype=np.uint8)
    for s in specs:
        tile_path = cabinets_dir / f"V{s['col']:03d}_R{s['row']:03d}.png"
        if not tile_path.exists():
            continue
        tile = cv2.imread(str(tile_path), cv2.IMREAD_GRAYSCALE)
        th, tw = tile.shape
        rx, ry, rw, rh = s["input_rect_px"]        # placement rect (DD6)
        x0 = rx + (rw - tw) // 2                    # center board in its rect
        y0 = ry + (rh - th) // 2
        full[y0:y0 + th, x0:x0 + tw] = tile
    cv2.imwrite(str(out_path), full)
```

(Placement is validated in `run_generate_pattern` before this runs — board fits rect, rect fits screen — so the slice is always in-bounds.)

Delete the now-unused `_preflight_capacity` (`pattern.py:120-139`) — its job moved inline (the per-cabinet `total_markers` check above).

- [ ] **Step 4: Run test to verify it passes**

Run: `cd python-sidecar && .venv/bin/python -m pytest tests/test_generate_pattern_e2e.py -v`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add python-sidecar/src/lmt_vba_sidecar/pattern.py python-sidecar/tests/test_generate_pattern_e2e.py
git commit -m "feat(vba): per-cabinet pitch-matched generation + pattern_meta v2"
```

---

## Phase 3 — Local-mm (`screen_mapping.py`)

### Task 3.1: Pitch-based per-cabinet `charuco_corner_local_mm`

**Files:**
- Modify: `python-sidecar/src/lmt_vba_sidecar/screen_mapping.py:112-170`
- Test: `python-sidecar/tests/test_local_mm_per_cabinet.py` (create)

- [ ] **Step 1: Write the failing test**

```python
# python-sidecar/tests/test_local_mm_per_cabinet.py
import numpy as np
from lmt_vba_sidecar.screen_mapping import ScreenMapping, ScreenMappingCabinet


def _sm():
    return ScreenMapping(
        screen_id="BENCH", expected_pattern_hash="x",
        cabinets=[ScreenMappingCabinet(
            cabinet_id="V000_R000", resolution_px=[960, 540],
            active_size_mm=[300.0, 168.75], pixel_pitch_mm=[0.3125, 0.3125],
            active_origin="center", input_rect_px=[0, 0, 960, 540],
            rotation=0, mirror_x=False, mirror_y=False)],
    )


def test_local_mm_uses_pitch_and_nonsquare_board():
    sm = _sm()
    # board 16x9, square_px=60 -> board 960x540 px, inner 15x8
    # charuco_id 0 = inner (r=0,c=0): px=((0+1)*60,(0+1)*60)=(60,60)
    # center origin: x=(60-960/2)*0.3125, y=(60-540/2)*0.3125
    p = sm.charuco_corner_local_mm("V000_R000", charuco_id=0,
                                   squares_x=16, squares_y=9, square_px=60)
    assert np.allclose(p, [(60 - 480) * 0.3125, (60 - 270) * 0.3125, 0.0])


def test_center_corner_is_near_origin_for_centered_board():
    sm = _sm()
    inner_x = 15
    mid_id = (inner_x) * (8 // 2) + (inner_x // 2)  # roughly central corner
    p = sm.charuco_corner_local_mm("V000_R000", charuco_id=mid_id,
                                   squares_x=16, squares_y=9, square_px=60)
    assert abs(p[0]) < 300 and abs(p[1]) < 170
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cd python-sidecar && .venv/bin/python -m pytest tests/test_local_mm_per_cabinet.py -v`
Expected: FAIL — signature still `inner=8`, formula square-only.

- [ ] **Step 3: Implement**

Replace `charuco_corner_local_mm` (`screen_mapping.py:112-170`) with:

```python
    def charuco_corner_local_mm(
        self,
        cabinet_id: str,
        charuco_id: int,
        *,
        squares_x: int,
        squares_y: int,
        square_px: int,
    ) -> np.ndarray:
        """Local-mm of a ChArUco inner corner [x, y, 0], pitch-based.

        Origin = board center. +x right, +y down (image convention). Uses the
        cabinet's own pixel pitch so coordinates are exact for any per-cabinet
        size/pitch and any non-square (squares_x != squares_y) board.
        """
        cab = self._cabinet(cabinet_id)
        if cab.rotation != 0 or cab.mirror_x or cab.mirror_y:
            raise ScreenMappingError(
                f"rotation/mirror not supported in local-mm. Cabinet '{cabinet_id}' "
                f"rotation={cab.rotation} mirror_x={cab.mirror_x} mirror_y={cab.mirror_y}.")

        pitch_x, pitch_y = cab.pixel_pitch_mm
        inner_x = squares_x - 1            # corners per row
        r, c = divmod(charuco_id, inner_x)  # ChArUco numbers L-R, T-B
        board_w_px = squares_x * square_px
        board_h_px = squares_y * square_px
        x_px = (c + 1) * square_px
        y_px = (r + 1) * square_px
        x_mm = (x_px - board_w_px / 2.0) * pitch_x
        y_mm = (y_px - board_h_px / 2.0) * pitch_y
        return np.array([x_mm, y_mm, 0.0], dtype=float)
```

Update the module docstring (`screen_mapping.py:10-24`) to describe the pitch-based convention (the old `active_size/(inner+1)` note is obsolete).

- [ ] **Step 4: Run test to verify it passes**

Run: `cd python-sidecar && .venv/bin/python -m pytest tests/test_local_mm_per_cabinet.py -v`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add python-sidecar/src/lmt_vba_sidecar/screen_mapping.py python-sidecar/tests/test_local_mm_per_cabinet.py
git commit -m "feat(vba): pitch-based per-cabinet local-mm (non-square boards)"
```

---

## Phase 4 — Detection: one scan + routing (`detect.py`)

### Task 4.1: Single-pass detect + marker routing + per-cabinet interpolation

**Files:**
- Modify: `python-sidecar/src/lmt_vba_sidecar/detect.py:15-100`
- Test: `python-sidecar/tests/test_detect_routing.py` (create)

- [ ] **Step 1: Write the failing test**

This test renders a known 2-cabinet pattern, photographs it virtually (flat, fronto-parallel), and asserts every detected corner is routed to the correct cabinet and `charuco_id`.

```python
# python-sidecar/tests/test_detect_routing.py
import pathlib
import cv2
import numpy as np
from lmt_vba_sidecar.pattern import generate_cabinet_png
from lmt_vba_sidecar.board_layout import markers_per_board
from lmt_vba_sidecar.detect import detect_charuco_corners


def test_detect_routes_two_cabinets(tmp_path: pathlib.Path):
    # Build a 1x2 vertical screen image from two square boards.
    b0 = tmp_path / "V000_R000.png"
    b1 = tmp_path / "V000_R001.png"
    n0 = generate_cabinet_png(out_path=b0, aruco_id_start=0,
                              squares_x=9, squares_y=9, square_px=80)
    generate_cabinet_png(out_path=b1, aruco_id_start=n0,
                         squares_x=9, squares_y=9, square_px=80)
    img0 = cv2.imread(str(b0), cv2.IMREAD_GRAYSCALE)
    img1 = cv2.imread(str(b1), cv2.IMREAD_GRAYSCALE)
    screen = np.full((img0.shape[0] + img1.shape[0] + 40, img0.shape[1] + 40), 255, np.uint8)
    screen[20:20 + img0.shape[0], 20:20 + img0.shape[1]] = img0
    y1 = 20 + img0.shape[0]
    screen[y1:y1 + img1.shape[0], 20:20 + img1.shape[1]] = img1
    shot = tmp_path / "v001.png"
    cv2.imwrite(str(shot), screen)

    boards = [
        {"cabinet": (0, 0), "aruco_id_start": 0, "aruco_id_end": n0 - 1,
         "squares_x": 9, "squares_y": 9},
        {"cabinet": (0, 1), "aruco_id_start": n0, "aruco_id_end": n0 + markers_per_board(9, 9) - 1,
         "squares_x": 9, "squares_y": 9},
    ]
    dets = detect_charuco_corners([str(shot)], boards=boards)[str(shot)]
    cabs = {d["cabinet"] for d in dets}
    assert (0, 0) in cabs and (0, 1) in cabs
    # No corner is misrouted: every charuco_id is within its board's inner-corner count
    for d in dets:
        assert 0 <= d["charuco_id"] < (9 - 1) * (9 - 1)
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cd python-sidecar && .venv/bin/python -m pytest tests/test_detect_routing.py -v`
Expected: FAIL — `boards` descriptors now carry `squares_x/squares_y`/`aruco_id_end`; current `detect.py` expects `inner_corners` and re-scans per board.

- [ ] **Step 3: Implement**

Replace `_charuco_board` + `detect_charuco_corners` (`detect.py:15-100`) with a single-scan + routing implementation:

```python
def _charuco_board(aruco_id_start: int, squares_x: int, squares_y: int):
    aruco_dict = _aruco_dict()
    from lmt_vba_sidecar.board_layout import markers_per_board
    n_markers = markers_per_board(squares_x, squares_y)
    sub_dict = cv2.aruco.Dictionary(
        aruco_dict.bytesList[aruco_id_start:aruco_id_start + n_markers],
        aruco_dict.markerSize)
    return cv2.aruco.CharucoBoard(
        size=(squares_x, squares_y), squareLength=1.0, markerLength=0.7,
        dictionary=sub_dict)


def detect_charuco_corners(image_paths, *, boards=None, board_lookup_for_test=False):
    """One detectMarkers pass per image; route each marker to its cabinet via a
    precomputed marker_id->cabinet map; per-cabinet interpolate ChArUco corners.

    boards item: {"cabinet": (col,row), "aruco_id_start", "aruco_id_end",
                  "squares_x", "squares_y"}.
    """
    from lmt_vba_sidecar.board_layout import build_marker_routing
    if board_lookup_for_test:
        boards = [{"cabinet": (0, 0), "aruco_id_start": 0, "aruco_id_end": 39,
                   "squares_x": 9, "squares_y": 9}]
    if not boards:
        return {path: [] for path in image_paths}

    routing = build_marker_routing(boards)  # global marker_id -> (col,row)
    # Per cabinet: a local board + the global id offset (to localize marker ids)
    cab_board = {}
    for b in boards:
        cr = tuple(b["cabinet"])
        cab_board[cr] = {
            "board": _charuco_board(b["aruco_id_start"], b["squares_x"], b["squares_y"]),
            "offset": b["aruco_id_start"],
        }

    dictionary = _aruco_dict()
    detector = cv2.aruco.ArucoDetector(dictionary, cv2.aruco.DetectorParameters())

    out = {}
    for path in image_paths:
        img = cv2.imread(path, cv2.IMREAD_GRAYSCALE)
        if img is None:
            out[path] = []
            continue
        corners, ids, _ = detector.detectMarkers(img)  # ONE scan
        observations = []
        if ids is not None:
            # Bucket detected markers by cabinet using the routing map.
            buckets = {}  # (col,row) -> ([corners],[local_ids])
            for mc, mid in zip(corners, ids.flatten()):
                cr = routing.get(int(mid))
                if cr is None:
                    continue
                local_id = int(mid) - cab_board[cr]["offset"]
                buckets.setdefault(cr, ([], [])) [0].append(mc)
                buckets[cr][1].append(local_id)
            for cr, (mcs, lids) in buckets.items():
                board = cab_board[cr]["board"]
                n, ch_corners, ch_ids = cv2.aruco.interpolateCornersCharuco(
                    mcs, np.array(lids, dtype=np.int32).reshape(-1, 1), img, board)
                if ch_ids is None:
                    continue
                for cid, (cx, cy) in zip(ch_ids.flatten(), ch_corners.reshape(-1, 2)):
                    observations.append({
                        "cabinet": cr, "charuco_id": int(cid),
                        "corner_px": [float(cx), float(cy)]})
        out[path] = observations
    return out
```

Add `import numpy as np` at the top of `detect.py`.

- [ ] **Step 4: Run test to verify it passes**

Run: `cd python-sidecar && .venv/bin/python -m pytest tests/test_detect_routing.py -v`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add python-sidecar/src/lmt_vba_sidecar/detect.py python-sidecar/tests/test_detect_routing.py
git commit -m "perf(vba): single-pass detect + O(1) marker routing + per-cabinet interpolation"
```

---

## Phase 5 — Reconstruct wiring (`reconstruct.py`)

### Task 5.1: Build per-cabinet board descriptors + route local-mm with per-cabinet shape

**Files:**
- Modify: `python-sidecar/src/lmt_vba_sidecar/reconstruct.py:258-292` (board build + local-mm call)
- Test: extend the existing reconstruct test (`python-sidecar/tests/` — find with `grep -rl run_reconstruct python-sidecar/tests`)

- [ ] **Step 1: Write the failing test**

Add to the located reconstruct test file (or create `tests/test_reconstruct_per_cabinet.py`): a smoke test that a v2 `pattern_meta.json` + matching `screen_mapping.json` produces a non-empty `measured.yaml` with corner names of the form `<screen>_V###_R###_C###`.

```python
# python-sidecar/tests/test_reconstruct_per_cabinet.py
# Uses the simulate path (Level 0A) to avoid needing real images.
# See tests/test_api_test or the existing simulate fixtures for the dataset shape.
def test_reconstruct_routes_per_cabinet_inner(monkeypatch, tmp_path):
    # Build a 1x2 pattern_meta v2 (square boards) + screen_mapping with matching
    # squares, run detect on a synthesized fronto-parallel screenshot, and assert
    # every observation's local-mm came from the per-cabinet (squares_x, squares_y).
    # (Detailed dataset construction mirrors test_detect_routing.py Step 1.)
    ...
```

> Implementation note for the engineer: reuse the screenshot-synthesis from `test_detect_routing.py` Step 1 and drive `run_reconstruct` with a `capture_manifest.json` + `screen_mapping.json` whose cabinet ids are `V000_R000`/`V000_R001`. Assert `measured.yaml` point names match `^BENCH_V\d{3}_R\d{3}_C\d{3}$`.

- [ ] **Step 2: Run test to verify it fails**

Run: `cd python-sidecar && .venv/bin/python -m pytest tests/test_reconstruct_per_cabinet.py -v`
Expected: FAIL — `reconstruct.py` builds boards with global `inner` and calls `charuco_corner_local_mm(..., inner=inner)`.

- [ ] **Step 3: Implement**

In `reconstruct.py`, replace the board-descriptor build (`:263-265`) with per-cabinet shape from v2 meta:

```python
    boards = [
        {"cabinet": (c.col, c.row),
         "aruco_id_start": c.aruco_id_start, "aruco_id_end": c.aruco_id_end,
         "squares_x": c.squares_x, "squares_y": c.squares_y}
        for c in pattern_meta.cabinets
    ]
    # quick lookup col,row -> (squares_x, squares_y, square_px)
    shape_by_cr = {(c.col, c.row): (c.squares_x, c.squares_y, c.square_px)
                   for c in pattern_meta.cabinets}
```

Replace the local-mm call (`:286-289`) with:

```python
                charuco_id = int(det["charuco_id"])
                sx, sy, spx = shape_by_cr[cab_cr]
                p_local = screen_mapping.charuco_corner_local_mm(
                    _cabinet_id(*cab_cr), charuco_id,
                    squares_x=sx, squares_y=sy, square_px=spx)
```

Remove the now-dead `inner = pattern_meta.checkerboard_inner_corners` lookup earlier in `run_reconstruct` (search for `checkerboard_inner_corners` in `reconstruct.py` and delete that line + the root-cabinet `inner` usage). Update the measured-point naming where points are emitted (search `_cabinet_id` near `:377-390`) to use `board_layout.corner_name(screen_id, col, row, charuco_id)` if it currently uses a bare cabinet id.

- [ ] **Step 4: Run test to verify it passes**

Run: `cd python-sidecar && .venv/bin/python -m pytest tests/test_reconstruct_per_cabinet.py tests/ -v`
Expected: PASS (new test + no regressions in the existing suite).

- [ ] **Step 5: Commit**

```bash
git add python-sidecar/src/lmt_vba_sidecar/reconstruct.py python-sidecar/tests/test_reconstruct_per_cabinet.py
git commit -m "feat(vba): reconstruct routes per-cabinet board shape into local-mm"
```

### Task 5.2: Full Python suite green

- [ ] **Step 1: Run the whole sidecar suite**

Run: `cd python-sidecar && .venv/bin/python -m pytest -q`
Expected: PASS. Fix any test that still constructs v1 `PatternMeta` (no `schema_version`) or calls the old `generate_cabinet_png`/`charuco_corner_local_mm` signatures — update them to v2.

- [ ] **Step 2: Commit any fixups**

```bash
git add python-sidecar
git commit -m "test(vba): migrate remaining tests to PatternMeta v2 signatures"
```

---

## Phase 6 — Rust adapter + CLI plumbing

### Task 6.1: Mirror PatternMeta v2 in Rust

**Files:**
- Modify: `crates/adapter-visual-ba/src/ipc.rs` (find `struct PatternMeta` / `PatternMetaCabinet`)
- Test: `crates/adapter-visual-ba/tests/` (add a serde roundtrip; mirror an existing ipc test)

- [ ] **Step 1: Write the failing test**

```rust
// crates/adapter-visual-ba/tests/pattern_meta_v2_test.rs
use lmt_adapter_visual_ba::ipc::PatternMeta;

#[test]
fn deserializes_v2_pattern_meta() {
    let json = r#"{"schema_version":2,"aruco_dict":"DICT_6X6_1000",
      "cabinets":[{"col":0,"row":0,"aruco_id_start":0,"aruco_id_end":39,
        "squares_x":9,"squares_y":9,"square_px":120,"pixel_pitch_mm":[0.2778,0.2778]}]}"#;
    let meta: PatternMeta = serde_json::from_str(json).unwrap();
    assert_eq!(meta.schema_version, 2);
    assert_eq!(meta.cabinets[0].squares_x, 9);
}
```

(Adjust the `use` path to the crate's actual module visibility — check how existing tests import `ipc`.)

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test -p lmt-adapter-visual-ba pattern_meta_v2 -- --nocapture`
Expected: FAIL — fields missing.

- [ ] **Step 3: Implement**

Update the Rust `PatternMeta`/`PatternMetaCabinet` to match v2 (remove `markers_per_cabinet`/`checkerboard_inner_corners`, add per-cabinet fields):

```rust
#[derive(Debug, Clone, serde::Deserialize, serde::Serialize)]
pub struct PatternMetaCabinet {
    pub col: i32,
    pub row: i32,
    pub aruco_id_start: i32,
    pub aruco_id_end: i32,
    pub squares_x: i32,
    pub squares_y: i32,
    pub square_px: i32,
    pub pixel_pitch_mm: [f64; 2],
}

#[derive(Debug, Clone, serde::Deserialize, serde::Serialize)]
pub struct PatternMeta {
    pub schema_version: i32,
    pub aruco_dict: String,
    pub cabinets: Vec<PatternMetaCabinet>,
}
```

Fix `api.rs:286-293` (the `generate_pattern` readback) if it referenced the removed fields (e.g. it may compute counts from `markers_per_cabinet`); compute counts from `cabinets.len()` / per-cabinet markers instead.

- [ ] **Step 4: Run test to verify it passes**

Run: `cargo test -p lmt-adapter-visual-ba pattern_meta_v2`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add crates/adapter-visual-ba
git commit -m "feat(adapter-visual-ba): mirror PatternMeta v2"
```

### Task 6.2: Thread `screen_mapping_path` through generate_pattern (adapter)

**Files:**
- Modify: `crates/adapter-visual-ba/src/api.rs:247-272` (`GeneratePatternArgs` + payload)

- [ ] **Step 1: Write the failing test**

```rust
// crates/adapter-visual-ba/tests/generate_pattern_payload_test.rs
// Assert the JSON payload carries screen_mapping_path only when Some.
// (Mirror an existing payload-shape test in this crate; if none exists,
//  expose a small `build_generate_payload(&GeneratePatternArgs) -> serde_json::Value`
//  pure fn and test it directly.)
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test -p lmt-adapter-visual-ba generate_pattern_payload`
Expected: FAIL.

- [ ] **Step 3: Implement**

In `api.rs`, add the field and forward it (mirror the existing `reconstruct` `screen_mapping_path` handling at `api.rs:135-138`):

```rust
pub struct GeneratePatternArgs {
    pub output_dir: String,
    pub cabinet_array: IpcCabinetArray,
    pub screen_resolution: [u32; 2],
    pub screen_mapping_path: Option<String>,   // NEW
}
```

In the payload build (`api.rs:265-272`):

```rust
    let mut payload = json!({
        "command": "generate_pattern",
        "version": 1,
        "project": { "screen_id": args.screen_id, "cabinet_array": &args.cabinet_array },
        "output_dir": args.output_dir,
        "screen_resolution": args.screen_resolution,
    });
    if let Some(p) = &args.screen_mapping_path {
        payload["screen_mapping_path"] = json!(p);
    }
```

- [ ] **Step 4: Run test to verify it passes**

Run: `cargo test -p lmt-adapter-visual-ba generate_pattern_payload`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add crates/adapter-visual-ba
git commit -m "feat(adapter-visual-ba): forward optional screen_mapping_path to generate_pattern"
```

### Task 6.3: `--screen-mapping` CLI flag on `generate-pattern`

**Files:**
- Modify: `crates/lmt-cli/src/cli.rs:278-287` (the `GeneratePattern` clap struct)
- Modify: `crates/lmt-cli/src/commands/visual.rs` (the generate-pattern handler — wire the flag into `GeneratePatternArgs`, resolve relative path against project root like `reconstruct` does for `--capture-manifest`)

- [ ] **Step 1: Write the failing test** — see Task 7.2 (CLI E2E). Implement the flag here, assert via E2E.

- [ ] **Step 2: Implement**

Add to the clap struct:

```rust
        /// Optional screen_mapping.json; when set, per-cabinet board geometry
        /// (size/pitch) drives generation instead of the uniform grid.
        #[arg(long)]
        screen_mapping: Option<String>,
```

In `commands/visual.rs`, resolve the path (project-root-relative, matching how `reconstruct` resolves `--capture-manifest`) and set `GeneratePatternArgs.screen_mapping_path`.

- [ ] **Step 3: Build**

Run: `cargo build -p lmt-cli`
Expected: compiles.

- [ ] **Step 4: Manual smoke**

Run: `./target/debug/lmt visual generate-pattern --help`
Expected: shows `--screen-mapping <SCREEN_MAPPING>`.

- [ ] **Step 5: Commit**

```bash
git add crates/lmt-cli/src/cli.rs crates/lmt-cli/src/commands/visual.rs
git commit -m "feat(lmt-cli): generate-pattern --screen-mapping flag"
```

---

## Phase 7 — Contract, docs, E2E, self-check

### Task 7.1: Update `docs/agents-cli.md`

**Files:**
- Modify: `docs/agents-cli.md` (the `visual generate-pattern` row)

- [ ] **Step 1: Edit** — update the `generate-pattern` row to document `--screen-mapping` and that, when supplied, boards are pitch-matched per cabinet (non-square allowed); note `pattern_meta.json` is now schema v2.

- [ ] **Step 2: Commit**

```bash
git add docs/agents-cli.md
git commit -m "docs(cli): document generate-pattern --screen-mapping + pattern_meta v2"
```

### Task 7.2: CLI E2E — happy + invalid-input

**Files:**
- Modify: `crates/lmt-cli/tests/cli_e2e.rs` (add cases; mirror existing `visual generate-pattern` test)

- [ ] **Step 1: Write the failing tests**

- happy (uniform): `generate-pattern --yes` (no `--screen-mapping`) exits 0 and writes `pattern_meta.json` with `"schema_version": 2` and per-cabinet `squares_x`.
- happy (mapped, unequal cabinets): `generate-pattern --screen-mapping <fixture> --yes` with a 2-cabinet `screen_mapping.json` where the two cabinets have **different** `resolution_px` and non-uniform `input_rect_px` → exits 0; assert the v2 meta has the two different `squares_x` values.
- invalid (over capacity): a `screen_mapping.json`/grid needing > 1000 markers → exit code for `invalid_input`, envelope `code:"invalid_input"`.
- invalid (missing cabinet): a 1×2 grid with a `screen_mapping.json` describing only `V000_R000` → `invalid_input`, message names the missing `V000_R001` (validates DD1a end-to-end).

Use `assert_cmd`/the crate's existing E2E harness; build a tmp project with `project.yaml` + the `screen_mapping.json` fixtures. The unequal-cabinet + missing-cabinet cases are the regression guards for the two Codex review findings.

- [ ] **Step 2: Run to verify they fail**

Run: `cargo test -p lmt-cli --test cli_e2e generate_pattern -- --nocapture`
Expected: FAIL.

- [ ] **Step 3: Implement** — fixtures + assertions (no product code change expected; this validates Phases 2/6).

- [ ] **Step 4: Run to verify pass**

Run: `cargo test -p lmt-cli --test cli_e2e generate_pattern`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add crates/lmt-cli/tests/cli_e2e.rs
git commit -m "test(lmt-cli): generate-pattern --screen-mapping E2E (happy + invalid_input)"
```

### Task 7.3: Update bench template + regenerate hash note

**Files:**
- Modify: `docs/poc/monitor-bench-report-template.md` §4.1 / §6 (generate step)

- [ ] **Step 1: Edit** — change the generate step to:
`lmt --yes visual generate-pattern $BENCH BENCH --method charuco --screen-mapping $BENCH/screen_mapping.json`
and add a one-line note: per-cabinet pitch now drives board size, so `screen_mapping.json` must be filled (S_A/S_B/pitch) **before** generate-pattern, and the pattern hash must be refreshed afterward (§3).

- [ ] **Step 2: Commit**

```bash
git add docs/poc/monitor-bench-report-template.md
git commit -m "docs(bench): generate-pattern --screen-mapping; pitch-matched boards"
```

### Task 7.4: Workspace self-check (project CLI contract)

- [ ] **Step 1: Run the contract self-checks (CLAUDE.md)**

```bash
cargo test --workspace
cd python-sidecar && .venv/bin/python -m pytest -q && cd ..
./target/debug/lmt --json schema | jq .            # DTO dump still valid
./target/debug/lmt visual generate-pattern --help  # flag present, prose readable
```
Expected: all green; `--screen-mapping` shown; schema dump parses.

- [ ] **Step 2: If any `lmt-shared` DTO is affected** (PatternMeta is internal to `adapter-visual-ba`, not `lmt-shared` — confirm with `grep -rn "PatternMeta" crates/lmt-shared`; if it appears there, add `schemars::JsonSchema` derive + register in `schema::dump_all()` per CLAUDE.md). If not present in `lmt-shared`, no schema action needed — note this in the commit.

- [ ] **Step 3: Commit**

```bash
git add -A
git commit -m "chore(vba): contract self-check pass for pattern_meta v2 + screen-mapping flag"
```

---

## Self-Review

**1. Spec coverage**
- Requirement #1 (per-cabinet pitch-matched, per-cabinet-size, single-cabinet ChArUco): DD2 + DD3 + Task 1.1 (shape), Task 2.1 (render), Task 2.2 (specs from screen_mapping), Task 2.3 (wire), Task 3.1 (pitch-based local-mm). Non-square support covered (Task 1.1/2.1 widescreen tests). ✓
- Requirement #2 (excellent ArUco naming + fast reverse-lookup rule): DD4 + Task 1.2 (`build_marker_routing`, `cabinet_name`, `corner_name`), DD5 + Task 4.1 (single scan + O(1) routing), Task 5.1 (names emitted into measured points). ✓
- UE borrow (confirmed non-square + dict-by-need): non-square realized; dict-by-need explicitly deferred in Scope (kept DICT_6X6_1000, per-cabinet capacity check) — documented, not silently dropped. ✓
- **Codex review fix #1 (assembly ignored `input_rect_px`):** DD6 + Task 2.2 (specs carry `input_rect_px`) + Task 2.3 (`_assemble_screen` places by rect, placement validated, divisibility uniform-only) + Task 7.2 unequal-cabinet E2E. ✓
- **Codex review fix #2 (silent uniform fallback for missing cabinets):** DD1a + Task 2.2 (`_resolve_cabinet_specs` exact-coverage `ValueError`) + Task 2.3 (`ValueError`→`invalid_input`) + Task 7.2 missing-cabinet E2E. ✓
- **Self-found fix (cap explosion):** DD2 anchors square count to the short side (`DEFAULT_SQUARES_SHORT=9`) so square cabinets reproduce the legacy 40-marker/25-cabinet budget instead of the 162-marker/~6-cabinet blowup a `MIN_SQUARE_PX`-target would cause; Task 1.1 legacy-equivalence test guards it. ✓

**2. Placeholder scan** — Task 5.1 Step 1 and Task 7.2 Step 1 describe test *construction* by reusing `test_detect_routing.py` Step 1's concrete synthesis rather than re-pasting it; the referenced code is fully written in Task 4.1. Task 6.1/6.2 tests say "mirror existing crate test" because the crate's exact harness must be matched — the production code in those tasks is complete. These are acceptable (they point at concrete, written code), not blanks. No "TODO/handle errors/etc." placeholders in product-code steps.

**3. Type consistency**
- `markers_per_board(squares_x, squares_y)` — same signature in `board_layout.py` (1.1), used in `pattern.py` (2.1), `detect.py` (4.1), Rust readback (6.1). ✓
- `choose_board_shape(resolution_px=..., squares_short=...) -> (squares_x, squares_y, square_px)` — consistent across 1.1 / 2.3 (2.3 calls with default `squares_short`). ✓
- spec dict keys (`col, row, resolution_px, pixel_pitch_mm, input_rect_px`, then `squares_x/squares_y/square_px` added in 2.3) — consistent producer (`_resolve_cabinet_specs` 2.2) → consumer (`run_generate_pattern`/`_assemble_screen` 2.3). ✓
- `charuco_corner_local_mm(cabinet_id, charuco_id, *, squares_x, squares_y, square_px)` — defined 3.1, called 5.1 with `sx, sy, spx`. ✓
- `PatternMetaCabinet` fields (`squares_x/squares_y/square_px/pixel_pitch_mm`) identical in Python (0.1) and Rust (6.1). ✓
- board descriptor dict keys (`cabinet, aruco_id_start, aruco_id_end, squares_x, squares_y`) consistent between `reconstruct.py` (5.1) producer and `detect.py` (4.1) consumer and `build_marker_routing` (1.2). ✓

Note for the engineer (verified, not assumed): the pin is `opencv-contrib-python>=4.8,<5.0`; the worktree resolves **4.11.0**, and `hasattr(cv2.aruco, "interpolateCornersCharuco")` is **True** there (also `CharucoDetector` exists). So Task 4.1's `interpolateCornersCharuco` path works on the current pin — it is deprecated but present. If a future bump to OpenCV 5.x removes it, swap the per-cabinet interpolation for `CharucoDetector(board).detectBoard(img)` fed the routed marker subset; the one-scan routing design is unchanged either way. No action needed now.

---

## Execution Handoff

Plan complete and saved to `docs/superpowers/plans/2026-05-26-per-cabinet-charuco-and-marker-routing.md`. Two execution options:

**1. Subagent-Driven (recommended)** — fresh subagent per task, review between tasks, fast iteration.

**2. Inline Execution** — execute tasks in this session with checkpoints for review.

Which approach?
