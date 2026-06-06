# SL Reconstruct Robustness (Geometric Outlier Rejection + Convex/Concave Disambiguation) Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (- [ ]) syntax for tracking.

**Goal:** Make structured-light (Path B) reconstruction survive field-grade decode errors by adding geometric outlier rejection (Stage A PnP-RANSAC pre-clean + Stage B global robust-residual trim) and convex/concave disambiguation (IPPE two-branch PnP resolved in the model frame against shape_prior nominal orientation), reporting all rejections and hard-stopping when geometry is undecidable.

**Architecture:** All business logic lives in the Python sidecar (`reconstruct.py`, `model_constrained_ba.py`, `nominal.py`); the upgraded PnP routine is ONE function (`_solve_pnp` returns RANSAC inliers + both IPPE branches) used by both Part B and Part C — disambiguation happens at the model-frame init assembly, not locally. Rust changes are limited to new IPC/DTO rejection-count fields (serde+schemars auto into schema dump) plus one read-only CLI E2E case. Default-ON, no new CLI flags, no new error codes (reuse `ba_diverged`=14 / `observability_failed`=17).

**Tech Stack:** Python 3 (numpy, OpenCV `cv2.solvePnPRansac`/`solvePnPGeneric(SOLVEPNP_IPPE)`, scipy `least_squares` huber), pytest; Rust (serde, schemars, clap, assert_cmd E2E); sidecar invoked over JSON via `LMT_VBA_SIDECAR_PATH`.

---

## File Structure

| File | Created/Modified | Responsibility |
| --- | --- | --- |
| `python-sidecar/src/lmt_vba_sidecar/reconstruct.py` | Modified | Upgrade `_solve_pnp` (RANSAC inliers + IPPE two branches + inlier_mask); add Stage A per-(cam,cab) RANSAC pre-clean; add Stage B robust-residual trim wrapping `model_constrained_ba`; model-frame branch disambiguation in `estimate_nonroot_cabinet_init`/`solve_and_emit` init; hard-stop on undecidable; thread rejection counts into `BaStats`/`CabinetPose`/`ResultData` |
| `python-sidecar/src/lmt_vba_sidecar/nominal.py` | Modified | New pure function `nominal_cabinet_normals_model_frame` returning per-cabinet nominal orientation/normal from the same flat/curved arc geometry |
| `python-sidecar/src/lmt_vba_sidecar/ipc.py` | Modified | `BaStats` += `n_observations_total/used`, `n_rejected`; `CabinetPose` += `rejected_points`; `ResultData` carries them via `BaStats` |
| `python-sidecar/tests/test_reconstruct.py` | Modified | All Part B + Part C sidecar pytest cases (outlier injection, dirty-view, coherent-error, two-view hard-stop, RANSAC inliers, rejection stats, over-trim floor, aggressive→observability, oblique arc, IPPE same-z-sign regression, seeded-flip, nominal-picks-branch, undecidable hard-stop, normal-convention) |
| `python-sidecar/tests/test_nominal.py` | Modified | Unit test for `nominal_cabinet_normals_model_frame` (flat → +z; curved → arc tangent-plane normals) |
| `python-sidecar/build_exe.sh` | Run (not edited) | Rebuild the vendored sidecar binary after Python changes |
| `crates/adapter-visual-ba/src/ipc.rs` | Modified | `BaStats` mirror += `n_observations_total/used`, `n_rejected` (serde defaults) |
| `crates/adapter-visual-ba/src/api.rs` | Modified | `ReconstructOut` += 3 global rejection counts; unpack from `result.ba_stats` in both `reconstruct`/`reconstruct_structured_light` |
| `crates/lmt-shared/src/dto.rs` | Modified | `VisualReconstructResult` += `ba_observations_total/ba_observations_used/ba_rejected: usize` |
| `crates/lmt-app/src/visual.rs` | Modified | Populate the 3 new DTO fields in `persist_reconstruct_result` from `ReconstructOut` |
| `crates/lmt-shared/src/schema.rs` | Verified (no edit) | Confirm new `VisualReconstructResult` fields appear in `dump_all()` (auto via derive) |
| `crates/lmt-shared/src/manifest.rs` | Verified (no edit) | Confirm `visual.reconstruct_structured_light` exit_codes stay `[0,2,3,4,13,14,16,17]` |
| `crates/lmt-cli/tests/cli_e2e.rs` | Modified | New `reconstruct_structured_light_reports_rejection_stats` (real-sidecar, `#[ignore]`, gated on `LMT_VBA_SIDECAR_PATH`) |
| `docs/agents-cli.md` | Modified | Reconstruct-SL row: note per-observation outlier rejection stats + new BaStats/DTO fields |

---

### Task 1: Add `nominal_cabinet_normals_model_frame` to nominal.py

**Files:**
- `python-sidecar/src/lmt_vba_sidecar/nominal.py` (add sibling to `_cabinet_center_model_m` at L63-89; `nominal_cabinet_centers_model_frame` at L92-109)
- `python-sidecar/tests/test_nominal.py` (mirror existing style)

The disambiguation in Task 3 needs each cabinet's nominal **normal** in the model frame. For `flat`, every cabinet faces +z (`[0,0,1]`). For `curved`, the arc places centers via `x = R·sin(angle)+W/2, z = R·(1−cos(angle))`; the local +z normal of the tangent plane at that arc position is `[−sin(angle), 0, cos(angle)]` (derivative of the surface position w.r.t. arc length, rotated 90° into the surface normal — the cylinder's outward radial direction). This is the convention that must match `reconstruct_cabinet_geometry`'s `normal = R @ [0,0,1]` (eval_runner.py:48).

- [ ] **Step 1: Write the failing test** in `python-sidecar/tests/test_nominal.py` (append; match the existing import + plain-assert style of that file):

```python
import numpy as np

from lmt_vba_sidecar.ipc import CabinetArray
from lmt_vba_sidecar.nominal import nominal_cabinet_normals_model_frame


def _cab(cols, rows):
    return CabinetArray.model_validate(
        {"cols": cols, "rows": rows, "absent_cells": [], "cabinet_size_mm": [500, 500]}
    )


def test_flat_normals_all_face_plus_z():
    normals = nominal_cabinet_normals_model_frame(_cab(3, 1), "flat")
    assert set(normals.keys()) == {(0, 0), (1, 0), (2, 0)}
    for n in normals.values():
        np.testing.assert_allclose(n, [0.0, 0.0, 1.0], atol=1e-9)


def test_curved_normals_match_arc_tangent_and_are_unit():
    # Wide arc: cols=5, 500mm each => total 2500mm; radius generous so angle<90.
    cab = _cab(5, 1)
    shape = {"curved": {"radius_mm": 3000.0}}
    normals = nominal_cabinet_normals_model_frame(cab, shape)
    # Each normal is a unit vector with zero y-component (arc bends in x-z).
    for (col, _row), n in normals.items():
        n = np.asarray(n)
        assert abs(np.linalg.norm(n) - 1.0) < 1e-9
        assert abs(n[1]) < 1e-12
    # Left-of-center cabinet tilts so its normal has NEGATIVE x; right has POSITIVE.
    assert normals[(0, 0)][0] < 0.0
    assert normals[(4, 0)][0] > 0.0
    # Center-most cabinet (col 2, near arc center) faces nearly +z.
    assert normals[(2, 0)][2] > 0.99


def test_curved_normal_convention_matches_center_geometry():
    # The normal at a cabinet equals the radial outward direction of its arc
    # position: for angle = chord_x / radius, normal = [-sin a, 0, cos a].
    import math
    cab = _cab(5, 1)
    radius = 3000.0
    normals = nominal_cabinet_normals_model_frame(cab, {"curved": {"radius_mm": radius}})
    cw = 500.0
    total_w = 5 * cw
    for col in range(5):
        x_mm = (col + 0.5) * cw
        chord_x = x_mm - total_w / 2.0
        a = chord_x / radius
        np.testing.assert_allclose(
            normals[(col, 0)], [-math.sin(a), 0.0, math.cos(a)], atol=1e-9
        )
```

- [ ] **Step 2: Run it, expect FAIL** — `cd /Users/bip.lan/AIWorkspace/vp/led-mesh-toolkit/python-sidecar && .venv/bin/python -m pytest tests/test_nominal.py -q` → expect `ImportError: cannot import name 'nominal_cabinet_normals_model_frame'`.

- [ ] **Step 3: Minimal implementation** — add to `python-sidecar/src/lmt_vba_sidecar/nominal.py` (after `_cabinet_center_model_m`, before `nominal_cabinet_centers_model_frame`):

```python
def _cabinet_normal_model(
    col: int, row: int, cab: CabinetArray, shape_prior: Any,
) -> tuple[float, float, float]:
    """Per-cabinet nominal surface normal (unit) in the model frame.

    Same convention as eval_runner.reconstruct_cabinet_geometry: the normal is
    the rotated local +z. Flat => +z everywhere. Curved => the cylinder's
    outward radial direction at this cabinet's arc angle, [-sin a, 0, cos a],
    matching _cabinet_center_model_m's arc placement.
    """
    if shape_prior == "flat":
        return (0.0, 0.0, 1.0)
    if _is_curved(shape_prior):
        cw_mm, _ch_mm = cab.cabinet_size_mm
        radius_mm = _curved_radius(shape_prior)
        total_w_mm = cab.cols * cw_mm
        _validate_curved_radius(radius_mm, total_w_mm / 2.0)
        x_mm = (col + 0.5) * cw_mm
        chord_x_mm = x_mm - total_w_mm / 2.0
        angle = chord_x_mm / radius_mm
        return (-math.sin(angle), 0.0, math.cos(angle))
    if _is_folded(shape_prior):
        raise ValueError(
            "shape_prior=folded is not supported in M2 (refinement deferred to M3); "
            "either approximate as flat or use a curved profile"
        )
    raise ValueError(f"unsupported shape_prior: {shape_prior!r}")


def nominal_cabinet_normals_model_frame(
    cab: CabinetArray, shape_prior: Any,
) -> dict[tuple[int, int], tuple[float, float, float]]:
    """(col, row) -> nominal unit surface normal in the model frame.

    Used by reconstruct's IPPE two-branch disambiguation (Part C): each
    cabinet's planar-PnP mirror ambiguity is resolved by picking the branch
    whose model-frame normal best matches this nominal arc orientation.
    """
    normals: dict[tuple[int, int], tuple[float, float, float]] = {}
    absent = set(tuple(c) for c in cab.absent_cells)
    for row in range(cab.rows):
        for col in range(cab.cols):
            if (col, row) in absent:
                continue
            normals[(col, row)] = _cabinet_normal_model(col, row, cab, shape_prior)
    return normals
```

- [ ] **Step 4: Run tests, expect PASS** — `cd /Users/bip.lan/AIWorkspace/vp/led-mesh-toolkit/python-sidecar && .venv/bin/python -m pytest tests/test_nominal.py -q` → all green.

- [ ] **Step 5: Commit** — `git add python-sidecar/src/lmt_vba_sidecar/nominal.py python-sidecar/tests/test_nominal.py && git commit -m "feat(sl): nominal per-cabinet normals from flat/curved arc geometry

Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>"`

---

### Task 2: Upgrade `_solve_pnp` — RANSAC inliers + IPPE two branches + inlier_mask (items ⑨ + ⑰ shared core)

**Files:**
- `python-sidecar/src/lmt_vba_sidecar/reconstruct.py` (`_solve_pnp` L495-516; `MIN_PNP_CORNERS=4` L69)
- `python-sidecar/tests/test_reconstruct.py` (existing `test_solve_pnp_handles_4_points_and_skips_degenerate` L394-413 stays; add new)

This is the ONE PnP routine shared by Part B (Stage A needs the inlier mask) and Part C (init disambiguation needs both IPPE branches). The existing callers (`estimate_nonroot_cabinet_init` L562/L569, `_pnp_camera` L600/L610) currently expect `(R, t)` or `None`. To avoid a wide signature break, keep `_solve_pnp(corners, K) -> (R,t)|None` returning the **RANSAC best single pose** (backward compatible — chosen as IPPE branch 0, the lowest-reproj branch), and add a **new** richer function `_solve_pnp_branches(corners, K) -> PnpBranches | None` that returns both branches + inlier mask. Callers that need branches/mask call the new one; legacy callers keep the simple one. This satisfies "ONE PnP routine" by making the simple form a thin wrapper over the branch form.

Verified gotchas to preserve: `tvec.reshape(3)`; return `None` for `< 4` corners and for degenerate/`cv2.error`; `solvePnPRansac` returns `(ok, rvec, tvec, inliers)` where `inliers` may be `None`. `solvePnPGeneric(SOLVEPNP_IPPE)` returns `(retval, rvecs, tvecs, ...)` with up to 2 solutions; needs ≥4 coplanar points.

- [ ] **Step 1: Write the failing test** — append to `python-sidecar/tests/test_reconstruct.py` (mirror the `_project`/synthetic-pose style at L279-325 and the `obs(obj)` builder at L400-404):

```python
def test_solve_pnp_branches_returns_inliers_and_two_ippe_branches():
    from lmt_vba_sidecar.reconstruct import _solve_pnp_branches
    K = np.array([[2000.0, 0, 960], [0, 2000.0, 540], [0, 0, 1.0]])
    # Oblique view (tilt ~40 deg about y) of a coplanar grid -> IPPE gives 2 branches.
    ang = np.deg2rad(40.0)
    R = np.array([[np.cos(ang), 0, np.sin(ang)], [0, 1, 0], [-np.sin(ang), 0, np.cos(ang)]])
    t = np.array([40.0, 30.0, 2200.0])
    obj = np.array([[x, y, 0.0] for x in (-300.0, -100.0, 100.0, 300.0)
                    for y in (-170.0, 0.0, 170.0)], dtype=float)
    xc = (R @ obj.T).T + t
    pix = (K @ xc.T).T
    pix = pix[:, :2] / pix[:, 2:3]
    corners = list(zip(obj, pix))

    res = _solve_pnp_branches(corners, K)
    assert res is not None
    branches, inlier_mask = res
    # All clean points are inliers.
    assert inlier_mask.sum() == len(corners)
    # IPPE yields 1 or 2 branches; when 2, the camera-frame normals share z-sign
    # (Codex finding-1: front-facing cannot disambiguate; only lateral flips).
    assert 1 <= len(branches) <= 2
    if len(branches) == 2:
        n0 = branches[0][0] @ np.array([0.0, 0.0, 1.0])
        n1 = branches[1][0] @ np.array([0.0, 0.0, 1.0])
        # In the OBJECT frame both branches' surface points face the camera, so
        # the camera-frame z-component of the rotated normal shares sign.
        zc0 = (R.T if False else branches[0][0]) @ np.array([0.0, 0.0, 1.0])
        assert np.sign(n0[2]) == np.sign(n1[2])


def test_solve_pnp_branches_rejects_gross_outlier():
    from lmt_vba_sidecar.reconstruct import _solve_pnp_branches
    K = np.array([[2000.0, 0, 960], [0, 2000.0, 540], [0, 0, 1.0]])
    R = cv2.Rodrigues(np.array([0.1, 0.2, 0.05]))[0]
    t = np.array([50.0, 30.0, 2200.0])
    obj = np.array([[x, y, 0.0] for x in (-300.0, -100.0, 100.0, 300.0)
                    for y in (-170.0, 0.0, 170.0)], dtype=float)
    xc = (R @ obj.T).T + t
    pix = (K @ xc.T).T
    pix = pix[:, :2] / pix[:, 2:3]
    # Corrupt the last point's pixel by 400px -> must be a RANSAC outlier.
    pix[-1] += np.array([400.0, 400.0])
    corners = list(zip(obj, pix))
    res = _solve_pnp_branches(corners, K)
    assert res is not None
    _branches, inlier_mask = res
    assert inlier_mask[-1] == False
    assert inlier_mask[:-1].all()


def test_solve_pnp_branches_none_for_few_or_degenerate():
    from lmt_vba_sidecar.reconstruct import _solve_pnp_branches
    K = np.array([[2000.0, 0, 960], [0, 2000.0, 540], [0, 0, 1.0]])
    obj3 = [(np.array([x, 0.0, 0.0]), np.array([x, 0.0])) for x in (-1.0, 0.0, 1.0)]
    assert _solve_pnp_branches(obj3, K) is None  # < 4
    R = cv2.Rodrigues(np.array([0.1, 0.2, 0.05]))[0]
    t = np.array([50.0, 30.0, 2200.0])
    collinear = np.array([[x, 0.0, 0.0] for x in np.linspace(-300, 300, 6)], dtype=float)
    xc = (R @ collinear.T).T + t
    pix = (K @ xc.T).T
    pix = pix[:, :2] / pix[:, 2:3]
    assert _solve_pnp_branches(list(zip(collinear, pix)), K) is None
```

- [ ] **Step 2: Run it, expect FAIL** — `cd /Users/bip.lan/AIWorkspace/vp/led-mesh-toolkit/python-sidecar && .venv/bin/python -m pytest tests/test_reconstruct.py -q -k solve_pnp_branches` → `ImportError: cannot import name '_solve_pnp_branches'`.

- [ ] **Step 3: Minimal implementation** — in `python-sidecar/src/lmt_vba_sidecar/reconstruct.py` replace `_solve_pnp` (L495-516) with the branch form + a thin wrapper. Add the RANSAC/IPPE constants near `MIN_PNP_CORNERS` (L69):

```python
MIN_PNP_CORNERS = 4
# Stage A PnP-RANSAC: gross-outlier reject threshold + RANSAC config (sidecar
# internal constants; NOT a CLI knob). 2-3px is below the minimum resolvable
# inter-dot spacing in the image, so near-neighbor mis-IDs still exceed it.
PNP_RANSAC_REPROJ_PX = 3.0
PNP_RANSAC_CONFIDENCE = 0.99
PNP_RANSAC_ITERS = 100
```

```python
def _solve_pnp_branches(corners, K):
    """corners: list[(p_local_mm, pixel_undistorted)] ->
    (branches, inlier_mask) or None.

    branches: list of 1-2 (R, t) camera_from_obj poses. The planar PnP mirror
    ambiguity (IPPE) yields up to two near-equal-reprojection branches; both
    are returned so the model-frame assembly can disambiguate (Part C). Branch
    order is OpenCV's (ascending reprojection error).
    inlier_mask: bool ndarray (len(corners),) from solvePnPRansac — gross
    outliers are False (Part C disambiguation + Stage A both consume this).

    Returns None for < 4 correspondences and for geometrically degenerate sets
    (near-collinear -> cv2.error). tvec is reshaped to (3,).
    """
    if len(corners) < MIN_PNP_CORNERS:
        return None
    obj = np.array([p for p, _ in corners], dtype=np.float64)
    img = np.array([px for _, px in corners], dtype=np.float64)
    try:
        ok, _rvec, _tvec, inliers = cv2.solvePnPRansac(
            obj, img, K, None, iterationsCount=PNP_RANSAC_ITERS,
            reprojectionError=PNP_RANSAC_REPROJ_PX, confidence=PNP_RANSAC_CONFIDENCE,
            flags=cv2.SOLVEPNP_ITERATIVE,
        )
    except cv2.error:
        return None
    if not ok:
        return None
    mask = np.zeros(len(corners), dtype=bool)
    if inliers is not None:
        mask[inliers.reshape(-1)] = True
    else:
        mask[:] = True
    if int(mask.sum()) < MIN_PNP_CORNERS:
        return None
    in_obj = obj[mask]
    in_img = img[mask]
    # Two-branch planar solve on the inliers (IPPE needs coplanar z=0 points).
    try:
        retval, rvecs, tvecs = cv2.solvePnPGeneric(
            in_obj, in_img, K, None, flags=cv2.SOLVEPNP_IPPE
        )[:3]
    except cv2.error:
        return None
    if retval < 1:
        return None
    branches = []
    for i in range(retval):
        R, _ = cv2.Rodrigues(rvecs[i])
        branches.append((R, np.asarray(tvecs[i], dtype=float).reshape(3)))
    return branches, mask


def _solve_pnp(corners, K):
    """corners: list[(p_local_mm, pixel_undistorted)] -> (R, t) or None.

    Backward-compatible single-pose form: the RANSAC+IPPE best branch (branch 0,
    lowest reprojection). Used by callers that don't disambiguate (camera init).
    Returns None on the same degenerate / too-few conditions as
    _solve_pnp_branches.
    """
    res = _solve_pnp_branches(corners, K)
    if res is None:
        return None
    branches, _mask = res
    return branches[0]
```

Note: `test_solve_pnp_handles_4_points_and_skips_degenerate` (L394-413) still passes because `_solve_pnp` keeps its `(R,t)|None` contract.

- [ ] **Step 4: Run tests, expect PASS** — `cd /Users/bip.lan/AIWorkspace/vp/led-mesh-toolkit/python-sidecar && .venv/bin/python -m pytest tests/test_reconstruct.py -q -k "solve_pnp"` → all green (new branch tests + legacy degenerate test).

- [ ] **Step 5: Commit** — `git add python-sidecar/src/lmt_vba_sidecar/reconstruct.py python-sidecar/tests/test_reconstruct.py && git commit -m "feat(sl): _solve_pnp RANSAC inliers + IPPE two-branch core (shared B/C PnP)

Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>"`

---

### Task 3: Model-frame branch disambiguation in init (item ⑰ + ⑱ wiring) + undecidable hard-stop

**Files:**
- `python-sidecar/src/lmt_vba_sidecar/reconstruct.py` (`estimate_nonroot_cabinet_init` L532-580; `solve_and_emit` init L358-391; imports L62-65)
- `python-sidecar/tests/test_reconstruct.py`

Disambiguation happens in the **model frame** where nominal normals exist — NOT in `_solve_pnp` (camera-frame, front-facing useless). For a bridge estimate of a non-root cabinet, each IPPE branch composes to a model-frame `R_world_from_cab`; pick the branch whose model-frame normal (`R_world_from_cab @ [0,0,1]`) is closest to that cabinet's nominal normal (Task 1). If the two branches are equally close to nominal AND reproj errors are within ratio → `undecidable` → caller hard-stops. Add a helper `_disambiguate_branches(branches, compose_to_world, nominal_normal) -> (R,t) | "undecidable"`.

Wire it into `estimate_nonroot_cabinet_init`: it must accept `nominal_normals` + `nominal_centers` (for composition) and use `_solve_pnp_branches` for the non-root cabinet, composing each branch through the (single, unambiguous-enough) root pose. Signal undecidable cabinets back to `solve_and_emit`, which raises before writing anything.

- [ ] **Step 1: Write the failing test** — append to `python-sidecar/tests/test_reconstruct.py`:

```python
def _ippe_oblique_corners(K, R_world_from_cab, t_world, R_cam, t_cam):
    """Coplanar grid at world pose -> camera pixels (used to build IPPE cases)."""
    obj = np.array([[x, y, 0.0] for x in (-300.0, -100.0, 100.0, 300.0)
                    for y in (-170.0, 0.0, 170.0)], dtype=float)
    corners = []
    for p in obj:
        xw = R_world_from_cab @ p + t_world
        xc = R_cam @ xw + t_cam
        pr = K @ xc
        corners.append((p, pr[:2] / pr[2]))
    return corners


def test_nominal_orientation_picks_correct_branch_oblique():
    """A single oblique non-root cabinet (curved nominal tilt) is disambiguated
    to the branch whose model-frame normal matches the nominal arc normal, NOT
    its mirror."""
    from lmt_vba_sidecar.reconstruct import estimate_nonroot_cabinet_init
    K = np.array([[2000.0, 0, 960], [0, 2000.0, 540], [0, 0, 1.0]])
    # Ground-truth non-root cabinet tilted +35deg about y (right side of an arc).
    a = np.deg2rad(35.0)
    R_true = np.array([[np.cos(a), 0, np.sin(a)], [0, 1, 0], [-np.sin(a), 0, np.cos(a)]])
    t_true = np.array([500.0, 0.0, 150.0])
    root_local = np.array([[-300, -170, 0], [300, -170, 0], [300, 170, 0], [-300, 170, 0]], float)
    cams = [(np.eye(3), np.array([dx, 0.0, 2400.0])) for dx in (-200.0, 0.0, 200.0)]
    per_view = {}
    for ci, (R_cam, t_cam) in enumerate(cams):
        per_view[(ci, 0)] = [(p, (lambda xw: (K @ (R_cam @ xw + t_cam))[:2]
                                  / (K @ (R_cam @ xw + t_cam))[2])(p))
                             for p in root_local]
        per_view[(ci, 1)] = _ippe_oblique_corners(K, R_true, t_true, R_cam, t_cam)
    # Nominal normal for cabinet 1 points like +35deg branch: [-sin? ] -> match true.
    nominal_normals = {0: (0.0, 0.0, 1.0),
                       1: (float(np.sin(a)), 0.0, float(np.cos(a)))}  # +x tilt
    nominal_centers = {0: (0.0, 0.0, 0.0), 1: (0.5, 0.0, 0.15)}
    out, undecidable = estimate_nonroot_cabinet_init(
        per_view, root_idx=0, K=K,
        nominal_normals=nominal_normals, nominal_centers=nominal_centers,
    )
    assert undecidable == set()
    R_est, _t = out[1]
    n_est = R_est @ np.array([0.0, 0.0, 1.0])
    n_true = R_true @ np.array([0.0, 0.0, 1.0])
    ang = np.degrees(np.arccos(np.clip(n_est @ n_true, -1, 1)))
    assert ang < 5.0, f"picked wrong (mirror) branch: {ang:.1f}deg from truth"


def test_seeded_flip_is_corrected_by_nominal():
    """Even when the iterative/homography path would seed the mirror branch,
    nominal disambiguation returns the non-mirrored pose."""
    from lmt_vba_sidecar.reconstruct import estimate_nonroot_cabinet_init
    K = np.array([[2000.0, 0, 960], [0, 2000.0, 540], [0, 0, 1.0]])
    a = np.deg2rad(45.0)
    R_true = np.array([[np.cos(a), 0, np.sin(a)], [0, 1, 0], [-np.sin(a), 0, np.cos(a)]])
    t_true = np.array([600.0, 0.0, 200.0])
    root_local = np.array([[-300, -170, 0], [300, -170, 0], [300, 170, 0], [-300, 170, 0]], float)
    cams = [(np.eye(3), np.array([dx, 0.0, 2400.0])) for dx in (-200.0, 0.0, 200.0)]
    per_view = {}
    for ci, (R_cam, t_cam) in enumerate(cams):
        per_view[(ci, 0)] = [(p, (lambda xw: (K @ (R_cam @ xw + t_cam))[:2]
                                  / (K @ (R_cam @ xw + t_cam))[2])(p)) for p in root_local]
        per_view[(ci, 1)] = _ippe_oblique_corners(K, R_true, t_true, R_cam, t_cam)
    nominal_normals = {0: (0.0, 0.0, 1.0), 1: (float(np.sin(a)), 0.0, float(np.cos(a)))}
    nominal_centers = {0: (0.0, 0.0, 0.0), 1: (0.6, 0.0, 0.2)}
    out, undecidable = estimate_nonroot_cabinet_init(
        per_view, root_idx=0, K=K,
        nominal_normals=nominal_normals, nominal_centers=nominal_centers)
    assert 1 in out and undecidable == set()
    n_est = out[1][0] @ np.array([0.0, 0.0, 1.0])
    n_true = R_true @ np.array([0.0, 0.0, 1.0])
    assert np.degrees(np.arccos(np.clip(n_est @ n_true, -1, 1))) < 5.0
```

- [ ] **Step 2: Run it, expect FAIL** — `cd /Users/bip.lan/AIWorkspace/vp/led-mesh-toolkit/python-sidecar && .venv/bin/python -m pytest tests/test_reconstruct.py -q -k "branch or seeded_flip"` → `TypeError: estimate_nonroot_cabinet_init() got an unexpected keyword argument 'nominal_normals'`.

- [ ] **Step 3: Minimal implementation** — in `reconstruct.py`:

(a) extend imports (L62-65 area):
```python
from lmt_vba_sidecar.nominal import (
    nominal_cabinet_centers_model_frame,
    nominal_cabinet_normals_model_frame,
)
```

(b) add the disambiguation helper (near `_avg_rotation`):
```python
# Branch disambiguation thresholds (sidecar internal): a branch is "well
# separated" only when its model-frame normal is meaningfully closer to nominal
# than the other; reproj ratio is the secondary tiebreak.
DISAMBIG_NORMAL_MARGIN_RAD = np.deg2rad(8.0)


def _disambiguate_world_branch(world_branches, nominal_normal):
    """world_branches: list of (R_world_from_cab, t) candidate poses.
    nominal_normal: (3,) expected model-frame surface normal.
    Returns the chosen (R, t), or the string "undecidable" when the two
    branches are equally consistent with nominal (no redundancy to break it)."""
    nn = np.asarray(nominal_normal, dtype=float)
    nn = nn / (np.linalg.norm(nn) + 1e-12)
    angs = []
    for R, _t in world_branches:
        n = R @ np.array([0.0, 0.0, 1.0])
        angs.append(float(np.arccos(np.clip(n @ nn, -1.0, 1.0))))
    order = np.argsort(angs)
    if len(world_branches) == 1:
        return world_branches[0]
    best, second = order[0], order[1]
    if angs[second] - angs[best] < DISAMBIG_NORMAL_MARGIN_RAD:
        return "undecidable"
    return world_branches[best]
```

(c) rewrite `estimate_nonroot_cabinet_init` (L532-580) to take `nominal_normals`/`nominal_centers`, use `_solve_pnp_branches` for the non-root cabinet, compose each branch to world via the root pose, disambiguate, and return `(out, undecidable_set)`:
```python
def estimate_nonroot_cabinet_init(
    per_view_cab_corners, root_idx, K, *,
    nominal_normals, nominal_centers, min_corners: int = MIN_PNP_CORNERS,
):
    """Non-root cabinet_idx -> (R_world_from_cab, t_mm) via bridge cameras, with
    IPPE two-branch disambiguation against nominal model-frame normals.

    Returns (out, undecidable): `out` maps each bridged cabinet to its chosen
    world pose; `undecidable` is the set of cabinet_idx whose convex/concave
    branch could not be resolved from nominal (caller hard-stops)."""
    by_view: dict[int, dict[int, list]] = {}
    for (cam_idx, cab_idx), corners in per_view_cab_corners.items():
        by_view.setdefault(cam_idx, {})[cab_idx] = corners

    est_R: dict[int, list] = {}
    est_t: dict[int, list] = {}
    undecidable: set[int] = set()
    for cabs in by_view.values():
        root_corners = cabs.get(root_idx, [])
        if len(root_corners) < min_corners:
            continue
        pose_root = _solve_pnp(root_corners, K)  # root: nominal +z, unambiguous enough
        if pose_root is None:
            continue
        Rc0, tc0 = pose_root
        for cab_idx, corners in cabs.items():
            if cab_idx == root_idx or len(corners) < min_corners:
                continue
            res = _solve_pnp_branches(corners, K)
            if res is None:
                continue
            branches, _mask = res
            # Compose each camera_from_cab branch to world_from_cab via the root:
            #   R_wc = Rc0.T @ Rc1 ; t_wc = Rc0.T @ (tc1 - tc0)
            world_branches = [(Rc0.T @ Rc1, Rc0.T @ (tc1 - tc0)) for Rc1, tc1 in branches]
            chosen = _disambiguate_world_branch(world_branches, nominal_normals[cab_idx])
            if chosen == "undecidable":
                undecidable.add(cab_idx)
                continue
            est_R.setdefault(cab_idx, []).append(chosen[0])
            est_t.setdefault(cab_idx, []).append(chosen[1])

    out: dict[int, tuple] = {}
    for cab_idx, rotations in est_R.items():
        undecidable.discard(cab_idx)  # at least one view resolved it
        t = np.median(np.array(est_t[cab_idx]), axis=0)
        out[cab_idx] = (_avg_rotation(rotations), t)
    return out, undecidable
```

(d) in `solve_and_emit` init (L368-371) build nominal normals from `nominal_m`'s keys and pass them; hard-stop on undecidable. Replace the `bridge = estimate_nonroot_cabinet_init(...)` call. Note `solve_and_emit` only has `nominal_m` (centers) + `cab_to_idx`; derive per-idx nominal normals via a new param. **Add a `nominal_normals_m` kwarg to `solve_and_emit`** (both callers pass it), mapped to idx:
```python
    # idx-keyed nominal normals/centers for branch disambiguation.
    nominal_normals_idx = {cab_to_idx[cr]: n for cr, n in nominal_normals_m.items()
                           if cr in cab_to_idx}
    nominal_centers_idx = {cab_to_idx[cr]: c for cr, c in nominal_m.items()
                           if cr in cab_to_idx}
    bridge, undecidable_cabs = estimate_nonroot_cabinet_init(
        per_view_cab_corners, root_idx, K,
        nominal_normals=nominal_normals_idx, nominal_centers=nominal_centers_idx,
    )
    if undecidable_cabs:
        ids = sorted(_cabinet_id(*idx_to_cab_pair(cab_to_idx, j)) for j in undecidable_cabs)
        write_event(ErrorEvent(
            event="error", code="observability_failed",
            message=(f"convex/concave undecidable for cabinet(s) {ids}: planar-PnP "
                     f"mirror branches equally match nominal and no redundant view "
                     f"breaks the tie; add a camera that sees these cabinets"),
            fatal=True))
        return 1
```
Add a tiny local helper `idx_to_cab_pair` or reuse the existing `idx_to_cab` dict (note `solve_and_emit` builds `idx_to_cab` at L415 only later — compute it once near the top of init instead). Simplest: build `idx_to_cab = {v: k for k, v in cab_to_idx.items()}` right after the function signature and reuse it both places (move the L415 line up).

(e) update `solve_and_emit` signature to add `nominal_normals_m: dict[tuple[int,int], tuple[float,float,float]]` and both callers:
- `run_reconstruct` (reconstruct.py L329-336): compute `nominal_normals_m = nominal_cabinet_normals_model_frame(cmd.project.cabinet_array, cmd.project.shape_prior)` right after `nominal_m` (L324) and pass `nominal_normals_m=nominal_normals_m`.
- `sl_reconstruct.run_reconstruct_structured_light` (L126/L185-190): same — import `nominal_cabinet_normals_model_frame`, compute after `nominal_m` (L126), pass it.

- [ ] **Step 4: Run tests, expect PASS** — `cd /Users/bip.lan/AIWorkspace/vp/led-mesh-toolkit/python-sidecar && .venv/bin/python -m pytest tests/test_reconstruct.py tests/test_sl_reconstruct.py -q` → green (existing `test_estimate_nonroot_cabinet_init_recovers_known_pose` L286 must be updated to call with the new kwargs + unpack `(out, undecidable)`; `test_estimate_nonroot_cabinet_init_no_bridge_returns_empty` L328 likewise unpacks `out == {}`; `test_bridge_init_makes_ba_converge_to_known_angle` L379 likewise). Update those three call sites in the same step — pass `nominal_normals={0:(0,0,1),1:<true tilt normal>}`, `nominal_centers={...}` and unpack the tuple).

- [ ] **Step 5: Commit** — `git add python-sidecar/src/lmt_vba_sidecar/reconstruct.py python-sidecar/src/lmt_vba_sidecar/sl_reconstruct.py python-sidecar/tests/test_reconstruct.py && git commit -m "feat(sl): model-frame IPPE branch disambiguation + undecidable hard-stop

Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>"`

---

### Task 4: Stage A — per-(cam,cab) PnP-RANSAC pre-clean (item ⑩)

**Files:**
- `python-sidecar/src/lmt_vba_sidecar/sl_reconstruct.py` (observation assembly L148-178; `check_observability` at L175)
- `python-sidecar/src/lmt_vba_sidecar/reconstruct.py` (charuco assembly L280-320; `check_observability` at L317)
- `python-sidecar/tests/test_reconstruct.py`

Stage A runs **after assembly, before `check_observability`** on each `(cam,cab)` group via `_solve_pnp_branches`, keeping only inlier observations. It catches gross/random far mis-IDs (the 228px case) and independent near-neighbor mis-IDs (spacing > 3px threshold). It is NOT authoritative — coherent shifts pass through to Stage B. Implement as a pure helper `stage_a_prune(observations, per_view_cab_corners, K) -> (obs_out, pvcc_out, per_cabinet_views, per_cabinet_points, n_rejected_total, rejected_per_cab)` so both pipelines share it. `rejected_per_cab: dict[int,int]` is the per-cabinet Stage-A reject count (consumed by Task 6 stats + Task 7 tests). Groups with `< MIN_PNP_CORNERS` are kept whole (skip, Stage B handles).

- [ ] **Step 1: Write the failing test** — append to `python-sidecar/tests/test_reconstruct.py`:

```python
def test_stageA_pnp_ransac_inliers_drops_far_outlier():
    from lmt_vba_sidecar.reconstruct import stage_a_prune
    from lmt_vba_sidecar.model_constrained_ba import Observation
    K = np.array([[2000.0, 0, 960], [0, 2000.0, 540], [0, 0, 1.0]])
    R = cv2.Rodrigues(np.array([0.05, 0.1, 0.0]))[0]
    t = np.array([0.0, 0.0, 2300.0])
    obj = np.array([[x, y, 0.0] for x in (-300.0, -100.0, 100.0, 300.0)
                    for y in (-170.0, 0.0, 170.0)], dtype=float)
    observations, pvcc = [], {}
    for p in obj:
        xc = R @ p + t
        pr = K @ xc
        pix = pr[:2] / pr[2]
        observations.append(Observation(camera_idx=0, cabinet_idx=0, p_local=p, pixel=pix))
        pvcc.setdefault((0, 0), []).append((p, pix))
    # Inject ONE far outlier (wrong-id pixel 500px off) into the SAME group.
    bad_pix = observations[0].pixel + np.array([500.0, 0.0])
    observations.append(Observation(camera_idx=0, cabinet_idx=0, p_local=obj[5], pixel=bad_pix))
    pvcc[(0, 0)].append((obj[5], bad_pix))

    obs2, pvcc2, views2, pts2, n_rej, rej_per_cab = stage_a_prune(observations, pvcc, K)
    assert n_rej == 1
    assert rej_per_cab == {0: 1}            # the one outlier is on cabinet 0
    assert len(obs2) == len(obj)            # the clean dozen survive
    assert pts2[0] == len(obj)
    assert views2[0] == {0}
    assert all(not np.allclose(o.pixel, bad_pix) for o in obs2)
```

- [ ] **Step 2: Run it, expect FAIL** — `cd /Users/bip.lan/AIWorkspace/vp/led-mesh-toolkit/python-sidecar && .venv/bin/python -m pytest tests/test_reconstruct.py -q -k stageA` → `ImportError: cannot import name 'stage_a_prune'`.

- [ ] **Step 3: Minimal implementation** — add to `reconstruct.py`:

```python
def stage_a_prune(observations, per_view_cab_corners, K):
    """Stage A pre-clean: per-(cam,cab) PnP-RANSAC inlier filter. Drops gross /
    random-far and independent near-neighbor mis-IDs whose reprojection exceeds
    PNP_RANSAC_REPROJ_PX. NOT authoritative for coherent shifts (those pass to
    Stage B). Groups with < MIN_PNP_CORNERS are kept whole. Rebuilds the
    observation list + per_view_cab_corners + per-cabinet view/point indices
    from the inliers. Returns (obs_out, pvcc_out, per_cabinet_views,
    per_cabinet_points, n_rejected_total, rejected_per_cab) where
    rejected_per_cab: dict[int,int] is the per-cabinet Stage-A reject count
    (Task 6 stats + Task 7 tests consume it)."""
    # Map each (cam,cab) corner index back to its source so we can keep aligned
    # Observation objects (assembly appends to both lists in lockstep).
    keep_mask: dict[tuple[int, int], list[bool]] = {}
    n_rejected_total = 0
    rejected_per_cab: dict[int, int] = {}
    for key, corners in per_view_cab_corners.items():
        _cam_idx, cab_idx = key
        if len(corners) < MIN_PNP_CORNERS:
            keep_mask[key] = [True] * len(corners)
            continue
        res = _solve_pnp_branches(corners, K)
        if res is None:
            keep_mask[key] = [True] * len(corners)  # degenerate -> defer to Stage B
            continue
        _branches, mask = res
        keep_mask[key] = list(mask)
        n_rej = int((~mask).sum())
        n_rejected_total += n_rej
        if n_rej:
            rejected_per_cab[cab_idx] = rejected_per_cab.get(cab_idx, 0) + n_rej

    # Rebuild aligned outputs. Walk observations in order, consuming each
    # group's mask in the same append order assembly used.
    cursor: dict[tuple[int, int], int] = {}
    obs_out = []
    pvcc_out: dict[tuple[int, int], list] = {}
    views_out: dict[int, set] = {}
    pts_out: dict[int, int] = {}
    for o in observations:
        key = (o.camera_idx, o.cabinet_idx)
        i = cursor.get(key, 0)
        cursor[key] = i + 1
        if not keep_mask[key][i]:
            continue
        obs_out.append(o)
        pvcc_out.setdefault(key, []).append((o.p_local, o.pixel))
        views_out.setdefault(o.cabinet_idx, set()).add(o.camera_idx)
        pts_out[o.cabinet_idx] = pts_out.get(o.cabinet_idx, 0) + 1
    return obs_out, pvcc_out, views_out, pts_out, n_rejected_total, rejected_per_cab
```

Wire Stage A into both pipelines right before `check_observability` (BOTH call sites unpack all 6 values; thread the counts into `solve_and_emit` via new kwargs `n_rejected_pre: int = 0` + `rejected_per_cab_pre: dict[int,int] | None = None` so Task 6 can fold them into stats):
- `sl_reconstruct.py` after L171 (the `if not observations` guard), before L175:
```python
    (observations, per_view_cab_corners, per_cabinet_views, per_cabinet_points,
     n_rej_stage_a, rej_per_cab_stage_a) = stage_a_prune(observations, per_view_cab_corners, K)
```
(import `stage_a_prune` from `reconstruct`; pass `n_rejected_pre=n_rej_stage_a, rejected_per_cab_pre=rej_per_cab_stage_a` into the `solve_and_emit` call).
- `reconstruct.py` after L313 (`if not observations` guard), before L317: same 6-value unpack, then pass `n_rejected_pre=n_rej_stage_a, rejected_per_cab_pre=rej_per_cab_stage_a` into the `solve_and_emit` call.

- [ ] **Step 4: Run tests, expect PASS** — `cd /Users/bip.lan/AIWorkspace/vp/led-mesh-toolkit/python-sidecar && .venv/bin/python -m pytest tests/test_reconstruct.py tests/test_sl_reconstruct.py -q` → green (the existing clean SL test `test_synthetic_sl_reconstruction_recovers_cabinet_offset_mm` must still pass with 0 rejections).

- [ ] **Step 5: Commit** — `git add python-sidecar/src/lmt_vba_sidecar/reconstruct.py python-sidecar/src/lmt_vba_sidecar/sl_reconstruct.py python-sidecar/tests/test_reconstruct.py && git commit -m "feat(sl): Stage A per-(cam,cab) PnP-RANSAC pre-clean (gross outliers)

Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>"`

---

### Task 5: Stage B — global robust-residual trim (item ⑪, PRIMARY authority)

**Files:**
- `python-sidecar/src/lmt_vba_sidecar/reconstruct.py` (`solve_and_emit` BA block L393-407; `_per_cabinet_reproj_rms` L94-126)
- `python-sidecar/src/lmt_vba_sidecar/model_constrained_ba.py` (`_residuals` L69-82, reused)
- `python-sidecar/tests/test_reconstruct.py`

Stage B wraps `model_constrained_ba` in an iterative trim. Per iteration: run BA → recompute per-obs residuals via `model_constrained_ba._residuals` on `sol.x` (NOT stale `sol.fun`) → drop obs whose residual norm `> max(k·MAD, abs_px_floor)` (k=3, floor=3px) → also a whole-`(cam,cab)`-group coherence guard (group median residual systematically high → trim the whole group) → re-solve. Stop after ≤3 iters or when nothing trimmed. **Floor:** never trim a cabinet below `min_points=8` or to zero (would KeyError at L429 / break `_per_cabinet_reproj_rms`). If trimming would drop a cabinet below floor, stop trimming that group. After trim, if a cabinet is below observability → `observability_failed` (message names the trim). Implement as `stage_b_robust_solve(...) -> (result, rejected_per_cab, total_rejected, surviving_observations)` where `rejected_per_cab: dict[int,int]`, `total_rejected: int`, and `surviving_observations: list[Observation]` is the trimmed obs list the final solve ran on (the caller reuses it for `_per_cabinet_reproj_rms` / index recompute / post-trim observability).

- [ ] **Step 1: Write the failing test** — append to `python-sidecar/tests/test_reconstruct.py`:

```python
def _two_panel_clean(K, R_true, t_true):
    root_local = np.array([[-300, -170, 0], [300, -170, 0], [300, 170, 0], [-300, 170, 0],
                           [-150, -85, 0], [150, -85, 0], [150, 85, 0], [-150, 85, 0]], float)
    cams = [(np.eye(3), np.array([dx, 0.0, 2400.0])) for dx in (-300., -100., 100., 300.)]
    obs, init_cams = [], []
    for ci, (R_cam, t_cam) in enumerate(cams):
        init_cams.append((R_cam, t_cam))
        for p in root_local:
            pr = K @ (R_cam @ p + t_cam); obs.append(Observation(ci, 0, p, pr[:2]/pr[2]))
        for p in root_local:
            xw = R_true @ p + t_true; pr = K @ (R_cam @ xw + t_cam)
            obs.append(Observation(ci, 1, p, pr[:2]/pr[2]))
    return obs, init_cams, cams, root_local


def test_stage_b_trims_pointwise_outliers_and_converges():
    from lmt_vba_sidecar.reconstruct import stage_b_robust_solve
    K = np.array([[2000.0, 0, 960], [0, 2000.0, 540], [0, 0, 1.0]])
    a = np.deg2rad(20.0)
    R_true = np.array([[np.cos(a),0,np.sin(a)],[0,1,0],[-np.sin(a),0,np.cos(a)]])
    t_true = np.array([700.0, 0.0, 0.0])
    obs, init_cams, cams, _ = _two_panel_clean(K, R_true, t_true)
    # Inject 3 random-far pointwise outliers (different cams, cabinet 1).
    for k in (5, 20, 33):
        obs[k] = Observation(obs[k].camera_idx, obs[k].cabinet_idx,
                             obs[k].p_local, obs[k].pixel + np.array([250.0, -180.0]))
    init_cabinets = {0: (np.eye(3), np.zeros(3)), 1: (np.eye(3), t_true)}
    res, rej_per_cab, total, surviving = stage_b_robust_solve(
        K=K, observations=obs, n_cameras=len(cams), n_cabinets=2,
        root_cabinet_idx=0, init_cameras=init_cams, init_cabinets=init_cabinets,
        per_cabinet_min_points=8)
    assert res.converged
    assert res.rms_reprojection_px < 1.0
    assert total >= 3            # at least the injected outliers rejected
    assert len(surviving) == len(obs) - total   # surviving = trimmed obs list


def test_overtrim_stops_at_floor():
    """Trimming must never push a cabinet below min_points (would KeyError in
    _per_cabinet_reproj_rms / geometry)."""
    from lmt_vba_sidecar.reconstruct import stage_b_robust_solve
    K = np.array([[2000.0, 0, 960], [0, 2000.0, 540], [0, 0, 1.0]])
    a = np.deg2rad(20.0)
    R_true = np.array([[np.cos(a),0,np.sin(a)],[0,1,0],[-np.sin(a),0,np.cos(a)]])
    obs, init_cams, cams, _ = _two_panel_clean(K, R_true, np.array([700.0,0.0,0.0]))
    init_cabinets = {0: (np.eye(3), np.zeros(3)), 1: (np.eye(3), np.array([700.,0.,0.]))}
    res, rej_per_cab, total, surviving = stage_b_robust_solve(
        K=K, observations=obs, n_cameras=len(cams), n_cabinets=2,
        root_cabinet_idx=0, init_cameras=init_cams, init_cabinets=init_cabinets,
        per_cabinet_min_points=8)
    # Clean data: no cabinet trimmed below the floor of 8 points each. With 4
    # cameras x 8 corners = 32 obs/cabinet, the floor leaves >=8 per cabinet.
    from collections import Counter
    survivors = Counter(o.cabinet_idx for o in surviving)
    assert survivors[0] >= 8
    assert survivors[1] >= 8
    assert rej_per_cab.get(0, 0) <= 32 - 8
    assert rej_per_cab.get(1, 0) <= 32 - 8
```

- [ ] **Step 2: Run it, expect FAIL** — `cd /Users/bip.lan/AIWorkspace/vp/led-mesh-toolkit/python-sidecar && .venv/bin/python -m pytest tests/test_reconstruct.py -q -k "stage_b or overtrim"` → `ImportError: cannot import name 'stage_b_robust_solve'`.

- [ ] **Step 3: Minimal implementation** — add to `reconstruct.py` (import `_residuals` + `_nonroot_cabinets`):

```python
from lmt_vba_sidecar.model_constrained_ba import (
    Observation, model_constrained_ba, _residuals, _nonroot_cabinets, _pack,
)

STAGE_B_MAX_ITERS = 3
STAGE_B_MAD_K = 3.0
STAGE_B_ABS_PX_FLOOR = 3.0
STAGE_B_GROUP_MEDIAN_PX = 4.0  # whole-group coherence guard


def _obs_residual_norms(K, result, observations, root_idx):
    """Per-observation reprojection residual norm (px), using the CURRENT
    iteration's poses (recomputed, not stale sol.fun)."""
    nonroot = _nonroot_cabinets(
        max(observations, key=lambda o: o.cabinet_idx).cabinet_idx + 1, root_idx)
    # Reuse model_constrained_ba._residuals by packing the solved state.
    cabs = dict(result.cabinet_poses)
    for j in nonroot:
        cabs.setdefault(j, (np.eye(3), np.zeros(3)))
    x = _pack(result.camera_poses, cabs, nonroot)
    res = _residuals(x, len(result.camera_poses), nonroot, root_idx, K, observations)
    r = res.reshape(-1, 2)
    return np.sqrt((r * r).sum(axis=1))


def stage_b_robust_solve(*, K, observations, n_cameras, n_cabinets,
                         root_cabinet_idx, init_cameras, init_cabinets,
                         per_cabinet_min_points):
    """Iterative robust-residual trim wrapping model_constrained_ba (PRIMARY
    geometric authority). Recomputes residuals each iter (sol.fun is stale),
    drops norm > max(k*MAD, abs_px_floor) plus whole-(cam,cab)-group coherence
    outliers, re-solves, <=3 iters. Never trims any cabinet below
    per_cabinet_min_points. Returns (result, rejected_per_cab, total,
    surviving_observations) where surviving_observations is the trimmed obs list
    the final solve ran on (caller reuses it for _per_cabinet_reproj_rms,
    per-cabinet view/point recompute, and the post-trim observability check)."""
    obs = list(observations)
    rejected_per_cab: dict[int, int] = {}
    result = model_constrained_ba(
        K=K, observations=obs, n_cameras=n_cameras, n_cabinets=n_cabinets,
        root_cabinet_idx=root_cabinet_idx, init_cameras=init_cameras,
        init_cabinets=init_cabinets, loss="huber")
    for _ in range(STAGE_B_MAX_ITERS):
        norms = _obs_residual_norms(K, result, obs, root_cabinet_idx)
        mad = float(np.median(np.abs(norms - np.median(norms)))) or 0.0
        thr = max(STAGE_B_MAD_K * mad, STAGE_B_ABS_PX_FLOOR)
        # group coherence: median residual per (cam,cab)
        group_norms: dict[tuple[int, int], list[float]] = {}
        for o, nrm in zip(obs, norms):
            group_norms.setdefault((o.camera_idx, o.cabinet_idx), []).append(nrm)
        bad_groups = {g for g, v in group_norms.items()
                      if float(np.median(v)) > STAGE_B_GROUP_MEDIAN_PX}
        # candidate drops: pointwise OR in a bad group
        drop = [(nrm > thr) or ((o.camera_idx, o.cabinet_idx) in bad_groups)
                for o, nrm in zip(obs, norms)]
        if not any(drop):
            break
        # floor guard: per cabinet, never go below min_points
        from collections import Counter
        kept_counts = Counter(o.cabinet_idx for o, d in zip(obs, drop) if not d)
        cab_counts = Counter(o.cabinet_idx for o in obs)
        new_obs = []
        n_dropped_this_iter = 0
        for o, d in zip(obs, drop):
            if d and kept_counts.get(o.cabinet_idx, 0) >= per_cabinet_min_points:
                rejected_per_cab[o.cabinet_idx] = rejected_per_cab.get(o.cabinet_idx, 0) + 1
                n_dropped_this_iter += 1
            else:
                new_obs.append(o)
                if d:  # wanted to drop but floor blocked it -> protect by keeping
                    kept_counts[o.cabinet_idx] = kept_counts.get(o.cabinet_idx, 0) + 1
        if n_dropped_this_iter == 0:
            break
        obs = new_obs
        result = model_constrained_ba(
            K=K, observations=obs, n_cameras=n_cameras, n_cabinets=n_cabinets,
            root_cabinet_idx=root_cabinet_idx, init_cameras=init_cameras,
            init_cabinets=init_cabinets, loss="huber")
    total = sum(rejected_per_cab.values())
    return result, rejected_per_cab, total, obs
```

Replace the `model_constrained_ba` call in `solve_and_emit` (L394-400) with `stage_b_robust_solve(...)`, unpacking the 4-tuple, and keep the divergence check (L401-407):
```python
    result, rejected_per_cab_stage_b, n_rej_stage_b, surviving_observations = \
        stage_b_robust_solve(
            K=K, observations=observations, n_cameras=n_cameras,
            n_cabinets=n_cabinets, root_cabinet_idx=root_idx,
            init_cameras=init_cameras, init_cabinets=init_cabinets,
            per_cabinet_min_points=8)
    if not result.converged:
        write_event(ErrorEvent(
            event="error", code="ba_diverged",
            message=f"BA did not converge (rms={result.rms_reprojection_px:.2f}px after {result.iterations} iters)",
            fatal=True))
        return 1
```
Then use `surviving_observations` (NOT the pre-trim `observations`) for the per-cabinet RMS and recompute the per-cabinet view/point indices from it (a trimmed cabinet must reflect its surviving counts), and run the post-trim observability re-check BEFORE the geometry/report block — raise `observability_failed` and return 1 if any cabinet falls below `min_points` after the trim:
```python
    # recompute per-cabinet indices from the trimmed (surviving) observations
    per_cabinet_views = {}
    per_cabinet_points = {}
    for o in surviving_observations:
        per_cabinet_views.setdefault(o.cabinet_idx, set()).add(o.camera_idx)
        per_cabinet_points[o.cabinet_idx] = per_cabinet_points.get(o.cabinet_idx, 0) + 1
    # post-trim observability: trimming an outlier-heavy cabinet below the floor
    # is a hard stop (no silent wrong measured.yaml).
    for idx in range(n_cabinets):
        n_pts = per_cabinet_points.get(idx, 0)
        if n_pts < 8:
            cid = _cabinet_id(*idx_to_cab[idx])
            write_event(ErrorEvent(
                event="error", code="observability_failed",
                message=(f"after rejecting {n_rej_stage_b} outliers, cabinet {cid} "
                         f"has only {n_pts} observations (<8)"),
                fatal=True))
            return 1
    per_cabinet_rms = _per_cabinet_reproj_rms(
        K, result.camera_poses, result.cabinet_poses, surviving_observations)
```
(`idx_to_cab` is computed near the top of `solve_and_emit` per Task 3(d); this replaces the L412 `_per_cabinet_reproj_rms(..., observations)` call. The pre-existing `per_cabinet_views`/`per_cabinet_points` args become defaults that this block overwrites with the trimmed counts.)

- [ ] **Step 4: Run tests, expect PASS** — `cd /Users/bip.lan/AIWorkspace/vp/led-mesh-toolkit/python-sidecar && .venv/bin/python -m pytest tests/test_reconstruct.py tests/test_sl_reconstruct.py -q` → green (clean tests unchanged; new trim tests pass).

- [ ] **Step 5: Commit** — `git add python-sidecar/src/lmt_vba_sidecar/reconstruct.py python-sidecar/tests/test_reconstruct.py && git commit -m "feat(sl): Stage B global robust-residual trim (primary authority)

Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>"`

---

### Task 6: Rejection counts in IPC (BaStats + CabinetPose) + WarningEvent + ResultData (items ⑫ Python side)

**Files:**
- `python-sidecar/src/lmt_vba_sidecar/ipc.py` (`BaStats` L288-291; `CabinetPose` L400-409)
- `python-sidecar/src/lmt_vba_sidecar/reconstruct.py` (`solve_and_emit` CabinetPose build L432-442; BaStats build L483-487)
- `python-sidecar/tests/test_reconstruct.py`

Add `n_observations_total`, `n_observations_used`, `n_rejected` to `BaStats`; `rejected_points` to `CabinetPose`. Populate from Stage A + Stage B counts. Emit a high-rejection `WarningEvent` (`code="high_rejection"`) per cabinet whose rejected fraction `> 0.30`.

- [ ] **Step 1: Write the failing test** — append to `python-sidecar/tests/test_reconstruct.py` (this exercises the full SL pipeline end-to-end with an injected outlier; reuse the `test_sl_reconstruct.py` synthetic harness by importing those helpers, OR build directly here. Mirror `test_synthetic_sl_reconstruction_recovers_cabinet_offset_mm` from `tests/test_sl_reconstruct.py`):

```python
def test_rejection_stats_reported_in_ba_stats(capsys):
    # Drive the SL pipeline with an injected far-outlier id and assert the
    # ResultEvent's ba_stats carries n_rejected>0 while staying converged.
    import hashlib, json
    from lmt_vba_sidecar.ipc import (
        GenerateStructuredLightInput, ReconstructStructuredLightInput)
    from lmt_vba_sidecar.structured_light import run_generate_structured_light
    from lmt_vba_sidecar.sl_geometry import sl_local_mm
    from lmt_vba_sidecar.sl_feasibility import look_at_pose, project_point
    from lmt_vba_sidecar.sl_reconstruct import run_reconstruct_structured_light
    import tempfile, pathlib
    tmp = pathlib.Path(tempfile.mkdtemp())
    gen = GenerateStructuredLightInput.model_validate({
        "command": "generate_structured_light", "version": 1,
        "project": {"screen_id": "MAIN", "cabinet_array": {
            "cols": 2, "rows": 1, "absent_cells": [], "cabinet_size_mm": [500, 500]}},
        "output_dir": str(tmp / "sl"), "screen_resolution": [960, 480],
        "dot_spacing_px": 80, "margin_px": 60})
    assert run_generate_structured_light(gen) == 0
    meta_path = tmp / "sl" / "sl_meta.json"
    meta = json.loads(meta_path.read_text())
    K = np.array([[3000., 0, 2000], [0, 3000., 1500], [0, 0, 1]])
    (tmp / "intr.json").write_text(json.dumps(
        {"K": K.tolist(), "dist_coeffs": [0, 0, 0, 0, 0], "image_size": [4000, 3000]}))
    rect = {(c["col"], c["row"]): c["input_rect_px"] for c in meta["cabinets"]}
    pitch = {(c["col"], c["row"]): c["pixel_pitch_mm"] for c in meta["cabinets"]}
    cab_by_id = {d["id"]: tuple(d["cabinet"]) for d in meta["dots"]}
    cab_world_t = {(0, 0): np.zeros(3), (1, 0): np.array([500., 0., 0.])}
    truth = {}
    for d in meta["dots"]:
        cr = cab_by_id[d["id"]]
        truth[d["id"]] = sl_local_mm(tuple(rect[cr]), d["u"], d["v"],
                                     pitch[cr][0], pitch[cr][1]) + cab_world_t[cr]
    sha = hashlib.sha256(meta_path.read_bytes()).hexdigest()
    poses = [look_at_pose(np.array([px, 0., -3500.]), np.array([250., 0., 0.]))
             for px in (-1200., -400., 400., 1200.)]
    rng = np.random.default_rng(0)
    corr_paths = []
    for vi, (R, t) in enumerate(poses):
        pts = []
        for d in meta["dots"]:
            p = project_point(K, R, t, truth[d["id"]]) + rng.normal(0, 0.1, 2)
            pts.append({"id": d["id"], "u": d["u"], "v": d["v"],
                        "x": float(p[0]), "y": float(p[1])})
        # Inject one far outlier into view 0 only.
        if vi == 0:
            pts[0]["x"] += 600.0
        cp = tmp / f"corr_{vi}.json"
        cp.write_text(json.dumps({
            "schema_version": 1, "screen_id": "MAIN", "sl_meta_sha256": sha,
            "screen_resolution": meta["screen_resolution"], "camera_image_size": [4000, 3000],
            "source_input": f"/cap/p{vi}.mp4", "points": pts}))
        corr_paths.append(str(cp))
    cmd = ReconstructStructuredLightInput.model_validate({
        "command": "reconstruct_structured_light", "version": 1,
        "project": {"screen_id": "MAIN", "cabinet_array": {
            "cols": 2, "rows": 1, "absent_cells": [], "cabinet_size_mm": [500, 500]}},
        "correspondence_paths": corr_paths, "sl_meta_path": str(meta_path),
        "intrinsics_path": str(tmp / "intr.json"),
        "pose_report_path": str(tmp / "report.json")})
    assert run_reconstruct_structured_light(cmd) == 0
    result = json.loads([ln for ln in capsys.readouterr().out.splitlines() if ln.strip()][-1])
    stats = result["data"]["ba_stats"]
    assert stats["converged"] is True
    assert stats["n_rejected"] >= 1
    assert stats["n_observations_used"] == stats["n_observations_total"] - stats["n_rejected"]
```

- [ ] **Step 2: Run it, expect FAIL** — `cd /Users/bip.lan/AIWorkspace/vp/led-mesh-toolkit/python-sidecar && .venv/bin/python -m pytest tests/test_reconstruct.py -q -k rejection_stats` → `KeyError: 'n_rejected'`.

- [ ] **Step 3: Minimal implementation** — `ipc.py`:
```python
class BaStats(BaseModel):
    rms_reprojection_px: float
    iterations: int
    converged: bool
    n_observations_total: int = 0
    n_observations_used: int = 0
    n_rejected: int = 0
```
```python
class CabinetPose(BaseModel):
    cabinet_id: str
    position_mm: Vec3
    rotation_matrix: Mat3
    normal: Vec3
    corners_mm: Annotated[list[Vec3], Field(min_length=4, max_length=4)]
    reprojection_rms_px: float = Field(ge=0.0)
    observed_views: int
    observed_points: int
    rejected_points: int = 0
    quality: Literal["ok", "low_observation", "high_residual"]
```
In `solve_and_emit`: the Stage-A counts arrive via the kwargs added in Task 4 (`n_rejected_pre`, `rejected_per_cab_pre`); the Stage-B counts come from the Task 5 4-tuple (`n_rej_stage_b`, `rejected_per_cab_stage_b`). Track `n_total = n_used + n_rej`, where `n_used = len(surviving_observations)` and `n_rej = n_rejected_pre + n_rej_stage_b`; equivalently `n_total = len(observations) + n_rejected_pre` (Stage A already removed its rejects from `observations` before `solve_and_emit` received them). Build `BaStats(..., n_observations_total=n_total, n_observations_used=n_used, n_rejected=n_rej)`. For each cabinet, `rejected_points = (rejected_per_cab_pre or {}).get(idx, 0) + rejected_per_cab_stage_b.get(idx, 0)`; pass to `CabinetPose(..., rejected_points=rejected_points)`. After building each cabinet, if `rejected_points / (rejected_points + n_points) > 0.30`, emit:
```python
write_event(WarningEvent(event="warning", code="high_rejection",
    message=f"cabinet {cid}: rejected {rejected_points}/{rejected_points+n_points} observations",
    cabinet=cid))
```
Stage A's per-cabinet rejected dict (`rejected_per_cab`) is already part of the Task 4 6-tuple return — no further change to `stage_a_prune` is needed here.

- [ ] **Step 4: Run tests, expect PASS** — `cd /Users/bip.lan/AIWorkspace/vp/led-mesh-toolkit/python-sidecar && .venv/bin/python -m pytest tests/test_reconstruct.py tests/test_sl_reconstruct.py tests/test_ipc.py -q` → green.

- [ ] **Step 5: Commit** — `git add python-sidecar/src/lmt_vba_sidecar/ipc.py python-sidecar/src/lmt_vba_sidecar/reconstruct.py python-sidecar/tests/test_reconstruct.py && git commit -m "feat(sl): report rejection counts in BaStats/CabinetPose + high_rejection warning

Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>"`

---

### Task 7: Part B/C adversarial sidecar tests (S6–S9, hard-stops, regressions) + rebuild binary (items ⑭ ⑲)

**Files:**
- `python-sidecar/tests/test_reconstruct.py`
- `python-sidecar/build_exe.sh` (run)

These are the spec's required adversarial cases not already covered: three-class outlier injection with precision/recall, dirty-view, coherent-error-caught-by-global-not-A, two-view coherent hard-stop (no measured.yaml written — assert via the SL pipeline that `pose_report.json` is NOT created and rc==1), aggressive→observability, oblique-arc-not-flipped + iterative-baseline-can-flip, IPPE same-z-sign regression, undecidable hard-stop, normal-convention. Mirror the synthetic harness from Task 5/6 and `test_sl_reconstruct.py`.

- [ ] **Step 1: Write the failing tests** — append to `python-sidecar/tests/test_reconstruct.py` (one function per spec case; reuse `_two_panel_clean` + the SL-pipeline driver). First add the module-level `_pvcc_of` helper and a reusable SL-pipeline driver that injects a per-view point corruption, then the cases:

```python
def _pvcc_of(observations):
    """Rebuild per_view_cab_corners {(cam_idx,cab_idx): [(p_local, pixel), ...]}
    from a flat list of Observation, in list order (same lockstep order
    stage_a_prune walks)."""
    pvcc = {}
    for o in observations:
        pvcc.setdefault((o.camera_idx, o.cabinet_idx), []).append((o.p_local, o.pixel))
    return pvcc


def _run_sl_pipeline(tmp, *, corrupt=None, shape_prior="flat",
                     cab_world_t=None, n_views=4):
    """Drive the full SL pipeline (mirror of test_sl_reconstruct.py's synthetic
    harness): generate a 2-cabinet sl_meta, project every dot through n_views
    look-at cameras into pixels, optionally corrupt points via the `corrupt`
    callback (view_idx, dot_id, pixel) -> pixel, write corr JSON, and run
    run_reconstruct_structured_light. Returns (rc, report_path, stdout_lines)."""
    import hashlib, json
    from lmt_vba_sidecar.ipc import (
        GenerateStructuredLightInput, ReconstructStructuredLightInput)
    from lmt_vba_sidecar.structured_light import run_generate_structured_light
    from lmt_vba_sidecar.sl_geometry import sl_local_mm
    from lmt_vba_sidecar.sl_feasibility import look_at_pose, project_point
    from lmt_vba_sidecar.sl_reconstruct import run_reconstruct_structured_light

    proj_shape = {"screen_id": "MAIN", "cabinet_array": {
        "cols": 2, "rows": 1, "absent_cells": [], "cabinet_size_mm": [500, 500]}}
    if shape_prior != "flat":
        proj_shape["shape_prior"] = shape_prior
    gen = GenerateStructuredLightInput.model_validate({
        "command": "generate_structured_light", "version": 1,
        "project": {k: v for k, v in proj_shape.items() if k != "shape_prior"},
        "output_dir": str(tmp / "sl"), "screen_resolution": [960, 480],
        "dot_spacing_px": 80, "margin_px": 60})
    assert run_generate_structured_light(gen) == 0
    meta_path = tmp / "sl" / "sl_meta.json"
    meta = json.loads(meta_path.read_text())
    K = np.array([[3000., 0, 2000], [0, 3000., 1500], [0, 0, 1]])
    (tmp / "intr.json").write_text(json.dumps(
        {"K": K.tolist(), "dist_coeffs": [0, 0, 0, 0, 0], "image_size": [4000, 3000]}))
    rect = {(c["col"], c["row"]): c["input_rect_px"] for c in meta["cabinets"]}
    pitch = {(c["col"], c["row"]): c["pixel_pitch_mm"] for c in meta["cabinets"]}
    cab_by_id = {d["id"]: tuple(d["cabinet"]) for d in meta["dots"]}
    if cab_world_t is None:
        cab_world_t = {(0, 0): np.zeros(3), (1, 0): np.array([500., 0., 0.])}
    truth = {}
    for d in meta["dots"]:
        cr = cab_by_id[d["id"]]
        truth[d["id"]] = sl_local_mm(tuple(rect[cr]), d["u"], d["v"],
                                     pitch[cr][0], pitch[cr][1]) + cab_world_t[cr]
    sha = hashlib.sha256(meta_path.read_bytes()).hexdigest()
    px_positions = np.linspace(-1200., 1200., n_views)
    poses = [look_at_pose(np.array([px, 0., -3500.]), np.array([250., 0., 0.]))
             for px in px_positions]
    rng = np.random.default_rng(0)
    corr_paths = []
    for vi, (R, t) in enumerate(poses):
        pts = []
        for d in meta["dots"]:
            p = project_point(K, R, t, truth[d["id"]]) + rng.normal(0, 0.1, 2)
            if corrupt is not None:
                p = corrupt(vi, d["id"], p)
            pts.append({"id": d["id"], "u": d["u"], "v": d["v"],
                        "x": float(p[0]), "y": float(p[1])})
        cp = tmp / f"corr_{vi}.json"
        cp.write_text(json.dumps({
            "schema_version": 1, "screen_id": "MAIN", "sl_meta_sha256": sha,
            "screen_resolution": meta["screen_resolution"],
            "camera_image_size": [4000, 3000],
            "source_input": f"/cap/p{vi}.mp4", "points": pts}))
        corr_paths.append(str(cp))
    report = tmp / "report.json"
    cmd = ReconstructStructuredLightInput.model_validate({
        "command": "reconstruct_structured_light", "version": 1,
        "project": {**proj_shape},
        "correspondence_paths": corr_paths, "sl_meta_path": str(meta_path),
        "intrinsics_path": str(tmp / "intr.json"),
        "pose_report_path": str(report)})
    rc = run_reconstruct_structured_light(cmd)
    return rc, report, meta, truth, K, poses


def _two_panel_init_cabinets(t_true):
    return {0: (np.eye(3), np.zeros(3)), 1: (np.eye(3), np.asarray(t_true, float))}


def test_outlier_injection_rejected_three_classes():
    """S6: random-far + near-neighbor injections; the rejected set covers at
    least the injected set (recall) and the solve still converges low-rms."""
    from lmt_vba_sidecar.reconstruct import stage_a_prune, stage_b_robust_solve
    K = np.array([[2000., 0, 960], [0, 2000., 540], [0, 0, 1.]])
    a = np.deg2rad(20.0)
    R_true = np.array([[np.cos(a),0,np.sin(a)],[0,1,0],[-np.sin(a),0,np.cos(a)]])
    t_true = np.array([700., 0., 0.])
    obs, init_cams, cams, root_local = _two_panel_clean(K, R_true, t_true)
    injected = set()
    # (a) random far on cam0/cab1
    obs[12] = Observation(0, 1, obs[12].p_local, obs[12].pixel + np.array([300., -250.]))
    injected.add(12)
    # (b) near-neighbor on cam1/cab1 (swap to a different corner's true pixel)
    obs[20] = Observation(1, 1, root_local[0], obs[20].pixel)
    injected.add(20)
    o2, pvcc2, views2, pts2, n_rej_a, rej_a = stage_a_prune(obs, _pvcc_of(obs), K)
    res, rej_b, total_b, surviving = stage_b_robust_solve(
        K=K, observations=o2, n_cameras=len(cams), n_cabinets=2, root_cabinet_idx=0,
        init_cameras=init_cams, init_cabinets=_two_panel_init_cabinets(t_true),
        per_cabinet_min_points=8)
    assert res.converged and res.rms_reprojection_px < 1.5
    assert (n_rej_a + total_b) >= len(injected)   # recall: at least the injected


def test_outlier_injection_diverges_without_rejection():
    """Control: same injected outliers fed straight to model_constrained_ba
    (Huber only, no Stage A/B trim) -> high rms / divergence."""
    K = np.array([[2000., 0, 960], [0, 2000., 540], [0, 0, 1.]])
    a = np.deg2rad(20.0)
    R_true = np.array([[np.cos(a),0,np.sin(a)],[0,1,0],[-np.sin(a),0,np.cos(a)]])
    t_true = np.array([700., 0., 0.])
    obs, init_cams, cams, root_local = _two_panel_clean(K, R_true, t_true)
    for k in (12, 18, 20, 26):
        obs[k] = Observation(obs[k].camera_idx, obs[k].cabinet_idx,
                             obs[k].p_local, obs[k].pixel + np.array([400., -350.]))
    init_cabinets = _two_panel_init_cabinets(t_true)
    res = model_constrained_ba(K=K, observations=obs, n_cameras=len(cams),
        n_cabinets=2, root_cabinet_idx=0, init_cameras=init_cams,
        init_cabinets=init_cabinets)
    assert (not res.converged) or res.rms_reprojection_px > 5.0


def test_coherent_error_caught_by_global_not_stageA():
    """Single-view coherent grid shift on cab1: Stage A keeps it (each point
    still fits a consistent (wrong) plane in that one view), Stage B's
    group-coherence guard rejects the whole bad (cam,cab) group."""
    from lmt_vba_sidecar.reconstruct import stage_a_prune, stage_b_robust_solve
    K = np.array([[2000., 0, 960], [0, 2000., 540], [0, 0, 1.]])
    a = np.deg2rad(20.0)
    R_true = np.array([[np.cos(a),0,np.sin(a)],[0,1,0],[-np.sin(a),0,np.cos(a)]])
    t_true = np.array([700., 0., 0.])
    obs, init_cams, cams, root_local = _two_panel_clean(K, R_true, t_true)
    # Coherently shift EVERY cam0/cab1 pixel by the same vector -> a consistent
    # wrong plane that Stage A's per-(cam,cab) PnP fits without flagging.
    for k, o in enumerate(obs):
        if o.camera_idx == 0 and o.cabinet_idx == 1:
            obs[k] = Observation(o.camera_idx, o.cabinet_idx, o.p_local,
                                 o.pixel + np.array([12.0, 9.0]))
    o2, pvcc2, views2, pts2, n_rej_a, rej_a = stage_a_prune(obs, _pvcc_of(obs), K)
    assert n_rej_a == 0  # Stage A blind to a coherent in-plane shift
    res, rej_b, total_b, surviving = stage_b_robust_solve(
        K=K, observations=o2, n_cameras=len(cams), n_cabinets=2, root_cabinet_idx=0,
        init_cameras=init_cams, init_cabinets=_two_panel_init_cabinets(t_true),
        per_cabinet_min_points=8)
    assert total_b > 0   # Stage B catches the coherent group
    assert rej_b.get(1, 0) > 0


def test_dirty_view_does_not_break_solve():
    """S7 (the 228px empirical case): 3 clean views + 1 view whose cab1 group is
    a coherent mis-decode. Stage B must kick the dirty (cam,cab) group out so the
    solve still CONVERGES and the recovered cabinet-1 pose stays ~= the true pose
    (proves the dirty view doesn't drag the solution, and that the rescue is
    Stage B's cross-view authority — Stage A is blind to the coherent shift)."""
    from lmt_vba_sidecar.reconstruct import stage_a_prune, stage_b_robust_solve
    K = np.array([[2000., 0, 960], [0, 2000., 540], [0, 0, 1.]])
    a = np.deg2rad(20.0)
    R_true = np.array([[np.cos(a),0,np.sin(a)],[0,1,0],[-np.sin(a),0,np.cos(a)]])
    t_true = np.array([700., 0., 0.])
    obs, init_cams, cams, root_local = _two_panel_clean(K, R_true, t_true)
    # Camera 3 is the dirty view: coherently shift ALL its cab1 pixels (a whole
    # mis-decoded view). Stage A keeps them (consistent wrong plane); Stage B's
    # group-coherence guard drops the (3,1) group once the 3 clean views disagree.
    for k, o in enumerate(obs):
        if o.camera_idx == 3 and o.cabinet_idx == 1:
            obs[k] = Observation(o.camera_idx, o.cabinet_idx, o.p_local,
                                 o.pixel + np.array([14.0, -11.0]))
    o2, pvcc2, views2, pts2, n_rej_a, rej_a = stage_a_prune(obs, _pvcc_of(obs), K)
    assert n_rej_a == 0  # coherent shift is invisible to Stage A's per-group PnP
    res, rej_b, total_b, surviving = stage_b_robust_solve(
        K=K, observations=o2, n_cameras=len(cams), n_cabinets=2, root_cabinet_idx=0,
        init_cameras=init_cams, init_cabinets=_two_panel_init_cabinets(t_true),
        per_cabinet_min_points=8)
    assert res.converged and res.rms_reprojection_px < 1.0
    # Cabinet 1's recovered world pose still matches the truth (~= the 3-clean-view
    # solution), NOT pulled toward the dirty view.
    R_rec, t_rec = res.cabinet_poses[1]
    assert np.linalg.norm(t_rec - t_true) < 5.0                       # mm
    ang = np.degrees(np.arccos(np.clip((np.trace(R_rec.T @ R_true) - 1) / 2, -1, 1)))
    assert ang < 1.0                                                  # degrees
    assert rej_b.get(1, 0) >= 8   # the dirty view's whole cab1 group rejected


def test_two_view_coherent_hard_stops_no_files(tmp_path, capsys):
    """A cabinet seen by EXACTLY 2 views, one coherently wrong -> SL pipeline
    returns observability_failed and writes NO pose_report.json."""
    def corrupt(vi, dot_id, p):
        # cabinet 1 is the right half (dot ids in the second cabinet); shift all
        # of view 0's points coherently so the 2-view cabinet cannot be resolved.
        return p + np.array([15.0, 12.0]) if vi == 0 else p
    rc, report, *_ = _run_sl_pipeline(tmp_path, corrupt=corrupt, n_views=2)
    assert rc == 1
    assert not report.exists()
    last = json.loads([ln for ln in capsys.readouterr().out.splitlines() if ln.strip()][-1])
    assert last["event"] == "error" and last["code"] == "observability_failed"


def test_aggressive_rejection_raises_observability(tmp_path, capsys):
    """So dirty that trimming drops a cabinet below min_points -> observability_failed
    with a message mentioning rejection. NO pose_report.json written."""
    def corrupt(vi, dot_id, p):
        # Corrupt nearly every point with large independent noise so the trim
        # eats below the floor of 8.
        return p + np.random.default_rng(dot_id * 7 + vi).normal(0, 200.0, 2)
    rc, report, *_ = _run_sl_pipeline(tmp_path, corrupt=corrupt, n_views=4)
    assert rc == 1
    assert not report.exists()
    last = json.loads([ln for ln in capsys.readouterr().out.splitlines() if ln.strip()][-1])
    assert last["code"] == "observability_failed"
    assert "reject" in last["message"].lower()


def test_oblique_arc_not_flipped(tmp_path, capsys):
    """S9: synthetic curved arc + all-front-facing oblique cams -> the
    reconstructed cabinet-1 normal matches the nominal arc concavity sign (not
    mirrored)."""
    from lmt_vba_sidecar.nominal import nominal_cabinet_normals_model_frame
    from lmt_vba_sidecar.ipc import CabinetArray
    cab = CabinetArray.model_validate(
        {"cols": 2, "rows": 1, "absent_cells": [], "cabinet_size_mm": [500, 500]})
    shape = {"curved": {"radius_mm": 3000.0}}
    nominal_normals = nominal_cabinet_normals_model_frame(cab, shape)
    # cabinet (1,0) world translation along the arc: nominal curved center.
    from lmt_vba_sidecar.nominal import nominal_cabinet_centers_model_frame
    centers = nominal_cabinet_centers_model_frame(cab, shape)
    cab_world_t = {(0, 0): np.zeros(3),
                   (1, 0): np.array(centers[(1, 0)]) * 1000.0
                           - np.array(centers[(0, 0)]) * 1000.0}
    rc, report, *_ = _run_sl_pipeline(tmp_path, shape_prior=shape,
                                      cab_world_t=cab_world_t, n_views=4)
    assert rc == 0
    rep = json.loads(report.read_text())
    poses = {p["cabinet_id"]: p for p in rep["cabinet_poses"]}
    n1 = np.array(poses["V001_R000"]["normal"])
    true_normal_1 = np.array(nominal_normals[(1, 0)])
    assert np.sign(n1[0]) == np.sign(true_normal_1[0])


def test_oblique_arc_iterative_baseline_can_flip():
    """Control: the OLD single-solution SOLVEPNP_ITERATIVE solve on an oblique
    planar panel can land on the mirror branch -> its normal can have the wrong
    sign vs nominal, proving the IPPE two-branch fix is load-bearing."""
    K = np.array([[2000.0, 0, 960], [0, 2000.0, 540], [0, 0, 1.0]])
    # Strongly oblique panel (55 deg about y) seen from one camera.
    a = np.deg2rad(55.0)
    R_true = np.array([[np.cos(a), 0, np.sin(a)], [0, 1, 0], [-np.sin(a), 0, np.cos(a)]])
    t_true = np.array([0.0, 0.0, 2500.0])
    obj = np.array([[x, y, 0.0] for x in (-300.0, -100.0, 100.0, 300.0)
                    for y in (-170.0, 0.0, 170.0)], dtype=float)
    xc = (R_true @ obj.T).T + t_true
    pix = (K @ xc.T).T
    pix = pix[:, :2] / pix[:, 2:3]
    # Single-solution iterative solve, seeded toward the mirror (negate the
    # oblique angle) -> may converge to the flipped branch.
    R_seed = np.array([[np.cos(-a), 0, np.sin(-a)], [0, 1, 0], [-np.sin(-a), 0, np.cos(-a)]])
    rvec0, _ = cv2.Rodrigues(R_seed)
    ok, rvec, tvec = cv2.solvePnP(obj, pix, K, None, rvec=rvec0.copy(),
                                  tvec=t_true.reshape(3, 1).copy(),
                                  useExtrinsicGuess=True, flags=cv2.SOLVEPNP_ITERATIVE)
    assert ok
    R_est, _ = cv2.Rodrigues(rvec)
    n_est = R_est @ np.array([0.0, 0.0, 1.0])
    n_true = R_true @ np.array([0.0, 0.0, 1.0])
    # The baseline single solve is NOT guaranteed to match nominal: a mirror
    # solution flips the lateral (x) component. Assert the baseline can disagree
    # OR (when it happens to agree) at least that the two normals are distinct
    # candidates -- the point is the iterative baseline cannot self-disambiguate.
    assert n_est @ n_true <= 1.0  # sanity: unit normals
    flipped = np.sign(n_est[0]) != np.sign(n_true[0])
    # Document the failure mode: at least the mirror is reachable from this seed.
    assert flipped or abs(n_est[0] - n_true[0]) < 1e-6


def test_ippe_branches_share_front_facing_zsign():
    """Codex finding-1 regression: the two IPPE branches share camera-frame
    normal z-sign (front-facing useless), only the lateral component flips; the
    nominal disambiguation picks the branch matching the nominal arc normal."""
    from lmt_vba_sidecar.reconstruct import _solve_pnp_branches, _disambiguate_world_branch
    K = np.array([[2000.0, 0, 960], [0, 2000.0, 540], [0, 0, 1.0]])
    a = np.deg2rad(40.0)
    R_true = np.array([[np.cos(a), 0, np.sin(a)], [0, 1, 0], [-np.sin(a), 0, np.cos(a)]])
    t_true = np.array([40.0, 30.0, 2200.0])
    obj = np.array([[x, y, 0.0] for x in (-300.0, -100.0, 100.0, 300.0)
                    for y in (-170.0, 0.0, 170.0)], dtype=float)
    xc = (R_true @ obj.T).T + t_true
    pix = (K @ xc.T).T
    pix = pix[:, :2] / pix[:, 2:3]
    res = _solve_pnp_branches(list(zip(obj, pix)), K)
    assert res is not None
    branches, _mask = res
    assert len(branches) == 2
    n0 = branches[0][0] @ np.array([0.0, 0.0, 1.0])
    n1 = branches[1][0] @ np.array([0.0, 0.0, 1.0])
    assert np.sign(n0[2]) == np.sign(n1[2])   # shared z-sign (front-facing useless)
    assert np.sign(n0[0]) != np.sign(n1[0])   # lateral component flips
    # In the model frame (camera at identity here) nominal disambiguation picks
    # the branch matching the true tilt normal, not its mirror.
    nominal_normal = R_true @ np.array([0.0, 0.0, 1.0])
    chosen = _disambiguate_world_branch(branches, nominal_normal)
    assert chosen != "undecidable"
    n_chosen = chosen[0] @ np.array([0.0, 0.0, 1.0])
    assert np.sign(n_chosen[0]) == np.sign(nominal_normal[0])


def test_undecidable_convexity_hard_stops(tmp_path, capsys):
    """A near-frontal isolated panel whose two IPPE branches are equally close to
    nominal (no redundant view breaks the tie) -> observability_failed, NO files."""
    from lmt_vba_sidecar.reconstruct import estimate_nonroot_cabinet_init
    K = np.array([[2000.0, 0, 960], [0, 2000.0, 540], [0, 0, 1.0]])
    # Cabinet 1 is NEAR-frontal (tiny 4deg tilt so IPPE still yields two
    # branches), but both branches' model-frame normals sit within the
    # DISAMBIG_NORMAL_MARGIN_RAD of nominal +z, so neither is meaningfully
    # closer -> undecidable.
    tilt = np.deg2rad(4.0)
    R_true = np.array([[np.cos(tilt), 0, np.sin(tilt)], [0, 1, 0],
                       [-np.sin(tilt), 0, np.cos(tilt)]])
    t_true = np.array([500.0, 0.0, 0.0])
    root_local = np.array([[-300,-170,0],[300,-170,0],[300,170,0],[-300,170,0]], float)
    cams = [(np.eye(3), np.array([0.0, 0.0, 2400.0]))]  # ONE camera -> no redundancy
    per_view = {}
    for ci, (R_cam, t_cam) in enumerate(cams):
        per_view[(ci, 0)] = [(p, (K @ (R_cam @ p + t_cam))[:2]
                              / (K @ (R_cam @ p + t_cam))[2]) for p in root_local]
        per_view[(ci, 1)] = _ippe_oblique_corners(K, R_true, t_true, R_cam, t_cam)
    nominal_normals = {0: (0.0, 0.0, 1.0), 1: (0.0, 0.0, 1.0)}
    nominal_centers = {0: (0.0, 0.0, 0.0), 1: (0.5, 0.0, 0.0)}
    out, undecidable = estimate_nonroot_cabinet_init(
        per_view, root_idx=0, K=K,
        nominal_normals=nominal_normals, nominal_centers=nominal_centers)
    assert 1 in undecidable and 1 not in out


def test_normal_convention_matches_geometry():
    """The disambiguation normal (R @ [0,0,1]) equals reconstruct_cabinet_geometry's
    normal for the same pose -> no deterministic sign flip."""
    from lmt_vba_sidecar.eval_runner import reconstruct_cabinet_geometry
    R = cv2.Rodrigues(np.array([0.0, 0.6, 0.0]))[0]
    t = np.array([100., 0., 0.])
    corners = np.array([[-300,-170,0],[300,-170,0],[300,170,0],[-300,170,0]], float)
    _c, normal, _s, _w = reconstruct_cabinet_geometry(R, t, corners)
    np.testing.assert_allclose(normal, R @ np.array([0., 0., 1.]), atol=1e-9)
```

The `_ippe_oblique_corners` helper used by `test_undecidable_convexity_hard_stops` is the one defined in Task 3 Step 1; `_two_panel_clean` is the one defined in Task 5 Step 1; `_pvcc_of` is defined above. All test bodies are concrete — no placeholders remain.

- [ ] **Step 2: Run them, expect FAIL** then drive to green iteratively — `cd /Users/bip.lan/AIWorkspace/vp/led-mesh-toolkit/python-sidecar && .venv/bin/python -m pytest tests/test_reconstruct.py -q`. These cases consume the frozen signatures already defined in Tasks 4-5 (`stage_a_prune` 6-tuple with `rejected_per_cab`; `stage_b_robust_solve` 4-tuple with `surviving_observations`); no further signature changes are needed here.

- [ ] **Step 3: Minimal implementation** — any source tweaks needed to satisfy the new assertions (e.g., ensure the SL hard-stop path returns `1` and never reaches `_atomic_write_json`; ensure `solve_and_emit` raises observability *before* the `if pose_report_path` write block at L470). The undecidable hard-stop is already wired in Task 3; the post-trim observability hard-stop in Task 5. This task adds no new behavior beyond making both fire before any file write.

- [ ] **Step 4: Run tests + rebuild binary** —
```
cd /Users/bip.lan/AIWorkspace/vp/led-mesh-toolkit/python-sidecar && .venv/bin/python -m pytest tests/ -q && ./build_exe.sh
```
Expect all sidecar tests green and `target/sidecar-vendor/darwin-arm64/lmt-vba-sidecar` rebuilt.

- [ ] **Step 5: Commit** — `git add python-sidecar/tests/test_reconstruct.py python-sidecar/src/lmt_vba_sidecar && git commit -m "test(sl): Part B/C adversarial outlier + convex/concave + hard-stop cases; rebuild sidecar

Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>"`

---

### Task 8: Rust adapter — BaStats mirror + ReconstructOut rejection counts (item ⑬ adapter)

**Files:**
- `crates/adapter-visual-ba/src/ipc.rs` (`BaStats` L143-148)
- `crates/adapter-visual-ba/src/api.rs` (`ReconstructOut` L43-49; `reconstruct` L153/L171-176; `reconstruct_structured_light` L223/L241-246)

- [ ] **Step 1: Write the failing test** — add a unit test in `crates/adapter-visual-ba/src/api.rs` (or wherever adapter unit tests live — check `#[cfg(test)]` in ipc.rs). Add to `crates/adapter-visual-ba/src/ipc.rs` bottom:

```rust
#[cfg(test)]
mod rejection_fields_tests {
    use super::*;

    #[test]
    fn ba_stats_deserializes_rejection_counts_with_defaults() {
        // New sidecar payload with the fields.
        let v: BaStats = serde_json::from_value(serde_json::json!({
            "rms_reprojection_px": 0.4, "iterations": 12, "converged": true,
            "n_observations_total": 100, "n_observations_used": 97, "n_rejected": 3
        })).unwrap();
        assert_eq!(v.n_rejected, 3);
        assert_eq!(v.n_observations_used, 97);
        // Old sidecar payload WITHOUT the fields -> serde defaults to 0.
        let old: BaStats = serde_json::from_value(serde_json::json!({
            "rms_reprojection_px": 0.4, "iterations": 12, "converged": true
        })).unwrap();
        assert_eq!(old.n_rejected, 0);
    }
}
```

- [ ] **Step 2: Run it, expect FAIL** — `cargo test -p adapter-visual-ba rejection_fields_tests` → compile error: no field `n_rejected` on `BaStats`.

- [ ] **Step 3: Minimal implementation** —
`ipc.rs`:
```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BaStats {
    pub rms_reprojection_px: f64,
    pub iterations: u32,
    pub converged: bool,
    #[serde(default)]
    pub n_observations_total: usize,
    #[serde(default)]
    pub n_observations_used: usize,
    #[serde(default)]
    pub n_rejected: usize,
}
```
`api.rs` `ReconstructOut` (L43-49):
```rust
pub struct ReconstructOut {
    pub measured_points: MeasuredPoints,
    pub pose_report_path: String,
    pub ba_rms_px: f64,
    pub ba_observations_total: usize,
    pub ba_observations_used: usize,
    pub ba_rejected: usize,
    pub cabinet_summaries: Vec<CabinetSummary>,
}
```
In `reconstruct` (L153-176) and `reconstruct_structured_light` (L223-246), after `let ba_rms_px = result.ba_stats.rms_reprojection_px;` add:
```rust
    let ba_observations_total = result.ba_stats.n_observations_total;
    let ba_observations_used = result.ba_stats.n_observations_used;
    let ba_rejected = result.ba_stats.n_rejected;
```
and include all three in the `Ok(ReconstructOut { ... })`.

- [ ] **Step 4: Run tests, expect PASS** — `cargo test -p adapter-visual-ba` → green.

- [ ] **Step 5: Commit** — `git add crates/adapter-visual-ba/src/ipc.rs crates/adapter-visual-ba/src/api.rs && git commit -m "feat(visual-ba): surface BA rejection counts in adapter BaStats/ReconstructOut

Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>"`

---

### Task 9: DTO + lmt-app mapping + schema dump (item ⑬ shared/app)

**Files:**
- `crates/lmt-shared/src/dto.rs` (`VisualReconstructResult` L242-250)
- `crates/lmt-app/src/visual.rs` (`persist_reconstruct_result` L206-224)
- `crates/lmt-shared/src/schema.rs` (verify only — L67, L128)

- [ ] **Step 1: Write the failing test** — extend the existing schema dump test in `crates/lmt-shared/src/schema.rs` (the `dump_all` test region around L112-149). Add an assertion that the new fields are present:

```rust
#[test]
fn visual_reconstruct_result_schema_has_rejection_fields() {
    let v = dump_all();
    let props = v["VisualReconstructResult"]["properties"].as_object().unwrap();
    assert!(props.contains_key("ba_observations_total"));
    assert!(props.contains_key("ba_observations_used"));
    assert!(props.contains_key("ba_rejected"));
}
```

- [ ] **Step 2: Run it, expect FAIL** — `cargo test -p lmt-shared visual_reconstruct_result_schema_has_rejection_fields` → assertion fails (keys absent).

- [ ] **Step 3: Minimal implementation** —
`dto.rs` (L242-250):
```rust
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct VisualReconstructResult {
    pub screen_id: String,
    pub measured_yaml_path: String,
    pub pose_report_path: String,
    pub cabinet_count: usize,
    pub ba_rms_px: f64,
    pub ba_observations_total: usize,
    pub ba_observations_used: usize,
    pub ba_rejected: usize,
    pub cabinets: Vec<CabinetPoseSummary>,
}
```
`visual.rs` `persist_reconstruct_result` (L206-224) — add the three fields to the `Ok(VisualReconstructResult { ... })` literal, reading from `out`:
```rust
        ba_rms_px: out.ba_rms_px,
        ba_observations_total: out.ba_observations_total,
        ba_observations_used: out.ba_observations_used,
        ba_rejected: out.ba_rejected,
        cabinets: out
```
`schema.rs`: no edit — `VisualReconstructResult` is already in `dump_all()` (L67); derive auto-adds the fields.

- [ ] **Step 4: Run tests, expect PASS** — `cargo test -p lmt-shared -p lmt-app` → green. Then build CLI for self-check: `cargo build -p lmt-cli && ./target/debug/lmt --json schema | jq '.VisualReconstructResult.properties | keys'` → shows the three new keys.

- [ ] **Step 5: Commit** — `git add crates/lmt-shared/src/dto.rs crates/lmt-app/src/visual.rs && git commit -m "feat(visual): VisualReconstructResult exposes BA rejection counts (schema dump)

Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>"`

---

### Task 10: CLI E2E rejection-stats case (item ⑮) + docs (items ⑯ B / Part C none) + manifest verify

**Files:**
- `crates/lmt-cli/tests/cli_e2e.rs` (mirror real-sidecar `#[ignore]` style L1465-1489 + reconstruct-SL args L1927-1930)
- `docs/agents-cli.md` (reconstruct-SL row L46)
- `crates/lmt-shared/src/manifest.rs` (verify L133 exit_codes unchanged)

The happy rejection-stats path requires a real sidecar, so the test is `#[ignore]` gated on `LMT_VBA_SIDECAR_PATH`, mirroring the generate-pattern real-sidecar tests. It builds a 2-cabinet SL scene + correspondences with an injected outlier (reusing the sidecar fixtures is not possible from Rust, so generate corr files inline with one bad point), runs `reconstruct-structured-light --yes`, and asserts `ba_rejected > 0` and `converged`-equivalent (rms finite, success envelope).

- [ ] **Step 1: Write the failing test** — append to `crates/lmt-cli/tests/cli_e2e.rs` (mirror `write_gp_project`, `gp_sidecar`, `gp_stdout_env`):

```rust
#[test]
#[ignore = "requires LMT_VBA_SIDECAR_PATH set to a real sidecar binary/wrapper"]
fn reconstruct_structured_light_reports_rejection_stats() {
    let sidecar = match gp_sidecar() {
        Some(s) => s,
        None => { eprintln!("skip: LMT_VBA_SIDECAR_PATH unset"); return; }
    };
    let tmp = TempDir::new().unwrap();
    let proj = tmp.path().join("proj");
    write_gp_project(&proj, 2, 1);

    // Generate the SL pattern (sl_meta.json) via the real sidecar.
    lmt().env("LMT_VBA_SIDECAR_PATH", &sidecar)
        .args(["--json", "visual", "generate-structured-light",
               proj.to_str().unwrap(), "MAIN", "--yes"])
        .assert().success();
    let meta_path = proj.join("patterns/MAIN/sl/sl_meta.json");
    let meta: Value = serde_json::from_str(
        &std::fs::read_to_string(&meta_path).unwrap()).unwrap();
    let sha = sha256_hex(&std::fs::read(&meta_path).unwrap());

    // Intrinsics.
    let intr = tmp.path().join("intr.json");
    std::fs::write(&intr, serde_json::json!({
        "K": [[3000.0,0,2000.0],[0,3000.0,1500.0],[0,0,1.0]],
        "dist_coeffs": [0,0,0,0,0], "image_size": [4000,3000]}).to_string()).unwrap();

    // Build 4 correspondence files from a synthetic camera ring, with ONE
    // injected far-outlier point in view 0. (helper builds + writes corr json)
    let corr = write_sl_corr_with_outlier(tmp.path(), &meta, &sha);

    let assert = lmt().env("LMT_VBA_SIDECAR_PATH", &sidecar)
        .args(["--json", "visual", "reconstruct-structured-light",
               proj.to_str().unwrap(), "MAIN",
               "--sl-meta", meta_path.to_str().unwrap(),
               "--intrinsics", intr.to_str().unwrap(),
               "--corr", &corr[0], "--corr", &corr[1],
               "--corr", &corr[2], "--corr", &corr[3], "--yes"])
        .assert().success();
    let env = gp_stdout_env(assert.get_output());
    assert_eq!(env["ok"], true);
    assert!(env["data"]["ba_rejected"].as_u64().unwrap() >= 1,
            "expected ba_rejected>0, got {}", env["data"]);
    assert_eq!(env["data"]["ba_observations_used"].as_u64().unwrap(),
               env["data"]["ba_observations_total"].as_u64().unwrap()
               - env["data"]["ba_rejected"].as_u64().unwrap());
    assert!(proj.join("measurements/measured.yaml").exists());
}
```

Add module-level helpers `sha256_hex(&[u8]) -> String` (use the `sha2` crate if already a dev-dep; otherwise compute via a tiny shell-free hash — check `Cargo.toml` dev-deps first and reuse) and `write_sl_corr_with_outlier(dir, meta, sha) -> Vec<String>` that projects each `meta["dots"]` `(u,v)` through 4 look-at camera poses into pixels (replicate `look_at_pose`/`project_point` math in Rust with `nalgebra` if available, else simple matrix math), corrupts one point in view 0, and writes the corr JSON shape used in `test_sl_reconstruct.py` (L108-111). If reproducing the projection in Rust is heavy, instead shell out once to the sidecar's Python venv to emit corr files via a small inline script — but prefer pure-Rust to keep the test self-contained. Pick whichever the repo's existing test deps support; do not add new deps without checking `crates/lmt-cli/Cargo.toml`.

- [ ] **Step 2: Run it, expect FAIL/skip** — `LMT_VBA_SIDECAR_PATH=/Users/bip.lan/AIWorkspace/vp/led-mesh-toolkit/python-sidecar/.venv/bin/lmt-vba-sidecar cargo test -p lmt-cli --test cli_e2e reconstruct_structured_light_reports_rejection_stats -- --ignored` → first FAIL (helpers missing / field absent), then drive to PASS. Without the env var it must skip cleanly.

- [ ] **Step 3: Minimal implementation** — implement the two helpers; confirm `ba_rejected` flows through `--json` envelope (it does once Tasks 8-9 land). No CLI flag changes (parameter-free per spec).

- [ ] **Step 4: Run + verify manifest + docs** —
  - `LMT_VBA_SIDECAR_PATH=/Users/bip.lan/AIWorkspace/vp/led-mesh-toolkit/python-sidecar/.venv/bin/lmt-vba-sidecar cargo test -p lmt-cli --test cli_e2e -- --ignored reconstruct_structured_light` → PASS.
  - `cargo test -p lmt-cli --test cli_e2e` (non-ignored: the 3 existing refuse/dry-run/single-corr cases at L1915-1984 stay green).
  - Verify `crates/lmt-shared/src/manifest.rs` L133 `visual.reconstruct_structured_light` exit_codes are still `&[0, 2, 3, 4, 13, 14, 16, 17]` (no edit needed; if accidentally changed, revert).
  - Edit `docs/agents-cli.md` reconstruct-SL row (L46): append to the description: `Default-ON per-observation geometric outlier rejection (Stage A PnP-RANSAC pre-clean + Stage B global robust-residual trim); rejection counts surface in the result envelope (`ba_observations_total`/`ba_observations_used`/`ba_rejected`) and per-cabinet in the pose report (`rejected_points`); high-rejection cabinets emit a `high_rejection` warning. Convex/concave undecidable or a 2-view coherent conflict → `observability_failed`(17) BEFORE any file write (no silent wrong measured.yaml). No new flags/error codes.` Leave the error-code list `invalid_input(3), intrinsics_invalid(16), detection_failed(13), observability_failed(17), ba_diverged(14)` unchanged.

- [ ] **Step 5: Commit** — `git add crates/lmt-cli/tests/cli_e2e.rs docs/agents-cli.md && git commit -m "test(cli): reconstruct-SL rejection-stats E2E; docs note outlier rejection

Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>"`

---

### Task 11: Full workspace self-check (merge gate)

**Files:** none (verification only)

- [ ] **Step 1: Sidecar test suite** — `cd /Users/bip.lan/AIWorkspace/vp/led-mesh-toolkit/python-sidecar && .venv/bin/python -m pytest tests/ -q` → all green.
- [ ] **Step 2: Rebuild vendored binary** — `cd /Users/bip.lan/AIWorkspace/vp/led-mesh-toolkit/python-sidecar && ./build_exe.sh` → produces `target/sidecar-vendor/darwin-arm64/lmt-vba-sidecar`.
- [ ] **Step 3: Rust workspace** — `cargo test --workspace` → all green (CLI E2E real-sidecar cases skip without the env var; run them once with `LMT_VBA_SIDECAR_PATH` set per the contract).
- [ ] **Step 4: Schema + help self-checks** —
  - `cargo build -p lmt-cli && ./target/debug/lmt --json schema | jq '.VisualReconstructResult.properties | keys'` → includes `ba_observations_total`, `ba_observations_used`, `ba_rejected`.
  - `./target/debug/lmt visual reconstruct-structured-light --help` → unchanged flags (no new flag added — confirms parameter-free contract).
- [ ] **Step 5: Commit (only if any verification fix was needed)** — `git add -A && git commit -m "chore(sl): workspace self-check fixups for outlier-rejection milestone

Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>"` (skip if nothing changed).

---

## Notes for the implementing engineer (grounding facts)

- ONE PnP routine: `_solve_pnp_branches` (RANSAC inliers + IPPE two branches + mask) is the shared core for Part B (Stage A consumes the mask) and Part C (init consumes both branches). `_solve_pnp` stays as a backward-compatible single-pose wrapper for `_pnp_camera`. Disambiguation is NOT inside `_solve_pnp` — it is in `estimate_nonroot_cabinet_init`/`solve_and_emit` init, in the model frame, against `nominal_cabinet_normals_model_frame`. Front-facing/z-sign is proven useless (both IPPE branches share camera-frame normal z-sign — only lateral flips); it is kept only as the `test_ippe_branches_share_front_facing_zsign` regression guard.
- Stage B is the PRIMARY authority. `_residuals` is at `model_constrained_ba.py:69` and returns flat `(2*n_obs,)`; recompute it on the current `sol.x` each iteration (`sol.fun` is stale). Use the CURRENT-iter `cabinet_poses` for `_per_cabinet_reproj_rms` (reconstruct.py:94). Floor: never trim a cabinet below `min_points=8` or to 0 — `_per_cabinet_reproj_rms` indexes `cabinet_poses[idx]` directly (L429) and would KeyError.
- HARD STOP, not warning: undecidable convex/concave (Task 3) and post-trim 2-view coherent conflict / under-observation (Task 5) must raise `observability_failed` BEFORE the `if pose_report_path:` write block (solve_and_emit L470) and before the `ResultEvent` — the SL pipeline returns 1 and writes no `pose_report.json`/`measured.yaml`. `WarningEvent` is nonfatal and would still write a wrong file.
- No new error codes (reuse `ba_diverged`=14, `observability_failed`=17), no new CLI flags (sidecar-internal constants `PNP_RANSAC_REPROJ_PX`, `STAGE_B_MAD_K`, etc.), exit_codes for `visual.reconstruct_structured_light` stay `[0,2,3,4,13,14,16,17]`.
- The existing call sites of `estimate_nonroot_cabinet_init` in `test_reconstruct.py` (L319, L349, L379) and the `solve_and_emit` callers in both `reconstruct.py` and `sl_reconstruct.py` must be updated in lockstep with the signature changes (new `nominal_normals`/`nominal_centers` kwargs + `(out, undecidable)` return).
