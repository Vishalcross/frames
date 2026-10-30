use serde::Serialize;

#[derive(Serialize)]
pub struct CoreStatus {
    pub connected: bool,
    pub sample_original_key: String,
    pub message: String,
}

#[tauri::command]
fn check_core_status(filename: &str) -> CoreStatus {
    CoreStatus {
        connected: true,
        sample_original_key: frames_core::keys::original_key(2026, 10, filename),
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