pub use lmt_app::total_station::{run_generate_card, run_import, run_save_pdf};

use lmt_shared::dto::{InstructionCardResult, TotalStationImportResult};
use lmt_shared::error::{LmtError, LmtResult};
use std::path::Path;

use crate::pdf_render::render_html_to_pdf;

#[tauri::command]
pub fn import_total_station_csv(
    project_abs_path: String,
    csv_path: String,
    screen_id: String,
) -> LmtResult<TotalStationImportResult> {
    run_import(
        Path::new(&project_abs_path),
        &screen_id,
        Path::new(&csv_path),
    )
}

#[tauri::command]
pub fn generate_instruction_card(
    project_abs_path: String,
    screen_id: String,
) -> LmtResult<InstructionCardResult> {
    run_generate_card(Path::new(&project_abs_path), &screen_id)
}

#[tauri::command]
pub async fn save_instruction_pdf(
    app: tauri::AppHandle,
    project_abs_path: String,
    screen_id: String,
    dst_pdf_path: String,
) -> LmtResult<String> {
    // CRITICAL: the macOS PDF renderer dispatches a closure onto the AppKit
    // main thread and then blocks on a channel waiting for the result. If
    // this command itself runs on the main thread (which is where sync
    // Tauri commands land on macOS), the queued closure can never execute
    // and we'd deadlock for the full 30s timeout. Route through
    // `spawn_blocking` so we're guaranteed to be on a worker thread.
    tokio::task::spawn_blocking(move || {
        run_save_pdf(
            Path::new(&project_abs_path),
            &screen_id,
            Path::new(&dst_pdf_path),
            |html, tmp| render_html_to_pdf(&app, html, tmp),
        )
    })
    .await
    .map_err(|e| LmtError::Other(format!("PDF task join: {e}")))?
}
