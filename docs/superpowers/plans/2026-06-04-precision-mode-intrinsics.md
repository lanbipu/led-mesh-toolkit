# Precision Mode — Intrinsics Core (L1) Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Make `reconstruct-structured-light` deliver tighter rigid-cabinet poses by (a) self-calibrating intrinsics from the SAME capture (`--intrinsics auto`), (b) adaptively solving a richer distortion model, and (c) an anti-absorption cross-check that surfaces screen pitch/1:1 errors instead of hiding them inside K.

**Architecture:** Zero change to the geometry model / `model_constrained_ba` / `solve_and_emit` / `align_to_nominal` gauge. The accuracy gain is fed in via a better, frame-matched camera model. The self-cal engine already exists (`calibrate_sl.run_calibrate_structured_light`); this plan extracts its K-solve into a reusable helper, adds adaptive distortion + a cross-check, and calls it inline from `sl_reconstruct` when `--intrinsics auto`. The cross-check is the **no-ship red line** (spec P6): a self-cal solved against the screen-as-target can absorb anisotropic/warp pitch errors into K, so we refuse or warn unless an independent anchor (or non-coplanar geometry) breaks the degeneracy.

**Tech Stack:** Python sidecar (`lmt_vba_sidecar`, OpenCV `calibrateCameraExtended`, Pydantic IPC, pytest) + Rust (`lmt-cli` clap, `lmt-app` service layer, `adapter-visual-ba` IPC, `lmt-shared` DTO/schemars, `cli_e2e.rs` assert_cmd).

**Source spec:** `docs/superpowers/specs/2026-06-04-precision-mode-design.md` (L1 = §A.1.1/A.1.2/A.1.3; guard = P6/§A.4; contract = §4/§7).

**Branch:** `feat/precision-mode` (already checked out).

**Scope note:** This is Plan 1 of 3. L2 (subpixel, spec item 4) and L3+compare-known thresholds (items 5/6) are separate small plans — outlined at the end. Phase 2 (L5 in-BA distortion refine) is bench-gated and gets its own plan after P6 passes.

---

## File Structure

| File | Responsibility | Change |
|---|---|---|
| `python-sidecar/src/lmt_vba_sidecar/calibrate_sl.py` | SL self-cal solver | Extract `solve_sl_intrinsics()` helper; adaptive distortion; cross-check call |
| `python-sidecar/src/lmt_vba_sidecar/intrinsics_solve.py` (NEW) | Pure reusable K-solver + adaptive distortion + cross-check (no IPC, no file IO) | Created |
| `python-sidecar/src/lmt_vba_sidecar/sl_reconstruct.py` | SL reconstruct | `intrinsics_path == "auto"` branch → inline self-cal |
| `python-sidecar/src/lmt_vba_sidecar/ipc.py` | IPC models | Add `crosscheck_intrinsics_path` to two inputs; new free-form warning codes (no enum change) |
| `python-sidecar/tests/test_intrinsics_solve.py` (NEW) | Solver/cross-check unit tests | Created |
| `python-sidecar/tests/test_sl_reconstruct.py` | reconstruct tests | Add auto-path + pitch-absorption-guard tests |
| `python-sidecar/tests/test_calibrate_sl.py` | calibrate tests | Add adaptive-distortion + cross-check refusal tests |
| `crates/lmt-cli/src/cli.rs` | clap structs | `--intrinsics-crosscheck` on Reconstruct + Calibrate (`--intrinsics auto` needs no schema change) |
| `crates/lmt-cli/src/commands/visual.rs` | transport | Thread sentinel + crosscheck; `auto` must not become a path |
| `crates/lmt-app/src/visual.rs` | service layer | Thread params; populate new DTO fields |
| `crates/adapter-visual-ba/src/api.rs` | IPC payload | New args fields + payload keys + read-back |
| `crates/lmt-shared/src/dto.rs` | DTOs | `intrinsics_source` on VisualReconstructResult; `distortion_model`/`focal_stddev_px`/`pp_stddev_px` on CalibrateResult |
| `crates/lmt-shared/src/schema.rs` | schema dump test | New presence test (no new `add!` for existing types) |
| `crates/lmt-cli/tests/cli_e2e.rs` | E2E | auto refuse/dry-run + flat-wall-no-anchor refuse + gated real-sidecar happy |
| `docs/agents-cli.md` | contract doc | Update reconstruct + calibrate rows |

**Crosscheck algorithm (the novel logic, defined once here):** Given the self-cal result `(K, dist, std_int, coplanar_ratio)` and an optional independent anchor `anchor_K`:
1. **Anchor present** → `focal_dev = |fx − anchor_fx| / anchor_fx`; `aspect_dev = |fx/fy − anchor_fx/anchor_fy|`. If `focal_dev > FOCAL_CROSSCHECK_MAX_FRAC` (0.02) OR `aspect_dev > ASPECT_CROSSCHECK_MAX` (0.01) → **refuse** `observability_failed` (msg names "auto intrinsics deviate from anchor — suspected screen pitch/1:1 absorbed into K"). This catches the absorbable classes (anisotropic sx≠sy → aspect; scale/warp → focal).
2. **No anchor + coplanar target** (`coplanar_ratio < COPLANAR_RATIO_MIN`) → **refuse** `observability_failed` (msg: "flat wall + no independent anchor; cannot separate screen pitch/1:1 from intrinsics — pass an independent intrinsics anchor"). A coplanar target cannot self-detect anisotropic absorption.
3. **No anchor + non-coplanar target** → rely on existing focal/pp covariance gates; emit `WarningEvent(code="no_intrinsics_anchor", ...)` so the operator knows the absorbable classes are unguarded.

---

## Part A — Python: reusable intrinsics solver + adaptive distortion

### Task 1: Extract `solve_sl_intrinsics()` pure helper (refactor, behavior-preserving)

**Files:**
- Create: `python-sidecar/src/lmt_vba_sidecar/intrinsics_solve.py`
- Modify: `python-sidecar/src/lmt_vba_sidecar/calibrate_sl.py:176-212`
- Test: `python-sidecar/tests/test_intrinsics_solve.py` (create)

- [ ] **Step 1: Write the failing test** (pin the helper's contract against a known camera)

```python
# python-sidecar/tests/test_intrinsics_solve.py
import numpy as np
from lmt_vba_sidecar.intrinsics_solve import solve_sl_intrinsics, IntrinsicsRefused
from lmt_vba_sidecar.nominal import nominal_dot_positions_world
from lmt_vba_sidecar.sl_feasibility import look_at_pose, project_point

K_TRUE = np.array([[3000.0, 0.0, 2000.0], [0.0, 3000.0, 1500.0], [0.0, 0.0, 1.0]])
IMG = (4000, 3000)


def _well_object_image_points(noise=0.0, seed=0):
    """6 oblique multi-distance poses of a 3x3 curved wall (the gate-passing envelope).
    Returns (object_points, image_points) lists of float32 arrays, one per pose."""
    from test_calibrate_sl import _well_meta, _wall_center, _well_poses, _grid_meta  # reuse builders
    meta, proj, cab, shape = _well_meta()
    world = nominal_dot_positions_world(meta, cab, shape)
    poses = _well_poses(_wall_center(meta, cab, shape))
    rng = np.random.default_rng(seed)
    obj, img = [], []
    ids = sorted(world.keys())
    for (R, t) in poses:
        o = np.array([world[i] for i in ids], dtype=np.float32)
        p = np.array([project_point(K_TRUE, R, t, world[i]) + rng.normal(0, noise, 2)
                      for i in ids], dtype=np.float32)
        obj.append(o)
        img.append(p)
    return obj, img


def test_solver_recovers_focal_noise_free():
    obj, img = _well_object_image_points(noise=0.0)
    res = solve_sl_intrinsics(obj, img, IMG, max_rms_px=1.5)
    assert abs(res.K[0, 0] - 3000.0) / 3000.0 < 0.01
    assert abs(res.K[0, 2] - 2000.0) < 1.5
    assert res.distortion_model in ("radial2", "full")
    assert res.focal_stddev_px[0] >= 0.0 and res.pp_stddev_px[0] >= 0.0
```

- [ ] **Step 2: Run it to verify it fails**

Run: `cd python-sidecar && .venv/bin/python -m pytest tests/test_intrinsics_solve.py -v`
Expected: FAIL with `ModuleNotFoundError: No module named 'lmt_vba_sidecar.intrinsics_solve'`

- [ ] **Step 3: Write the helper** (lift the solve block from `calibrate_sl.py:176-212` verbatim into a pure function; no `write_event`/file IO — gate failures raise `IntrinsicsRefused(code, msg)`)

```python
# python-sidecar/src/lmt_vba_sidecar/intrinsics_solve.py
"""Pure SL intrinsics solver (no IPC, no file IO) shared by calibrate-structured-light
and reconstruct-structured-light's --intrinsics auto. Gate failures raise
IntrinsicsRefused(code, msg); callers translate to an ErrorEvent or re-raise."""
from __future__ import annotations

from dataclasses import dataclass

import cv2
import numpy as np

from lmt_vba_sidecar.calibrate import FOCAL_BOUNDS_FRACTION

# Gate constants (mirror calibrate_sl.py:42-60 so behavior is unchanged after extraction).
COVERAGE_MIN_FRAC = 0.20
COPLANAR_RATIO_MIN = 1e-3
POSE_ROT_DIVERSITY_DEG = 5.0
PP_STDDEV_MAX_PX = 3.0
FOCAL_STDDEV_MAX_FRAC = 0.005
MIN_DOTS_PER_POSE = 4


class IntrinsicsRefused(Exception):
    def __init__(self, code: str, message: str):
        super().__init__(message)
        self.code = code
        self.message = message


@dataclass
class IntrinsicsResult:
    K: np.ndarray
    dist: np.ndarray
    rms: float
    focal_stddev_px: tuple[float, float]
    pp_stddev_px: tuple[float, float]
    distortion_model: str          # "radial2" | "full"
    coplanar_ratio: float
    rvecs: list


def _coplanarity_ratio(pts: np.ndarray) -> float:
    if len(pts) < 3:
        return 0.0
    s = np.linalg.svd(pts - pts.mean(axis=0), compute_uv=False)
    return float(s[-1] / s[0]) if s[0] > 0 else 0.0


def _max_pairwise_rot_deg(rvecs) -> float:
    Rs = [cv2.Rodrigues(np.asarray(r))[0] for r in rvecs]
    best = 0.0
    for a in range(len(Rs)):
        for b in range(a + 1, len(Rs)):
            Rrel = Rs[a].T @ Rs[b]
            cos = (np.trace(Rrel) - 1.0) / 2.0
            best = max(best, float(np.degrees(np.arccos(np.clip(cos, -1.0, 1.0)))))
    return best


def _coverage_frac(image_points, image_size) -> float:
    allpts = np.concatenate([np.asarray(p).reshape(-1, 2) for p in image_points], axis=0)
    w = (allpts[:, 0].max() - allpts[:, 0].min()) / image_size[0]
    h = (allpts[:, 1].max() - allpts[:, 1].min()) / image_size[1]
    return float(min(w, h))


def solve_sl_intrinsics(object_points, image_points, image_size, *, max_rms_px: float) -> IntrinsicsResult:
    """Solve K + distortion from per-pose (object_points, image_points). Raises
    IntrinsicsRefused on any gate. Distortion model is fixed k1,k2 here (Task 1
    is a behavior-preserving extraction); Task 2 makes it adaptive."""
    if len(object_points) < 1:
        raise IntrinsicsRefused("observability_failed", f"no pose has >= {MIN_DOTS_PER_POSE} dots")
    all_obj = np.concatenate(object_points, axis=0)
    ratio = _coplanarity_ratio(all_obj)
    if ratio < COPLANAR_RATIO_MIN and len(object_points) < 3:
        raise IntrinsicsRefused("observability_failed",
                                f"near-coplanar target (ratio={ratio:.2e}) with only {len(object_points)} pose(s)")
    cover = _coverage_frac(image_points, image_size)
    if cover < COVERAGE_MIN_FRAC:
        raise IntrinsicsRefused("observability_failed", f"image coverage {cover:.2f} < {COVERAGE_MIN_FRAC}")

    long_dim = max(image_size)
    K0 = np.array([[1.2 * long_dim, 0.0, image_size[0] / 2.0],
                   [0.0, 1.2 * long_dim, image_size[1] / 2.0],
                   [0.0, 0.0, 1.0]])
    dist0 = np.zeros(5)
    flags = cv2.CALIB_USE_INTRINSIC_GUESS | cv2.CALIB_ZERO_TANGENT_DIST | cv2.CALIB_FIX_K3
    try:
        rms, K, dist, rvecs, _tvecs, std_int, _std_ext, _pv = cv2.calibrateCameraExtended(
            object_points, image_points, image_size, K0, dist0, flags=flags)
    except cv2.error as e:
        raise IntrinsicsRefused("intrinsics_invalid", f"calibrateCamera failed: {e}")

    if len(rvecs) >= 2 and _max_pairwise_rot_deg(rvecs) < POSE_ROT_DIVERSITY_DEG:
        raise IntrinsicsRefused("observability_failed",
                                f"pose rotation diversity < {POSE_ROT_DIVERSITY_DEG} deg (near-duplicate captures)")
    if not (np.isfinite(K).all() and np.isfinite(dist).all() and np.isfinite(rms)):
        raise IntrinsicsRefused("intrinsics_invalid", f"calibration produced non-finite values (rms={rms})")
    fx, fy, cx, cy = float(K[0, 0]), float(K[1, 1]), float(K[0, 2]), float(K[1, 2])
    f_lo, f_hi = FOCAL_BOUNDS_FRACTION
    if not (f_lo * long_dim < fx < f_hi * long_dim) or not (f_lo * long_dim < fy < f_hi * long_dim):
        raise IntrinsicsRefused("intrinsics_invalid", f"focal ({fx:.1f},{fy:.1f}) outside plausible range")
    if not (0 < cx < image_size[0]) or not (0 < cy < image_size[1]):
        raise IntrinsicsRefused("intrinsics_invalid", f"principal point ({cx:.1f},{cy:.1f}) outside image")
    if rms > max_rms_px:
        raise IntrinsicsRefused("intrinsics_invalid", f"reproj RMS {rms:.2f}px exceeds gate {max_rms_px}px")
    std = np.asarray(std_int).flatten()
    pp_std = (float(std[2]), float(std[3]))
    foc_std = (float(std[0]), float(std[1]))
    if max(pp_std) > PP_STDDEV_MAX_PX:
        raise IntrinsicsRefused("observability_failed", f"principal-point std {pp_std} px > {PP_STDDEV_MAX_PX}")
    if max(foc_std) > FOCAL_STDDEV_MAX_FRAC * fx:
        raise IntrinsicsRefused("observability_failed", f"focal std {foc_std} px > {FOCAL_STDDEV_MAX_FRAC*100:.1f}%")

    return IntrinsicsResult(K=K, dist=dist, rms=float(rms), focal_stddev_px=foc_std,
                            pp_stddev_px=pp_std, distortion_model="radial2",
                            coplanar_ratio=ratio, rvecs=list(rvecs))
```

- [ ] **Step 4: Run the test to verify it passes**

Run: `cd python-sidecar && .venv/bin/python -m pytest tests/test_intrinsics_solve.py -v`
Expected: PASS

- [ ] **Step 5: Rewire `calibrate_sl.py` to call the helper (behavior-preserving)** — replace the solve+gate block at `calibrate_sl.py:176-212` with a call to `solve_sl_intrinsics(...)`, translating `IntrinsicsRefused` → `_err(e.code, e.message)`. Keep the constants in `calibrate_sl.py` re-exported from `intrinsics_solve` (`from lmt_vba_sidecar.intrinsics_solve import solve_sl_intrinsics, IntrinsicsRefused, COVERAGE_MIN_FRAC, ...`) so existing imports don't break. The output-write block (`calibrate_sl.py:214-238`) reads `res.K`, `res.dist`, `res.rms`, `res.pp_stddev_px`, `res.focal_stddev_px`, `res.distortion_model`.

```python
# calibrate_sl.py — replace lines 176-212 with:
    try:
        res = solve_sl_intrinsics(object_points, image_points, image_size, max_rms_px=cmd.max_rms_px)
    except IntrinsicsRefused as e:
        return _err(e.code, e.message)
    K, dist, rms = res.K, res.dist, res.rms
    pp_std, foc_std = res.pp_stddev_px, res.focal_stddev_px
```

- [ ] **Step 6: Run the full calibrate suite to verify no regression** (the refactor's safety net)

Run: `cd python-sidecar && .venv/bin/python -m pytest tests/test_calibrate_sl.py tests/test_calibrate_sl_ipc.py -v`
Expected: PASS (all existing tests green — behavior unchanged)

- [ ] **Step 7: Commit**

```bash
git add python-sidecar/src/lmt_vba_sidecar/intrinsics_solve.py python-sidecar/src/lmt_vba_sidecar/calibrate_sl.py python-sidecar/tests/test_intrinsics_solve.py
git commit -m "refactor(sl): extract solve_sl_intrinsics pure helper from calibrate_sl"
```

---

### Task 2: Adaptive distortion model (k1,k2 ↔ full) + `distortion_model` output

**Files:**
- Modify: `python-sidecar/src/lmt_vba_sidecar/intrinsics_solve.py` (`solve_sl_intrinsics`)
- Modify: `python-sidecar/src/lmt_vba_sidecar/calibrate_sl.py:214-225` (add `distortion_model` to output JSON)
- Test: `python-sidecar/tests/test_intrinsics_solve.py`, `python-sidecar/tests/test_calibrate_sl.py`

- [ ] **Step 1: Write the failing tests** (full distortion solved when well-conditioned; output JSON carries the model)

```python
# append to tests/test_intrinsics_solve.py
# Codex #6 fix: project_point (sl_feasibility) is DISTORTION-FREE pinhole, so a "full"
# model can NEVER be accepted from _well_object_image_points (k3/p1/p2 ~ 0 < their stddev
# -> always falls back to radial2). The full-distortion POSITIVE case must synthesize
# image points THROUGH a known non-zero distortion with cv2.projectPoints.
DIST_TRUE = np.array([-0.12, 0.04, 0.0008, -0.0006, 0.02])   # [k1,k2,p1,p2,k3]

def _well_object_image_points_distorted(dist_true, seed=0):
    """Same 6-oblique-pose well-conditioned geometry as _well_object_image_points,
    but pixels are projected through dist_true so k3/tangential become observable."""
    from test_calibrate_sl import _well_meta, _wall_center, _well_poses
    meta, proj, cab, shape = _well_meta()
    world = nominal_dot_positions_world(meta, cab, shape)
    poses = _well_poses(_wall_center(meta, cab, shape))
    ids = sorted(world.keys())
    obj, img = [], []
    for (R, t) in poses:
        o = np.array([world[i] for i in ids], dtype=np.float32)
        rvec, _ = cv2.Rodrigues(R.astype(np.float64))
        proj_pts, _ = cv2.projectPoints(o.reshape(-1, 1, 3), rvec, t.reshape(3, 1).astype(np.float64),
                                        K_TRUE, dist_true)
        obj.append(o)
        img.append(proj_pts.reshape(-1, 2).astype(np.float32))
    return obj, img


def test_solver_solves_full_distortion_when_well_conditioned():
    # Distortion is REAL here, so k3 + tangential are observable -> "full".
    obj, img = _well_object_image_points_distorted(DIST_TRUE)
    res = solve_sl_intrinsics(obj, img, IMG, max_rms_px=1.5, allow_full_distortion=True)
    assert res.distortion_model == "full"
    assert len(res.dist.flatten()) >= 5  # [k1,k2,p1,p2,k3]
    assert abs(res.dist.flatten()[4] - DIST_TRUE[4]) < 0.01   # k3 recovered


def test_solver_falls_back_to_radial2_on_distortion_free_data():
    # Distortion-free truth: k3/tangential are unobservable (~0 < stddev) -> radial2,
    # even with allow_full_distortion=True. (This is the correct fallback, not a bug.)
    obj, img = _well_object_image_points(noise=0.0)
    res = solve_sl_intrinsics(obj, img, IMG, max_rms_px=1.5, allow_full_distortion=True)
    assert res.distortion_model == "radial2"
```

(`cv2` and `K_TRUE` are already imported/defined at the top of `test_intrinsics_solve.py` from Task 1. `cv2.projectPoints` needs no new dependency — `cv2` is already used by `solve_sl_intrinsics`.)

```python
# append to tests/test_calibrate_sl.py (output carries distortion_model)
def test_output_records_distortion_model(tmp_path):
    meta, proj, cab, shape = _well_meta()
    world = nominal_dot_positions_world(meta, cab, shape)
    poses = _well_poses(_wall_center(meta, cab, shape))
    paths = _write_corr(tmp_path, meta, world, poses, noise=0.0)
    rc, out = _run(tmp_path, meta, proj, paths)
    assert rc == 0
    intr = json.loads(out.read_text())
    assert intr["distortion_model"] in ("radial2", "full")
```

- [ ] **Step 2: Run to verify failure**

Run: `cd python-sidecar && .venv/bin/python -m pytest tests/test_intrinsics_solve.py::test_solver_solves_full_distortion_when_well_conditioned tests/test_calibrate_sl.py::test_output_records_distortion_model -v`
Expected: FAIL — `solve_sl_intrinsics()` has no `allow_full_distortion` kwarg; `intr` has no `distortion_model` key.

- [ ] **Step 3: Implement adaptive distortion in `solve_sl_intrinsics`** — add `allow_full_distortion: bool = False` param. When True, solve once with full flags; if a distortion stddev (`std[4]`/`std[5]`) is unstable (> a fraction of the coefficient) or RMS got worse, fall back to radial2.

```python
# solve_sl_intrinsics signature gains: *, max_rms_px, allow_full_distortion: bool = False
# Replace the single `flags = ...; calibrateCameraExtended(...)` with:
    def _solve(full: bool):
        if full:
            f = cv2.CALIB_USE_INTRINSIC_GUESS   # free k1,k2,k3,p1,p2
        else:
            f = cv2.CALIB_USE_INTRINSIC_GUESS | cv2.CALIB_ZERO_TANGENT_DIST | cv2.CALIB_FIX_K3
        return cv2.calibrateCameraExtended(object_points, image_points, image_size,
                                           K0, np.zeros(5), flags=f)
    model = "radial2"
    try:
        rms, K, dist, rvecs, _tvecs, std_int, _std_ext, _pv = _solve(full=False)
        if allow_full_distortion:
            r2, K2, d2, rv2, _t2, si2, _se2, _pv2 = _solve(full=True)
            s2 = np.asarray(si2).flatten()
            # Accept full only if it did not worsen RMS and the extra coeffs are
            # observable (stddev < |coeff|, guarding against runaway distortion DOF).
            k3_ok = abs(d2.flatten()[4]) > s2[8] if len(s2) > 8 else False
            tan_ok = abs(d2.flatten()[2]) > s2[6] and abs(d2.flatten()[3]) > s2[7] if len(s2) > 7 else False
            if r2 <= rms * 1.05 and k3_ok and tan_ok:
                rms, K, dist, rvecs, std_int, model = r2, K2, d2, rv2, si2, "full"
    except cv2.error as e:
        raise IntrinsicsRefused("intrinsics_invalid", f"calibrateCamera failed: {e}")
# ... gates unchanged ... then set IntrinsicsResult(..., distortion_model=model, ...)
```

- [ ] **Step 4: Add `distortion_model` to the calibrate output JSON** — in `calibrate_sl.py:215-225`, add `"distortion_model": res.distortion_model,` to the `json.dumps({...})` dict. Pass `allow_full_distortion` from the cross-check decision (Task 4 wires the real gate; for now calibrate passes `allow_full_distortion=False` to preserve behavior until the cross-check exists).

- [ ] **Step 5: Run tests to verify they pass**

Run: `cd python-sidecar && .venv/bin/python -m pytest tests/test_intrinsics_solve.py tests/test_calibrate_sl.py -v`
Expected: PASS

- [ ] **Step 6: Commit**

```bash
git add python-sidecar/src/lmt_vba_sidecar/intrinsics_solve.py python-sidecar/src/lmt_vba_sidecar/calibrate_sl.py python-sidecar/tests/
git commit -m "feat(sl): adaptive distortion model (k1k2<->full) gated on observability"
```

---

## Part B — Python: anti-absorption cross-check (spec P6 red line)

### Task 3: `crosscheck_intrinsics()` — anchor deviation + flat-wall-no-anchor refusal

**Files:**
- Modify: `python-sidecar/src/lmt_vba_sidecar/intrinsics_solve.py`
- Test: `python-sidecar/tests/test_intrinsics_solve.py`

- [ ] **Step 1: Write the failing tests** (the three cross-check branches)

```python
# append to tests/test_intrinsics_solve.py
from lmt_vba_sidecar.intrinsics_solve import crosscheck_intrinsics, IntrinsicsResult

def _res(K, dist=None, coplanar=0.3):
    return IntrinsicsResult(K=np.asarray(K, float),
                            dist=np.zeros(5) if dist is None else np.asarray(dist, float),
                            rms=0.2, focal_stddev_px=(1.0, 1.0), pp_stddev_px=(0.5, 0.5),
                            distortion_model="radial2", coplanar_ratio=coplanar, rvecs=[])

ANCHOR_K = np.array([[3000.0, 0, 2000.0], [0, 3000.0, 1500.0], [0, 0, 1.0]])

def test_crosscheck_refuses_when_anchor_disagrees_on_aspect():
    # Anisotropic absorption (class b): fx/fy ratio drifted ~2% vs a square-pixel anchor.
    res = _res([[3060.0, 0, 2000.0], [0, 3000.0, 1500.0], [0, 0, 1]])
    refusal = crosscheck_intrinsics(res, anchor_K=ANCHOR_K, anchor_dist=np.zeros(5))
    assert refusal is not None and refusal.code == "observability_failed"
    assert "aspect" in refusal.message.lower() or "anchor" in refusal.message.lower()

def test_crosscheck_refuses_when_anchor_disagrees_on_distortion():
    # Smooth nonlinear remap (class c): focal & aspect UNCHANGED, but distortion drifted.
    # A focal+aspect-only check would MISS this; the distortion-magnitude term catches it.
    res = _res(ANCHOR_K, dist=[-0.15, 0.05, 0.0, 0.0, 0.03])   # nonzero k1,k2,k3 vs anchor 0
    refusal = crosscheck_intrinsics(res, anchor_K=ANCHOR_K, anchor_dist=np.zeros(5))
    assert refusal is not None and refusal.code == "observability_failed"
    assert "distortion" in refusal.message.lower()

def test_crosscheck_passes_when_anchor_agrees():
    res = _res([[3005.0, 0, 2000.0], [0, 3004.0, 1500.0], [0, 0, 1]], dist=[-0.12, 0.04, 0, 0, 0.02])
    assert crosscheck_intrinsics(res, anchor_K=ANCHOR_K, anchor_dist=[-0.12, 0.04, 0, 0, 0.02]) is None

def test_crosscheck_refuses_flat_wall_without_anchor():
    res = _res(np.eye(3), coplanar=1e-5)  # coplanar (flat wall), no anchor
    refusal = crosscheck_intrinsics(res, anchor_K=None, anchor_dist=None)
    assert refusal is not None and refusal.code == "observability_failed"
    assert "flat wall" in refusal.message.lower() or "anchor" in refusal.message.lower()

def test_crosscheck_warns_curved_wall_without_anchor():
    res = _res(np.eye(3), coplanar=0.3)   # non-coplanar (curved), no anchor
    assert crosscheck_intrinsics(res, anchor_K=None, anchor_dist=None) is None  # caller warns
```

- [ ] **Step 2: Run to verify failure**

Run: `cd python-sidecar && .venv/bin/python -m pytest tests/test_intrinsics_solve.py -k crosscheck -v`
Expected: FAIL — `crosscheck_intrinsics` not defined.

- [ ] **Step 3: Implement `crosscheck_intrinsics`**

```python
# intrinsics_solve.py — add constants near the top and the function
FOCAL_CROSSCHECK_MAX_FRAC = 0.02      # |fx - anchor_fx| / anchor_fx   (class a: isotropic scale)
ASPECT_CROSSCHECK_MAX = 0.01          # |fx/fy - anchor_fx/anchor_fy|  (class b: anisotropic)
DISTORTION_CROSSCHECK_MAX_PX = 1.5    # radial-displacement gap at the corner (class c: smooth remap)
_CORNER_R_NORM = 0.6                  # representative normalized radius (wide-lens corner-ish)


def _radial_disp_px(dist, fx) -> float:
    """Radial distortion displacement (px) at the representative corner radius. The
    smooth-remap class lands in k1,k2,k3 and does NOT move fx/aspect, so this term is
    what catches it (Codex critical #1 / spec A.1.3 '畸变量级')."""
    d = np.asarray(dist, float).flatten()
    k1 = d[0] if len(d) > 0 else 0.0
    k2 = d[1] if len(d) > 1 else 0.0
    k3 = d[4] if len(d) > 4 else 0.0
    r = _CORNER_R_NORM
    return abs(fx) * abs(r * (k1 * r**2 + k2 * r**4 + k3 * r**6))


def crosscheck_intrinsics(res: IntrinsicsResult, *, anchor_K, anchor_dist=None) -> IntrinsicsRefused | None:
    """Anti-absorption guard (spec P6/A.1.3). Compares THREE things vs an independent
    anchor — focal (class a), fx/fy aspect (class b), and distortion magnitude (class c).
    Returns IntrinsicsRefused to REFUSE, or None to proceed. A None with no anchor on a
    non-coplanar target means the caller SHOULD emit WarningEvent(code='no_intrinsics_anchor')."""
    fx, fy = float(res.K[0, 0]), float(res.K[1, 1])
    if anchor_K is not None:
        afx, afy = float(anchor_K[0, 0]), float(anchor_K[1, 1])
        focal_dev = abs(fx - afx) / afx if afx else 1.0
        aspect_dev = abs((fx / fy) - (afx / afy)) if (fy and afy) else 1.0
        # class c: compare radial-displacement at the corner (anchor_dist defaults to 0 = no distortion).
        a_dist = np.zeros(5) if anchor_dist is None else np.asarray(anchor_dist, float)
        disp_dev_px = abs(_radial_disp_px(res.dist, fx) - _radial_disp_px(a_dist, afx))
        if focal_dev > FOCAL_CROSSCHECK_MAX_FRAC or aspect_dev > ASPECT_CROSSCHECK_MAX \
                or disp_dev_px > DISTORTION_CROSSCHECK_MAX_PX:
            return IntrinsicsRefused(
                "observability_failed",
                f"auto intrinsics deviate from anchor (focal {focal_dev*100:.1f}%, "
                f"aspect {aspect_dev:.3f}, distortion {disp_dev_px:.2f}px) — "
                "suspected screen pitch/1:1 absorbed into K")
        return None
    if res.coplanar_ratio < COPLANAR_RATIO_MIN:
        return IntrinsicsRefused(
            "observability_failed",
            "flat wall + no independent intrinsics anchor; cannot separate screen "
            "pitch/1:1 from intrinsics — pass an anchor via --intrinsics-crosscheck")
    return None
```

- [ ] **Step 4: Run to verify pass**

Run: `cd python-sidecar && .venv/bin/python -m pytest tests/test_intrinsics_solve.py -k crosscheck -v`
Expected: PASS

- [ ] **Step 5: Commit**

```bash
git add python-sidecar/src/lmt_vba_sidecar/intrinsics_solve.py python-sidecar/tests/test_intrinsics_solve.py
git commit -m "feat(sl): anti-absorption intrinsics cross-check (anchor + flat-wall guard)"
```

---

### Task 4: Wire cross-check into `calibrate-structured-light` (optional anchor) + tie full distortion to anchor

**Files:**
- Modify: `python-sidecar/src/lmt_vba_sidecar/ipc.py:134-145` (`CalibrateStructuredLightInput` gains `crosscheck_intrinsics_path: str | None = None`)
- Modify: `python-sidecar/src/lmt_vba_sidecar/calibrate_sl.py` (load anchor, pass `allow_full_distortion=anchor is not None`, call cross-check)
- Test: `python-sidecar/tests/test_calibrate_sl.py`, `python-sidecar/tests/test_calibrate_sl_ipc.py`

- [ ] **Step 1: Write the failing tests**

```python
# tests/test_calibrate_sl_ipc.py — new field default
def test_crosscheck_path_defaults_none():
    m = CalibrateStructuredLightInput.model_validate({
        "command": "calibrate_structured_light", "version": 1,
        "project": _project().model_dump(),
        "correspondence_paths": ["a.json"], "sl_meta_path": "m.json", "output_path": "o.json",
    })
    assert m.crosscheck_intrinsics_path is None
```

```python
# tests/test_calibrate_sl.py — flat wall + no anchor refuses (cross-check)
def test_flat_wall_no_anchor_refused(tmp_path, capsys):
    # _grid_meta(radius_mm=None) builds a FLAT (coplanar) wall.
    meta, proj, cab, shape = _grid_meta(cols=3, rows=3, radius_mm=None, grid=3)
    world = nominal_dot_positions_world(meta, cab, shape)
    poses = _well_poses(_wall_center(meta, cab, shape))
    paths = _write_corr(tmp_path, meta, world, poses, noise=0.0)
    rc, _ = _run(tmp_path, meta, proj, paths)  # no crosscheck path
    assert rc == 1
    errs = [json.loads(l) for l in capsys.readouterr().out.splitlines()
            if l.strip() and json.loads(l).get("event") == "error"]
    assert errs[-1]["code"] == "observability_failed"
    assert "anchor" in errs[-1]["message"].lower()
```

- [ ] **Step 2: Run to verify failure**

Run: `cd python-sidecar && .venv/bin/python -m pytest tests/test_calibrate_sl_ipc.py::test_crosscheck_path_defaults_none tests/test_calibrate_sl.py::test_flat_wall_no_anchor_refused -v`
Expected: FAIL — field missing; flat wall currently solves (no cross-check) so `rc == 0`.

- [ ] **Step 3: Add the IPC field** — `ipc.py` `CalibrateStructuredLightInput` (after `max_rms_px`):

```python
    # Optional independent intrinsics anchor (checkerboard intrinsics.json) for the
    # anti-absorption cross-check. Without it, a coplanar (flat) wall is refused.
    crosscheck_intrinsics_path: str | None = None
```

- [ ] **Step 4: Wire it in `calibrate_sl.py`** — after the helper call, load the anchor K (if any), choose `allow_full_distortion`, and run the cross-check:

```python
    anchor_K = anchor_dist = None
    if cmd.crosscheck_intrinsics_path:
        try:
            anchor = json.loads(pathlib.Path(cmd.crosscheck_intrinsics_path).read_text())
            anchor_K = np.array(anchor["K"], float)
            anchor_dist = np.array(anchor.get("dist_coeffs", [0, 0, 0, 0, 0]), float)
        except (OSError, json.JSONDecodeError, KeyError, ValueError) as e:
            return _err("invalid_input", f"crosscheck intrinsics load failed: {e}")
    try:
        res = solve_sl_intrinsics(object_points, image_points, image_size,
                                  max_rms_px=cmd.max_rms_px,
                                  allow_full_distortion=anchor_K is not None)
    except IntrinsicsRefused as e:
        return _err(e.code, e.message)
    refusal = crosscheck_intrinsics(res, anchor_K=anchor_K, anchor_dist=anchor_dist)
    if refusal is not None:
        return _err(refusal.code, refusal.message)
    if anchor_K is None and res.coplanar_ratio >= COPLANAR_RATIO_MIN:
        write_event(WarningEvent(event="warning", code="no_intrinsics_anchor",
            message="no independent intrinsics anchor; anisotropic pitch/1:1 absorption is unguarded"))
    K, dist, rms = res.K, res.dist, res.rms
    pp_std, foc_std = res.pp_stddev_px, res.focal_stddev_px
```

(Import `WarningEvent` and `crosscheck_intrinsics` at the top of `calibrate_sl.py`.)

- [ ] **Step 5: Run tests + full calibrate regression**

Run: `cd python-sidecar && .venv/bin/python -m pytest tests/test_calibrate_sl.py tests/test_calibrate_sl_ipc.py -v`
Expected: PASS (note: a previously-passing flat-wall happy test, if any, must now supply `crosscheck_intrinsics_path` — update it to pass an anchor; curved-wall tests are unaffected).

- [ ] **Step 6: Commit**

```bash
git add python-sidecar/src/lmt_vba_sidecar/calibrate_sl.py python-sidecar/src/lmt_vba_sidecar/ipc.py python-sidecar/tests/
git commit -m "feat(sl): calibrate cross-check anchor + flat-wall refusal; full distortion needs anchor"
```

---

## Part C — Python: `--intrinsics auto` inline self-cal in reconstruct

### Task 5: `intrinsics_path == "auto"` branch in `sl_reconstruct`

**Files:**
- Modify: `python-sidecar/src/lmt_vba_sidecar/ipc.py:121-131` (`ReconstructStructuredLightInput` gains `crosscheck_intrinsics_path: str | None = None`)
- Modify: `python-sidecar/src/lmt_vba_sidecar/sl_reconstruct.py:107-121` (auto branch)
- Test: `python-sidecar/tests/test_sl_reconstruct.py`

- [ ] **Step 1: Write the failing test** (auto path recovers the same pose as the file path)

```python
# append to tests/test_sl_reconstruct.py
# Codex #7 fix: shape_prior CANNOT be the bare string "curved" (IPC ShapePrior accepts
# only Literal "flat" or {"curved": {"radius_mm": ...}}; bare "curved" raises ValidationError).
# Simplest sound auto test = FLAT wall (default shape_prior) + an explicit anchor (flat wall
# without an anchor is refused by the cross-check), avoiding curved-arc truth geometry entirely.
def _write_anchor(tmp_path, K, name="anchor.json"):
    p = tmp_path / name
    p.write_text(json.dumps({"K": K.tolist(), "dist_coeffs": [0, 0, 0, 0, 0], "image_size": [4000, 3000]}))
    return p

def test_intrinsics_auto_self_calibrates(tmp_path):
    """--intrinsics auto solves K from the same corr files and reconstructs. Flat wall +
    anchor (K_TRUE): self-cal recovers ~K_TRUE so the cross-check passes."""
    meta_path = _gen_two_cabinet_meta(tmp_path)          # FLAT 2-cabinet wall (exists at line 53)
    meta = json.loads(meta_path.read_text())
    _, K = _write_intrinsics(tmp_path)                   # K_TRUE = synthesis camera
    anchor = _write_anchor(tmp_path, K)                  # independent anchor == K_TRUE
    rect_by_cr = {(c["col"], c["row"]): c["input_rect_px"] for c in meta["cabinets"]}
    pitch_by_cr = {(c["col"], c["row"]): c["pixel_pitch_mm"] for c in meta["cabinets"]}
    cab_by_id = {d["id"]: tuple(d["cabinet"]) for d in meta["dots"]}
    cab_world_t = {(0, 0): np.zeros(3), (1, 0): np.array([500.0, 0.0, 0.0])}
    truth = {}
    for d in meta["dots"]:
        cr = cab_by_id[d["id"]]
        truth[d["id"]] = sl_local_mm(tuple(rect_by_cr[cr]), d["u"], d["v"],
                                     pitch_by_cr[cr][0], pitch_by_cr[cr][1]) + cab_world_t[cr]
    sha = hashlib.sha256(meta_path.read_bytes()).hexdigest()
    poses = [look_at_pose(np.array([px, py, -3500.0]), np.array([250.0, 0.0, 0.0]))
             for (px, py) in [(-1200, -400), (-400, 400), (400, -400), (1200, 400), (0, 800), (0, -800)]]
    rng = np.random.default_rng(0)
    corr_paths = []
    for vi, (R, t) in enumerate(poses):
        pts = [{"id": d["id"], "u": d["u"], "v": d["v"],
                **dict(zip(("x", "y"), (project_point(K, R, t, truth[d["id"]]) + rng.normal(0, 0.1, 2)).tolist()))}
               for d in meta["dots"]]
        cp = tmp_path / f"corr_{vi}.json"
        cp.write_text(json.dumps({"schema_version": 1, "screen_id": "MAIN", "sl_meta_sha256": sha,
            "screen_resolution": meta["screen_resolution"], "camera_image_size": [4000, 3000],
            "source_input": f"/cap/p{vi}.mp4", "points": pts}))
        corr_paths.append(str(cp))
    report = tmp_path / "rep.json"
    cmd = ReconstructStructuredLightInput.model_validate({
        "command": "reconstruct_structured_light", "version": 1,
        "project": {"screen_id": "MAIN", "cabinet_array": {"cols": 2, "rows": 1,
                    "absent_cells": [], "cabinet_size_mm": [500, 500]}},   # default shape_prior="flat"
        "correspondence_paths": corr_paths, "sl_meta_path": str(meta_path),
        "intrinsics_path": "auto", "crosscheck_intrinsics_path": str(anchor),
        "pose_report_path": str(report)})
    assert run_reconstruct_structured_light(cmd) == 0
    by_id = {c["cabinet_id"]: c for c in json.loads(report.read_text())["cabinet_poses"]}
    rel = np.array(by_id["V001_R000"]["position_mm"]) - np.array(by_id["V000_R000"]["position_mm"])
    assert np.linalg.norm(rel - np.array([500.0, 0.0, 0.0])) < 8.0  # self-cal noisier than given K
```

(No curved helper needed — `_gen_two_cabinet_meta` at `test_sl_reconstruct.py:53` is already a flat 2-cabinet wall. The anchor makes the flat-wall cross-check pass; a flat-wall `auto` WITHOUT an anchor is refused by design — that refusal is its own assertion below.)

- [ ] **Step 2: Run to verify failure**

Run: `cd python-sidecar && .venv/bin/python -m pytest tests/test_sl_reconstruct.py::test_intrinsics_auto_self_calibrates -v`
Expected: FAIL — `intrinsics_path="auto"` hits `json.loads(pathlib.Path("auto").read_text())` → `OSError` → `intrinsics_invalid` (rc 1).

- [ ] **Step 3: Add the IPC field + the auto branch** — `ipc.py` `ReconstructStructuredLightInput` gains `crosscheck_intrinsics_path: str | None = None`. In `sl_reconstruct.py`, replace the unconditional intrinsics load at lines 108-115 with:

```python
    # --- 3. intrinsics (file path OR inline self-cal via the "auto" sentinel) ---
    if cmd.intrinsics_path == "auto":
        try:
            K, dist, image_size = _self_calibrate_inline(meta, corr_files, cmd)
            intrinsics_source = "auto_self_calibrated"
        except IntrinsicsRefused as e:
            write_event(ErrorEvent(event="error", code=e.code, message=e.message, fatal=True))
            return 1
    else:
        try:
            intr = json.loads(pathlib.Path(cmd.intrinsics_path).read_text())
            K = np.array(intr["K"], dtype=float)
            dist = np.array(intr["dist_coeffs"], dtype=float)
            image_size = tuple(int(v) for v in intr["image_size"])
            intrinsics_source = "file"
        except (OSError, json.JSONDecodeError, KeyError, ValueError) as e:
            write_event(ErrorEvent(event="error", code="intrinsics_invalid", message=f"intrinsics load failed: {e}", fatal=True))
            return 1
    # (existing camera_image_size consistency loop at 116-121 stays, runs against image_size)
```

Add the inline helper (near the top of `sl_reconstruct.py`, after imports) that assembles object/image points exactly like `calibrate_sl.py:149-162`, calls `solve_sl_intrinsics` + `crosscheck_intrinsics`, and emits the `no_intrinsics_anchor` warning:

```python
def _self_calibrate_inline(meta, corr_files, cmd):
    from lmt_vba_sidecar.intrinsics_solve import (solve_sl_intrinsics, crosscheck_intrinsics,
                                                  IntrinsicsRefused, COPLANAR_RATIO_MIN)
    from lmt_vba_sidecar.nominal import nominal_dot_positions_world
    dot_world = nominal_dot_positions_world(meta, cmd.project.cabinet_array, cmd.project.shape_prior)
    object_points, image_points = [], []
    image_size = tuple(int(v) for v in corr_files[0].camera_image_size)
    for cf in corr_files:
        objp = [dot_world[int(p.id)] for p in cf.points if int(p.id) in dot_world]
        imgp = [[p.x, p.y] for p in cf.points if int(p.id) in dot_world]
        if len(objp) >= 4:
            object_points.append(np.asarray(objp, dtype=np.float32))
            image_points.append(np.asarray(imgp, dtype=np.float32))
    res = solve_sl_intrinsics(object_points, image_points, image_size, max_rms_px=1.5,
                              allow_full_distortion=bool(cmd.crosscheck_intrinsics_path))
    anchor_K = anchor_dist = None
    if cmd.crosscheck_intrinsics_path:
        anchor = json.loads(pathlib.Path(cmd.crosscheck_intrinsics_path).read_text())
        anchor_K = np.array(anchor["K"], float)
        anchor_dist = np.array(anchor.get("dist_coeffs", [0, 0, 0, 0, 0]), float)
    refusal = crosscheck_intrinsics(res, anchor_K=anchor_K, anchor_dist=anchor_dist)
    if refusal is not None:
        raise refusal
    if anchor_K is None and res.coplanar_ratio >= COPLANAR_RATIO_MIN:
        write_event(WarningEvent(event="warning", code="no_intrinsics_anchor",
            message="auto intrinsics solved without an independent anchor; anisotropic pitch/1:1 unguarded"))
    return res.K, res.dist, image_size
```

Thread `intrinsics_source` into the result: pass it to `solve_and_emit` is overkill — instead emit it on the result. Simplest: store it and add to the `ResultData` via a new optional field. **Add `intrinsics_source: str = "file"` to `ipc.py` `ResultData` (after `procrustes_align_rms_m`, with default for back-compat)** and set it where `solve_and_emit` builds `ResultData` (`reconstruct.py:711-728`) — thread `intrinsics_source` as a new `solve_and_emit` kwarg (default `"file"`), passed `="auto_self_calibrated"` from the auto branch.

- [ ] **Step 4: Run to verify pass**

Run: `cd python-sidecar && .venv/bin/python -m pytest tests/test_sl_reconstruct.py::test_intrinsics_auto_self_calibrates -v`
Expected: PASS

- [ ] **Step 5: Run full sl_reconstruct + reconstruct suites (regression)**

Run: `cd python-sidecar && .venv/bin/python -m pytest tests/test_sl_reconstruct.py tests/test_reconstruct.py -v`
Expected: PASS

- [ ] **Step 6: Commit**

```bash
git add python-sidecar/src/lmt_vba_sidecar/sl_reconstruct.py python-sidecar/src/lmt_vba_sidecar/reconstruct.py python-sidecar/src/lmt_vba_sidecar/ipc.py python-sidecar/tests/test_sl_reconstruct.py
git commit -m "feat(sl): --intrinsics auto inline self-cal in reconstruct-structured-light"
```

---

### Task 6: P6 pitch-absorption-guard test (no-ship red line)

**Files:**
- Test: `python-sidecar/tests/test_sl_reconstruct.py`

- [ ] **Step 1: Write the guard matrix** (Codex critical #1 + #2 + #7) — flat wall + anchor; parametrize the two **K-absorbable** classes: (b) anisotropic `sx≠sy` (injected **3% > the 1% aspect threshold**, fixing Codex #2's below-threshold 0.6%) and (c) smooth nonlinear radial **remap** (drifts distortion, which focal+aspect-only would miss — this is the class the distortion-magnitude term now catches). For each: GUARD ON (anchor) → refuse + no file; CONTROL (no injection, anchor) → passes — proving the refusal is error-caused. The isotropic class (a) is the procrustes/`nominal_misfit` guard, tested in **Plan 3** (`test_nominal_misfit_warns_on_global_scale`).

```python
# append to tests/test_sl_reconstruct.py
def _warp_local(pl, kind):
    """Inject a screen-side pitch/1:1 error into a centered-origin local-mm point."""
    if kind == "anisotropic":
        return pl * np.array([1.03, 1.0, 1.0])              # 3% x-stretch -> aspect drift > 1%
    if kind == "remap":                                     # smooth radial barrel: +2% at the edge
        r = float(np.hypot(pl[0], pl[1]))
        r_max = 353.0                                        # ~half-diagonal of a 500mm cabinet
        s = 1.0 + 0.02 * (r / r_max) ** 2
        return pl * np.array([s, s, 1.0])
    raise ValueError(kind)


@pytest.mark.parametrize("kind", ["anisotropic", "remap"])
def test_pitch_absorption_guard(tmp_path, kind):
    """P6 red line: a K-absorbable screen-pitch error projected into a FLAT-wall capture is
    caught by the cross-check when an anchor is supplied (REFUSE, no file). A control run
    (no injection, same anchor) passes — proving the refusal is caused by the error, not the
    anchor. (Isotropic scale -> nominal_misfit guard, tested in Plan 3.)"""
    meta_path = _gen_two_cabinet_meta(tmp_path)              # FLAT wall
    meta = json.loads(meta_path.read_text())
    _, K = _write_intrinsics(tmp_path)
    anchor = _write_anchor(tmp_path, K)                      # K_TRUE, dist=0 (helper from Task 5)
    rect_by_cr = {(c["col"], c["row"]): c["input_rect_px"] for c in meta["cabinets"]}
    pitch_by_cr = {(c["col"], c["row"]): c["pixel_pitch_mm"] for c in meta["cabinets"]}
    cab_by_id = {d["id"]: tuple(d["cabinet"]) for d in meta["dots"]}
    cab_world_t = {(0, 0): np.zeros(3), (1, 0): np.array([500.0, 0.0, 0.0])}
    poses = [look_at_pose(np.array([px, py, -3500.0]), np.array([250.0, 0.0, 0.0]))
             for (px, py) in [(-1200, -400), (-400, 400), (400, -400), (1200, 400), (0, 800), (0, -800)]]

    def _corr(inject):
        rng = np.random.default_rng(0)
        paths = []
        sha = hashlib.sha256(meta_path.read_bytes()).hexdigest()
        for vi, (R, t) in enumerate(poses):
            pts = []
            for d in meta["dots"]:
                cr = cab_by_id[d["id"]]
                pl = sl_local_mm(tuple(rect_by_cr[cr]), d["u"], d["v"], pitch_by_cr[cr][0], pitch_by_cr[cr][1])
                if inject:
                    pl = _warp_local(pl, kind)
                p = project_point(K, R, t, pl + cab_world_t[cr]) + rng.normal(0, 0.1, 2)
                pts.append({"id": d["id"], "u": d["u"], "v": d["v"], "x": float(p[0]), "y": float(p[1])})
            cp = tmp_path / f"corr_{inject}_{vi}.json"
            cp.write_text(json.dumps({"schema_version": 1, "screen_id": "MAIN", "sl_meta_sha256": sha,
                "screen_resolution": meta["screen_resolution"], "camera_image_size": [4000, 3000],
                "source_input": f"/cap/{inject}{vi}.mp4", "points": pts}))
            paths.append(str(cp))
        return paths

    base = {"command": "reconstruct_structured_light", "version": 1,
            "project": {"screen_id": "MAIN", "cabinet_array": {"cols": 2, "rows": 1,
                        "absent_cells": [], "cabinet_size_mm": [500, 500]}},   # default flat
            "sl_meta_path": str(meta_path), "intrinsics_path": "auto",
            "crosscheck_intrinsics_path": str(anchor)}

    # GUARD ON, error injected -> the absorbed deviation trips the cross-check -> refuse, no file.
    rep_on = tmp_path / "on.json"
    cmd_on = ReconstructStructuredLightInput.model_validate(
        {**base, "correspondence_paths": _corr(inject=True), "pose_report_path": str(rep_on)})
    assert run_reconstruct_structured_light(cmd_on) == 1
    assert not rep_on.exists()                               # no silent wrong file

    # CONTROL, no injection, same anchor -> self-cal ~ anchor -> passes (refusal was error-caused).
    rep_ctl = tmp_path / "ctl.json"
    cmd_ctl = ReconstructStructuredLightInput.model_validate(
        {**base, "correspondence_paths": _corr(inject=False), "pose_report_path": str(rep_ctl)})
    assert run_reconstruct_structured_light(cmd_ctl) == 0
```

- [ ] **Step 2: Run it**

Run: `cd python-sidecar && .venv/bin/python -m pytest tests/test_sl_reconstruct.py::test_pitch_absorption_guard -v`
Expected: PASS for both `kind` params (injected → refuse + no file; control → solves). If an injected case does NOT refuse, the injection is below the cross-check threshold — raise it (anisotropic `sx`, remap edge %) or tighten `ASPECT_CROSSCHECK_MAX`/`DISTORTION_CROSSCHECK_MAX_PX` in `intrinsics_solve.py`. (Codex #2: the injection MUST exceed the threshold for the red/green to be sound.)

- [ ] **Step 3: Commit**

```bash
git add python-sidecar/tests/test_sl_reconstruct.py
git commit -m "test(sl): P6 pitch-absorption guard matrix (anisotropic + remap, anchor refuse + control)"
```

---

## Part D — Rust surface (cli → transport → service → IPC → DTO → schema)

### Task 7: clap flags — `--intrinsics-crosscheck` on Reconstruct + Calibrate (`--intrinsics auto` needs no schema change)

**Files:**
- Modify: `crates/lmt-cli/src/cli.rs:374-388` (Reconstruct), `:391-412` (Calibrate)

- [ ] **Step 1: Add the flags** — `--intrinsics` is already a `String`, so `auto` is accepted as a value (document it). Add `--intrinsics-crosscheck`:

```rust
// cli.rs ReconstructStructuredLight, insert before line 388 `},`:
        /// 内参 anchor JSON 路径(独立棋盘格标定),仅 --intrinsics auto 时用于防吸收交叉校验。
        #[arg(long = "intrinsics-crosscheck")]
        intrinsics_crosscheck: Option<String>,
// cli.rs CalibrateStructuredLight, insert before line 412 `},`:
        /// 内参 anchor JSON 路径,启用防吸收交叉校验(平面墙无 anchor 将被拒)。
        #[arg(long = "intrinsics-crosscheck")]
        intrinsics_crosscheck: Option<String>,
```

- [ ] **Step 2: Build to verify it compiles** (the dispatch arms now fail to compile — expected, fixed in Task 8)

Run: `cargo build -p lmt-cli 2>&1 | head -5`
Expected: error E0027 (pattern does not mention field `intrinsics_crosscheck`) — proves the field was added.

- [ ] **Step 3: Commit after Task 8 compiles** (this task's code is committed together with Task 8 since they must compile as a unit).

### Task 8: Thread sentinel + crosscheck through transport → service → IPC

**Files:**
- Modify: `crates/lmt-cli/src/commands/visual.rs:132-152` (dispatch), `:538-606` (reconstruct fn), `:613-689` (calibrate fn)
- Modify: `crates/lmt-app/src/visual.rs:235-268` (reconstruct), `:279-328` (calibrate)
- Modify: `crates/adapter-visual-ba/src/api.rs:196-224`, `:338-361` (args + payload)

- [ ] **Step 1: Update the dispatch + transport fns** — destructure the new field and pass it. For reconstruct, `intrinsics` stays a `&str` (NOT `Path::new`) so `"auto"` survives:

```rust
// visual.rs dispatch (132-140) — add intrinsics_crosscheck to the pattern + call:
        VisualCmd::ReconstructStructuredLight {
            project_path, screen_id, sl_meta, intrinsics, correspondences, intrinsics_crosscheck,
        } => reconstruct_structured_light(
            mode, &project_path, &screen_id, &sl_meta, &intrinsics,
            intrinsics_crosscheck.as_deref(), &correspondences, yes, dry_run,
        ),
// reconstruct_structured_light fn (538): add param `intrinsics_crosscheck: Option<&str>`,
// keep `intrinsics: &str`; in Execute (588-594) pass intrinsics as &str (NOT Path::new) and
// the crosscheck through to run_reconstruct_structured_light.
```

```rust
// app/visual.rs run_reconstruct_structured_light (235): change
//   intrinsics: &Path  ->  intrinsics: &str   (so "auto" is not path-normalized)
//   add param: intrinsics_crosscheck: Option<&str>
// args build (254-262): intrinsics_path: intrinsics.to_string(),
//   crosscheck_intrinsics_path: intrinsics_crosscheck.map(str::to_string),
// persist_reconstruct_result must carry through `intrinsics_source` from ReconstructOut.
```

```rust
// api.rs ReconstructStructuredLightArgs (196-206): add
//   pub crosscheck_intrinsics_path: Option<String>,
// payload (216-224): add "crosscheck_intrinsics_path": &args.crosscheck_intrinsics_path,
// ReconstructOut (258-267): add `intrinsics_source: String` read from result (api.rs:234 ResultData).
```

Mirror the same `intrinsics_crosscheck` threading for calibrate (`visual.rs:141-152` dispatch, `:613-689` fn, `app/visual.rs:279-328`, `api.rs:338-361` payload + `CalibrateStructuredLightArgs`).

- [ ] **Step 2: Build the workspace**

Run: `cargo build --workspace 2>&1 | tail -5`
Expected: compiles clean (0 errors).

- [ ] **Step 3: Commit**

```bash
git add crates/lmt-cli/src/cli.rs crates/lmt-cli/src/commands/visual.rs crates/lmt-app/src/visual.rs crates/adapter-visual-ba/src/api.rs
git commit -m "feat(cli): --intrinsics auto sentinel + --intrinsics-crosscheck threading"
```

### Task 9: DTO fields + schema presence test

**Files:**
- Modify: `crates/lmt-shared/src/dto.rs:242-258` (VisualReconstructResult), `:307-312` (CalibrateResult)
- Modify: `crates/lmt-app/src/visual.rs` (populate), `crates/adapter-visual-ba/src/api.rs:284-388` (CalibrateOut + IntrinsicsFile)
- Test: `crates/lmt-shared/src/schema.rs` (presence test)

- [ ] **Step 1: Write the failing schema presence test** — clone `schema.rs:155-164`:

```rust
#[test]
fn visual_reconstruct_result_schema_has_intrinsics_source() {
    let v = dump_all();
    let props = v["types"]["VisualReconstructResult"]["properties"].as_object().unwrap();
    assert!(props.contains_key("intrinsics_source"));
}
#[test]
fn calibrate_result_schema_has_distortion_model() {
    let v = dump_all();
    let props = v["types"]["CalibrateResult"]["properties"].as_object().unwrap();
    assert!(props.contains_key("distortion_model"));
    assert!(props.contains_key("focal_stddev_px"));
}
```

- [ ] **Step 2: Run to verify failure**

Run: `cargo test -p lmt-shared schema 2>&1 | tail -8`
Expected: FAIL — properties absent.

- [ ] **Step 3: Add the DTO fields** (no new `add!` — both types already registered at `schema.rs:67,73`):

```rust
// dto.rs VisualReconstructResult, after line 256 (procrustes_align_rms_m), before `cabinets`:
    /// "file" (provided intrinsics) | "auto_self_calibrated" (--intrinsics auto).
    #[serde(default)]
    pub intrinsics_source: String,
// dto.rs CalibrateResult, before closing `}` at 312:
    #[serde(default)]
    pub distortion_model: String,
    #[serde(default)]
    pub focal_stddev_px: Option<[f64; 2]>,
    #[serde(default)]
    pub pp_stddev_px: Option<[f64; 2]>,
```

- [ ] **Step 4: Populate them** — `api.rs` `IntrinsicsFile` (373-377) reads `distortion_model`, `focal_stddev_px`, `pp_stddev_px` from the intrinsics JSON; `CalibrateOut` (284-288) carries them; `run_calibrate_structured_light` (`app/visual.rs:323-327`) sets them. `intrinsics_source` flows from `ReconstructOut` (api.rs:258-267) into `persist_reconstruct_result`. The checkerboard `run_calibrate` path sets `distortion_model: "radial2".into()` and `None` stddevs (it has none).

- [ ] **Step 5: Run schema test + verify dump**

Run: `cargo test -p lmt-shared schema && cargo build --workspace && ./target/debug/lmt --json schema | jq '.types.VisualReconstructResult.properties.intrinsics_source, .types.CalibrateResult.properties.distortion_model'`
Expected: tests PASS; jq prints two non-null schema objects.

- [ ] **Step 6: Commit**

```bash
git add crates/lmt-shared/src/dto.rs crates/lmt-shared/src/schema.rs crates/lmt-app/src/visual.rs crates/adapter-visual-ba/src/api.rs
git commit -m "feat(dto): intrinsics_source + calibrate distortion_model/stddev fields"
```

---

## Part E — E2E + docs

### Task 10: CLI E2E (mock refuse/dry-run + gated real-sidecar happy)

**Files:**
- Modify: `crates/lmt-cli/tests/cli_e2e.rs`

- [ ] **Step 1: Add mock-level tests** (clone the templates at `cli_e2e.rs:2089-2135`)

```rust
#[test]
fn reconstruct_structured_light_auto_dry_run_writes_nothing() {
    // --intrinsics auto must be accepted by clap and reach the dry-run payload (no file load).
    let tmp = TempDir::new().unwrap();
    let proj = tmp.path().join("proj");
    write_gp_project(&proj, 2, 1);
    let meta = tmp.path().join("sl_meta.json"); std::fs::write(&meta, "{}").unwrap();
    let c0 = tmp.path().join("c0.json"); std::fs::write(&c0, "{}").unwrap();
    let c1 = tmp.path().join("c1.json"); std::fs::write(&c1, "{}").unwrap();
    let assert = lmt().args(["--json", "--dry-run", "visual", "reconstruct-structured-light",
        proj.to_str().unwrap(), "MAIN", "--sl-meta", meta.to_str().unwrap(),
        "--intrinsics", "auto",
        "--corr", c0.to_str().unwrap(), "--corr", c1.to_str().unwrap()])
        .assert().success();
    let env: Value = serde_json::from_slice(&assert.get_output().stdout).unwrap();
    assert_eq!(env["data"]["dry_run"], true);
    assert_eq!(env["data"]["intrinsics"], "auto");
    assert!(!proj.join("measurements/measured.yaml").exists());
}
```

- [ ] **Step 2: Add a gated real-sidecar test** (clone `cli_e2e.rs:2245-2312`, `#[ignore]`): generate SL, decode N poses, run `reconstruct-structured-light --intrinsics auto --yes`, assert `env["data"]["intrinsics_source"] == "auto_self_calibrated"` and `ba_rms_px` finite. Also a flat-wall-no-anchor variant asserting failure with `error.code == "observability_failed"` (use the error-mock helper at `cli_e2e.rs:1308-1341` style if a real flat-wall fixture is too heavy).

- [ ] **Step 3: Run E2E**

Run: `cargo test -p lmt-cli --test cli_e2e reconstruct_structured_light_auto 2>&1 | tail -8`
Expected: mock tests PASS; `#[ignore]` real-sidecar tests skipped unless `LMT_VBA_SIDECAR_PATH` set.

- [ ] **Step 4: Commit**

```bash
git add crates/lmt-cli/tests/cli_e2e.rs
git commit -m "test(e2e): --intrinsics auto dry-run + gated real-sidecar self-cal"
```

### Task 11: Update `docs/agents-cli.md`

**Files:**
- Modify: `docs/agents-cli.md`

- [ ] **Step 1: Update the `reconstruct-structured-light` row** — add `[--intrinsics-crosscheck <path>]` to the signature and the `--intrinsics auto` semantics (inline self-cal from the same corr; flat wall without anchor → `observability_failed`; curved wall without anchor → `no_intrinsics_anchor` warning). Add `intrinsics_source` to its result-field list.
- [ ] **Step 2: Update the `calibrate-structured-light` row** — `[--intrinsics-crosscheck <path>]`; adaptive `distortion_model` (radial2|full, full needs anchor); new result fields `distortion_model`/`focal_stddev_px`/`pp_stddev_px`.
- [ ] **Step 3: Note in "Not exposed in the GUI"** that the precision additions stay CLI-only (the whole `visual` group already is). No error-code table change (reuses `observability_failed`/`intrinsics_invalid`).
- [ ] **Step 4: Final self-check**

Run: `cargo test --workspace 2>&1 | tail -5 && cd python-sidecar && .venv/bin/python -m pytest tests/ 2>&1 | tail -5`
Expected: all green.

- [ ] **Step 5: Commit**

```bash
git add docs/agents-cli.md
git commit -m "docs(agents-cli): --intrinsics auto + crosscheck + adaptive distortion"
```

---

## Follow-on plans (separate, smaller)

**Plan 2 — L2 subpixel (spec item 4, `decode-structured-light`).** Independent; precision does not depend on it.
- Task: capture `lbl` at `sl_decode.py:186`; intensity-weighted centroid at `:197` with the Otsu `cent[i]` retained as fallback when weight-sum degenerates.
- Task: field-like fixtures (`test_sl_decode.py`) — bloom, saturation flat-top, glare, merged components — gate on **end-to-end pose error**, not just centroid error; if weighted-centroid doesn't beat Otsu on these, keep Otsu default.
- No CLI/DTO/schema change (corr.json format unchanged). No new flag.

**Plan 3 — L3 min-views + compare-known thresholds (spec items 5/6).**
- L3: add `min_views: int = 2` to `PlanCaptureInput` (ipc.py:550); thread through `cmd.py:49-54` → `optimize`/`_score`(optimize.py:48,56,58) → `score_screen`(scoring.py:36) → `coverage_report`(visibility.py:124,147)/`bridging_report`. Keep `gates.MIN_VIEWS` as the default (the mirror test `test_capture_planner_gates.py` requires it). Rust: `--min-views` on `PlanCapture` (cli.rs:486) → `run_plan_capture`(app/visual.rs:789).
- compare-known thresholds: Python already honors `CompareKnownInput.thresholds`; the gap is Rust never sends them. Add `--max-size-mm/--max-dist-mm/--max-angle-deg` (spec-contract names) on `CompareKnown` (cli.rs:454) → `run_compare_known`(app/visual.rs:703) → `CompareKnownArgs`(api.rs:707) → payload. `CompareKnownResult.thresholds` already echoes them back.

**Phase 2 — L5 in-BA distortion refine.** Bench-gated; its own plan after P6 passes (spec §B.1): extend `visual simulate` to inject distortion + the three pitch/1:1 classes, `visual eval` to compare frozen-K vs in-BA refine; promote `--refine-distortion` only if pose error drops AND injected focal error is not absorbed into a fake cabinet bow.

---

## Self-Review

**Spec coverage:** L1 distortion adaptive + **distortion-aware full-positive test** (Task 2 ✓, Codex #6), `--intrinsics auto` on **flat-wall+anchor** (Task 5 ✓, Codex #7 — bare `"curved"` is invalid IPC), cross-check on **focal+aspect+distortion magnitude** (Tasks 3-4 ✓, Codex critical #1 — focal/aspect alone misses the smooth-remap class), P6 guard as a **2-class matrix (anisotropic + remap) with injection above threshold + control** (Task 6 ✓, Codex #1/#2), DTO `intrinsics_source`/`distortion_model`/stddev (Task 9 ✓), CLI flags (Tasks 7-8 ✓), E2E (Task 10 ✓), docs (Task 11 ✓). L2/L3/compare-known deferred to Plans 2/3. Phase-2/L5 deferred. **The third P6 class (isotropic scale) is the `nominal_misfit` procrustes guard in Plan 3** (`test_nominal_misfit_warns_on_global_scale`) — three classes × three guards, cross-referenced both ways.

**Placeholder scan:** no TBD/TODO; every code step shows real code; Rust mechanical edits reference verbatim current line numbers + the exact insertion.

**Type consistency:** `solve_sl_intrinsics`/`IntrinsicsResult`/`IntrinsicsRefused`/`crosscheck_intrinsics` names are consistent across Tasks 1-6; `intrinsics_source` string values `"file"`/`"auto_self_calibrated"` consistent across Python (Task 5) and Rust DTO (Task 9); `crosscheck_intrinsics_path` (Python IPC) ↔ `--intrinsics-crosscheck`/`intrinsics_crosscheck` (Rust) ↔ `crosscheck_intrinsics_path` (api.rs args) consistent; `distortion_model` ("radial2"|"full") consistent calibrate output → DTO.
