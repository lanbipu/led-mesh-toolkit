# Step-1 SL Camera Calibration Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add `lmt visual calibrate-structured-light` — calibrate one camera's intrinsics (fx,fy,cx,cy,k1,k2) from its structured-light white-dot captures of the as-built wall, using the nominal design wall as a known 3D target.

**Architecture:** Sibling of `reconstruct-structured-light`, reusing the same SL transport chain (clap → lmt-app → adapter → sidecar) minus the `--intrinsics` input (we *produce* it). New sidecar module `calibrate_sl.py` runs `cv2.calibrateCameraExtended` against per-dot nominal 3D world points and refuses on degenerate observability. Output is a NON-destructive `<screen_id>_sl_intrinsics.json` in the existing 5-key intrinsics contract.

**Tech Stack:** Rust (clap, tokio, serde) for CLI/app/adapter; Python sidecar (pydantic, numpy, OpenCV) for the solver; pytest + assert_cmd E2E.

**Spec:** `docs/superpowers/specs/2026-05-30-sl-camera-calibration-design.md` (v2, post Codex review).

**Conventions verified against current `origin/main`:**
- Intrinsics file = `{K, dist_coeffs, image_size, reproj_error_px, frames_used}` (calibrate.py:211-217). Readers consume only K/dist/image_size; the Rust adapter reads reproj_error_px+frames_used.
- `invalid_input` → exit **2**; `intrinsics_invalid` → 16; `observability_failed` → 17 (cli_e2e.rs refuse test asserts code 2 for invalid_input).
- Sidecar command wiring = argparse subparser + `SUBCOMMAND_MODULES` + `SUBCOMMAND_ENTRYPOINTS` (`(fn_name, PydanticInputModel)`) in `__main__.py`; the run fn returns `int` and MUST emit a `result` event or `run_sidecar` errors `NoResultEvent`.
- `map_vba_err` (visual.rs:40-61) already maps `invalid_input`/`intrinsics_invalid`/`observability_failed`.
- Use the venv at `python-sidecar/.venv` per memory `reference_worktree_venv_isolation` — do NOT symlink the main repo's `.venv`.

---

## Task 0: Baseline — confirm the worktree builds and tests are green

**Files:** none.

- [ ] **Step 1: Build the workspace**

Run: `cargo build --workspace`
Expected: completes (lmt-cli already built clean off origin/main).

- [ ] **Step 2: Confirm the sidecar venv imports**

Run: `python-sidecar/.venv/bin/python -c "import lmt_vba_sidecar.calibrate, lmt_vba_sidecar.sl_reconstruct, lmt_vba_sidecar.nominal, lmt_vba_sidecar.sl_feasibility; print('ok')"`
Expected: `ok`. If ImportError, fix the venv per memory `reference_worktree_venv_isolation` (independent venv + hand-written `.pth`) BEFORE proceeding.

- [ ] **Step 3: Baseline python tests**

Run: `python-sidecar/.venv/bin/python -m pytest python-sidecar/tests -q`
Expected: all pass. Record the count; this is the regression baseline.

---

## Task 1: `nominal_dot_positions_world` — per-dot nominal 3D world table (+ independent golden tests)

This is the missing geometry piece. **Test FIRST against independent oracles** (Codex F4) so a sign/unit/`R_y` bug can't round-trip through the calibrator's own synthetic data.

**Files:**
- Create: `python-sidecar/tests/test_nominal_dot_positions.py`
- Modify: `python-sidecar/src/lmt_vba_sidecar/nominal.py` (add public fn at end)

- [ ] **Step 1: Write the failing golden tests**

```python
# python-sidecar/tests/test_nominal_dot_positions.py
import math
import numpy as np
import pytest

from lmt_vba_sidecar.ipc import (
    CabinetArray, CabinetRect, CodeSpec, SequenceSpec,
    ShapePriorCurved, ShapePriorCurvedBody, StructuredLightDot, StructuredLightMeta,
)
from lmt_vba_sidecar.nominal import (
    nominal_dot_positions_world,
    nominal_cabinet_centers_model_frame,
    nominal_cabinet_normals_model_frame,
)
from lmt_vba_sidecar.sl_geometry import sl_local_mm


def _meta(cabinets, dots, screen_res=(1080, 540)):
    return StructuredLightMeta(
        schema_version=1, screen_id="MAIN", screen_resolution=list(screen_res),
        dot_radius_px=4,
        code=CodeSpec(data_bits=8, total_bits=9),
        sequence=SequenceSpec(n_code_frames=9, hold_ms=100, fps=30),
        cabinets=cabinets, dots=dots,
    )


def test_flat_dot_is_center_plus_local_offset():
    # One flat cabinet (0,0), 500x500mm, 540x540px → pitch ~0.9259 mm/px.
    cab = CabinetArray(cols=1, rows=1, cabinet_size_mm=[500.0, 500.0])
    rect = CabinetRect(col=0, row=0, input_rect_px=[0, 0, 540, 540], pixel_pitch_mm=[500.0/540, 500.0/540])
    # A dot at the cabinet pixel center (u,v)=(270,270) → local (0,0,0).
    dot = StructuredLightDot(id=0, u=270.0, v=270.0, cabinet=[0, 0])
    meta = _meta([rect], [dot], screen_res=(540, 540))
    world = nominal_dot_positions_world(meta, cab, "flat")
    center = np.array(nominal_cabinet_centers_model_frame(cab, "flat")[(0, 0)])
    assert np.allclose(world[0], center, atol=1e-9)


def test_flat_offset_dot_matches_sl_local_mm_translation():
    cab = CabinetArray(cols=1, rows=1, cabinet_size_mm=[500.0, 500.0])
    rect = CabinetRect(col=0, row=0, input_rect_px=[0, 0, 540, 540], pixel_pitch_mm=[500.0/540, 500.0/540])
    dot = StructuredLightDot(id=7, u=400.0, v=120.0, cabinet=[0, 0])
    meta = _meta([rect], [dot], screen_res=(540, 540))
    world = nominal_dot_positions_world(meta, cab, "flat")
    center = np.array(nominal_cabinet_centers_model_frame(cab, "flat")[(0, 0)])
    local_m = sl_local_mm((0, 0, 540, 540), 400.0, 120.0, 500.0/540, 500.0/540) / 1000.0
    assert np.allclose(world[7], center + local_m, atol=1e-9)


def test_curved_cabinet_dot_centroid_is_cabinet_center():
    # 3 cols curved; the centroid of a cabinet's dots == its nominal center,
    # and the dot-plane normal == the nominal normal (independent oracles).
    cab = CabinetArray(cols=3, rows=1, cabinet_size_mm=[500.0, 500.0])
    shape = ShapePriorCurved(curved=ShapePriorCurvedBody(radius_mm=4000.0))
    rects = [CabinetRect(col=c, row=0, input_rect_px=[c*540, 0, 540, 540], pixel_pitch_mm=[500.0/540, 500.0/540]) for c in range(3)]
    # 4 symmetric dots around each cabinet center → centroid == center.
    dots, did = [], 0
    for c in range(3):
        for (u, v) in [(c*540+135, 135), (c*540+405, 135), (c*540+135, 405), (c*540+405, 405)]:
            dots.append(StructuredLightDot(id=did, u=float(u), v=float(v), cabinet=[c, 0])); did += 1
    meta = _meta(rects, dots, screen_res=(1620, 540))
    world = nominal_dot_positions_world(meta, cab, shape)
    centers = nominal_cabinet_centers_model_frame(cab, shape)
    normals = nominal_cabinet_normals_model_frame(cab, shape)
    for c in range(3):
        ids = [d.id for d in dots if d.cabinet == [c, 0]]
        pts = np.array([world[i] for i in ids])
        assert np.allclose(pts.mean(axis=0), np.array(centers[(c, 0)]), atol=1e-6)
        # Plane normal via SVD: smallest singular vector of centered points.
        u_, s_, vt = np.linalg.svd(pts - pts.mean(axis=0))
        n = vt[-1]
        nominal_n = np.array(normals[(c, 0)])
        assert abs(abs(np.dot(n, nominal_n)) - 1.0) < 1e-6  # parallel (sign-free)
```

- [ ] **Step 2: Run the tests to verify they fail**

Run: `python-sidecar/.venv/bin/python -m pytest python-sidecar/tests/test_nominal_dot_positions.py -q`
Expected: FAIL — `ImportError: cannot import name 'nominal_dot_positions_world'`.

- [ ] **Step 3: Implement the helper**

Append to `python-sidecar/src/lmt_vba_sidecar/nominal.py`:

```python
def _cabinet_R_y_model(col: int, row: int, cab: CabinetArray, shape_prior: Any) -> "np.ndarray":
    """R_world_from_cabinet for this cabinet (rigid tile). Flat ⇒ I; curved ⇒ R_y(α)
    where α is the arc angle of the cabinet center (consistent with
    _cabinet_normal_model: R_y(α)·[0,0,1] = [sin α,0,cos α])."""
    import numpy as np
    if shape_prior == "flat":
        return np.eye(3)
    if _is_curved(shape_prior):
        cw_mm, _ch = cab.cabinet_size_mm
        radius_mm = _curved_radius(shape_prior)
        total_w_mm = cab.cols * cw_mm
        _validate_curved_radius(radius_mm, total_w_mm / 2.0)
        x_mm = (col + 0.5) * cw_mm
        angle = (x_mm - total_w_mm / 2.0) / radius_mm
        c, s = math.cos(angle), math.sin(angle)
        return np.array([[c, 0.0, s], [0.0, 1.0, 0.0], [-s, 0.0, c]])
    if _is_folded(shape_prior):
        raise ValueError("shape_prior=folded is not supported in M2")
    raise ValueError(f"unsupported shape_prior: {shape_prior!r}")


def nominal_dot_positions_world(meta, cab: CabinetArray, shape_prior: Any) -> dict[int, "np.ndarray"]:
    """dot_id → [x,y,z] (meters) in the model/design frame.

    world_m = center_m(col,row) + R_y(α)·(sl_local_mm(rect,u,v,pitch)/1000).
    Flat ⇒ pure translation. Used by Step-1 SL calibration as the known 3D target.
    Raises ValueError (mapped to invalid_input) on unsupported shape or a dot whose
    cabinet is absent / not in meta.cabinets.
    """
    import numpy as np
    from lmt_vba_sidecar.sl_geometry import sl_local_mm

    centers = nominal_cabinet_centers_model_frame(cab, shape_prior)  # present cells only
    rect_by_cr = {(c.col, c.row): tuple(int(v) for v in c.input_rect_px) for c in meta.cabinets}
    pitch_by_cr = {(c.col, c.row): (float(c.pixel_pitch_mm[0]), float(c.pixel_pitch_mm[1])) for c in meta.cabinets}
    R_by_cr = {cr: _cabinet_R_y_model(cr[0], cr[1], cab, shape_prior) for cr in centers.keys()}

    out: dict[int, np.ndarray] = {}
    for d in meta.dots:
        cr = (int(d.cabinet[0]), int(d.cabinet[1]))
        if cr not in centers:
            raise ValueError(f"dot {d.id} references absent/unknown cabinet {cr}")
        if cr not in rect_by_cr:
            raise ValueError(f"dot {d.id} cabinet {cr} not in sl_meta.cabinets")
        local_m = sl_local_mm(rect_by_cr[cr], float(d.u), float(d.v), pitch_by_cr[cr][0], pitch_by_cr[cr][1]) / 1000.0
        out[int(d.id)] = np.asarray(centers[cr]) + R_by_cr[cr] @ local_m
    return out
```

- [ ] **Step 4: Run the tests to verify they pass**

Run: `python-sidecar/.venv/bin/python -m pytest python-sidecar/tests/test_nominal_dot_positions.py -q`
Expected: PASS (3 tests).

- [ ] **Step 5: Commit**

```bash
git add python-sidecar/src/lmt_vba_sidecar/nominal.py python-sidecar/tests/test_nominal_dot_positions.py
git commit -m "feat(sl): nominal_dot_positions_world — per-dot nominal 3D target (+ golden tests)"
```

---

## Task 2: `CalibrateStructuredLightInput` IPC model

**Files:**
- Modify: `python-sidecar/src/lmt_vba_sidecar/ipc.py` (add after `ReconstructStructuredLightInput`, ~line 131)
- Create: `python-sidecar/tests/test_calibrate_sl_ipc.py`

- [ ] **Step 1: Write the failing test**

```python
# python-sidecar/tests/test_calibrate_sl_ipc.py
import pytest
from pydantic import ValidationError
from lmt_vba_sidecar.ipc import CalibrateStructuredLightInput, ReconstructProject, CabinetArray


def _project():
    return ReconstructProject(screen_id="MAIN", cabinet_array=CabinetArray(cols=2, rows=1, cabinet_size_mm=[500.0, 500.0]), shape_prior="flat")


def test_valid_input_parses_with_default_max_rms():
    m = CalibrateStructuredLightInput.model_validate({
        "command": "calibrate_structured_light", "version": 1,
        "project": _project().model_dump(),
        "correspondence_paths": ["a.json"], "sl_meta_path": "m.json", "output_path": "o.json",
    })
    assert m.max_rms_px == 1.5
    assert len(m.correspondence_paths) == 1


def test_zero_correspondences_rejected():
    with pytest.raises(ValidationError):
        CalibrateStructuredLightInput.model_validate({
            "command": "calibrate_structured_light", "version": 1,
            "project": _project().model_dump(),
            "correspondence_paths": [], "sl_meta_path": "m.json", "output_path": "o.json",
        })
```

- [ ] **Step 2: Run to verify it fails**

Run: `python-sidecar/.venv/bin/python -m pytest python-sidecar/tests/test_calibrate_sl_ipc.py -q`
Expected: FAIL — `ImportError: cannot import name 'CalibrateStructuredLightInput'`.

- [ ] **Step 3: Add the model**

In `python-sidecar/src/lmt_vba_sidecar/ipc.py`, right after the `ReconstructStructuredLightInput` class (mirrors it, drops `intrinsics_path`, adds `output_path` + `max_rms_px`, allows ≥1 corr):

```python
class CalibrateStructuredLightInput(BaseModel):
    command: Literal["calibrate_structured_light"]
    version: Literal[1]
    project: ReconstructProject
    # One CorrespondenceFile per camera pose of ONE camera (decode_structured_light output).
    correspondence_paths: Annotated[list[str], Field(min_length=1)]
    sl_meta_path: str
    # Where the intrinsics JSON is written (NON-destructive default chosen by lmt-app).
    output_path: str
    # reproj RMS gate (px). Looser than checkerboard's 0.5 — SL centroids are noisier.
    max_rms_px: float = Field(default=1.5, gt=0.0)
```

- [ ] **Step 4: Run to verify it passes**

Run: `python-sidecar/.venv/bin/python -m pytest python-sidecar/tests/test_calibrate_sl_ipc.py -q`
Expected: PASS (2 tests).

- [ ] **Step 5: Commit**

```bash
git add python-sidecar/src/lmt_vba_sidecar/ipc.py python-sidecar/tests/test_calibrate_sl_ipc.py
git commit -m "feat(sl): CalibrateStructuredLightInput ipc model"
```

---

## Task 3: `calibrate_sl.py` solver + observability gates + dispatch registration

The core. TDD against a synthetic substrate (reuses the *independent* `project_point`/`look_at_pose` + the now-pinned `nominal_dot_positions_world`).

**Files:**
- Create: `python-sidecar/src/lmt_vba_sidecar/calibrate_sl.py`
- Modify: `python-sidecar/src/lmt_vba_sidecar/__main__.py` (3 places: subparser, SUBCOMMAND_MODULES, SUBCOMMAND_ENTRYPOINTS + import)
- Create: `python-sidecar/tests/test_calibrate_sl.py`

- [ ] **Step 1: Read calibrate.py end-to-end to copy its event imports + success tail**

Run: `sed -n '1,40p;215,235p' python-sidecar/src/lmt_vba_sidecar/calibrate.py`
Note the exact: (a) imports of `write_event` / `ProgressEvent` / `ErrorEvent` / the result event type and `_atomic_write`; (b) the success tail after `_atomic_write` — it MUST emit a `result` event (else `run_sidecar` returns `NoResultEvent`) and `return 0`. Mirror these exactly in calibrate_sl.py (the adapter ignores the result payload but the event must exist).

- [ ] **Step 2: Write the synthetic-substrate test file (failing)**

```python
# python-sidecar/tests/test_calibrate_sl.py
import json
import numpy as np
import pytest

from lmt_vba_sidecar.ipc import (
    CabinetArray, CabinetRect, CodeSpec, SequenceSpec, ReconstructProject,
    ShapePriorCurved, ShapePriorCurvedBody, StructuredLightDot, StructuredLightMeta,
    CalibrateStructuredLightInput,
)
from lmt_vba_sidecar.nominal import nominal_dot_positions_world
from lmt_vba_sidecar.sl_feasibility import look_at_pose, project_point
from lmt_vba_sidecar.calibrate_sl import run_calibrate_structured_light

K_TRUE = np.array([[3000.0, 0.0, 2000.0], [0.0, 3000.0, 1500.0], [0.0, 0.0, 1.0]])
IMG = (4000, 3000)


def _curved_meta(cols=4, radius_mm=4000.0, grid=4):
    cab = CabinetArray(cols=cols, rows=1, cabinet_size_mm=[500.0, 500.0])
    shape = ShapePriorCurved(curved=ShapePriorCurvedBody(radius_mm=radius_mm))
    rects, dots, did = [], [], 0
    px = 540
    for c in range(cols):
        rects.append(CabinetRect(col=c, row=0, input_rect_px=[c*px, 0, px, px], pixel_pitch_mm=[500.0/px, 500.0/px]))
        for i in range(grid):
            for j in range(grid):
                u = c*px + (i + 0.5) * px / grid
                v = (j + 0.5) * px / grid
                dots.append(StructuredLightDot(id=did, u=float(u), v=float(v), cabinet=[c, 0])); did += 1
    meta = StructuredLightMeta(
        schema_version=1, screen_id="MAIN", screen_resolution=[cols*px, px], dot_radius_px=4,
        code=CodeSpec(data_bits=8, total_bits=9), sequence=SequenceSpec(n_code_frames=9, hold_ms=100, fps=30),
        cabinets=rects, dots=dots,
    )
    proj = ReconstructProject(screen_id="MAIN", cabinet_array=cab, shape_prior=shape)
    return meta, proj, cab, shape


def _write_corr(tmp, meta, world, poses, sha="sha-test", noise=0.0, seed=0):
    rng = np.random.default_rng(seed)
    paths = []
    for vi, (R, t) in enumerate(poses):
        pts = []
        for d in meta.dots:
            p = project_point(K_TRUE, R, t, world[d.id]) + rng.normal(0, noise, 2)
            pts.append({"id": d.id, "u": d.u, "v": d.v, "x": float(p[0]), "y": float(p[1])})
        cp = tmp / f"corr_{vi}.json"
        cp.write_text(json.dumps({
            "schema_version": 1, "screen_id": "MAIN", "sl_meta_sha256": sha,
            "screen_resolution": meta.screen_resolution, "camera_image_size": list(IMG),
            "source_input": f"/cap/pose{vi}.mp4", "points": pts,
        }))
        paths.append(str(cp))
    return paths


def _ring_poses(n=4, dist_m=6.0):
    # Cameras on a shallow arc in front of the wall (meters; world is meters).
    poses = []
    for k in range(n):
        x = -1.0 + 2.0 * k / max(1, n - 1)
        poses.append(look_at_pose(np.array([x, 0.0, -dist_m]), np.array([1.0, 0.0, 0.0])))
    return poses


def _run(tmp, meta, proj, paths, monkeypatch):
    # sl_meta on disk; sha must match what the corr files claim.
    import hashlib
    meta_path = tmp / "sl_meta.json"
    meta_path.write_text(meta.model_dump_json())
    sha = hashlib.sha256(meta_path.read_bytes()).hexdigest()
    # rewrite corr sha to the real file hash
    for p in paths:
        d = json.loads(open(p).read()); d["sl_meta_sha256"] = sha; open(p, "w").write(json.dumps(d))
    out = tmp / "sl_intrinsics.json"
    cmd = CalibrateStructuredLightInput(
        command="calibrate_structured_light", version=1, project=proj,
        correspondence_paths=paths, sl_meta_path=str(meta_path), output_path=str(out),
    )
    rc = run_calibrate_structured_light(cmd)
    return rc, out


def test_recovers_K_noise_free(tmp_path, monkeypatch):
    meta, proj, cab, shape = _curved_meta()
    world = nominal_dot_positions_world(meta, cab, shape)
    paths = _write_corr(tmp_path, meta, world, _ring_poses(4), noise=0.0)
    rc, out = _run(tmp_path, meta, proj, paths, monkeypatch)
    assert rc == 0
    intr = json.loads(out.read_text())
    K = np.array(intr["K"])
    assert abs(K[0, 0] - 3000.0) / 3000.0 < 0.01    # focal <1%
    assert abs(K[0, 2] - 2000.0) < 1.5              # principal point ~1px
    assert abs(K[1, 2] - 1500.0) < 1.5
    assert intr["calibration_method"] == "structured_light_nominal"
    assert intr["frames_used"] == 4


def test_recovers_K_with_noise_within_budget(tmp_path, monkeypatch):
    meta, proj, cab, shape = _curved_meta()
    world = nominal_dot_positions_world(meta, cab, shape)
    paths = _write_corr(tmp_path, meta, world, _ring_poses(4), noise=0.3)
    rc, out = _run(tmp_path, meta, proj, paths, monkeypatch)
    assert rc == 0
    K = np.array(json.loads(out.read_text())["K"])
    assert abs(K[0, 0] - 3000.0) / 3000.0 < 0.02    # focal <2% budget


def test_structured_deviation_within_budget_or_refused(tmp_path, monkeypatch):
    # Calibrate against NOMINAL but project a STRUCTURALLY deviated truth
    # (as-built radius +2%). Either recovered K stays in budget, OR we refuse.
    meta, proj, cab, shape = _curved_meta(radius_mm=4000.0)
    nominal_world = nominal_dot_positions_world(meta, cab, shape)  # calibration target
    dev_shape = ShapePriorCurved(curved=ShapePriorCurvedBody(radius_mm=4080.0))  # +2% as-built
    truth_world = nominal_dot_positions_world(meta, cab, dev_shape)              # what camera sees
    paths = _write_corr(tmp_path, meta, truth_world, _ring_poses(4), noise=0.3)
    rc, out = _run(tmp_path, meta, proj, paths, monkeypatch)
    if rc == 0:
        K = np.array(json.loads(out.read_text())["K"])
        assert abs(K[0, 0] - 3000.0) / 3000.0 < 0.02, "absorbed deviation blew the focal budget without refusing"
    else:
        assert rc == 1  # refused (observability/quality gate) — acceptable


def test_near_flat_single_pose_refused(tmp_path, monkeypatch):
    # One flat cabinet, one pose → planar + single view = degenerate.
    cab = CabinetArray(cols=1, rows=1, cabinet_size_mm=[500.0, 500.0])
    px = 540
    rect = CabinetRect(col=0, row=0, input_rect_px=[0, 0, px, px], pixel_pitch_mm=[500.0/px, 500.0/px])
    dots, did = [], 0
    for i in range(6):
        for j in range(6):
            dots.append(StructuredLightDot(id=did, u=(i+0.5)*px/6, v=(j+0.5)*px/6, cabinet=[0, 0])); did += 1
    meta = StructuredLightMeta(schema_version=1, screen_id="MAIN", screen_resolution=[px, px], dot_radius_px=4,
        code=CodeSpec(data_bits=8, total_bits=9), sequence=SequenceSpec(n_code_frames=9, hold_ms=100, fps=30),
        cabinets=[rect], dots=dots)
    proj = ReconstructProject(screen_id="MAIN", cabinet_array=cab, shape_prior="flat")
    world = nominal_dot_positions_world(meta, cab, "flat")
    paths = _write_corr(tmp_path, meta, world, _ring_poses(1), noise=0.0)
    rc, _ = _run(tmp_path, meta, proj, paths, monkeypatch)
    assert rc == 1  # observability_failed


def test_near_duplicate_poses_refused(tmp_path, monkeypatch):
    meta, proj, cab, shape = _curved_meta()
    world = nominal_dot_positions_world(meta, cab, shape)
    # 3 nearly identical viewpoints (baseline collapse).
    dup = [look_at_pose(np.array([0.0 + 1e-3*k, 0.0, -6.0]), np.array([1.0, 0.0, 0.0])) for k in range(3)]
    paths = _write_corr(tmp_path, meta, world, dup, noise=0.1)
    rc, _ = _run(tmp_path, meta, proj, paths, monkeypatch)
    assert rc == 1  # observability_failed (pose/baseline diversity)
```

- [ ] **Step 3: Run to verify the tests fail**

Run: `python-sidecar/.venv/bin/python -m pytest python-sidecar/tests/test_calibrate_sl.py -q`
Expected: FAIL — `ModuleNotFoundError: lmt_vba_sidecar.calibrate_sl`.

- [ ] **Step 4: Implement `calibrate_sl.py`**

Create `python-sidecar/src/lmt_vba_sidecar/calibrate_sl.py`. Replace the `# --- emit result + return 0 ---` placeholder with the exact success-tail you copied from calibrate.py in Step 1 (the result-event emit). Everything else is complete:

```python
"""Step 1: calibrate camera intrinsics from structured-light white dots vs the
nominal design wall (a known 3D target). Solves fx,fy,cx,cy,k1,k2 with
cv2.calibrateCameraExtended and REFUSES on degenerate observability instead of
emitting a confidently-wrong K (spec 2026-05-30-sl-camera-calibration-design)."""
from __future__ import annotations

import hashlib
import json
import pathlib

import cv2
import numpy as np

from lmt_vba_sidecar.ipc import (
    CalibrateStructuredLightInput, CorrespondenceFile, StructuredLightMeta,
)
from lmt_vba_sidecar.nominal import nominal_dot_positions_world
from lmt_vba_sidecar.sl_reconstruct import validate_sl_provenance
from lmt_vba_sidecar.calibrate import (
    _atomic_write, FOCAL_BOUNDS_FRACTION,
    write_event, ProgressEvent, ErrorEvent,   # adjust imports to match calibrate.py Step 1
)

# Observability gate constants (spec §8 starting values).
COVERAGE_MIN_FRAC = 0.40
COPLANAR_RATIO_MIN = 1e-3
POSE_ROT_DIVERSITY_DEG = 5.0
PP_STDDEV_MAX_PX = 3.0
FOCAL_STDDEV_MAX_FRAC = 0.01
MIN_DOTS_PER_POSE = 4


def _err(code: str, msg: str) -> int:
    write_event(ErrorEvent(event="error", code=code, message=msg, fatal=True))
    return 1


def _coplanarity_ratio(pts: np.ndarray) -> float:
    """σ_min/σ_max of the centered 3D cloud: ~0 ⇒ coplanar, →1 ⇒ volumetric."""
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
    return float(w * h)


def run_calibrate_structured_light(cmd: CalibrateStructuredLightInput) -> int:
    # 1. sl_meta + provenance sha
    meta_path = pathlib.Path(cmd.sl_meta_path)
    try:
        meta = StructuredLightMeta.model_validate_json(meta_path.read_text())
    except (OSError, ValueError) as e:
        return _err("invalid_input", f"sl_meta unreadable: {e}")
    expected_sha = hashlib.sha256(meta_path.read_bytes()).hexdigest()

    # 2. correspondence files + provenance gate (reused from sl_reconstruct)
    corr_files: list[CorrespondenceFile] = []
    for p in cmd.correspondence_paths:
        try:
            corr_files.append(CorrespondenceFile.model_validate_json(pathlib.Path(p).read_text()))
        except (OSError, ValueError) as e:
            return _err("invalid_input", f"correspondence '{p}' unreadable: {e}")
    try:
        validate_sl_provenance(corr_files, expected_sha=expected_sha, expected_screen_id=cmd.project.screen_id)
    except ValueError as e:
        return _err("invalid_input", str(e))

    # 3. same-camera precondition: one camera_image_size across all poses
    sizes = {tuple(int(v) for v in c.camera_image_size) for c in corr_files}
    if len(sizes) != 1:
        return _err("invalid_input", f"correspondences disagree on camera_image_size: {sorted(sizes)}")
    (image_size,) = sizes

    # 4. per-dot nominal 3D world (known target). keys() == project present cells.
    try:
        dot_world = nominal_dot_positions_world(meta, cmd.project.cabinet_array, cmd.project.shape_prior)
    except ValueError as e:
        return _err("invalid_input", str(e))

    # 5. assemble per-pose object/image points (canonical (u,v) is implicit via dot id)
    write_event(ProgressEvent(event="progress", stage="subpixel_refine", percent=0.3, message="assembling observations"))
    object_points, image_points = [], []
    for cf in corr_files:
        objp, imgp = [], []
        for pt in cf.points:
            X = dot_world.get(int(pt.id))
            if X is None:
                continue
            objp.append(X)
            imgp.append([pt.x, pt.y])
        if len(objp) >= MIN_DOTS_PER_POSE:
            object_points.append(np.asarray(objp, dtype=np.float32))
            image_points.append(np.asarray(imgp, dtype=np.float32))
    if len(object_points) < 1:
        return _err("observability_failed", f"no pose has >= {MIN_DOTS_PER_POSE} dots mapping to nominal")

    # 6. coplanarity OR >=3 poses gate (planar-PoC degeneracy)
    all_obj = np.concatenate(object_points, axis=0)
    ratio = _coplanarity_ratio(all_obj)
    if ratio < COPLANAR_RATIO_MIN and len(object_points) < 3:
        return _err("observability_failed",
                    f"near-coplanar target (ratio={ratio:.2e}) with only {len(object_points)} pose(s)")

    # 7. coverage gate
    cover = _coverage_frac(image_points, image_size)
    if cover < COVERAGE_MIN_FRAC:
        return _err("observability_failed", f"image coverage {cover:.2f} < {COVERAGE_MIN_FRAC}")

    # 8. solve (use intrinsic guess; radial k1,k2 only)
    write_event(ProgressEvent(event="progress", stage="bundle_adjustment", percent=0.7, message="solving intrinsics"))
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
        return _err("intrinsics_invalid", f"calibrateCamera failed: {e}")

    # 9. pose/baseline diversity gate (count != observability)
    if len(rvecs) >= 2 and _max_pairwise_rot_deg(rvecs) < POSE_ROT_DIVERSITY_DEG:
        return _err("observability_failed",
                    f"pose rotation diversity < {POSE_ROT_DIVERSITY_DEG}° (near-duplicate captures)")

    # 10. quality + parameter-observability gates
    if not (np.isfinite(K).all() and np.isfinite(dist).all() and np.isfinite(rms)):
        return _err("intrinsics_invalid", f"calibration produced non-finite values (rms={rms})")
    fx, fy, cx, cy = float(K[0, 0]), float(K[1, 1]), float(K[0, 2]), float(K[1, 2])
    f_lo, f_hi = FOCAL_BOUNDS_FRACTION
    if not (f_lo * long_dim < fx < f_hi * long_dim) or not (f_lo * long_dim < fy < f_hi * long_dim):
        return _err("intrinsics_invalid", f"focal ({fx:.1f},{fy:.1f}) outside plausible range for {image_size}")
    if not (0 < cx < image_size[0]) or not (0 < cy < image_size[1]):
        return _err("intrinsics_invalid", f"principal point ({cx:.1f},{cy:.1f}) outside image {image_size}")
    if rms > cmd.max_rms_px:
        return _err("intrinsics_invalid", f"reproj RMS {rms:.2f}px exceeds gate {cmd.max_rms_px}px")
    std = np.asarray(std_int).flatten()
    pp_std = (float(std[2]), float(std[3]))
    foc_std = (float(std[0]), float(std[1]))
    if max(pp_std) > PP_STDDEV_MAX_PX:
        return _err("observability_failed", f"principal-point std {pp_std} px > {PP_STDDEV_MAX_PX} (under-constrained)")
    if max(foc_std) > FOCAL_STDDEV_MAX_FRAC * fx:
        return _err("observability_failed", f"focal std {foc_std} px > {FOCAL_STDDEV_MAX_FRAC*100:.0f}% of focal")

    # 11. write intrinsics (5-key contract + provenance)
    payload = json.dumps({
        "K": K.tolist(),
        "dist_coeffs": dist.flatten().tolist(),
        "image_size": list(image_size),
        "reproj_error_px": float(rms),
        "frames_used": len(object_points),
        "calibration_method": "structured_light_nominal",
        "pp_stddev_px": list(pp_std),
        "focal_stddev_px": list(foc_std),
        "n_poses": len(object_points),
    }, indent=2)
    _atomic_write(pathlib.Path(cmd.output_path), payload)

    # --- emit result + return 0 ---  (copy calibrate.py's exact success tail here)
    return 0
```

- [ ] **Step 5: Register the command in `__main__.py`**

Add the import near the other input-model imports, then add the three entries (mirror the `reconstruct_structured_light` lines):

```python
# imports: add CalibrateStructuredLightInput to the ipc import list

# in the argparse block (after reconstruct_structured_light):
    sub.add_parser("calibrate_structured_light")

# in SUBCOMMAND_MODULES:
        "calibrate_structured_light": "lmt_vba_sidecar.calibrate_sl",

# in SUBCOMMAND_ENTRYPOINTS:
        "calibrate_structured_light": ("run_calibrate_structured_light", CalibrateStructuredLightInput),
```

- [ ] **Step 6: Run the solver tests until green**

Run: `python-sidecar/.venv/bin/python -m pytest python-sidecar/tests/test_calibrate_sl.py -q`
Expected: PASS (5 tests). If `test_recovers_K_*` misses tolerance, check unit consistency (world is meters; `_ring_poses` is meters) and that the result-event tail returns 0. If a refusal test passes K instead of refusing, tighten the named gate and pin the threshold in spec §8.

- [ ] **Step 7: Run the full sidecar suite (no regressions)**

Run: `python-sidecar/.venv/bin/python -m pytest python-sidecar/tests -q`
Expected: baseline count + new tests, all pass.

- [ ] **Step 8: Commit**

```bash
git add python-sidecar/src/lmt_vba_sidecar/calibrate_sl.py python-sidecar/src/lmt_vba_sidecar/__main__.py python-sidecar/tests/test_calibrate_sl.py
git commit -m "feat(sl): calibrate_sl solver + observability gates + dispatch"
```

---

## Task 4: Adapter `calibrate_structured_light` async fn

**Files:**
- Modify: `crates/adapter-visual-ba/src/api.rs` (add `CalibrateStructuredLightArgs` + the async fn near `calibrate`)

- [ ] **Step 1: Add the args struct + async fn**

Mirror `calibrate` (reads back `{reproj_error_px, frames_used}` from the written file) but build the SL payload (mirrors `reconstruct_structured_light`'s payload shape). Reuses `ReconstructProject`, `CalibrateOut`, the local `IntrinsicsFile` readback struct, `run_sidecar`, `SidecarRequest`, `validate_project_eagerly`.

```rust
pub struct CalibrateStructuredLightArgs {
    pub project: ReconstructProject,
    pub correspondence_paths: Vec<String>,
    pub sl_meta_path: String,
    pub output_path: String,
    pub max_rms_px: f64,
    pub progress_tx: Option<mpsc::Sender<Event>>,
    pub cancel: Option<oneshot::Receiver<()>>,
}

pub async fn calibrate_structured_light(args: CalibrateStructuredLightArgs) -> VbaResult<CalibrateOut> {
    validate_project_eagerly(&args.project)?;

    let payload = json!({
        "command": "calibrate_structured_light",
        "version": 1,
        "project": &args.project,
        "correspondence_paths": &args.correspondence_paths,
        "sl_meta_path": &args.sl_meta_path,
        "output_path": &args.output_path,
        "max_rms_px": args.max_rms_px,
    });

    let _value = run_sidecar(SidecarRequest {
        subcommand: "calibrate_structured_light".into(),
        payload,
        progress_tx: args.progress_tx,
        cancel: args.cancel,
    })
    .await?;

    // Read authoritative reproj_error_px + frames_used from the intrinsics JSON
    // the sidecar wrote (same pattern as `calibrate`).
    #[derive(serde::Deserialize)]
    struct IntrinsicsFile {
        reproj_error_px: f64,
        frames_used: u32,
    }
    let intr: IntrinsicsFile = serde_json::from_str(
        &std::fs::read_to_string(&args.output_path)
            .map_err(|e| VbaError::InvalidInput(format!("intrinsics file unreadable: {e}")))?,
    )
    .map_err(|e| VbaError::InvalidInput(format!("intrinsics file decode failed: {e}")))?;

    Ok(CalibrateOut {
        intrinsics_path: args.output_path,
        reproj_error_px: intr.reproj_error_px,
        frames_used: intr.frames_used,
    })
}
```

- [ ] **Step 2: Verify it compiles**

Run: `cargo build -p lmt-adapter-visual-ba`
Expected: compiles. (Adapter behavior is exercised by the Task 8 E2E happy test against the real sidecar.)

- [ ] **Step 3: Commit**

```bash
git add crates/adapter-visual-ba/src/api.rs
git commit -m "feat(sl): adapter calibrate_structured_light async fn"
```

---

## Task 5: lmt-app `run_calibrate_structured_light` (out-path + force guard)

**Files:**
- Modify: `crates/lmt-app/src/visual.rs` (add fn near `run_reconstruct_structured_light`; add `calibrate_structured_light` + `CalibrateStructuredLightArgs` to the adapter `use`)

- [ ] **Step 1: Add the service fn**

Mirrors `run_reconstruct_structured_light` (project build via `load_project_yaml_from_path`/`load_screen`/`ipc_cabinet_array`/`ipc_shape_prior`) + `run_calibrate`'s output-path resolution, with the non-destructive guard:

```rust
#[allow(clippy::too_many_arguments)]
pub fn run_calibrate_structured_light(
    project_path: &Path,
    screen_id: &str,
    sl_meta: &Path,
    correspondences: &[String],
    out: Option<&Path>,
    force: bool,
    max_rms_px: f64,
) -> LmtResult<CalibrateResult> {
    let cfg = load_project_yaml_from_path(project_path)?;
    let screen_cfg = load_screen(&cfg, screen_id)?;
    let project = ipc::ReconstructProject {
        screen_id: screen_id.to_string(),
        cabinet_array: ipc_cabinet_array(screen_cfg),
        shape_prior: ipc_shape_prior(screen_cfg),
    };

    let calibration_dir = project_path.join("calibration");
    std::fs::create_dir_all(&calibration_dir)?;
    let output_path = match out {
        Some(p) => p.to_path_buf(),
        None => calibration_dir.join(format!("{screen_id}_sl_intrinsics.json")),
    };
    if output_path.exists() && !force {
        return Err(LmtError::InvalidInput(format!(
            "would overwrite existing intrinsics {}; pass --force or --out",
            output_path.display()
        )));
    }

    let args = CalibrateStructuredLightArgs {
        project,
        correspondence_paths: correspondences.to_vec(),
        sl_meta_path: sl_meta.display().to_string(),
        output_path: output_path.display().to_string(),
        max_rms_px,
        progress_tx: None,
        cancel: None,
    };

    let out = rt()?
        .block_on(calibrate_structured_light(args))
        .map_err(map_vba_err)?;

    Ok(CalibrateResult {
        intrinsics_path: out.intrinsics_path,
        reproj_error_px: out.reproj_error_px,
        frames_used: out.frames_used,
    })
}
```

- [ ] **Step 2: Verify it compiles**

Run: `cargo build -p lmt-app`
Expected: compiles. Fix the adapter `use` line to include `calibrate_structured_light, CalibrateStructuredLightArgs` if the build complains.

- [ ] **Step 3: Commit**

```bash
git add crates/lmt-app/src/visual.rs
git commit -m "feat(sl): lmt-app run_calibrate_structured_light (force/out guard)"
```

---

## Task 6: CLI clap variant + transport handler + dispatch

**Files:**
- Modify: `crates/lmt-cli/src/cli.rs` (add `CalibrateStructuredLight` variant after `ReconstructStructuredLight`)
- Modify: `crates/lmt-cli/src/commands/visual.rs` (add dispatch arm in `run` + the handler fn)

- [ ] **Step 1: Add the clap variant** (cli.rs, after the `ReconstructStructuredLight` variant)

```rust
    /// 结构光白点 + nominal 设计墙(3D 靶) → <screen_id>_sl_intrinsics.json
    /// (cv2.calibrateCamera,病态拒标)。side_effect: destructive
    #[command(name = "calibrate-structured-light")]
    CalibrateStructuredLight {
        /// 项目根目录。
        project_path: String,
        /// screen id。
        screen_id: String,
        /// sl_meta.json 路径(generate-structured-light 产出)。
        #[arg(long)]
        sl_meta: String,
        /// 同一台相机每个机位一个 corr.json(decode-structured-light 产出);重复传入。
        #[arg(long = "corr", required = true, num_args = 1.., action = clap::ArgAction::Append)]
        correspondences: Vec<String>,
        /// 内参输出路径(默认 <project>/calibration/<screen_id>_sl_intrinsics.json)。
        #[arg(long)]
        out: Option<String>,
        /// 覆盖已存在的内参文件(否则拒绝,以免覆盖可信棋盘格标定)。
        #[arg(long)]
        force: bool,
        /// reproj RMS 门槛(px)。
        #[arg(long = "max-rms-px", default_value_t = 1.5)]
        max_rms_px: f64,
    },
```

- [ ] **Step 2: Add the dispatch arm** (visual.rs `run`, after the `ReconstructStructuredLight` arm)

```rust
        VisualCmd::CalibrateStructuredLight {
            project_path,
            screen_id,
            sl_meta,
            correspondences,
            out,
            force,
            max_rms_px,
        } => calibrate_structured_light(
            mode, &project_path, &screen_id, &sl_meta, &correspondences,
            out.as_deref(), force, max_rms_px, yes, dry_run,
        ),
```

- [ ] **Step 3: Add the handler fn** (visual.rs, near `calibrate`/`reconstruct_structured_light`)

```rust
#[allow(clippy::too_many_arguments)]
fn calibrate_structured_light(
    mode: Mode,
    project_path: &str,
    screen_id: &str,
    sl_meta: &str,
    correspondences: &[String],
    out: Option<&str>,
    force: bool,
    max_rms_px: f64,
    yes: bool,
    dry_run: bool,
) -> i32 {
    let decision = match util::gate_destructive(yes, dry_run, "visual calibrate-structured-light") {
        Ok(d) => d,
        Err(e) => return output::err(mode, e),
    };

    let out_path = out
        .map(str::to_string)
        .unwrap_or_else(|| format!("{project_path}/calibration/{screen_id}_sl_intrinsics.json"));

    match decision {
        DestructiveDecision::DryRun => {
            let payload = serde_json::json!({
                "dry_run": true,
                "would_write": out_path,
                "sl_meta": sl_meta,
                "correspondences": correspondences,
                "force": force,
                "max_rms_px": max_rms_px,
            });
            output::ok(mode, payload, |_| {
                let _ = writeln!(
                    std::io::stdout(),
                    "[dry-run] would calibrate screen {screen_id} from {} poses → {out_path}",
                    correspondences.len()
                );
            })
        }
        DestructiveDecision::Execute => {
            match lmt_app::visual::run_calibrate_structured_light(
                Path::new(project_path),
                screen_id,
                Path::new(sl_meta),
                correspondences,
                out.map(Path::new),
                force,
                max_rms_px,
            ) {
                Ok(r) => output::ok(mode, r, |p| {
                    let _ = writeln!(
                        std::io::stdout(),
                        "calibrated (SL): reproj={:.3}px frames={} → {}",
                        p.reproj_error_px, p.frames_used, p.intrinsics_path
                    );
                }),
                Err(e) => output::err(mode, ApiError::from(e)),
            }
        }
    }
}
```

- [ ] **Step 4: Verify build + the subcommand is registered**

Run: `cargo build -p lmt-cli && ./target/debug/lmt visual calibrate-structured-light --help`
Expected: help text lists `--sl-meta`, `--corr`, `--out`, `--force`, `--max-rms-px`.

- [ ] **Step 5: Commit**

```bash
git add crates/lmt-cli/src/cli.rs crates/lmt-cli/src/commands/visual.rs
git commit -m "feat(sl): calibrate-structured-light CLI subcommand + transport"
```

---

## Task 7: Tauri shim

**Files:**
- Modify: `src-tauri/src/commands/visual.rs` (or wherever the visual `#[tauri::command]`s live — grep `run_reconstruct_structured_light` in `src-tauri/`), add a thin command mirroring the existing SL command. Register it in `src-tauri/src/lib.rs`'s `invoke_handler`.

- [ ] **Step 1: Find the existing SL Tauri command to mirror**

Run: `grep -rn "run_reconstruct_structured_light\|run_calibrate" src-tauri/src`
Expected: locate the existing `#[tauri::command]` wrappers. (If reconstruct-structured-light has NO Tauri command — only CLI — then SL calibration is CLI-only too: skip the shim, and instead document it under "Not exposed in GUI" per the CLAUDE.md contract. Verify which case applies before writing code.)

- [ ] **Step 2: Add the thin command (only if the sibling has one)**

Mirror the sibling exactly — transport translation only (resolve `app_data_dir`/paths, call `lmt_app::visual::run_calibrate_structured_light`, return the DTO). No business logic. Register in the `invoke_handler![...]` list in `src-tauri/src/lib.rs`.

- [ ] **Step 3: Verify build**

Run: `cargo build -p <tauri-crate-name>` (the package under src-tauri)
Expected: compiles.

- [ ] **Step 4: Commit**

```bash
git add src-tauri/
git commit -m "feat(sl): tauri shim for calibrate-structured-light (or document CLI-only)"
```

---

## Task 8: CLI E2E tests (happy / refuse / dry-run / envelope)

**Files:**
- Modify: `crates/lmt-cli/tests/cli_e2e.rs` (add a calibration corr generator + 4 tests)

- [ ] **Step 1: Add the synthetic calibration-corr generator** (mirror `SL_CORR_GEN_PY`)

This projects each dot's nominal 3D world through a known K + 4-pose ring with light noise — no outlier (we want clean K recovery). Add near `SL_CORR_GEN_PY`:

```rust
/// Project nominal dot 3D (curved wall) through a known K from 4 poses → 4 corr
/// files. argv: meta_path intr_path out_dir. Prints the JSON path array.
const SL_CALIB_CORR_GEN_PY: &str = r#"
import json, hashlib, sys
import numpy as np
from lmt_vba_sidecar.ipc import StructuredLightMeta, CabinetArray, ShapePriorCurved, ShapePriorCurvedBody
from lmt_vba_sidecar.nominal import nominal_dot_positions_world
from lmt_vba_sidecar.sl_feasibility import look_at_pose, project_point

meta_path, intr_path, out_dir = sys.argv[1], sys.argv[2], sys.argv[3]
meta = StructuredLightMeta.model_validate_json(open(meta_path).read())
K = np.array(json.loads(open(intr_path).read())["K"], float)
# Project geometry must match the test's project.yaml: 4 cols curved r=4000mm.
cab = CabinetArray(cols=4, rows=1, cabinet_size_mm=[500.0, 500.0])
shape = ShapePriorCurved(curved=ShapePriorCurvedBody(radius_mm=4000.0))
world = nominal_dot_positions_world(meta, cab, shape)
sha = hashlib.sha256(open(meta_path, "rb").read()).hexdigest()
poses = [look_at_pose(np.array([x, 0.0, -6.0]), np.array([1.0, 0.0, 0.0]))
         for x in (-1.0, -0.33, 0.33, 1.0)]
rng = np.random.default_rng(0)
paths = []
for vi, (R, t) in enumerate(poses):
    pts = []
    for d in meta.dots:
        p = project_point(K, R, t, world[d.id]) + rng.normal(0, 0.2, 2)
        pts.append({"id": d.id, "u": d.u, "v": d.v, "x": float(p[0]), "y": float(p[1])})
    cp = f"{out_dir}/ccorr_{vi}.json"
    open(cp, "w").write(json.dumps({
        "schema_version": 1, "screen_id": "MAIN", "sl_meta_sha256": sha,
        "screen_resolution": meta.screen_resolution, "camera_image_size": [4000, 3000],
        "source_input": f"/cap/pose{vi}.mp4", "points": pts}))
    paths.append(cp)
print(json.dumps(paths))
"#;
```

- [ ] **Step 2: Add a curved-project writer + corr-gen helper**

```rust
fn write_curved_project(dir: &Path, cols: u32) {
    let yaml = format!(
        "project: {{ name: GP, unit: mm }}\nscreens:\n  MAIN:\n    cabinet_count: [{cols}, 1]\n    cabinet_size_mm: [500, 500]\n    pixels_per_cabinet: [540, 540]\n    shape_prior: {{ type: curved, radius_mm: 4000 }}\n    shape_mode: rectangle\n    irregular_mask: []\ncoordinate_system:\n  origin_point: MAIN_V000_R000\n  x_axis_point: MAIN_V000_R000\n  xy_plane_point: MAIN_V000_R000\noutput:\n  target: neutral\n  obj_filename: \\\"{{screen_id}}.obj\\\"\n  weld_vertices_tolerance_mm: 1.0\n  triangulate: true\n"
    );
    std::fs::create_dir_all(dir).unwrap();
    std::fs::write(dir.join("project.yaml"), yaml).unwrap();
}

fn write_sl_calib_corr(dir: &Path, sidecar: &str, meta_path: &Path, intr: &Path) -> Vec<String> {
    let python = Path::new(sidecar).parent().expect("sidecar parent").join("python");
    let script = dir.join("gen_ccorr.py");
    std::fs::write(&script, SL_CALIB_CORR_GEN_PY).unwrap();
    let out = Command::new(&python).arg(&script).arg(meta_path).arg(intr).arg(dir)
        .output().expect("run calib corr generator");
    assert!(out.status.success(), "calib corr gen failed: {}", String::from_utf8_lossy(&out.stderr));
    serde_json::from_slice(out.stdout.trim_ascii_end()).expect("JSON path array")
}
```

(The `intr` arg only carries the ground-truth K for projection; it is NOT passed to the calibrate command.)

- [ ] **Step 3: Add the happy-path test (real sidecar)**

```rust
/// Real-sidecar happy path: a synthetic curved 4-cabinet SL scene calibrated from
/// 4 poses must recover the ground-truth focal within ~2% and write _sl_intrinsics.json.
#[test]
#[ignore = "requires LMT_VBA_SIDECAR_PATH set to a real sidecar binary/wrapper"]
fn calibrate_structured_light_recovers_focal() {
    let sidecar = match gp_sidecar() {
        Some(s) => s,
        None => { eprintln!("skip: LMT_VBA_SIDECAR_PATH unset"); return; }
    };
    let tmp = TempDir::new().unwrap();
    let proj = tmp.path().join("proj");
    write_curved_project(&proj, 4);

    lmt().env("LMT_VBA_SIDECAR_PATH", &sidecar)
        .args(["--json", "visual", "generate-structured-light", proj.to_str().unwrap(), "MAIN", "--yes"])
        .assert().success();
    let meta_path = proj.join("patterns/MAIN/sl/sl_meta.json");
    assert!(meta_path.exists());

    // ground-truth K used ONLY to synthesize the corr pixels
    let intr = tmp.path().join("truthK.json");
    std::fs::write(&intr, serde_json::json!({
        "K": [[3000.0, 0, 2000.0], [0, 3000.0, 1500.0], [0, 0, 1.0]],
        "dist_coeffs": [0,0,0,0,0], "image_size": [4000,3000]
    }).to_string()).unwrap();
    let corr = write_sl_calib_corr(tmp.path(), &sidecar, &meta_path, &intr);

    let mut args = vec!["--json", "visual", "calibrate-structured-light",
        proj.to_str().unwrap(), "MAIN", "--sl-meta", meta_path.to_str().unwrap()];
    for c in &corr { args.push("--corr"); args.push(c); }
    args.push("--yes");
    let assert = lmt().env("LMT_VBA_SIDECAR_PATH", &sidecar).args(&args).assert().success();

    let env = gp_stdout_env(assert.get_output());
    assert_eq!(env["ok"], true, "envelope ok: {env}");
    let out_file = proj.join("calibration/MAIN_sl_intrinsics.json");
    assert!(out_file.exists(), "must write _sl_intrinsics.json");
    let intr_out: Value = serde_json::from_slice(&std::fs::read(&out_file).unwrap()).unwrap();
    let fx = intr_out["K"][0][0].as_f64().unwrap();
    assert!((fx - 3000.0).abs() / 3000.0 < 0.02, "focal within 2%, got {fx}");
    assert_eq!(intr_out["calibration_method"], "structured_light_nominal");
}
```

- [ ] **Step 4: Add refuse + dry-run + overwrite-guard tests (no sidecar needed)**

```rust
#[test]
fn calibrate_structured_light_refuses_without_yes() {
    let tmp = TempDir::new().unwrap();
    let proj = tmp.path().join("proj");
    write_gp_project(&proj, 2, 1);
    let meta = tmp.path().join("sl_meta.json"); std::fs::write(&meta, "{}").unwrap();
    let c0 = tmp.path().join("c0.json"); std::fs::write(&c0, "{}").unwrap();
    let assert = lmt().args(["--json", "visual", "calibrate-structured-light",
        proj.to_str().unwrap(), "MAIN", "--sl-meta", meta.to_str().unwrap(),
        "--corr", c0.to_str().unwrap()]).assert().failure();
    let out = assert.get_output();
    assert_eq!(out.status.code(), Some(2));
    let env: Value = serde_json::from_str(std::str::from_utf8(&out.stderr).unwrap().trim_end()).unwrap();
    assert_eq!(env["error"]["code"], "invalid_input");
}

#[test]
fn calibrate_structured_light_dry_run_writes_nothing() {
    let tmp = TempDir::new().unwrap();
    let proj = tmp.path().join("proj");
    write_gp_project(&proj, 2, 1);
    let meta = tmp.path().join("sl_meta.json"); std::fs::write(&meta, "{}").unwrap();
    let c0 = tmp.path().join("c0.json"); std::fs::write(&c0, "{}").unwrap();
    let assert = lmt().args(["--json", "--dry-run", "visual", "calibrate-structured-light",
        proj.to_str().unwrap(), "MAIN", "--sl-meta", meta.to_str().unwrap(),
        "--corr", c0.to_str().unwrap()]).assert().success();
    let env: Value = serde_json::from_slice(&assert.get_output().stdout).unwrap();
    assert_eq!(env["ok"], true);
    assert_eq!(env["data"]["dry_run"], true);
    assert!(!proj.join("calibration/MAIN_sl_intrinsics.json").exists());
}

#[cfg(unix)]
#[test]
fn calibrate_structured_light_refuses_overwrite_without_force() {
    // Pre-create the default out file; without --force the command must refuse
    // (invalid_input) BEFORE touching the sidecar. Uses an error mock so we never
    // reach a real solver.
    let tmp = TempDir::new().unwrap();
    let proj = tmp.path().join("proj");
    write_gp_project(&proj, 2, 1);
    std::fs::create_dir_all(proj.join("calibration")).unwrap();
    std::fs::write(proj.join("calibration/MAIN_sl_intrinsics.json"), "{}").unwrap();
    let meta = tmp.path().join("sl_meta.json"); std::fs::write(&meta, "{}").unwrap();
    let c0 = tmp.path().join("c0.json"); std::fs::write(&c0, "{}").unwrap();
    let mock = write_error_mock(tmp.path(), "internal_error");
    let assert = lmt().env("LMT_VBA_SIDECAR_PATH", &mock)
        .args(["--json", "visual", "calibrate-structured-light",
            proj.to_str().unwrap(), "MAIN", "--sl-meta", meta.to_str().unwrap(),
            "--corr", c0.to_str().unwrap(), "--yes"]).assert().failure();
    let out = assert.get_output();
    assert_eq!(out.status.code(), Some(2));
    let env: Value = serde_json::from_str(std::str::from_utf8(&out.stderr).unwrap().trim_end()).unwrap();
    assert_eq!(env["error"]["code"], "invalid_input");
}
```

- [ ] **Step 5: Run the no-sidecar tests**

Run: `cargo test -p lmt-cli --test cli_e2e calibrate_structured_light_refuses_without_yes calibrate_structured_light_dry_run_writes_nothing calibrate_structured_light_refuses_overwrite_without_force`
Expected: 3 pass.

- [ ] **Step 6: Run the real-sidecar happy test**

Build the sidecar wrapper, then:
Run: `LMT_VBA_SIDECAR_PATH=python-sidecar/.venv/bin/lmt-vba-sidecar cargo test -p lmt-cli --test cli_e2e calibrate_structured_light_recovers_focal -- --ignored`
Expected: PASS (focal within 2%). (Confirm the wrapper path; mirror how the other `#[ignore]` SL tests are invoked in CI.)

- [ ] **Step 7: Commit**

```bash
git add crates/lmt-cli/tests/cli_e2e.rs
git commit -m "test(sl): calibrate-structured-light E2E (happy/refuse/dry-run/overwrite-guard)"
```

---

## Task 9: Docs — `docs/agents-cli.md` command row

**Files:**
- Modify: `docs/agents-cli.md` (add a row after the `reconstruct-structured-light` row, ~line 46)

- [ ] **Step 1: Add the command-table row**

```markdown
| `lmt visual calibrate-structured-light <project> <screen_id> --sl-meta <json> --corr <c.json> ... [--out <path>] [--force] [--max-rms-px <f>]` | destructive | Calibrate ONE camera's intrinsics (`fx,fy,cx,cy,k1,k2`) from its structured-light white-dot captures, using the project's nominal design wall as a known 3D target (curved wall = non-coplanar target). Produces `calibration/<screen_id>_sl_intrinsics.json` (5-key intrinsics contract + `calibration_method`/`pp_stddev_px`/`focal_stddev_px`/`n_poses`); **non-destructive** — refuses to overwrite an existing intrinsics file at the out path without `--force`. Provenance-gated like `reconstruct-structured-light` (all `--corr` share one `screen_id` + `sl_meta_sha256` matching `--sl-meta`, cabinet set == project present cells); additionally hard-gates a single `camera_image_size` across corr (one camera). Refuses on degenerate observability (near-coplanar target + <3 diverse poses, near-duplicate poses, too-low image coverage, or high principal-point/focal covariance) → `observability_failed`(17) BEFORE any write, never a confidently-wrong K. `--max-rms-px` (default 1.5) gates reproj RMS. Errors: `invalid_input`(2), `intrinsics_invalid`(16), `observability_failed`(17). NOTE: distinct output from checkerboard `visual calibrate` (`_intrinsics.json`); Step 2 `reconstruct-structured-light` consumes either via `--intrinsics`. |
```

- [ ] **Step 2: Confirm the error-code table already covers 2/16/17** (no change expected — reused codes). If a "Not exposed in CLI/GUI" note is needed (Task 7 found CLI-only), add it to the relevant section.

- [ ] **Step 3: Commit**

```bash
git add docs/agents-cli.md
git commit -m "docs(sl): agents-cli row for calibrate-structured-light"
```

---

## Task 10: Full verification + schema self-check

**Files:** none.

- [ ] **Step 1: Full workspace tests**

Run: `cargo test --workspace`
Expected: all pass (incl. the new no-sidecar E2E cases).

- [ ] **Step 2: Full sidecar tests**

Run: `python-sidecar/.venv/bin/python -m pytest python-sidecar/tests -q`
Expected: all pass.

- [ ] **Step 3: Schema dump still valid (no new DTO, but verify no break)**

Run: `./target/debug/lmt --json schema | jq '.ok'`
Expected: `true`. (CalibrateResult was already registered; we added no new DTO.)

- [ ] **Step 4: Subcommand + help present**

Run: `./target/debug/lmt --help | grep calibrate-structured-light && ./target/debug/lmt visual calibrate-structured-light --help`
Expected: subcommand listed; help shows all flags.

- [ ] **Step 5: Real end-to-end smoke (optional, with sidecar)**

Run the happy E2E once more with `--ignored` (Task 8 Step 6) to confirm the whole chain (CLI → app → adapter → sidecar → file) works against the real solver.

- [ ] **Step 6: Final commit (if any cleanup)**

```bash
git add -A && git commit -m "chore(sl): step-1 calibration verification pass"
```

---

## Self-Review notes (author)

- **Spec coverage:** §3.1 → Task 1; §3.2 solver+gates → Task 3; §3.3 contracts → Tasks 2/4/5; §4 transport (lmt-app/adapter/clap/tauri/docs/DTO) → Tasks 4/5/6/7/9; §5 errors → exercised in Tasks 3/8; §6 tests (golden oracle + substrate + adversarial + E2E) → Tasks 1/3/8; §8 thresholds → constants in Task 3 + refusal tests.
- **`invalid_input` = exit 2** (verified vs cli_e2e.rs), not 3.
- **Result-event tail** in calibrate_sl.py is the one spot the engineer must copy from calibrate.py (Step 3.1) — flagged explicitly, not left vague.
- **Tauri shim** is conditional on the sibling having one (Task 7 Step 1 decides) — if reconstruct-structured-light is CLI-only, calibration is too + documented.
- **Units:** world = meters (nominal), test cameras = meters; calibrate object-points unit is intrinsics-invariant.
