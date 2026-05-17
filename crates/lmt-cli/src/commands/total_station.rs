//! `lmt total-station ...` 子命令。
//!
//! 不暴露 `save-pdf`——它依赖 macOS WKWebView / Windows WebView2 原生
//! webview,跟 headless CLI 不兼容(细节见 docs/architecture-audit 与
//! AGENTS.md)。Agent 想要 PDF 时拿 `instruction-card` 的 HTML 自己
//! 走 headless Chrome 之类的外部工具。

use crate::cli::TotalStationCmd;
use crate::commands::util::{self, DestructiveDecision};
use crate::output::{self, Mode};
use lmt_shared::envelope::{error_codes, ApiError};
use std::io::Write as _;
use std::path::Path;

pub fn run(cmd: TotalStationCmd, mode: Mode, yes: bool, dry_run: bool) -> i32 {
    match cmd {
        TotalStationCmd::Import {
            project_abs_path,
            screen_id,
            csv_path,
        } => import(mode, &project_abs_path, &screen_id, &csv_path, yes, dry_run),
        TotalStationCmd::InstructionCard {
            project_abs_path,
            screen_id,
        } => instruction_card(mode, &project_abs_path, &screen_id),
    }
}

fn import(
    mode: Mode,
    project_abs_path: &str,
    screen_id: &str,
    csv_path: &str,
    yes: bool,
    dry_run: bool,
) -> i32 {
    let decision = match util::gate_destructive(yes, dry_run, "total-station import") {
        Ok(d) => d,
        Err(e) => return output::err(mode, e),
    };

    match decision {
        DestructiveDecision::Execute => {
            match lmt_app::total_station::run_import(
                Path::new(project_abs_path),
                screen_id,
                Path::new(csv_path),
            ) {
                Ok(r) => output::ok(mode, r, |s| {
                    let _ = writeln!(
                        std::io::stdout(),
                        "imported: measured={} fabricated={} outliers={} missing={}\n  measured_yaml: {}\n  report:       {}",
                        s.measured_count,
                        s.fabricated_count,
                        s.outlier_count,
                        s.missing_count,
                        s.measurements_yaml_path,
                        s.report_json_path
                    );
                    if !s.warnings.is_empty() {
                        let _ = writeln!(std::io::stderr(), "warnings:");
                        for w in &s.warnings {
                            let _ = writeln!(std::io::stderr(), "  - {w}");
                        }
                    }
                }),
                Err(e) => output::err(mode, ApiError::from(e)),
            }
        }
        DestructiveDecision::DryRun => {
            // 轻量预演:校验 project.yaml + screen 存在 + csv 存在 + 既有
            // measured.yaml 的 cross-screen guard(跟 execute 共用 helper,
            // 防止 dry-run greenlight 一个 --yes 必定失败的导入)。
            let project_path = Path::new(project_abs_path);
            let cfg = match lmt_app::projects::load_project_yaml_from_path(project_path) {
                Ok(c) => c,
                Err(e) => return output::err(mode, ApiError::from(e)),
            };
            if !cfg.screens.contains_key(screen_id) {
                return output::err(
                    mode,
                    ApiError::new(
                        error_codes::NOT_FOUND,
                        format!("screen '{screen_id}' not in project"),
                    ),
                );
            }
            if !Path::new(csv_path).is_file() {
                return output::err(
                    mode,
                    ApiError::new(
                        error_codes::NOT_FOUND,
                        format!("csv not found: {csv_path}"),
                    ),
                );
            }
            if let Err(e) = lmt_app::total_station::check_import_no_screen_conflict(
                project_path,
                screen_id,
            ) {
                return output::err(mode, ApiError::from(e));
            }
            let payload = serde_json::json!({
                "dry_run": true,
                "would_write": [
                    format!("{}/measurements/measured.yaml", project_abs_path),
                    format!("{}/measurements/import_report.json", project_abs_path),
                ],
                "screen_id": screen_id,
                "csv_path": csv_path,
            });
            output::ok(mode, payload, |_| {
                let _ = writeln!(
                    std::io::stdout(),
                    "[dry-run] would write measured.yaml + import_report.json for screen {screen_id}"
                );
            })
        }
    }
}

fn instruction_card(mode: Mode, project_abs_path: &str, screen_id: &str) -> i32 {
    match lmt_app::total_station::run_generate_card(Path::new(project_abs_path), screen_id) {
        Ok(r) => output::ok(mode, r, |c| {
            // human 模式 stdout 出 HTML;agent 也可以用 `lmt total-station
            // instruction-card ... > card.html` 直接拿 HTML 字节。
            let _ = std::io::stdout().write_all(c.html_content.as_bytes());
            // 收尾换行,人眼读 stdout 时不会跟下个 prompt 粘在一起。
            let _ = writeln!(std::io::stdout());
        }),
        Err(e) => output::err(mode, ApiError::from(e)),
    }
}
