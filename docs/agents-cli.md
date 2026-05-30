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
| `lmt export pose-obj <pose_report> <target> --out <path> [--root <cabinet_id>] [--ground]` | destructive | 把 `cabinet_pose_report.json` 的所有箱体合并导出成**一个**世界坐标 OBJ：每箱体独立面片（不焊接）+ 一张整体 0-1 网格 UV（每块占自己格子，导入 disguise 内容横铺整面墙）。**`disguise`** 始终套 disguise 约定(发光面 +Y up / 朝观众 +Z / 内容正向:flipY + winding 反转 + UV cell 内 V 翻;对账已验证模型 lmt_test_v02):**默认(无 `--root`)=标准摆法**(中心列自动转正 + 水平居中 + 贴地,逐箱体偏差 1:1 保留),**`--root <cab>`**=重定根到该箱体(轴对齐落原点)+ 同套 disguise 约定 + 贴地;无法定向的墙(法向近垂直,仅默认路径)→ 报错要求 `--root`。**`neutral`=原始帧**(右手 +Z up,+可选 `--ground`,不套 disguise 约定);**`--root` 对 neutral=手动覆盖朝向**。Result: `{target, cabinet_count, file}` |
| `lmt seed-example <name> <dst>` | destructive | Copy a built-in example (curved-flat / curved-arc) into `<dst>/<name>` |
| `lmt visual calibrate <project> <screen_id> <checkerboard_dir> [--square-mm <f>] [--inner <RxC>]` | destructive | Checkerboard images → `calibration/<screen_id>_intrinsics.json` |
| `lmt visual generate-pattern <project> <screen_id> [--method charuco] [--screen-mapping <json>]` | destructive | Generate ChArUco pattern — per-cabinet PNGs + `full_screen` + `pattern_meta` (schema **v2**) under `patterns/<screen_id>/`. `--screen-mapping`: read per-cabinet size/pitch from a `screen_mapping.json` (path resolved against the project root) and generate a pitch-matched board per cabinet (non-square / unequal cabinets supported); boards are placed at each cabinet's `input_rect_px` and the framebuffer size is their bounding box. The mapping must cover every present cabinet exactly (missing/extra id → `invalid_input`). Without the flag, the uniform grid is used (square cabinets reproduce the legacy 9×9/40-marker board). Result reports `total_markers` (per-cabinet counts vary in v2). |
| `lmt visual generate-structured-light <project> <screen_id> [--dot-spacing N] [--dot-radius N] [--margin N] [--seq-format auto\|none\|tiff] [--screen-mapping <json>]` | destructive | Generate a structured-light dot-array capture sequence under `patterns/<screen_id>/sl/`: `frames/*.png` (white sentinel + all-on anchor + binary-blink-coded dot frames), `sequence.mp4` (drop-in full-screen playback), `sl_meta.json` (per-cabinet rects + dot screen coords + code/sequence spec, with `screen_id`). `--dot-spacing` and `--margin` auto-derive **per cabinet** from its pixel resolution when omitted (spacing ≈ 1/8, margin ≈ 1/16 of the cabinet's shorter edge → a ~filled 8×8 grid on any screen size with no tuning); pass explicit pixel values to override. `--seq-format` (default `auto`) controls a disguise-ready image sequence `<screen_id>.seq/` of uncompressed 24-bit TIFFs named `<screen_id>_NNNNN.tif` from 0 (disguise `.seq` ingest convention): `auto` emits it iff the project's `output.target == "disguise"`, `tiff` forces it, `none` suppresses it. Mapping-aware: with `--screen-mapping` dots are tiled inside each cabinet's `input_rect_px`, honoring absent/non-uniform cabinets; without it, the uniform grid is used (even-divisibility required). Identity is carried in each dot's blink sequence (binary + even parity), not appearance — no dictionary-capacity limit. Result reports `n_dots` and `n_frames`. |
| `lmt visual decode-structured-light <input> <sl_meta> --out <corr.json> [--sentinel-threshold F] [--screen-roi X,Y,W,H] [--emit-debug-image]` | destructive | Decode a recorded structured-light capture (video, frame-image directory, or a disguise `.seq` directory of 10-bit `.dpx` frames — DPX is read by a built-in parser and downscaled to 8-bit, no transcode needed) into a provenance-stamped screen↔camera correspondence file (`screen_id`, `sl_meta_sha256`, `camera_image_size`, `source_input`, `screen_roi`, points). Three-pass temporal frontend that decides by **change, not brightness** so it works on any-brightness textured static backgrounds with off-screen moving objects: Pass 1 per-pixel temporal range (max−min) → auto screen ROI (largest solid activity rectangle; `--screen-roi X,Y,W,H` overrides; auto-derive failure → `detection_failed`; auto ROI keys off the global peak temporal range, so a dim/oblique screen filmed alongside a brighter off-screen *moving* object may be missed — pass `--screen-roi` for that case); Pass 2 ROI-restricted white-sentinel mean + plateau indexing; Pass 3 ROI Otsu seeding (recovers the all-off `id=0`) + dot shape/size filter + per-dot relative (own min/max) bit reading + binary+parity decode gate. `corr.json` records the `screen_roi` actually used (detection provenance; `reconstruct-structured-light` ignores it). `--emit-debug-image` additionally writes `<out>.debug.png` (a black-background white-dot seed mask) for eyeball QA. `--sentinel-threshold` (default 0.85) now applies to the ROI-region mean. `decode_failed` (18) if sentinels/plateaus don't parse; `detection_failed` (13) if the ROI can't be auto-derived or too few dots decode. |
| `lmt visual reconstruct <project> <screen_id> --capture-manifest <json> [--method charuco]` | destructive | Multi-view photos → `measurements/measured.yaml` + `measurements/<screen_id>_cabinet_pose_report.json` (model-constrained BA, zero total station) |
| `lmt visual reconstruct-structured-light <project> <screen_id> --sl-meta <json> --intrinsics <json> --corr <c.json> ...` | destructive | Reconstruct a metric per-cabinet 3D model from N per-pose structured-light correspondence files (decode-structured-light output). Provenance-gated: all `--corr` must share one `screen_id` + `sl_meta_sha256` matching `--sl-meta`, and match the project screen; `sl_meta` is schema-validated and its cabinet set must equal the project's present cells (stale meta / edited layout → `invalid_input`). `p_local` derives from canonical `sl_meta` dot `(u,v)` (the per-pose corr `(u,v)` is ignored). Runs the SAME model-constrained BA as `reconstruct` (root cabinet = world gauge, scale from pixel pitch), writing `measurements/measured.yaml` + `<screen>_cabinet_pose_report.json` (reuses `VisualReconstructResult`). Default-ON per-observation geometric outlier rejection (Stage A PnP-RANSAC pre-clean + Stage B global robust-residual trim); rejection counts surface in the result envelope (`ba_observations_total`/`ba_observations_used`/`ba_rejected`) and per-cabinet in the pose report (`rejected_points`); high-rejection cabinets emit a `high_rejection` warning. Convex/concave undecidable or a 2-view coherent conflict → `observability_failed`(17) BEFORE any file write (no silent wrong measured.yaml). No new flags/error codes. Errors: `invalid_input`(3), `intrinsics_invalid`(16), `detection_failed`(13), `observability_failed`(17), `ba_diverged`(14). NOTE: this is a separate subcommand; `reconstruct --method structured-light` stays `unsupported`(7). |
| `lmt visual simulate <config> --out <dir>` | destructive | Generate a synthetic geometry dataset (`scene.npz` + `meta.json`) for BA validation |
| `lmt visual eval <dataset> [--method charuco] [--seed-matrix <list>]` | write_safe | Evaluate a method vs ground truth on a synthetic dataset (gauge-invariant metrics) |
| `lmt visual compare-known <report.json> <known.json>` | write_safe | Compare a `cabinet_pose_report.json` against known monitor geometry — per-cabinet size error (from corners), per-pair distance error (from positions), per-pair angle error (from normals), + pass/fail vs thresholds (size≤2.0mm / distance≤3.0mm / angle≤0.3°). Reads two JSON files, writes nothing. |
| `lmt visual plan-capture <project> <screen_id> --image-size <WxH> (--hfov-deg <f> \| --vfov-deg <f>) --standoff <MIN..MAX> --height <MIN..MAX> [--target-mm 3.0] [--trials 20] [--seed 0]` | write_safe | Camera capture-guidance planner: from a screen's geometry (`project.yaml` cabinet grid + curved/flat shape) and the camera intrinsics (sensor `WxH` + one FOV), recommend a set of capture **stations** (position/aim/standoff/height/role/covered-cabinets) plus a per-cabinet **coverage** report (`p95_residual_mm` — `null` when not reconstructable, `reconstructable`/`low_observation`/`bridged`/`pass`) and `unreachable_regions`. Returns a `CapturePlan` in the envelope; **writes nothing**. Per-sample-point visibility (cheirality/frame/incidence) aggregated to the real reconstruct observability gate (≥2 views, ≥8 obs/cabinet, ≥4 pts/view); a recipe seed (FOV-fill standoff + front fan + top/bottom) is refined by a greedy optimizer over a reachable shell (standoff×height range). Exactly one of `--hfov-deg`/`--vfov-deg`; bad `WxH`/range/screen → `invalid_input`(3) / `not_found`(3). Residuals are a guidance-grade prediction (visibility-gated triangulation feasibility, not full BA). |
| `lmt visual capture-card <project> <screen_id> <same flags as plan-capture>` | read_only | Render the capture plan as a **self-contained HTML guidance card** (inline SVG, no external deps): a top-down plan view (screen footprint + station dots + aim arrows), a front-elevation coverage heatmap (cabinet grid colored 达标/低观测/超目标/不可重建), and a per-station table + unreachable-region warnings. Human mode → raw HTML to stdout (`... > card.html`); `--json` wraps `{html_content}`. Runs the same planner as `plan-capture` (no file writing). |

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

The entire `visual` command group — `calibrate`, `generate-pattern`,
`generate-structured-light`, `decode-structured-light`, `reconstruct`,
`reconstruct-structured-light`, `simulate`, `eval`, `compare-known` — is
CLI-only by design: it has no `#[tauri::command]` shim and is not registered in
the GUI's `generate_handler!`. The camera/structured-light pipeline is an
agent/headless workflow (long-running sidecar runs, no native-webview
dependency), so the deliverables are files on disk that any front-end can
consume. `generate-structured-light`, `decode-structured-light`, and
`reconstruct-structured-light` follow this convention. The service-layer helpers
(`lmt_app::visual::run_generate_structured_light` / `run_decode_structured_light`
/ `run_reconstruct_structured_light`) are plain functions, so a future GUI shim
is a thin transport wrapper if one is ever needed.

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
| `detection_failed` | 13 | ChArUco/checkerboard corner detection found too few corners in one or more frames |
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
