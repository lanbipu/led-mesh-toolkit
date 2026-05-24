//! `lmt total-station ...` 子命令。
//!
//! 不暴露 `save-pdf`——它依赖 macOS WKWebView / Windows WebView2 原生
//! webview,跟 headless CLI 不兼容(细节见 docs/architecture-audit 与
//! AGENTS.md)。Agent 想要 PDF 时拿 `instruction-card` 的 HTML 自己
//! 走 headless Chrome 之类的外部工具。

use crate::cli::{ImportMode, TotalStationCmd};
use crate::commands::util::{self, DestructiveDecision};
use crate::output::{self, Mode};
use lmt_adapter_total_station::scatter_csv::ColumnMap;
use lmt_shared::envelope::{error_codes, ApiError};
use std::io::Write as _;
use std::path::Path;

pub fn run(cmd: TotalStationCmd, mode: Mode, yes: bool, dry_run: bool) -> i32 {
    match cmd {
        TotalStationCmd::Import {
            project_abs_path,
            screen_id,
            csv_path,
            mode: import_mode,
            columns,
        } => import(
            mode,
            &project_abs_path,
            &screen_id,
            &csv_path,
            import_mode,
            columns.as_deref(),
            yes,
            dry_run,
        ),
        TotalStationCmd::InstructionCard {
            project_abs_path,
            screen_id,
        } => instruction_card(mode, &project_abs_path, &screen_id),
    }
}

/// 解析 `x=3,y=4,z=5[,label=1]` 格式的 columns 字符串（1-based 列号）。
/// x/y/z 必填，label 可选，未知 key 或非数字值均报错。
fn parse_columns(s: &str) -> Result<ColumnMap, String> {
    let mut x: Option<usize> = None;
    let mut y: Option<usize> = None;
    let mut z: Option<usize> = None;
    let mut label: Option<usize> = None;

    for segment in s.split(',') {
        let segment = segment.trim();
        if segment.is_empty() {
            continue;
        }
        let mut parts = segment.splitn(2, '=');
        let key = parts.next().unwrap_or("").trim();
        let val_str = parts.next().ok_or_else(|| format!("expected k=v, got '{segment}'"))?;
        let val: usize = val_str
            .trim()
            .parse()
            .map_err(|_| format!("column '{key}' value '{val_str}' is not a positive integer"))?;
        if val == 0 {
            return Err(format!("column '{key}' must be 1-based (got 0)"));
        }
        match key {
            "x" => x = Some(val),
            "y" => y = Some(val),
            "z" => z = Some(val),
            "label" => label = Some(val),
            other => return Err(format!("unknown column key '{other}' (allowed: x, y, z, label)")),
        }
    }

    let x = x.ok_or("columns: 'x' is required")?;
    let y = y.ok_or("columns: 'y' is required")?;
    let z = z.ok_or("columns: 'z' is required")?;

    Ok(ColumnMap { x, y, z, label })
}

#[allow(clippy::too_many_arguments)]
fn import(
    mode: Mode,
    project_abs_path: &str,
    screen_id: &str,
    csv_path: &str,
    import_mode: ImportMode,
    columns: Option<&str>,
    yes: bool,
    dry_run: bool,
) -> i32 {
    let decision = match util::gate_destructive(yes, dry_run, "total-station import") {
        Ok(d) => d,
        Err(e) => return output::err(mode, e),
    };

    match decision {
        DestructiveDecision::Execute => match import_mode {
            ImportMode::Grid => {
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
            ImportMode::Scatter => {
                // 解析 columns（None = 自动推断）
                let col_map = match columns {
                    Some(s) => match parse_columns(s) {
                        Ok(c) => Some(c),
                        Err(e) => {
                            return output::err(
                                mode,
                                ApiError::new(error_codes::INVALID_INPUT, e),
                            )
                        }
                    },
                    None => None,
                };
                match lmt_app::total_station::run_import_scatter(
                    Path::new(project_abs_path),
                    screen_id,
                    Path::new(csv_path),
                    col_map,
                ) {
                    Ok(r) => output::ok(mode, r, |s| {
                        let _ = writeln!(
                            std::io::stdout(),
                            "scatter import: measured={} fabricated={}\n  measured_yaml: {}\n  report:       {}",
                            s.measured_count,
                            s.fabricated_count,
                            s.measurements_yaml_path,
                            s.report_json_path
                        );
                    }),
                    Err(e) => output::err(mode, ApiError::from(e)),
                }
            }
        },
        DestructiveDecision::DryRun => {
            // scatter 模式额外提前解析 columns（格式错 → invalid_input）
            if let ImportMode::Scatter = import_mode {
                if let Some(s) = columns {
                    if let Err(e) = parse_columns(s) {
                        return output::err(
                            mode,
                            ApiError::new(error_codes::INVALID_INPUT, e),
                        );
                    }
                }
            }

            // 共用：project.yaml + screen + csv 存在性校验
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
            // grid 模式才做 cross-screen guard（scatter 不需要 SOP 校验）
            if let ImportMode::Grid = import_mode {
                if let Err(e) = lmt_app::total_station::check_import_no_screen_conflict(
                    project_path,
                    screen_id,
                ) {
                    return output::err(mode, ApiError::from(e));
                }
            }
            let payload = serde_json::json!({
                "dry_run": true,
                "mode": match import_mode { ImportMode::Grid => "grid", ImportMode::Scatter => "scatter" },
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
