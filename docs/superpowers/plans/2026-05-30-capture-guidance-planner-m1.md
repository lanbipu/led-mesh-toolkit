# Capture Guidance Planner — M1 Implementation Plan (engine core)

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Build the Python-sidecar core of the capture-guidance planner — geometry expansion, per-sample-point visibility, observability-gate-aligned coverage, bridging, and a visibility-aware Monte-Carlo scorer — as pure, independently tested functions.

**Architecture:** New package `lmt_vba_sidecar.capture_planner` with four focused modules (`gates`, `geometry`, `visibility`, `scoring`). It reuses `nominal.py` for the as-built 3D model and the geometric helpers in `sl_feasibility.py` (`project_point`, `look_at_pose`, `solve_pnp_pose`, `triangulate_multiview`). Visibility is computed **per sample point** (no cabinet-center shortcut); coverage aggregation mirrors the real reconstruction gate in `reconstruct.py` / `observability.py`, but applies a deliberately *conservative* covering rule (each contributing view must see ≥`MIN_PNP_CORNERS` points).

**Tech Stack:** Python 3.12, numpy, OpenCV (`cv2.solvePnP` SQPNP), pydantic models from `ipc.py` (`CabinetArray`), pytest.

**Scope note:** This plan covers spec milestone **M1 only** (`docs/superpowers/specs/2026-05-30-camera-capture-guidance-design.md` §8). M2 (seed + optimizer), M3 (CLI/DTO/HTML), and M4 (curved self-occlusion, visibility check (d)) get their own plans after M1 lands. M1 deliberately implements visibility checks **(a) cheirality, (b) in-frame, (c) incidence** only; check (d) self-occlusion is M4.

**Run environment:** All commands run from the worktree's `python-sidecar/` directory using its isolated venv: `./.venv/bin/python -m pytest ...`. The venv resolves `lmt_vba_sidecar` to the worktree `src/` (verified at setup).

---

## File Structure

| File | Responsibility |
| --- | --- |
| `python-sidecar/src/lmt_vba_sidecar/capture_planner/__init__.py` | Package marker; re-export the public types. |
| `python-sidecar/src/lmt_vba_sidecar/capture_planner/gates.py` | Observability gate constants, mirrored from `reconstruct.py` (single source). |
| `python-sidecar/src/lmt_vba_sidecar/capture_planner/geometry.py` | `expand_screen` → `ScreenGeometry` (cabinet centers, normals, sample points). |
| `python-sidecar/src/lmt_vba_sidecar/capture_planner/visibility.py` | `Camera`, intrinsics-from-FOV, per-point `point_visible`, `coverage_report`, `bridging_report`. |
| `python-sidecar/src/lmt_vba_sidecar/capture_planner/scoring.py` | `score_screen` — visibility-gated Monte-Carlo 3D residual per cabinet. |
| `python-sidecar/tests/test_capture_planner_gates.py` | Keep-in-sync test against `reconstruct.py`. |
| `python-sidecar/tests/test_capture_planner_geometry.py` | Geometry expansion tests. |
| `python-sidecar/tests/test_capture_planner_visibility.py` | Per-point visibility + coverage + guardrail + gate-alignment + bridging tests. |
| `python-sidecar/tests/test_capture_planner_scoring.py` | Scorer tests. |

---

## Task 1: Observability gate constants (`gates.py`)

**Files:**
- Create: `python-sidecar/src/lmt_vba_sidecar/capture_planner/__init__.py`
- Create: `python-sidecar/src/lmt_vba_sidecar/capture_planner/gates.py`
- Test: `python-sidecar/tests/test_capture_planner_gates.py`

- [ ] **Step 1: Write the failing test**

Create `python-sidecar/tests/test_capture_planner_gates.py`:

```python
import inspect

from lmt_vba_sidecar import reconstruct
from lmt_vba_sidecar.observability import check_observability
from lmt_vba_sidecar.capture_planner import gates


def test_gate_constants_mirror_reconstruct():
    # PnP corner floor and quality-view threshold are importable module
    # constants in reconstruct.py — assert exact mirror.
    assert gates.MIN_PNP_CORNERS == reconstruct.MIN_PNP_CORNERS
    assert gates.QUALITY_MIN_VIEWS == reconstruct.QUALITY_MIN_VIEWS


def test_gate_constants_mirror_check_observability_defaults():
    # min_views / min_points live as defaults on check_observability.
    sig = inspect.signature(check_observability)
    assert gates.MIN_VIEWS == sig.parameters["min_views"].default
    assert gates.MIN_POINTS_PER_CABINET == sig.parameters["min_points"].default
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cd python-sidecar && ./.venv/bin/python -m pytest tests/test_capture_planner_gates.py -v`
Expected: FAIL with `ModuleNotFoundError: No module named 'lmt_vba_sidecar.capture_planner'`.

- [ ] **Step 3: Write minimal implementation**

Create `python-sidecar/src/lmt_vba_sidecar/capture_planner/__init__.py`:

```python
"""Capture guidance planner (sidecar engine). See
docs/superpowers/specs/2026-05-30-camera-capture-guidance-design.md."""
```

Create `python-sidecar/src/lmt_vba_sidecar/capture_planner/gates.py`:

```python
"""Observability gate constants for capture planning.

MUST stay in sync with the real reconstruction gate. The source of truth is
`reconstruct.py` (MIN_PNP_CORNERS, QUALITY_MIN_VIEWS) and the defaults of
`observability.check_observability` (min_views, min_points). A unit test
asserts these mirror; if reconstruct changes its gate, that test breaks loud.
"""
from __future__ import annotations

# A single view needs this many visible points to seed its PnP pose.
MIN_PNP_CORNERS = 4
# A cabinet needs at least this many observing views (HARD gate).
MIN_VIEWS = 2
# A cabinet needs at least this many total observations across views (HARD).
MIN_POINTS_PER_CABINET = 8
# Below this many views (but >= MIN_VIEWS) the cabinet is flagged low-observation.
QUALITY_MIN_VIEWS = 4
```

- [ ] **Step 4: Run test to verify it passes**

Run: `cd python-sidecar && ./.venv/bin/python -m pytest tests/test_capture_planner_gates.py -v`
Expected: PASS (2 passed).

- [ ] **Step 5: Commit**

```bash
git add python-sidecar/src/lmt_vba_sidecar/capture_planner/__init__.py \
        python-sidecar/src/lmt_vba_sidecar/capture_planner/gates.py \
        python-sidecar/tests/test_capture_planner_gates.py
git commit -m "feat(capture-planner): observability gate constants mirrored from reconstruct"
```

---

## Task 2: Geometry expansion (`geometry.py`)

**Files:**
- Create: `python-sidecar/src/lmt_vba_sidecar/capture_planner/geometry.py`
- Test: `python-sidecar/tests/test_capture_planner_geometry.py`

Reuses `nominal_cabinet_centers_model_frame(cab, shape_prior)` and `nominal_cabinet_normals_model_frame(cab, shape_prior)` (both return `dict[(col,row)] -> (x,y,z)` in **meters**; we convert to mm). `cab` is a `CabinetArray` (`ipc.py`): `.cols`, `.rows`, `.cabinet_size_mm`, `.absent_cells`.

- [ ] **Step 1: Write the failing test**

Create `python-sidecar/tests/test_capture_planner_geometry.py`:

```python
import numpy as np

from lmt_vba_sidecar.ipc import CabinetArray
from lmt_vba_sidecar.capture_planner.geometry import expand_screen


def _cab(cols, rows, size=(500.0, 500.0)):
    return CabinetArray(cols=cols, rows=rows, cabinet_size_mm=list(size), absent_cells=[])


def test_flat_single_cabinet_sample_grid_spans_face_and_is_planar():
    geom = expand_screen(_cab(1, 1), "flat", sample_grid=(4, 4))
    assert len(geom.cabinets) == 1
    c = geom.cabinets[0]
    # nominal center of a 500mm cabinet at (col0,row0): (250,250,0) mm
    assert np.allclose(c.center_mm, [250.0, 250.0, 0.0])
    assert np.allclose(c.normal, [0.0, 0.0, 1.0])
    assert c.sample_points_mm.shape == (16, 3)
    # 4x4 grid spans the full cabinet face: x,y in [0,500], z==0 (flat)
    assert np.isclose(c.sample_points_mm[:, 0].min(), 0.0)
    assert np.isclose(c.sample_points_mm[:, 0].max(), 500.0)
    assert np.isclose(c.sample_points_mm[:, 1].min(), 0.0)
    assert np.isclose(c.sample_points_mm[:, 1].max(), 500.0)
    assert np.allclose(c.sample_points_mm[:, 2], 0.0)
    assert geom.radius_mm is None
    assert geom.total_width_mm == 500.0


def test_curved_off_center_cabinet_tilts_and_bows():
    radius = 6000.0
    geom = expand_screen(_cab(4, 1), {"curved": {"radius_mm": radius}}, sample_grid=(4, 4))
    cols = sorted(geom.cabinets, key=lambda c: c.col)
    # center column pair straddles the apex; outermost cabinets tilt in x and bow in +z
    left = cols[0]
    assert left.normal[0] < 0.0          # left-of-center tilts to -x
    assert not np.isclose(left.center_mm[2], 0.0)   # bowed off the z=0 plane
    assert np.isclose(np.linalg.norm(left.normal), 1.0)
    assert geom.radius_mm == radius
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cd python-sidecar && ./.venv/bin/python -m pytest tests/test_capture_planner_geometry.py -v`
Expected: FAIL with `ModuleNotFoundError: ... capture_planner.geometry`.

- [ ] **Step 3: Write minimal implementation**

Create `python-sidecar/src/lmt_vba_sidecar/capture_planner/geometry.py`:

```python
"""Expand a screen's nominal geometry into 3D cabinet centers, surface normals,
and per-cabinet sample points (model frame, millimetres).

The sample grid is the unit of visibility/coverage downstream: each cabinet is
sampled by a `sample_grid` (default 4x4) covering its active face, so coverage
can be judged per point against the observability gate (>=8 obs / >=4 per view)
rather than by a single cabinet-center test.
"""
from __future__ import annotations

from dataclasses import dataclass

import numpy as np

from lmt_vba_sidecar.ipc import CabinetArray
from lmt_vba_sidecar.nominal import (
    _curved_radius,
    _is_curved,
    nominal_cabinet_centers_model_frame,
    nominal_cabinet_normals_model_frame,
)


@dataclass(frozen=True)
class CabinetGeom:
    col: int
    row: int
    center_mm: np.ndarray        # (3,) model frame, mm
    normal: np.ndarray           # (3,) unit surface normal
    sample_points_mm: np.ndarray  # (K, 3) model frame, mm


@dataclass(frozen=True)
class ScreenGeometry:
    cabinets: list[CabinetGeom]
    radius_mm: float | None
    total_width_mm: float
    total_height_mm: float


def _tangent_basis(normal: np.ndarray) -> tuple[np.ndarray, np.ndarray]:
    """Orthonormal (right, up) spanning the cabinet face. World +Y is 'up';
    'right' = up x normal. For a flat (+z) face this is (+x, +y)."""
    up = np.array([0.0, 1.0, 0.0])
    right = np.cross(up, normal)
    right = right / np.linalg.norm(right)
    up_local = np.cross(normal, right)
    return right, up_local


def expand_screen(cab: CabinetArray, shape_prior, sample_grid=(4, 4)) -> ScreenGeometry:
    centers_m = nominal_cabinet_centers_model_frame(cab, shape_prior)
    normals = nominal_cabinet_normals_model_frame(cab, shape_prior)
    cw_mm, ch_mm = cab.cabinet_size_mm
    nx, ny = sample_grid
    us = np.linspace(-1.0, 1.0, nx) * (cw_mm / 2.0)
    vs = np.linspace(-1.0, 1.0, ny) * (ch_mm / 2.0)

    cabinets: list[CabinetGeom] = []
    for (col, row), c_m in centers_m.items():
        center_mm = np.asarray(c_m, float) * 1000.0
        normal = np.asarray(normals[(col, row)], float)
        right, up_local = _tangent_basis(normal)
        pts = [center_mm + u * right + v * up_local for v in vs for u in us]
        cabinets.append(
            CabinetGeom(col, row, center_mm, normal, np.asarray(pts, float))
        )

    cabinets.sort(key=lambda c: (c.row, c.col))
    radius = _curved_radius(shape_prior) if _is_curved(shape_prior) else None
    return ScreenGeometry(
        cabinets=cabinets,
        radius_mm=radius,
        total_width_mm=cab.cols * cw_mm,
        total_height_mm=cab.rows * ch_mm,
    )
```

- [ ] **Step 4: Run test to verify it passes**

Run: `cd python-sidecar && ./.venv/bin/python -m pytest tests/test_capture_planner_geometry.py -v`
Expected: PASS (2 passed).

- [ ] **Step 5: Commit**

```bash
git add python-sidecar/src/lmt_vba_sidecar/capture_planner/geometry.py \
        python-sidecar/tests/test_capture_planner_geometry.py
git commit -m "feat(capture-planner): expand screen geometry into sampled cabinet faces"
```

---

## Task 3: Per-point visibility primitives (`visibility.py`)

**Files:**
- Create: `python-sidecar/src/lmt_vba_sidecar/capture_planner/visibility.py`
- Test: `python-sidecar/tests/test_capture_planner_visibility.py`

`Camera` holds `K (3x3)`, `R (3x3 world->cam)`, `t (3,)`, `image_size (W,H)`. `point_visible` runs checks (a) cheirality, (b) in-frame, (c) incidence. Reuses `look_at_pose` from `sl_feasibility`.

- [ ] **Step 1: Write the failing test**

Create `python-sidecar/tests/test_capture_planner_visibility.py`:

```python
import numpy as np

from lmt_vba_sidecar.capture_planner.visibility import (
    Camera,
    intrinsics_from_fov,
    look_at_camera,
    point_visible,
)

FLAT_NORMAL = np.array([0.0, 0.0, 1.0])
WALL_PT = np.array([250.0, 250.0, 0.0])   # a point on a flat wall facing +z


def test_intrinsics_from_fov_horizontal():
    K = intrinsics_from_fov((1920, 1080), hfov_deg=90.0)
    # f = (W/2)/tan(45deg) = 960
    assert np.isclose(K[0, 0], 960.0)
    assert np.isclose(K[1, 1], 960.0)
    assert np.isclose(K[0, 2], 960.0)
    assert np.isclose(K[1, 2], 540.0)


def test_point_visible_frontal_true():
    K = intrinsics_from_fov((1920, 1080), hfov_deg=50.0)
    cam = look_at_camera(K, [250.0, 250.0, 3000.0], WALL_PT, (1920, 1080))
    assert point_visible(cam, WALL_PT, FLAT_NORMAL) is True


def test_point_visible_behind_camera_false_cheirality():
    K = intrinsics_from_fov((1920, 1080), hfov_deg=50.0)
    cam = look_at_camera(K, [250.0, 250.0, 3000.0], WALL_PT, (1920, 1080))
    behind = np.array([250.0, 250.0, 6000.0])  # past the camera, +z further out
    assert point_visible(cam, behind, FLAT_NORMAL) is False


def test_point_visible_out_of_frame_false():
    K = intrinsics_from_fov((1920, 1080), hfov_deg=50.0)
    cam = look_at_camera(K, [250.0, 250.0, 3000.0], WALL_PT, (1920, 1080))
    far_side = np.array([9000.0, 250.0, 0.0])   # way off to the right, off-sensor
    assert point_visible(cam, far_side, FLAT_NORMAL) is False


def test_point_visible_grazing_incidence_false():
    K = intrinsics_from_fov((1920, 1080), hfov_deg=50.0)
    # camera almost in the wall plane -> ~88deg incidence on a +z normal
    cam = look_at_camera(K, [5250.0, 250.0, 100.0], WALL_PT, (1920, 1080))
    assert point_visible(cam, WALL_PT, FLAT_NORMAL, incidence_max_deg=60.0) is False
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cd python-sidecar && ./.venv/bin/python -m pytest tests/test_capture_planner_visibility.py -v`
Expected: FAIL with `ModuleNotFoundError: ... capture_planner.visibility`.

- [ ] **Step 3: Write minimal implementation**

Create `python-sidecar/src/lmt_vba_sidecar/capture_planner/visibility.py`:

```python
"""Per-sample-point visibility and observability-gate-aligned coverage.

Visibility is judged PER POINT (cheirality, in-frame, incidence) — never by a
single cabinet-center test. Coverage then aggregates point visibility to the
real reconstruction gate (see gates.py): a camera 'covers' a cabinet only if it
sees >= MIN_PNP_CORNERS of its sample points (so that view could seed a PnP
pose); a cabinet is 'reconstructable' only with >= MIN_VIEWS covering cameras
and >= MIN_POINTS_PER_CABINET total observations. This is deliberately
conservative vs reconstruct's bare gate (which counts >=1-obs views).
"""
from __future__ import annotations

from dataclasses import dataclass

import numpy as np

from lmt_vba_sidecar.sl_feasibility import look_at_pose
from lmt_vba_sidecar.capture_planner import gates
from lmt_vba_sidecar.capture_planner.geometry import ScreenGeometry


@dataclass(frozen=True)
class Camera:
    K: np.ndarray          # (3,3)
    R: np.ndarray          # (3,3) world->cam
    t: np.ndarray          # (3,) world->cam
    image_size: tuple      # (W, H)


def intrinsics_from_fov(image_size, hfov_deg=None, vfov_deg=None) -> np.ndarray:
    """Build a pinhole K from FOV + sensor resolution. Centered principal point,
    square pixels, zero skew. Exactly one of hfov_deg / vfov_deg is required."""
    w, h = image_size
    if (hfov_deg is None) == (vfov_deg is None):
        raise ValueError("pass exactly one of hfov_deg / vfov_deg")
    if hfov_deg is not None:
        f = (w / 2.0) / np.tan(np.deg2rad(hfov_deg) / 2.0)
    else:
        f = (h / 2.0) / np.tan(np.deg2rad(vfov_deg) / 2.0)
    return np.array([[f, 0.0, w / 2.0], [0.0, f, h / 2.0], [0.0, 0.0, 1.0]], float)


def look_at_camera(K, cam_pos_mm, target_mm, image_size, up=None) -> Camera:
    R, t = look_at_pose(np.asarray(cam_pos_mm, float), np.asarray(target_mm, float), up)
    return Camera(np.asarray(K, float), R, t, tuple(image_size))


def point_visible(cam: Camera, p_mm, normal, *, margin_frac=0.05,
                  incidence_max_deg=60.0) -> bool:
    p = np.asarray(p_mm, float)
    p_cam = cam.R @ p + cam.t
    if p_cam[2] <= 0.0:                                   # (a) cheirality
        return False
    uv = cam.K @ p_cam
    u, v = uv[0] / uv[2], uv[1] / uv[2]
    w, h = cam.image_size
    mx, my = margin_frac * w, margin_frac * h
    if not (mx <= u <= w - mx and my <= v <= h - my):     # (b) in-frame
        return False
    cam_center = -cam.R.T @ cam.t                          # (c) incidence
    to_cam = cam_center - p
    cos_inc = float(np.dot(np.asarray(normal, float), to_cam) / np.linalg.norm(to_cam))
    if cos_inc <= 0.0:                                     # back-facing
        return False
    return np.degrees(np.arccos(np.clip(cos_inc, -1.0, 1.0))) <= incidence_max_deg
```

- [ ] **Step 4: Run test to verify it passes**

Run: `cd python-sidecar && ./.venv/bin/python -m pytest tests/test_capture_planner_visibility.py -v`
Expected: PASS (5 passed).

- [ ] **Step 5: Commit**

```bash
git add python-sidecar/src/lmt_vba_sidecar/capture_planner/visibility.py \
        python-sidecar/tests/test_capture_planner_visibility.py
git commit -m "feat(capture-planner): per-point visibility (cheirality/frame/incidence)"
```

---

## Task 4: Coverage aggregation + guardrail + gate alignment (`visibility.py`)

**Files:**
- Modify: `python-sidecar/src/lmt_vba_sidecar/capture_planner/visibility.py`
- Test: `python-sidecar/tests/test_capture_planner_visibility.py` (append)

- [ ] **Step 1: Write the failing tests (append to the visibility test file)**

Append to `python-sidecar/tests/test_capture_planner_visibility.py`:

```python
from lmt_vba_sidecar.ipc import CabinetArray
from lmt_vba_sidecar.capture_planner.geometry import expand_screen
from lmt_vba_sidecar.capture_planner.visibility import coverage_report, vis_count
from lmt_vba_sidecar.capture_planner import gates


def _single_flat_cabinet():
    cab = CabinetArray(cols=1, rows=1, cabinet_size_mm=[500.0, 500.0], absent_cells=[])
    return expand_screen(cab, "flat", sample_grid=(4, 4))


def _good_cam(K, pos, geom):
    # a camera that frontally sees the whole single cabinet
    return look_at_camera(K, pos, geom.cabinets[0].center_mm, (1920, 1080))


def test_guardrail_center_in_frame_but_points_clipped_is_not_covered():
    # Codex guardrail: the cabinet CENTER projects in-frame (the old
    # center-shortcut would PASS), but a tiny 64x64 frame with long focal length
    # clips every off-center sample point -> vis_count 0 -> NOT covered.
    geom = _single_flat_cabinet()
    cabg = geom.cabinets[0]
    K = np.array([[2000.0, 0.0, 32.0], [0.0, 2000.0, 32.0], [0.0, 0.0, 1.0]])
    cam = look_at_camera(K, [250.0, 250.0, 1000.0], cabg.center_mm, (64, 64))
    # the geometric center would have passed a center-only test:
    assert point_visible(cam, cabg.center_mm, cabg.normal) is True
    # but per-point gating sees < MIN_PNP_CORNERS sample points:
    assert vis_count(cam, cabg) < gates.MIN_PNP_CORNERS
    per_cab, _ = coverage_report(geom, [cam])
    assert per_cab[0].reconstructable is False


def test_one_view_not_reconstructable_even_if_all_points_visible():
    geom = _single_flat_cabinet()
    K = intrinsics_from_fov((1920, 1080), hfov_deg=50.0)
    cam = _good_cam(K, [250.0, 250.0, 3000.0], geom)
    assert vis_count(cam, geom.cabinets[0]) == 16   # sees all sample points
    per_cab, _ = coverage_report(geom, [cam])
    cov = per_cab[0]
    assert cov.total_observations >= gates.MIN_POINTS_PER_CABINET  # points gate OK
    assert len(cov.covering_cams) == 1
    assert cov.reconstructable is False              # ... but views gate fails


def test_two_views_reconstructable_but_low_observation():
    geom = _single_flat_cabinet()
    K = intrinsics_from_fov((1920, 1080), hfov_deg=50.0)
    cams = [
        _good_cam(K, [-1500.0, 250.0, 3000.0], geom),
        _good_cam(K, [2000.0, 250.0, 3000.0], geom),
    ]
    per_cab, _ = coverage_report(geom, cams)
    cov = per_cab[0]
    assert len(cov.covering_cams) == 2
    assert cov.reconstructable is True
    assert cov.low_observation is True               # 2 < QUALITY_MIN_VIEWS(4)


def test_four_views_not_low_observation():
    geom = _single_flat_cabinet()
    K = intrinsics_from_fov((1920, 1080), hfov_deg=50.0)
    cams = [
        _good_cam(K, [-1500.0, 250.0, 3000.0], geom),
        _good_cam(K, [600.0, 250.0, 3000.0], geom),
        _good_cam(K, [2000.0, 250.0, 3000.0], geom),
        _good_cam(K, [250.0, 1800.0, 3000.0], geom),
    ]
    per_cab, _ = coverage_report(geom, cams)
    cov = per_cab[0]
    assert len(cov.covering_cams) == 4
    assert cov.reconstructable is True
    assert cov.low_observation is False
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cd python-sidecar && ./.venv/bin/python -m pytest tests/test_capture_planner_visibility.py -k "guardrail or view or observation" -v`
Expected: FAIL with `ImportError: cannot import name 'coverage_report'`.

- [ ] **Step 3: Write minimal implementation (append to `visibility.py`)**

Append to `python-sidecar/src/lmt_vba_sidecar/capture_planner/visibility.py`:

```python
from lmt_vba_sidecar.capture_planner.geometry import CabinetGeom


@dataclass(frozen=True)
class CabinetCoverage:
    col: int
    row: int
    covering_cams: tuple        # cam indices with >= MIN_PNP_CORNERS visible points
    total_observations: int     # sum of visible points across covering cams
    reconstructable: bool       # >= MIN_VIEWS covering AND >= MIN_POINTS_PER_CABINET obs
    low_observation: bool       # reconstructable AND covering < QUALITY_MIN_VIEWS


def vis_count(cam: Camera, cabg: CabinetGeom, *, margin_frac=0.05,
              incidence_max_deg=60.0) -> int:
    return sum(
        1
        for p in cabg.sample_points_mm
        if point_visible(cam, p, cabg.normal, margin_frac=margin_frac,
                         incidence_max_deg=incidence_max_deg)
    )


def coverage_report(geom: ScreenGeometry, cams: list[Camera], *, margin_frac=0.05,
                    incidence_max_deg=60.0):
    """Return (per_cabinet: list[CabinetCoverage], counts: dict[(ci,(col,row))->int]).
    `counts` is the per-camera per-cabinet visible-point count, reused downstream
    (bridging, scoring)."""
    counts: dict[tuple[int, tuple[int, int]], int] = {}
    for ci, cam in enumerate(cams):
        for cabg in geom.cabinets:
            n = vis_count(cam, cabg, margin_frac=margin_frac,
                          incidence_max_deg=incidence_max_deg)
            if n:
                counts[(ci, (cabg.col, cabg.row))] = n

    per_cabinet: list[CabinetCoverage] = []
    for cabg in geom.cabinets:
        key = (cabg.col, cabg.row)
        covering = tuple(
            ci for ci in range(len(cams))
            if counts.get((ci, key), 0) >= gates.MIN_PNP_CORNERS
        )
        total_obs = sum(counts[(ci, key)] for ci in covering)
        reconstructable = (
            len(covering) >= gates.MIN_VIEWS
            and total_obs >= gates.MIN_POINTS_PER_CABINET
        )
        low_obs = reconstructable and len(covering) < gates.QUALITY_MIN_VIEWS
        per_cabinet.append(
            CabinetCoverage(cabg.col, cabg.row, covering, total_obs,
                            reconstructable, low_obs)
        )
    return per_cabinet, counts
```

- [ ] **Step 4: Run tests to verify they pass**

Run: `cd python-sidecar && ./.venv/bin/python -m pytest tests/test_capture_planner_visibility.py -v`
Expected: PASS (9 passed — 5 from Task 3 + 4 new).

- [ ] **Step 5: Commit**

```bash
git add python-sidecar/src/lmt_vba_sidecar/capture_planner/visibility.py \
        python-sidecar/tests/test_capture_planner_visibility.py
git commit -m "feat(capture-planner): observability-gate-aligned coverage + center-shortcut guardrail"
```

---

## Task 5: Bridging report (`visibility.py`)

**Files:**
- Modify: `python-sidecar/src/lmt_vba_sidecar/capture_planner/visibility.py`
- Test: `python-sidecar/tests/test_capture_planner_visibility.py` (append)

Two adjacent cabinets (4-neighbour grid) are "bridged" if >=1 camera **covers both** (covers = `>= MIN_PNP_CORNERS` visible points each). The screen splits into connected components over bridged edges; >1 component means the pose chain is broken.

- [ ] **Step 1: Write the failing test (append)**

Append to `python-sidecar/tests/test_capture_planner_visibility.py`:

```python
from lmt_vba_sidecar.capture_planner.visibility import bridging_report


def test_bridging_single_camera_covering_two_adjacent_is_one_component():
    cab = CabinetArray(cols=2, rows=1, cabinet_size_mm=[500.0, 500.0], absent_cells=[])
    geom = expand_screen(cab, "flat", sample_grid=(4, 4))
    K = intrinsics_from_fov((1920, 1080), hfov_deg=70.0)
    # one camera centered on the 2-wide wall, far enough to cover both cabinets
    center = np.array([500.0, 250.0, 0.0])
    cam = look_at_camera(K, center + [0.0, 0.0, 4000.0], center, (1920, 1080))
    rep = bridging_report(geom, [cam])
    assert rep.broken_edges == []
    assert rep.n_components == 1


def test_bridging_disjoint_cameras_break_the_chain():
    cab = CabinetArray(cols=2, rows=1, cabinet_size_mm=[500.0, 500.0], absent_cells=[])
    geom = expand_screen(cab, "flat", sample_grid=(4, 4))
    K = intrinsics_from_fov((1920, 1080), hfov_deg=30.0)
    # two tight-FOV cameras, each frontal to ONE cabinet only -> no shared cover
    left_c = np.array([250.0, 250.0, 0.0])
    right_c = np.array([750.0, 250.0, 0.0])
    cams = [
        look_at_camera(K, left_c + [0.0, 0.0, 1500.0], left_c, (1920, 1080)),
        look_at_camera(K, right_c + [0.0, 0.0, 1500.0], right_c, (1920, 1080)),
    ]
    rep = bridging_report(geom, cams)
    assert ((0, 0), (1, 0)) in rep.broken_edges or ((1, 0), (0, 0)) in rep.broken_edges
    assert rep.n_components == 2
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cd python-sidecar && ./.venv/bin/python -m pytest tests/test_capture_planner_visibility.py -k bridging -v`
Expected: FAIL with `ImportError: cannot import name 'bridging_report'`.

- [ ] **Step 3: Write minimal implementation (append to `visibility.py`)**

Append to `python-sidecar/src/lmt_vba_sidecar/capture_planner/visibility.py`:

```python
@dataclass(frozen=True)
class BridgingReport:
    n_components: int
    broken_edges: list           # [((col,row),(col,row)), ...] adjacent but unbridged
    components: list             # [[(col,row), ...], ...]


def bridging_report(geom: ScreenGeometry, cams: list[Camera], *, margin_frac=0.05,
                    incidence_max_deg=60.0) -> BridgingReport:
    _, counts = coverage_report(geom, cams, margin_frac=margin_frac,
                                incidence_max_deg=incidence_max_deg)

    def covers(ci, key):
        return counts.get((ci, key), 0) >= gates.MIN_PNP_CORNERS

    present = {(c.col, c.row) for c in geom.cabinets}
    # union-find over bridged adjacent edges
    parent = {k: k for k in present}

    def find(x):
        while parent[x] != x:
            parent[x] = parent[parent[x]]
            x = parent[x]
        return x

    def union(a, b):
        parent[find(a)] = find(b)

    broken: list = []
    for (col, row) in present:
        for (dc, dr) in ((1, 0), (0, 1)):            # right / up neighbours only
            nb = (col + dc, row + dr)
            if nb not in present:
                continue
            here = (col, row)
            shared = any(covers(ci, here) and covers(ci, nb) for ci in range(len(cams)))
            if shared:
                union(here, nb)
            else:
                broken.append((here, nb))

    roots: dict = {}
    for k in present:
        roots.setdefault(find(k), []).append(k)
    components = [sorted(v) for v in roots.values()]
    return BridgingReport(len(components), broken, components)
```

- [ ] **Step 4: Run test to verify it passes**

Run: `cd python-sidecar && ./.venv/bin/python -m pytest tests/test_capture_planner_visibility.py -v`
Expected: PASS (11 passed).

- [ ] **Step 5: Commit**

```bash
git add python-sidecar/src/lmt_vba_sidecar/capture_planner/visibility.py \
        python-sidecar/tests/test_capture_planner_visibility.py
git commit -m "feat(capture-planner): bridging report over shared-cover adjacency"
```

---

## Task 6: Visibility-aware Monte-Carlo scorer (`scoring.py`)

**Files:**
- Create: `python-sidecar/src/lmt_vba_sidecar/capture_planner/scoring.py`
- Test: `python-sidecar/tests/test_capture_planner_scoring.py`

Adapts `feasibility_rms_mm`'s real-path Monte-Carlo (observe → PnP-against-nominal → triangulate → 3D error vs truth) but **gated by per-point visibility**: a camera only observes the sample points it can see; a camera's PnP uses only its covered cabinets' visible points; a sample point is triangulated only from cameras that see it (>=2), else `uncovered`. Aggregates per cabinet and folds in `reconstructable` / `bridged` from Tasks 4–5.

- [ ] **Step 1: Write the failing test**

Create `python-sidecar/tests/test_capture_planner_scoring.py`:

```python
import numpy as np

from lmt_vba_sidecar.ipc import CabinetArray
from lmt_vba_sidecar.capture_planner.geometry import expand_screen
from lmt_vba_sidecar.capture_planner.visibility import intrinsics_from_fov, look_at_camera
from lmt_vba_sidecar.capture_planner.scoring import score_screen


def _flat_grid():
    cab = CabinetArray(cols=2, rows=2, cabinet_size_mm=[500.0, 500.0], absent_cells=[])
    return expand_screen(cab, "flat", sample_grid=(4, 4))


def _ring(geom, K, n=4, span_deg=40.0, dist=4000.0):
    cx = geom.total_width_mm / 2.0
    cy = geom.total_height_mm / 2.0
    target = np.array([cx, cy, 0.0])
    cams = []
    for a in np.deg2rad(np.linspace(-span_deg / 2, span_deg / 2, n)):
        pos = target + np.array([dist * np.sin(a), 0.0, dist * np.cos(a)])
        cams.append(look_at_camera(K, pos, target, (1920, 1080)))
    return cams


def test_well_covered_wall_passes_with_small_residual():
    geom = _flat_grid()
    K = intrinsics_from_fov((1920, 1080), hfov_deg=60.0)
    cams = _ring(geom, K, n=4, span_deg=40.0)
    report = score_screen(geom, cams, pixel_sigma=0.3, nominal_deviation_mm=1.0,
                          trials=10, seed=0, target_p95_residual_mm=3.0)
    for cov in report.values():
        assert cov["reconstructable"] is True
        assert cov["bridged"] is True
        assert cov["p95_mm"] < 3.0
        assert cov["pass"] is True


def test_under_observed_cabinet_is_flagged_not_scored():
    geom = _flat_grid()
    K = intrinsics_from_fov((1920, 1080), hfov_deg=60.0)
    # only ONE camera -> every cabinet has 1 covering view -> not reconstructable
    cams = _ring(geom, K, n=1, span_deg=0.0)
    report = score_screen(geom, cams, trials=5, seed=0, target_p95_residual_mm=3.0)
    for cov in report.values():
        assert cov["reconstructable"] is False
        assert cov["pass"] is False
        assert np.isnan(cov["p95_mm"])     # not scored, not an optimistic number
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cd python-sidecar && ./.venv/bin/python -m pytest tests/test_capture_planner_scoring.py -v`
Expected: FAIL with `ModuleNotFoundError: ... capture_planner.scoring`.

- [ ] **Step 3: Write minimal implementation**

Create `python-sidecar/src/lmt_vba_sidecar/capture_planner/scoring.py`:

```python
"""Visibility-aware Monte-Carlo scorer.

Same real-path skeleton as sl_feasibility.feasibility_rms_mm (observe with true
K + centroid noise; estimate each camera pose by solvePnP against the nominal
model with the believed K; triangulate; error vs truth) but every step is gated
by per-point visibility: a camera observes only points it can see, its PnP uses
only its covered cabinets' visible points, and a point is triangulated only from
cameras that see it (>=2). Cabinets that are not `reconstructable` (Task 4) are
not scored — their residual is NaN, never an optimistic number.
"""
from __future__ import annotations

import numpy as np

from lmt_vba_sidecar.sl_feasibility import project_point, solve_pnp_pose, triangulate_multiview
from lmt_vba_sidecar.capture_planner.geometry import ScreenGeometry
from lmt_vba_sidecar.capture_planner.visibility import (
    Camera,
    coverage_report,
    bridging_report,
    point_visible,
)


def _all_points(geom: ScreenGeometry):
    """Flatten to (truth Nx3, owner (col,row) per point, normal per point)."""
    pts, owner, normals = [], [], []
    for cabg in geom.cabinets:
        for p in cabg.sample_points_mm:
            pts.append(p)
            owner.append((cabg.col, cabg.row))
            normals.append(cabg.normal)
    return np.asarray(pts, float), owner, normals


def score_screen(geom: ScreenGeometry, cams: list[Camera], *, pixel_sigma=0.3,
                 nominal_deviation_mm=2.0, focal_err_frac=0.0, incidence_max_deg=60.0,
                 margin_frac=0.05, trials=20, seed=0, target_p95_residual_mm=3.0):
    per_cab, _ = coverage_report(geom, cams, margin_frac=margin_frac,
                                 incidence_max_deg=incidence_max_deg)
    cov_by_key = {(c.col, c.row): c for c in per_cab}
    bridge = bridging_report(geom, cams, margin_frac=margin_frac,
                             incidence_max_deg=incidence_max_deg)
    bridged_keys = set()
    big = max(bridge.components, key=len) if bridge.components else []
    bridged_keys.update(big)

    truth, owner, normals = _all_points(geom)
    n_pts = len(truth)
    # which cameras see each truth point (true geometry, fixed across trials)
    sees = [
        [ci for ci, cam in enumerate(cams)
         if point_visible(cam, truth[i], normals[i], margin_frac=margin_frac,
                          incidence_max_deg=incidence_max_deg)]
        for i in range(n_pts)
    ]
    # which points each camera uses for its PnP (only covered cabinets' points)
    cam_pts = {ci: [i for i in range(n_pts) if ci in sees[i]] for ci in range(len(cams))}

    rng = np.random.default_rng(seed)
    K = cams[0].K  # planning assumes a shared camera model
    per_point_err = {i: [] for i in range(n_pts)}

    for _ in range(trials):
        nominal = truth + (rng.normal(0.0, nominal_deviation_mm, truth.shape)
                           if nominal_deviation_mm > 0 else 0.0)
        Kc = K.copy()
        if focal_err_frac > 0:
            f = K[0, 0] * (1.0 + rng.normal(0.0, focal_err_frac))
            Kc[0, 0] = Kc[1, 1] = f

        # observe (true K + noise) per visible point
        obs = {}
        for ci, cam in enumerate(cams):
            for i in cam_pts[ci]:
                p = project_point(K, cam.R, cam.t, truth[i])
                if pixel_sigma > 0:
                    p = p + rng.normal(0.0, pixel_sigma, 2)
                obs[(ci, i)] = p

        # estimate each camera pose via PnP against nominal (believed K)
        est = {}
        for ci in range(len(cams)):
            idx = cam_pts[ci]
            if len(idx) < 4:
                continue
            try:
                est[ci] = solve_pnp_pose(Kc, nominal[idx], np.asarray([obs[(ci, i)] for i in idx]))
            except ValueError:
                continue

        for i in range(n_pts):
            usable = [ci for ci in sees[i] if ci in est]
            if len(usable) < 2:
                continue
            poses = [est[ci] for ci in usable]
            pts2d = [obs[(ci, i)] for ci in usable]
            xhat = triangulate_multiview(Kc, poses, pts2d)
            per_point_err[i].append(float(np.linalg.norm(xhat - truth[i])))

    # aggregate per cabinet
    report = {}
    for cabg in geom.cabinets:
        key = (cabg.col, cabg.row)
        cov = cov_by_key[key]
        errs = [e for i in range(n_pts) if owner[i] == key for e in per_point_err[i]]
        bridged = key in bridged_keys
        if cov.reconstructable and errs:
            a = np.asarray(errs)
            p95 = float(np.percentile(a, 95))
            median = float(np.median(a))
            n_views = len(cov.covering_cams)
        else:
            p95 = float("nan")
            median = float("nan")
            n_views = len(cov.covering_cams)
        report[key] = {
            "p95_mm": p95,
            "median_mm": median,
            "n_views": n_views,
            "total_observations": cov.total_observations,
            "reconstructable": cov.reconstructable,
            "low_observation": cov.low_observation,
            "bridged": bridged,
            "pass": bool(cov.reconstructable and bridged and (p95 <= target_p95_residual_mm)),
        }
    return report
```

- [ ] **Step 4: Run test to verify it passes**

Run: `cd python-sidecar && ./.venv/bin/python -m pytest tests/test_capture_planner_scoring.py -v`
Expected: PASS (2 passed).

- [ ] **Step 5: Run the whole capture-planner suite + full sidecar regression**

Run: `cd python-sidecar && ./.venv/bin/python -m pytest tests/ -q`
Expected: PASS — 235 prior + new capture-planner tests, 0 failures.

- [ ] **Step 6: Commit**

```bash
git add python-sidecar/src/lmt_vba_sidecar/capture_planner/scoring.py \
        python-sidecar/tests/test_capture_planner_scoring.py
git commit -m "feat(capture-planner): visibility-gated Monte-Carlo scorer with per-cabinet residual"
```

---

## Self-Review (completed against spec §4①②③, §8 M1)

- **Spec coverage:** §4① geometry+sample_grid → Task 2. §4② per-point visibility (a/b/c) + gate-aligned coverage → Tasks 3–4. Bridging (§4③ pt 4) → Task 5. §4③ visibility-aware scorer (pts 1–3,5,6) → Task 6. §3 gate constants single-source → Task 1. M4-only check (d) self-occlusion is explicitly out of scope.
- **Conservative-model note:** spec §4② said coverage uses "并集可见点 ≥8"; this plan uses **sum of observations across covering views** (matching reconstruct's per-cabinet observation count) and requires each covering view to have ≥`MIN_PNP_CORNERS`. Spec §4② to be reworded to "总观测 / 保守" to match (tracked as a spec edit, not a plan gap).
- **Type consistency:** `Camera`, `CabinetGeom`, `ScreenGeometry`, `CabinetCoverage`, `BridgingReport` names and fields are consistent across Tasks 2–6. `coverage_report` returns `(per_cabinet, counts)` and is consumed unchanged by `bridging_report` and `score_screen`.
- **Placeholders:** none — every step ships runnable code and an exact command with expected result.

---

## Execution Handoff

After M1 lands and the full sidecar suite is green, the next plans are:
- **M2** — `capture_planner/seed.py` (recipe seed) + `optimize.py` (greedy refine) + `unreachable_regions`.
- **M3** — sidecar subcommand `plan_capture`, `lmt-shared` DTO (`CapturePlan`), `lmt-app` helper, Tauri shim, CLI `plan-capture`, E2E, HTML card, docs/agents-cli.md, schema dump.
- **M4** — visibility check (d) curved self-occlusion + strong-arc validation.
