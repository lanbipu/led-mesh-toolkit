# Spec — Step 1: On-site camera lens calibration from structured-light white dots vs nominal design geometry

- Date: 2026-05-30
- Status: design v2 — revised per Codex adversarial review (2026-05-30); proceeding to implementation plan
- Branch: `worktree-feat+sl-onsite-camera-calibration` (off `origin/main`)
- Scope: Step 1 of the disguise-style as-built reverse-engineering pipeline (calibrate → reconstruct → anchor → export). Steps 2–4 are out of scope here.

## 1. Problem

The on-site as-built reconstruction (`reconstruct-structured-light`) requires a per-camera
intrinsics file (`{K, dist_coeffs, image_size}`) and today the ONLY producer is
`visual calibrate`, which needs a **separate printed-checkerboard session**. That is awkward
on a live LED wall: you have already played a structured-light (SL) white-dot sequence on the
wall for reconstruction — that same sequence should double as the calibration target.

**Goal:** Calibrate a single on-site camera's intrinsics (`fx, fy, cx, cy`) + radial distortion
(`k1, k2`) directly from its SL white-dot capture(s) of the as-built wall, using the project's
**nominal design wall geometry as a known 3D calibration target** (disguise-style). Emit an
intrinsics file in the existing contract so Step 2 consumes it unchanged via `--intrinsics`.

## 2. Why this works — and the one hard constraint

Prior planar self-calibration (PoC, each cabinet treated as a flat Zhang target) **failed on the
principal point**: reproj RMS 3.78px, principal-point uncertainty ±12.7/±8.7px, downstream
verdict "use external checkerboard." A planar target cannot disambiguate focal length from
camera distance, nor pin the principal point, from few views.

The nominal **curved** wall is a genuinely **non-coplanar 3D target**. A 3D target (plus multiple
camera poses) resolves exactly the focal/principal-point ambiguity that killed the planar PoC.
This is the whole reason Step 1 is viable where the earlier self-cal was not.

**Hard constraint (the load-bearing risk):** calibration is only well-conditioned when the dots
a camera sees span enough depth/curvature **or** enough pose diversity. A near-flat patch seen
from a single pose is degenerate. **Step 1 MUST detect degeneracy and refuse — never emit a
confidently-wrong K.** Phase-0 synthetic study sets the accuracy budget: focal must be ≲2%,
principal point within a few px, or the downstream 0.3° angle gate blows.

### 2.1 Calibrating against nominal (which has as-built deviation) — what actually threatens K

We calibrate against the *design* wall, but the *as-built* wall deviates. The original worry (Codex
F1) was that deviation gets *absorbed into K* as a confidently-wrong, low-RMS estimate. We swept this
empirically against the synthetic substrate (radius error 0–15%, global scale ±10%, rigid tilt 0–20°)
and the finding reshapes the risk model:

- **Global structured deviation** (as-built arc radius ≠ nominal, global scale, rigid tilt) is
  geometrically **absorbed into the per-pose extrinsics, NOT into K** — single-camera intrinsic
  calibration can't distinguish "the wall is 2% bigger" from "the camera is 2% farther." In the sweep,
  fx recovers to **~0% error even at 15% radius error** (noise-free). So global deviation is **not** a
  threat to K; it shifts where the solver thinks the camera was, leaving K intact. This is a property
  of the geometry — not something a gate enforces.
- **Random per-cabinet deviation** (~mm–cm) averages out → object-point error ~0.01–0.1% relative,
  well under the 2% focal budget.
- **The real threat to K is UNDER-CONSTRAINT, not bias:** too few / near-duplicate poses, a near-coplanar
  patch, fronto-parallel-only views, a shallow arc, or thin (≈1D) image coverage leave K genuinely
  uncertain (wide covariance), and *that* can land on a wrong value. Gross non-absorbable target error
  instead inflates reproj RMS.

**FAIL-SAFE operating envelope (decided).** The gates are tuned to **refuse an under-constrained
capture rather than emit a confidently-wrong K** — stricter is correct; more refusals on bad geometry
is the goal. Empirically (synthetic substrate, 0.3 px centroid noise) the good vs under-constrained
geometries are cleanly separable, so the gates are pinned to PASS a well-conditioned capture and
REFUSE everything weaker:

- **PASS (the required envelope):** a **multi-row** wall (2D image coverage) seen from **oblique** views
  at **≥2 distinct camera distances**. Such a capture recovers fx to **<0.3%** with focal-std **~0.15–0.20%**
  and pp-std **~2–2.7 px**.
- **REFUSE (under-constrained):** shallow-arc + few/fronto-parallel poses (fx 1–10% wrong, focal-std
  2–9%), 1D / wide-thin coverage (collapsed image axis → fy/cy unconstrained), single-pose noisy,
  near-duplicate poses, near-flat single pose. These are ~10× outside the good geometry's covariance.

This is a **precision + coverage** envelope (covariance gates + 2-axis coverage), **not** a bias
detector: global as-built deviation is absorbed into the extrinsics (above) and correctly does NOT
refuse.

So the safety design targets the threat that actually exists:
1. **RMS gate** catches gross non-absorbable target error (§3.2).
2. **Observability gate, not just RMS** (§3.2): pose/baseline diversity, coverage, and parameter
   covariance / condition number refuse an **under-constrained** solve — the case where K is uncertain.
   This gate detects *variance* (under-constraint), not *bias*; bias from global deviation is a non-issue
   per the sweep above.
3. **K-robustness acceptance test** (§6): the deviation case asserts recovered K stays within the
   focal/pp budget across the deviation range — verifying K's robustness to absorbed deviation (NOT that
   a gate refuses, which it correctly need not). The gate-*refusal* path is pinned separately by the
   under-constraint tests (single-pose covariance, near-flat, near-duplicate).
4. **Non-destructive output** (§3.2/§4): the SL-derived K never overwrites a trusted checkerboard
   intrinsics file by default and records `calibration_method`, so a questionable SL calibration cannot
   silently become Step 2's input.

A single pass against nominal is thus sound for the deviation regimes a real wall exhibits; iterating
calibrate↔reconstruct against the *recovered* as-built geometry remains a possible future refinement,
explicitly **out of scope** here.

## 3. Architecture

Mirror the existing `reconstruct` / `reconstruct-structured-light` sibling split. The new path is
a sibling of `calibrate`, reusing the SL transport machinery of `reconstruct-structured-light`
minus the `--intrinsics` input (we *produce* intrinsics, not consume them).

```
lmt visual calibrate-structured-light <project> <screen_id>
     --sl-meta <sl_meta.json> --corr <c.json> [--corr ...] [--out <path>] [--force]
          │  (clap subcommand, destructive → gate_destructive + --yes/--dry-run)
          ▼
lmt_app::visual::run_calibrate_structured_light(project, screen_id, sl_meta, corrs, out, force)
          │  (service layer: provenance gate, resolve+guard out path, call adapter)
          ▼
adapter-visual-ba::api::calibrate_structured_light  → run_sidecar(subcommand="calibrate_structured_light")
          │  (payload {command, version, project, sl_meta_path, correspondence_paths, output_path})
          ▼
python-sidecar  lmt_vba_sidecar.calibrate_sl::run_calibrate_structured_light(cmd)
          │  1. per-dot nominal 3D world table  2. cv2.calibrateCamera  3. observability gates  4. write intrinsics.json
          ▼
<project>/calibration/<screen_id>_sl_intrinsics.json   (5-key contract + provenance; NON-destructive default)
```

### 3.1 Component: per-dot nominal 3D world table (NEW — the missing piece)

`nominal.py` today gives per-**cabinet** center + normal only; `sl_geometry.sl_local_mm` gives
per-dot **cabinet-local** mm (z=0). Neither chains into per-dot world 3D. New helper:

```
nominal_dot_positions_world(sl_meta, cab: CabinetArray, shape_prior) -> dict[int, np.ndarray]
    # dot_id -> [x, y, z] meters in the model/design frame
```

For each dot in `sl_meta.dots` (with its `cabinet=[col,row]`, `(u,v)`, and that cabinet's
`input_rect_px` + `pixel_pitch_mm`):

```
local_m   = sl_local_mm(rect, u, v, pitch_x, pitch_y) / 1000.0        # [lx, ly, 0] m
center_m  = _cabinet_center_model_m(col, row, cab, shape_prior)        # existing
α         = chord_x / radius     (flat ⇒ α = 0)                        # existing arc angle
world_m   = center_m + R_y(α) · local_m
           where R_y(α) = [[cosα,0,sinα],[0,1,0],[-sinα,0,cosα]]
```

This is **consistent with the existing nominal model**: each cabinet is a rigid flat tile, the
arc is the faceted approximation of tilting each tile by its center's arc angle (R_y(α)·[0,0,1] =
the cabinet normal nominal.py already returns). Flat ⇒ R_y(0)=I ⇒ pure translation. Folded ⇒
fails fast (M2 unsupported), same as nominal.py.

### 3.2 Component: the calibration solver (sidecar `calibrate_sl.py`)

```
run_calibrate_structured_light(cmd) -> writes intrinsics.json, returns {reproj_error_px, frames_used}
```

1. Load `sl_meta` (schema-validated) + project (lmt-shared ProjectConfig variant) → build the
   per-dot nominal 3D world table (3.1).
2. For each correspondence file (one camera pose): `objectPoints[i]` = nominal 3D world of the
   dots decoded in that pose; `imagePoints[i]` = that pose's camera pixels `(x, y)`. Dot identity
   is the `id`; canonical `(u,v)` comes from `sl_meta` (not the corr file), matching how
   `sl_reconstruct` already resolves correspondences.
3. Seed `K0`: focal from EXIF if present else `1.2 × max(image_size)` heuristic; principal point
   at image center. `dist0 = 0`.
4. `cv2.calibrateCamera(objectPoints, imagePoints, image_size, K0, dist0,
   flags=CALIB_USE_INTRINSIC_GUESS | CALIB_ZERO_TANGENT_DIST | CALIB_FIX_K3)` → solve
   `fx, fy, cx, cy, k1, k2`. Use `calibrateCameraExtended` to also get per-intrinsic std-devs.
5. **Observability + quality gates — refuse BEFORE any write** (see §5). A pose *count* is not
   observability: three near-duplicate captures constrain K no better than one. Gate on actual
   constraint, mirroring the gates `calibrate.py` already has for checkerboards:
   - **reproj RMS** ≤ `--max-rms-px` (default **1.5px**; looser than checkerboard's 0.5px — SL dot
     centroids on a live LED wall are noisier: bloom, large dots. Concrete default, tunable).
   - **pose / baseline diversity**: reject near-duplicate captures — require the estimated per-pose
     extrinsics to span a minimum rotation + translation baseline; collapse below threshold ⇒
     `observability_failed`. (calibrate.py uses mean pairwise corner RMS > 5px for the same purpose.)
   - **image-space coverage (BOTH axes)**: detected dots must span ≥ a minimum fraction of the frame on
     the **smaller** image axis (`min(w, h)` of the union bbox, **not** `max`) — a near-1D distribution
     (dots on ~one scanline, or a wide/short wall seen fronto-parallel) passes a `max` gate while the
     collapsed axis is wholly unconstrained → fy/cy garbage. FAIL-SAFE: require 2D coverage. Same intent
     as calibrate.py's corner-coverage gate.
   - **target non-coplanarity OR genuine multi-pose**: smallest/largest singular-value ratio of the
     centered object-point cloud below threshold AND <3 *diverse* poses ⇒ refuse (the planar-PoC
     degeneracy: a flat patch from one viewpoint).
   - **parameter observability** (the gate that catches an UNDER-CONSTRAINED solve — wide covariance,
     §2.1; it detects variance, not bias): from `cv2.calibrateCameraExtended` std-deviations —
     principal-point and focal std-dev ≤ documented bounds (FAIL-SAFE, tightened to the well-conditioned
     substrate's covariance, see §8); focal within `(0.2..5.0)×long_dim`; principal point inside image
     (reuse calibrate.py bounds). Focal-std + 2-axis coverage alone separate good from under-constrained;
     the pp-std bound is a backstop. No extrinsic-diversity gate is needed.
6. Write `<out>` with the 5-key contract **plus provenance**: `K` (3×3), `dist_coeffs` = `[k1,k2,0,0,0]`
   (tangential & k3 forced 0), `image_size`, `reproj_error_px`, `frames_used` (= poses), and diagnostic
   keys `calibration_method: "structured_light_nominal"`, `pp_stddev_px`, `focal_stddev_px`, `n_poses`.
   The extra keys are ignored by every existing reader (they read only K/dist/image_size; the Rust
   adapter reads reproj_error_px+frames_used), so the file contract stays intact while the method is
   recorded for audit/rollback. Atomic write, like calibrate.py. **Default out path is non-destructive**
   (§4) — it never clobbers a trusted checkerboard intrinsics file.

### 3.3 Inputs / contracts (verified against current code)

- `intrinsics.json` out: `{K, dist_coeffs(5), image_size, reproj_error_px, frames_used}` — same
  keys `visual calibrate` writes and `reconstruct-structured-light` reads (only K/dist/image_size).
- `sl_meta.json`: `cabinets[].input_rect_px=[x,y,w,h]` + `pixel_pitch_mm=[px,py]`,
  `dots[]={id,u,v,cabinet:[col,row]}`, `screen_resolution`. (structured_light.py writer schema.)
- `corr.json` (decode output): per-dot `{id, u, v, x, y}` + `screen_id`, `sl_meta_sha256`,
  `camera_image_size`, `screen_roi`. `image_size` for K comes from `camera_image_size`.
- `project.yaml`: lmt-shared `ProjectConfig` (`cabinet_count`, `cabinet_size_mm`, `shape_prior`,
  `irregular_mask`, `pixels_per_cabinet`) — the variant `lmt-app`/visual already uses.

## 4. CLI / transport (per CLAUDE.md maintenance contract)

| Layer | Deliverable |
| --- | --- |
| lmt-app helper | `run_calibrate_structured_light(project_path:&Path, screen_id:&str, sl_meta:&Path, correspondences:&[String], out:Option<&Path>, force:bool) -> LmtResult<CalibrateResult>` in `crates/lmt-app/src/visual.rs`. Default out = `<project>/calibration/<screen_id>_sl_intrinsics.json` (distinct from the checkerboard `_intrinsics.json`). If the resolved out path already exists and `force=false`, refuse with `invalid_input` ("would overwrite existing intrinsics; pass --force or --out"). Runs provenance gate, calls adapter. |
| adapter | `calibrate_structured_light` async fn in `crates/adapter-visual-ba/src/api.rs` — builds `json!({"command":"calibrate_structured_light","version":1, project, sl_meta_path, correspondence_paths, output_path})`, runs sidecar, reads back `{reproj_error_px, frames_used}`. |
| sidecar | new module `calibrate_sl.py` with `run_calibrate_structured_light(cmd)`; register in `__main__.py` `SUBCOMMAND_MODULES` + `SUBCOMMAND_ENTRYPOINTS` (+ argparse); new ipc input model `CalibrateStructuredLightInput {project, sl_meta_path, correspondence_paths, output_path}`. |
| Tauri shim | thin `#[tauri::command]` in `src-tauri/src/commands/` delegating to the lmt-app helper (transport translation only). |
| DTO | **reuse existing `CalibrateResult`** `{intrinsics_path, reproj_error_px, frames_used}` — already derives `JsonSchema` and is in `schema::dump_all`. No new DTO ⇒ no schema-dump gap. |
| docs | `docs/agents-cli.md`: add the command row, note `side_effect=destructive`, list error codes. |

- **Destructive**: writes a file ⇒ `gate_destructive` + `--yes` / `--dry-run` (dry-run echoes the
  resolved out path, writes nothing). `--force` is required to overwrite an existing intrinsics file
  at the resolved out path (see §4 table) so a proposed SL calibration can't silently clobber a
  trusted checkerboard one.
- **Provenance gate** (reuse `reconstruct-structured-light`'s): all `--corr` share one `screen_id`
  + `sl_meta_sha256` matching `--sl-meta`; `sl_meta` cabinet set equals the project's present cells;
  ≥1 corr required (≥3 recommended; the §3.2 observability gate enforces real quality).
- **Same-camera precondition** (the calibrator fits ONE K across all `--corr` as one camera's poses):
  hard-gate that every `--corr` reports the **same `camera_image_size`** (different resolution ⇒
  `invalid_input` — and `calibrateCamera` needs one image size anyway). Same-resolution *different
  cameras / focal* are not distinguishable from corr provenance today (corr.json carries no camera/lens
  identity), so the operator is responsible for passing one camera's captures; the RMS + parameter-
  observability gates (§3.2) are the backstop (two different lenses cannot fit one K at low RMS).
  *Future hardening (out of scope):* plumb EXIF camera/lens serial + focal through decode → corr and
  hard-gate a single calibration group.
- **DB**: none. This command does not open the project DB (it reads project.yaml + files only),
  matching `reconstruct-structured-light`.

## 5. Error handling (reuse existing codes — no new codes/flags)

| Condition | Error | Code |
| --- | --- | --- |
| missing/unschema'd sl_meta, provenance mismatch, stale meta vs layout, bad files, mixed `camera_image_size` across corr, out path exists without `--force` | `invalid_input` | 2 |
| degenerate observability: near-coplanar target + <3 diverse poses, pose/baseline collapse (near-duplicate captures), too-low image coverage, or principal-point / focal std-dev (covariance) too high | `observability_failed` | 17 |
| solver produced unusable K (focal/pp out of bounds, reproj RMS > max, non-finite) | `intrinsics_invalid` | 16 |
| SL decode/segmentation issues surfaced from corr | `decode_failed` | 18 |

Refusal happens **before any file write** (no silent wrong intrinsics.json).

## 6. Testing

**(a) Independent golden geometry tests for `nominal_dot_positions_world` (close the oracle loop).**
The synthetic substrate (b) both *generates* data with this helper and the calibrator *solves* with it,
so a sign / unit / arc-angle / `R_y` bug could round-trip and pass silently while a live wall fails.
Pin the helper against independent oracles **first**:
- hand-written analytic fixtures for one flat + one curved cabinet (a few dots computed by hand);
- cross-check, for every cabinet: the centroid of its dot 3D == `nominal_cabinet_centers_model_frame`,
  the dot-plane normal == `nominal_cabinet_normals_model_frame`, and the 4 extreme dots match
  `sl_cabinet_corners_mm` rotated by the cabinet pose — all three are *existing, independent* functions.
These tests validate the geometry helper without going through the calibrator at all.

**(b) Sidecar TDD substrate (NEW, reusable).** Given a ground-truth `K`, a curved nominal wall, and N
camera poses, project the per-dot nominal 3D (from the now-pinned `nominal_dot_positions_world`) to
pixels with the *independent* `sl_feasibility.project_point` / `look_at_pose`, and write synthetic
`corr.json` + `sl_meta.json`. (The missing SL-dot ground-truth generator — `visual simulate` only
emits ChArUco corners. The projector is independent of the geometry helper, so with (a) closed the
substrate has no shared-oracle blind spot.)

**The happy substrate is well-conditioned (FAIL-SAFE envelope).** The acceptance tests use a **3×3
curved wall** (2D image coverage, non-coplanar) seen from **6 oblique poses at two distinct camera
distances** (`_well_meta` / `_well_poses` in `test_calibrate_sl.py`). A marginal substrate (single-row
wall, shallow single-distance front arc) is intentionally **refused** — it is outside the operating
envelope (§2.1).

Acceptance (synthetic, noise-free → noisy → adversarial):
- noise-free well-conditioned ⇒ recovered focal within **<1%**, principal point within **~1.5px**
  (`test_recovers_K_noise_free`).
- 0.3px centroid noise ⇒ focal within **<2%** (`test_recovers_K_with_noise_within_budget`).
- **structured as-built deviation** injected into the *true* scene while calibrating against *nominal*
  (arc radius +2%): recovered K stays within the focal/pp budget (fx_err ~0.24% across seeds). This
  verifies **K-robustness** — global deviation is absorbed into the per-pose extrinsics, not K (§2.1),
  so the solver returns a good K and the gate correctly does NOT refuse
  (`test_structured_deviation_within_budget_or_refused`).
- single noisy pose of the well-conditioned target (passes coverage) ⇒ `observability_failed` via the
  covariance gate (foc_std ~1.8%, pp_std ~11px); the SAME pose **noise-free** fits perfectly and is
  **accepted** — proving the gate fires on genuine under-constraint, not pose count
  (`test_single_pose_covariance_gate_refused` + `test_single_pose_noise_free_accepted`).
- **shallow-arc / few-pose** (sagitta ~0.09m, 2 fronto-parallel-ish poses; the reviewer's fx-38–41%-wrong
  case) ⇒ `observability_failed` (`test_shallow_arc_few_pose_refused`).
- **1D / wide-thin coverage** (8×1 wall seen fronto-parallel, vertical image axis collapses) ⇒
  `observability_failed` via the min-axis coverage gate (`test_one_dimensional_coverage_refused`).
- near-flat single pose ⇒ `observability_failed` (`test_near_flat_single_pose_refused`).
- **near-duplicate poses** (3 captures from almost the same viewpoint) ⇒ `observability_failed`
  (`test_near_duplicate_poses_refused`).

**CLI E2E (`crates/lmt-cli/tests/cli_e2e.rs`, ≥ happy/refuse/dry-run/envelope):**
- happy: synthetic curved scene ⇒ writes `<screen_id>_sl_intrinsics.json`, recovered K within tolerance,
  envelope reports `reproj_error_px` + `frames_used` + `calibration_method`.
- refuse: no `--yes` ⇒ refuse envelope, no write.
- dry-run: prints resolved out path, writes nothing.
- error envelopes: provenance mismatch ⇒ `invalid_input` (exit 2); mixed `camera_image_size` across corr
  ⇒ `invalid_input` (exit 2); out path exists without `--force` ⇒ `invalid_input` (exit 2); near-coplanar
  single pose ⇒ `observability_failed` (exit 17).

## 7. Non-goals

- No joint bundle adjustment / no refining cabinet geometry (that is Step 2).
- No calibrate↔reconstruct iteration against as-built (single-pass vs nominal; §2.1).
- No multi-camera joint calibration (one camera per invocation; `--out` lets you keep per-camera files).
- No EXIF/serial camera-identity gating (corr.json carries none today; deferred — §4 "future hardening").
  Same-resolution mixed-camera input is guarded only by the RMS/observability backstop, not provenance.
- No tangential / k3 distortion (radial k1,k2 only; emitted as 5-coeff with zeros).
- Flat-wall single-view is **outside** the well-conditioned envelope ⇒ refused, not approximated.

## 8. Observability thresholds = blocking acceptance criteria (not "open decisions")

The gate thresholds are **not** a post-hoc tuning afterthought; they are pinned by the §6 tests and
the build is not complete until those tests pass. Starting values (the plan pins final numbers against
the synthetic substrate, and each must have a passing refusal test):

| Threshold | Pinned value | Pinned by test |
| --- | --- | --- |
| reproj RMS max (`--max-rms-px`) | 1.5 px | `test_recovers_K_noise_free` / `_with_noise_within_budget` |
| coplanarity ratio (σ_min/σ_max of object cloud) | ≥ 1e-3 of extent **OR** ≥3 diverse poses | `test_near_flat_single_pose_refused` |
| pose/baseline rotation diversity min | extrinsic rotation span ≥ 5° (≥2 poses) | `test_near_duplicate_poses_refused` |
| image coverage min — **smaller** per-axis span `min(w, h)` of union bbox | ≥ **0.20** of frame (BOTH axes) | `test_one_dimensional_coverage_refused`, `test_shallow_arc_few_pose_refused` |
| principal-point std-dev (covariance) | ≤ **3 px** (backstop) | `test_single_pose_covariance_gate_refused` |
| focal std-dev (covariance) | ≤ **0.5%** of focal | `test_single_pose_covariance_gate_refused`, `test_shallow_arc_few_pose_refused` |

Settled: `frames_used` = **poses used** (parallels checkerboard `frames_used`).

**Threshold tuning (FAIL-SAFE re-pinning).** An earlier round loosened three gates
(focal-std 1%→1.5%, pp-std 3→12 px, coverage area→`max`-axis) to fit a **marginal**
substrate (single-row wall, shallow single-distance front arc). That loosening let
a 38–41%-wrong fx through (focal-std ~1.0–2.5% slipped under 1.5%; the collapsed
image axis slipped under a `max`-axis coverage gate). The fix re-tightens to a
**genuinely well-conditioned substrate** (3×3 curved wall + oblique + multi-distance
poses) and pins the gates so good and under-constrained geometries — which are ~10×
apart in covariance — are cleanly separated:

| Geometry | min-axis coverage | focal-std | pp-std | verdict |
| --- | --- | --- | --- | --- |
| well-conditioned 3×3 + oblique + 2-distance | ~0.22–0.33 | **0.15–0.20%** | **2.0–2.7 px** | PASS |
| shallow-arc 2-pose (sagitta 0.09m) | ~0.06 | 6.2–6.9% | 12.6–16.2 px | REFUSE |
| old single-row 4-pose front arc | ~0.06 | 2.3–2.6% | 19.6–27.9 px | REFUSE |
| 1D / wide-thin 8×1 fronto-parallel | ~0.04 | 6.1–8.6% | 16.7–19.8 px | REFUSE |
| single noisy pose (passes coverage) | ~0.33 | ~1.8% | ~11 px | REFUSE |

- **image coverage** — `min(w, h) ≥ 0.20` (BOTH axes), replacing the `max`-axis
  gate. A near-1D distribution collapses one image axis (fy/cy unconstrained) but
  passes a `max` gate via the dominant axis; `min` forces 2D coverage. The good
  substrate spans ≥ 0.22 on its smaller axis; every under-constrained case
  collapses to ≤ 0.06.
- **focal std-dev** — `≤ 0.5%`. The well-conditioned substrate sits at 0.15–0.20%
  (huge margin); every under-constrained case is ≥ 2.3%. This gate + min-axis
  coverage alone refuse every under-constrained case while passing the good one.
- **principal-point std-dev** — `≤ 3 px` (backstop). Good ≤ 2.7 px; under-constrained
  ≥ 12.6 px.

No separate extrinsic-diversity (camera-distance / obliquity spread) gate was
needed — focal-std + 2-axis coverage already separate the cases cleanly. These are
**precision + coverage** gates, not a bias detector: global as-built deviation is
absorbed into the extrinsics (§2.1) and correctly does NOT refuse.

**The covariance gate has a dedicated refusal + companion accept test**
(`test_single_pose_covariance_gate_refused` + `test_single_pose_noise_free_accepted`,
§6b). They isolate the parameter-observability gate by feeding the well-conditioned
3×3 target from a SINGLE close frontal pose: coplanarity passes, **coverage passes**
(min-axis ~0.33), rotation-diversity is skipped (one rvec), the fit's RMS stays
~0.4 px (under 1.5 px), but one view cannot pin focal/pp so focal-std (~1.8% > 0.5%)
/ pp-std (~11 px > 3 px) trips the gate. The companion noise-free single-pose case
fits perfectly (foc_std ≈ 0.001%, pp_std ≈ 0 px) and is *accepted*, confirming the
gate fires on genuine under-constraint rather than on pose count.
