# Structured-Light Screen Reconstruction Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Recover an LED screen's real 3D model (per-cabinet positions/orientations, deviation from nominal) at millimeter accuracy using one ordinary camera moved between several poses, by displaying a binary-blink-coded dot array, decoding dense screen↔camera correspondences, and bundle-adjusting across poses tied by the screen-coordinate invariant.

**Architecture:** Four phases, each independently testable.
- **Phase 0 — Feasibility gate (Python).** A Monte-Carlo harness that models the *actual* reconstruction path: estimate each camera pose from noisy correspondences via PnP-against-nominal (with calibration-error and nominal-deviation perturbations), then triangulate. Reports achievable 3D RMS in mm. **Gates the whole project**: if mm is unreachable with the available rig, stop here. Produces the precision target Phase 3 re-confirms with full BA.
- **Phase 1 — Pattern generation.** New sidecar command `generate_structured_light` → a frame sequence (PNG frames + drop-in `sequence.mp4`) of a per-cabinet dot grid where each dot blinks a binary+parity code, bracketed by full-screen white sentinels and a full-on anchor frame, plus `sl_meta.json`. Mapping-aware (honors `screen_mapping`/absent cells). Full 6-point CLI contract.
- **Phase 2 — Decode.** New sidecar command `decode_structured_light` → segment a recorded video by the white sentinels, index frames by plateau detection, seed dot locations from the anchor frame, read each dot's blink code → screen `(u,v)` id, parity-check, write a provenance-stamped per-pose correspondence file. Full 6-point CLI contract.
- **Phase 3 — Reconstruct (multi-view BA).** Validate provenance across N pose correspondences, run multi-view bundle adjustment (reusing `ba.py`) tied by the shared screen coordinates, anchor metric scale via known pixel pitch, output per-cabinet poses + surface. **Designed here; expanded into its own detailed plan after Phase 0 passes and Phase 2 lands**, because its solver code must be written against real Phase-2 output and the existing `ba.py`.

**Tech Stack:** Python sidecar (`opencv-contrib-python` 4.11, `numpy<2`, `scipy<2`, `pydantic` v2, `pytest`); Rust workspace (`lmt-cli` clap CLI, `lmt-app` service layer, `adapter-visual-ba` sidecar IPC, `lmt-shared` DTO/envelope/schemas).

---

## Revision Log

**rev2 (post-Codex adversarial review)** — four high-severity findings accepted and folded in:
1. **Phase 0 was an oracle gate.** Rewrote `feasibility_rms_mm` to *estimate* poses via `cv2.solvePnP` against (optionally perturbed) nominal 3D and to perturb intrinsics, so the gate reflects PnP/intrinsic/deviation error — not a best-case lower bound. Phase 0 now depends on OpenCV.
2. **Generation ignored screen geometry.** Generation is now mapping-aware: it reuses `pattern.py::_resolve_cabinet_specs`, tiles dots inside each *present* cabinet's `input_rect_px`, honors absent cells, and inherits the even-divisibility / mapping-coverage guards (irregular screens without a valid mapping → `invalid_input`).
3. **`id=0` (all-off codeword) was undetectable.** The sequence now includes a full-on **anchor frame** right after the opening sentinel; decode seeds every dot location from the anchor, so a dot dark in all code frames (e.g. `id=0`) is still found and decoded. Added an `id=0` regression test.
4. **Correspondence files had no provenance.** `CorrespondenceFile` now carries `screen_id`, `sl_meta_sha256`, `screen_resolution`, `camera_image_size`, `source_input`; `sl_meta.json` carries `screen_id`. Phase 3 must validate that all pose files share one `screen_id` + `sl_meta_sha256` before BA.

---

## Scope Check (per writing-plans skill)

This spec spans **four independent subsystems**. Each phase is structured to produce working, testable software on its own:
- Phase 0 alone answers "can this rig hit mm under the *real* pose-estimation path?" — a usable deliverable.
- Phase 1 alone produces mapping-correct displayable patterns + metadata — verifiable without a camera.
- Phase 1+2 produce a full generate→decode loop verifiable on synthetic frames (no real camera needed).
- Phase 3 needs real captured data and the existing BA machinery; it is **deliberately left at design granularity** and becomes its own plan once Phases 0–2 land. Forcing line-by-line solver code now would be speculative fiction (CLAUDE.md: never present inference as verified fact).

**Recommendation:** Execute Phase 0 first as a hard gate. Do not start Phase 1 until Phase 0's numbers confirm mm is reachable with the intended rig.

---

## File Structure

**Python sidecar** (`python-sidecar/src/lmt_vba_sidecar/`):
- `sl_feasibility.py` — **new.** PnP-based Monte-Carlo precision model + scene/camera-ring builders. Uses `cv2.solvePnP`. No CLI exposure (analysis/validation tool).
- `sl_codec.py` — **new.** OpenCV-free helpers: id↔bit encoding with even parity, per-rect dot layout. Imported by both generation and decode (DRY).
- `structured_light.py` — **new.** `run_generate_structured_light`: mapping-aware per-cabinet dot frames + anchor + sentinels + `sequence.mp4` + `sl_meta.json` (atomic staging swap mirroring `pattern.py`).
- `sl_decode.py` — **new.** `run_decode_structured_light`: load frames → sentinel segmentation → plateau indexing → anchor-seeded dot detection → code decode + parity → provenance-stamped correspondence file.
- `ipc.py` — **modify.** Add `GenerateStructuredLightInput`, `DecodeStructuredLightInput`, `StructuredLightDot`, `CabinetRect`, `CodeSpec`, `SequenceSpec`, `StructuredLightMeta`, `CorrespondenceFile`; add `"decode_failed"` to `ErrorEvent.code` Literal. Reuse existing `GeneratePatternProject`.
- `__main__.py` — **modify.** Register the two new subcommands in `sub.add_parser`, `SUBCOMMAND_MODULES`, `SUBCOMMAND_ENTRYPOINTS`.
- `tests/` — **new** `test_sl_feasibility.py`, `test_sl_codec.py`, `test_generate_structured_light.py`, `test_sl_decode.py`; **modify** `test_main_dispatch.py`, `test_ipc.py`.

**Rust workspace:**
- `crates/adapter-visual-ba/src/ipc.rs` — **modify.** `StructuredLightMeta` (count mirror) + `CorrespondenceFile` (provenance mirror).
- `crates/adapter-visual-ba/src/api.rs` — **modify.** `GenerateStructuredLightArgs`/`Out` + `generate_structured_light`; `DecodeStructuredLightArgs`/`Out` + `decode_structured_light`.
- `crates/lmt-app/src/visual.rs` — **modify.** `run_generate_structured_light` (mapping-aware, mirrors `run_generate_pattern`), `run_decode_structured_light`.
- `crates/lmt-shared/src/dto.rs` — **modify.** `GenerateStructuredLightResult`, `DecodeStructuredLightResult`.
- `crates/lmt-shared/src/schema.rs` — **modify.** `add!(...)` for both DTOs.
- `crates/lmt-shared/src/manifest.rs` — **modify.** One `Operation` row per command.
- `crates/lmt-cli/src/cli.rs` — **modify.** `VisualCmd::GenerateStructuredLight` (with `--screen-mapping`), `VisualCmd::DecodeStructuredLight`.
- `crates/lmt-cli/src/commands/visual.rs` — **modify.** Both handlers (gate_destructive → DryRun/Execute → envelope).
- `crates/lmt-cli/tests/cli_e2e.rs` — **modify.** Refuse/dry-run tests (no sidecar) + `#[ignore]` happy tests (real sidecar).
- `docs/agents-cli.md` — **modify.** Command rows; clarify `decode_failed`.
- `src-tauri/src/commands/visual.rs` — **modify.** Thin Tauri shims.

---

## PHASE 0 — Feasibility Gate

**Deliverable:** `python-sidecar/.venv/bin/python -m lmt_vba_sidecar.sl_feasibility` prints a precision table that reflects PnP pose estimation + intrinsic error; pytest proves the model is sound. Decision: proceed only if mm is reachable.

### Task 0.1: Projection + PnP pose estimation + triangulation core

**Files:**
- Create: `python-sidecar/src/lmt_vba_sidecar/sl_feasibility.py`
- Test: `python-sidecar/tests/test_sl_feasibility.py`

- [ ] **Step 1: Write the failing test**

```python
# python-sidecar/tests/test_sl_feasibility.py
import numpy as np
import pytest
from lmt_vba_sidecar.sl_feasibility import (
    project_point, triangulate_multiview, look_at_pose, solve_pnp_pose,
)

def _K(f=3000.0, cx=1920.0, cy=1080.0):
    return np.array([[f, 0, cx], [0, f, cy], [0, 0, 1]], float)

def test_project_then_triangulate_is_exact_without_noise():
    K = _K()
    X = np.array([100.0, -50.0, 0.0])
    poses = [look_at_pose(np.array([-1500.0, 0.0, -4000.0])),
             look_at_pose(np.array([1500.0, 0.0, -4000.0]))]
    pts = [project_point(K, R, t, X) for (R, t) in poses]
    Xhat = triangulate_multiview(K, poses, pts)
    assert np.linalg.norm(Xhat - X) < 1e-6

def test_triangulate_requires_two_views():
    K = _K()
    with pytest.raises(ValueError):
        triangulate_multiview(K, [look_at_pose(np.array([0.0, 0.0, -4000.0]))],
                              [np.array([1920.0, 1080.0])])

def test_solve_pnp_recovers_true_pose_without_noise():
    K = _K()
    R_true, t_true = look_at_pose(np.array([800.0, 0.0, -4000.0]))
    # a non-trivial (slightly curved) object so PnP is well-posed
    obj = np.array([[x, y, 5.0 * np.cos(x / 500.0)]
                    for y in (-600, 0, 600) for x in (-900, -300, 300, 900)], float)
    img = np.array([project_point(K, R_true, t_true, X) for X in obj])
    R_est, t_est = solve_pnp_pose(K, obj, img)
    assert np.linalg.norm(R_est - R_true) < 1e-3
    assert np.linalg.norm(t_est - t_true) < 1e-2
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cd python-sidecar && .venv/bin/python -m pytest tests/test_sl_feasibility.py -v`
Expected: FAIL with `ModuleNotFoundError: No module named 'lmt_vba_sidecar.sl_feasibility'`

- [ ] **Step 3: Write minimal implementation**

```python
# python-sidecar/src/lmt_vba_sidecar/sl_feasibility.py
"""Feasibility model for structured-light screen reconstruction.

Models the ACTUAL reconstruction path so the gate is honest:
  1. project true 3D screen points into N views (true poses, true K)
  2. add Gaussian centroid noise
  3. ESTIMATE each camera pose with cv2.solvePnP against the nominal model
     (the as-built screen the pipeline assumes), using the CAMERA'S believed K
  4. triangulate with the ESTIMATED poses and believed K
This captures PnP pose error, intrinsic/calibration error, and nominal-deviation
error — not just centroid noise. The definitive gate is re-confirmed by Phase 3's
full BA, but this is a valid stop/proceed screen before any production code.
"""
from __future__ import annotations

import cv2
import numpy as np

Pose = tuple[np.ndarray, np.ndarray]  # (R world->cam 3x3, t world->cam 3,)


def project_point(K: np.ndarray, R: np.ndarray, t: np.ndarray, X: np.ndarray) -> np.ndarray:
    xc = R @ X + t
    p = K @ xc
    return p[:2] / p[2]


def triangulate_multiview(K: np.ndarray, poses: list[Pose], pts2d: list[np.ndarray]) -> np.ndarray:
    if len(poses) < 2:
        raise ValueError("triangulation needs >= 2 camera poses")
    rows = []
    for (R, t), (x, y) in zip(poses, pts2d):
        P = K @ np.hstack([R, t.reshape(3, 1)])
        rows.append(x * P[2] - P[0])
        rows.append(y * P[2] - P[1])
    _, _, Vt = np.linalg.svd(np.asarray(rows))
    Xh = Vt[-1]
    return Xh[:3] / Xh[3]


def look_at_pose(cam_pos_mm: np.ndarray, target_mm: np.ndarray | None = None,
                 up: np.ndarray | None = None) -> Pose:
    target_mm = np.zeros(3) if target_mm is None else target_mm
    up = np.array([0.0, 1.0, 0.0]) if up is None else up
    z = target_mm - cam_pos_mm
    z = z / np.linalg.norm(z)
    x = np.cross(up, z)
    x = x / np.linalg.norm(x)
    y = np.cross(z, x)
    R = np.stack([x, y, z], axis=0)
    return R, -R @ cam_pos_mm


def solve_pnp_pose(K: np.ndarray, object_pts_mm: np.ndarray, image_pts: np.ndarray) -> Pose:
    """Estimate (R, t) from 3D-2D correspondences. SQPNP handles planar and
    general configurations without an initial guess (cv2 4.11)."""
    obj = np.ascontiguousarray(np.asarray(object_pts_mm, float).reshape(-1, 1, 3))
    img = np.ascontiguousarray(np.asarray(image_pts, float).reshape(-1, 1, 2))
    ok, rvec, tvec = cv2.solvePnP(obj, img, K, None, flags=cv2.SOLVEPNP_SQPNP)
    if not ok:
        raise ValueError("solvePnP failed")
    R, _ = cv2.Rodrigues(rvec)
    return R, tvec.reshape(3)
```

- [ ] **Step 4: Run test to verify it passes**

Run: `cd python-sidecar && .venv/bin/python -m pytest tests/test_sl_feasibility.py -v`
Expected: PASS (3 passed)

- [ ] **Step 5: Commit**

```bash
git add python-sidecar/src/lmt_vba_sidecar/sl_feasibility.py python-sidecar/tests/test_sl_feasibility.py
git commit -m "feat(sidecar): SL feasibility core (project/PnP/triangulate)"
```

### Task 0.2: PnP-based Monte-Carlo gate + builders + report

**Files:**
- Modify: `python-sidecar/src/lmt_vba_sidecar/sl_feasibility.py`
- Test: `python-sidecar/tests/test_sl_feasibility.py`

- [ ] **Step 1: Write the failing test**

```python
# append to python-sidecar/tests/test_sl_feasibility.py
from lmt_vba_sidecar.sl_feasibility import (
    build_screen, camera_ring, feasibility_rms_mm,
)

def test_zero_perturbation_is_near_exact():
    K = _K()
    pts = build_screen(2000.0, 1200.0, 6, 5, curve_mm=20.0)  # mild curvature -> well-posed PnP
    poses = camera_ring(4000.0, 4, 45.0)
    s = feasibility_rms_mm(K=K, screen_points_mm=pts, camera_poses=poses,
                           pixel_sigma=0.0, nominal_deviation_mm=0.0,
                           focal_err_frac=0.0, trials=3, seed=0)
    assert s["rms_mm"] < 1e-2

def test_estimated_pose_is_worse_than_oracle():
    """The PnP gate must not be more optimistic than oracle triangulation."""
    K = _K()
    pts = build_screen(2000.0, 1200.0, 6, 5, curve_mm=20.0)
    poses = camera_ring(4000.0, 4, 45.0)
    est = feasibility_rms_mm(K=K, screen_points_mm=pts, camera_poses=poses,
                             pixel_sigma=0.1, trials=20, seed=2)
    # oracle: triangulate with TRUE poses (no PnP) at the same noise
    rng = np.random.default_rng(2)
    errs = []
    for _ in range(20):
        for X in pts:
            obs = [project_point(K, R, t, X) + rng.normal(0, 0.1, 2) for (R, t) in poses]
            errs.append(np.linalg.norm(triangulate_multiview(K, poses, obs) - X))
    oracle_rms = float(np.sqrt(np.mean(np.square(errs))))
    assert est["rms_mm"] >= oracle_rms

def test_focal_error_increases_rms():
    K = _K()
    pts = build_screen(2000.0, 1200.0, 6, 5, curve_mm=20.0)
    poses = camera_ring(4000.0, 5, 50.0)
    base = feasibility_rms_mm(K=K, screen_points_mm=pts, camera_poses=poses,
                              pixel_sigma=0.1, focal_err_frac=0.0, trials=20, seed=3)
    perturbed = feasibility_rms_mm(K=K, screen_points_mm=pts, camera_poses=poses,
                                   pixel_sigma=0.1, focal_err_frac=0.02, trials=20, seed=3)
    assert perturbed["rms_mm"] > base["rms_mm"]
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cd python-sidecar && .venv/bin/python -m pytest tests/test_sl_feasibility.py -v`
Expected: FAIL with `ImportError: cannot import name 'build_screen'`

- [ ] **Step 3: Write minimal implementation**

```python
# append to python-sidecar/src/lmt_vba_sidecar/sl_feasibility.py

def build_screen(width_mm: float, height_mm: float, nx: int, ny: int,
                 curve_mm: float = 0.0) -> np.ndarray:
    """(nx*ny, 3) grid centered at origin. curve_mm bows it along z (a mild
    cylinder) so PnP is non-degenerate; curve_mm=0 gives a flat (planar) wall."""
    xs = np.linspace(-width_mm / 2, width_mm / 2, nx)
    ys = np.linspace(-height_mm / 2, height_mm / 2, ny)
    pts = []
    for y in ys:
        for x in xs:
            z = curve_mm * (1.0 - (x / (width_mm / 2)) ** 2) if width_mm > 0 else 0.0
            pts.append([x, y, z])
    return np.asarray(pts, float)


def camera_ring(distance_mm: float, n: int, span_deg: float,
                target_mm: np.ndarray | None = None) -> list[Pose]:
    target_mm = np.zeros(3) if target_mm is None else target_mm
    angs = np.linspace(-span_deg / 2, span_deg / 2, n) if n > 1 else np.array([0.0])
    poses: list[Pose] = []
    for a in np.deg2rad(angs):
        pos = target_mm + np.array([distance_mm * np.sin(a), 0.0, -distance_mm * np.cos(a)])
        poses.append(look_at_pose(pos, target_mm))
    return poses


def feasibility_rms_mm(*, K: np.ndarray, screen_points_mm: np.ndarray,
                       camera_poses: list[Pose], pixel_sigma: float,
                       nominal_deviation_mm: float = 0.0, focal_err_frac: float = 0.0,
                       trials: int = 50, seed: int = 0) -> dict:
    """Monte-Carlo of the real path: observe (true K + noise) -> estimate poses
    via PnP against nominal (true + deviation) with believed K (true * focal err)
    -> triangulate with estimated poses + believed K -> 3D error vs truth."""
    if len(camera_poses) < 2:
        raise ValueError("feasibility needs >= 2 camera poses")
    rng = np.random.default_rng(seed)
    errs: list[float] = []
    for _ in range(trials):
        nominal = screen_points_mm.copy()
        if nominal_deviation_mm > 0:
            nominal = nominal + rng.normal(0.0, nominal_deviation_mm, nominal.shape)
        Kc = K.copy()
        if focal_err_frac > 0:
            f = K[0, 0] * (1.0 + rng.normal(0.0, focal_err_frac))
            Kc[0, 0] = f
            Kc[1, 1] = f
        obs = []
        for (R, t) in camera_poses:
            view = []
            for X in screen_points_mm:
                p = project_point(K, R, t, X)
                if pixel_sigma > 0:
                    p = p + rng.normal(0.0, pixel_sigma, 2)
                view.append(p)
            obs.append(view)
        try:
            est = [solve_pnp_pose(Kc, nominal, np.asarray(view)) for view in obs]
        except ValueError:
            continue  # degenerate trial; skip
        for i, X in enumerate(screen_points_mm):
            Xhat = triangulate_multiview(Kc, est, [obs[v][i] for v in range(len(est))])
            errs.append(float(np.linalg.norm(Xhat - X)))
    a = np.asarray(errs)
    return {
        "rms_mm": float(np.sqrt((a ** 2).mean())),
        "median_mm": float(np.median(a)),
        "p95_mm": float(np.percentile(a, 95)),
        "n_points": int(len(screen_points_mm)),
        "n_views": int(len(camera_poses)),
    }


def _report() -> None:
    """Operator sweep: edit rig numbers, run `python -m lmt_vba_sidecar.sl_feasibility`.
    Numbers INCLUDE PnP pose error + intrinsic error, so they reflect the real path."""
    K = np.array([[3000.0, 0, 1920.0], [0, 3000.0, 1080.0], [0, 0, 1]], float)
    pts = build_screen(3000.0, 1800.0, 7, 5, curve_mm=30.0)
    print(f"{'dist':>6} {'views':>6} {'span':>5} {'sigma':>6} {'devmm':>6} "
          f"{'fperr':>6} {'rms_mm':>8} {'p95_mm':>8}")
    for dist in (3000.0, 6000.0):
        for span in (25.0, 50.0, 70.0):
            for sigma in (0.1, 0.3):
                for fperr in (0.0, 0.01):
                    s = feasibility_rms_mm(K=K, screen_points_mm=pts,
                                           camera_poses=camera_ring(dist, 5, span),
                                           pixel_sigma=sigma, nominal_deviation_mm=2.0,
                                           focal_err_frac=fperr, trials=30, seed=0)
                    print(f"{dist:6.0f} {5:6d} {span:5.0f} {sigma:6.2f} {2.0:6.1f} "
                          f"{fperr:6.2f} {s['rms_mm']:8.3f} {s['p95_mm']:8.3f}")


if __name__ == "__main__":
    _report()
```

- [ ] **Step 4: Run test to verify it passes**

Run: `cd python-sidecar && .venv/bin/python -m pytest tests/test_sl_feasibility.py -v`
Expected: PASS (6 passed)

- [ ] **Step 5: Run the operator sweep and record the gate decision**

Run: `cd python-sidecar && .venv/bin/python -m lmt_vba_sidecar.sl_feasibility`
Expected: a printed table whose RMS/p95 already include PnP + intrinsic error. **GATE:** find the row matching your real rig (distance, achievable span, expected centroid noise, your calibration's focal error, your wall's deviation-from-nominal). If `rms_mm`/`p95_mm` meet your mm tolerance (SX12 baseline), proceed to Phase 1. If not, STOP and fix the rig (closer / wider baseline / better calibration / tile the wall) before building. Phase 3 re-confirms with full BA.

- [ ] **Step 6: Commit**

```bash
git add python-sidecar/src/lmt_vba_sidecar/sl_feasibility.py python-sidecar/tests/test_sl_feasibility.py
git commit -m "feat(sidecar): PnP-based SL feasibility gate + operator sweep"
```

---

## PHASE 1 — Pattern Generation

**Deliverable:** `lmt visual generate-structured-light <project> <screen_id> --yes [--screen-mapping <json>]` writes `patterns/<screen_id>/sl/{frames/, sequence.mp4, sl_meta.json}`, honoring screen mapping / absent cells.

### Task 1.1: Shared codec helpers (id↔bits + parity + per-rect dot layout)

**Files:**
- Create: `python-sidecar/src/lmt_vba_sidecar/sl_codec.py`
- Test: `python-sidecar/tests/test_sl_codec.py`

- [ ] **Step 1: Write the failing test**

```python
# python-sidecar/tests/test_sl_codec.py
from lmt_vba_sidecar.sl_codec import (
    data_bits_for, even_parity, encode_id, decode_bits, build_dots_in_rect,
)

def test_data_bits_for():
    assert data_bits_for(1) == 1
    assert data_bits_for(2) == 1
    assert data_bits_for(3) == 2
    assert data_bits_for(1000) == 10

def test_encode_decode_roundtrip_including_zero():
    db = data_bits_for(500)
    for i in (0, 1, 255, 499):       # 0 must round-trip
        bits = encode_id(i, db)
        assert len(bits) == db + 1
        assert decode_bits(bits, db) == i

def test_decode_rejects_bad_parity():
    db = data_bits_for(500)
    bits = encode_id(42, db)
    bits[-1] ^= 1
    assert decode_bits(bits, db) is None

def test_build_dots_in_rect_places_inside_with_margin():
    # rect (x=100,y=50,w=960,h=540), spacing 240, margin 120, ids from 7
    dots = build_dots_in_rect(rect=(100, 50, 960, 540), spacing_px=240,
                              margin_px=120, id_start=7)
    assert dots[0][0] == 7
    for (_id, u, v) in dots:
        assert 100 + 120 <= u <= 100 + 960 - 120
        assert 50 + 120 <= v <= 50 + 540 - 120
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cd python-sidecar && .venv/bin/python -m pytest tests/test_sl_codec.py -v`
Expected: FAIL with `ModuleNotFoundError: No module named 'lmt_vba_sidecar.sl_codec'`

- [ ] **Step 3: Write minimal implementation**

```python
# python-sidecar/src/lmt_vba_sidecar/sl_codec.py
"""OpenCV-free shared helpers for the structured-light dot codec.

A dot's identity is carried in TIME (its on/off blink sequence), not appearance.
Each id is `data_bits` little-endian binary bits + one trailing even-parity bit.
The all-off codeword (id=0) is legal: the decoder seeds dot locations from a
full-on ANCHOR frame, so a dot dark in every code frame is still found.
"""
from __future__ import annotations

import math


def data_bits_for(n_dots: int) -> int:
    if n_dots <= 1:
        return 1
    return max(1, math.ceil(math.log2(n_dots)))


def even_parity(bits: list[int]) -> int:
    return sum(bits) & 1


def encode_id(dot_id: int, data_bits: int) -> list[int]:
    bits = [(dot_id >> b) & 1 for b in range(data_bits)]
    bits.append(even_parity(bits))
    return bits


def decode_bits(bits: list[int], data_bits: int) -> int | None:
    if len(bits) != data_bits + 1:
        return None
    data = bits[:data_bits]
    if even_parity(data) != bits[-1]:
        return None
    return sum(b << i for i, b in enumerate(data))


def build_dots_in_rect(*, rect: tuple[int, int, int, int], spacing_px: int,
                       margin_px: int, id_start: int) -> list[tuple[int, int, int]]:
    """Row-major dot centers inside one cabinet's placement rect [x,y,w,h],
    inset by margin_px. Returns [(id, u, v), ...] with ids from id_start."""
    x, y, w, h = rect
    us = list(range(x + margin_px, x + w - margin_px + 1, spacing_px))
    vs = list(range(y + margin_px, y + h - margin_px + 1, spacing_px))
    dots: list[tuple[int, int, int]] = []
    i = id_start
    for v in vs:
        for u in us:
            dots.append((i, u, v))
            i += 1
    return dots
```

- [ ] **Step 4: Run test to verify it passes**

Run: `cd python-sidecar && .venv/bin/python -m pytest tests/test_sl_codec.py -v`
Expected: PASS (4 passed)

- [ ] **Step 5: Commit**

```bash
git add python-sidecar/src/lmt_vba_sidecar/sl_codec.py python-sidecar/tests/test_sl_codec.py
git commit -m "feat(sidecar): SL dot codec (id/parity, per-rect grid, id=0 legal)"
```

### Task 1.2: IPC models for generation + decode + sl_meta + provenance

**Files:**
- Modify: `python-sidecar/src/lmt_vba_sidecar/ipc.py`
- Test: `python-sidecar/tests/test_ipc.py`

- [ ] **Step 1: Write the failing test**

```python
# append to python-sidecar/tests/test_ipc.py
from lmt_vba_sidecar.ipc import GenerateStructuredLightInput, StructuredLightMeta, CorrespondenceFile

def test_generate_structured_light_input_mirrors_generate_pattern():
    m = GenerateStructuredLightInput.model_validate({
        "command": "generate_structured_light", "version": 1,
        "project": {"screen_id": "MAIN",
                    "cabinet_array": {"cols": 1, "rows": 1,
                                      "absent_cells": [], "cabinet_size_mm": [500, 500]}},
        "output_dir": "/tmp/out", "screen_resolution": [1920, 1080],
    })
    assert m.dot_spacing_px == 64 and m.screen_mapping_path is None

def test_meta_and_correspondence_carry_provenance():
    meta = StructuredLightMeta.model_validate({
        "schema_version": 1, "screen_id": "MAIN", "screen_resolution": [1920, 1080],
        "dot_radius_px": 6,
        "code": {"data_bits": 9, "total_bits": 10, "parity": "even", "encoding": "binary"},
        "sequence": {"sentinel": "white_full", "anchor": "all_on",
                     "n_code_frames": 10, "hold_ms": 500, "fps": 30},
        "cabinets": [{"col": 0, "row": 0, "input_rect_px": [0, 0, 540, 540],
                      "pixel_pitch_mm": [0.93, 0.93]}],
        "dots": [{"id": 0, "u": 240.0, "v": 240.0, "cabinet": [0, 0]}],
    })
    assert meta.screen_id == "MAIN" and meta.dots[0].cabinet == [0, 0]
    corr = CorrespondenceFile.model_validate({
        "schema_version": 1, "screen_id": "MAIN", "sl_meta_sha256": "abc",
        "screen_resolution": [1920, 1080], "camera_image_size": [4000, 3000],
        "source_input": "/cap/pose1.mp4",
        "points": [{"id": 0, "u": 240.0, "v": 240.0, "x": 12.0, "y": 34.0}],
    })
    assert corr.sl_meta_sha256 == "abc" and corr.points[0].id == 0
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cd python-sidecar && .venv/bin/python -m pytest tests/test_ipc.py -k "structured_light or provenance" -v`
Expected: FAIL with `ImportError: cannot import name 'GenerateStructuredLightInput'`

- [ ] **Step 3: Write minimal implementation**

Add to `python-sidecar/src/lmt_vba_sidecar/ipc.py` (after `GeneratePatternInput`; it reuses the existing `GeneratePatternProject`, `PositiveIntPair`, `PositiveSizePair`):

```python
class GenerateStructuredLightInput(BaseModel):
    command: Literal["generate_structured_light"]
    version: Literal[1]
    project: GeneratePatternProject
    output_dir: str
    screen_resolution: PositiveIntPair
    # When set, per-cabinet placement (input_rect_px) + pitch come from this
    # screen_mapping.json — same single-source-of-truth contract as generate_pattern.
    screen_mapping_path: str | None = None
    dot_spacing_px: int = Field(gt=0, default=64)
    dot_radius_px: int = Field(gt=0, default=6)
    margin_px: int = Field(ge=0, default=64)
    hold_ms: int = Field(gt=0, default=500)
    fps: int = Field(gt=0, default=30)


class StructuredLightDot(BaseModel):
    id: int = Field(ge=0)
    u: float
    v: float
    cabinet: Annotated[list[int], Field(min_length=2, max_length=2)]


class CabinetRect(BaseModel):
    col: int
    row: int
    input_rect_px: Annotated[list[int], Field(min_length=4, max_length=4)]
    pixel_pitch_mm: PositiveSizePair


class CodeSpec(BaseModel):
    data_bits: int = Field(ge=1)
    total_bits: int = Field(ge=2)
    parity: Literal["even"] = "even"
    encoding: Literal["binary"] = "binary"


class SequenceSpec(BaseModel):
    sentinel: Literal["white_full"] = "white_full"
    anchor: Literal["all_on"] = "all_on"
    n_code_frames: int = Field(ge=1)   # == code.total_bits
    hold_ms: int = Field(gt=0)
    fps: int = Field(gt=0)


class StructuredLightMeta(BaseModel):
    schema_version: Literal[1]
    screen_id: str
    screen_resolution: PositiveIntPair
    dot_radius_px: int = Field(gt=0)
    code: CodeSpec
    sequence: SequenceSpec
    cabinets: list[CabinetRect]
    dots: list[StructuredLightDot]


class DecodeStructuredLightInput(BaseModel):
    command: Literal["decode_structured_light"]
    version: Literal[1]
    input_path: str           # a video file OR a directory of frame images
    sl_meta_path: str
    output_path: str
    sentinel_threshold: float = Field(gt=0.0, le=1.0, default=0.85)


class CorrespondencePoint(BaseModel):
    id: int = Field(ge=0)
    u: float   # screen pixel (from sl_meta)
    v: float
    x: float   # camera pixel (sub-pixel centroid)
    y: float


class CorrespondenceFile(BaseModel):
    schema_version: Literal[1]
    screen_id: str
    sl_meta_sha256: str        # provenance: which pattern/meta produced this
    screen_resolution: PositiveIntPair
    camera_image_size: Annotated[list[int], Field(min_length=2, max_length=2)]
    source_input: str          # the decoded video/dir path
    points: list[CorrespondencePoint]
```

Add `"decode_failed"` to the `ErrorEvent.code` Literal (insert before `"internal_error"`).

- [ ] **Step 4: Run test to verify it passes**

Run: `cd python-sidecar && .venv/bin/python -m pytest tests/test_ipc.py -k "structured_light or provenance" -v`
Expected: PASS (2 passed)

- [ ] **Step 5: Commit**

```bash
git add python-sidecar/src/lmt_vba_sidecar/ipc.py python-sidecar/tests/test_ipc.py
git commit -m "feat(sidecar): SL IPC models (mapping-aware input, provenance meta/corr)"
```

### Task 1.3: Mapping-aware rendering (anchor + sentinels) + sl_meta + dispatch

**Files:**
- Create: `python-sidecar/src/lmt_vba_sidecar/structured_light.py`
- Modify: `python-sidecar/src/lmt_vba_sidecar/__main__.py`
- Test: `python-sidecar/tests/test_generate_structured_light.py`

- [ ] **Step 1: Write the failing test**

```python
# python-sidecar/tests/test_generate_structured_light.py
import json
import cv2
from lmt_vba_sidecar.ipc import GenerateStructuredLightInput
from lmt_vba_sidecar.structured_light import run_generate_structured_light
from lmt_vba_sidecar.sl_codec import data_bits_for

def _run(tmp_path, cols=1, rows=1, **over):
    cmd = GenerateStructuredLightInput.model_validate({
        "command": "generate_structured_light", "version": 1,
        "project": {"screen_id": "MAIN",
                    "cabinet_array": {"cols": cols, "rows": rows,
                                      "absent_cells": [], "cabinet_size_mm": [500, 500]}},
        "output_dir": str(tmp_path / "sl"), "screen_resolution": [480 * cols, 480 * rows],
        "dot_spacing_px": 160, "margin_px": 80, **over,
    })
    return run_generate_structured_light(cmd)

def test_frame_count_includes_anchor_and_two_sentinels(tmp_path):
    assert _run(tmp_path) == 0
    out = tmp_path / "sl"
    meta = json.loads((out / "sl_meta.json").read_text())
    total_bits = meta["code"]["total_bits"]
    frames = sorted((out / "frames").glob("frame_*.png"))
    # WHITE + ALLON + total_bits code frames + WHITE
    assert len(frames) == total_bits + 3
    assert meta["screen_id"] == "MAIN"
    assert (out / "sequence.mp4").exists()

def test_sentinels_white_and_anchor_lights_every_dot(tmp_path):
    _run(tmp_path)
    out = tmp_path / "sl"
    meta = json.loads((out / "sl_meta.json").read_text())
    frames = sorted((out / "frames").glob("frame_*.png"))
    first = cv2.imread(str(frames[0]), cv2.IMREAD_GRAYSCALE)
    last = cv2.imread(str(frames[-1]), cv2.IMREAD_GRAYSCALE)
    assert int(first.min()) == 255 and int(last.min()) == 255  # white sentinels
    anchor = cv2.imread(str(frames[1]), cv2.IMREAD_GRAYSCALE)   # all-on anchor
    for d in meta["dots"]:                                      # every dot lit, incl id 0
        assert int(anchor[int(d["v"]), int(d["u"])]) == 255

def test_absent_cabinet_gets_no_dots(tmp_path):
    cmd = GenerateStructuredLightInput.model_validate({
        "command": "generate_structured_light", "version": 1,
        "project": {"screen_id": "MAIN",
                    "cabinet_array": {"cols": 2, "rows": 1, "absent_cells": [[1, 0]],
                                      "cabinet_size_mm": [500, 500]}},
        "output_dir": str(tmp_path / "sl"), "screen_resolution": [960, 480],
        "dot_spacing_px": 160, "margin_px": 80,
    })
    assert run_generate_structured_light(cmd) == 0
    meta = json.loads((tmp_path / "sl" / "sl_meta.json").read_text())
    # no dot lands in the absent right-half cabinet (u >= 480)
    assert all(d["u"] < 480 for d in meta["dots"])
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cd python-sidecar && .venv/bin/python -m pytest tests/test_generate_structured_light.py -v`
Expected: FAIL with `ModuleNotFoundError: No module named 'lmt_vba_sidecar.structured_light'`

- [ ] **Step 3: Write minimal implementation**

```python
# python-sidecar/src/lmt_vba_sidecar/structured_light.py
"""Mapping-aware structured-light dot-array generation.

Reuses pattern.py::_resolve_cabinet_specs so placement honors screen_mapping /
absent cells / non-uniform cabinets exactly like generate_pattern. Dots are
tiled inside each PRESENT cabinet's input_rect_px.

Frame sequence (display order):
  [WHITE sentinel] [ALL-ON anchor] [code_0 .. code_{B-1}] [WHITE sentinel]
B = total_bits = data_bits + 1. The anchor lights every dot so the decoder can
seed all dot locations (incl. the all-off id=0). Outputs frames/, sequence.mp4,
sl_meta.json via the same atomic staging swap as pattern.py.
"""
from __future__ import annotations

import hashlib
import json
import pathlib
import shutil
import tempfile

import cv2
import numpy as np

from lmt_vba_sidecar.io_utils import write_event
from lmt_vba_sidecar.ipc import (
    BaStats, ErrorEvent, GenerateStructuredLightInput, ProgressEvent, ResultData, ResultEvent,
)
from lmt_vba_sidecar.pattern import _resolve_cabinet_specs
from lmt_vba_sidecar.sl_codec import build_dots_in_rect, data_bits_for, encode_id

ATOMIC_BACKUP_SUFFIX = ".lmt-sl-old"


def _draw_dots(w: int, h: int, dots, lit_ids: set[int], radius: int) -> np.ndarray:
    img = np.zeros((h, w), dtype=np.uint8)
    for (did, u, v) in dots:
        if did in lit_ids:
            cv2.circle(img, (int(u), int(v)), int(radius), 255, -1, cv2.LINE_AA)
    return img


def run_generate_structured_light(cmd: GenerateStructuredLightInput) -> int:
    w, h = cmd.screen_resolution
    cols = cmd.project.cabinet_array.cols
    rows = cmd.project.cabinet_array.rows
    absent = set(tuple(c) for c in cmd.project.cabinet_array.absent_cells)

    screen_mapping = None
    if cmd.screen_mapping_path is not None:
        from lmt_vba_sidecar.screen_mapping import ScreenMapping
        try:
            screen_mapping = ScreenMapping.model_validate_json(
                pathlib.Path(cmd.screen_mapping_path).read_text())
        except (OSError, ValueError) as exc:
            write_event(ErrorEvent(event="error", code="invalid_input",
                message=f"screen_mapping load/validate failed: {exc}", fatal=True))
            return 1

    # Uniform path requires even divisibility (mirror pattern.py). Mapping path
    # defines placement via input_rect_px, so divisibility is irrelevant there.
    if screen_mapping is None and (w % cols != 0 or h % rows != 0):
        write_event(ErrorEvent(event="error", code="invalid_input",
            message=f"screen_resolution {w}x{h} must divide evenly by grid {cols}x{rows}",
            fatal=True))
        return 1

    try:
        specs = _resolve_cabinet_specs(
            cols=cols, rows=rows, absent=absent, screen_resolution=(w, h),
            screen_mapping=screen_mapping,
            cabinet_size_mm=list(cmd.project.cabinet_array.cabinet_size_mm))
    except ValueError as exc:
        write_event(ErrorEvent(event="error", code="invalid_input", message=str(exc), fatal=True))
        return 1

    # Tile dots inside each present cabinet's placement rect; global row-major ids.
    dots: list[tuple[int, int, int]] = []
    dot_cabinet: dict[int, tuple[int, int]] = {}
    for s in specs:
        rect = tuple(int(v) for v in s["input_rect_px"])
        cab_dots = build_dots_in_rect(rect=rect, spacing_px=cmd.dot_spacing_px,
                                      margin_px=cmd.margin_px, id_start=len(dots))
        for (did, u, v) in cab_dots:
            dot_cabinet[did] = (s["col"], s["row"])
        dots.extend(cab_dots)

    if len(dots) < 4:
        write_event(ErrorEvent(event="error", code="invalid_input",
            message=f"only {len(dots)} dots fit; reduce dot_spacing/margin", fatal=True))
        return 1

    db = data_bits_for(len(dots))
    total_bits = db + 1
    lit_by_bit: list[set[int]] = [set() for _ in range(total_bits)]
    all_ids = {did for (did, _u, _v) in dots}
    for (did, _u, _v) in dots:
        for b, bit in enumerate(encode_id(did, db)):
            if bit:
                lit_by_bit[b].add(did)

    out_dir = pathlib.Path(cmd.output_dir)
    out_dir.parent.mkdir(parents=True, exist_ok=True)
    staging = pathlib.Path(tempfile.mkdtemp(prefix=f".{out_dir.name}-staging-", dir=str(out_dir.parent)))
    frames_dir = staging / "frames"
    frames_dir.mkdir(parents=True)

    try:
        white = np.full((h, w), 255, dtype=np.uint8)
        anchor = _draw_dots(w, h, dots, all_ids, cmd.dot_radius_px)
        logical = [white, anchor] + [_draw_dots(w, h, dots, lit_by_bit[b], cmd.dot_radius_px)
                                     for b in range(total_bits)] + [white]
        for i, img in enumerate(logical):
            cv2.imwrite(str(frames_dir / f"frame_{i:04d}.png"), img)
            write_event(ProgressEvent(event="progress", stage="output",
                        percent=(i + 1) / len(logical), message=f"frame {i}"))

        hold_repeat = max(1, round(cmd.hold_ms / 1000.0 * cmd.fps))
        vw = cv2.VideoWriter(str(staging / "sequence.mp4"),
                             cv2.VideoWriter_fourcc(*"mp4v"), float(cmd.fps), (w, h), isColor=False)
        for img in logical:
            for _ in range(hold_repeat):
                vw.write(img)
        vw.release()

        meta = {
            "schema_version": 1,
            "screen_id": cmd.project.screen_id,
            "screen_resolution": [w, h],
            "dot_radius_px": cmd.dot_radius_px,
            "code": {"data_bits": db, "total_bits": total_bits, "parity": "even", "encoding": "binary"},
            "sequence": {"sentinel": "white_full", "anchor": "all_on",
                         "n_code_frames": total_bits, "hold_ms": cmd.hold_ms, "fps": cmd.fps},
            "cabinets": [{"col": s["col"], "row": s["row"],
                          "input_rect_px": [int(v) for v in s["input_rect_px"]],
                          "pixel_pitch_mm": [s["pixel_pitch_mm"][0], s["pixel_pitch_mm"][1]]}
                         for s in specs],
            "dots": [{"id": did, "u": float(u), "v": float(v),
                      "cabinet": list(dot_cabinet[did])} for (did, u, v) in dots],
        }
        (staging / "sl_meta.json").write_text(json.dumps(meta, indent=2))

        backup: pathlib.Path | None = None
        if out_dir.exists():
            backup = out_dir.with_suffix(out_dir.suffix + ATOMIC_BACKUP_SUFFIX)
            if backup.exists():
                shutil.rmtree(backup)
            out_dir.rename(backup)
        try:
            staging.rename(out_dir)
        except OSError:
            if backup is not None and not out_dir.exists():
                backup.rename(out_dir)
            raise
        if backup is not None:
            shutil.rmtree(backup, ignore_errors=True)
    except Exception:
        shutil.rmtree(staging, ignore_errors=True)
        raise

    write_event(ResultEvent(event="result", data=ResultData(
        measured_points=[], ba_stats=BaStats(rms_reprojection_px=0.0, iterations=0, converged=True),
        frame_strategy_used="nominal_anchoring", procrustes_align_rms_m=0.0)))
    return 0
```

Register both subcommands in `python-sidecar/src/lmt_vba_sidecar/__main__.py`:
- Add `GenerateStructuredLightInput`, `DecodeStructuredLightInput` to the `from lmt_vba_sidecar.ipc import (...)` block.
- Add `sub.add_parser("generate_structured_light")` and `sub.add_parser("decode_structured_light")`.
- `SUBCOMMAND_MODULES`: `"generate_structured_light": "lmt_vba_sidecar.structured_light"`, `"decode_structured_light": "lmt_vba_sidecar.sl_decode"`.
- `SUBCOMMAND_ENTRYPOINTS`: `"generate_structured_light": ("run_generate_structured_light", GenerateStructuredLightInput)`, `"decode_structured_light": ("run_decode_structured_light", DecodeStructuredLightInput)`.

- [ ] **Step 4: Run test to verify it passes**

Run: `cd python-sidecar && .venv/bin/python -m pytest tests/test_generate_structured_light.py -v`
Expected: PASS (3 passed)

- [ ] **Step 5: Commit**

```bash
git add python-sidecar/src/lmt_vba_sidecar/structured_light.py python-sidecar/src/lmt_vba_sidecar/__main__.py python-sidecar/tests/test_generate_structured_light.py
git commit -m "feat(sidecar): mapping-aware SL generation (anchor frame, absent cells)"
```

### Task 1.4: Rust contract — generate-structured-light (6-point sync)

**Files:** `crates/adapter-visual-ba/src/{ipc.rs,api.rs}`, `crates/lmt-shared/src/{dto.rs,schema.rs,manifest.rs}`, `crates/lmt-app/src/visual.rs`, `crates/lmt-cli/src/cli.rs`, `crates/lmt-cli/src/commands/visual.rs`, `crates/lmt-cli/tests/cli_e2e.rs`.

- [ ] **Step 1: Write the failing test (refuse + dry-run, no sidecar)**

```rust
// append to crates/lmt-cli/tests/cli_e2e.rs
#[test]
fn generate_structured_light_refuses_without_yes() {
    let tmp = TempDir::new().unwrap();
    let proj = tmp.path().join("proj");
    write_gp_project(&proj, 1, 1);
    let assert = lmt().args(["--json", "visual", "generate-structured-light",
        proj.to_str().unwrap(), "MAIN"]).assert().failure();
    let out = assert.get_output();
    assert_eq!(out.status.code(), Some(2));
    let env: Value = serde_json::from_str(std::str::from_utf8(&out.stderr).unwrap().trim_end()).unwrap();
    assert_eq!(env["error"]["code"], "invalid_input");
}

#[test]
fn generate_structured_light_dry_run_writes_nothing() {
    let tmp = TempDir::new().unwrap();
    let proj = tmp.path().join("proj");
    write_gp_project(&proj, 1, 1);
    let assert = lmt().args(["--json", "--dry-run", "visual", "generate-structured-light",
        proj.to_str().unwrap(), "MAIN"]).assert().success();
    let env: Value = serde_json::from_slice(&assert.get_output().stdout).unwrap();
    assert_eq!(env["ok"], true);
    assert_eq!(env["data"]["dry_run"], true);
    assert!(!proj.join("patterns/MAIN/sl").exists());
}
```

- [ ] **Step 2: Run to verify it fails**

Run: `cargo test -p lmt-cli generate_structured_light`
Expected: FAIL to compile (no `GenerateStructuredLight` variant).

- [ ] **Step 3: Implement the 6-point contract**

`crates/lmt-shared/src/dto.rs` (after `GeneratePatternResult`):
```rust
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct GenerateStructuredLightResult {
    pub output_dir: String,
    pub n_dots: usize,
    pub n_frames: usize,
}
```

`crates/lmt-shared/src/schema.rs` (in `dump_all`): `add!("GenerateStructuredLightResult", dto::GenerateStructuredLightResult);`

`crates/adapter-visual-ba/src/ipc.rs`:
```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SlDot { pub id: u32 }

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StructuredLightMeta {
    pub schema_version: u32,
    pub dots: Vec<SlDot>,
    #[serde(default)]
    pub sequence: serde_json::Value,
}
```

`crates/adapter-visual-ba/src/api.rs` (mirror `generate_pattern`; payload mirrors generate_pattern's `project`/`screen_resolution`/optional `screen_mapping_path`):
```rust
pub struct GenerateStructuredLightArgs {
    pub project_screen_id: String,
    pub cabinet_array: IpcCabinetArray,
    pub output_dir: String,
    pub screen_resolution: [u32; 2],
    pub screen_mapping_path: Option<String>,
    pub dot_spacing_px: u32,
    pub dot_radius_px: u32,
    pub progress_tx: Option<mpsc::Sender<Event>>,
    pub cancel: Option<oneshot::Receiver<()>>,
}

#[derive(Debug, Clone)]
pub struct GenerateStructuredLightOut {
    pub output_dir: String,
    pub n_dots: u32,
    pub n_frames: u32,
}

pub async fn generate_structured_light(
    args: GenerateStructuredLightArgs,
) -> VbaResult<GenerateStructuredLightOut> {
    let mut payload = json!({
        "command": "generate_structured_light", "version": 1,
        "project": { "screen_id": &args.project_screen_id,
                     "cabinet_array": &args.cabinet_array },
        "output_dir": &args.output_dir,
        "screen_resolution": args.screen_resolution,
        "dot_spacing_px": args.dot_spacing_px,
        "dot_radius_px": args.dot_radius_px,
    });
    if let Some(p) = &args.screen_mapping_path {
        payload["screen_mapping_path"] = json!(p);
    }
    let _ = run_sidecar(SidecarRequest {
        subcommand: "generate_structured_light".into(),
        payload, progress_tx: args.progress_tx, cancel: args.cancel,
    }).await?;

    let meta_path = Path::new(&args.output_dir).join("sl_meta.json");
    let meta: crate::ipc::StructuredLightMeta = serde_json::from_str(
        &std::fs::read_to_string(&meta_path)
            .map_err(|e| VbaError::InvalidInput(format!("sl_meta.json unreadable: {e}")))?,
    ).map_err(|e| VbaError::InvalidInput(format!("sl_meta.json decode failed: {e}")))?;
    let total_bits = meta.sequence.get("n_code_frames").and_then(|v| v.as_u64()).unwrap_or(0) as u32;
    Ok(GenerateStructuredLightOut {
        output_dir: args.output_dir,
        n_dots: meta.dots.len() as u32,
        // frames = WHITE + ALL-ON anchor + total_bits code frames + WHITE
        n_frames: total_bits + 3,
    })
}
```

`crates/lmt-app/src/visual.rs` (mirror `run_generate_pattern` — same project/screen load, same screen_resolution-from-mapping-or-uniform logic; only the args struct + output mapping differ). Add `GenerateStructuredLightArgs, GenerateStructuredLightOut, generate_structured_light` to the adapter `use` and `GenerateStructuredLightResult` to the dto `use`:
```rust
/// Generate a structured-light dot sequence for one screen into
/// `<project>/patterns/<screen_id>/sl`. Mapping-aware: with `screen_mapping_path`
/// the framebuffer is the input_rect_px bounding box (mirrors run_generate_pattern).
pub fn run_generate_structured_light(
    project_path: &Path,
    screen_id: &str,
    dot_spacing_px: u32,
    dot_radius_px: u32,
    screen_mapping_path: Option<&Path>,
) -> LmtResult<GenerateStructuredLightResult> {
    let cfg = load_project_yaml_from_path(project_path)?;
    let screen_cfg = load_screen(&cfg, screen_id)?;
    let cabinet_array = ipc_cabinet_array(screen_cfg);

    let sm_abs = screen_mapping_path.map(|p| {
        if p.is_absolute() { p.to_path_buf() } else { project_path.join(p) }
    });
    // Reuse the EXACT screen_resolution resolution block from run_generate_pattern
    // (mapping bounding box vs uniform pixels_per_cabinet * cabinet_count).
    let screen_resolution: [u32; 2] = compute_screen_resolution(&sm_abs, screen_cfg, screen_id)?;

    let output_dir = project_path.join("patterns").join(screen_id).join("sl");
    std::fs::create_dir_all(output_dir.parent().unwrap())?;

    let args = GenerateStructuredLightArgs {
        project_screen_id: screen_id.to_string(),
        cabinet_array,
        output_dir: output_dir.display().to_string(),
        screen_resolution,
        screen_mapping_path: sm_abs.map(|p| p.display().to_string()),
        dot_spacing_px, dot_radius_px,
        progress_tx: None, cancel: None,
    };
    let out = rt()?.block_on(generate_structured_light(args)).map_err(map_vba_err)?;
    Ok(GenerateStructuredLightResult {
        output_dir: out.output_dir,
        n_dots: out.n_dots as usize,
        n_frames: out.n_frames as usize,
    })
}
```
> NOTE: `run_generate_pattern` currently inlines the screen_resolution computation (mapping bbox vs uniform). Extract it into a private `fn compute_screen_resolution(sm_abs: &Option<PathBuf>, screen_cfg: &ScreenConfig, screen_id: &str) -> LmtResult<[u32;2]>` and call it from BOTH `run_generate_pattern` and `run_generate_structured_light` (DRY). This is a pure refactor; run `cargo test -p lmt-app` to confirm `run_generate_pattern` behavior is unchanged.

`crates/lmt-cli/src/cli.rs` — `VisualCmd` variant (note: adds `--screen-mapping`, matching generate-pattern):
```rust
    /// 生成结构光点阵序列(帧 PNG + sequence.mp4 + sl_meta.json)。side_effect: destructive
    #[command(name = "generate-structured-light")]
    GenerateStructuredLight {
        project_path: String,
        screen_id: String,
        #[arg(long, default_value_t = 64)]
        dot_spacing: u32,
        #[arg(long, default_value_t = 6)]
        dot_radius: u32,
        /// 可选 screen_mapping.json:按每箱体 input_rect_px 放点(支持非均匀/缺失箱体)。
        #[arg(long)]
        screen_mapping: Option<String>,
    },
```

`crates/lmt-cli/src/commands/visual.rs` — dispatch arm + handler:
```rust
        VisualCmd::GenerateStructuredLight { project_path, screen_id, dot_spacing, dot_radius, screen_mapping } =>
            generate_structured_light(mode, &project_path, &screen_id, dot_spacing, dot_radius,
                                      screen_mapping.as_deref(), yes, dry_run),
```
```rust
#[allow(clippy::too_many_arguments)]
fn generate_structured_light(mode: Mode, project_path: &str, screen_id: &str,
        dot_spacing: u32, dot_radius: u32, screen_mapping: Option<&str>,
        yes: bool, dry_run: bool) -> i32 {
    let decision = match util::gate_destructive(yes, dry_run, "visual generate-structured-light") {
        Ok(d) => d,
        Err(e) => return output::err(mode, e),
    };
    match decision {
        DestructiveDecision::DryRun => output::ok(mode,
            serde_json::json!({"dry_run": true, "would_write": format!("{project_path}/patterns/{screen_id}/sl/")}),
            |_| { let _ = writeln!(std::io::stdout(),
                "[dry-run] would generate structured-light sequence for screen {screen_id}"); }),
        DestructiveDecision::Execute => {
            match lmt_app::visual::run_generate_structured_light(
                Path::new(project_path), screen_id, dot_spacing, dot_radius,
                screen_mapping.map(Path::new)) {
                Ok(r) => output::ok(mode, r, |p| { let _ = writeln!(std::io::stdout(),
                    "generated {} dots across {} frames → {}", p.n_dots, p.n_frames, p.output_dir); }),
                Err(e) => output::err(mode, ApiError::from(e)),
            }
        }
    }
}
```

`crates/lmt-shared/src/manifest.rs` — `Operation` row: `"visual.generate_structured_light"`, cli `"lmt visual generate-structured-light <project> <screen_id> [--dot-spacing N] [--dot-radius N] [--screen-mapping <json>]"`, `SideEffect::Destructive`, result `Some("GenerateStructuredLightResult")`, exit-code set `&[0, 2, 3, 4, 6, 7]`.

- [ ] **Step 4: Run test to verify it passes**

Run: `cargo test -p lmt-cli generate_structured_light` then `cargo build`
Expected: PASS (2 tests); `./target/debug/lmt visual generate-structured-light --help` works.

- [ ] **Step 5: Verify schema + manifest + lmt-app refactor**

Run: `./target/debug/lmt --json schema | jq '.types.GenerateStructuredLightResult.title'` → `"GenerateStructuredLightResult"`
Run: `cargo test -p lmt-app` → green (confirms `compute_screen_resolution` refactor didn't change `run_generate_pattern`).

- [ ] **Step 6: Commit**

```bash
git add crates/
git commit -m "feat(cli): visual generate-structured-light (mapping-aware, 6-point contract)"
```

### Task 1.5: Docs + Tauri shim for generate-structured-light

**Files:** `docs/agents-cli.md`, `src-tauri/src/commands/visual.rs`, `src-tauri/src/lib.rs`.

- [ ] **Step 1: Add the command-table row** (after the `generate-pattern` row):

```markdown
| `lmt visual generate-structured-light <project> <screen_id> [--dot-spacing N] [--dot-radius N] [--screen-mapping <json>]` | destructive | Generate a structured-light dot-array capture sequence under `patterns/<screen_id>/sl/`: `frames/*.png` (white sentinel + all-on anchor + binary-blink-coded dot frames), `sequence.mp4` (drop-in full-screen playback), `sl_meta.json` (per-cabinet rects + dot screen coords + code/sequence spec, with `screen_id`). Mapping-aware: with `--screen-mapping` dots are tiled inside each cabinet's `input_rect_px`, honoring absent/non-uniform cabinets; without it, the uniform grid is used (even-divisibility required). Identity is carried in each dot's blink sequence (binary + even parity), not appearance — no dictionary-capacity limit. Result reports `n_dots` and `n_frames`. |
```

- [ ] **Step 2: Add the Tauri shim** (transport only) in `src-tauri/src/commands/visual.rs` and register in `src-tauri/src/lib.rs`'s `generate_handler!`:

```rust
#[tauri::command]
pub async fn generate_structured_light(
    project_path: String, screen_id: String,
    dot_spacing: u32, dot_radius: u32, screen_mapping: Option<String>,
) -> Result<lmt_shared::dto::GenerateStructuredLightResult, String> {
    lmt_app::visual::run_generate_structured_light(
        std::path::Path::new(&project_path), &screen_id, dot_spacing, dot_radius,
        screen_mapping.as_deref().map(std::path::Path::new),
    ).map_err(|e| e.to_string())
}
```

- [ ] **Step 3: Verify build**

Run: `cargo build`
Expected: workspace builds clean.

- [ ] **Step 4: Commit**

```bash
git add docs/agents-cli.md src-tauri/
git commit -m "docs+tauri: generate-structured-light exposure"
```

---

## PHASE 2 — Decode

**Deliverable:** `lmt visual decode-structured-light <input> <sl_meta> --out <corr.json> --yes` turns a recorded capture into a provenance-stamped correspondence file. The full generate→decode loop (incl. `id=0`) is verified on synthetic frames.

### Task 2.1: Frame loading + sentinel segmentation + plateau indexing (anchor-aware)

**Files:**
- Create: `python-sidecar/src/lmt_vba_sidecar/sl_decode.py`
- Test: `python-sidecar/tests/test_sl_decode.py`

- [ ] **Step 1: Write the failing test**

```python
# python-sidecar/tests/test_sl_decode.py
import numpy as np
import pytest
from lmt_vba_sidecar.sl_decode import load_frames, segment_code_region, index_plateaus

def _white(h=120, w=160): return np.full((h, w), 255, np.uint8)
def _g(v, h=120, w=160): return np.full((h, w), v, np.uint8)

def test_segment_excludes_sentinels():
    frames = [_white(), _g(10), _g(200), _g(10), _white()]
    assert segment_code_region(frames, sentinel_threshold=0.85) == (1, 4)

def test_index_plateaus_counts_anchor_plus_code():
    # anchor + 1 code frame, captured 3x each
    region = [_g(180), _g(180), _g(180), _g(40), _g(40), _g(40)]
    reps = index_plateaus(region, expected=2)   # expected = total_bits + 1
    assert len(reps) == 2

def test_index_plateaus_raises_on_mismatch():
    with pytest.raises(ValueError):
        index_plateaus([_g(10), _g(200)], expected=5)
```

- [ ] **Step 2: Run to verify it fails**

Run: `cd python-sidecar && .venv/bin/python -m pytest tests/test_sl_decode.py -v`
Expected: FAIL with `ModuleNotFoundError: No module named 'lmt_vba_sidecar.sl_decode'`

- [ ] **Step 3: Write minimal implementation**

```python
# python-sidecar/src/lmt_vba_sidecar/sl_decode.py
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
    mb = np.array([float(f.mean()) for f in frames])
    idx = np.where(mb > sentinel_threshold * 255.0)[0]
    if idx.size < 2:
        raise ValueError("could not find two white sentinel frames")
    return int(idx[0]) + 1, int(idx[-1])


def index_plateaus(region: list[np.ndarray], *, expected: int) -> list[int]:
    """Split into `expected` plateaus by global frame diff; return middle index of
    each. Raises if the count != expected. `expected` == total_bits + 1 (anchor)."""
    if not region:
        raise ValueError("empty code region")
    diffs = np.array([0.0] + [float(np.abs(region[i].astype(np.int16)
                      - region[i - 1].astype(np.int16)).mean()) for i in range(1, len(region))])
    thr = max(2.0, diffs.max() * 0.5)
    bounds = [0] + [i for i in range(1, len(region)) if diffs[i] > thr] + [len(region)]
    segs = [(bounds[k], bounds[k + 1]) for k in range(len(bounds) - 1) if bounds[k + 1] > bounds[k]]
    if len(segs) != expected:
        raise ValueError(f"expected {expected} plateaus (anchor + code), found {len(segs)}")
    return [(a + b) // 2 for (a, b) in segs]
```

- [ ] **Step 4: Run to verify it passes**

Run: `cd python-sidecar && .venv/bin/python -m pytest tests/test_sl_decode.py -v`
Expected: PASS (3 passed)

- [ ] **Step 5: Commit**

```bash
git add python-sidecar/src/lmt_vba_sidecar/sl_decode.py python-sidecar/tests/test_sl_decode.py
git commit -m "feat(sidecar): SL segmentation + anchor-aware plateau indexing"
```

### Task 2.2: Anchor-seeded centroid detection + decode + provenance writer

**Files:**
- Modify: `python-sidecar/src/lmt_vba_sidecar/sl_decode.py`
- Test: `python-sidecar/tests/test_sl_decode.py`

- [ ] **Step 1: Write the failing test (generate→decode roundtrip incl. id=0)**

```python
# append to python-sidecar/tests/test_sl_decode.py
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

def test_correspondence_has_provenance(tmp_path):
    sl = _gen(tmp_path)
    dec = DecodeStructuredLightInput.model_validate({
        "command": "decode_structured_light", "version": 1,
        "input_path": str(sl / "frames"), "sl_meta_path": str(sl / "sl_meta.json"),
        "output_path": str(tmp_path / "corr.json")})
    run_decode_structured_light(dec)
    corr = json.loads((tmp_path / "corr.json").read_text())
    expect_hash = __import__("hashlib").sha256((sl / "sl_meta.json").read_bytes()).hexdigest()
    assert corr["screen_id"] == "MAIN"
    assert corr["sl_meta_sha256"] == expect_hash
    assert corr["camera_image_size"] == [960, 540]
```

- [ ] **Step 2: Run to verify it fails**

Run: `cd python-sidecar && .venv/bin/python -m pytest tests/test_sl_decode.py -k roundtrip -v`
Expected: FAIL with `ImportError: cannot import name 'run_decode_structured_light'`

- [ ] **Step 3: Write minimal implementation**

```python
# append to python-sidecar/src/lmt_vba_sidecar/sl_decode.py

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
```

- [ ] **Step 4: Run to verify it passes**

Run: `cd python-sidecar && .venv/bin/python -m pytest tests/test_sl_decode.py -v`
Expected: PASS (5 passed)

- [ ] **Step 5: Add dispatch test + run full sidecar suite**

Append to `python-sidecar/tests/test_main_dispatch.py`:
```python
def test_dispatch_knows_structured_light_subcommands():
    import subprocess, sys, json
    p = subprocess.run([sys.executable, "-m", "lmt_vba_sidecar", "decode_structured_light"],
                       input="{}", capture_output=True, text=True)
    assert p.returncode == 1
    ev = json.loads(p.stdout.strip().splitlines()[-1])
    assert ev["event"] == "error" and ev["code"] == "invalid_input"
```
Run: `cd python-sidecar && .venv/bin/python -m pytest -q`
Expected: all pass.

- [ ] **Step 6: Commit**

```bash
git add python-sidecar/src/lmt_vba_sidecar/sl_decode.py python-sidecar/tests/
git commit -m "feat(sidecar): anchor-seeded SL decode + provenance (id=0 recovered)"
```

### Task 2.3: Rust contract — decode-structured-light (6-point sync)

**Files:** same set as Task 1.4 (for decode). Concrete differences:

- [ ] **Step 1: Write the failing test** — append to `crates/lmt-cli/tests/cli_e2e.rs`:
```rust
#[test]
fn decode_structured_light_refuses_without_yes() {
    let tmp = TempDir::new().unwrap();
    let meta = tmp.path().join("sl_meta.json");
    std::fs::write(&meta, "{}").unwrap();
    let assert = lmt().args(["--json", "visual", "decode-structured-light",
        tmp.path().to_str().unwrap(), meta.to_str().unwrap(),
        "--out", tmp.path().join("c.json").to_str().unwrap()]).assert().failure();
    let out = assert.get_output();
    assert_eq!(out.status.code(), Some(2));
    let env: Value = serde_json::from_str(std::str::from_utf8(&out.stderr).unwrap().trim_end()).unwrap();
    assert_eq!(env["error"]["code"], "invalid_input");
}
```

- [ ] **Step 2: Run to verify it fails** — `cargo test -p lmt-cli decode_structured_light` → no variant.

- [ ] **Step 3: Implement**

`dto.rs`:
```rust
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct DecodeStructuredLightResult {
    pub output_path: String,
    pub n_dots_decoded: usize,
}
```
`schema.rs`: `add!("DecodeStructuredLightResult", dto::DecodeStructuredLightResult);`

`adapter ipc.rs` (provenance mirror — enough for count + Phase 3 validation):
```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CorrespondencePoint { pub id: u32, pub u: f64, pub v: f64, pub x: f64, pub y: f64 }

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CorrespondenceFile {
    pub schema_version: u32,
    pub screen_id: String,
    pub sl_meta_sha256: String,
    pub points: Vec<CorrespondencePoint>,
}
```

`api.rs`:
```rust
pub struct DecodeStructuredLightArgs {
    pub input_path: String,
    pub sl_meta_path: String,
    pub output_path: String,
    pub progress_tx: Option<mpsc::Sender<Event>>,
    pub cancel: Option<oneshot::Receiver<()>>,
}
#[derive(Debug, Clone)]
pub struct DecodeStructuredLightOut { pub output_path: String, pub n_dots_decoded: u32 }

pub async fn decode_structured_light(args: DecodeStructuredLightArgs) -> VbaResult<DecodeStructuredLightOut> {
    let payload = json!({
        "command": "decode_structured_light", "version": 1,
        "input_path": &args.input_path, "sl_meta_path": &args.sl_meta_path,
        "output_path": &args.output_path,
    });
    let _ = run_sidecar(SidecarRequest {
        subcommand: "decode_structured_light".into(),
        payload, progress_tx: args.progress_tx, cancel: args.cancel,
    }).await?;
    let corr: crate::ipc::CorrespondenceFile = serde_json::from_str(
        &std::fs::read_to_string(&args.output_path)
            .map_err(|e| VbaError::InvalidInput(format!("correspondence unreadable: {e}")))?,
    ).map_err(|e| VbaError::InvalidInput(format!("correspondence decode failed: {e}")))?;
    Ok(DecodeStructuredLightOut { output_path: args.output_path, n_dots_decoded: corr.points.len() as u32 })
}
```

`lmt-app/src/visual.rs`:
```rust
pub fn run_decode_structured_light(input_path: &Path, sl_meta_path: &Path, output_path: &Path)
    -> LmtResult<DecodeStructuredLightResult> {
    let args = DecodeStructuredLightArgs {
        input_path: input_path.display().to_string(),
        sl_meta_path: sl_meta_path.display().to_string(),
        output_path: output_path.display().to_string(),
        progress_tx: None, cancel: None,
    };
    let out = rt()?.block_on(decode_structured_light(args)).map_err(map_vba_err)?;
    Ok(DecodeStructuredLightResult { output_path: out.output_path, n_dots_decoded: out.n_dots_decoded as usize })
}
```

`cli.rs` `VisualCmd`:
```rust
    /// 解码结构光录像 → 屏幕↔相机对应文件(带 provenance)。side_effect: destructive
    #[command(name = "decode-structured-light")]
    DecodeStructuredLight {
        input_path: String,
        sl_meta: String,
        #[arg(long)]
        out: String,
    },
```

`commands/visual.rs` dispatch arm + handler:
```rust
        VisualCmd::DecodeStructuredLight { input_path, sl_meta, out } =>
            decode_structured_light(mode, &input_path, &sl_meta, &out, yes, dry_run),
```
```rust
fn decode_structured_light(mode: Mode, input_path: &str, sl_meta: &str, out: &str,
                           yes: bool, dry_run: bool) -> i32 {
    let decision = match util::gate_destructive(yes, dry_run, "visual decode-structured-light") {
        Ok(d) => d,
        Err(e) => return output::err(mode, e),
    };
    match decision {
        DestructiveDecision::DryRun => output::ok(mode,
            serde_json::json!({"dry_run": true, "would_write": out}),
            |_| { let _ = writeln!(std::io::stdout(), "[dry-run] would decode → {out}"); }),
        DestructiveDecision::Execute => match lmt_app::visual::run_decode_structured_light(
                Path::new(input_path), Path::new(sl_meta), Path::new(out)) {
            Ok(r) => output::ok(mode, r, |p| { let _ = writeln!(std::io::stdout(),
                "decoded {} dots → {}", p.n_dots_decoded, p.output_path); }),
            Err(e) => output::err(mode, ApiError::from(e)),
        },
    }
}
```

`manifest.rs` — row `"visual.decode_structured_light"`, `SideEffect::Destructive`, result `Some("DecodeStructuredLightResult")`, exit-code set `&[0, 2, 3, 4, 13, 18]` (13 detection_failed, 18 decode_failed).

- [ ] **Step 4: Run to verify it passes** — `cargo test -p lmt-cli decode_structured_light` then `cargo test --workspace` green.

- [ ] **Step 5: Docs + Tauri shim**

`docs/agents-cli.md` row:
```markdown
| `lmt visual decode-structured-light <input> <sl_meta> --out <corr.json>` | destructive | Decode a recorded structured-light capture (video or frame directory) into a provenance-stamped screen↔camera correspondence file (`screen_id`, `sl_meta_sha256`, `camera_image_size`, `source_input`, points). Segments by white sentinels, indexes plateaus, seeds dots from the all-on anchor (so `id=0` is recovered), decodes each dot's binary+parity blink code. `decode_failed` (18) if sentinels/plateaus don't parse; `detection_failed` (13) if too few dots decode. |
```
Clarify the `decode_failed` error-code row to: `Structured-light segmentation/plateau decode failed, or image decode/unsupported format`.
Add the Tauri shim `run_decode_structured_light` and register in `generate_handler!`.

- [ ] **Step 6: Commit**

```bash
git add crates/ docs/ src-tauri/
git commit -m "feat(cli): visual decode-structured-light (provenance, 6-point contract)"
```

---

## PHASE 3 — Reconstruct (multi-view BA)  *[design; own plan after Phase 0 gate + Phase 2 land]*

**Why deferred:** the solver must be written against (a) Phase 0's measured precision target, (b) real Phase-2 `CorrespondenceFile` output, and (c) the existing `python-sidecar/.../ba.py` `bundle_adjust` (scipy `least_squares` + `jac_sparsity`). Writing line-by-line solver code now would be speculative. This section locks architecture, interfaces, reuse, provenance gates, and the gating test so the follow-on plan is mechanical.

**Data flow:** N pose `CorrespondenceFile`s → **provenance validation** → assemble observations `(view_idx, dot_id) -> camera (x,y)` → camera pose init (PnP vs nominal) → multi-view BA → per-cabinet poses + surface → `measured.yaml` + `cabinet_pose_report.json` (reuse `reconstruct.py` writer).

**Provenance gate (mandatory first step, from Codex finding 4):** before any geometry, assert every input `CorrespondenceFile` shares one `screen_id` and one `sl_meta_sha256`, that `sl_meta_sha256` matches the `sl_meta.json` being used, and that `screen_id` matches the project/screen. Any mismatch → `invalid_input` (stale/mixed capture). This mirrors the existing `pattern_hash` preflight in `reconstruct.py`.

**Screen-coordinate invariant:** a dot `id` has a fixed screen `(u,v)` in every pose file, so the same `id` across poses is the same 3D point — free cross-pose stitching and overlapping-tile fusion for large walls.

**Reuse map:**
- Pose init / gate sensitivity: `sl_feasibility.solve_pnp_pose` + `triangulate_multiview` (Phase 0).
- BA solver: `ba.py::bundle_adjust` (`scipy.optimize.least_squares`, `_residuals`, `_build_sparsity`, RMS) — residual = reproject screen point through camera pose; unknowns = camera poses (R,t) + per-cabinet rigid transform (or per-dot 3D constrained to a per-cabinet plane).
- Metric scale: per-cabinet `pixel_pitch_mm` from `sl_meta.cabinets[]` → exact mm between dots on one cabinet pins the gauge; reuse `reconstruct.py` `fix_root_cabinet`/`align_to_nominal` `FrameSpec`.
- Cabinet assignment: `sl_meta.dots[].cabinet` already tags each id to a `(col,row)` — no re-derivation needed.
- Output: reuse `reconstruct.py` `measured.yaml` + `cabinet_pose_report.json` writer.

**Contract surface (6-point, follow-on plan):** sidecar `reconstruct_structured_light` (input: list of correspondence paths + project + intrinsics + frame spec), reusing `VisualReconstructResult` + `CabinetPoseReportFile`; CLI `visual reconstruct --structured-light --correspondence <c.json> ...`. Error codes already exist: `ba_diverged` (14), `observability_failed` (17).

**Named tasks (to expand):**
1. Provenance validation + IPC `ReconstructStructuredLightInput` + observation assembler from N `CorrespondenceFile`s.
2. Camera pose init via PnP vs nominal; reject under-observed views (`observability_failed`).
3. BA assembly reusing `ba.py` (residual + sparsity for cross-view screen-point reprojection).
4. Metric gauge + frame fixing + surface fit (reuse `reconstruct.py` shape_prior).
5. Output writer + Rust 6-point contract.

**GATING TEST (write first in the follow-on plan):**
```python
# python-sidecar/tests/test_reconstruct_structured_light.py  (Phase 3)
import numpy as np
from lmt_vba_sidecar.sl_feasibility import build_screen, camera_ring, project_point

def test_synthetic_sl_reconstruction_hits_mm_target():
    """Synthetic perfect correspondences + pixel noise: recovered 3D RMS must
    beat the Phase-0 feasibility target for this rig."""
    K = np.array([[3000.,0,1920.],[0,3000.,1080.],[0,0,1]])
    truth = build_screen(3000., 1800., 8, 5, curve_mm=30.)
    poses = camera_ring(4000., 5, 50.)
    rng = np.random.default_rng(0)
    corr_per_pose = []
    for (R, t) in poses:
        corr_per_pose.append([{"id": i, "x": float(p[0]), "y": float(p[1])}
            for i, X in enumerate(truth)
            for p in [project_point(K, R, t, X) + rng.normal(0, 0.1, 2)]])
    # from lmt_vba_sidecar.reconstruct_sl import reconstruct_points  # implement
    # recovered = reconstruct_points(K, corr_per_pose, nominal=truth, gauge="align_to_nominal")
    # rms = float(np.sqrt(((recovered - truth)**2).sum(axis=1).mean()))
    # assert rms < <Phase-0 number for this rig>   # mm
    assert True  # placeholder until reconstruct_sl exists (Phase 3 plan)
```

---

## Self-Review

**1. Spec coverage:**
- single camera moved to multiple poses → Phase 0 PnP-pose model; Phase 3 BA over N poses. ✓
- multi-frame capture, tool emits video, existing chain plays it → Phase 1 `sequence.mp4` + `frames/`. ✓
- frame sync via full-screen white sentinel + plateau indexing, no corner block → Phase 2 `segment_code_region` + `index_plateaus`. ✓
- white dots, identity in time (binary blink), gamma-immune, mm → Phase 1 binary+parity (black/white only); Phase 0 gates mm with real pose error; Phase 3 hits mm. ✓
- no dictionary capacity wall → temporal ids, `data_bits_for` scales. ✓
- mapping/irregular screens (Codex 2) → generation reuses `_resolve_cabinet_specs`, honors absent cells, rejects irregular-without-mapping. ✓
- id=0 observable (Codex 3) → all-on anchor seeds locations; explicit `id=0` regression. ✓
- provenance (Codex 4) → `sl_meta.screen_id`, `CorrespondenceFile.{screen_id,sl_meta_sha256,...}`, Phase 3 validation gate. ✓
- honest gate (Codex 1) → PnP-based `feasibility_rms_mm` + perturbation knobs; `test_estimated_pose_is_worse_than_oracle`. ✓
- CLAUDE.md 6-point contract → Tasks 1.4/1.5 + 2.3. ✓

**2. Placeholder scan:** The only intentional placeholder is the Phase 3 gating test's `assert True`, flagged as filled by the follow-on plan (Phase 3 is design-granularity by the scope decision). Two NOTEs (`compute_screen_resolution` refactor; ScreenConfig shape) are "extract/verify against existing `run_generate_pattern`" with exact references, not vague TODOs.

**3. Type consistency:**
- `StructuredLightMeta` (`code.{data_bits,total_bits}`, `sequence.{anchor,n_code_frames}`, `cabinets[].{col,row,input_rect_px,pixel_pitch_mm}`, `dots[].{id,u,v,cabinet}`) identical across `ipc.py` (1.2), writer (1.3), reader (2.2). ✓
- Sequence math consistent everywhere: frames = total_bits + 3 (1.3 test, api.rs n_frames); plateaus = total_bits + 1 (2.1/2.2). ✓
- `encode_id`/`decode_bits`/`data_bits_for`/`build_dots_in_rect` signatures identical across codec (1.1), generator (1.3), decoder (2.2). ✓
- `CorrespondenceFile` provenance fields identical: ipc.py (1.2), decode writer (2.2), test (2.2), adapter mirror (2.3). ✓
- DTOs `GenerateStructuredLightResult{output_dir,n_dots,n_frames}` / `DecodeStructuredLightResult{output_path,n_dots_decoded}` identical in dto.rs, api.rs Out, lmt-app, handlers. ✓
- subcommand strings `"generate_structured_light"`/`"decode_structured_light"` identical in `__main__.py` and `api.rs`. ✓

---

## Execution Handoff

Two execution options:
1. **Subagent-Driven (recommended)** — fresh subagent per task, review between tasks.
2. **Inline Execution** — execute in this session with checkpoints.

**Strong recommendation:** execute **Phase 0 first as a hard gate** (now an honest PnP-based gate). Only proceed to Phase 1+ if the sweep confirms mm is reachable with the real rig.
