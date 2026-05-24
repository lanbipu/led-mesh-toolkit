# lmt CLI 功能完整 + Agent 可自助发现 Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 让 `lmt` CLI 在 Terminal 与 Claude Code 里能自助发现并执行项目的全部功能——补齐 Contract Manifest(可发现性)、seed-example(功能完整)、`--output` 三档与 `--no-color`/`--no-input`(调用友好)、`version`/`completion`(自描述)。

**Architecture:** 全部改动落在 transport / 自描述层(`lmt-cli` + `lmt-shared`),不碰 `lmt-core` 几何算法,不改现有 `Envelope`/`exit_codes` 语义,因此现有 ~20 个 `cli_e2e` 用例保持绿。新增命令一律走现有 `output::ok/err` + `Mode` 约定,业务逻辑调 `lmt-app` helper(`seed_example_to_dir` 已存在)。Contract Manifest 作为新的 canonical 自描述源,`--json schema` 仍保留(dump DTO JsonSchema),二者互补:manifest 答"有哪些操作",schema 答"数据长什么样"。

**Tech Stack:** Rust 2021 / clap 4(derive + env)/ clap_complete / include_dir / schemars 0.8 / serde / assert_cmd + predicates(E2E)。

**本 plan 明确不做(范围声明,非遗漏):**
- MCP server 与 Skill Package(用户已确认本轮只做 CLI)。
- envelope 加 `operation_id`/`status` 运行时字段、exit code 重映射到 spec §5——这些是为跨适配器一致性,纯 CLI 单 app 用价值低,且是 breaking change。Manifest 在自身结构里携带 `operation_id`,已足够让 Claude Code 发现命令,无需改运行时 envelope。**这些推迟项将来加 MCP 时怎么补,见文末 §面向 MCP / Skill 的前瞻设计——本 plan 已确保它们不堵路。**
- `reconstruct surface` 的 ndjson 进度事件与 cancellation——属算法层改造(独立 L 工作量)。本 plan 在 Phase 2 把 `--output ndjson` 的语法与单事件输出框架建好,长任务多事件流留作后续独立 plan。

---

## File Structure

| 文件 | 责任 | 本 plan 动作 |
|---|---|---|
| `crates/lmt-shared/src/manifest.rs` | Contract Manifest 结构定义 + `build()` 硬编码 operation 清单 | **新建**(Phase 1) |
| `crates/lmt-shared/src/lib.rs` | crate 模块导出 | 改:`pub mod manifest;`(Phase 1) |
| `crates/lmt-shared/src/schema.rs` | DTO JsonSchema dump | 改:把 manifest 类型加进 dump(Phase 1) |
| `crates/lmt-cli/src/commands/manifest.rs` | `lmt manifest` 子命令(read-only) | **新建**(Phase 1) |
| `crates/lmt-cli/src/commands/version.rs` | `lmt version` 子命令(read-only) | **新建**(Phase 3) |
| `crates/lmt-cli/src/commands/seed.rs` | `lmt seed-example` 子命令(destructive) | **新建**(Phase 4) |
| `crates/lmt-cli/src/cli.rs` | clap 命令树 + 全局 flag | 改:`--output`/`--no-color`/`--no-input`,新增子命令(Phase 1-4) |
| `crates/lmt-cli/src/commands/mod.rs` | dispatch | 改:路由新子命令 + 传 `OutputFormat`(Phase 1-4) |
| `crates/lmt-cli/src/output.rs` | 输出模式 + envelope writer | 改:`Mode` 加 `Ndjson` + ndjson 单事件(Phase 2) |
| `crates/lmt-cli/src/main.rs` | 入口 + early `--json` peek | 改:peek 识别 `--output`(Phase 2) |
| `crates/lmt-cli/Cargo.toml` | 依赖 | 改:`clap_complete`(Phase 3) |
| `crates/lmt-app/Cargo.toml` | 依赖 | 改:`include_dir`(Phase 4) |
| `crates/lmt-app/src/projects.rs` | seed 业务 + examples 嵌入 | 改:`seed_embedded_example` / `embedded_example_names`(Phase 4) |
| `crates/lmt-cli/tests/cli_e2e.rs` | E2E | 改:每个新命令加用例(Phase 1-4) |
| `docs/agents-cli.md` | Agent 契约文档 | 改:命令表 + 新命令(Phase 1,3,4) |
| `docs/contract-manifest.json` | manifest 快照 | **新建**:由 `lmt manifest --json` 生成(Phase 1) |

---

## Phase 1 — `lmt manifest`(Contract Manifest:可发现性核心)

让 Claude Code / agent 跑一条 `lmt --json manifest` 就拿到全部操作清单(operation_id、summary、cli 命令串、side_effect、是否支持 dry-run/stdin、输出类型名、可能的 exit codes)。这是"功能可发现"的地基。

### Task 1.1: 定义 Contract Manifest 结构与 `build()`

**Files:**
- Create: `crates/lmt-shared/src/manifest.rs`
- Modify: `crates/lmt-shared/src/lib.rs:15`(模块导出区)

- [ ] **Step 1: 写失败测试**

在 `crates/lmt-shared/src/manifest.rs` 末尾(先连同主体一起建文件,但 Step 1 只为跑红，主体函数留空壳):

```rust
//! Contract Manifest —— canonical 操作清单。
//!
//! 回答 "这个 app 有哪些操作"(operation_id + cli 命令 + side_effect);
//! 数据形状由 `schema::dump_all()` 的 JsonSchema 回答。两者互补。
//!
//! 新增 / 删除 CLI 子命令时,必须同步更新 `build()` 的 operations 列表——
//! 这是 manifest 作为契约源的维护成本,也是 `docs/agents-cli.md` 命令表的
//! 机器可读对应物。

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum SideEffect {
    ReadOnly,
    WriteSafe,
    Destructive,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct Operation {
    /// canonical 标识,形如 "project.list_recent"。跨工具稳定。
    pub operation_id: String,
    /// 一句话用途。
    pub summary: String,
    /// 规范 CLI 调用串,如 "lmt project list-recent"。
    pub cli: String,
    pub side_effect: SideEffect,
    /// destructive 操作需 --yes 或 --dry-run;此处声明是否支持 --dry-run 预演。
    pub supports_dry_run: bool,
    /// 是否从 stdin 读复杂输入(如 project save 的 YAML/JSON)。
    pub supports_stdin: bool,
    /// 多次调用结果一致。MCP `idempotentHint` 的数据源(前瞻字段,CLI 当前不用)。
    pub idempotent: bool,
    /// 与外部世界交互、不可纯回滚。MCP `openWorldHint` 的数据源(前瞻字段)。
    pub open_world: bool,
    /// 成功输出对应 `schema dump` 里的类型名;无固定 DTO 时为 None。
    pub output_type: Option<String>,
    /// 该操作可能返回的退出码集合(见 lmt_shared::exit_codes)。
    pub exit_codes: Vec<i32>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct ContractManifest {
    pub contract_version: String,
    pub schema_version: String,
    pub operations: Vec<Operation>,
}

pub fn build() -> ContractManifest {
    todo!("filled in Step 3")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn manifest_lists_all_known_operations_with_stable_ids() {
        let m = build();
        assert_eq!(m.contract_version, "1.0");
        assert_eq!(m.schema_version, crate::envelope::SCHEMA_VERSION);

        let ids: Vec<&str> = m.operations.iter().map(|o| o.operation_id.as_str()).collect();
        for expected in [
            "schema",
            "manifest",
            "project.list_recent",
            "project.add_recent",
            "project.remove_recent",
            "project.load",
            "project.save",
            "measurements.load",
            "total_station.import",
            "total_station.instruction_card",
            "reconstruct.surface",
            "reconstruct.list_runs",
            "reconstruct.get_run_report",
            "export.obj",
        ] {
            assert!(ids.contains(&expected), "manifest missing operation_id {expected}; got {ids:?}");
        }
        assert_eq!(m.operations.len(), 14, "operation count changed — update both build() and this test");
    }

    #[test]
    fn every_operation_has_nonempty_cli_and_valid_exit_zero() {
        let m = build();
        for op in &m.operations {
            assert!(op.cli.starts_with("lmt "), "cli must start with 'lmt ': {}", op.cli);
            assert!(op.exit_codes.contains(&0), "{} must allow exit 0", op.operation_id);
        }
    }

    #[test]
    fn destructive_ops_support_dry_run() {
        let m = build();
        for op in &m.operations {
            if op.side_effect == SideEffect::Destructive {
                assert!(op.supports_dry_run, "{} is destructive but no dry-run", op.operation_id);
            }
        }
    }

    #[test]
    fn manifest_carries_mcp_annotation_source() {
        // 前瞻:idempotent / open_world 是将来 MCP annotation 的数据源。
        // idempotent 严格定义 = 重复相同调用对可观测状态无额外改变。
        // 锁住关键不变量,避免有人改 build() 时把这些语义改坏。
        let m = build();
        let find = |id: &str| m.operations.iter().find(|o| o.operation_id == id).unwrap();
        assert!(find("project.list_recent").idempotent, "read-only is idempotent");
        assert!(find("project.remove_recent").idempotent, "delete is no-op-safe on retry");
        // 有副作用累积的写操作必须 NOT idempotent,否则 MCP 会错误地自动重试。
        for id in [
            "project.add_recent",
            "project.save",
            "total_station.import",
            "reconstruct.surface",
            "export.obj",
        ] {
            assert!(!find(id).idempotent, "{id} mutates observable state -> not idempotent");
        }
        assert!(m.operations.iter().all(|o| !o.open_world), "no operation calls external APIs yet");
    }
}
```

在 `crates/lmt-shared/src/lib.rs` 模块区(第 10-15 行那组 `pub mod`)加一行,放在 `pub mod exit_codes;` 之后:

```rust
pub mod manifest;
```

- [ ] **Step 2: 跑测试确认失败**

Run: `cargo test -p lmt-shared manifest:: 2>&1 | tail -20`
Expected: 编译通过但 panic —— `not yet implemented` / `todo!`,三个测试 FAIL。

- [ ] **Step 3: 实现 `build()`**

把 `manifest.rs` 里的 `pub fn build()` 替换为完整实现:

```rust
pub fn build() -> ContractManifest {
    use SideEffect::*;
    // 小 helper 减少重复(DRY)。
    fn op(
        id: &str,
        summary: &str,
        cli: &str,
        se: SideEffect,
        dry: bool,
        stdin: bool,
        idem: bool,
        out: Option<&str>,
        codes: &[i32],
    ) -> Operation {
        Operation {
            operation_id: id.into(),
            summary: summary.into(),
            cli: cli.into(),
            side_effect: se,
            supports_dry_run: dry,
            supports_stdin: stdin,
            idempotent: idem,
            // 本项目所有操作都是本地文件 / sqlite,无外部网络调用,故恒 false。
            // 将来若新增调用外部 API 的操作,这里改 true(MCP openWorldHint 数据源)。
            open_world: false,
            output_type: out.map(|s| s.into()),
            exit_codes: codes.to_vec(),
        }
    }

    // idem 参数 = idempotent,严格定义为"重复相同调用对可观测状态无额外改变"。
    // 只有 read-only(无写)与 remove(删除天然 no-op-safe,删两次最终态一致)为 true。
    // 其余写操作都有副作用累积,标 false:add_recent 每次改 last_opened_at、
    // import 滚动 .bak + 重写 report、surface 插新 run row、save/export 覆盖写 +
    // 写 DB metadata。标 false 是为了防止将来 MCP/Skill 据此做错误的自动重试 / dedup。
    let operations = vec![
        op("schema", "Dump JsonSchema of all public DTOs + envelope + error types",
           "lmt schema", ReadOnly, false, false, true, None, &[0]),
        op("manifest", "Dump the Contract Manifest (this operation list)",
           "lmt manifest", ReadOnly, false, false, true, Some("ContractManifest"), &[0]),
        op("project.list_recent", "List recent_projects rows",
           "lmt project list-recent", ReadOnly, false, false, true, Some("RecentProject"), &[0, 2, 5]),
        op("project.add_recent", "Upsert a recent-projects row (normalized path)",
           "lmt project add-recent <abs_path> <display_name>", WriteSafe, true, false, false, Some("RecentProject"), &[0, 2, 4, 5]),
        op("project.remove_recent", "Delete a recent-projects row by id",
           "lmt project remove-recent <id>", Destructive, true, false, true, None, &[0, 2, 5]),
        op("project.load", "Read <dir>/project.yaml into ProjectConfig",
           "lmt project load <abs_path>", ReadOnly, false, false, true, Some("ProjectConfig"), &[0, 2, 3, 4, 6]),
        op("project.save", "Atomic write <dir>/project.yaml from YAML/JSON (stdin or --input)",
           "lmt project save <abs_path> [--input <path>]", Destructive, true, true, false, None, &[0, 2, 4, 6]),
        op("measurements.load", "Read a measured.yaml",
           "lmt measurements load <path>", ReadOnly, false, false, true, None, &[0, 2, 3, 4, 6]),
        op("total_station.import", "Trimble CSV -> measurements/measured.yaml + import_report.json",
           "lmt total-station import <project> <screen_id> <csv> [--mode grid|scatter] [--columns <spec>]", Destructive, true, false, false, Some("TotalStationImportResult"), &[0, 2, 3, 4]),
        op("total_station.instruction_card", "Render instruction-card HTML on stdout (no PDF)",
           "lmt total-station instruction-card <project> <screen_id>", ReadOnly, false, false, true, Some("InstructionCardResult"), &[0, 2, 3]),
        op("reconstruct.surface", "Run reconstruction, write report.json + reconstruction_runs row",
           "lmt reconstruct surface <project> <screen_id> <measurements_rel>", Destructive, true, false, false, None, &[0, 2, 3, 4, 5, 12]),
        op("reconstruct.list_runs", "List reconstruction_runs for a project",
           "lmt reconstruct list-runs <project> [--screen-id <id>]", ReadOnly, false, false, true, Some("ReconstructionRun"), &[0, 2, 5]),
        op("reconstruct.get_run_report", "Return the full report.json for a run",
           "lmt reconstruct get-run-report <run_id>", ReadOnly, false, false, true, None, &[0, 2, 3, 4, 5]),
        op("export.obj", "Write an OBJ for a run (target: disguise|unreal|neutral)",
           "lmt export obj <run_id> <target> [--dst <path>]", Destructive, true, false, false, None, &[0, 2, 3, 4, 5]),
    ];

    ContractManifest {
        contract_version: "1.0".into(),
        schema_version: crate::envelope::SCHEMA_VERSION.into(),
        operations,
    }
}
```

- [ ] **Step 4: 跑测试确认通过**

Run: `cargo test -p lmt-shared manifest:: 2>&1 | tail -20`
Expected: 3 passed。

- [ ] **Step 5: 提交**

```bash
git add crates/lmt-shared/src/manifest.rs crates/lmt-shared/src/lib.rs
git commit -m "feat(shared): add Contract Manifest module + build() operation list

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>"
```

### Task 1.2: 把 manifest 类型加进 `schema dump`

**Files:**
- Modify: `crates/lmt-shared/src/schema.rs:26`(use 行)、`:60-67`(add! 区)、`:84-101`(测试)

- [ ] **Step 1: 写失败断言(扩现有测试)**

在 `schema.rs` 的 `dump_contains_known_types_and_incomplete_list` 测试里(第 90-100 行那个 `for expected in [...]` 数组),加入两个新类型名:

```rust
        for expected in [
            "RecentProject",
            "ProjectConfig",
            "TotalStationImportResult",
            "InstructionCardResult",
            "ReconstructionRun",
            "LmtError",
            "ApiError",
            "Envelope",
            "ErrorEnvelope",
            "ContractManifest",
            "Operation",
        ] {
```

- [ ] **Step 2: 跑测试确认失败**

Run: `cargo test -p lmt-shared schema::tests::dump_contains 2>&1 | tail -15`
Expected: FAIL —— `missing schema for ContractManifest`。

- [ ] **Step 3: 注册类型**

`schema.rs` 第 26 行 use 改为(加 `manifest`):

```rust
use crate::{dto, envelope, error, manifest};
```

在 `dump_all()` 的 `add!("ErrorEnvelope", ...)` 那行(约第 67 行)之后,加:

```rust
    // Contract Manifest
    add!("ContractManifest", manifest::ContractManifest);
    add!("Operation", manifest::Operation);
```

- [ ] **Step 4: 跑测试确认通过**

Run: `cargo test -p lmt-shared schema:: 2>&1 | tail -15`
Expected: all passed。

- [ ] **Step 5: 提交**

```bash
git add crates/lmt-shared/src/schema.rs
git commit -m "feat(shared): expose ContractManifest/Operation in schema dump

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>"
```

### Task 1.3: `lmt manifest` 子命令

**Files:**
- Create: `crates/lmt-cli/src/commands/manifest.rs`
- Modify: `crates/lmt-cli/src/cli.rs:71-72`(Schema 子命令旁)、`crates/lmt-cli/src/commands/mod.rs:4-10`(mod 声明)、`:33-40`(dispatch)

- [ ] **Step 1: 写失败 E2E 测试**

在 `crates/lmt-cli/tests/cli_e2e.rs` 的 `schema_json_envelope_has_known_types` 测试之后插入:

```rust
#[test]
fn manifest_json_lists_operations_with_ids() {
    let out = lmt().args(["--json", "manifest"]).assert().success().get_output().clone();
    let env: Value = serde_json::from_slice(&out.stdout).expect("stdout must be JSON envelope");
    assert_eq!(env["ok"], true);
    let ops = env["data"]["operations"].as_array().expect("operations array");
    let ids: Vec<&str> = ops.iter().map(|o| o["operation_id"].as_str().unwrap()).collect();
    assert!(ids.contains(&"reconstruct.surface"), "ids: {ids:?}");
    assert!(ids.contains(&"project.list_recent"), "ids: {ids:?}");
    assert_eq!(env["data"]["contract_version"], "1.0");
}

#[test]
fn manifest_human_mode_is_text_not_json() {
    let out = lmt().arg("manifest").assert().success().get_output().clone();
    let s = String::from_utf8_lossy(&out.stdout);
    assert!(serde_json::from_str::<Value>(&s).is_err(), "human mode should not be JSON: {s}");
    assert!(s.contains("reconstruct.surface"), "stdout: {s}");
}
```

- [ ] **Step 2: 跑测试确认失败**

Run: `cargo test -p lmt-cli manifest_ 2>&1 | tail -20`
Expected: FAIL —— clap 报 `unrecognized subcommand 'manifest'`,断言 `success()` 失败。

- [ ] **Step 3: 实现子命令**

Create `crates/lmt-cli/src/commands/manifest.rs`:

```rust
//! `lmt manifest` —— dump Contract Manifest(operation 清单)。
//!
//! side_effect: read_only;不需要 DB / project / network。
//! 与 `schema` 互补:manifest 答 "有哪些操作",schema 答 "数据形状"。

use crate::output::{self, Mode};
use std::io::Write;

pub fn run(mode: Mode) -> i32 {
    let manifest = lmt_shared::manifest::build();
    output::ok(mode, manifest, |m| {
        // human 模式:每行一个操作的紧凑摘要。用 writeln! 避免 BrokenPipe panic。
        let mut out = std::io::stdout();
        let _ = writeln!(
            out,
            "Contract v{} (schema v{}) — {} operations:",
            m.contract_version,
            m.schema_version,
            m.operations.len()
        );
        for op in &m.operations {
            let _ = writeln!(
                out,
                "  {:<32} [{:?}]  {}",
                op.operation_id, op.side_effect, op.cli
            );
        }
        let _ = writeln!(out);
        let _ = writeln!(out, "Run `lmt --json manifest` for the machine-readable form.");
    })
}
```

`crates/lmt-cli/src/cli.rs`:在 `Command` enum 里 `Schema,`(第 72 行)之后加:

```rust
    /// dump Contract Manifest —— 全部 operation 的清单(operation_id / cli / side_effect)。
    Manifest,
```

`crates/lmt-cli/src/commands/mod.rs`:在 mod 声明区(第 4-10 行)加 `mod manifest;`(按字母序放在 `mod export;` 后、`mod measurements;` 前即可);在 `dispatch` 的 `match cli.command` 里(`Command::Schema => ...` 那行旁)加:

```rust
        Command::Manifest => manifest::run(mode),
```

- [ ] **Step 4: 跑测试确认通过**

Run: `cargo test -p lmt-cli manifest_ 2>&1 | tail -20`
Expected: 2 passed。

- [ ] **Step 5: 跑全量回归 + 提交**

Run: `cargo test --workspace 2>&1 | tail -15`
Expected: 全绿(确认没破坏现有 ~20 个 E2E)。

```bash
git add crates/lmt-cli/src/commands/manifest.rs crates/lmt-cli/src/cli.rs crates/lmt-cli/src/commands/mod.rs crates/lmt-cli/tests/cli_e2e.rs
git commit -m "feat(cli): add 'lmt manifest' self-describe subcommand

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>"
```

### Task 1.4: 生成 manifest 快照 + 更新 AGENTS doc

**Files:**
- Create: `docs/contract-manifest.json`
- Modify: `docs/agents-cli.md`(命令表 + 自描述段)

- [ ] **Step 1: 生成快照**

Run:
```bash
cargo build -p lmt-cli && ./target/debug/lmt --json manifest | jq . > docs/contract-manifest.json
```
Expected: `docs/contract-manifest.json` 写出,`jq .` 能解析(非空 operations 数组)。

- [ ] **Step 2: 验证快照内容**

Run: `jq -r '.data.operations[].operation_id' docs/contract-manifest.json 2>/dev/null || jq -r '.operations[].operation_id' docs/contract-manifest.json`
Expected: 列出 13 个 operation_id(注意:`lmt --json manifest` 输出是 envelope,operations 在 `.data.operations`;如希望快照为裸 manifest,改用 `./target/debug/lmt --json manifest | jq .data > docs/contract-manifest.json`)。统一选裸 manifest 形式重跑 Step 1 的 `jq .data` 版本。

- [ ] **Step 3: 更新 `docs/agents-cli.md`**

在 `## Command tree` 表格顶部(`schema` 行之后)新增一行:

```markdown
| `lmt manifest` | read_only | Dump Contract Manifest: all operations with operation_id / cli / side_effect / exit_codes |
```

并在 `## DTO / schema discovery` 段落末尾追加一段:

```markdown
## Operation discovery

Run `lmt --json manifest` to list every operation with its stable `operation_id`,
canonical CLI string, `side_effect`, and possible exit codes. This is the
machine-readable counterpart of the Command tree table above. A snapshot lives
at `docs/contract-manifest.json`. When you add/remove a subcommand, regenerate
the snapshot and update `lmt_shared::manifest::build()`.
```

- [ ] **Step 4: 提交**

```bash
git add docs/contract-manifest.json docs/agents-cli.md
git commit -m "docs: add contract-manifest.json snapshot + manifest discovery in agents-cli

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>"
```

---

## Phase 2 — `--output text|json|ndjson` + `--no-color` / `--no-input`

把布尔 `--json` 升级为显式三档 `--output`(保留 `--json` 为 `--output json` 别名,旧脚本与现有 E2E 不破),补齐 spec §3.2 关键 flag。ndjson 模式下当前命令输出单条 `result` 事件(为将来 reconstruct 流式留接口)。

### Task 2.1: `Mode` 加 `Ndjson` + ndjson 单事件输出

**Files:**
- Modify: `crates/lmt-cli/src/output.rs:16-31`(Mode enum + from_flag)、`:40-52`(ok)

- [ ] **Step 1: 写失败单元测试**

在 `crates/lmt-cli/src/output.rs` 末尾加测试模块:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::Value;

    #[test]
    fn ndjson_ok_emits_single_result_event_with_final_true() {
        // 用一个内存 buffer 替代 stdout 不方便(ok 直接写 stdout)。
        // 改为验证事件构造函数 result_event 的形状。
        let ev = result_event(&serde_json::json!({"foo": 1}));
        let v: Value = serde_json::from_str(&ev).unwrap();
        assert_eq!(v["type"], "result");
        assert_eq!(v["final"], true);
        assert_eq!(v["status"], "ok");
        assert_eq!(v["data"]["foo"], 1);
        assert!(v["sequence"].is_number());
    }
}
```

- [ ] **Step 2: 跑测试确认失败**

Run: `cargo test -p lmt-cli --lib output::tests 2>&1 | tail -15`
Expected: 编译错误 —— `result_event` 未定义。

- [ ] **Step 3: 实现**

`output.rs` 的 `Mode` enum(第 18-21 行)改为:

```rust
#[derive(Debug, Clone, Copy)]
pub enum Mode {
    Human,
    Json,
    Ndjson,
}
```

把 `from_flag`(第 23-31 行)替换为 `from_format`(保留 `from_flag` 作兼容别名):

```rust
impl Mode {
    /// 由 --output 值映射。`from_flag` 保留给旧调用点(--json bool)。
    pub fn from_format(fmt: OutputFormat) -> Self {
        match fmt {
            OutputFormat::Text => Mode::Human,
            OutputFormat::Json => Mode::Json,
            OutputFormat::Ndjson => Mode::Ndjson,
        }
    }
    pub fn from_flag(json: bool) -> Self {
        if json { Mode::Json } else { Mode::Human }
    }
}
```

在文件顶部 `use` 之后加 `OutputFormat`(它将在 cli.rs 定义并 re-export;这里从 crate::cli 引):

```rust
use crate::cli::OutputFormat;
```

在 `ok()` 之前加事件构造 helper(单调递增 sequence 对单事件够用,固定 0):

```rust
/// 构造一条 ndjson `result` 事件(单次命令的终态)。长任务将来可在前面插
/// `start` / `progress` 事件,此处只覆盖单结果场景。
pub fn result_event<T: Serialize>(data: &T) -> String {
    let ev = serde_json::json!({
        "type": "result",
        "sequence": 0,
        "final": true,
        "status": "ok",
        "schema_version": lmt_shared::envelope::SCHEMA_VERSION,
        "data": data,
    });
    serde_json::to_string(&ev).expect("result event is serializable")
}
```

把 `ok()`(第 40-52 行)的 `match mode` 扩为三分支:

```rust
pub fn ok<T: Serialize>(mode: Mode, data: T, summary: impl FnOnce(&T)) -> i32 {
    match mode {
        Mode::Json => {
            let env = Envelope::ok(data);
            let s = serde_json::to_string(&env).expect("Envelope is always serializable");
            let _ = writeln!(std::io::stdout(), "{s}");
        }
        Mode::Ndjson => {
            let s = result_event(&data);
            let _ = writeln!(std::io::stdout(), "{s}");
        }
        Mode::Human => {
            summary(&data);
        }
    }
    exit_codes::OK
}
```

把 `err()`(第 58-74 行)的 `match mode` 里 `Mode::Json` 分支改为同时覆盖 `Json | Ndjson`(失败统一走 ErrorEnvelope on stderr):

```rust
    match mode {
        Mode::Json | Mode::Ndjson => {
            let env = ErrorEnvelope::from_error(error);
            let s = serde_json::to_string(&env).expect("ErrorEnvelope is always serializable");
            let _ = writeln!(std::io::stderr(), "{s}");
        }
        Mode::Human => {
            let _ = writeln!(std::io::stderr(), "error: {} — {}", error.code, error.message);
            if let Some(details) = &error.details {
                let _ = writeln!(std::io::stderr(), "details: {details}");
            }
        }
    }
```

- [ ] **Step 4: 跑测试确认通过**

Run: `cargo test -p lmt-cli --lib output::tests 2>&1 | tail -15`
Expected: passed。

- [ ] **Step 5: 提交**

```bash
git add crates/lmt-cli/src/output.rs
git commit -m "feat(cli): add Ndjson output mode + single result event

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>"
```

### Task 2.2: `--output` / `--no-color` / `--no-input` flag + dispatch 接线

**Files:**
- Modify: `crates/lmt-cli/src/cli.rs:15-47`(全局 flag)、`crates/lmt-cli/src/commands/mod.rs:15-16`(mode 来源)、`crates/lmt-cli/src/main.rs:20`(early peek)

- [ ] **Step 1: 写失败 E2E 测试**

在 `cli_e2e.rs` 加:

```rust
#[test]
fn output_json_is_alias_for_legacy_json_flag() {
    let out = lmt().args(["--output", "json", "schema"]).assert().success().get_output().clone();
    let env: Value = serde_json::from_slice(&out.stdout).expect("stdout JSON envelope");
    assert_eq!(env["ok"], true);
}

#[test]
fn output_ndjson_schema_emits_result_event() {
    let out = lmt().args(["--output", "ndjson", "schema"]).assert().success().get_output().clone();
    let line = String::from_utf8_lossy(&out.stdout);
    let v: Value = serde_json::from_str(line.trim()).expect("one ndjson line");
    assert_eq!(v["type"], "result");
    assert_eq!(v["final"], true);
}

#[test]
fn legacy_json_flag_still_works() {
    lmt().args(["--json", "schema"]).assert().success();
}

#[test]
fn no_color_and_no_input_flags_accepted() {
    lmt().args(["--no-color", "--no-input", "schema"]).assert().success();
}

#[test]
fn output_equals_json_invalid_flag_yields_envelope_on_stderr() {
    // spec §3.1 要求 parser 接受 --key=value;machine 模式检测不能漏 --output=json,
    // 否则 parse error 会 fallback 到 human clap 输出而非 JSON envelope。
    let assert = lmt().args(["--output=json", "--bogus"]).assert().failure();
    let out = assert.get_output();
    assert_eq!(out.status.code(), Some(2));
    let env: Value = serde_json::from_slice(&out.stderr).expect("stderr JSON envelope");
    assert_eq!(env["ok"], false);
}
```

- [ ] **Step 2: 跑测试确认失败**

Run: `cargo test -p lmt-cli output_ndjson output_json no_color 2>&1 | tail -20`
Expected: FAIL —— clap 报 `--output` / `--no-color` / `--no-input` 未知。

- [ ] **Step 3: 实现 flag**

`crates/lmt-cli/src/cli.rs`:在 `use clap::{Parser, Subcommand};`(第 5 行)下加 `ValueEnum`:

```rust
use clap::{Parser, Subcommand, ValueEnum};
```

在文件靠近顶部(`Cli` struct 之前)定义 enum:

```rust
#[derive(Debug, Clone, Copy, ValueEnum)]
#[value(rename_all = "lowercase")]
pub enum OutputFormat {
    Text,
    Json,
    Ndjson,
}
```

把 `Cli` struct 的 `json` 字段(第 16-18 行)改为(保留 `json` 作隐藏别名 + 新增 `output`):

```rust
    /// 输出格式:text(人类,默认)/ json(单 envelope)/ ndjson(每行一事件)。
    #[arg(long, short = 'o', global = true, value_enum)]
    pub output: Option<OutputFormat>,

    /// [别名] 等价 `--output json`。保留兼容旧脚本。
    #[arg(long, global = true)]
    pub json: bool,

    /// 禁用 ANSI 颜色(human 模式当前本就无色,接受为 no-op 以满足契约;
    /// 同时尊重 NO_COLOR 环境变量)。
    #[arg(long, global = true)]
    pub no_color: bool,

    /// 拒绝任何交互提示(本 CLI 不发起交互,destructive 仍需 --yes;
    /// 接受此 flag 让 agent 调用显式无人值守)。
    #[arg(long, global = true)]
    pub no_input: bool,
```

在 `Cli` struct 上新增一个解析方法(放在 `cli.rs` 末尾):

```rust
impl Cli {
    /// 综合 --output / --json 别名 / NO_COLOR env 解析最终输出模式。
    /// 优先级:--output > --json > 默认 text。
    pub fn resolved_format(&self) -> OutputFormat {
        match self.output {
            Some(f) => f,
            None if self.json => OutputFormat::Json,
            None => OutputFormat::Text,
        }
    }
}
```

`crates/lmt-cli/src/commands/mod.rs`:把 `dispatch` 第 16 行 `let mode = Mode::from_flag(cli.json);` 改为:

```rust
    let mode = Mode::from_format(cli.resolved_format());
```

`crates/lmt-cli/src/main.rs`:把 machine-output 检测抽成一个顶层 helper(放 `fn main()` 之前),让 early tracing 抑制与 parse-error envelope 两处共用,且**同时认 `--output json`(双 token)与 `--output=json` / `-o=json`(单 token)**——spec §3.1 明确 parser 要接受 `--key=value`:

```rust
/// machine 模式信号:`--json`,或 `--output` / `-o` 指定 `json`|`ndjson`。
/// 双 token(`--output json`)、单 token(`--output=json` / `-o=json`)、compact(`-ojson`)都认。
fn wants_machine_output(argv: &[std::ffi::OsString]) -> bool {
    use std::ffi::OsStr;
    let is_val = |s: &OsStr| s == OsStr::new("json") || s == OsStr::new("ndjson");
    if argv.iter().any(|a| a == OsStr::new("--json")) {
        return true;
    }
    argv.iter().enumerate().any(|(i, a)| {
        if let Some(s) = a.to_str() {
            if let Some(v) = s.strip_prefix("--output=").or_else(|| s.strip_prefix("-o=")) {
                return v == "json" || v == "ndjson";
            }
            // compact short value: -ojson / -ondjson(clap 接受 -o<value> 无分隔符)
            if let Some(v) = s.strip_prefix("-o") {
                if v == "json" || v == "ndjson" {
                    return true;
                }
            }
        }
        (a == OsStr::new("--output") || a == OsStr::new("-o"))
            && argv.get(i + 1).map(|n| is_val(n)).unwrap_or(false)
    })
}
```

`main()` 第 20 行的 early peek 改为:

```rust
    let argv: Vec<std::ffi::OsString> = std::env::args_os().collect();
    let json_mode_early = wants_machine_output(&argv);
```

(其下的 `if !json_mode_early { tracing_subscriber... }` 不变。)

parse-error 分支(原第 50-52 行)里的 `wants_json` 判定改为复用同一 helper:

```rust
            let wants_json =
                !is_help_or_version && wants_machine_output(&std::env::args_os().collect::<Vec<_>>());
```

- [ ] **Step 4: 跑测试确认通过**

Run: `cargo test -p lmt-cli 2>&1 | tail -15`
Expected: 新增测试(含 `--output=json` envelope 用例)passed,且现有 E2E 全绿(`--json` 别名保活)。

- [ ] **Step 5: 提交**

```bash
git add crates/lmt-cli/src/cli.rs crates/lmt-cli/src/commands/mod.rs crates/lmt-cli/src/main.rs crates/lmt-cli/tests/cli_e2e.rs
git commit -m "feat(cli): add --output text|json|ndjson + --no-color/--no-input (--json kept as alias)

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>"
```

---

## Phase 3 — `lmt version`(JSON 版本元信息)+ `lmt completion <shell>`

补齐 spec §10.1 的两个自描述出口。`--version` 纯文本保留;新增 `version` 子命令给机器读;`completion` 生成 shell 补全。

### Task 3.1: `lmt version` 子命令

**Files:**
- Create: `crates/lmt-cli/src/commands/version.rs`
- Modify: `crates/lmt-cli/src/cli.rs`(Command enum)、`crates/lmt-cli/src/commands/mod.rs`

- [ ] **Step 1: 写失败 E2E 测试**

`cli_e2e.rs` 加:

```rust
#[test]
fn version_subcommand_json_has_version_and_schema() {
    let out = lmt().args(["--json", "version"]).assert().success().get_output().clone();
    let env: Value = serde_json::from_slice(&out.stdout).expect("JSON envelope");
    assert_eq!(env["ok"], true);
    assert!(env["data"]["version"].as_str().unwrap().len() > 0);
    assert_eq!(env["data"]["schema_version"], "1");
}
```

- [ ] **Step 2: 跑测试确认失败**

Run: `cargo test -p lmt-cli version_subcommand 2>&1 | tail -15`
Expected: FAIL —— `unrecognized subcommand 'version'`。

- [ ] **Step 3: 实现**

Create `crates/lmt-cli/src/commands/version.rs`:

```rust
//! `lmt version` —— 机器可读版本元信息(--version 纯文本出口保留)。
//! side_effect: read_only。

use crate::output::{self, Mode};
use serde::Serialize;
use std::io::Write;

#[derive(Serialize)]
struct VersionInfo {
    version: String,
    schema_version: String,
    contract_version: String,
}

pub fn run(mode: Mode) -> i32 {
    let info = VersionInfo {
        version: env!("CARGO_PKG_VERSION").to_string(),
        schema_version: lmt_shared::envelope::SCHEMA_VERSION.to_string(),
        contract_version: lmt_shared::manifest::build().contract_version,
    };
    output::ok(mode, info, |i| {
        let _ = writeln!(
            std::io::stdout(),
            "lmt {} (schema v{}, contract v{})",
            i.version, i.schema_version, i.contract_version
        );
    })
}
```

`cli.rs` Command enum 加(`Manifest,` 之后):

```rust
    /// 机器可读版本元信息(--version 是纯文本简版)。
    Version,
```

`commands/mod.rs`:加 `mod version;`,dispatch 加 `Command::Version => version::run(mode),`。

- [ ] **Step 4: 跑测试确认通过**

Run: `cargo test -p lmt-cli version_subcommand 2>&1 | tail -15`
Expected: passed。

- [ ] **Step 5: 提交**

```bash
git add crates/lmt-cli/src/commands/version.rs crates/lmt-cli/src/cli.rs crates/lmt-cli/src/commands/mod.rs crates/lmt-cli/tests/cli_e2e.rs
git commit -m "feat(cli): add 'lmt version' machine-readable subcommand

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>"
```

### Task 3.2: `lmt completion <shell>`

**Files:**
- Modify: `crates/lmt-cli/Cargo.toml`(加 `clap_complete`)、`crates/lmt-cli/src/cli.rs`、`crates/lmt-cli/src/commands/mod.rs`
- Create: `crates/lmt-cli/src/commands/completion.rs`

- [ ] **Step 1: 写失败 E2E 测试**

`cli_e2e.rs` 加:

```rust
#[test]
fn completion_bash_emits_script_to_stdout() {
    let out = lmt().args(["completion", "bash"]).assert().success().get_output().clone();
    let s = String::from_utf8_lossy(&out.stdout);
    assert!(s.contains("lmt"), "bash completion should mention lmt: first 80 = {:?}", &s[..s.len().min(80)]);
}
```

- [ ] **Step 2: 跑测试确认失败**

Run: `cargo test -p lmt-cli completion_bash 2>&1 | tail -15`
Expected: FAIL —— `unrecognized subcommand 'completion'`。

- [ ] **Step 3: 实现**

`crates/lmt-cli/Cargo.toml` 的 `[dependencies]` 加:

```toml
clap_complete = "4"
```

`cli.rs`:Command enum 加(注意需要 `clap_complete::Shell`):

```rust
    /// 生成 shell 补全脚本到 stdout(bash / zsh / fish / powershell / elvish)。
    Completion {
        /// 目标 shell。
        shell: clap_complete::Shell,
    },
```

Create `crates/lmt-cli/src/commands/completion.rs`:

```rust
//! `lmt completion <shell>` —— 生成 shell 补全脚本到 stdout。
//! side_effect: read_only;纯文本输出,不走 envelope(补全脚本不是数据)。

use crate::cli::Cli;
use clap::CommandFactory;
use clap_complete::Shell;

pub fn run(shell: Shell) -> i32 {
    let mut cmd = Cli::command();
    let bin = cmd.get_name().to_string();
    clap_complete::generate(shell, &mut cmd, bin, &mut std::io::stdout());
    lmt_shared::exit_codes::OK
}
```

`commands/mod.rs`:加 `mod completion;`;dispatch 加(注意它不吃 mode,直接 shell):

```rust
        Command::Completion { shell } => completion::run(shell),
```

- [ ] **Step 4: 跑测试确认通过**

Run: `cargo test -p lmt-cli completion_bash 2>&1 | tail -15`
Expected: passed。

- [ ] **Step 5: 提交**

```bash
git add crates/lmt-cli/Cargo.toml crates/lmt-cli/src/commands/completion.rs crates/lmt-cli/src/cli.rs crates/lmt-cli/src/commands/mod.rs crates/lmt-cli/tests/cli_e2e.rs
git commit -m "feat(cli): add 'lmt completion <shell>' via clap_complete

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>"
```

---

## Phase 4 — `lmt seed-example`(功能完整性:补齐 examples 工作流)

`lmt_app::projects::seed_example_to_dir` 已存在,但 CLI 未暴露——因 headless 无 Tauri `resource_dir` 定位 examples。用 `include_dir` 编译期嵌入 `examples/`,运行时写出到目标。

**前瞻关键**:嵌入 + seed 逻辑放在 **`lmt-app`**(不放 `lmt-cli`),让未来的 `lmt-mcp` crate 也能直接调同一个 `seed_embedded_example`,examples 只嵌入一份。把它放 cli 会逼 MCP 反向依赖 cli——方向错。这符合项目 CLAUDE.md "业务写 lmt-app,transport 只翻译" 的契约。

### Task 4.1: 在 lmt-app 嵌入 examples 并经 CLI 暴露

**Files:**
- Modify: `crates/lmt-app/Cargo.toml`(加 `include_dir` 到 `[dependencies]`)
- Modify: `crates/lmt-app/src/projects.rs`(加嵌入资源 + `seed_embedded_example` + `embedded_example_names`)
- Create: `crates/lmt-cli/src/commands/seed.rs`(thin transport wrapper)
- Modify: `crates/lmt-cli/src/cli.rs`、`crates/lmt-cli/src/commands/mod.rs`

- [ ] **Step 1: 写失败 E2E 测试**

`cli_e2e.rs` 加:

```rust
#[test]
fn seed_example_dry_run_does_not_write() {
    let tmp = TempDir::new().unwrap();
    let dst = tmp.path();
    let out = lmt()
        .args(["--json", "--dry-run", "seed-example", "curved-flat"])
        .arg(dst)
        .assert().success().get_output().clone();
    let env: Value = serde_json::from_slice(&out.stdout).expect("JSON envelope");
    assert_eq!(env["data"]["dry_run"], true);
    assert!(!dst.join("curved-flat/project.yaml").exists(), "dry-run must not write");
}

#[test]
fn seed_example_yes_writes_project_yaml() {
    let tmp = TempDir::new().unwrap();
    let dst = tmp.path();
    lmt().args(["--json", "--yes", "seed-example", "curved-flat"])
        .arg(dst)
        .assert().success();
    assert!(dst.join("curved-flat/project.yaml").is_file(), "expected seeded project.yaml");
}

#[test]
fn seed_example_unknown_name_is_not_found() {
    let tmp = TempDir::new().unwrap();
    let assert = lmt()
        .args(["--json", "--yes", "seed-example", "does-not-exist"])
        .arg(tmp.path())
        .assert().failure();
    // not_found -> exit 3
    assert_eq!(assert.get_output().status.code(), Some(3));
}

#[test]
fn seed_example_dry_run_unknown_name_fails_fast() {
    // dry-run preflight 必须对未知 name 失败,而不是报 ok 让 agent 误以为安全。
    let tmp = TempDir::new().unwrap();
    let assert = lmt()
        .args(["--json", "--dry-run", "seed-example", "does-not-exist"])
        .arg(tmp.path())
        .assert().failure();
    assert_eq!(assert.get_output().status.code(), Some(3));
}

#[test]
fn seed_example_refuses_existing_destination_and_leaves_it_intact() {
    let tmp = TempDir::new().unwrap();
    let dst = tmp.path();
    // 第一次 seed 成功
    lmt().args(["--json", "--yes", "seed-example", "curved-flat"]).arg(dst).assert().success();
    // 在目标里放一个 sentinel,证明第二次 seed 不碰它
    let sentinel = dst.join("curved-flat/SENTINEL.txt");
    std::fs::write(&sentinel, "keep-me").unwrap();
    // 第二次 seed 同目标 -> 拒绝(invalid_input -> exit 2),sentinel 原样保留
    let assert = lmt()
        .args(["--json", "--yes", "seed-example", "curved-flat"]).arg(dst)
        .assert().failure();
    assert_eq!(assert.get_output().status.code(), Some(2));
    assert_eq!(std::fs::read_to_string(&sentinel).unwrap(), "keep-me");
}
```

- [ ] **Step 2: 跑测试确认失败**

Run: `cargo test -p lmt-cli seed_example 2>&1 | tail -20`
Expected: FAIL —— `unrecognized subcommand 'seed-example'`。

- [ ] **Step 3: 实现**

`crates/lmt-app/Cargo.toml` `[dependencies]` 加(仅嵌入资源;seed 用目标同级 staging 目录做原子 rename,不需要 `tempfile`):

```toml
include_dir = "0.7"
```

在 `crates/lmt-app/src/projects.rs` 顶部 use 区(第 1-3 行那组)之下加嵌入资源,并在 `seed_example_to_dir` 之后加两个公共函数(CLI 与未来 MCP server 共用同一份):

```rust
use include_dir::{include_dir, Dir};

// examples/ 在编译期嵌入(相对 crates/lmt-app -> ../../examples)。
static EXAMPLES: Dir = include_dir!("$CARGO_MANIFEST_DIR/../../examples");

/// 内置 example 名列表(供 dry-run 校验 / 错误提示)。
pub fn embedded_example_names() -> Vec<String> {
    EXAMPLES
        .dirs()
        .filter_map(|d| d.path().file_name().and_then(|n| n.to_str()).map(String::from))
        .collect()
}

/// 把内置 example 释放到 `target_dir/<name>`。原子语义:先写到目标同级的
/// staging 目录,成功后 `rename` 到目标;任何一步失败都清掉 staging,目标保持
/// 不存在(无半成品残留)。**拒绝覆盖已存在目标**——避免与既有文件混合成
/// 损坏的 example;要重新 seed 须自己先删目标。
/// transport-free:CLI 与未来 MCP server 共用这一份。
pub fn seed_embedded_example(name: &str, target_dir: &Path) -> LmtResult<PathBuf> {
    let src = EXAMPLES.get_dir(name).ok_or_else(|| {
        LmtError::NotFound(format!(
            "example '{name}' not found; available: {:?}",
            embedded_example_names()
        ))
    })?;
    let dst = target_dir.join(name);
    if dst.exists() {
        return Err(LmtError::InvalidInput(format!(
            "destination already exists: {} (remove it first to re-seed)",
            dst.display()
        )));
    }
    std::fs::create_dir_all(target_dir)?;
    // staging 放在目标同一父目录下,保证 rename 不跨文件系统(/tmp 会 EXDEV)。
    let staging = target_dir.join(format!(".{name}.seed.{}.tmp", std::process::id()));
    let _ = std::fs::remove_dir_all(&staging);
    let staged = (|| -> LmtResult<()> {
        std::fs::create_dir_all(&staging)?;
        write_embedded_dir_contents(src, &staging)
    })();
    match staged {
        Ok(()) => {
            std::fs::rename(&staging, &dst)
                .map_err(|e| LmtError::Io(format!("finalize seed rename: {e}")))?;
            Ok(dst)
        }
        Err(e) => {
            let _ = std::fs::remove_dir_all(&staging);
            Err(e)
        }
    }
}

/// 把 include_dir 的某个 Dir 的内容(剥掉自身名字前缀)递归写到 `out_dir`。
fn write_embedded_dir_contents(dir: &Dir, out_dir: &Path) -> LmtResult<()> {
    for f in dir.files() {
        let name = f.path().file_name().expect("embedded file has a name");
        std::fs::write(out_dir.join(name), f.contents())?;
    }
    for sub in dir.dirs() {
        let name = sub.path().file_name().expect("embedded dir has a name");
        let sub_out = out_dir.join(name);
        std::fs::create_dir_all(&sub_out)?;
        write_embedded_dir_contents(sub, &sub_out)?;
    }
    Ok(())
}
```

`cli.rs` Command enum 加:

```rust
    /// 把内置 example(curved-flat / curved-arc)拷贝到目标目录。
    /// side_effect: destructive(写文件,需 --yes 或 --dry-run)。
    #[command(name = "seed-example")]
    SeedExample {
        /// example 名:curved-flat / curved-arc。
        name: String,
        /// 目标父目录;会在其下创建 <name>/ 子目录。
        dst: std::path::PathBuf,
    },
```

Create `crates/lmt-cli/src/commands/seed.rs`:

```rust
//! `lmt seed-example <name> <dst>` —— 释放内置 example 到目标目录。
//!
//! 业务逻辑(嵌入资源 + 释放)在 `lmt_app::projects`;本文件只做 transport:
//! destructive 守门 + dry-run 预览 + envelope 输出。未来 MCP server 调用
//! 同一个 `lmt_app::projects::seed_embedded_example`,无需重复嵌入。
//! side_effect: destructive。

use crate::commands::util::{self, DestructiveDecision};
use crate::output::{self, Mode};
use lmt_shared::envelope::{error_codes, ApiError};
use std::io::Write;
use std::path::Path;

pub fn run(mode: Mode, name: &str, dst: &Path, yes: bool, dry_run: bool) -> i32 {
    let decision = match util::gate_destructive(yes, dry_run, "seed-example") {
        Ok(d) => d,
        Err(e) => return output::err(mode, e),
    };
    match decision {
        DestructiveDecision::DryRun => {
            // dry-run 是真正的 preflight:未知 name 与已存在目标都要在这里失败,
            // 不能报 ok 让 agent 以为安全、然后 --yes 才炸(破坏 dry-run 契约)。
            let names = lmt_app::projects::embedded_example_names();
            if !names.iter().any(|n| n == name) {
                return output::err(
                    mode,
                    ApiError::new(
                        error_codes::NOT_FOUND,
                        format!("example '{name}' not found; available: {names:?}"),
                    ),
                );
            }
            let target = dst.join(name);
            if target.exists() {
                return output::err(
                    mode,
                    ApiError::new(
                        error_codes::INVALID_INPUT,
                        format!(
                            "destination already exists: {} (remove it first to re-seed)",
                            target.display()
                        ),
                    ),
                );
            }
            let payload = serde_json::json!({
                "dry_run": true,
                "would_seed": name,
                "would_write_under": target.display().to_string(),
            });
            output::ok(mode, payload, |_| {
                let _ = writeln!(
                    std::io::stdout(),
                    "[dry-run] would seed example '{name}' into {}",
                    target.display()
                );
            })
        }
        DestructiveDecision::Execute => {
            match lmt_app::projects::seed_embedded_example(name, dst) {
                Ok(seeded) => output::ok(
                    mode,
                    serde_json::json!({"seeded": name, "path": seeded.display().to_string()}),
                    |_| {
                        let _ = writeln!(
                            std::io::stdout(),
                            "seeded example '{name}' into {}",
                            seeded.display()
                        );
                    },
                ),
                Err(e) => output::err(mode, ApiError::from(e)),
            }
        }
    }
}
```

`commands/mod.rs`:加 `mod seed;`;dispatch 加:

```rust
        Command::SeedExample { name, dst } => seed::run(mode, &name, &dst, yes, dry_run),
```

- [ ] **Step 4: 跑测试确认通过**

Run: `cargo test -p lmt-cli seed_example 2>&1 | tail -20`
Expected: 5 个 seed-example 用例 passed(dry-run / happy / unknown / dry-run-unknown / existing-dest)。

- [ ] **Step 5: 提交**

```bash
git add crates/lmt-app/Cargo.toml crates/lmt-app/src/projects.rs \
        crates/lmt-cli/src/commands/seed.rs crates/lmt-cli/src/cli.rs \
        crates/lmt-cli/src/commands/mod.rs crates/lmt-cli/tests/cli_e2e.rs
git commit -m "feat(app): embed examples + seed_embedded_example; expose via 'lmt seed-example'

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>"
```

### Task 4.2: 把 seed-example 加进 manifest + 刷新快照 + docs

**Files:**
- Modify: `crates/lmt-shared/src/manifest.rs`(`build()` operations)、`docs/contract-manifest.json`、`docs/agents-cli.md`

- [ ] **Step 1: 扩 manifest 测试**

在 `manifest.rs` 的 `manifest_lists_all_known_operations_with_stable_ids` 测试的期望 id 数组里加 `"seed_example"`,并把 count 断言从 14 改为 15:

```rust
            "export.obj",
            "seed_example",
```
（数组末尾追加;同时把 `assert_eq!(m.operations.len(), 14, ...)` 改成 `15`。）

- [ ] **Step 2: 跑测试确认失败**

Run: `cargo test -p lmt-shared manifest::tests::manifest_lists 2>&1 | tail -15`
Expected: FAIL —— `manifest missing operation_id seed_example`。

- [ ] **Step 3: 加 operation**

在 `manifest.rs` `build()` 的 operations vec 里 `export.obj` 那条之后加:

```rust
        op("seed_example", "Copy a built-in example project (curved-flat / curved-arc) into a directory",
           "lmt seed-example <name> <dst>", Destructive, true, false, false, None, &[0, 2, 3, 4]),
```

- [ ] **Step 4: 跑测试 + 刷新快照 + 更新 docs**

Run:
```bash
cargo test -p lmt-shared manifest:: 2>&1 | tail -10
cargo build -p lmt-cli && ./target/debug/lmt --json manifest | jq .data > docs/contract-manifest.json
```
Expected: manifest 测试绿;快照含 15 个 operation。

在 `docs/agents-cli.md` 的 `### Not exposed in CLI` 段落里,把 `seed-example` 那条**移除**(它现在已暴露),并在 Command tree 表 `export obj` 行后加:

```markdown
| `lmt seed-example <name> <dst>` | destructive | Copy a built-in example (curved-flat / curved-arc) into `<dst>/<name>` |
```

- [ ] **Step 5: 全量回归 + 提交**

Run: `cargo test --workspace 2>&1 | tail -15`
Expected: 全绿。

```bash
git add crates/lmt-shared/src/manifest.rs docs/contract-manifest.json docs/agents-cli.md
git commit -m "feat(shared): register seed_example in manifest + refresh snapshot/docs

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>"
```

---

## 完成标准(全 Phase 跑完后验证)

Run:
```bash
cargo test --workspace 2>&1 | tail -5
./target/debug/lmt --help          # manifest / version / completion / seed-example 都在列
./target/debug/lmt --json manifest | jq '.data.operations | length'   # = 14
./target/debug/lmt --output ndjson schema | jq -c 'select(.type=="result")'  # 一条 result 事件
./target/debug/lmt --json version | jq .data
./target/debug/lmt completion zsh | head -1
```
Expected:全测试绿;manifest 列 14 个 operation;ndjson 出单 result 事件;version 出 JSON;completion 出脚本。

---

## 面向 MCP / Skill 的前瞻设计

本 plan 只交付 CLI,但每个决策都按"将来零返工接上 MCP server 与 Skill"来定。

### 已经为 MCP/Skill 铺好的(本 plan 内完成)

| 决策 | 为什么对 MCP/Skill 有用 |
|---|---|
| `manifest::build()` 放 `lmt-shared` | MCP server 调它生成 `tools/list`,Skill 引用 `docs/contract-manifest.json`。单一契约源,不与 CLI 漂移(spec §2.2 / §7.2 / §11.2) |
| `Operation` 带 `operation_id` | MCP tool 的 `_meta.operation_id`、Skill 的操作引用都用它(spec §7.2)。运行时 envelope 不带也行——MCP 层知道自己调哪个 operation,可自注入 |
| `Operation` 带 `side_effect` + `idempotent` + `open_world` | MCP 四个 annotation 全可派生:`readOnlyHint = side_effect==ReadOnly`、`destructiveHint = ==Destructive`、`idempotentHint = idempotent`、`openWorldHint = open_world`(spec §7.5)。`idempotent` 用严格定义(重复调用对可观测状态无额外改变),只有 read-only + delete 为 true——防止 MCP 据假元数据自动重试。现在填好,MCP plan 零改契约 |
| seed-example 嵌入 + 业务在 `lmt-app` | 未来 `lmt-mcp` 直接调 `lmt_app::projects::seed_embedded_example`,examples 只嵌一份。放 cli 会逼 MCP 反向依赖 cli |
| 新命令业务全在 `lmt-app`/`lmt-shared`,cli 只翻译 | spec §0.2"业务只写一次"。MCP server 与 CLI 平级,复用同一服务层 |
| envelope `{ok,data,meta}` | 可整体塞进 MCP `structuredContent`(spec §7.3),无需重设计响应体 |

### 有意推迟、且不堵 MCP/Skill 的

| 推迟项 | 为什么现在不做 | 将来加 MCP 时怎么补 |
|---|---|---|
| envelope 运行时 `operation_id` / `status` | 纯 CLI 用不到;改了是 breaking + 动 ~20 个测试 | MCP 层调 operation 时从 manifest 注入 operation_id;或届时统一 bump envelope `schema_version` 到 2 一次性补 |
| exit code 重映射到 spec §5 | 单 app Skill 读自己的 `error.code` 表即可,不跨工具 | MCP §7.4 靠 `retryable` 语义而非 exit code;真要跨生态一致再做独立 plan |
| `reconstruct surface` 进度 / cancellation | 算法层改造(独立 L) | 把进度回调 + cancellation token 设计成 transport-agnostic 放 `lmt-app`:CLI 的 `--output ndjson`(本 plan 已建单事件框架)消费它发 `progress`,MCP 消费它发协议级 progress notification。**两个 transport 共用一个算法层抽象** |

### 将来 MCP / Skill plan 的接入点(本 plan 不实现,仅登记)

1. **新建 `crates/lmt-mcp`**:协议壳 → `tools/list` 从 `lmt_shared::manifest::build()` 派生、annotations 从 `side_effect`/`idempotent`/`open_world` 派生 → `tools/call` 路由到与 CLI 相同的 `lmt-app` helper → 响应包成 `structuredContent` + 现有 envelope。
2. **Skill**:`.claude/skills/lmt/SKILL.md`,transport `mcp-first` + cli fallback(`lmt <cmd> --output json --no-color --no-input`,本 plan flag 已就绪),reference 引 `docs/contract-manifest.json`,error policy 按 `error.code` + exit code(manifest `exit_codes` 已登记)。
3. **维护契约**:新增 CLI 命令必须同步往 `manifest::build()` 加一条 + 刷新 `docs/contract-manifest.json`——做到位则 MCP tool 列表自动跟上,不出现"CLI 有、MCP 没有"的 gap。

---

## Self-Review Notes

- **Spec coverage(对照审计报告 CLI 部分):** Contract Manifest(P1-1)→ Phase 1;`version --output json`(P1-2)→ Phase 3;`--output` 三档 + `--no-color`/`--no-input`(P0-1 的 CLI 友好子集)→ Phase 2;`completion`(P2-2)→ Phase 3;功能完整性(seed-example)→ Phase 4;`docs/contract-manifest.json`(P2-1 部分)→ Phase 1/4。**有意不覆盖**(已在头部范围声明):envelope 运行时 `operation_id`/`status`、exit code 重映射、ndjson 长任务多事件、cancellation、`--config`/`--log-level`、MCP、Skill。
- **Placeholder scan:** 每个改代码的 Step 都含完整代码块;`build()` 的 `todo!()` 仅作 Step 1 跑红用,Step 3 给出完整替换体,非遗留占位符。
- **Type consistency:** `Mode`(`Human`/`Json`/`Ndjson`)、`OutputFormat`(`Text`/`Json`/`Ndjson`)、`Mode::from_format`、`Cli::resolved_format`、`SideEffect`、`Operation`(含 `idempotent`/`open_world`)、`ContractManifest`、`result_event`、`seed_embedded_example` / `embedded_example_names` / `write_embedded_dir_contents`、`wants_machine_output` 在各引用处命名一致(现有 `seed_example_to_dir` 保留给 GUI,本 plan 不动它)。`output_type` 用 `Option<String>`,引用 schema dump 里已存在的类型名(`RecentProject`/`ProjectConfig`/`TotalStationImportResult`/`InstructionCardResult`/`ReconstructionRun`)。
- **MCP/Skill 前瞻(本轮用户新增要求):** 嵌入与 seed 业务下沉到 `lmt-app`(非 cli),`Operation` 现在就带 MCP 四个 annotation 的数据源(`side_effect`/`idempotent`/`open_world`),manifest 在 `lmt-shared` 作单一契约源——确保将来 `lmt-mcp` + Skill 是"接上去"而非"推倒重来"。推迟项(envelope 字段、exit code、长任务进度)均在 §前瞻设计 登记了补法且不堵路。
- **已采纳的 Codex adversarial review(4 条):** (1) seed-example 改原子语义 + 拒绝已存在目标(staging→rename→失败清理),Finding [high];(2) dry-run 对未知 name / 已存在目标 fail-fast,不再误报 ok,Finding [high];(3) `idempotent` 重定义为严格语义,add_recent/save/import/export/surface 全标 false,加测试锁住,Finding [high];(4) machine-output 检测抽 helper 兼容 `--output=json` 单 token 写法,加 E2E,Finding [medium]。未纳入:Codex 的 [critical] `.claude/worktrees` 污染是 working-tree 卫生问题、非 plan 内容,另行处理。
