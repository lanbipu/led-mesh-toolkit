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
    // op(id, summary, cli, side_effect, dry_run, stdin, idempotent, output_type, exit_codes)
    let operations = vec![
        op("schema", "Dump JsonSchema of all public DTOs + envelope + error types",
           "lmt schema", ReadOnly, false, false, true, None, &[0]),
        op("manifest", "Dump the Contract Manifest (this operation list)",
           "lmt manifest", ReadOnly, false, false, true, Some("ContractManifest"), &[0]),
        op("version", "Machine-readable version metadata (semver + schema/contract versions)",
           "lmt version", ReadOnly, false, false, true, None, &[0]),
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
           "lmt total-station import <project> <screen_id> <csv> [--mode grid|scatter] [--columns <spec>]", Destructive, true, false, false, Some("TotalStationImportResult"), &[0, 2, 3, 4, 6]),
        op("total_station.instruction_card", "Render instruction-card HTML on stdout (no PDF)",
           "lmt total-station instruction-card <project> <screen_id>", ReadOnly, false, false, true, Some("InstructionCardResult"), &[0, 2, 3, 4, 6]),
        op("reconstruct.surface", "Run reconstruction, write report.json + reconstruction_runs row",
           "lmt reconstruct surface <project> <screen_id> <measurements_rel>", Destructive, true, false, false, None, &[0, 2, 3, 4, 5, 6, 12]),
        op("reconstruct.list_runs", "List reconstruction_runs for a project",
           "lmt reconstruct list-runs <project> [--screen-id <id>]", ReadOnly, false, false, true, Some("ReconstructionRun"), &[0, 2, 5]),
        op("reconstruct.get_run_report", "Return the full report.json for a run",
           "lmt reconstruct get-run-report <run_id>", ReadOnly, false, false, true, None, &[0, 2, 3, 4, 5, 6]),
        op("export.obj", "Write an OBJ for a run (target: disguise|unreal|neutral)",
           "lmt export obj <run_id> <target> [--dst <path>]", Destructive, true, false, false, None, &[0, 2, 3, 4, 5, 6]),
        op("seed_example", "Copy a built-in example project (curved-flat / curved-arc) into a directory",
           "lmt seed-example <name> <dst>", Destructive, true, false, false, None, &[0, 2, 3, 4]),
        op("visual.calibrate", "Checkerboard images -> camera intrinsics.json",
           "lmt visual calibrate <project> <screen_id> <checkerboard_dir> [--square-mm <f>] [--inner <RxC>]", Destructive, true, false, false, Some("CalibrateResult"), &[0, 2, 3, 4, 13, 16]),
        op("visual.generate_pattern", "Generate ChArUco pattern (per-cabinet PNGs + full_screen + pattern_meta)",
           "lmt visual generate-pattern <project> <screen_id> [--method charuco]", Destructive, true, false, false, Some("GeneratePatternResult"), &[0, 2, 3, 4, 6, 7]),
        op("visual.reconstruct", "Multi-view photos -> measured.yaml + cabinet_pose_report.json (model-constrained BA, zero total station)",
           "lmt visual reconstruct <project> <screen_id> --capture-manifest <json> [--method charuco]", Destructive, true, false, false, Some("VisualReconstructResult"), &[0, 2, 3, 4, 6, 7, 13, 14, 15, 16, 17]),
        op("visual.simulate", "Generate a synthetic geometry dataset (scene.npz) for BA validation",
           "lmt visual simulate <config> --out <dir>", Destructive, true, false, false, Some("SimulateResult"), &[0, 2, 4, 6]),
        op("visual.eval", "Evaluate a method vs ground truth on a synthetic dataset (gauge-invariant metrics)",
           "lmt visual eval <dataset> [--method charuco] [--seed-matrix <list>]", WriteSafe, false, false, true, Some("EvalResult"), &[0, 2, 4]),
    ];

    ContractManifest {
        contract_version: "1.0".into(),
        schema_version: crate::envelope::SCHEMA_VERSION.into(),
        operations,
    }
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
            "version",
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
            "seed_example",
            "visual.calibrate",
            "visual.generate_pattern",
            "visual.reconstruct",
            "visual.simulate",
            "visual.eval",
        ] {
            assert!(ids.contains(&expected), "manifest missing operation_id {expected}; got {ids:?}");
        }
        assert_eq!(m.operations.len(), 21, "operation count changed — update both build() and this test");
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
            "seed_example",
            "visual.calibrate",
            "visual.generate_pattern",
            "visual.reconstruct",
            "visual.simulate",
        ] {
            assert!(!find(id).idempotent, "{id} mutates observable state -> not idempotent");
        }
        assert!(m.operations.iter().all(|o| !o.open_world), "no operation calls external APIs yet");
    }
}
