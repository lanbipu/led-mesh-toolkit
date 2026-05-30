# Capture Guidance Planner — M4 Implementation Plan (curved self-occlusion)

> **For agentic workers:** REQUIRED SUB-SKILL: superpowers:executing-plans. Steps use checkbox (`- [ ]`).

**Goal:** Add visibility check **(d) curved self-occlusion** so the planner stops being optimistic about strong arcs: on a cylindrical wall, a near arc segment can occlude a far sample point that is itself front-facing (check (c) only culls back-facing). Validate that a strong arc reports far cabinets as not-covered / unreachable instead of all-pass.

**Architecture:** `expand_screen` emits an `ArcOccluder` (cylinder axis `(cx, R)` in XZ + screen angular range) on `ScreenGeometry` (None for flat). `point_visible` gains an optional `arc=` param; when present it runs an analytic segment↔circle test in the XZ plane and treats an intersection nearer than the target — whose angle falls inside the screen arc — as occlusion. `vis_count`/`coverage_report`/`score_screen` thread `arc` through (read from `geom.arc_occluder`). Defaults preserve all existing flat behavior.

**Geometry:** nominal wall point at arc angle `a` sits at `(cx + R·sin a, R − R·cos a)` (relative to axis `C=(cx,R)`: `(R sin a, −R cos a)`, so its angle is `atan2(dx, −dz) = a`). Screen range `a ∈ [−W/2R, +W/2R]`. Camera at `P`, target `Q` on the circle (`t=1`). Solve `|P + t(Q−P) − C|² = R²`; if the other root `t ∈ (ε, 1−ε)` and the intersection's angle is within the screen range → the wall body is between camera and target → occluded.

**Scope:** spec §4②(d), §8 M4. Curved only; flat unaffected. Folded still unsupported.

**Run env:** worktree `python-sidecar/`, `./.venv/bin/python`.

---

## Task 1: ArcOccluder + check (d) in `point_visible` + threading

**Files:** `geometry.py`, `visibility.py`, `tests/test_capture_planner_visibility.py`

- [ ] **Step 1: Failing tests** (append to visibility test file)

```python
from lmt_vba_sidecar.capture_planner.geometry import ArcOccluder
from lmt_vba_sidecar.capture_planner.visibility import point_visible as pv


def test_arc_occlusion_blocks_far_point_from_end_camera():
    # Strong arc: 6m wide wall, tight 2.5m radius (wraps ~137 deg). A camera off
    # the far LEFT, low, looking across — the near (left) arc occludes a
    # front-facing point near the RIGHT end.
    radius = 2500.0
    width = 6000.0
    arc = ArcOccluder(cx=width / 2.0, cz=radius, radius=radius,
                      a_min=-width / (2 * radius), a_max=width / (2 * radius))
    # right-end wall point at a = +W/2R
    a = width / (2 * radius)
    import numpy as np
    q = np.array([arc.cx + radius * np.sin(a), 250.0, radius - radius * np.cos(a)])
    n = np.array([np.sin(a), 0.0, np.cos(a)])
    # camera far to the left, in front, aimed at q
    from lmt_vba_sidecar.capture_planner.visibility import intrinsics_from_fov, look_at_camera
    K = intrinsics_from_fov((3840, 2160), hfov_deg=70.0)
    cam = look_at_camera(K, [-4000.0, 250.0, 3500.0], q, (3840, 2160))
    # without (d) it may pass (a)(b)(c); with (d) it must be occluded
    assert pv(cam, q, n, arc=arc) is False


def test_arc_occlusion_does_not_block_frontal_view():
    # Same wall, a frontal centered camera sees the apex point unoccluded.
    radius = 2500.0
    width = 6000.0
    arc = ArcOccluder(cx=width / 2.0, cz=radius, radius=radius,
                      a_min=-width / (2 * radius), a_max=width / (2 * radius))
    import numpy as np
    q = np.array([arc.cx, 250.0, 0.0])               # apex (a=0)
    n = np.array([0.0, 0.0, 1.0])
    from lmt_vba_sidecar.capture_planner.visibility import intrinsics_from_fov, look_at_camera
    K = intrinsics_from_fov((3840, 2160), hfov_deg=70.0)
    cam = look_at_camera(K, [arc.cx, 250.0, 5000.0], q, (3840, 2160))
    assert pv(cam, q, n, arc=arc) is True
```

- [ ] **Step 2:** Run → FAIL (`ArcOccluder` undefined / `arc=` kwarg unknown).

- [ ] **Step 3a: `geometry.py`** — add `ArcOccluder` and emit it from `expand_screen`:

```python
@dataclass(frozen=True)
class ArcOccluder:
    cx: float
    cz: float
    radius: float
    a_min: float
    a_max: float
```
Add field to `ScreenGeometry`: `arc_occluder: "ArcOccluder | None" = None`. In `expand_screen`, after computing `radius`:
```python
    arc_occluder = None
    if radius is not None:
        half = total_w / 2.0  # total_w = cab.cols * cw_mm
        arc_occluder = ArcOccluder(cx=half, cz=radius, radius=radius,
                                   a_min=-half / radius, a_max=half / radius)
```
(Compute `total_w` before the return; pass `arc_occluder=arc_occluder` into `ScreenGeometry(...)`.)

- [ ] **Step 3b: `visibility.py`** — add the occlusion helper + `arc=` param on `point_visible`:

```python
import math

def _arc_occludes(arc, cam_center, p) -> bool:
    # XZ-plane segment vs cylinder cross-section (axis vertical).
    px, pz = float(cam_center[0]), float(cam_center[2])
    qx, qz = float(p[0]), float(p[2])
    dx, dz = qx - px, qz - pz
    fx, fz = px - arc.cx, pz - arc.cz
    a = dx * dx + dz * dz
    if a < 1e-9:
        return False
    b = 2.0 * (fx * dx + fz * dz)
    c = fx * fx + fz * fz - arc.radius * arc.radius
    disc = b * b - 4.0 * a * c
    if disc <= 0.0:
        return False
    sq = math.sqrt(disc)
    for t in ((-b - sq) / (2.0 * a), (-b + sq) / (2.0 * a)):
        if 1e-4 < t < 1.0 - 1e-3:
            ix = px + t * dx
            iz = pz + t * dz
            ang = math.atan2(ix - arc.cx, -(iz - arc.cz))
            if arc.a_min - 1e-6 <= ang <= arc.a_max + 1e-6:
                return True
    return False
```
In `point_visible`, add `arc=None` to the signature and, right before the final `return`, insert:
```python
    if arc is not None:
        cam_center = -cam.R.T @ cam.t
        if _arc_occludes(arc, cam_center, p):
            return False
```
(Note `cam_center` is already computed for incidence — reuse it; don't compute twice.)

- [ ] **Step 3c: thread `arc`** through `vis_count` and `coverage_report`:
- `vis_count(cam, cabg, *, margin_frac=0.05, incidence_max_deg=60.0, arc=None)` → pass `arc=arc` into `point_visible`.
- `coverage_report(geom, cams, ...)` → call `vis_count(..., arc=geom.arc_occluder)`.

- [ ] **Step 4:** Run → PASS. `./.venv/bin/python -m pytest tests/test_capture_planner_visibility.py -q` (existing 11 + 2 new = 13).

- [ ] **Step 5: Commit** `feat(capture-planner): curved self-occlusion visibility (check d)`.

---

## Task 2: scorer uses (d) + strong-arc validation

**Files:** `scoring.py`, `tests/test_capture_planner_scoring.py`

- [ ] **Step 1:** In `scoring.py::score_screen`, pass `arc=geom.arc_occluder` into the `point_visible(...)` call inside the `sees` comprehension (so the Monte-Carlo path is occlusion-aware too).

- [ ] **Step 2: Failing test** (append to scoring test)

```python
def test_strong_arc_far_end_not_optimistically_covered():
    # A strong wide arc with only frontal-ish cameras: the far ends must NOT all
    # pass (self-occlusion + grazing make them under-observed), proving the
    # planner is honest rather than optimistic.
    from lmt_vba_sidecar.ipc import CabinetArray
    from lmt_vba_sidecar.capture_planner.geometry import expand_screen
    cab = CabinetArray(cols=10, rows=1, cabinet_size_mm=[500.0, 500.0], absent_cells=[])
    geom = expand_screen(cab, {"curved": {"radius_mm": 2200.0}}, sample_grid=(4, 4))
    K = intrinsics_from_fov((3840, 2160), hfov_deg=60.0)
    cams = _ring(geom, K, n=3, span_deg=30.0, dist=4000.0)
    report = score_screen(geom, cams, pixel_sigma=0.3, nominal_deviation_mm=1.0,
                          trials=6, seed=0, target_p95_residual_mm=3.0)
    assert not all(v["pass"] for v in report.values())   # far ends not all covered
```

- [ ] **Step 3:** Run → PASS (with (d) wired, the far ends fail; without it they'd falsely pass).

- [ ] **Step 4:** Full sidecar regression: `./.venv/bin/python -m pytest tests/ -q` → 0 failures.

- [ ] **Step 5: Commit** `feat(capture-planner): occlusion-aware scorer + strong-arc honesty test`.

---

## Task 3: end-to-end strong-arc card eyeball (optional, no commit)

- [ ] Render a strong-arc card via the CLI and confirm the far cabinets show red (`不可重建/断链`) and `unreachable_regions` is non-empty — the failure-honesty the whole tool exists for.

---

## Self-Review (against spec §4②(d), §8 M4)

- **Coverage:** (d) occlusion in `point_visible` (Task 1), threaded through coverage + scorer (Tasks 1–2), strong-arc honesty test (Task 2). Flat path untouched (arc=None default) — existing tests must stay green.
- **Limitation:** analytic single-cylinder occluder; irregular masks / multi-segment folds not modeled (flagged in spec §9). Conservative: if uncertain, the angle-range gate simply doesn't fire (no false occlusion on shallow arcs).
- **After M4:** the capture-guidance feature is complete (M1–M4); only the deferred GUI 3D overlay (consuming the same `CapturePlan`) remains, out of this feature's scope.
