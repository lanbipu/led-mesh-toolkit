//! Clap-derived CLI surface.
//!
//! 子命令一对一映射 lmt-app 的 use case,方便日后被 MCP wrapper 平移成 tool。

use clap::{Parser, Subcommand};
use std::path::PathBuf;

#[derive(Debug, Parser)]
#[command(
    name = "lmt",
    version,
    about = "LED Mesh Toolkit CLI",
    long_about = "Agent-friendly CLI. Use --json for machine-stable envelope output."
)]
pub struct Cli {
    /// 切到稳定 JSON envelope 输出(stdout)。默认是 human-readable。
    #[arg(long, global = true)]
    pub json: bool,

    /// 显式 DB 路径。优先级:--db > LMT_DB_PATH env > OS 标准位置
    /// (即 Tauri GUI 用的 lmt.sqlite,默认共用)。
    ///
    /// 测试 / CI / 隔离运行务必显式指定,避免污染默认 DB。
    #[arg(long, global = true, env = "LMT_DB_PATH")]
    pub db: Option<PathBuf>,

    /// 破坏性操作的 dry-run 预演。具体命令的语义见各自 --help。
    #[arg(long, global = true)]
    pub dry_run: bool,

    /// 破坏性操作的确认开关。当 --dry-run 不在,某些命令仍要求 --yes
    /// 显式确认。
    #[arg(long, global = true)]
    pub yes: bool,

    /// 单条命令的总超时(秒)。
    ///
    /// **v0 暂未实现**——传任何值都会立刻被 dispatch 拒绝并报 `unsupported`,
    /// 而不是默默忽略让 agent 误以为有上限。flag 保留是为了未来加上时不需要
    /// 改 CLI surface。Native PDF render 在 src-tauri 内有 30s 内置超时,但
    /// 本 CLI 不暴露 PDF。
    #[arg(long, global = true, value_name = "SECS")]
    pub timeout: Option<u64>,

    #[command(subcommand)]
    pub command: Command,
}

#[derive(Debug, Subcommand)]
pub enum Command {
    /// 项目元数据 / project.yaml / recent_projects 管理。
    #[command(subcommand)]
    Project(ProjectCmd),

    /// measured.yaml 读取。
    #[command(subcommand)]
    Measurements(MeasurementsCmd),

    /// M1 全站仪 CSV adapter:导入 + 指引卡 HTML 输出。不暴露 PDF。
    #[command(name = "total-station", subcommand)]
    TotalStation(TotalStationCmd),

    /// 几何重建 + run 历史查询。
    #[command(subcommand)]
    Reconstruct(ReconstructCmd),

    /// run 导出为 OBJ。
    #[command(subcommand)]
    Export(ExportCmd),

    /// dump lmt-shared 全部公开 DTO / error / envelope 的 JSON Schema。
    Schema,
}

#[derive(Debug, Subcommand)]
pub enum ProjectCmd {
    /// 列出 recent_projects 表内全部条目(按 last_opened_at desc)。
    /// side_effect: read_only
    ListRecent,

    /// upsert 一条 recent_projects 记录,返回完整行。
    /// side_effect: write_safe
    AddRecent {
        /// 项目绝对路径,作为 conflict key。
        abs_path: String,
        /// 显示用名字。
        display_name: String,
    },

    /// 删除 recent_projects 内 id == ID 的行。不存在则 no-op。
    /// side_effect: destructive(需要 --yes 或 --dry-run)
    RemoveRecent {
        /// recent_projects 表的主键。
        id: i64,
    },

    /// 读取 `<dir>/project.yaml`,输出 ProjectConfig。
    /// side_effect: read_only
    Load {
        /// 项目根目录(包含 project.yaml 的目录)。
        abs_path: String,
    },

    /// 把 ProjectConfig(从 stdin / --input 文件读 YAML 或 JSON)atomic
    /// 写到 `<dir>/project.yaml`。
    /// side_effect: destructive(需要 --yes 或 --dry-run)
    Save {
        /// 项目根目录,会被创建出来。
        abs_path: String,
        /// ProjectConfig YAML/JSON 文件路径;省略走 stdin。
        #[arg(long, value_name = "PATH")]
        input: Option<PathBuf>,
    },
}

#[derive(Debug, Subcommand)]
pub enum MeasurementsCmd {
    /// 读 measured.yaml,输出 MeasuredPoints。
    /// side_effect: read_only
    Load {
        /// measured.yaml 绝对路径。
        path: String,
    },
}

/// 采样模式：网格命名（grid，默认）或曲面拟合（scatter）。
#[derive(Debug, Clone, Copy, clap::ValueEnum)]
pub enum ImportMode {
    /// 标准网格 CSV（SOP 校验 + 网格命名）。
    Grid,
    /// 散点 CSV（跳过 SOP，直接存原始坐标，reconstruct 走曲面拟合）。
    Scatter,
}

#[derive(Debug, Subcommand)]
pub enum TotalStationCmd {
    /// 把 Trimble CSV 导入 `<project>/measurements/measured.yaml`(+ import_report.json)。
    /// 已有 measured.yaml 会被 rename 成 .bak。失败时回滚。
    /// side_effect: destructive(需要 --yes 或 --dry-run)
    Import {
        /// 项目根目录。
        project_abs_path: String,
        /// 要导入的 screen id。
        screen_id: String,
        /// Trimble CSV 绝对路径。
        csv_path: String,
        /// 采样模式：grid（默认，网格命名）或 scatter（曲面拟合）。
        #[arg(long, value_enum, default_value_t = ImportMode::Grid)]
        mode: ImportMode,
        /// scatter 模式列映射，1-based，形如 `x=3,y=4,z=5[,label=1]`。
        /// 省略则自动推断末尾 3 数值列。
        #[arg(long)]
        columns: Option<String>,
    },

    /// 渲染指引卡 HTML(给 iframe 预览或外部 PDF 工具)。不输出 PDF。
    /// side_effect: read_only
    InstructionCard {
        /// 项目根目录。
        project_abs_path: String,
        /// screen id。
        screen_id: String,
    },
}

#[derive(Debug, Subcommand)]
pub enum ReconstructCmd {
    /// 重建表面,写 report.json + 记录 reconstruction_runs DB 行。
    /// side_effect: destructive(写文件 + DB 行,需要 --yes 或 --dry-run)
    Surface {
        /// 项目根目录。
        project_path: String,
        /// screen id。
        screen_id: String,
        /// measured.yaml 相对 project 的路径,通常是
        /// `measurements/measured.yaml`。
        measurements_path: String,
    },

    /// 列出某项目 / screen 的全部 reconstruction_runs。
    /// side_effect: read_only
    ListRuns {
        /// 项目根目录。
        project_path: String,
        /// 仅列某个 screen,不传则列全部。
        #[arg(long)]
        screen_id: Option<String>,
    },

    /// 读取某条 run 的完整 report.json(原始 JSON,不重新序列化)。
    /// side_effect: read_only
    GetRunReport {
        /// reconstruction_runs.id。
        run_id: i64,
    },
}

#[derive(Debug, Subcommand)]
pub enum ExportCmd {
    /// 把某条 run 导出为 OBJ。
    /// side_effect: destructive(写文件,需要 --yes 或 --dry-run)
    Obj {
        /// reconstruction_runs.id。
        run_id: i64,
        /// target software: disguise / unreal / neutral。
        target: String,
        /// 目标 OBJ 绝对路径;省略走默认 `<project>/output/<screen>_<target>_run<id>.obj`。
        #[arg(long, value_name = "PATH")]
        dst: Option<PathBuf>,
    },
}
