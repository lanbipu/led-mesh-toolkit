pub mod commands;
pub mod data;
pub mod dto;
pub mod error;

use std::path::PathBuf;
use tauri::Manager;

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info")),
        )
        .init();

    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .setup(|app| {
            let db_path: PathBuf = app
                .path()
                .app_data_dir()
                .expect("failed to resolve app_data_dir")
                .join("lmt.sqlite");
            std::fs::create_dir_all(db_path.parent().unwrap())?;
            let db = data::open(&db_path)?;
            {
                let mut conn = db.lock().unwrap();
                data::schema::migrate(&mut conn)?;
            }
            app.manage(db);
            tracing::info!("LMT started, db at {}", db_path.display());
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            commands::projects::list_recent_projects,
            commands::projects::add_recent_project,
            commands::projects::remove_recent_project,
            commands::projects::seed_example_project,
            commands::projects::load_project_yaml,
            commands::projects::save_project_yaml,
            commands::measurements::load_measurements_yaml,
            commands::reconstruct::reconstruct_surface,
            commands::export::export_obj,
            commands::reconstruct::list_runs,
            commands::reconstruct::get_run_report,
            commands::total_station::import_total_station_csv,
            commands::total_station::generate_instruction_card,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
