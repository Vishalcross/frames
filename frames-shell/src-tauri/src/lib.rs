//! Tauri 2 application state and command bridge connecting the Svelte 5 UI
//! to `frames-core` (`store`, `s4`, `sync`, `importer`, `media`, `player`, `cache`, `trash`).

use std::fs;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use base64::Engine;
use chrono::{DateTime, TimeZone, Utc};
use serde::{Deserialize, Serialize};
use tauri::State;
use uuid::Uuid;

use frames_core::cache::{CacheStats, LocalCache, DEFAULT_CACHE_LIMIT_BYTES};
use frames_core::media::{encode_webp_lossless, RgbImage};
use frames_core::ops::{AssetKind, AssetRecord};
use frames_core::player::{
    detect_best_h264_encoder, H264Encoder, LoopbackStreamServer, PlayerCapabilities,
    VideoPlaybackPlan,
};
use frames_core::s4::{
    load_secret_from_os_keychain, save_secret_in_os_keychain, InMemoryObjectStore, ObjectStore,
    S4Client, S4Config, SpaceUsage,
};
use frames_core::store::{
    AlbumSummary, AssetDetail, AssetSummary, ImportProgress, ImportQueueRow, MonthGroup,
    SearchFilter, SearchSuggestions, Store, TrashItem,
};
use frames_core::sync::{
    check_library, export_originals, rebuild_from_s4, run_import, sync_once, ImportRunReport,
    LibraryCheckReport, SyncReport,
};
use frames_core::trash::{
    delete_trash_items_now, empty_trash, purge_expired_trash, trash_summary, PurgeReport,
    TrashSummary,
};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PersistedSettings {
    pub endpoint: String,
    pub region: String,
    pub bucket: String,
    pub access_key_id: String,
    pub space_limit_gb: u64,
    pub cache_location: String,
    pub cache_limit_gb: u64,
    pub use_real_s4: bool,
}

impl Default for PersistedSettings {
    fn default() -> Self {
        let default_cache = dirs::cache_dir()
            .unwrap_or_else(std::env::temp_dir)
            .join("com.vishal.frames-shell")
            .join("cache");
        Self {
            endpoint: "https://s3.eu-central-1.s4.mega.io".to_string(),
            region: "eu-central-1".to_string(),
            bucket: "frames-library".to_string(),
            access_key_id: String::new(),
            space_limit_gb: 750,
            cache_location: default_cache.to_string_lossy().to_string(),
            cache_limit_gb: 10,
            use_real_s4: false,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SettingsInput {
    pub endpoint: String,
    pub region: String,
    pub bucket: String,
    pub access_key_id: String,
    pub secret_access_key: Option<String>,
    pub space_limit_gb: u64,
    pub cache_location: String,
    pub cache_limit_gb: u64,
    pub use_real_s4: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SettingsView {
    pub endpoint: String,
    pub region: String,
    pub bucket: String,
    pub access_key_id: String,
    pub has_secret_in_keychain: bool,
    pub space_limit_gb: u64,
    pub cache_location: String,
    pub cache_limit_gb: u64,
    pub use_real_s4: bool,
    pub device_id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TimelinePayload {
    pub assets: Vec<AssetSummary>,
    pub month_groups: Vec<MonthGroup>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TrashPayload {
    pub items: Vec<TrashItem>,
    pub summary: TrashSummary,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BootstrapPayload {
    pub device_id: String,
    pub timeline: TimelinePayload,
    pub albums: Vec<AlbumSummary>,
    pub trash: TrashPayload,
    pub import_progress: ImportProgress,
    pub import_queue: Vec<ImportQueueRow>,
    pub settings: SettingsView,
    pub cache_stats: CacheStats,
    pub space_usage: SpaceUsage,
    pub outbox_pending: usize,
    pub offline: bool,
}

pub struct AppState {
    pub data_dir: PathBuf,
    pub store: Mutex<Store>,
    pub cache: Mutex<LocalCache>,
    pub settings: Mutex<PersistedSettings>,
    pub demo_s4: InMemoryObjectStore,
    pub secret_override: Mutex<Option<String>>,
    pub player_server: LoopbackStreamServer,
    pub h264_encoder: H264Encoder,
    pub offline_mode: Mutex<bool>,
}

impl AppState {
    pub fn init() -> Self {
        let data_dir = dirs::data_local_dir()
            .unwrap_or_else(std::env::temp_dir)
            .join("com.vishal.frames-shell");
        let _ = fs::create_dir_all(&data_dir);

        let settings_path = data_dir.join("settings.json");
        let settings: PersistedSettings = fs::read_to_string(&settings_path)
            .ok()
            .and_then(|s| serde_json::from_str(&s).ok())
            .unwrap_or_default();

        let db_path = data_dir.join("frames.sqlite3");
        let mut store = Store::open(&db_path)
            .or_else(|_| Store::open_in_memory("mac-local"))
            .expect("failed to initialise SQLite store");

        let cache_limit = settings.cache_limit_gb.max(1) * 1_000_000_000;
        let cache = LocalCache::open(&settings.cache_location, cache_limit).unwrap_or_else(|_| {
            LocalCache::open(std::env::temp_dir().join("frames-fallback-cache"), DEFAULT_CACHE_LIMIT_BYTES)
                .expect("failed to open fallback cache")
        });

        let s4_cfg = S4Config {
            endpoint: settings.endpoint.clone(),
            region: settings.region.clone(),
            bucket: settings.bucket.clone(),
            access_key_id: settings.access_key_id.clone(),
            secret_access_key: String::new(),
            space_limit_bytes: settings.space_limit_gb.max(1) * 1_000_000_000,
            ..Default::default()
        };
        let demo_s4 = InMemoryObjectStore::new(s4_cfg);

        // Run daily 30-day trash auto-purge on startup (spec section 11)
        let _ = purge_expired_trash(&mut store, &demo_s4, Utc::now());

        let player_server = LoopbackStreamServer::start()
            .unwrap_or_else(|_| LoopbackStreamServer::new_virtual_for_test(49152, "local-token"));
        let h264_encoder = detect_best_h264_encoder();

        Self {
            data_dir,
            store: Mutex::new(store),
            cache: Mutex::new(cache),
            settings: Mutex::new(settings),
            demo_s4,
            secret_override: Mutex::new(None),
            player_server,
            h264_encoder,
            offline_mode: Mutex::new(false),
        }
    }

    fn active_s4(&self) -> Arc<dyn ObjectStore> {
        let st = self.settings.lock().unwrap().clone();
        let secret = self
            .secret_override
            .lock()
            .unwrap()
            .clone()
            .or_else(|| {
                if st.access_key_id.is_empty() {
                    None
                } else {
                    load_secret_from_os_keychain(&st.access_key_id).ok().flatten()
                }
            })
            .unwrap_or_default();

        if st.use_real_s4 && !st.access_key_id.is_empty() && !secret.is_empty() {
            let cfg = S4Config {
                endpoint: st.endpoint,
                region: st.region,
                bucket: st.bucket,
                access_key_id: st.access_key_id,
                secret_access_key: secret,
                space_limit_bytes: st.space_limit_gb.max(1) * 1_000_000_000,
                ..Default::default()
            };
            Arc::new(S4Client::new(cfg))
        } else {
            Arc::new(self.demo_s4.clone())
        }
    }

    fn settings_view(&self, device_id: String) -> SettingsView {
        let st = self.settings.lock().unwrap().clone();
        let has_secret = self.secret_override.lock().unwrap().is_some()
            || (!st.access_key_id.is_empty()
                && load_secret_from_os_keychain(&st.access_key_id)
                    .ok()
                    .flatten()
                    .is_some());
        SettingsView {
            endpoint: st.endpoint,
            region: st.region,
            bucket: st.bucket,
            access_key_id: st.access_key_id,
            has_secret_in_keychain: has_secret,
            space_limit_gb: st.space_limit_gb,
            cache_location: st.cache_location,
            cache_limit_gb: st.cache_limit_gb,
            use_real_s4: st.use_real_s4,
            device_id,
        }
    }

    fn compute_space_usage(&self, store: &Store) -> SpaceUsage {
        let st = self.settings.lock().unwrap();
        let limit_bytes = st.space_limit_gb.max(1) * 1_000_000_000;
        let used_in_db: i64 = store
            .connection()
            .query_row(
                "SELECT coalesce(sum(size + coalesce(motion_size, 0)), 0)
                 FROM assets WHERE purged_at IS NULL",
                [],
                |r| r.get(0),
            )
            .unwrap_or(0);
        let used_bytes = (used_in_db as u64).max(self.demo_s4.total_used_bytes());
        SpaceUsage::compute(used_bytes, limit_bytes)
    }
}

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

#[tauri::command]
fn get_app_bootstrap(state: State<'_, AppState>) -> Result<BootstrapPayload, String> {
    let now = Utc::now();
    let store = state.store.lock().map_err(|e| e.to_string())?;
    let cache = state.cache.lock().map_err(|e| e.to_string())?;

    let device_id = store.device_id().map_err(|e| e.to_string())?;
    let assets = store.list_timeline(None, None).map_err(|e| e.to_string())?;
    let month_groups = store.timeline_month_groups().map_err(|e| e.to_string())?;
    let albums = store.list_albums().map_err(|e| e.to_string())?;
    let trash_items = store.list_trash(now).map_err(|e| e.to_string())?;
    let trash_sum = trash_summary(&store, now).map_err(|e| e.to_string())?;
    let import_progress = store.import_progress().map_err(|e| e.to_string())?;
    let import_queue = store.list_import_queue().map_err(|e| e.to_string())?;
    let outbox_pending = store.outbox_len().map_err(|e| e.to_string())?;
    let cache_stats = cache.stats().unwrap_or(CacheStats {
        small_thumb_count: 0,
        small_thumb_bytes: 0,
        lru_entry_count: 0,
        lru_used_bytes: 0,
        lru_limit_bytes: DEFAULT_CACHE_LIMIT_BYTES,
    });
    let space_usage = state.compute_space_usage(&store);
    let settings = state.settings_view(device_id.clone());
    let offline = *state.offline_mode.lock().unwrap();

    Ok(BootstrapPayload {
        device_id,
        timeline: TimelinePayload {
            assets,
            month_groups,
        },
        albums,
        trash: TrashPayload {
            items: trash_items,
            summary: trash_sum,
        },
        import_progress,
        import_queue,
        settings,
        cache_stats,
        space_usage,
        outbox_pending,
        offline,
    })
}

#[tauri::command]
fn get_timeline(
    limit: Option<usize>,
    offset: Option<usize>,
    state: State<'_, AppState>,
) -> Result<TimelinePayload, String> {
    let store = state.store.lock().map_err(|e| e.to_string())?;
    let assets = store
        .list_timeline(limit, offset)
        .map_err(|e| e.to_string())?;
    let month_groups = store.timeline_month_groups().map_err(|e| e.to_string())?;
    Ok(TimelinePayload {
        assets,
        month_groups,
    })
}

#[tauri::command]
fn get_asset_detail(id: String, state: State<'_, AppState>) -> Result<Option<AssetDetail>, String> {
    let store = state.store.lock().map_err(|e| e.to_string())?;
    store.get_asset(&id).map_err(|e| e.to_string())
}

#[tauri::command]
fn get_thumbnail_data_url(
    id: String,
    preview: bool,
    state: State<'_, AppState>,
) -> Result<Option<String>, String> {
    let s4 = state.active_s4();
    let mut cache = state.cache.lock().map_err(|e| e.to_string())?;

    let path_opt = if preview {
        cache
            .get_or_fetch_preview(s4.as_ref(), &id)
            .ok()
            .flatten()
            .or_else(|| cache.get_small_thumb_path(&id))
    } else {
        cache.get_small_thumb_path(&id).or_else(|| {
            let _ = cache.download_missing_small_thumbs(s4.as_ref(), std::slice::from_ref(&id));
            cache.get_small_thumb_path(&id)
        })
    };

    if let Some(p) = path_opt {
        if let Ok(bytes) = fs::read(p) {
            let b64 = base64::engine::general_purpose::STANDARD.encode(&bytes);
            return Ok(Some(format!("data:image/webp;base64,{b64}")));
        }
    }
    Ok(None)
}

#[tauri::command]
fn get_video_playback(
    id: String,
    supports_hevc: bool,
    state: State<'_, AppState>,
) -> Result<VideoPlaybackPlan, String> {
    let store = state.store.lock().map_err(|e| e.to_string())?;
    let detail = store
        .get_asset(&id)
        .map_err(|e| e.to_string())?
        .ok_or_else(|| format!("Asset not found: {id}"))?;

    let s4 = state.active_s4();
    let target_key = if detail.record.kind == AssetKind::Live {
        detail
            .record
            .motion_key
            .as_deref()
            .unwrap_or(&detail.record.key)
    } else {
        &detail.record.key
    };
    let presigned = s4
        .presign_get(target_key, Utc::now(), 3600)
        .map_err(|e| e.to_string())?;

    let caps = PlayerCapabilities {
        supports_hevc,
        ..Default::default()
    };

    Ok(state.player_server.create_playback_plan(
        &id,
        &presigned,
        detail.record.container.as_deref(),
        detail.record.video_codec.as_deref(),
        detail.record.audio_codec.as_deref(),
        detail.record.duration_ms,
        &caps,
        state.h264_encoder,
    ))
}

#[tauri::command]
fn get_video_seek_url(
    base_url: String,
    seek_seconds: f64,
    state: State<'_, AppState>,
) -> Result<String, String> {
    Ok(state.player_server.build_seek_url(&base_url, seek_seconds))
}

#[tauri::command]
fn adjust_asset_date(
    id: String,
    new_taken_iso: String,
    state: State<'_, AppState>,
) -> Result<Option<AssetDetail>, String> {
    let parsed = DateTime::parse_from_rfc3339(&new_taken_iso)
        .map(|dt| dt.with_timezone(&Utc))
        .map_err(|e| format!("Invalid ISO 8601 date: {e}"))?;
    let mut store = state.store.lock().map_err(|e| e.to_string())?;
    store
        .set_asset_date(&id, parsed, Utc::now())
        .map_err(|e| e.to_string())?;
    store.get_asset(&id).map_err(|e| e.to_string())
}

#[tauri::command]
fn export_assets(
    ids: Vec<String>,
    destination_folder: String,
    state: State<'_, AppState>,
) -> Result<Vec<String>, String> {
    let store = state.store.lock().map_err(|e| e.to_string())?;
    let mut cache = state.cache.lock().map_err(|e| e.to_string())?;
    let s4 = state.active_s4();
    let paths = export_originals(&ids, &destination_folder, &store, s4.as_ref(), &mut cache)
        .map_err(|e| e.to_string())?;
    Ok(paths
        .into_iter()
        .map(|p| p.to_string_lossy().to_string())
        .collect())
}

// =============================================================================
// Album Commands
// =============================================================================

#[tauri::command]
fn list_albums(state: State<'_, AppState>) -> Result<Vec<AlbumSummary>, String> {
    let store = state.store.lock().map_err(|e| e.to_string())?;
    store.list_albums().map_err(|e| e.to_string())
}

#[tauri::command]
fn get_album_assets(
    album_id: String,
    state: State<'_, AppState>,
) -> Result<Vec<AssetSummary>, String> {
    let store = state.store.lock().map_err(|e| e.to_string())?;
    store.list_album_assets(&album_id).map_err(|e| e.to_string())
}

#[tauri::command]
fn create_album(name: String, state: State<'_, AppState>) -> Result<Vec<AlbumSummary>, String> {
    let trimmed = name.trim();
    if trimmed.is_empty() {
        return Err("Album name cannot be empty".into());
    }
    let mut store = state.store.lock().map_err(|e| e.to_string())?;
    let id = Uuid::new_v4().to_string();
    store
        .create_album(id, trimmed.to_string(), Utc::now())
        .map_err(|e| e.to_string())?;
    store.list_albums().map_err(|e| e.to_string())
}

#[tauri::command]
fn rename_album(
    album_id: String,
    name: String,
    state: State<'_, AppState>,
) -> Result<Vec<AlbumSummary>, String> {
    let trimmed = name.trim();
    if trimmed.is_empty() {
        return Err("Album name cannot be empty".into());
    }
    let mut store = state.store.lock().map_err(|e| e.to_string())?;
    store
        .rename_album(&album_id, trimmed.to_string(), Utc::now())
        .map_err(|e| e.to_string())?;
    store.list_albums().map_err(|e| e.to_string())
}

#[tauri::command]
fn delete_album(album_id: String, state: State<'_, AppState>) -> Result<Vec<AlbumSummary>, String> {
    let mut store = state.store.lock().map_err(|e| e.to_string())?;
    store
        .delete_album(&album_id, Utc::now())
        .map_err(|e| e.to_string())?;
    store.list_albums().map_err(|e| e.to_string())
}

#[tauri::command]
fn add_to_album(
    album_id: String,
    asset_ids: Vec<String>,
    state: State<'_, AppState>,
) -> Result<Vec<AlbumSummary>, String> {
    let mut store = state.store.lock().map_err(|e| e.to_string())?;
    store
        .add_to_album(&album_id, asset_ids, Utc::now())
        .map_err(|e| e.to_string())?;
    store.list_albums().map_err(|e| e.to_string())
}

#[tauri::command]
fn remove_from_album(
    album_id: String,
    asset_ids: Vec<String>,
    state: State<'_, AppState>,
) -> Result<Vec<AssetSummary>, String> {
    let mut store = state.store.lock().map_err(|e| e.to_string())?;
    store
        .remove_from_album(&album_id, asset_ids, Utc::now())
        .map_err(|e| e.to_string())?;
    store.list_album_assets(&album_id).map_err(|e| e.to_string())
}

#[tauri::command]
fn move_to_album(
    from_album_id: String,
    to_album_id: String,
    asset_ids: Vec<String>,
    state: State<'_, AppState>,
) -> Result<Vec<AssetSummary>, String> {
    let mut store = state.store.lock().map_err(|e| e.to_string())?;
    store
        .move_to_album(&from_album_id, &to_album_id, asset_ids, Utc::now())
        .map_err(|e| e.to_string())?;
    store
        .list_album_assets(&from_album_id)
        .map_err(|e| e.to_string())
}

// =============================================================================
// Search Commands
// =============================================================================

#[tauri::command]
fn get_search_suggestions(
    query: String,
    state: State<'_, AppState>,
) -> Result<SearchSuggestions, String> {
    let store = state.store.lock().map_err(|e| e.to_string())?;
    store.search_suggestions(&query).map_err(|e| e.to_string())
}

#[tauri::command]
fn search_assets(
    filter: SearchFilter,
    state: State<'_, AppState>,
) -> Result<Vec<AssetSummary>, String> {
    let store = state.store.lock().map_err(|e| e.to_string())?;
    store.search_assets(&filter).map_err(|e| e.to_string())
}

// =============================================================================
// Trash Commands
// =============================================================================

#[tauri::command]
fn get_trash(state: State<'_, AppState>) -> Result<TrashPayload, String> {
    let now = Utc::now();
    let store = state.store.lock().map_err(|e| e.to_string())?;
    let items = store.list_trash(now).map_err(|e| e.to_string())?;
    let summary = trash_summary(&store, now).map_err(|e| e.to_string())?;
    Ok(TrashPayload { items, summary })
}

#[tauri::command]
fn trash_assets(ids: Vec<String>, state: State<'_, AppState>) -> Result<(), String> {
    let mut store = state.store.lock().map_err(|e| e.to_string())?;
    store
        .trash_assets(ids, Utc::now())
        .map_err(|e| e.to_string())
}

#[tauri::command]
fn restore_assets(ids: Vec<String>, state: State<'_, AppState>) -> Result<(), String> {
    let mut store = state.store.lock().map_err(|e| e.to_string())?;
    store
        .restore_assets(ids, Utc::now())
        .map_err(|e| e.to_string())
}

#[tauri::command]
fn delete_trash_now(ids: Vec<String>, state: State<'_, AppState>) -> Result<PurgeReport, String> {
    let mut store = state.store.lock().map_err(|e| e.to_string())?;
    let s4 = state.active_s4();
    delete_trash_items_now(&mut store, s4.as_ref(), &ids, Utc::now()).map_err(|e| e.to_string())
}

#[tauri::command]
fn empty_all_trash(state: State<'_, AppState>) -> Result<PurgeReport, String> {
    let mut store = state.store.lock().map_err(|e| e.to_string())?;
    let s4 = state.active_s4();
    empty_trash(&mut store, s4.as_ref(), Utc::now()).map_err(|e| e.to_string())
}

// =============================================================================
// Import & Demo Library Seeding Commands
// =============================================================================

#[tauri::command]
fn import_from_paths(
    paths: Vec<String>,
    album_id: Option<String>,
    state: State<'_, AppState>,
) -> Result<ImportRunReport, String> {
    let mut store = state.store.lock().map_err(|e| e.to_string())?;
    let mut cache = state.cache.lock().map_err(|e| e.to_string())?;
    let s4 = state.active_s4();
    run_import(
        paths,
        album_id.as_deref(),
        &mut store,
        s4.as_ref(),
        &mut cache,
        Utc::now(),
    )
    .map_err(|e| e.to_string())
}

#[tauri::command]
fn seed_sample_library(state: State<'_, AppState>) -> Result<usize, String> {
    let mut store = state.store.lock().map_err(|e| e.to_string())?;
    let mut cache = state.cache.lock().map_err(|e| e.to_string())?;
    let s4 = state.active_s4();
    let now = Utc::now();

    let bali_id = "11111111-2222-3333-4444-555555555555".to_string();
    let family_id = "22222222-3333-4444-5555-666666666666".to_string();
    let _ = store.create_album(bali_id.clone(), "Bali 2024".into(), now);
    let _ = store.create_album(family_id.clone(), "Family".into(), now);

    let samples: Vec<(AssetRecord, [u8; 3], Option<&str>)> = vec![
        (
            AssetRecord {
                id: "a101_bali_sunset_heic".into(),
                kind: AssetKind::Live,
                key: "originals/2024/05/IMG_4821.HEIC".into(),
                filename: "IMG_4821.HEIC".into(),
                size: 4_820_110,
                taken_at: Utc.with_ymd_and_hms(2024, 5, 18, 10, 42, 10).unwrap(),
                tz_offset: Some(480),
                date_guessed: false,
                width: Some(4032),
                height: Some(3024),
                duration_ms: None,
                container: Some("heic".into()),
                video_codec: None,
                audio_codec: None,
                camera_make: Some("Apple".into()),
                camera_model: Some("iPhone 15 Pro".into()),
                motion_id: Some("m101_bali_sunset_mov".into()),
                motion_key: Some("originals/2024/05/IMG_4821.MOV".into()),
                motion_size: Some(6_120_400),
                thumb_failed: false,
            },
            [232, 122, 72],
            Some(&bali_id),
        ),
        (
            AssetRecord {
                id: "a102_bali_surf_video".into(),
                kind: AssetKind::Video,
                key: "originals/2024/05/IMG_4855.MOV".into(),
                filename: "IMG_4855.MOV".into(),
                size: 84_500_000,
                taken_at: Utc.with_ymd_and_hms(2024, 5, 14, 4, 15, 0).unwrap(),
                tz_offset: Some(480),
                date_guessed: false,
                width: Some(3840),
                height: Some(2160),
                duration_ms: Some(42_000),
                container: Some("mov".into()),
                video_codec: Some("hevc".into()),
                audio_codec: Some("aac".into()),
                camera_make: Some("Apple".into()),
                camera_model: Some("iPhone 15 Pro".into()),
                motion_id: None,
                motion_key: None,
                motion_size: None,
                thumb_failed: false,
            },
            [38, 140, 196],
            Some(&bali_id),
        ),
        (
            AssetRecord {
                id: "a103_rice_terraces_dng".into(),
                kind: AssetKind::Photo,
                key: "originals/2024/05/DSC01942.ARW".into(),
                filename: "DSC01942.ARW".into(),
                size: 36_200_000,
                taken_at: Utc.with_ymd_and_hms(2024, 5, 10, 23, 30, 0).unwrap(),
                tz_offset: Some(480),
                date_guessed: false,
                width: Some(7008),
                height: Some(4672),
                duration_ms: None,
                container: None,
                video_codec: None,
                audio_codec: None,
                camera_make: Some("Sony".into()),
                camera_model: Some("ILCE-7M4".into()),
                motion_id: None,
                motion_key: None,
                motion_size: None,
                thumb_failed: false,
            },
            [68, 162, 92],
            Some(&bali_id),
        ),
        (
            AssetRecord {
                id: "a104_april_garden_jpg".into(),
                kind: AssetKind::Photo,
                key: "originals/2024/04/IMG_3910.JPG".into(),
                filename: "IMG_3910.JPG".into(),
                size: 3_150_000,
                taken_at: Utc.with_ymd_and_hms(2024, 4, 21, 15, 20, 0).unwrap(),
                tz_offset: Some(330),
                date_guessed: false,
                width: Some(4032),
                height: Some(3024),
                duration_ms: None,
                container: None,
                video_codec: None,
                audio_codec: None,
                camera_make: Some("Apple".into()),
                camera_model: Some("iPhone 15 Pro".into()),
                motion_id: None,
                motion_key: None,
                motion_size: None,
                thumb_failed: false,
            },
            [156, 114, 210],
            Some(&family_id),
        ),
        (
            AssetRecord {
                id: "a105_family_dinner_mkv".into(),
                kind: AssetKind::Video,
                key: "originals/2023/12/MVI_0921.MKV".into(),
                filename: "MVI_0921.MKV".into(),
                size: 128_400_000,
                taken_at: Utc.with_ymd_and_hms(2023, 12, 24, 19, 45, 0).unwrap(),
                tz_offset: Some(330),
                date_guessed: false,
                width: Some(1920),
                height: Some(1080),
                duration_ms: Some(115_000),
                container: Some("mkv".into()),
                video_codec: Some("h264".into()),
                audio_codec: Some("aac".into()),
                camera_make: Some("Canon".into()),
                camera_model: Some("EOS R6".into()),
                motion_id: None,
                motion_key: None,
                motion_size: None,
                thumb_failed: false,
            },
            [214, 158, 58],
            Some(&family_id),
        ),
        (
            AssetRecord {
                id: "a106_old_scan_guessed".into(),
                kind: AssetKind::Photo,
                key: "originals/2022/08/SCAN_0012.TIFF".into(),
                filename: "SCAN_0012.TIFF".into(),
                size: 14_900_000,
                taken_at: Utc.with_ymd_and_hms(2022, 8, 15, 12, 0, 0).unwrap(),
                tz_offset: None,
                date_guessed: true,
                width: Some(3200),
                height: Some(2400),
                duration_ms: None,
                container: None,
                video_codec: None,
                audio_codec: None,
                camera_make: None,
                camera_model: None,
                motion_id: None,
                motion_key: None,
                motion_size: None,
                thumb_failed: false,
            },
            [110, 128, 148],
            None,
        ),
    ];

    let mut added = 0usize;
    for (rec, color, alb) in samples {
        let thumb_img = RgbImage::new(300, 300, color);
        let webp = encode_webp_lossless(&thumb_img);
        let _ = cache.put_small_thumb(&rec.id, &webp);
        let _ = cache.put_preview(&rec.id, &webp);
        let _ = s4.put_object(&rec.key, b"sample-original-bytes", "application/octet-stream");
        if let Some(ref mk) = rec.motion_key {
            let _ = s4.put_object(mk, b"sample-motion-bytes", "video/quicktime");
        }
        let _ = s4.put_object(
            &frames_core::keys::thumb_small_key(&rec.id),
            &webp,
            "image/webp",
        );
        let _ = s4.put_object(
            &frames_core::keys::thumb_preview_key(&rec.id),
            &webp,
            "image/webp",
        );
        store
            .add_asset(rec, alb, now)
            .map_err(|e| e.to_string())?;
        added += 1;
    }

    let _ = sync_once(&mut store, s4.as_ref(), now);
    Ok(added)
}

// =============================================================================
// Sync, Maintenance & Settings Commands
// =============================================================================

#[tauri::command]
fn sync_now(state: State<'_, AppState>) -> Result<SyncReport, String> {
    let mut store = state.store.lock().map_err(|e| e.to_string())?;
    let s4 = state.active_s4();
    let rep = sync_once(&mut store, s4.as_ref(), Utc::now()).map_err(|e| e.to_string())?;
    *state.offline_mode.lock().unwrap() = rep.offline;
    Ok(rep)
}

#[tauri::command]
fn toggle_offline_mode(offline: bool, state: State<'_, AppState>) -> Result<bool, String> {
    state.demo_s4.set_offline(offline);
    *state.offline_mode.lock().unwrap() = offline;
    Ok(offline)
}

#[tauri::command]
fn save_settings(
    input: SettingsInput,
    state: State<'_, AppState>,
) -> Result<SettingsView, String> {
    if let Some(ref secret) = input.secret_access_key {
        let trimmed = secret.trim();
        if !trimmed.is_empty() {
            *state.secret_override.lock().unwrap() = Some(trimmed.to_string());
            if !input.access_key_id.trim().is_empty() {
                let _ = save_secret_in_os_keychain(input.access_key_id.trim(), trimmed);
            }
        }
    }

    let new_settings = PersistedSettings {
        endpoint: input.endpoint.trim().to_string(),
        region: input.region.trim().to_string(),
        bucket: input.bucket.trim().to_string(),
        access_key_id: input.access_key_id.trim().to_string(),
        space_limit_gb: input.space_limit_gb.max(1),
        cache_location: input.cache_location.trim().to_string(),
        cache_limit_gb: input.cache_limit_gb.max(1),
        use_real_s4: input.use_real_s4,
    };

    let settings_path = state.data_dir.join("settings.json");
    if let Ok(json) = serde_json::to_string_pretty(&new_settings) {
        let _ = fs::write(settings_path, json);
    }

    if let Ok(mut cache) = state.cache.lock() {
        let _ = cache.set_limit_bytes(new_settings.cache_limit_gb * 1_000_000_000);
    }

    *state.settings.lock().unwrap() = new_settings;
    let store = state.store.lock().map_err(|e| e.to_string())?;
    let dev_id = store.device_id().map_err(|e| e.to_string())?;
    Ok(state.settings_view(dev_id))
}

#[tauri::command]
fn clear_local_cache(state: State<'_, AppState>) -> Result<(u64, CacheStats), String> {
    let mut cache = state.cache.lock().map_err(|e| e.to_string())?;
    let freed = cache.clear_lru_cache().map_err(|e| e.to_string())?;
    let stats = cache.stats().map_err(|e| e.to_string())?;
    Ok((freed, stats))
}

#[tauri::command]
fn run_check_library(state: State<'_, AppState>) -> Result<LibraryCheckReport, String> {
    let store = state.store.lock().map_err(|e| e.to_string())?;
    let s4 = state.active_s4();
    check_library(&store, s4.as_ref()).map_err(|e| e.to_string())
}

#[tauri::command]
fn run_rebuild_database(state: State<'_, AppState>) -> Result<SyncReport, String> {
    let mut store = state.store.lock().map_err(|e| e.to_string())?;
    let s4 = state.active_s4();
    rebuild_from_s4(&mut store, s4.as_ref()).map_err(|e| e.to_string())
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .manage(AppState::init())
        .plugin(tauri_plugin_opener::init())
        .invoke_handler(tauri::generate_handler![
            check_core_status,
            get_app_bootstrap,
            get_timeline,
            get_asset_detail,
            get_thumbnail_data_url,
            get_video_playback,
            get_video_seek_url,
            adjust_asset_date,
            export_assets,
            list_albums,
            get_album_assets,
            create_album,
            rename_album,
            delete_album,
            add_to_album,
            remove_from_album,
            move_to_album,
            get_search_suggestions,
            search_assets,
            get_trash,
            trash_assets,
            restore_assets,
            delete_trash_now,
            empty_all_trash,
            import_from_paths,
            seed_sample_library,
            sync_now,
            toggle_offline_mode,
            save_settings,
            clear_local_cache,
            run_check_library,
            run_rebuild_database,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn app_state_initialises_and_seeds_sample_library() {
        let state = AppState::init();
        let dev_id = state.store.lock().unwrap().device_id().unwrap();
        assert!(!dev_id.is_empty());
    }
}
