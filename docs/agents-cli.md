# AGENTS.md — Agent / MCP integration guide

LED Mesh Toolkit ships an agent-friendly CLI binary called `lmt`. This document
is the contract for any caller (Claude Code, Codex, MCP wrapper, CI script).

## Quick start

```bash
# Inspect available subcommands and DTO/error schemas
lmt --help
lmt --json schema              # full JsonSchema dump on stdout
lmt --version

# Default DB path is the same lmt.sqlite that the Tauri GUI uses.
# Override per-process with --db <path> or env LMT_DB_PATH.
lmt --db /path/to/lmt.sqlite project list-recent
```

## Command tree

| Command | Side effect | Purpose |
| --- | --- | --- |
| `lmt schema` | read_only | Dump JsonSchema of all public DTOs + envelope + error types |
| `lmt manifest` | read_only | Dump Contract Manifest: all operations with operation_id / cli / side_effect / exit_codes |
| `lmt version` | read_only | Machine-readable version metadata (version string, schema_version, contract_version) |
| `lmt completion <shell>` | read_only | Generate shell completion script to stdout (raw script, not an envelope — see note below) |
| `lmt project list-recent` | read_only | List `recent_projects` table |
| `lmt project add-recent <abs_path> <display_name>` | write_safe | Upsert a recent-projects row. Path is normalized (canonicalize if exists, else absolutize) before write so GUI and CLI hit the same UNIQUE key. |
| `lmt project remove-recent <id>` | destructive | Delete a recent-projects row |
| `lmt project load <abs_path>` | read_only | Read `<abs_path>/project.yaml` |
| `lmt project save <abs_path> [--input path|-]` | destructive | Atomic write `<abs_path>/project.yaml` from YAML/JSON on stdin or file |
| `lmt measurements load <path>` | read_only | Read a `measured.yaml` |
| `lmt total-station import <project> <screen_id> <csv> [--mode grid\|scatter] [--columns x=C,y=C,z=C[,label=C]]` | destructive | Trimble CSV → `measurements/measured.yaml` + `import_report.json`. Default `--mode grid` runs full SOP grid import. `--mode scatter` stores raw scatter points (no SOP, fitting deferred to `reconstruct surface`). |
| `lmt total-station instruction-card <project> <screen_id>` | read_only | Output instruction-card **HTML** on stdout (no PDF — see below) |
| `lmt reconstruct surface <project> <screen_id> <measurements_rel>` | destructive | Run reconstruction, write `reports/<stamp>.json`, insert a `reconstruction_runs` row |
| `lmt reconstruct list-runs <project> [--screen-id S]` | read_only | List runs for a project (raw + canonical path keys both searched) |
| `lmt reconstruct get-run-report <run_id>` | read_only | Return the full `report.json` for a run |
| `lmt export obj <run_id> <target> [--dst path]` | destructive | Write an OBJ for a run; `target` ∈ `{disguise, unreal, neutral}` |
| `lmt export pose-obj <pose_report> <target> --out <path> [--root <cabinet_id>] [--ground]` | destructive | 把 `cabinet_pose_report.json` 的所有箱体合并导出成**一个**世界坐标 OBJ：每箱体独立面片（不焊接）+ 一张整体 0-1 网格 UV（每块占自己格子，导入 disguise 内容横铺整面墙）。**`disguise`** 始终套 disguise 约定(发光面 +Y up / 朝观众 +Z / 内容正向:flipY + winding 反转 + UV cell 内 V 翻;对账已验证模型 lmt_test_v02):**默认(无 `--root`)=标准摆法**(中心列自动转正 + 水平居中 + 贴地,逐箱体偏差 1:1 保留),**`--root <cab>`**=重定根到该箱体(轴对齐落原点)+ 同套 disguise 约定 + 贴地;无法定向的墙(法向近垂直,仅默认路径)→ 报错要求 `--root`。**`neutral`=原始帧**(右手 +Z up,+可选 `--ground`,不套 disguise 约定);**`--root` 对 neutral=手动覆盖朝向**。**按 pose report `frame.gauge_strategy` 分支**:`align_to_nominal`(SL 重建产出,几何已在 nominal 设计帧)**跳过标准摆法猜测**、disguise 约定照旧、**拒 `--root`**(已定帧,`invalid_input`);`fix_root_cabinet`(老 report / charuco)维持上述全部行为不变。Result: `{target, cabinet_count, file}` |
| `lmt seed-example <name> <dst>` | destructive | Copy a built-in example (curved-flat / curved-arc) into `<dst>/<name>` |
| `lmt visual calibrate <project> <screen_id> <checkerboard_dir> [--square-mm <f>] [--inner <RxC>]` | destructive | Checkerboard images → `calibration/<screen_id>_intrinsics.json` |
| `lmt visual generate-pattern <project> <screen_id> [--method vpqsp\|charuco] [--screen-id-code <0-15>] [--screen-mapping <json>]` | destructive | Generate the marker pattern — per-cabinet PNGs + `full_screen` + `pattern_meta` under `patterns/<screen_id>/`. **`--method vpqsp` (default)**: self-encoding VP-QSP markers (each encodes `screen_id`/cabinet `(col,row)`/`local_id` + CRC-8/AUTOSAR), `pattern_meta` schema `"vpqsp.v1"` with per-cabinet `markers_x/markers_y/marker_px/resolution_px/pixel_pitch_mm`. **NO ArUco dictionary capacity ceiling** — supports real LED-wall scale (2000+ cabinets), unlike ChArUco's ~13. `--screen-id-code <0-15>` (default 0) is baked into every marker (distinct per screen in a multi-screen Volume). A cabinet too small to host ≥8 markers → `invalid_input`. **`--method charuco`**: legacy ChArUco path, `pattern_meta` schema **v2** (square cabinets reproduce the legacy 9×9/40-marker board); exceeding the 1000-marker dictionary capacity → `invalid_input`. `--screen-mapping`: read per-cabinet size/pitch from a `screen_mapping.json` (relative paths resolve against the **current working directory**, like every other path argument — NOT against the project root) and generate a pitch-matched tile per cabinet (non-square / unequal cabinets supported); tiles are placed at each cabinet's `input_rect_px` (`[x, y, w, h]` = the cabinet's placement rect on the shared screen canvas — cabinets do NOT each start at `0,0`; overlapping rects → `invalid_input` with a tiling hint). `expected_pattern_hash` is **optional** in the mapping for this command (the hash doesn't exist until the pattern is generated; it is only enforced at `visual reconstruct` preflight). The mapping must cover every present cabinet exactly (missing/extra id → `invalid_input`). Without it, the uniform grid is used. Each VP-QSP marker fills ~90% of its grid cell to maximise screen utilisation (centre positions stay cell-centred — seam-optimal so abutting cabinets on a seamless wall keep one inter-marker gap). Result reports `cabinet_count` + `total_markers`. |
| `lmt visual generate-structured-light <project> <screen_id> [--dot-spacing N] [--dot-radius N] [--margin N] [--seq-format auto\|none\|tiff] [--screen-mapping <json>]` | destructive | Generate a structured-light dot-array capture sequence under `patterns/<screen_id>/sl/`: `frames/*.png` (white sentinel + all-on anchor + binary-blink-coded dot frames), `sequence.mp4` (drop-in full-screen playback), `sl_meta.json` (per-cabinet rects + dot screen coords + code/sequence spec, with `screen_id`). `--dot-spacing` and `--margin` auto-derive **per cabinet** from its pixel resolution when omitted (spacing ≈ 1/8, margin ≈ 1/16 of the cabinet's shorter edge → a ~filled 8×8 grid on any screen size with no tuning); pass explicit pixel values to override. `--seq-format` (default `auto`) controls a disguise-ready image sequence `<screen_id>.seq/` of uncompressed 24-bit TIFFs named `<screen_id>_NNNNN.tif` from 0 (disguise `.seq` ingest convention): `auto` emits it iff the project's `output.target == "disguise"`, `tiff` forces it, `none` suppresses it. Mapping-aware: with `--screen-mapping` dots are tiled inside each cabinet's `input_rect_px`, honoring absent/non-uniform cabinets; without it, the uniform grid is used (even-divisibility required). Identity is carried in each dot's blink sequence (binary + even parity), not appearance — no dictionary-capacity limit. Result reports `n_dots` and `n_frames`. |
| `lmt visual decode-structured-light <input> <sl_meta> --out <corr.json> [--sentinel-threshold F] [--screen-roi X,Y,W,H] [--emit-debug-image]` | destructive | Decode a recorded structured-light capture (video, frame-image directory, or a disguise `.seq` directory of 10-bit `.dpx` frames — DPX is read by a built-in parser and downscaled to 8-bit, no transcode needed) into a provenance-stamped screen↔camera correspondence file (`screen_id`, `sl_meta_sha256`, `camera_image_size`, `source_input`, `screen_roi`, points). Three-pass temporal frontend that decides by **change, not brightness** so it works on any-brightness textured static backgrounds with off-screen moving objects: Pass 1 per-pixel temporal range (max−min) → auto screen ROI (largest solid activity rectangle; `--screen-roi X,Y,W,H` overrides; auto-derive failure → `detection_failed`; auto ROI keys off the global peak temporal range, so a dim/oblique screen filmed alongside a brighter off-screen *moving* object may be missed — pass `--screen-roi` for that case); Pass 2 ROI-restricted white-sentinel mean + plateau indexing; Pass 3 ROI Otsu seeding (recovers the all-off `id=0`) + dot shape/size filter + per-dot relative (own min/max) bit reading + binary+parity decode gate. `corr.json` records the `screen_roi` actually used (detection provenance; `reconstruct-structured-light` ignores it). `--emit-debug-image` additionally writes `<out>.debug.png` (a black-background white-dot seed mask) for eyeball QA. `--sentinel-threshold` (default 0.85) now applies to the ROI-region mean. `decode_failed` (18) if sentinels/plateaus don't parse; `detection_failed` (13) if the ROI can't be auto-derived or too few dots decode. |
| `lmt visual reconstruct <project> <screen_id> --capture-manifest <json> [--method vpqsp\|charuco]` | destructive | Multi-view photos → `measurements/measured.yaml` + `measurements/<screen_id>_cabinet_pose_report.json` (model-constrained BA, zero total station). The capture manifest's own `method` field (`vpqsp` \| `charuco`) selects the detector; **`--method` (default `vpqsp`) is a forward-compat guard** — the manifest is authoritative. **vpqsp**: decode self-encoding VP-QSP markers → each marker's `(screen_id, col, row, local_id)` routes directly to its cabinet + nominal local-mm (`+y`-up, center-origin), feeding the SAME `solve_and_emit` BA as charuco (`gauge_strategy=fix_root_cabinet`, root cabinet = gauge). Observability/Stage-A/Stage-B robust trim are shared. Markers from other screens (different `screen_id_code`) are filtered out. The manifest's `screen_mapping.expected_pattern_hash` (if set) is preflight-checked against the captured pattern; if the mapping omits it, reconstruct proceeds but emits a `pattern_hash_unset` warning (the capture↔pattern binding is unverified) — warnings ride `VisualReconstructResult.warnings`. Errors reuse the shared set: `invalid_input`(3/2), `detection_failed`(13), `observability_failed`(17), `ba_diverged`(14). |
| `lmt visual reconstruct-structured-light <project> <screen_id> --sl-meta <json> --intrinsics <json\|auto> [--intrinsics-crosscheck <json>] --corr <c.json> ...` | destructive | Reconstruct a metric per-cabinet 3D model from N per-pose structured-light correspondence files (decode-structured-light output). Provenance-gated: all `--corr` must share one `screen_id` + `sl_meta_sha256` matching `--sl-meta`, and match the project screen; `sl_meta` is schema-validated and its cabinet set must equal the project's present cells (stale meta / edited layout → `invalid_input`). `p_local` derives from canonical `sl_meta` dot `(u,v)` (the per-pose corr `(u,v)` is ignored). Runs the SAME model-constrained BA as `reconstruct` (root cabinet = BA gauge, scale from pixel pitch), then — unlike `reconstruct` (charuco) which stays in the root-local frame — rigidly aligns the whole wall to the nominal design grid (Procrustes over all cabinet corners): the pose report's `frame.gauge_strategy` is `align_to_nominal` and `VisualReconstructResult.procrustes_align_rms_m` carries the alignment residual (meters; large = as-built far from nominal or wrong shape_prior). Writes `measurements/measured.yaml` + `<screen>_cabinet_pose_report.json` (reuses `VisualReconstructResult`). Degenerate alignment (<3 cabinets / collinear) → `procrustes_failed`(15). Default-ON per-observation geometric outlier rejection (Stage A PnP-RANSAC pre-clean + Stage B global robust-residual trim); rejection counts surface in the result envelope (`ba_observations_total`/`ba_observations_used`/`ba_rejected`) and per-cabinet in the pose report (`rejected_points`); high-rejection cabinets emit a `high_rejection` warning. Convex/concave undecidable or a 2-view coherent conflict → `observability_failed`(17) BEFORE any file write (no silent wrong measured.yaml). **`--intrinsics auto`** (precision): instead of a file, inline self-calibrate K from the SAME `--corr` (frame-matched), then reconstruct — `VisualReconstructResult.intrinsics_source` = `auto_self_calibrated` (vs `file`). With `--intrinsics-crosscheck <anchor.json>` (independent checkerboard intrinsics) an anti-absorption cross-check compares the self-cal K to the anchor on focal + fx/fy aspect + radial & tangential distortion; deviation → `observability_failed`(17) BEFORE any write (catches screen pitch/1:1 errors absorbed into K — including shear/decentering absorbed into tangential `p1/p2`). A malformed anchor (non-3×3 or non-finite K/dist) → `invalid_input`. A **flat (coplanar) wall** without an anchor is refused (`observability_failed`); a curved wall without an anchor is admitted but emits a `no_intrinsics_anchor` warning. A large `align_to_nominal` residual (> 3.0mm) emits a `nominal_misfit` warning — the NON-absorbable pitch/shape class (global isotropic pitch scale / shape deviation rigid Procrustes cannot absorb), the complement to the L1 cross-check's absorbable class. All non-fatal warnings (`no_intrinsics_anchor` / `nominal_misfit` / `high_rejection` / `cabinet_quality` / `missing_covariance`) ride `VisualReconstructResult.warnings` — a `[{code, message, cabinet}]` list ([`WarningDto`]) the adapter collects off the sidecar event stream so they survive the headless path (where the live WarningEvents, having no progress consumer, are dropped); the CLI prints them and `--json` carries the list. Errors: `invalid_input`(3), `intrinsics_invalid`(16), `detection_failed`(13), `observability_failed`(17), `ba_diverged`(14). NOTE: this is a separate subcommand; `reconstruct --method structured-light` stays `unsupported`(7). |
| `lmt visual calibrate-structured-light <project> <screen_id> --sl-meta <json> --corr <c.json> ... [--out <path>] [--force] [--max-rms-px <f>] [--intrinsics-crosscheck <json>]` | destructive | Calibrate ONE camera's intrinsics (`fx,fy,cx,cy` + **adaptive distortion**: `radial2` = `k1,k2`, or `full` = `k1,k2,k3`+tangential when an anchor + pose diversity make the extra coeffs observable — reported as `CalibrateResult.distortion_model`) from its structured-light white-dot captures, using the project's nominal design wall as a known 3D target (curved wall = non-coplanar target, which resolves the focal/principal-point ambiguity a planar target can't). Produces `calibration/<screen_id>_sl_intrinsics.json` — the existing intrinsics contract (`K`, `dist_coeffs`, `image_size`, `reproj_error_px`, `frames_used`) plus provenance (`calibration_method`, `pp_stddev_px`, `focal_stddev_px`, `n_poses`); Step 2 `reconstruct-structured-light` consumes it unchanged via `--intrinsics`. **Non-destructive**: default out is distinct from the checkerboard `visual calibrate` (`_intrinsics.json`) and it refuses to overwrite an existing intrinsics file at the out path without `--force`. Provenance-gated like `reconstruct-structured-light` (all `--corr` share one `screen_id` + `sl_meta_sha256` matching `--sl-meta`, cabinet set == project present cells) and additionally hard-gates a single `camera_image_size` across all `--corr` (one camera). Refuses on degenerate observability — near-coplanar target with <3 diverse poses, near-duplicate/low-baseline poses, too-low image coverage, or high principal-point/focal parameter covariance — with `observability_failed`(17) BEFORE any write (never a confidently-wrong K). `--max-rms-px` (default 1.5) gates reproj RMS. **`--intrinsics-crosscheck <anchor.json>`** (precision) runs the anti-absorption cross-check (focal + fx/fy aspect + radial & tangential distortion vs the anchor; deviation → `observability_failed`; malformed anchor → `invalid_input`) and enables the `full` distortion model; a **flat (coplanar) wall WITHOUT** an anchor is refused (`observability_failed`, since a coplanar target can't observe distortion and the self-cal overfits it), a curved wall without one emits a `no_intrinsics_anchor` warning, surfaced on `CalibrateResult.warnings` (same `[{code, message, cabinet}]` channel as reconstruct, durable on the headless path). New result fields: `distortion_model`, `focal_stddev_px`, `pp_stddev_px`, `warnings`. Errors: `invalid_input`(2), `intrinsics_invalid`(16), `observability_failed`(17). |
| `lmt visual simulate <config> --out <dir>` | destructive | Generate a synthetic geometry dataset (`scene.npz` + `meta.json`) for BA validation |
| `lmt visual eval <dataset> [--method charuco] [--seed-matrix <list>]` | write_safe | Evaluate a method vs ground truth on a synthetic dataset (gauge-invariant metrics) |
| `lmt visual compare-known <report.json> <known.json> [--max-size-mm F] [--max-dist-mm F] [--max-angle-deg F]` | write_safe | Compare a `cabinet_pose_report.json` against known monitor geometry — per-cabinet size error (from corners), per-pair distance error (from positions), per-pair angle error (from normals), + pass/fail vs thresholds (defaults size≤2.0mm / distance≤3.0mm / angle≤0.3°). The three `--max-*` flags (precision-grade acceptance) override the defaults; only provided ones change, omitted ones keep the default. The applied thresholds are echoed back on `CompareKnownResult.thresholds`. Reads two JSON files, writes nothing. |
| `lmt visual plan-capture <project> <screen_id> --image-size <WxH> (--hfov-deg <f> \| --vfov-deg <f>) --standoff <MIN..MAX> --height <MIN..MAX> [--target-mm 3.0] [--trials 20] [--seed 0] [--min-views 2]` | write_safe | Camera capture-guidance planner: from a screen's geometry (`project.yaml` cabinet grid + curved/flat shape) and the camera intrinsics (sensor `WxH` + one FOV), recommend a set of capture **stations** (position/aim/standoff/height/role/covered-cabinets) plus a per-cabinet **coverage** report (`p95_residual_mm` — `null` when not reconstructable, `reconstructable`/`low_observation`/`bridged`/`pass`/`fail_reason`) and `unreachable_regions`. Returns a `CapturePlan` in the envelope; **writes nothing**. Per-sample-point visibility (cheirality/frame/incidence) aggregated to the real reconstruct observability gate (≥2 views, ≥8 obs/cabinet, ≥4 pts/view); a recipe seed (FOV-fill standoff + front fan + top/bottom) is refined by a greedy optimizer over a reachable shell (standoff×height range). **`--min-views`** (default 2 = reconstruct's observation gate; precision profile passes 3) raises the covering-view count a cabinet needs to be `reconstructable`. Each non-passing cabinet carries a `fail_reason` diagnostic (no new gate): `low_coverage` (too few views/points or unbridged) vs `low_parallax` (count-reconstructable + bridged but p95 over target = degenerate near-duplicate baseline). Exactly one of `--hfov-deg`/`--vfov-deg`; bad `WxH`/range/screen → `invalid_input`(3) / `not_found`(3). Residuals are a guidance-grade prediction (visibility-gated triangulation feasibility, not full BA). |
| `lmt visual capture-card <project> <screen_id> <same flags as plan-capture>` | read_only | Render the capture plan as a **self-contained interactive 3D HTML guidance card** (Three.js + OrbitControls inlined, no CDN / external deps, fully offline): a rotatable 3D view of the wall + camera stations with FOV frustums, click-to-select per-station highlight with mini viewport (simulated camera view), per-cabinet grid texture with cell labels, a per-station table, and unreachable-region warnings. Human mode → raw HTML to stdout (`... > card.html`); `--json` wraps `{html_content}`. Runs the same planner as `plan-capture` (no file writing). |

### Scatter import mode

`lmt total-station import … --mode scatter` is a lightweight path for LED panels
measured with a total station as unstructured (non-grid) scatter points. Key
differences from the default `grid` mode:

- **No SOP校验**: the command does not look at `coordinate_system:` in
  `project.yaml` and does not require `origin_point` / `x_axis_point` /
  `xy_plane_point` grid markers. The raw (x, y, z) coordinates are stored as-is
  in `measured.yaml` with `sampling_mode: Scatter`.
- **Cabinet / shape read from `project.yaml`**: `cabinet_count`, `cabinet_size_mm`,
  and `shape_prior` are still read from the target screen's config and stored in
  `measured.yaml` so that `reconstruct surface` can run without the GUI.
- **Fitting and outlier detection happen at `reconstruct surface`**, not at import
  time. The import step never fails due to bad geometry — it just stores the raw
  points.
- **`--columns x=C,y=C,z=C[,label=C]`** (1-based column numbers, optional):
  explicitly maps CSV columns. Omit to let the adapter auto-detect from the CSV
  header row. `x`, `y`, `z` are required; `label` is optional.

#### Critical coordinate-unit requirement

The scatter CSV **must use meters (m) as the unit for (x, y, z) coordinates**.
This is a hard constraint imposed by the surface-fit algorithm:

- The inlier threshold in `reconstruct surface` is **0.05 m** (5 cm). Points
  further than 0.05 m from the fitted surface are treated as outliers.
- If coordinates are in millimetres, every point is ≫ 0.05 from the surface and
  the fit will either fail (`surface_fit_failed`) or produce nonsense output.
- The boundary check compares the point cloud bounding box against the cabinet
  physical dimensions. Cabinet dimensions in `project.yaml` are stored in mm
  (`unit: mm`), so the boundary check internally converts them to metres before
  comparison. A millimetre point cloud will match cabinet millimetre dimensions
  and will therefore pass the boundary check — but the surface fit will still
  fail downstream. **Do not confuse "boundary check passed" with "units are
  correct".**
- Trimble instruments typically output meters when the job is set up with a
  metric datum. Verify the instrument job settings before exporting the CSV.

### `completion` is not in the Contract Manifest

`lmt completion <shell>` emits a raw shell script to stdout — not a JSON
envelope. It is therefore intentionally excluded from `lmt manifest` and the
Contract Manifest snapshot. Agents that need completions should capture stdout
directly and not expect `{"ok": true, ...}` wrapping.

### Not exposed in CLI

- **`save-pdf`** — instruction-card PDF rendering goes through the platform
  native WebView (WKWebView on macOS, WebView2 on Windows) and only works inside
  the Tauri GUI process. CLI agents can get the HTML from `instruction-card`
  and run their own renderer (headless Chrome, wkhtmltopdf, etc.).

### Not exposed in the GUI (CLI-only)

The entire `visual` command group — `calibrate`, `calibrate-structured-light`,
`generate-pattern`, `generate-structured-light`, `decode-structured-light`,
`reconstruct`, `reconstruct-structured-light`, `simulate`, `eval`,
`compare-known` — is
CLI-only by design: it has no `#[tauri::command]` shim and is not registered in
the GUI's `generate_handler!`. The camera/structured-light pipeline is an
agent/headless workflow (long-running sidecar runs, no native-webview
dependency), so the deliverables are files on disk that any front-end can
consume. `generate-structured-light`, `decode-structured-light`, and
`reconstruct-structured-light` follow this convention. The service-layer helpers
(`lmt_app::visual::run_generate_structured_light` / `run_decode_structured_light`
/ `run_reconstruct_structured_light` / `run_calibrate_structured_light`) are plain
functions, so a future GUI shim is a thin transport wrapper if one is ever needed.

## Global flags

| Flag | Meaning |
| --- | --- |
| `--output text\|json\|ndjson` (`-o`) | Output format. `json` and `ndjson` are machine modes (envelope / event stream). `--json` is a legacy alias for `--output json`. |
| `--json` | Legacy alias for `--output json`. Emit machine-stable envelope output. Success → stdout; failure → stderr; nothing else gets mixed in (no tracing, no colors). |
| `--no-color` | Disable ANSI color. Human mode currently has no ANSI color anyway, so this flag is accepted as a no-op to satisfy callers that always pass it. |
| `--no-input` | Refuse interactive prompts. The CLI never prompts interactively; this flag is accepted as a no-op so agents can pass it unconditionally. Destructive commands still require `--yes`. |
| `--db <path>` | SQLite path override. Env fallback: `LMT_DB_PATH`. Final fallback: OS-standard `app_data_dir/com.lanbipu.lmt/lmt.sqlite` — the **same file** the Tauri GUI uses. |
| `--dry-run` | Preview a destructive command. Side effects are skipped; output reports `{dry_run: true, would_*: ...}`. DB is opened read-only (no `--db` creation), so dry-run never mutates state. |
| `--yes` | Confirm a destructive command. Required unless `--dry-run` is set. |
| `--timeout <secs>` | **v0: not implemented.** Passing this flag returns `unsupported` with exit code 7 — agents see a deterministic refusal rather than a silent no-op. |
| `--version` | Print `lmt <version>` and exit 0. |
| `--help` | Standard clap help. |

## JSON envelope contract

Success (`--json`):

```json
{ "ok": true, "data": <T>, "meta": { "schema_version": "1" } }
```

Failure (`--json`):

```json
{ "ok": false, "error": { "code": "<snake_case>", "message": "...", "details"?: <object> } }
```

- Success → stdout (single JSON line).
- Failure → stderr (single JSON line). stdout stays empty on failure, so the
  shell idiom `lmt --json … > out.json` lets you detect failure via empty file
  + non-zero exit code.
- Tracing is disabled in `--json` mode so stderr is reserved for the envelope.

## Error code & exit code table

| String code | Exit code | Trigger |
| --- | ---: | --- |
| `invalid_input` | 2 | Bad parameter, missing argument, parse error, destructive without `--yes`/`--dry-run` |
| `not_found` | 3 | Resource (file, screen, run id) does not exist |
| `io` | 4 | Filesystem error (read/write/canonicalize) |
| `db` | 5 | SQLite error (open/query) |
| `serialization` | 6 | YAML/JSON encode or decode error |
| `unsupported` | 7 | Feature not implemented (e.g. `--timeout`, `save-pdf` route) |
| `cancelled` | 8 | Long task cancelled by caller (not currently produced by any CLI subcommand) |
| `timeout` | 9 | Long task timed out (not currently produced) |
| `conflict` | 10 | Write conflict (reserved; current `cross-screen` import refusal still maps to `invalid_input` for API simplicity) |
| `internal` | 11 | Uncategorized internal error |
| `surface_fit_failed` | 12 | 散点曲面拟合失败：数据不成形 / inlier 比例 < 0.5 / 边界校验 reject |
| `detection_failed` | 13 | Too few features detected to reconstruct: ChArUco/checkerboard corners, VP-QSP marker decode, or structured-light dots/ROI in one or more frames/views |
| `ba_diverged` | 14 | Bundle adjustment did not converge or final reprojection error exceeds threshold |
| `procrustes_failed` | 15 | Procrustes alignment between estimated and model geometry failed (too few correspondences or degenerate configuration) |
| `intrinsics_invalid` | 16 | Camera intrinsics are unusable (distortion overflow, focal length ≤ 0, or calibration not found) |
| `observability_failed` | 17 | Insufficient visual overlap across views — one or more cabinets have no shared observations |
| `decode_failed` | 18 | Structured-light segmentation/plateau decode failed, or image decode error / unsupported image format |
| _unknown_ | 1 | Caller saw a code not in this table (forward-compat) |
| _success_ | 0 | OK |

The table is the source of truth. New error codes must be added here and to
both `crates/lmt-shared/src/envelope.rs::error_codes` and
`crates/lmt-shared/src/exit_codes.rs`.

## DTO / schema discovery

Run `lmt --json schema` to dump every public type's JsonSchema. The output is
shaped as `{schema_version, types: {<TypeName>: <JsonSchema>}, incomplete: [...]}`.

`ReconstructionResult` and `ReconstructionReport` are listed under `incomplete`
— they embed `lmt_core` domain types (`ReconstructedSurface`, `QualityMetrics`,
`CabinetArray`) which deliberately do not derive `JsonSchema` to keep the core
crate transport-free. If you need their shape, read the rendered
`reports/<stamp>.json` from `get-run-report` directly.

## Operation discovery

Run `lmt --json manifest` to list every operation with its stable `operation_id`,
canonical CLI string, `side_effect`, and possible exit codes. This is the
machine-readable counterpart of the Command tree table above. A snapshot lives
at `docs/contract-manifest.json`. When you add/remove a subcommand, regenerate
the snapshot and update `lmt_shared::manifest::build()`.

## DB path convention

The CLI defaults to `app_data_dir/com.lanbipu.lmt/lmt.sqlite`, identical to the
Tauri GUI's resolved path. Three implications:

1. **Common workflow shares state.** GUI adds a recent project → CLI sees it in
   `list-recent`; CLI runs a reconstruction → GUI's Runs view shows it.
2. **Tests / CI / Agent harnesses must override the default** with `--db
   /tmp/test.sqlite` or `LMT_DB_PATH=/tmp/test.sqlite`, otherwise default-DB
   pollution is unavoidable.
3. **Concurrency**: `open()` sets `journal_mode=WAL` + `busy_timeout=5000ms`
   (read order: `busy_timeout` before `journal_mode`). Readonly opens use
   standard WAL-aware readonly (so a recent GUI write is visible), accepting
   that SQLite may create `.sqlite-shm` sidecars per WAL protocol — but never
   the main DB file.

### `recent_projects.abs_path` normalization

The shared write helper `upsert_normalized` canonicalizes the input path
(canonicalize when the file exists, otherwise absolutize) before insertion.
Both the Tauri GUI command and the CLI subcommand go through the same helper,
so the same project always lands on the same UNIQUE key regardless of which
side wrote it.

**Legacy data**: `schema::migrate()` 在每次 `open()` 之后跑一次幂等的
`normalize_legacy_paths` sweep,把 `recent_projects.abs_path` 与
`reconstruction_runs.project_path` 里的老 raw / symlink path canonicalize 到
统一字符串;`recent_projects` 上的 UNIQUE 冲突通过比较 `last_opened_at`
(较新的 `display_name` 与 `last_opened_at` 合并到 canonical row),再删除
raw alias 解决。GUI 之前用 `/var/...` 写入的 row,在 GUI 启动或任何写入命令
首次 open DB 时自动迁移到 `/private/var/...` canonical 形式。

注意:**`open_readonly()` 不调 `migrate()`**——这是 read-only 契约的代价。
如果用户从一个未升级的 DB 直接跑 `lmt project list-recent` 之类 read-only
命令,sweep 不会触发,结果里可能仍有 raw + canonical 两条 row。任何一次
GUI 启动 / `add-recent` / `reconstruct surface` 之类的写命令都会触发 sweep
并收敛。`list_runs_for` 内部仍保留 raw + canonical OR-query 作为查询时的
二次兜底,所以 read-only 路径上"看不到旧 run"是不会发生的。

## Side-effect taxonomy (for MCP wrapping)

| Class | Allowed without confirmation? | CLI commands |
| --- | :---: | --- |
| `read_only` | yes | `schema`, `project list-recent` / `load`, `measurements load`, `total-station instruction-card`, `reconstruct list-runs` / `get-run-report` |
| `write_safe` | yes (no `--yes`) | `project add-recent` (still honors `--dry-run`), `visual eval`, `visual compare-known` |
| `destructive` | no (requires `--yes` or `--dry-run`) | `project remove-recent` / `save`, `total-station import`, `reconstruct surface`, `export obj`, `export pose-obj`, `visual calibrate`, `visual generate-pattern`, `visual generate-structured-light`, `visual decode-structured-light`, `visual reconstruct`, `visual reconstruct-structured-light`, `visual simulate` |

An MCP tool wrapper should propagate these as the tool's `side_effect`
annotation and route `destructive` tools through a confirmation step.

## Logging

Set `LMT_LOG=info` (or `debug`, `trace`) to get tracing output on stderr in
**human mode only**. `--json` mode keeps tracing fully off — stderr is reserved
for `ErrorEnvelope`.

## Not yet implemented

- `--timeout` enforcement (deadline + cancellation) — returns `unsupported`.
- HTTP API surface — out of scope for this phase. Envelope + error model are
  already compatible with `api_spec` so a thin axum/actix layer can be added
  later by wrapping the same `lmt-app` service functions.
- Long-running cancellation tokens for `reconstruct surface` — currently runs
  to completion or fails inside the algorithm.
