# CLAUDE.md

<!-- DOCSMITH:KNOWLEDGE:BEGIN -->
## Knowledge Base (Managed by Docsmith)

- Knowledge entrypoint: `.claude/knowledge/_INDEX.md`
- Config file: `.claude/knowledge.json`

### Current Sources
- `help-disguise-one` (262 files) → `.claude/knowledge/help-disguise-one/`
- `trimble-sx-docs` (33 files) → `.claude/knowledge/trimble-sx-docs/`
- `ue57-docs` (411 files) → `.claude/knowledge/ue57-docs/`

### Query Protocol
1. Read `.claude/knowledge/_INDEX.md` to route to the relevant source.
2. Open `<source>/_INDEX.md` and shortlist target documents by `topic/summary/keywords`.
3. Read target file TL;DR first, then read full content when needed.
4. Before answering, prioritize evidence from `KnowledgeBase docs`; use external knowledge only when KB coverage is insufficient.
5. In every answer, include:
   - `Knowledge Sources`: exact KB document paths used.
   - `External Inputs`: non-KB knowledge used and why.
   - If no KB match: `No relevant KnowledgeBase docs found`.

### Refresh Command
```bash
.venv/bin/python -m cli --project-links --refresh-index .
```
<!-- DOCSMITH:KNOWLEDGE:END -->

## CLI 底座维护契约(必读)

本项目同时通过 Tauri GUI 与 `lmt` CLI 两个 transport 暴露功能。两边共用同一个
`lmt-app` 服务层。**任何新增功能从设计阶段就必须考虑 CLI 暴露**,不能只做 GUI 然后
说"以后补 CLI"——历史经验是"以后"永远不来,CLI 会逐渐落后于 GUI 直到失去 agent
可调价值。

### 新功能开发规则

| 规则 | 说明 |
| --- | --- |
| **业务逻辑写在 `crates/lmt-app`,不写在 `src-tauri`** | `#[tauri::command]` 只允许做 transport 翻译(`State<Db>` → `Db`、`String` → `&Path`、`AppHandle` → 解析 `app_data_dir`),业务调用 `lmt_app::xxx::run_xxx`。 |
| **DTO / error 写在 `crates/lmt-shared`** | 任何新增对外暴露的类型放 `lmt-shared/src/dto.rs` 或新 module,**派生 `serde::{Serialize, Deserialize} + schemars::JsonSchema`**(纯类型);引用 `lmt-core` 域类型的 DTO 放 `schema::dump_all()` 的 `incomplete` 列表并写明原因。 |
| **错误新分类必须三处同步** | `lmt_shared::envelope::error_codes::*` 加常量、`lmt_shared::exit_codes::*` 加退出码、`docs/agents-cli.md` 加错误码对照表。三处不同步 = 错误码契约破。 |
| **新 Tauri command 同步加 CLI 子命令** | 每个 `#[tauri::command]` 注册到 `src-tauri/src/lib.rs` 时,checklist:① `crates/lmt-cli/src/cli.rs` 加 clap subcommand;② `crates/lmt-cli/src/commands/<group>.rs` 实现;③ destructive 操作走 `gate_destructive` + `--yes` / `--dry-run`;④ DB 命令选 `open_db`(写)vs `open_db_readonly`(读)。 |
| **CLI 必须有 E2E 测试** | 新子命令同步加 `crates/lmt-cli/tests/cli_e2e.rs` case:happy / refuse / dry-run / error envelope 四类至少各一。 |
| **AGENTS doc 必须更新** | `docs/agents-cli.md` 的命令表、side_effect 标注、错误码表三处任一变化都要同步。 |
| **故意不暴露的命令也要写明** | 跟 GUI 桌面进程紧耦合的(原生 webview、`AppHandle::path()::resource_dir()` 等)不暴露 CLI,但 `docs/agents-cli.md` 的 "Not exposed in CLI" 段落必须列出 + 说明替代方案。 |

### 开发规划必须把 CLI 接口列为交付项

新功能的实施计划里(spec / design / PR 描述)必须把以下作为独立任务:

1. **lmt-app helper 设计** — service-layer 函数签名、错误返回、是否需要 DB。
2. **Tauri shim** — `#[tauri::command]` thin wrapper(`src-tauri/src/commands/`)。
3. **CLI 子命令** — clap struct + commands module + 错误映射 + dry-run 路径。
4. **CLI E2E 测试** — 至少 happy + refuse + envelope。
5. **docs/agents-cli.md 更新** — 命令表新增一行,可能还要更新错误码表 / side_effect 类别。
6. **DTO schemars 派生** — 新类型加 `JsonSchema`,加进 `schema::dump_all()`。

任何一项缺失,都视为"功能没完成",不能合并。**如果是与 GUI 桌面进程紧耦合、无法 CLI
化的命令**(原生 webview、resource_dir 等),仍要写第 5 步,在 "Not exposed in CLI"
段落列出 + 解释技术原因 + 给 agent 一个替代路径(例如"用 instruction-card 拿 HTML
自己渲染")。

### 不要做的事

- **不要在 `src-tauri/src/commands/*.rs` 里写业务逻辑**:那里只剩 transport
  翻译,所有逻辑应在 `lmt-app`。在 src-tauri 写业务等于让 CLI 拿不到。
- **不要让 `lmt-shared` / `lmt-app` 依赖 `tauri`**:这两个 crate 的纯净是
  CLI 能存在的前提。它们当前的依赖白名单见 `Cargo.toml`。
- **不要在 `--json` 模式下 println! / 走 stdout 写非 envelope 内容**:agent 解析
  会破。CLI `output.rs::ok` / `err` 是唯一允许 stdout/stderr 的出口。
- **不要把 `tauri::AppHandle` 之类的 transport-bound 类型签名渗到 `lmt-app`**:
  例如 PDF render 用闭包注入 `render: impl FnOnce(&str, &Path) -> LmtResult<()>`
  是正确做法;直接接 `AppHandle` 就把 lmt-app 拖下水了。

### 自检命令

新功能合并前跑一遍:

```bash
cargo test --workspace                     # 全测试(含 lmt-cli E2E)
./target/debug/lmt --json schema | jq      # 新 DTO 是否真的进了 schema dump
./target/debug/lmt --help                  # 新子命令是否注册
./target/debug/lmt <new-cmd> --help        # 子命令文档是否人话
```

任何一步缺失或不通,等于 CLI 底座出现了 GUI 没有的 gap,后续要补的成本会指数级上涨。
