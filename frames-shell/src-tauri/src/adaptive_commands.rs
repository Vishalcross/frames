//! Adaptive theme and palette Tauri commands (Phase 4).

use std::sync::Arc;
use tauri::{AppHandle, Emitter, State};
use crate::AppState;
use frames_core::{AlbumTheme, Palette};

#[tauri::command]
pub fn get_palette(
    state: State<'_, AppState>,
    asset_id: String,
) -> Result<Option<Palette>, String> {
    let store = state.store.lock().map_err(|e| e.to_string())?;
    let conn = store.connection();

    // 1. If row exists in photo_palettes, return it
    if let Ok(Some(pal)) = frames_core::palette::get_palette_from_db(conn, &asset_id) {
        return Ok(Some(pal));
    }

    // 2. If row is missing and thumbnail exists, compute now and store
    let thumb = frames_core::thumbs::thumb_path(&asset_id, frames_core::thumbs::ThumbSize::Small);
    if thumb.exists() {
        if let Ok(pal) = frames_core::palette::compute_and_store_palette(conn, &asset_id) {
            return Ok(Some(pal));
        }
    }

    // 3. If thumbnail does not exist yet, return null
    Ok(None)
}

#[tauri::command]
pub fn get_album_theme(
    state: State<'_, AppState>,
    album_id: String,
) -> Result<AlbumTheme, String> {
    let store = state.store.lock().map_err(|e| e.to_string())?;
    frames_core::palette::get_album_theme(store.connection(), &album_id)
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub fn get_asset_album_id(
    state: State<'_, AppState>,
    asset_id: String,
) -> Result<Option<String>, String> {
    let store = state.store.lock().map_err(|e| e.to_string())?;
    let conn = store.connection();
    let mut stmt = conn.prepare(
        "SELECT album_id FROM album_items WHERE asset_id = ?1 AND in_album = 1
         UNION
         SELECT album_id FROM assets WHERE id = ?1 AND album_id IS NOT NULL AND album_id != ''
         LIMIT 1"
    ).map_err(|e| e.to_string())?;
    let mut rows = stmt.query([&asset_id]).map_err(|e| e.to_string())?;
    if let Some(row) = rows.next().map_err(|e| e.to_string())? {
        Ok(Some(row.get(0).map_err(|e| e.to_string())?))
    } else {
        Ok(None)
    }
}

#[tauri::command]
pub fn set_album_theme(
    app: AppHandle,
    state: State<'_, AppState>,
    album_id: String,
    mode: String,
    source: String,
    bg: Option<String>,
    sidebar: Option<String>,
    accent: Option<String>,
) -> Result<AlbumTheme, String> {
    let store = state.store.lock().map_err(|e| e.to_string())?;
    let theme = frames_core::palette::set_album_theme(
        store.connection(),
        &album_id,
        &mode,
        &source,
        bg,
        sidebar,
        accent,
    )?;

    // If source is auto and mode is album, run regenerate_album_palette (Task 4-f #7)
    if mode == "album" && source == "auto" {
        let app_handle = app.clone();
        let alb_id = album_id.clone();
        let store_arc = Arc::clone(&state.store);
        tauri::async_runtime::spawn_blocking(move || {
            if let Ok(store) = store_arc.lock() {
                if frames_core::album_palette::generate_and_save_album_palette(store.connection(), &alb_id).is_ok() {
                    let _ = app_handle.emit("album-palette-ready", serde_json::json!({ "album_id": alb_id }));
                }
            }
        });
    }

    Ok(theme)
}

#[tauri::command]
pub fn regenerate_album_palette(
    app: AppHandle,
    state: State<'_, AppState>,
    album_id: String,
) -> Result<(), String> {
    let app_handle = app.clone();
    let alb_id = album_id.clone();
    let store_arc = Arc::clone(&state.store);
    tauri::async_runtime::spawn_blocking(move || {
        if let Ok(store) = store_arc.lock() {
            if frames_core::album_palette::generate_and_save_album_palette(store.connection(), &alb_id).is_ok() {
                let _ = app_handle.emit("album-palette-ready", serde_json::json!({ "album_id": alb_id }));
            }
        }
    });
    Ok(())
}
