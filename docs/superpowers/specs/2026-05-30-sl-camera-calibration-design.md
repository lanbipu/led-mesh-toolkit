# Spec — Step 1: On-site camera lens calibration from structured-light white dots vs nominal design geometry

- Date: 2026-05-30
- Status: design proposed, awaiting user review
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

### 2.1 Calibrating against nominal (which has as-built deviation) — why single-pass is fine

We calibrate against the *design* wall, but the *as-built* wall deviates by ~mm–cm while the wall
spans meters → object-point error is ~0.01–0.1% relative, far under the 2% focal budget; the
principal point is constrained by global 3D structure, not local mm deviations. So **a single
pass against nominal is self-consistent.** Iterating calibrate↔reconstruct-against-as-built is a
possible future refinement, explicitly **out of scope** here.

## 3. Architecture

Mirror the existing `reconstruct` / `reconstruct-structured-light` sibling split. The new path is
a sibling of `calibrate`, reusing the SL transport machinery of `reconstruct-structured-light`
minus the `--intrinsics` input (we *produce* intrinsics, not consume them).

```
lmt visual calibrate-structured-light <project> <screen_id>
     --sl-meta <sl_meta.json> --corr <c.json> [--corr ...] [--out <path>]
          │  (clap subcommand, destructive → gate_destructive + --yes/--dry-run)
          ▼
lmt_app::visual::run_calibrate_structured_light(project, screen_id, sl_meta, corrs, out)
          │  (service layer: provenance gate, resolve out path, call adapter)
          ▼
adapter-visual-ba::api::calibrate_structured_light  → run_sidecar(subcommand="calibrate_structured_light")
          │  (payload {command, version, project, sl_meta_path, correspondence_paths, output_path})
          ▼
python-sidecar  lmt_vba_sidecar.calibrate_sl::run_calibrate_structured_light(cmd)
          │  1. per-dot nominal 3D world table  2. cv2.calibrateCamera  3. conditioning gates  4. write intrinsics.json
          ▼
<project>/calibration/<screen_id>_intrinsics.json   (same 5-key contract as `visual calibrate`)
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
5. **Conditioning / quality gates — refuse on failure** (see §5):
   - reproj RMS ≤ `--max-rms-px` (default **1.5px**; looser than checkerboard's 0.5px because SL
     dot centroids on a live LED wall are noisier — bloom, large dots. Tunable, concrete default).
   - **3D-conditioning gate**: the union of object points actually used must not be near-coplanar
     unless ≥3 distinct poses are present. Measure via the ratio of the smallest singular value of
     the centered object-point cloud to its largest (the "flatness" of the target). Below a
     documented threshold AND <3 poses ⇒ refuse `observability_failed`.
   - principal-point std-dev (from calibrateCameraExtended) ≤ a few px; principal point inside
     image; focal within `(0.2..5.0) × long_dim` (reuse calibrate.py bounds).
6. Write `<out>` with the 5-key contract: `K` (3×3), `dist_coeffs` = `[k1,k2,0,0,0]` (tangential &
   k3 forced 0), `image_size`, `reproj_error_px`, `frames_used` (= n poses). Atomic write, exactly
   like calibrate.py, so the Rust `CalibrateOut` adapter readback and both reconstruct readers work
   unchanged.

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
| lmt-app helper | `run_calibrate_structured_light(project_path:&Path, screen_id:&str, sl_meta:&Path, correspondences:&[String], out:Option<&Path>) -> LmtResult<CalibrateResult>` in `crates/lmt-app/src/visual.rs`. Resolves out path (default `<project>/calibration/<screen_id>_intrinsics.json`), runs provenance gate, calls adapter. |
| adapter | `calibrate_structured_light` async fn in `crates/adapter-visual-ba/src/api.rs` — builds `json!({"command":"calibrate_structured_light","version":1, project, sl_meta_path, correspondence_paths, output_path})`, runs sidecar, reads back `{reproj_error_px, frames_used}`. |
| sidecar | new module `calibrate_sl.py` with `run_calibrate_structured_light(cmd)`; register in `__main__.py` `SUBCOMMAND_MODULES` + `SUBCOMMAND_ENTRYPOINTS` (+ argparse); new ipc input model `CalibrateStructuredLightInput {project, sl_meta_path, correspondence_paths, output_path}`. |
| Tauri shim | thin `#[tauri::command]` in `src-tauri/src/commands/` delegating to the lmt-app helper (transport translation only). |
| DTO | **reuse existing `CalibrateResult`** `{intrinsics_path, reproj_error_px, frames_used}` — already derives `JsonSchema` and is in `schema::dump_all`. No new DTO ⇒ no schema-dump gap. |
| docs | `docs/agents-cli.md`: add the command row, note `side_effect=destructive`, list error codes. |

- **Destructive**: writes a file ⇒ `gate_destructive` + `--yes` / `--dry-run` (dry-run echoes the
  resolved out path, writes nothing).
- **Provenance gate** (reuse `reconstruct-structured-light`'s): all `--corr` share one `screen_id`
  + `sl_meta_sha256` matching `--sl-meta`; `sl_meta` cabinet set equals the project's present cells;
  ≥1 corr required (≥3 recommended; the conditioning gate enforces real quality).
- **DB**: none. This command does not open the project DB (it reads project.yaml + files only),
  matching `reconstruct-structured-light`.

## 5. Error handling (reuse existing codes — no new codes/flags)

| Condition | Error | Code |
| --- | --- | --- |
| missing/unschema'd sl_meta, provenance mismatch, stale meta vs layout, bad files | `invalid_input` | 3 |
| degenerate conditioning: near-coplanar target + <3 poses, or principal-point std-dev too high | `observability_failed` | 17 |
| solver produced unusable K (focal/pp out of bounds, reproj RMS > max, non-finite) | `intrinsics_invalid` | 16 |
| SL decode/segmentation issues surfaced from corr | `decode_failed` | 18 |

Refusal happens **before any file write** (no silent wrong intrinsics.json).

## 6. Testing

**Sidecar TDD substrate (NEW, reusable):** a test helper that, given a known ground-truth `K`,
a curved nominal wall, and N camera poses, projects the per-dot nominal 3D world points to pixels
(reuse `sl_feasibility.project_point` / `look_at_pose` + the new `nominal_dot_positions_world`) and
writes synthetic `corr.json` + `sl_meta.json`. This is the missing SL-dot ground-truth generator
(`visual simulate` only emits ChArUco corners).

Acceptance (synthetic, noise-free → noisy):
- noise-free curved + 3 poses ⇒ recovered focal within **<1%**, principal point within **~1px** of truth.
- 0.3px centroid noise ⇒ focal within **<2%** (the downstream budget), pp std-dev reported.
- near-flat single pose ⇒ `observability_failed` (refused), NOT a wrong K.

**CLI E2E (`crates/lmt-cli/tests/cli_e2e.rs`, ≥ happy/refuse/dry-run/envelope):**
- happy: synthetic curved scene ⇒ writes intrinsics.json, recovered K within tolerance, envelope
  reports `reproj_error_px` + `frames_used`.
- refuse: no `--yes` ⇒ refuse envelope, no write.
- dry-run: prints resolved out path, writes nothing.
- error envelope: provenance mismatch ⇒ `invalid_input(3)`; near-coplanar single pose ⇒
  `observability_failed(17)`.

## 7. Non-goals

- No joint bundle adjustment / no refining cabinet geometry (that is Step 2).
- No calibrate↔reconstruct iteration against as-built (single-pass vs nominal; §2.1).
- No multi-camera joint calibration (one camera per invocation; `--out` lets you keep per-camera files).
- No tangential / k3 distortion (radial k1,k2 only; emitted as 5-coeff with zeros).
- Flat-wall single-view is **outside** the well-conditioned envelope ⇒ refused, not approximated.

## 8. Open decisions deferred to the plan (not blocking)

- Exact numeric thresholds for the 3D-conditioning singular-value ratio and pp std-dev — pin
  concrete defaults during TDD against the synthetic substrate (start: coplanarity ratio ≥ 1e-3 of
  extent OR ≥3 poses; pp std-dev ≤ 3px).
- Whether `frames_used` should mean "poses used" vs "dots used" — proposed: **poses** (parallels
  checkerboard `frames_used`).
