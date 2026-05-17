//! 输出 + 退出码 helper。
//!
//! 关键约定(对齐 `cli_spec` 的 JSON 约定):
//! - 成功 → stdout 单条 JSON envelope(`--json` 模式)/ 人类摘要(human 模式)。
//! - 失败 → stderr 单条 `ErrorEnvelope`(`--json` 模式)/ `error: <code> — <message>`
//!   (human 模式)。stdout 在失败时一律为空,便于 agent / pipeline 用
//!   `lmt foo > out.json` 时通过 "stdout 空 + 非零 exit code" 直接判定失败。
//! - 退出码由 [`lmt_shared::exit_codes::from_api_error_code`] 派生,
//!   成功一律 0。

use lmt_shared::envelope::{ApiError, Envelope, ErrorEnvelope};
use lmt_shared::exit_codes;
use serde::Serialize;
use std::io::Write;

/// 输出模式;由 `--json` flag 触发。
#[derive(Debug, Clone, Copy)]
pub enum Mode {
    Human,
    Json,
}

impl Mode {
    pub fn from_flag(json: bool) -> Self {
        if json {
            Mode::Json
        } else {
            Mode::Human
        }
    }
}

/// 成功输出。
///
/// - JSON:序列化 `Envelope<T>` 到 stdout(单行紧凑 JSON,便于 pipeline 解析)。
/// - human:调用方提供的 `summary` 闭包负责写 stdout,可以多行。
///
/// 返回值是退出码,统一 0。把 `summary` 失败也吞掉——CLI 写 stdout 出错通常
/// 意味着 broken pipe,这种场景没必要再上报 envelope。
pub fn ok<T: Serialize>(mode: Mode, data: T, summary: impl FnOnce(&T)) -> i32 {
    match mode {
        Mode::Json => {
            let env = Envelope::ok(data);
            let s = serde_json::to_string(&env).expect("Envelope is always serializable");
            let _ = writeln!(std::io::stdout(), "{s}");
        }
        Mode::Human => {
            summary(&data);
        }
    }
    exit_codes::OK
}

/// 失败输出。stdout 保持空白;envelope / 错误信息一律走 stderr,这是
/// `cli_spec` 的 JSON 约定(成功 → stdout / 失败 → stderr)。
///
/// 退出码按 `error.code` 字符串映射;未知 code 落到 `UNKNOWN`。
pub fn err(mode: Mode, error: ApiError) -> i32 {
    let exit = exit_codes::from_api_error_code(&error.code);
    match mode {
        Mode::Json => {
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
    exit
}
