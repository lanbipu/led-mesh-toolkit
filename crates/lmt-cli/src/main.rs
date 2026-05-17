//! `lmt` — agent-friendly CLI for the LED Mesh Toolkit.
//!
//! 整体形态见 `cli_spec`:
//! - 默认 human-readable 输出;`--json` 切到稳定 envelope。
//! - stdout 输出业务结果(JSON 模式下含完整 envelope);stderr 输出错误
//!   envelope + 人类日志,二者互不污染。
//! - 退出码语义化,见 `lmt_shared::exit_codes`。

mod cli;
mod output;
mod commands;

use clap::Parser;

fn main() {
    // `--json` 模式下 stderr 是 ErrorEnvelope 的专属通道;tracing 也写 stderr
    // 会让 agent 看到 log line + envelope 两份内容,解析失败。所以在 parse 前
    // 先 peek 一下 `--json`,只在 human 模式启用 tracing。
    // 用 args_os 是因为 args() 在非 UTF-8 argv 上会 panic。
    let json_mode_early = std::env::args_os().any(|a| a == std::ffi::OsStr::new("--json"));
    if !json_mode_early {
        tracing_subscriber::fmt()
            .with_writer(std::io::stderr)
            .with_env_filter(
                tracing_subscriber::EnvFilter::try_from_env("LMT_LOG")
                    .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("warn")),
            )
            .with_target(false)
            .init();
    }

    let args = match cli::Cli::try_parse() {
        Ok(args) => args,
        Err(e) => {
            // clap 的 parse error 走自己默认渠道。但 `--help` / `--version` 也走
            // 这个分支(它们是"成功的"非业务出口),原样让 clap 自己 print + 退出。
            //
            // 只在真实参数错误(`ErrorKind::*` 非 help/version)且用户传了 `--json` 时
            // 才把 error 包成稳定 envelope —— 否则破坏 agent / pipeline 的契约。
            // 只把用户主动请求的 --help / --version 当成非错误出口。
            // `DisplayHelpOnMissingArgumentOrSubcommand` 看起来像 "show help",
            // 实际是缺 required arg / subcommand 时 clap 自带的 fallback,
            // 它仍是 parse failure;`--json` 模式下要走 envelope,否则 agent
            // 看到的是 human help 而非机器可读错误。
            use clap::error::ErrorKind;
            let is_help_or_version =
                matches!(e.kind(), ErrorKind::DisplayHelp | ErrorKind::DisplayVersion);
            // 用 args_os 而非 args,Unix argv 可以是任意字节序列;遇到非 UTF-8
            // 参数时 std::env::args() 会 panic,把我们 envelope handler 也炸掉。
            let json_flag = std::ffi::OsStr::new("--json");
            let wants_json =
                !is_help_or_version && std::env::args_os().any(|a| a == json_flag);
            if wants_json {
                let api = lmt_shared::envelope::ApiError::new(
                    lmt_shared::envelope::error_codes::INVALID_INPUT,
                    format!("argument parse error: {e}"),
                );
                let exit = output::err(output::Mode::Json, api);
                std::process::exit(exit);
            }
            e.exit();
        }
    };
    let exit_code = commands::dispatch(args);
    std::process::exit(exit_code);
}
