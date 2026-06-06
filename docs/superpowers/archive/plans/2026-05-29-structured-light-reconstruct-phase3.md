# Structured-Light Reconstruct (Phase 3) Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Turn N per-pose structured-light `CorrespondenceFile`s into a metric per-cabinet 3D model (`measured.yaml` + `cabinet_pose_report.json`) by feeding screen↔camera correspondences into the SAME model-constrained bundle adjustment the ChArUco path already uses — no total station, scale anchored by per-cabinet pixel pitch.

**Architecture:** Structured-light reconstruction is the existing ChArUco reconstruction with a different observation source. Both reduce to: build `list[Observation(camera_idx, cabinet_idx, p_local_mm, undistorted_pixel)]`, init camera/cabinet poses via PnP, run `model_constrained_ba`, emit per-cabinet geometry. So Phase 3 (a) adds SL-specific front-end glue (provenance validation, `screen (u,v) → cabinet-local mm`, observation assembly from correspondence files), (b) extracts the shared init→BA→output block out of `reconstruct.py` into one behavior-preserving helper both paths call, and (c) wires a dedicated `visual reconstruct-structured-light` CLI under the 6-point contract, reusing the existing `VisualReconstructResult` DTO.

**Tech Stack:** Python sidecar (`opencv-contrib-python` 4.11, `numpy<2`, `scipy<2`, `pydantic` v2, `pytest`); Rust workspace (`lmt-cli` clap CLI, `lmt-app` service layer, `adapter-visual-ba` sidecar IPC, `lmt-shared` DTO/envelope/schema/manifest).

---

## Why this is its own plan (not inlined in the Phase 0–2 plan)

The parent plan (`2026-05-29-structured-light-screen-reconstruction.md`) deferred Phase 3 because the solver had to be written against (a) real Phase-2 `CorrespondenceFile` output, (b) the existing `ba.py` / `model_constrained_ba.py`, and (c) the existing `reconstruct.py` orchestration. Those now exist and are verified, so this plan is mechanical. Key discovery while reading the real code: **the existing `run_reconstruct` already does exactly the BA we need** — `model_constrained_ba` solves per-camera SE3 + per-non-root-cabinet SE3 with the root cabinet fixed as the world gauge, scale fixed by metric local-mm coordinates. The ChArUco path gets local mm from `screen_mapping.charuco_corner_local_mm`; the SL path gets it from `sl_meta` cabinet rects + pixel pitch. Everything after observation assembly is identical → reuse, don't reimplement.

---

## Revision Log

**rev2 (post-Codex adversarial review)** — three findings triaged:
1. **[ACCEPTED] sl_meta untrusted/unmatched.** Task 3.4 now `StructuredLightMeta.model_validate_json`s the meta (malformed → `invalid_input`, not an internal-error traceback), asserts `meta.screen_id == project.screen_id`, and asserts the sl_meta cabinet set equals the project present-cell set (`nominal_m.keys()`) — rejecting a stale meta / edited project layout that shares `screen_id`+`sha`. New tests: `test_run_rejects_malformed_sl_meta`, `test_run_rejects_meta_project_cabinet_mismatch`.
2. **[ACCEPTED] corr (u,v) trusted over canonical meta.** Observation assembly now derives `p_local` from the canonical `(u,v)` in `sl_meta.dots` (loaded into `cab_by_id`); only the camera pixel `(x,y)` is read from the correspondence file. The screen-coordinate invariant is now structural. New guard inside the gating test: corrupt every corr `(u,v)` → identical recovered pose.
3. **[REJECTED — would break existing behavior] `MAIN_` name prefix.** Codex proposed `f"{screen_id}_{cid}"`. Verified the existing tests pin `MAIN_` as a fixed literal independent of `screen_id` (`test_reconstruct_per_cabinet.py:16,43`: `screen_id="S"` → `MAIN_V000_R000`), so the change would break the charuco path and exceed Phase-3 scope. The SL path inherits the same (cosmetic) wart consistently; no downstream consumer parses the prefix. Documented in Task 3.3 as a pre-existing wart with a separate-cleanup path. (Container-level `MeasuredPoints.screen_id` is correct on both paths.)

---

## Reuse map (verified against current source)

| Need | Reuse | Location (verified) |
| --- | --- | --- |
| BA solver | `model_constrained_ba(...) -> BAResult{camera_poses, cabinet_poses, rms_reprojection_px, iterations, converged, cabinet_covariances}` | `model_constrained_ba.py:98` |
| Observation record | `Observation(camera_idx, cabinet_idx, p_local, pixel)` | `model_constrained_ba.py:21` |
| Undistort one pixel | `_undistort_obs(pix, K, dist) -> (2,)` | `reconstruct.py:127` |
| Non-root cabinet init (bridge cameras) | `estimate_nonroot_cabinet_init(per_view_cab_corners, root_idx, K)` | `reconstruct.py:501` |
| Per-view camera init via PnP | `_pnp_camera(cam_idx, root_idx, init_cabinets, per_view_cab_corners, K)` | `reconstruct.py:552` |
| Per-cabinet reproj RMS | `_per_cabinet_reproj_rms(K, cam_poses, cab_poses, observations)` | `reconstruct.py:92` |
| Soft quality flag | `_classify_cabinet_quality(views, rms)`, `QUALITY_MIN_VIEWS=4`, `QUALITY_MAX_CABINET_RMS_PX=2.0` | `reconstruct.py:79` |
| Cabinet geometry from pose | `reconstruct_cabinet_geometry(R, t, corners_local) -> (center, normal, size, world_corners)` | `eval_runner.py:21` |
| Nominal cabinet centers (meters) | `nominal_cabinet_centers_model_frame(cabinet_array, shape_prior) -> dict[(col,row)]→center_m` | `nominal.py:92` |
| Observability gate | `check_observability(obs, n_cabinets, min_views=2, min_points=8)` raising `ObservabilityError` | `observability.py` (via `reconstruct.py:62`) |
| Atomic JSON write | `_atomic_write_json(path, payload)` | `reconstruct.py:182` |
| Output DTOs | `CabinetPose`, `CabinetPoseReport`, `FrameSpec`, `MeasuredPoint`, `Uncertainty`, `PointSource`, `PointSourceVisualBa`, `BaStats`, `ResultData`, `ResultEvent` | `ipc.py` |
| Constants | `ROOT_CABINET=(0,0)`, `FALLBACK_ISOTROPIC_M=0.005` | `reconstruct.py:66,68` |
| SL provenance source | `CorrespondenceFile{screen_id, sl_meta_sha256, screen_resolution, camera_image_size, source_input, points[{id,u,v,x,y}]}`; `StructuredLightMeta{screen_id, cabinets[{col,row,input_rect_px,pixel_pitch_mm}], dots[{id,u,v,cabinet}]}` | `ipc.py:191,219` |
| Rust result DTO (REUSED, no new DTO) | `VisualReconstructResult{screen_id, measured_yaml_path, pose_report_path, cabinet_count, ba_rms_px, cabinets:[CabinetPoseSummary]}` | `dto.rs:242` |
| Error codes already wired | `ba_diverged`(14), `observability_failed`(17), `detection_failed`(13), `invalid_input`(3) | `envelope.rs:120-130`, `exit_codes.rs:24-29` |

**Frame-convention invariant (load-bearing):** `model_constrained_ba` reprojects `xc = Rc @ (Rb @ p_local + tb) + tc`; `screen_mapping.charuco_corner_local_mm` (`screen_mapping.py:146`) builds `p_local` with **center origin, +y UP** (docstring: feeding y-down points yields a mirrored pose). The SL `p_local` MUST use the identical frame or every cabinet pose silently mirrors. Task 3.1 enforces +y-up and a test pins the sign.

---

## File structure

**Python sidecar** (`python-sidecar/src/lmt_vba_sidecar/`):
- `sl_geometry.py` — **new.** numpy-only (no cv2): `sl_local_mm(rect, u, v, pitch_x, pitch_y)` and `sl_cabinet_corners_mm(rect, pitch_x, pitch_y)`. The SL analogs of `charuco_corner_local_mm` / `_active_surface_corners_mm`. Isolated so it unit-tests without OpenCV.
- `reconstruct.py` — **modify.** Extract steps 7–11 (init → BA → per-cabinet geometry → report → result) into `solve_and_emit(...)`, parameterized by a `corners_local_provider` callable + `nominal_m`. `run_reconstruct` keeps steps 1–6 then delegates. Behavior-preserving; existing tests are the guard.
- `sl_reconstruct.py` — **new.** `validate_sl_provenance(...)`, `run_reconstruct_structured_light(cmd)`: load sl_meta, validate provenance across N correspondence files, load intrinsics, assemble observations (id→cabinet via `sl_meta.dots`, screen(u,v)→local mm via `sl_geometry`, undistort camera (x,y)), observability gate, then call `reconstruct.solve_and_emit`.
- `ipc.py` — **modify.** Add `ReconstructStructuredLightInput`.
- `__main__.py` — **modify.** Register `reconstruct_structured_light` in `sub.add_parser`, `SUBCOMMAND_MODULES`, `SUBCOMMAND_ENTRYPOINTS`, and the `ipc` import block.
- `tests/` — **new** `test_sl_geometry.py`, `test_sl_reconstruct.py`; **modify** `test_ipc.py`, `test_main_dispatch.py`. Existing `test_reconstruct.py` / `test_reconstruct_per_cabinet.py` are the refactor regression guard (unchanged).

**Rust workspace:**
- `crates/adapter-visual-ba/src/api.rs` — **modify.** `ReconstructStructuredLightArgs` + `reconstruct_structured_light` returning the existing `ReconstructOut`.
- `crates/lmt-app/src/visual.rs` — **modify.** Extract `persist_reconstruct_result(project_path, screen_id, out) -> VisualReconstructResult` (the measured.yaml backup/rollback + summary build, currently inline in `run_reconstruct`), then add `run_reconstruct_structured_light(...)` calling it.
- `crates/lmt-cli/src/cli.rs` — **modify.** `VisualCmd::ReconstructStructuredLight { project_path, screen_id, sl_meta, intrinsics, correspondences (repeatable --corr) }`.
- `crates/lmt-cli/src/commands/visual.rs` — **modify.** Dispatch arm + `reconstruct_structured_light` handler (gate_destructive → DryRun/Execute → envelope).
- `crates/lmt-shared/src/manifest.rs` — **modify.** One `Operation` row `visual.reconstruct_structured_light` (result reuses `VisualReconstructResult`).
- `crates/lmt-cli/tests/cli_e2e.rs` — **modify.** refuse-without-yes + dry-run-writes-nothing + `#[ignore]` happy (real sidecar). The existing `visual_reconstruct_structured_light_is_unsupported` test (the `reconstruct --method structured-light` path, `cli_e2e.rs:858`) STAYS unchanged — that path remains unsupported; SL now has its own subcommand.
- `docs/agents-cli.md` — **modify.** New command row + "Not exposed in CLI / Tauri" note (the `visual` group has no GUI shim; CLI-only, per existing convention).

**No new Rust DTO** (reuses `VisualReconstructResult`) → no `schema.rs` / `dto.rs` change.

---

## TASK 3.1 — SL local-mm geometry (numpy-only)

**Files:**
- Create: `python-sidecar/src/lmt_vba_sidecar/sl_geometry.py`
- Test: `python-sidecar/tests/test_sl_geometry.py`

- [ ] **Step 1: Write the failing test**

```python
# python-sidecar/tests/test_sl_geometry.py
import numpy as np
from lmt_vba_sidecar.sl_geometry import sl_local_mm, sl_cabinet_corners_mm


def test_center_dot_maps_to_origin():
    # rect [x=100, y=50, w=400, h=300], dot at the rect center
    p = sl_local_mm((100, 50, 400, 300), u=100 + 200, v=50 + 150,
                    pitch_x=0.5, pitch_y=0.5)
    assert np.allclose(p, [0.0, 0.0, 0.0], atol=1e-9)


def test_y_axis_is_up():
    # a dot ABOVE center on screen (smaller v) must have LARGER local y (+y up),
    # matching screen_mapping.charuco_corner_local_mm's frame.
    rect = (0, 0, 400, 300)
    top = sl_local_mm(rect, u=200, v=50, pitch_x=1.0, pitch_y=1.0)   # v<150 = upper
    bot = sl_local_mm(rect, u=200, v=250, pitch_x=1.0, pitch_y=1.0)  # v>150 = lower
    assert top[1] > 0.0 and bot[1] < 0.0
    assert top[2] == 0.0


def test_x_axis_and_pitch_scale():
    # +x right, scaled by pitch_x. dot 100px right of center, pitch 0.4 -> +40mm
    p = sl_local_mm((0, 0, 400, 300), u=200 + 100, v=150, pitch_x=0.4, pitch_y=0.4)
    assert np.isclose(p[0], 40.0) and np.isclose(p[1], 0.0)


def test_corners_match_active_size_and_order():
    # corners derived from rect w*pitch x h*pitch, order BL,BR,TR,TL (+y up)
    c = sl_cabinet_corners_mm((0, 0, 400, 300), pitch_x=0.5, pitch_y=0.5)
    assert c.shape == (4, 3)
    hw, hh = 400 * 0.5 / 2, 300 * 0.5 / 2     # 100, 75
    np.testing.assert_allclose(c, [[-hw, -hh, 0], [hw, -hh, 0],
                                   [hw, hh, 0], [-hw, hh, 0]])
```

- [ ] **Step 2: Run to verify it fails**

Run: `cd python-sidecar && .venv/bin/python -m pytest tests/test_sl_geometry.py -v`
Expected: FAIL with `ModuleNotFoundError: No module named 'lmt_vba_sidecar.sl_geometry'`

- [ ] **Step 3: Write minimal implementation**

```python
# python-sidecar/src/lmt_vba_sidecar/sl_geometry.py
"""Structured-light screen-pixel -> cabinet-local-mm geometry (numpy-only, no cv2).

The SL analogs of screen_mapping.charuco_corner_local_mm / reconstruct._active_
surface_corners_mm. A dot's screen pixel (u,v) is absolute; its cabinet occupies
input_rect_px = [x,y,w,h]. Local mm uses CENTER origin and **+y UP** so SL
observations feed model_constrained_ba with the identical frame convention as
the ChArUco path (feeding y-down points yields a mirrored cabinet pose — see
screen_mapping.charuco_corner_local_mm). Scale comes from the cabinet's own
pixel pitch (mm/px), so mm is exact for any per-cabinet size/pitch.
"""
from __future__ import annotations

import numpy as np


def sl_local_mm(rect: tuple[int, int, int, int], u: float, v: float,
                pitch_x: float, pitch_y: float) -> np.ndarray:
    """Screen pixel (u,v) -> cabinet-local [x,y,0] mm; center origin, +x right, +y up."""
    x, y, w, h = rect
    local_x_px = (u - x) - w / 2.0          # +x right
    local_y_px = h / 2.0 - (v - y)          # +y UP (smaller v = higher = +y)
    return np.array([local_x_px * pitch_x, local_y_px * pitch_y, 0.0], dtype=float)


def sl_cabinet_corners_mm(rect: tuple[int, int, int, int],
                          pitch_x: float, pitch_y: float) -> np.ndarray:
    """The 4 active-surface corners in local mm, order BL,BR,TR,TL (+y up).

    Matches reconstruct._active_surface_corners_mm's order — load-bearing:
    compare_known derives size from this order (width=‖c1-c0‖, height=‖c2-c1‖).
    Active size derives from the cabinet's pixel extent x pitch (the 1:1-feed
    guarantee means rect w/h == cabinet resolution_px)."""
    _x, _y, w, h = rect
    hw, hh = (w * pitch_x) / 2.0, (h * pitch_y) / 2.0
    return np.array([[-hw, -hh, 0.0], [hw, -hh, 0.0],
                     [hw, hh, 0.0], [-hw, hh, 0.0]], dtype=float)
```

- [ ] **Step 4: Run to verify it passes**

Run: `cd python-sidecar && .venv/bin/python -m pytest tests/test_sl_geometry.py -v`
Expected: PASS (4 passed)

- [ ] **Step 5: Commit**

```bash
git add python-sidecar/src/lmt_vba_sidecar/sl_geometry.py python-sidecar/tests/test_sl_geometry.py
git commit -m "feat(sidecar): SL screen-pixel->cabinet-local-mm geometry (+y up)"
```

---

## TASK 3.2 — IPC input + provenance validator

**Files:**
- Modify: `python-sidecar/src/lmt_vba_sidecar/ipc.py`
- Create: `python-sidecar/src/lmt_vba_sidecar/sl_reconstruct.py` (provenance fn only this task)
- Test: `python-sidecar/tests/test_ipc.py` (input model), `python-sidecar/tests/test_sl_reconstruct.py` (provenance)

- [ ] **Step 1: Write the failing tests**

```python
# append to python-sidecar/tests/test_ipc.py
from lmt_vba_sidecar.ipc import ReconstructStructuredLightInput

def test_reconstruct_structured_light_input_defaults():
    m = ReconstructStructuredLightInput.model_validate({
        "command": "reconstruct_structured_light", "version": 1,
        "project": {"screen_id": "MAIN",
                    "cabinet_array": {"cols": 2, "rows": 1,
                                      "absent_cells": [], "cabinet_size_mm": [500, 500]}},
        "correspondence_paths": ["/c/p0.json", "/c/p1.json"],
        "sl_meta_path": "/sl/sl_meta.json", "intrinsics_path": "/cal/intr.json",
    })
    assert m.project.shape_prior == "flat"          # ReconstructProject default
    assert m.pose_report_path is None
    assert len(m.correspondence_paths) == 2
```

```python
# python-sidecar/tests/test_sl_reconstruct.py
import pytest
from lmt_vba_sidecar.ipc import CorrespondenceFile
from lmt_vba_sidecar.sl_reconstruct import validate_sl_provenance


def _corr(screen_id="MAIN", sha="abc"):
    return CorrespondenceFile.model_validate({
        "schema_version": 1, "screen_id": screen_id, "sl_meta_sha256": sha,
        "screen_resolution": [960, 540], "camera_image_size": [4000, 3000],
        "source_input": "/cap/p.mp4",
        "points": [{"id": 0, "u": 1.0, "v": 2.0, "x": 3.0, "y": 4.0}]})


def test_provenance_accepts_consistent_set():
    validate_sl_provenance([_corr(), _corr()], expected_sha="abc", expected_screen_id="MAIN")


def test_provenance_rejects_mixed_screen_id():
    with pytest.raises(ValueError, match="screen_id"):
        validate_sl_provenance([_corr(screen_id="MAIN"), _corr(screen_id="FLOOR")],
                               expected_sha="abc", expected_screen_id="MAIN")


def test_provenance_rejects_sha_mismatch_vs_meta():
    with pytest.raises(ValueError, match="sl_meta_sha256"):
        validate_sl_provenance([_corr(sha="abc")], expected_sha="DIFFERENT",
                               expected_screen_id="MAIN")


def test_provenance_rejects_screen_id_not_matching_project():
    with pytest.raises(ValueError, match="project"):
        validate_sl_provenance([_corr(screen_id="MAIN")], expected_sha="abc",
                               expected_screen_id="FLOOR")
```

- [ ] **Step 2: Run to verify they fail**

Run: `cd python-sidecar && .venv/bin/python -m pytest tests/test_ipc.py -k reconstruct_structured_light tests/test_sl_reconstruct.py -v`
Expected: FAIL — `ImportError: cannot import name 'ReconstructStructuredLightInput'` / `No module named 'lmt_vba_sidecar.sl_reconstruct'`

- [ ] **Step 3: Implement**

Add to `python-sidecar/src/lmt_vba_sidecar/ipc.py` (after `ReconstructInput`; reuses `ReconstructProject` + `FrameSpec`, both already defined):

```python
class ReconstructStructuredLightInput(BaseModel):
    command: Literal["reconstruct_structured_light"]
    version: Literal[1]
    project: ReconstructProject
    # One CorrespondenceFile per camera pose (decode_structured_light output).
    correspondence_paths: Annotated[list[str], Field(min_length=2)]
    sl_meta_path: str
    # Camera intrinsics JSON (visual calibrate output): {K, dist_coeffs, image_size}.
    intrinsics_path: str
    # If set, the sidecar writes cabinet_pose_report.json here (spec §9).
    pose_report_path: str | None = None
```

Create `python-sidecar/src/lmt_vba_sidecar/sl_reconstruct.py` with ONLY the validator for now (the full `run_*` lands in Task 3.4):

```python
# python-sidecar/src/lmt_vba_sidecar/sl_reconstruct.py
"""Structured-light multi-view reconstruction: N CorrespondenceFiles -> metric
per-cabinet model, via the SAME model_constrained_ba the ChArUco path uses.

The SL path differs from reconstruct.py only in observation SOURCE:
  - cabinet id    : sl_meta.dots[id].cabinet (already tagged at generation)
  - p_local mm    : sl_geometry.sl_local_mm(cabinet_rect, u, v, pitch)
  - camera pixel  : correspondence (x,y), undistorted via reconstruct._undistort_obs
Everything after observation assembly is reconstruct.solve_and_emit (shared).
"""
from __future__ import annotations

from lmt_vba_sidecar.ipc import CorrespondenceFile


def validate_sl_provenance(corr_files: list[CorrespondenceFile], *,
                           expected_sha: str, expected_screen_id: str) -> None:
    """Codex finding 4 gate: every pose file must share ONE screen_id + ONE
    sl_meta_sha256, that sha must equal the sl_meta.json actually being used,
    and the screen_id must match the project/screen. Any mismatch = stale/mixed
    capture -> ValueError (mapped to invalid_input upstream)."""
    screen_ids = {c.screen_id for c in corr_files}
    shas = {c.sl_meta_sha256 for c in corr_files}
    if len(screen_ids) != 1:
        raise ValueError(f"correspondence files disagree on screen_id: {sorted(screen_ids)}")
    if len(shas) != 1:
        raise ValueError(f"correspondence files disagree on sl_meta_sha256: {sorted(shas)}")
    (only_screen,) = screen_ids
    (only_sha,) = shas
    if only_sha != expected_sha:
        raise ValueError(
            f"sl_meta_sha256 mismatch: correspondences were decoded against "
            f"'{only_sha}' but the supplied sl_meta.json hashes to '{expected_sha}'")
    if only_screen != expected_screen_id:
        raise ValueError(
            f"screen_id '{only_screen}' in correspondences != project screen "
            f"'{expected_screen_id}'")
```

- [ ] **Step 4: Run to verify they pass**

Run: `cd python-sidecar && .venv/bin/python -m pytest tests/test_ipc.py -k reconstruct_structured_light tests/test_sl_reconstruct.py -v`
Expected: PASS (5 passed)

- [ ] **Step 5: Commit**

```bash
git add python-sidecar/src/lmt_vba_sidecar/ipc.py python-sidecar/src/lmt_vba_sidecar/sl_reconstruct.py python-sidecar/tests/test_ipc.py python-sidecar/tests/test_sl_reconstruct.py
git commit -m "feat(sidecar): SL reconstruct IPC input + provenance validator"
```

---

## TASK 3.3 — Refactor reconstruct.py: extract `solve_and_emit` (behavior-preserving)

> **Tradeoff (flag for reviewer):** this touches stable, tested code. The alternative is to copy the ~110-line init→BA→output block into `sl_reconstruct.py`. Chosen the extraction because the two paths MUST stay behaviorally identical (same BA, same quality classification, same output schema); duplication invites silent divergence. The guard is: the existing `test_reconstruct.py` (11 tests) + `test_reconstruct_per_cabinet.py` pass UNCHANGED. If the reviewer prefers zero edits to `reconstruct.py`, swap this task for "copy the block into sl_reconstruct" — the rest of the plan is unaffected.

**Files:**
- Modify: `python-sidecar/src/lmt_vba_sidecar/reconstruct.py`
- Guard: `python-sidecar/tests/test_reconstruct.py`, `python-sidecar/tests/test_reconstruct_per_cabinet.py` (no edits)

- [ ] **Step 1: Run the guard tests GREEN first (baseline)**

Run: `cd python-sidecar && .venv/bin/python -m pytest tests/test_reconstruct.py tests/test_reconstruct_per_cabinet.py -v`
Expected: all PASS (records the pre-refactor baseline).

- [ ] **Step 2: Extract the helper**

In `reconstruct.py`, add this function (it is steps 7–11 of `run_reconstruct` verbatim, with two parameters replacing the two ChArUco-specific lookups: `corners_local_provider` replaces `_active_surface_corners_mm(screen_mapping, cid)`, and `nominal_m` is passed in instead of computed inline):

```python
from collections.abc import Callable

def solve_and_emit(
    *,
    K: np.ndarray,
    observations: list[Observation],
    per_view_cab_corners: dict[tuple[int, int], list[tuple[np.ndarray, np.ndarray]]],
    n_cameras: int,
    cab_to_idx: dict[tuple[int, int], int],
    root_idx: int,
    n_cabinets: int,
    nominal_m: dict[tuple[int, int], tuple[float, float, float]],
    per_cabinet_views: dict[int, set[int]],
    per_cabinet_points: dict[int, int],
    corners_local_provider: Callable[[str], np.ndarray],
    pose_report_path: str | None,
) -> int:
    """Shared init -> model_constrained_ba -> per-cabinet geometry -> report ->
    ResultEvent. Used by both run_reconstruct (charuco) and
    sl_reconstruct.run_reconstruct_structured_light. corners_local_provider maps
    a cabinet_id string to its (4,3) active-surface corners in local mm."""
    # --- 7. init ---
    write_event(ProgressEvent(event="progress", stage="bundle_adjustment", percent=0.5, message="initializing"))
    if ROOT_CABINET not in nominal_m:
        write_event(ErrorEvent(event="error", code="invalid_input",
            message="root cabinet (0,0) missing from nominal model (absent_cells?)", fatal=True))
        return 1
    root_nominal_mm = np.array(nominal_m[ROOT_CABINET], dtype=float) * 1000.0
    bridge = estimate_nonroot_cabinet_init(per_view_cab_corners, root_idx, K)
    init_cabinets: dict[int, tuple[np.ndarray, np.ndarray]] = {}
    for cr, idx in cab_to_idx.items():
        if idx == root_idx:
            init_cabinets[idx] = (np.eye(3), np.zeros(3))
        elif idx in bridge:
            init_cabinets[idx] = bridge[idx]
        elif cr in nominal_m:
            t_mm = np.array(nominal_m[cr], dtype=float) * 1000.0 - root_nominal_mm
            init_cabinets[idx] = (np.eye(3), t_mm)
        else:
            init_cabinets[idx] = (np.eye(3), np.zeros(3))
    init_cameras: list[tuple[np.ndarray, np.ndarray]] = [
        _pnp_camera(cam_idx, root_idx, init_cabinets, per_view_cab_corners, K)
        for cam_idx in range(n_cameras)
    ]

    # --- 8. BA ---
    result = model_constrained_ba(
        K=K, observations=observations,
        n_cameras=n_cameras, n_cabinets=n_cabinets,
        root_cabinet_idx=root_idx,
        init_cameras=init_cameras, init_cabinets=init_cabinets, loss="huber")
    if not result.converged:
        write_event(ErrorEvent(event="error", code="ba_diverged",
            message=f"BA did not converge (rms={result.rms_reprojection_px:.2f}px after {result.iterations} iters)",
            fatal=True))
        return 1

    # --- 9. per-cabinet geometry ---
    write_event(ProgressEvent(event="progress", stage="output", percent=0.9, message="building pose report"))
    per_cabinet_rms = _per_cabinet_reproj_rms(K, result.camera_poses, result.cabinet_poses, observations)
    idx_to_cab = {idx: cr for cr, idx in cab_to_idx.items()}
    cabinet_poses: list[CabinetPose] = []
    measured_points: list[MeasuredPoint] = []
    for idx in range(n_cabinets):
        col, row = idx_to_cab[idx]
        cid = _cabinet_id(col, row)
        R, t = result.cabinet_poses[idx]
        corners_local = corners_local_provider(cid)
        center, normal, _size, world_corners = reconstruct_cabinet_geometry(R, t, corners_local)
        n_views = len(per_cabinet_views.get(idx, set()))
        n_points = per_cabinet_points.get(idx, 0)
        cab_rms = per_cabinet_rms[idx]
        quality = _classify_cabinet_quality(n_views, cab_rms)
        cabinet_poses.append(CabinetPose(
            cabinet_id=cid, position_mm=center.tolist(), rotation_matrix=R.tolist(),
            normal=normal.tolist(), corners_mm=[c.tolist() for c in world_corners],
            reprojection_rms_px=cab_rms, observed_views=n_views,
            observed_points=n_points, quality=quality))
        if quality != "ok":
            write_event(WarningEvent(event="warning", code="cabinet_quality",
                message=f"cabinet {cid}: {quality} (views={n_views}, rms={cab_rms:.2f}px)", cabinet=cid))
        cov_mm = result.cabinet_covariances.get(idx)
        if cov_mm is not None and np.isfinite(cov_mm).all():
            uncertainty = Uncertainty(covariance=(np.asarray(cov_mm, dtype=float) / 1.0e6).tolist())
        else:
            write_event(WarningEvent(event="warning", code="missing_covariance",
                message=f"cabinet {cid} has no usable BA covariance; falling back to isotropic 5mm",
                cabinet=f"MAIN_{cid}"))
            uncertainty = Uncertainty(isotropic=FALLBACK_ISOTROPIC_M)
        measured_points.append(MeasuredPoint(
            name=f"MAIN_{cid}", position=(center / 1000.0).tolist(),
            uncertainty=uncertainty,
            source=PointSource(visual_ba=PointSourceVisualBa(camera_count=max(1, n_views)))))

    # --- 10. write report ---
    if pose_report_path:
        report = CabinetPoseReport(
            schema_version="visual_pose_report.v1",
            frame=FrameSpec(root_cabinet=list(ROOT_CABINET)),
            cabinet_poses=cabinet_poses)
        _atomic_write_json(pose_report_path, report.model_dump_json(indent=2))

    # --- 11. result ---
    write_event(ResultEvent(event="result", data=ResultData(
        measured_points=measured_points,
        ba_stats=BaStats(rms_reprojection_px=float(result.rms_reprojection_px),
                         iterations=int(result.iterations), converged=True),
        frame_strategy_used="nominal_anchoring", procrustes_align_rms_m=0.0)))
    return 0
```

Now replace `run_reconstruct`'s steps 7–11 (currently `reconstruct.py:320` "--- 7. init ---" through the final `return 0` at line 461) with a build of `per_cabinet_views`/`per_cabinet_points` (already built during step 5) + nominal computation + a delegated call:

```python
    # --- 7. nominal model (kept here: needs cmd.project) ---
    try:
        nominal_m = nominal_cabinet_centers_model_frame(cmd.project.cabinet_array, cmd.project.shape_prior)
    except ValueError as e:
        write_event(ErrorEvent(event="error", code="invalid_input", message=str(e), fatal=True))
        return 1

    return solve_and_emit(
        K=K, observations=observations, per_view_cab_corners=per_view_cab_corners,
        n_cameras=len(view_images), cab_to_idx=cab_to_idx, root_idx=root_idx,
        n_cabinets=n_cabinets, nominal_m=nominal_m,
        per_cabinet_views=per_cabinet_views, per_cabinet_points=per_cabinet_points,
        corners_local_provider=lambda cid: _active_surface_corners_mm(screen_mapping, cid),
        pose_report_path=cmd.pose_report_path)
```

(`per_cabinet_views` / `per_cabinet_points` are already populated in step 5 at `reconstruct.py:281-303`; `n_cameras` == `len(view_images)`.)

> **NOTE — `MAIN_` prefix is a PRE-EXISTING hardcode, preserved verbatim (do NOT "fix" it here).** `solve_and_emit` keeps `name=f"MAIN_{cid}"` / `cabinet=f"MAIN_{cid}"` exactly as `reconstruct.py:428-432` has them today. This is intentional behavior preservation: the existing tests pin it as a fixed literal independent of screen_id (`test_reconstruct_per_cabinet.py:16,43` runs `screen_id="S"` and asserts point names `MAIN_V000_R000`). Changing it to `f"{screen_id}_{cid}"` would break those tests and is out of Phase-3 scope. Consequence: the SL path (arbitrary `screen_id`) persists `MeasuredPoints.screen_id = <screen_id>` (Rust container) while point *names* read `MAIN_V...` — but this is the SAME (cosmetic) wart the ChArUco path already has, applied consistently. No downstream consumer was found parsing the `MAIN_` name prefix (`compare_known.py` keys by cabinet geometry, not name prefix; screen identity lives in the container's `screen_id`). Flagged for a separate, screen-wide cleanup if the user wants the prefix to track `screen_id` — that cleanup must touch both paths and update the pinned tests together.

- [ ] **Step 3: Run the guard tests — must still be GREEN**

Run: `cd python-sidecar && .venv/bin/python -m pytest tests/test_reconstruct.py tests/test_reconstruct_per_cabinet.py -v`
Expected: identical PASS set to Step 1 (behavior preserved).

- [ ] **Step 4: Commit**

```bash
git add python-sidecar/src/lmt_vba_sidecar/reconstruct.py
git commit -m "refactor(sidecar): extract reconstruct.solve_and_emit (shared by SL path)"
```

---

## TASK 3.4 — `run_reconstruct_structured_light` + dispatch + gating test

**Files:**
- Modify: `python-sidecar/src/lmt_vba_sidecar/sl_reconstruct.py`
- Modify: `python-sidecar/src/lmt_vba_sidecar/__main__.py`
- Test: `python-sidecar/tests/test_sl_reconstruct.py`, `python-sidecar/tests/test_main_dispatch.py`

- [ ] **Step 1: Write the failing GATING test (end-to-end, synthetic)**

```python
# append to python-sidecar/tests/test_sl_reconstruct.py
import json, hashlib, pathlib
import numpy as np
from lmt_vba_sidecar.ipc import GenerateStructuredLightInput, ReconstructStructuredLightInput
from lmt_vba_sidecar.structured_light import run_generate_structured_light
from lmt_vba_sidecar.sl_geometry import sl_local_mm
from lmt_vba_sidecar.sl_feasibility import look_at_pose, project_point
from lmt_vba_sidecar.sl_reconstruct import run_reconstruct_structured_light


def _gen_two_cabinet_meta(tmp_path):
    cmd = GenerateStructuredLightInput.model_validate({
        "command": "generate_structured_light", "version": 1,
        "project": {"screen_id": "MAIN",
                    "cabinet_array": {"cols": 2, "rows": 1, "absent_cells": [],
                                      "cabinet_size_mm": [500, 500]}},
        "output_dir": str(tmp_path / "sl"), "screen_resolution": [960, 480],
        "dot_spacing_px": 80, "margin_px": 60})
    assert run_generate_structured_light(cmd) == 0
    return tmp_path / "sl" / "sl_meta.json"


def _write_intrinsics(tmp_path, f=3000.0, cx=2000.0, cy=1500.0, w=4000, h=3000):
    p = tmp_path / "intr.json"
    p.write_text(json.dumps({"K": [[f, 0, cx], [0, f, cy], [0, 0, 1]],
                             "dist_coeffs": [0, 0, 0, 0, 0], "image_size": [w, h]}))
    return p, np.array([[f, 0, cx], [0, f, cy], [0, 0, 1]], float)


def test_synthetic_sl_reconstruction_recovers_cabinet_offset_mm(tmp_path):
    """Synthetic perfect correspondences (+0.1px noise) for a 2-cabinet wall with
    a KNOWN deviation on cabinet 1; recovered pose must place it within mm of
    the true (deviated) position. This is the Phase-3 gating test (was assert True)."""
    meta_path = _gen_two_cabinet_meta(tmp_path)
    meta = json.loads(meta_path.read_text())
    intr_path, K = _write_intrinsics(tmp_path)
    rect_by_cr = {(c["col"], c["row"]): c["input_rect_px"] for c in meta["cabinets"]}
    pitch_by_cr = {(c["col"], c["row"]): c["pixel_pitch_mm"] for c in meta["cabinets"]}
    cab_by_id = {d["id"]: tuple(d["cabinet"]) for d in meta["dots"]}

    # True world: root (0,0) frame = world; cabinet (1,0) nominally +500mm x,
    # plus a KNOWN 4mm deviation (3mm x, 2mm y, 1mm z) we expect to recover.
    nominal_offset = np.array([500.0, 0.0, 0.0])
    deviation = np.array([3.0, 2.0, 1.0])
    cab_world_t = {(0, 0): np.zeros(3), (1, 0): nominal_offset + deviation}

    truth_world = {}
    for d in meta["dots"]:
        cr = cab_by_id[d["id"]]
        p_local = sl_local_mm(tuple(rect_by_cr[cr]), d["u"], d["v"],
                              pitch_by_cr[cr][0], pitch_by_cr[cr][1])
        truth_world[d["id"]] = p_local + cab_world_t[cr]   # identity cabinet rotation

    sha = hashlib.sha256(meta_path.read_bytes()).hexdigest()
    poses = [look_at_pose(np.array([px, 0.0, -3500.0]), np.array([250.0, 0.0, 0.0]))
             for px in (-1200.0, -400.0, 400.0, 1200.0)]
    rng = np.random.default_rng(0)
    corr_paths = []
    for vi, (R, t) in enumerate(poses):
        pts = []
        for d in meta["dots"]:
            p = project_point(K, R, t, truth_world[d["id"]]) + rng.normal(0, 0.1, 2)
            pts.append({"id": d["id"], "u": d["u"], "v": d["v"],
                        "x": float(p[0]), "y": float(p[1])})
        cp = tmp_path / f"corr_{vi}.json"
        cp.write_text(json.dumps({
            "schema_version": 1, "screen_id": "MAIN", "sl_meta_sha256": sha,
            "screen_resolution": meta["screen_resolution"], "camera_image_size": [4000, 3000],
            "source_input": f"/cap/pose{vi}.mp4", "points": pts}))
        corr_paths.append(str(cp))

    report_path = tmp_path / "report.json"
    cmd = ReconstructStructuredLightInput.model_validate({
        "command": "reconstruct_structured_light", "version": 1,
        "project": {"screen_id": "MAIN",
                    "cabinet_array": {"cols": 2, "rows": 1, "absent_cells": [],
                                      "cabinet_size_mm": [500, 500]}},
        "correspondence_paths": corr_paths, "sl_meta_path": str(meta_path),
        "intrinsics_path": str(intr_path), "pose_report_path": str(report_path)})
    assert run_reconstruct_structured_light(cmd) == 0

    report = json.loads(report_path.read_text())
    by_id = {c["cabinet_id"]: c for c in report["cabinet_poses"]}
    # cabinet (1,0) measured center vs its TRUE center (= cabinet origin offset,
    # since the dot grid is symmetric about the cabinet center).
    true_center_10 = cab_world_t[(1, 0)]
    got = np.array(by_id["V001_R000"]["position_mm"])
    assert np.linalg.norm(got - true_center_10) < 5.0   # mm (conservative; BA + 0.1px noise)

    # Finding-2 guard: correspondence (u,v) must be IGNORED (canonical (u,v) comes
    # from sl_meta). Corrupt every corr point's u,v to garbage, reconstruct again ->
    # identical cabinet pose. If p_local trusted corr (u,v), this would diverge/fail.
    for cp in corr_paths:
        d = json.loads(pathlib.Path(cp).read_text())
        for p in d["points"]:
            p["u"], p["v"] = 0.0, 0.0
        pathlib.Path(cp).write_text(json.dumps(d))
    report2 = tmp_path / "report2.json"
    assert run_reconstruct_structured_light(cmd.model_copy(update={"pose_report_path": str(report2)})) == 0
    got2 = np.array({c["cabinet_id"]: c for c in json.loads(report2.read_text())["cabinet_poses"]}
                    ["V001_R000"]["position_mm"])
    np.testing.assert_allclose(got, got2, atol=1e-6)


def _valid_corr(tmp_path, sha, n=2, screen_res=(960, 480)):
    """n minimal corr files that pass provenance (shared screen_id MAIN + sha)."""
    paths = []
    for i in range(n):
        cp = tmp_path / f"vc{i}.json"
        cp.write_text(json.dumps({
            "schema_version": 1, "screen_id": "MAIN", "sl_meta_sha256": sha,
            "screen_resolution": list(screen_res), "camera_image_size": [4000, 3000],
            "source_input": "x", "points": [{"id": 0, "u": 1, "v": 1, "x": 1, "y": 1}]}))
        paths.append(str(cp))
    return paths


def test_run_rejects_malformed_sl_meta(tmp_path):
    bad = tmp_path / "bad_meta.json"
    bad.write_text('{"schema_version": 1, "screen_id": "MAIN"}')   # missing required fields
    intr_path, _ = _write_intrinsics(tmp_path)
    cmd = ReconstructStructuredLightInput.model_validate({
        "command": "reconstruct_structured_light", "version": 1,
        "project": {"screen_id": "MAIN",
                    "cabinet_array": {"cols": 2, "rows": 1, "absent_cells": [],
                                      "cabinet_size_mm": [500, 500]}},
        "correspondence_paths": _valid_corr(tmp_path, "x"),
        "sl_meta_path": str(bad), "intrinsics_path": str(intr_path)})
    assert run_reconstruct_structured_light(cmd) == 1     # invalid_input, not a traceback


def test_run_rejects_meta_project_cabinet_mismatch(tmp_path):
    # sl_meta present = {(0,0),(1,0)}; project declares (1,0) ABSENT -> {(0,0)}.
    meta_path = _gen_two_cabinet_meta(tmp_path)
    sha = hashlib.sha256(meta_path.read_bytes()).hexdigest()
    intr_path, _ = _write_intrinsics(tmp_path)
    cmd = ReconstructStructuredLightInput.model_validate({
        "command": "reconstruct_structured_light", "version": 1,
        "project": {"screen_id": "MAIN",
                    "cabinet_array": {"cols": 2, "rows": 1, "absent_cells": [[1, 0]],
                                      "cabinet_size_mm": [500, 500]}},
        "correspondence_paths": _valid_corr(tmp_path, sha),
        "sl_meta_path": str(meta_path), "intrinsics_path": str(intr_path)})
    assert run_reconstruct_structured_light(cmd) == 1     # cabinet-set mismatch -> invalid_input


def test_run_rejects_provenance_mismatch(tmp_path):
    meta_path = _gen_two_cabinet_meta(tmp_path)
    intr_path, _ = _write_intrinsics(tmp_path)
    # two corr files with DIFFERENT sha -> invalid_input (return 1)
    for vi, sha in enumerate(("aaa", "bbb")):
        (tmp_path / f"c{vi}.json").write_text(json.dumps({
            "schema_version": 1, "screen_id": "MAIN", "sl_meta_sha256": sha,
            "screen_resolution": [960, 480], "camera_image_size": [4000, 3000],
            "source_input": "x", "points": [{"id": 0, "u": 1, "v": 1, "x": 1, "y": 1}]}))
    cmd = ReconstructStructuredLightInput.model_validate({
        "command": "reconstruct_structured_light", "version": 1,
        "project": {"screen_id": "MAIN",
                    "cabinet_array": {"cols": 2, "rows": 1, "absent_cells": [],
                                      "cabinet_size_mm": [500, 500]}},
        "correspondence_paths": [str(tmp_path / "c0.json"), str(tmp_path / "c1.json")],
        "sl_meta_path": str(meta_path), "intrinsics_path": str(intr_path)})
    assert run_reconstruct_structured_light(cmd) == 1
```

- [ ] **Step 2: Run to verify it fails**

Run: `cd python-sidecar && .venv/bin/python -m pytest tests/test_sl_reconstruct.py -v`
Expected: FAIL — `ImportError: cannot import name 'run_reconstruct_structured_light'`

- [ ] **Step 3: Implement `run_reconstruct_structured_light`**

Append to `python-sidecar/src/lmt_vba_sidecar/sl_reconstruct.py`:

```python
import hashlib
import json
import pathlib

import numpy as np

from lmt_vba_sidecar.io_utils import write_event
from lmt_vba_sidecar.ipc import (
    ErrorEvent, ProgressEvent, ReconstructStructuredLightInput, StructuredLightMeta,
)
from lmt_vba_sidecar.model_constrained_ba import Observation
from lmt_vba_sidecar.nominal import nominal_cabinet_centers_model_frame
from lmt_vba_sidecar.observability import ObservabilityError, check_observability
from lmt_vba_sidecar.reconstruct import ROOT_CABINET, _undistort_obs, solve_and_emit
from lmt_vba_sidecar.sl_geometry import sl_cabinet_corners_mm, sl_local_mm


def run_reconstruct_structured_light(cmd: ReconstructStructuredLightInput) -> int:
    write_event(ProgressEvent(event="progress", stage="load", percent=0.0, message="loading sl_meta + correspondences"))

    # --- 1. sl_meta: SCHEMA-validate (not raw json), so malformed meta -> invalid_input
    #         instead of an internal_error traceback. screen_id is a system-boundary
    #         check (sl_meta is an external file). ---
    meta_path = pathlib.Path(cmd.sl_meta_path)
    try:
        meta = StructuredLightMeta.model_validate_json(meta_path.read_text())
        expected_sha = hashlib.sha256(meta_path.read_bytes()).hexdigest()
    except (OSError, ValueError) as e:
        write_event(ErrorEvent(event="error", code="invalid_input", message=f"sl_meta load/validate failed: {e}", fatal=True))
        return 1
    if meta.screen_id != cmd.project.screen_id:
        write_event(ErrorEvent(event="error", code="invalid_input",
            message=f"sl_meta screen_id '{meta.screen_id}' != project screen '{cmd.project.screen_id}'", fatal=True))
        return 1

    rect_by_cr = {(c.col, c.row): tuple(int(v) for v in c.input_rect_px) for c in meta.cabinets}
    pitch_by_cr = {(c.col, c.row): (float(c.pixel_pitch_mm[0]), float(c.pixel_pitch_mm[1])) for c in meta.cabinets}
    # CANONICAL screen coords come from sl_meta, NOT the correspondence file. The
    # screen-coordinate invariant (one id -> one fixed (u,v)) must be STRUCTURAL: a
    # stale/edited corr that kept id+sha but moved (u,v) must not shift p_local.
    cab_by_id = {d.id: (tuple(d.cabinet), float(d.u), float(d.v)) for d in meta.dots}

    # --- 2. correspondence files + provenance gate ---
    from lmt_vba_sidecar.ipc import CorrespondenceFile
    corr_files: list[CorrespondenceFile] = []
    for p in cmd.correspondence_paths:
        try:
            corr_files.append(CorrespondenceFile.model_validate_json(pathlib.Path(p).read_text()))
        except (OSError, ValueError) as e:
            write_event(ErrorEvent(event="error", code="invalid_input", message=f"correspondence '{p}' unreadable: {e}", fatal=True))
            return 1
    try:
        validate_sl_provenance(corr_files, expected_sha=expected_sha, expected_screen_id=cmd.project.screen_id)
    except ValueError as e:
        write_event(ErrorEvent(event="error", code="invalid_input", message=str(e), fatal=True))
        return 1

    # --- 3. intrinsics ---
    try:
        intr = json.loads(pathlib.Path(cmd.intrinsics_path).read_text())
        K = np.array(intr["K"], dtype=float)
        dist = np.array(intr["dist_coeffs"], dtype=float)
        image_size = tuple(int(v) for v in intr["image_size"])
    except (OSError, json.JSONDecodeError, KeyError, ValueError) as e:
        write_event(ErrorEvent(event="error", code="intrinsics_invalid", message=f"intrinsics load failed: {e}", fatal=True))
        return 1
    for c in corr_files:
        if tuple(c.camera_image_size) != image_size:
            write_event(ErrorEvent(event="error", code="invalid_input",
                message=f"correspondence camera_image_size {tuple(c.camera_image_size)} != intrinsics image_size {image_size}",
                fatal=True))
            return 1

    # --- 4. nominal model (project) + cabinet-set match. nominal_m.keys() IS the
    #         project present-cell set (nominal.py skips absent_cells), so this ties
    #         the sl_meta universe to the project: a stale sl_meta or an edited
    #         project layout (same screen_id+sha) is rejected instead of silently
    #         reconstructing the wrong cabinet universe. ---
    try:
        nominal_m = nominal_cabinet_centers_model_frame(cmd.project.cabinet_array, cmd.project.shape_prior)
    except ValueError as e:
        write_event(ErrorEvent(event="error", code="invalid_input", message=str(e), fatal=True))
        return 1
    present = sorted(rect_by_cr.keys(), key=lambda cr: (cr[1], cr[0]))
    if set(present) != set(nominal_m.keys()):
        write_event(ErrorEvent(event="error", code="invalid_input",
            message=f"sl_meta cabinet set {present} != project present cells "
                    f"{sorted(nominal_m.keys())} (stale sl_meta or edited project layout)", fatal=True))
        return 1
    if ROOT_CABINET not in present:
        write_event(ErrorEvent(event="error", code="invalid_input",
            message="root cabinet V000_R000 (0,0) not present in sl_meta cabinets", fatal=True))
        return 1
    cab_to_idx = {cr: i for i, cr in enumerate(present)}
    root_idx = cab_to_idx[ROOT_CABINET]
    n_cabinets = len(present)

    # --- 5. assemble observations (one camera per correspondence file). p_local uses
    #         CANONICAL (cu,cv) from sl_meta; ONLY the camera pixel (pt.x,pt.y) comes
    #         from the correspondence file. ---
    write_event(ProgressEvent(event="progress", stage="subpixel_refine", percent=0.3, message="assembling observations"))
    observations: list[Observation] = []
    per_view_cab_corners: dict[tuple[int, int], list[tuple[np.ndarray, np.ndarray]]] = {}
    per_cabinet_views: dict[int, set[int]] = {}
    per_cabinet_points: dict[int, int] = {}
    for cam_idx, cf in enumerate(corr_files):
        for pt in cf.points:
            info = cab_by_id.get(int(pt.id))
            if info is None:
                continue  # decoded id not in this sl_meta (defensive)
            cr, cu, cv = info
            cab_idx = cab_to_idx.get(cr)
            if cab_idx is None:
                continue  # dot references a cabinet not in meta.cabinets (hand-edited meta)
            p_local = sl_local_mm(rect_by_cr[cr], cu, cv, pitch_by_cr[cr][0], pitch_by_cr[cr][1])
            pixel = _undistort_obs(np.array([pt.x, pt.y], dtype=float), K, dist)
            observations.append(Observation(camera_idx=cam_idx, cabinet_idx=cab_idx, p_local=p_local, pixel=pixel))
            per_view_cab_corners.setdefault((cam_idx, cab_idx), []).append((p_local, pixel))
            per_cabinet_views.setdefault(cab_idx, set()).add(cam_idx)
            per_cabinet_points[cab_idx] = per_cabinet_points.get(cab_idx, 0) + 1

    if not observations:
        write_event(ErrorEvent(event="error", code="detection_failed",
            message="no usable correspondences across any pose", fatal=True))
        return 1

    # --- 6. observability ---
    try:
        check_observability(observations, n_cabinets, min_views=2, min_points=8)
    except ObservabilityError as e:
        write_event(ErrorEvent(event="error", code="observability_failed", message=str(e), fatal=True))
        return 1

    # --- 7-11. shared solve + emit ---
    def corners_provider(cabinet_id: str) -> np.ndarray:
        col, row = int(cabinet_id[1:4]), int(cabinet_id[6:9])  # V{col:03d}_R{row:03d}
        return sl_cabinet_corners_mm(rect_by_cr[(col, row)], *pitch_by_cr[(col, row)])

    return solve_and_emit(
        K=K, observations=observations, per_view_cab_corners=per_view_cab_corners,
        n_cameras=len(corr_files), cab_to_idx=cab_to_idx, root_idx=root_idx,
        n_cabinets=n_cabinets, nominal_m=nominal_m,
        per_cabinet_views=per_cabinet_views, per_cabinet_points=per_cabinet_points,
        corners_local_provider=corners_provider, pose_report_path=cmd.pose_report_path)
```

Register the subcommand in `python-sidecar/src/lmt_vba_sidecar/__main__.py`:
- Add `ReconstructStructuredLightInput` to the `from lmt_vba_sidecar.ipc import (...)` block.
- Add `sub.add_parser("reconstruct_structured_light")`.
- `SUBCOMMAND_MODULES`: `"reconstruct_structured_light": "lmt_vba_sidecar.sl_reconstruct"`.
- `SUBCOMMAND_ENTRYPOINTS`: `"reconstruct_structured_light": ("run_reconstruct_structured_light", ReconstructStructuredLightInput)`.

- [ ] **Step 4: Run to verify it passes**

Run: `cd python-sidecar && .venv/bin/python -m pytest tests/test_sl_reconstruct.py -v`
Expected: PASS (8 passed — 4 provenance unit + gating (incl. corr-u/v-ignored guard) + malformed-meta + cabinet-set-mismatch + provenance-run)

- [ ] **Step 5: Add dispatch test + run full sidecar suite**

Append to `python-sidecar/tests/test_main_dispatch.py`:
```python
def test_dispatch_knows_reconstruct_structured_light():
    import subprocess, sys, json
    p = subprocess.run([sys.executable, "-m", "lmt_vba_sidecar", "reconstruct_structured_light"],
                       input="{}", capture_output=True, text=True)
    assert p.returncode == 1
    ev = json.loads(p.stdout.strip().splitlines()[-1])
    assert ev["event"] == "error" and ev["code"] == "invalid_input"
```
Run: `cd python-sidecar && .venv/bin/python -m pytest -q`
Expected: all pass (incl. unchanged charuco reconstruct tests — the refactor guard).

- [ ] **Step 6: Commit**

```bash
git add python-sidecar/src/lmt_vba_sidecar/sl_reconstruct.py python-sidecar/src/lmt_vba_sidecar/__main__.py python-sidecar/tests/
git commit -m "feat(sidecar): SL multi-view reconstruct (provenance + model-constrained BA)"
```

---

## TASK 3.5 — Rust 6-point contract: `visual reconstruct-structured-light`

**Files:** `crates/adapter-visual-ba/src/api.rs`, `crates/lmt-app/src/visual.rs`, `crates/lmt-cli/src/cli.rs`, `crates/lmt-cli/src/commands/visual.rs`, `crates/lmt-shared/src/manifest.rs`, `crates/lmt-cli/tests/cli_e2e.rs`, `docs/agents-cli.md`. **No DTO/schema change** (reuses `VisualReconstructResult`).

- [ ] **Step 1: Write the failing tests (refuse + dry-run, no sidecar)**

```rust
// append to crates/lmt-cli/tests/cli_e2e.rs
#[test]
fn reconstruct_structured_light_refuses_without_yes() {
    let tmp = TempDir::new().unwrap();
    let proj = tmp.path().join("proj");
    write_gp_project(&proj, 2, 1);                       // existing helper (2x1 wall)
    let meta = tmp.path().join("sl_meta.json"); std::fs::write(&meta, "{}").unwrap();
    let intr = tmp.path().join("intr.json"); std::fs::write(&intr, "{}").unwrap();
    let c0 = tmp.path().join("c0.json"); std::fs::write(&c0, "{}").unwrap();
    let c1 = tmp.path().join("c1.json"); std::fs::write(&c1, "{}").unwrap();
    let assert = lmt().args(["--json", "visual", "reconstruct-structured-light",
        proj.to_str().unwrap(), "MAIN", "--sl-meta", meta.to_str().unwrap(),
        "--intrinsics", intr.to_str().unwrap(),
        "--corr", c0.to_str().unwrap(), "--corr", c1.to_str().unwrap()])
        .assert().failure();
    let out = assert.get_output();
    assert_eq!(out.status.code(), Some(2));
    let env: Value = serde_json::from_str(std::str::from_utf8(&out.stderr).unwrap().trim_end()).unwrap();
    assert_eq!(env["error"]["code"], "invalid_input");
}

#[test]
fn reconstruct_structured_light_dry_run_writes_nothing() {
    let tmp = TempDir::new().unwrap();
    let proj = tmp.path().join("proj");
    write_gp_project(&proj, 2, 1);
    let meta = tmp.path().join("sl_meta.json"); std::fs::write(&meta, "{}").unwrap();
    let intr = tmp.path().join("intr.json"); std::fs::write(&intr, "{}").unwrap();
    let c0 = tmp.path().join("c0.json"); std::fs::write(&c0, "{}").unwrap();
    let c1 = tmp.path().join("c1.json"); std::fs::write(&c1, "{}").unwrap();
    let assert = lmt().args(["--json", "--dry-run", "visual", "reconstruct-structured-light",
        proj.to_str().unwrap(), "MAIN", "--sl-meta", meta.to_str().unwrap(),
        "--intrinsics", intr.to_str().unwrap(),
        "--corr", c0.to_str().unwrap(), "--corr", c1.to_str().unwrap()])
        .assert().success();
    let env: Value = serde_json::from_slice(&assert.get_output().stdout).unwrap();
    assert_eq!(env["ok"], true);
    assert_eq!(env["data"]["dry_run"], true);
    assert!(!proj.join("measurements/measured.yaml").exists());
}
```

> If `write_gp_project` doesn't accept a `2,1` grid, reuse whatever project-fixture helper the neighboring reconstruct tests use (`cli_e2e.rs:830`, `:1039`) — the grid size is irrelevant for the refuse/dry-run paths (they never reach the sidecar).

- [ ] **Step 2: Run to verify it fails**

Run: `cargo test -p lmt-cli reconstruct_structured_light`
Expected: FAIL to compile (no `ReconstructStructuredLight` variant).

- [ ] **Step 3: Implement the contract**

`crates/adapter-visual-ba/src/api.rs` (mirror `reconstruct`; returns the SAME `ReconstructOut`):
```rust
pub struct ReconstructStructuredLightArgs {
    pub project: ReconstructProject,
    pub correspondence_paths: Vec<String>,
    pub sl_meta_path: String,
    pub intrinsics_path: String,
    pub pose_report_path: String,
    pub progress_tx: Option<mpsc::Sender<Event>>,
    pub cancel: Option<oneshot::Receiver<()>>,
}

pub async fn reconstruct_structured_light(
    args: ReconstructStructuredLightArgs,
) -> VbaResult<ReconstructOut> {
    validate_project_eagerly(&args.project)?;
    let payload = json!({
        "command": "reconstruct_structured_light", "version": 1,
        "project": &args.project,
        "correspondence_paths": &args.correspondence_paths,
        "sl_meta_path": &args.sl_meta_path,
        "intrinsics_path": &args.intrinsics_path,
        "pose_report_path": &args.pose_report_path,
    });
    let value = run_sidecar(SidecarRequest {
        subcommand: "reconstruct_structured_light".into(),
        payload, progress_tx: args.progress_tx, cancel: args.cancel,
    }).await?;
    let result: ResultData = serde_json::from_value(value).map_err(VbaError::BadEventJson)?;
    let ba_rms_px = result.ba_stats.rms_reprojection_px;
    let points: Vec<lmt_core::point::MeasuredPoint> =
        result.measured_points.into_iter().map(|dto| dto.into_ir()).collect();
    let measured_points = MeasuredPoints {
        screen_id: args.project.screen_id.clone(),
        coordinate_frame: identity_frame()?,
        cabinet_array: ipc_to_ir_cabinet(&args.project.cabinet_array)?,
        shape_prior: ipc_to_ir_shape(&args.project.shape_prior)?,
        points, sampling_mode: lmt_core::sampling::SamplingMode::Grid,
    };
    let cabinet_summaries = read_cabinet_summaries(&args.pose_report_path);
    Ok(ReconstructOut { measured_points, pose_report_path: args.pose_report_path, ba_rms_px, cabinet_summaries })
}
```

`crates/lmt-app/src/visual.rs` — first extract the persist+build block from `run_reconstruct` (lines ~154-217) into a private helper, then both call it:
```rust
fn persist_reconstruct_result(
    project_path: &Path, screen_id: &str, out: ReconstructOut,
) -> LmtResult<VisualReconstructResult> {
    // (the measured.yaml backup/rollback/atomic-write block currently inline in
    //  run_reconstruct, verbatim — uses out.measured_points / out.pose_report_path /
    //  out.ba_rms_px / out.cabinet_summaries; ends with the VisualReconstructResult build)
    ...
}

pub fn run_reconstruct_structured_light(
    project_path: &Path, screen_id: &str,
    sl_meta: &Path, intrinsics: &Path, correspondences: &[String],
) -> LmtResult<VisualReconstructResult> {
    let cfg = load_project_yaml_from_path(project_path)?;
    let screen_cfg = load_screen(&cfg, screen_id)?;
    let project = ipc::ReconstructProject {
        screen_id: screen_id.to_string(),
        cabinet_array: ipc_cabinet_array(screen_cfg),
        shape_prior: ipc_shape_prior(screen_cfg),
    };
    let measurements_dir = project_path.join("measurements");
    std::fs::create_dir_all(&measurements_dir)?;
    let pose_report_path = measurements_dir.join(format!("{screen_id}_cabinet_pose_report.json"));
    let args = ReconstructStructuredLightArgs {
        project,
        correspondence_paths: correspondences.to_vec(),
        sl_meta_path: sl_meta.display().to_string(),
        intrinsics_path: intrinsics.display().to_string(),
        pose_report_path: pose_report_path.display().to_string(),
        progress_tx: None, cancel: None,
    };
    let out = rt()?.block_on(reconstruct_structured_light(args)).map_err(map_vba_err)?;
    persist_reconstruct_result(project_path, screen_id, out)
}
```
(Add `reconstruct_structured_light`, `ReconstructStructuredLightArgs` to the `use adapter_visual_ba::api::{...}` import. `run_reconstruct` now ends with `persist_reconstruct_result(project_path, screen_id, out)`.)

`crates/lmt-cli/src/cli.rs` — new `VisualCmd` variant:
```rust
    /// 多机位结构光对应文件 → measured.yaml + cabinet_pose_report.json
    /// (model-constrained BA,复用 charuco 重建内核)。side_effect: destructive
    #[command(name = "reconstruct-structured-light")]
    ReconstructStructuredLight {
        /// 项目根目录。
        project_path: String,
        /// screen id。
        screen_id: String,
        /// sl_meta.json 路径(generate-structured-light 产出)。
        #[arg(long)]
        sl_meta: String,
        /// intrinsics.json 路径(visual calibrate 产出)。
        #[arg(long)]
        intrinsics: String,
        /// 每个机位一个 corr.json(decode-structured-light 产出);重复传入 >=2 个。
        #[arg(long = "corr", required = true, num_args = 1.., action = clap::ArgAction::Append)]
        correspondences: Vec<String>,
    },
```

`crates/lmt-cli/src/commands/visual.rs` — dispatch arm + handler:
```rust
        VisualCmd::ReconstructStructuredLight { project_path, screen_id, sl_meta, intrinsics, correspondences }
            => reconstruct_structured_light(mode, &project_path, &screen_id, &sl_meta, &intrinsics, &correspondences, yes, dry_run),
```
```rust
#[allow(clippy::too_many_arguments)]
fn reconstruct_structured_light(mode: Mode, project_path: &str, screen_id: &str,
        sl_meta: &str, intrinsics: &str, correspondences: &[String],
        yes: bool, dry_run: bool) -> i32 {
    let decision = match util::gate_destructive(yes, dry_run, "visual reconstruct-structured-light") {
        Ok(d) => d, Err(e) => return output::err(mode, e),
    };
    match decision {
        DestructiveDecision::DryRun => {
            let would_write = vec![
                format!("{project_path}/measurements/measured.yaml"),
                format!("{project_path}/measurements/{screen_id}_cabinet_pose_report.json"),
            ];
            output::ok(mode, serde_json::json!({
                "dry_run": true, "would_write": would_write,
                "correspondences": correspondences, "sl_meta": sl_meta, "intrinsics": intrinsics,
            }), |_| { let _ = writeln!(std::io::stdout(),
                "[dry-run] would reconstruct screen {screen_id} from {} poses", correspondences.len()); })
        }
        DestructiveDecision::Execute => match lmt_app::visual::run_reconstruct_structured_light(
                Path::new(project_path), screen_id, Path::new(sl_meta), Path::new(intrinsics), correspondences) {
            Ok(r) => output::ok(mode, r, |p| { let _ = writeln!(std::io::stdout(),
                "reconstructed {} cabinets (ba_rms={:.3}px)\n  measured: {}\n  poses: {}",
                p.cabinet_count, p.ba_rms_px, p.measured_yaml_path, p.pose_report_path); }),
            Err(e) => output::err(mode, ApiError::from(e)),
        },
    }
}
```

`crates/lmt-shared/src/manifest.rs` — add after the `visual.reconstruct` row (and to BOTH name lists at `:178` and `:229`):
```rust
        op("visual.reconstruct_structured_light",
           "Multi-view structured-light correspondences -> measured.yaml + cabinet_pose_report.json (model-constrained BA; provenance-gated; zero total station)",
           "lmt visual reconstruct-structured-light <project> <screen_id> --sl-meta <json> --intrinsics <json> --corr <c0.json> --corr <c1.json> ...",
           Destructive, true, false, false, Some("VisualReconstructResult"), &[0, 2, 3, 4, 13, 14, 17]),
```

- [ ] **Step 4: Run to verify it passes**

Run: `cargo test -p lmt-cli reconstruct_structured_light` then `cargo test --workspace`
Expected: new tests PASS; whole workspace green (incl. the existing `visual_reconstruct_structured_light_is_unsupported` — untouched). Also confirm `./target/debug/lmt visual reconstruct-structured-light --help` reads as human prose and `./target/debug/lmt --json manifest | jq '.data.operations[] | select(.id=="visual.reconstruct_structured_light")'` shows the row.

- [ ] **Step 5: Docs + (no) Tauri shim**

`docs/agents-cli.md` — add the command row:
```markdown
| `lmt visual reconstruct-structured-light <project> <screen_id> --sl-meta <json> --intrinsics <json> --corr <c.json> ...` | destructive | Reconstruct a metric per-cabinet 3D model from N per-pose structured-light correspondence files (decode-structured-light output). Provenance-gated (all `--corr` must share one `screen_id` + `sl_meta_sha256` matching `--sl-meta`, and match the project screen). Runs the same model-constrained BA as `reconstruct` (root cabinet = world gauge, scale from pixel pitch). Writes `measurements/measured.yaml` + `<screen>_cabinet_pose_report.json`. Errors: `invalid_input`(3) provenance/path, `intrinsics_invalid`(16), `detection_failed`(13), `observability_failed`(17), `ba_diverged`(14). |
```
Under the existing **"Not exposed in CLI"** section, confirm the note that the whole `visual` command group has no Tauri/GUI shim (CLI-only); `lmt_app::visual::run_reconstruct_structured_light` is the entry point a future GUI shim would call. (No `src-tauri` change — the `visual` group never had shims; matches `generate_structured_light` / `decode_structured_light`.)

- [ ] **Step 6: Commit**

```bash
git add crates/ docs/
git commit -m "feat(cli): visual reconstruct-structured-light (6-point contract, reuses VisualReconstructResult)"
```

---

## Self-Review

**1. Spec coverage** (vs the parent plan's Phase-3 design block, lines 1627-1676):
- N pose CorrespondenceFiles → provenance validation → assemble → init → BA → per-cabinet poses + surface → report. ✓ (Tasks 3.2/3.4)
- Provenance gate (parent Codex finding 4): shared `screen_id` + `sl_meta_sha256`, sha matches the `sl_meta.json` used, screen matches project. ✓ (`validate_sl_provenance`, 4 tests + run-level test)
- sl_meta integrity (rev2 finding 1): schema-validated, `screen_id` matched, cabinet-set matched to project present cells. ✓ (Task 3.4 step 1+4; `test_run_rejects_malformed_sl_meta`, `test_run_rejects_meta_project_cabinet_mismatch`)
- Screen-coordinate invariant (same `id` = same 3D point across poses), made STRUCTURAL (rev2 finding 2): `p_local` derives from canonical `sl_meta.dots[id].(u,v)`, not the per-pose correspondence copy; only camera `(x,y)` is taken from the corr file. ✓ (observation assembler + corr-u/v-ignored guard in the gating test)
- Reuse map honored: `model_constrained_ba`, `estimate_nonroot_cabinet_init`, `_pnp_camera`, `_undistort_obs`, `reconstruct_cabinet_geometry`, `_per_cabinet_reproj_rms`, `_classify_cabinet_quality`, `nominal_cabinet_centers_model_frame`, `_atomic_write_json`, output DTOs. ✓ (Task 3.3 `solve_and_emit` + Task 3.4)
- Metric gauge from per-cabinet pixel pitch; root cabinet = world. ✓ (sl_geometry uses `pixel_pitch_mm`; `ROOT_CABINET`)
- 6-point CLI contract. ✓ (Task 3.5: lmt-app helper, adapter, CLI subcommand, E2E refuse/dry-run + ignored happy, manifest row, docs; DTO reused so schema unchanged; Tauri shim documented as CLI-only)
- Gating test is now REAL (the parent plan's `assert True` placeholder is replaced by `test_synthetic_sl_reconstruction_recovers_cabinet_offset_mm`). ✓

**2. Placeholder scan:** none. Every code step has complete code; every command has expected output. The one conditional note (write_gp_project grid arity in Task 3.5 Step 1) gives an exact fallback referencing real neighboring tests.

**3. Type consistency:**
- `solve_and_emit(...)` parameter names/types identical in definition (3.3) and both call sites (3.3 charuco delegation, 3.4 SL). ✓
- `Observation(camera_idx, cabinet_idx, p_local, pixel)` matches `model_constrained_ba.py:21`. ✓
- `sl_local_mm(rect, u, v, pitch_x, pitch_y)` / `sl_cabinet_corners_mm(rect, pitch_x, pitch_y)` identical across 3.1 (def+test), 3.4 (observation assembler + corners_provider), gating test. ✓
- `ReconstructStructuredLightInput` fields (`project, correspondence_paths, sl_meta_path, intrinsics_path, pose_report_path`) identical across ipc.py (3.2), `__main__` entrypoint (3.4), adapter payload (3.5), tests. ✓
- Rust result reuses `VisualReconstructResult` (`dto.rs:242`) → manifest `Some("VisualReconstructResult")`; no new schema entry needed. ✓
- subcommand string `"reconstruct_structured_light"` identical in `__main__.py` (3.4) and adapter `api.rs` payload + `run_sidecar` (3.5). ✓
- CLI `--corr` (repeatable) → `correspondences: Vec<String>` → adapter `correspondence_paths` → IPC `correspondence_paths`. ✓
- `cabinet_id` parse `V{col:03d}_R{row:03d}` in `corners_provider` matches `reconstruct._cabinet_id` format (`reconstruct.py:147`). ✓

**4. Frame-convention pin:** `sl_local_mm` uses +y up; `test_y_axis_is_up` fails if anyone flips it. This is the one subtle correctness trap (mirrored cabinet poses) and it has a dedicated guard. ✓

---

## Open assumptions

1. **Intrinsics file shape** `{K, dist_coeffs, image_size}` — CONFIRMED: `calibrate.py:212-214` writes exactly `"K"` / `"dist_coeffs"` / `"image_size"` (+ `reproj_error_px`), and `reconstruct.py:226-229` reads those same three keys. The SL path reuses that reader pattern.
2. **Bridge-init topology** (genuine scope note) — `estimate_nonroot_cabinet_init` only does DIRECT root↔non-root bridging (documented limitation, `reconstruct.py:516`). For a single camera that can't frame the root cabinet + a far cabinet together (large walls), distant cabinets fall back to nominal init; BA still refines if some view chains them. This matches the existing ChArUco scope (monitor bench / small screens). Transitive bridging is out of scope for this plan — note it as future work, don't build it speculatively.
3. **`intrinsics_invalid` exit code** — CONFIRMED: `INTRINSICS_INVALID = 16` exists in both `exit_codes.rs:27` and `envelope.rs:126` (with `LmtError::IntrinsicsInvalid` mapping). The Task 3.4 `intrinsics_invalid` error code and the Task 3.5 manifest exit-code set are valid as written.

---

## Execution Handoff

Two execution options:
1. **Subagent-Driven (recommended)** — fresh subagent per task (3.1→3.5), review between tasks. Task 3.3 (refactor) and Task 3.4 (new core) are the review-critical ones.
2. **Inline Execution** — execute in this session with checkpoints after each task.

**Recommended order:** 3.1 → 3.2 → 3.3 (refactor, guarded by existing tests) → 3.4 (gating test must hit < 5mm) → 3.5 (Rust contract). Do NOT start 3.5 until the Python gating test passes — the contract is plumbing around a core that must already work.
