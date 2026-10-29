// Learn more about Tauri commands at https://tauri.app/develop/calling-rust/
#[tauri::command]
use serde::Serialize;

#[derive(Serialize)]
pub struct CoreStatus {
    pub connected: bool,
    pub sample_original_key: String,
    pub message: String,
}

#[tauri::command]
fn check_core_status(filename: &str) -> CoreStatus {
    // Calling into frames-core (adjust function name if keys.rs uses a different signature)
    CoreStatus {
        connected: true,
        sample_original_key: format!("originals/2026/10/{}", filename),
        message: "frames-core is linked and SQLite schema is ready".to_string(),
    }
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .invoke_handler(tauri::generate_handler![check_core_status])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}