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

### Not exposed in CLI

- **`save-pdf`** — instruction-card PDF rendering goes through the platform
  native WebView (WKWebView on macOS, WebView2 on Windows) and only works inside
  the Tauri GUI process. CLI agents can get the HTML from `instruction-card`
  and run their own renderer (headless Chrome, wkhtmltopdf, etc.).
- **`seed-example`** — pulls examples from the Tauri bundle's `resource_dir`,
  which doesn't exist for a headless CLI. Agents working from a checkout can
  copy `examples/<name>/` themselves.

## Global flags

| Flag | Meaning |
| --- | --- |
| `--json` | Emit machine-stable envelope output. Success → stdout; failure → stderr; nothing else gets mixed in (no tracing, no colors). |
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
| `write_safe` | yes (no `--yes`) | `project add-recent` (still honors `--dry-run`) |
| `destructive` | no (requires `--yes` or `--dry-run`) | `project remove-recent` / `save`, `total-station import`, `reconstruct surface`, `export obj` |

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
