//! Tauri 2 application state and command bridge connecting the Svelte 5 UI
//! to `frames-core` (`store`, `s4`, `sync`, `importer`, `media`, `player`, `cache`, `trash`).

use std::fs;
use std::path::PathBuf;
use std::sync::{Arc, Mutex, RwLock};
use std::time::Duration;

use base64::Engine;
use chrono::{DateTime, TimeZone, Utc};
use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Emitter, Manager, State};
use uuid::Uuid;

use frames_core::cache::{CacheStats, LocalCache, DEFAULT_CACHE_LIMIT_BYTES};
use frames_core::media::{encode_webp_lossless, RgbImage};
#[cfg(target_os = "windows")]
use frames_core::media::spawn_hidden_command;
use frames_core::ops::{AssetKind, AssetRecord};
use frames_core::player::{
    decide_playback, detect_best_h264_encoder, H264Encoder, LoopbackStreamServer,
    PlaybackDecision, PlayerCapabilities, UrlRefresher, VideoPlaybackPlan,
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
    check_library, export_originals, import_unknown_s4_keys, rebuild_from_s4,
    resume_unfinished_imports, run_import_concurrent, sync_once, unfinished_imports,
    ImportControl, ImportRunReport, LibraryCheckReport, SyncReport,
};
use frames_core::thumbnails::ThumbService;
use frames_core::trash::{
    delete_trash_items_now, empty_trash, purge_expired_trash, trash_summary,
    PurgeReport, TrashSummary,
};

pub mod adaptive_commands;
mod protocol;
pub use adaptive_commands::*;

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
            .join("app.frames.desktop")
            .join("cache");
        Self {
            endpoint: "https://s3.eu-central-1.s4.mega.io".to_string(),
            region: "eu-central-1".to_string(),
            bucket: "frames-library".to_string(),
            access_key_id: String::new(),
            space_limit_gb: 750,
            cache_location: default_cache.to_string_lossy().to_string(),
            cache_limit_gb: 10,
            use_real_s4: true,
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
    pub s4_configured: bool,
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

/// The timeline at startup: its size and months, not its assets. Grids fetch
/// assets a page at a time (`query_assets`), so startup and every refresh
/// stay small however large the library grows.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TimelineSummary {
    pub total_assets: usize,
    pub month_groups: Vec<MonthGroup>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TrashPayload {
    pub items: Vec<TrashItem>,
    pub summary: TrashSummary,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ImportStatusView {
    pub running: bool,
    pub paused: bool,
    pub cancelled: bool,
    pub progress: ImportProgress,
    pub queue: Vec<ImportQueueRow>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BootstrapPayload {
    pub device_id: String,
    pub timeline: TimelineSummary,
    pub albums: Vec<AlbumSummary>,
    pub trash: TrashPayload,
    pub import_progress: ImportProgress,
    pub import_queue: Vec<ImportQueueRow>,
    pub settings: SettingsView,
    pub cache_stats: CacheStats,
    pub space_usage: SpaceUsage,
    pub outbox_pending: usize,
    pub offline: bool,
    pub db_damaged: Option<String>,
}

pub struct AppState {
    pub data_dir: PathBuf,
    pub store: Arc<Mutex<Store>>,
    pub cache: Arc<Mutex<LocalCache>>,
    pub thumbs: Arc<ThumbService>,
    pub settings: Mutex<PersistedSettings>,
    pub demo_s4: InMemoryObjectStore,
    pub secret_override: Mutex<Option<String>>,
    /// Built once from settings and the keychain; cleared when settings change.
    s4_client: RwLock<Option<Arc<dyn ObjectStore>>>,
    pub player_server: LoopbackStreamServer,
    pub player_error: Option<String>,
    pub h264_encoder: H264Encoder,
    pub offline_mode: Mutex<bool>,
    pub db_damaged: Mutex<Option<String>>,
    pub import_control: Arc<ImportControl>,
}

impl AppState {
    pub fn init() -> Self {
        let data_dir = dirs::data_local_dir()
            .unwrap_or_else(std::env::temp_dir)
            .join("app.frames.desktop");
        Self::init_with_dir(data_dir)
    }

    pub fn init_with_dir(data_dir: PathBuf) -> Self {
        let _ = fs::create_dir_all(&data_dir);

        let settings_path = data_dir.join("settings.json");
        let settings: PersistedSettings = fs::read_to_string(&settings_path)
            .ok()
            .and_then(|s| serde_json::from_str(&s).ok())
            .unwrap_or_default();

        let db_path = data_dir.join("frames.sqlite3");
        let (mut store, db_damaged_err) = match Store::open(&db_path) {
            Ok(s) => (s, None),
            Err(err) => {
                let fallback = Store::open_in_memory("mac-recovery")
                    .expect("failed to initialise recovery SQLite store");
                (
                    fallback,
                    Some(format!(
                        "Local database could not be opened ({err}). Use Rebuild Database to restore from S4."
                    )),
                )
            }
        };
        // One-time move to content-addressed assets; a full copy is kept first.
        let _ = store.remove_legacy_folder_assets(
            &data_dir.join("frames.sqlite3.before-content-addressed.bak"),
        );

        let cache_limit = settings.cache_limit_gb.max(1) * 1_000_000_000;
        let cache = LocalCache::open(&settings.cache_location, cache_limit).unwrap_or_else(|_| {
            LocalCache::open(
                data_dir.join("fallback-cache"),
                DEFAULT_CACHE_LIMIT_BYTES,
            )
            .expect("failed to open fallback cache")
        });
        let cache = Arc::new(Mutex::new(cache));
        let thumbs = ThumbService::new(Arc::clone(&cache), &db_path);

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

        let (player_server, player_error) = match LoopbackStreamServer::start() {
            Ok(srv) => (srv, None),
            Err(err) => (
                LoopbackStreamServer::new_virtual_for_test(49152, "local-token"),
                Some(format!("Local streaming server failed to bind: {err}")),
            ),
        };
        let h264_encoder = detect_best_h264_encoder();

        // Do NOT run trash purge synchronously on startup so launching the app
        // while offline never blocks the window from opening (Blocker 5).
        let state = Self {
            data_dir,
            store: Arc::new(Mutex::new(store)),
            cache,
            thumbs,
            settings: Mutex::new(settings),
            demo_s4,
            secret_override: Mutex::new(None),
            s4_client: RwLock::new(None),
            player_server,
            player_error,
            h264_encoder,
            offline_mode: Mutex::new(false),
            db_damaged: Mutex::new(db_damaged_err),
            import_control: Arc::new(ImportControl::new()),
        };
        // Hand the thumbnail service its S4 client if settings already allow one.
        let _ = state.require_s4();
        state
    }

    /// Resolve the active S4 object store, or return an explicit error if
    /// `use_real_s4` is enabled without valid MEGA S4 credentials.
    /// NEVER silently falls back to `demo_s4` when `use_real_s4` is `true` (Blocker 1).
    pub fn require_s4(&self) -> Result<Arc<dyn ObjectStore>, String> {
        if let Some(s4) = self.s4_client.read().unwrap().as_ref() {
            return Ok(Arc::clone(s4));
        }
        let s4 = self.build_s4()?;
        *self.s4_client.write().unwrap() = Some(Arc::clone(&s4));
        self.thumbs.set_s4(Some(Arc::clone(&s4)));
        Ok(s4)
    }

    /// Forget the cached S4 client after a settings change.
    fn reset_s4(&self) {
        *self.s4_client.write().unwrap() = None;
        self.thumbs.set_s4(None);
        let _ = self.require_s4();
    }

    fn build_s4(&self) -> Result<Arc<dyn ObjectStore>, String> {
        let st = self.settings.lock().unwrap().clone();
        if !st.use_real_s4 {
            return Ok(Arc::new(self.demo_s4.clone()));
        }

        if st.endpoint.trim().is_empty() || st.bucket.trim().is_empty() {
            return Err(
                "MEGA S4 endpoint or bucket is empty. Open Settings to configure your S4 bucket."
                    .into(),
            );
        }
        if st.access_key_id.trim().is_empty() {
            return Err(
                "MEGA S4 Access Key ID is not configured. Open Settings to enter your S4 keys, or switch to Local Sandbox Mode."
                    .into(),
            );
        }

        let secret = self
            .secret_override
            .lock()
            .unwrap()
            .clone()
            .or_else(|| {
                load_secret_from_os_keychain(&st.access_key_id)
                    .ok()
                    .flatten()
            })
            .unwrap_or_default();

        if secret.trim().is_empty() {
            return Err(
                "MEGA S4 Secret Access Key is missing from the OS Keychain. Open Settings and save your Secret Access Key."
                    .into(),
            );
        }

        let cfg = S4Config {
            endpoint: st.endpoint,
            region: st.region,
            bucket: st.bucket,
            access_key_id: st.access_key_id,
            secret_access_key: secret,
            space_limit_bytes: st.space_limit_gb.max(1) * 1_000_000_000,
            ..Default::default()
        };
        Ok(Arc::new(S4Client::new(cfg)))
    }

    fn settings_view(&self, device_id: String) -> SettingsView {
        let st = self.settings.lock().unwrap().clone();
        let has_secret = self.secret_override.lock().unwrap().is_some()
            || (!st.access_key_id.is_empty()
                && load_secret_from_os_keychain(&st.access_key_id)
                    .ok()
                    .flatten()
                    .is_some());
        let s4_configured = !st.use_real_s4
            || (!st.endpoint.trim().is_empty()
                && !st.bucket.trim().is_empty()
                && !st.access_key_id.trim().is_empty()
                && has_secret);
        SettingsView {
            endpoint: st.endpoint,
            region: st.region,
            bucket: st.bucket,
            access_key_id: st.access_key_id,
            has_secret_in_keychain: has_secret,
            s4_configured,
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
        let used_bytes = if st.use_real_s4 {
            used_in_db as u64
        } else {
            (used_in_db as u64).max(self.demo_s4.total_used_bytes())
        };
        SpaceUsage::compute(used_bytes, limit_bytes)
    }

    /// Fill in missing `s.webp` thumbnails in the background (spec section 10),
    /// telling the UI as each one lands. Safe to call repeatedly.
    pub fn start_thumb_backfill(&self, app: &AppHandle) {
        let app = app.clone();
        self.thumbs.start_backfill(Some(Arc::new(move |id: &str| {
            let _ = app.emit("thumb-ready", serde_json::json!({ "asset_id": id }));
        })));
    }
}

/// How often the library syncs with S4 and unfinished imports are retried
/// (spec section 6: "every 5 minutes and after coming back online").
const BACKGROUND_SYNC_INTERVAL: Duration = Duration::from_secs(5 * 60);

/// One background round: push local changes, pull the other computer's, tell
/// the UI if anything arrived or the connection changed, then carry on with
/// unfinished imports. The store is only locked between S4 calls.
fn background_sync(app: &AppHandle) {
    let state = app.state::<AppState>();
    if let Ok(s4) = state.require_s4() {
        if let Ok(rep) = sync_once(&mut &*state.store, s4.as_ref(), Utc::now()) {
            let was_offline =
                std::mem::replace(&mut *state.offline_mode.lock().unwrap(), rep.offline);
            if rep.batches_pulled > 0 {
                state.start_thumb_backfill(app);
            }
            if rep.batches_pulled > 0 || was_offline != rep.offline {
                let _ = app.emit("library-changed", &rep);
            }
        }
    }
    resume_imports_in_background(app);
}

/// Carry on with imports an earlier run left unfinished, if S4 is usable and
/// no import is running. Reports through the usual `frames://import-progress`
/// events, so the Imports view and sidebar show it like any other import.
fn resume_imports_in_background(app: &AppHandle) {
    let state = app.state::<AppState>();
    if state.import_control.is_running() {
        return;
    }
    let Ok(s4) = state.require_s4() else {
        return;
    };
    let has_work = state
        .store
        .lock()
        .ok()
        .and_then(|st| unfinished_imports(&st).ok())
        .is_some_and(|groups| !groups.is_empty());
    if !has_work {
        return;
    }
    let store = Arc::clone(&state.store);
    let cache = Arc::clone(&state.cache);
    let control = Arc::clone(&state.import_control);
    let app = app.clone();
    tauri::async_runtime::spawn_blocking(move || {
        let emitter = app.clone();
        let _ = resume_unfinished_imports(store, s4, cache, control, Utc::now(), move |rep| {
            let _ = emitter.emit("frames://import-progress", rep);
        });
    });
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

#[tauri::command(async)]
fn get_app_bootstrap(app: AppHandle, state: State<'_, AppState>) -> Result<BootstrapPayload, String> {
    let now = Utc::now();
    let payload = {
        let store = state.store.lock().map_err(|e| e.to_string())?;
        let cache = state.cache.lock().map_err(|e| e.to_string())?;

        let device_id = store.device_id().map_err(|e| e.to_string())?;
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
        let db_damaged = state.db_damaged.lock().unwrap().clone();

        BootstrapPayload {
            device_id,
            timeline: TimelineSummary {
                total_assets: month_groups.iter().map(|g| g.count).sum(),
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
            db_damaged,
        }
    };

    state.start_thumb_backfill(&app);
    Ok(payload)
}

#[tauri::command(async)]
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

#[tauri::command(async)]
fn get_asset_detail(id: String, state: State<'_, AppState>) -> Result<Option<AssetDetail>, String> {
    let store = state.store.lock().map_err(|e| e.to_string())?;
    store.get_asset(&id).map_err(|e| e.to_string())
}

#[tauri::command]
async fn get_thumbnail_data_url(
    id: String,
    preview: bool,
    state: State<'_, AppState>,
) -> Result<Option<String>, String> {
    let thumbs = Arc::clone(&state.thumbs);
    tauri::async_runtime::spawn_blocking(move || {
        let path = if preview { thumbs.preview(&id) } else { thumbs.small(&id) };
        path.and_then(|p| fs::read(p).ok()).map(|bytes| {
            let b64 = base64::engine::general_purpose::STANDARD.encode(bytes);
            format!("data:image/webp;base64,{b64}")
        })
    })
    .await
    .map_err(|e| e.to_string())
}

/// How the viewer plays a video or a Live Photo's motion (spec section 8).
/// `PlayDirect` comes back without a URL: the web view loads the file through
/// `frames://` (this computer first, S4 ranges otherwise). Anything else gets
/// a loopback stream that repackages or converts, reading the local file when
/// there is one and S4 otherwise. `force_convert` is the retry after the web
/// view failed to play a file it said it supports.
#[tauri::command(async)]
fn get_video_playback(
    id: String,
    supports_hevc: bool,
    force_convert: Option<bool>,
    state: State<'_, AppState>,
) -> Result<VideoPlaybackPlan, String> {
    let rec = {
        let store = state.store.lock().map_err(|e| e.to_string())?;
        store
            .get_asset(&id)
            .map_err(|e| e.to_string())?
            .ok_or_else(|| format!("Asset not found: {id}"))?
            .record
    };
    let is_live = rec.kind == AssetKind::Live;

    // A Live Photo's motion codecs are not indexed. It is an iPhone MOV, HEVC
    // on any recent iPhone, so the Mac plays it as is and Windows without the
    // HEVC add-on converts it.
    let (container, video_codec, audio_codec) = if is_live {
        (Some("mov"), Some("hevc"), Some("aac"))
    } else {
        (
            rec.container.as_deref(),
            rec.video_codec.as_deref(),
            rec.audio_codec.as_deref(),
        )
    };
    let caps = PlayerCapabilities {
        supports_hevc,
        ..Default::default()
    };
    let decision = if force_convert.unwrap_or(false) {
        PlaybackDecision::Convert {
            copy_video: false,
            copy_audio: false,
        }
    } else {
        decide_playback(container, video_codec, audio_codec, &caps)
    };
    if decision == PlaybackDecision::PlayDirect {
        return Ok(VideoPlaybackPlan {
            asset_id: id,
            decision,
            playback_url: String::new(),
            duration_ms: rec.duration_ms,
            can_seek_via_query: false,
        });
    }
    if let Some(ref srv_err) = state.player_error {
        return Err(srv_err.clone());
    }

    let local = if is_live {
        state.thumbs.local_motion(&id)
    } else {
        state.thumbs.local_original(&id)
    };
    let (source, refresher): (String, Option<UrlRefresher>) = match local {
        Some(path) => (path.to_string_lossy().to_string(), None),
        None => {
            let key = if is_live {
                rec.motion_key
                    .clone()
                    .ok_or_else(|| "This Live Photo has no motion file.".to_string())?
            } else {
                rec.key.clone()
            };
            let s4 = state.require_s4()?;
            let url = s4
                .presign_get(&key, Utc::now(), 3600)
                .map_err(|e| e.to_string())?;
            // Long sessions re-sign before each seek; links last an hour.
            let refresher: UrlRefresher =
                Arc::new(move || s4.presign_get(&key, Utc::now(), 3600).ok());
            (url, Some(refresher))
        }
    };

    Ok(state.player_server.create_stream_session(
        &id,
        &source,
        refresher,
        decision,
        rec.duration_ms,
        state.h264_encoder,
    ))
}

#[tauri::command(async)]
fn get_video_seek_url(
    base_url: String,
    seek_seconds: f64,
    state: State<'_, AppState>,
) -> Result<String, String> {
    Ok(state.player_server.build_seek_url(&base_url, seek_seconds))
}

#[tauri::command(async)]
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
async fn export_assets(
    ids: Vec<String>,
    destination_folder: String,
    state: State<'_, AppState>,
) -> Result<Vec<String>, String> {
    let store_arc = Arc::clone(&state.store);
    let cache_arc = Arc::clone(&state.cache);
    let s4 = state.require_s4()?;

    tauri::async_runtime::spawn_blocking(move || {
        let paths =
            export_originals(&ids, &destination_folder, &mut &*store_arc, s4.as_ref(), &cache_arc)
                .map_err(|e| e.to_string())?;
        Ok(paths
            .into_iter()
            .map(|p| p.to_string_lossy().to_string())
            .collect())
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
async fn pick_folder_dialog(prompt: Option<String>) -> Result<Option<String>, String> {
    let title = prompt.unwrap_or_else(|| "Select Folder".to_string());
    tauri::async_runtime::spawn_blocking(move || {
        #[cfg(target_os = "macos")]
        {
            let safe_title = title.replace('\\', "\\\\").replace('"', "\\\"");
            let script = format!(
                "tell application \"System Events\" to activate\nPOSIX path of (choose folder with prompt \"{safe_title}\")"
            );
            let out = std::process::Command::new("/usr/bin/osascript")
                .args(["-e", &script])
                .output()
                .map_err(|e| format!("Could not open the folder dialog: {e}"))?;
            if !out.status.success() {
                let err = String::from_utf8_lossy(&out.stderr);
                // -128 is "User canceled."
                if err.contains("-128") {
                    return Ok(None);
                }
                return Err(format!("Folder dialog failed: {}", err.trim()));
            }
            let chosen = String::from_utf8_lossy(&out.stdout).trim().to_string();
            if chosen.is_empty() {
                Ok(None)
            } else {
                Ok(Some(chosen))
            }
        }
        #[cfg(target_os = "windows")]
        {
            let safe_title = title.replace('\'', "''");
            let ps = format!(
                "Add-Type -AssemblyName System.Windows.Forms; $f = New-Object System.Windows.Forms.FolderBrowserDialog; $f.Description = '{safe_title}'; if ($f.ShowDialog() -eq 'OK') {{ Write-Output $f.SelectedPath }}"
            );
            let out = spawn_hidden_command("powershell")
                .args(["-NoProfile", "-NonInteractive", "-Command", &ps])
                .output()
                .map_err(|e| e.to_string())?;
            if !out.status.success() {
                return Ok(None);
            }
            let chosen = String::from_utf8_lossy(&out.stdout).trim().to_string();
            if chosen.is_empty() {
                Ok(None)
            } else {
                Ok(Some(chosen))
            }
        }
        #[cfg(not(any(target_os = "macos", target_os = "windows")))]
        {
            let _ = title;
            Ok(None)
        }
    })
    .await
    .map_err(|e| e.to_string())?
}

// =============================================================================
// Album Commands
// =============================================================================

#[tauri::command(async)]
fn list_albums(state: State<'_, AppState>) -> Result<Vec<AlbumSummary>, String> {
    let store = state.store.lock().map_err(|e| e.to_string())?;
    store.list_albums().map_err(|e| e.to_string())
}

#[tauri::command(async)]
fn album_asset_count(album_id: String, state: State<'_, AppState>) -> Result<usize, String> {
    let store = state.store.lock().map_err(|e| e.to_string())?;
    store.album_asset_count(&album_id).map_err(|e| e.to_string())
}

#[tauri::command(async)]
fn get_album_assets(
    album_id: String,
    offset: Option<usize>,
    limit: Option<usize>,
    state: State<'_, AppState>,
) -> Result<Vec<AssetSummary>, String> {
    let store = state.store.lock().map_err(|e| e.to_string())?;
    store
        .list_album_assets_paged(&album_id, offset, limit)
        .map_err(|e| e.to_string())
}

#[tauri::command(async)]
fn debug_album(
    album_id: String,
    state: State<'_, AppState>,
) -> Result<frames_core::AlbumDebugInfo, String> {
    let store = state.store.lock().map_err(|e| e.to_string())?;
    store.debug_album(&album_id).map_err(|e| e.to_string())
}

#[tauri::command(async)]
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

#[tauri::command(async)]
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

#[tauri::command(async)]
fn delete_album(album_id: String, state: State<'_, AppState>) -> Result<Vec<AlbumSummary>, String> {
    let mut store = state.store.lock().map_err(|e| e.to_string())?;
    store
        .delete_album(&album_id, Utc::now())
        .map_err(|e| e.to_string())?;
    store.list_albums().map_err(|e| e.to_string())
}

#[tauri::command(async)]
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

#[tauri::command(async)]
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

#[tauri::command(async)]
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

#[tauri::command(async)]
fn get_search_suggestions(
    query: String,
    state: State<'_, AppState>,
) -> Result<SearchSuggestions, String> {
    let store = state.store.lock().map_err(|e| e.to_string())?;
    store.search_suggestions(&query).map_err(|e| e.to_string())
}

/// One page of the assets matching `filter`, newest first.
#[tauri::command(async)]
fn query_assets(
    filter: SearchFilter,
    offset: usize,
    limit: usize,
    state: State<'_, AppState>,
) -> Result<Vec<AssetSummary>, String> {
    let store = state.store.lock().map_err(|e| e.to_string())?;
    store
        .query_assets(&filter, offset, limit.min(1000))
        .map_err(|e| e.to_string())
}

#[tauri::command(async)]
fn count_assets(filter: SearchFilter, state: State<'_, AppState>) -> Result<usize, String> {
    let store = state.store.lock().map_err(|e| e.to_string())?;
    store.count_assets(&filter).map_err(|e| e.to_string())
}

#[tauri::command(async)]
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

#[tauri::command(async)]
fn get_trash(state: State<'_, AppState>) -> Result<TrashPayload, String> {
    let now = Utc::now();
    let store = state.store.lock().map_err(|e| e.to_string())?;
    let items = store.list_trash(now).map_err(|e| e.to_string())?;
    let summary = trash_summary(&store, now).map_err(|e| e.to_string())?;
    Ok(TrashPayload { items, summary })
}

#[tauri::command(async)]
fn trash_assets(ids: Vec<String>, state: State<'_, AppState>) -> Result<(), String> {
    let mut store = state.store.lock().map_err(|e| e.to_string())?;
    store
        .trash_assets(ids, Utc::now())
        .map_err(|e| e.to_string())
}

#[tauri::command(async)]
fn restore_assets(ids: Vec<String>, state: State<'_, AppState>) -> Result<(), String> {
    let mut store = state.store.lock().map_err(|e| e.to_string())?;
    store
        .restore_assets(ids, Utc::now())
        .map_err(|e| e.to_string())
}

#[tauri::command]
async fn delete_trash_now(ids: Vec<String>, state: State<'_, AppState>) -> Result<PurgeReport, String> {
    let s4 = state.require_s4()?;
    let store = Arc::clone(&state.store);
    tauri::async_runtime::spawn_blocking(move || {
        delete_trash_items_now(&mut &*store, s4.as_ref(), &ids, Utc::now()).map_err(|e| e.to_string())
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
async fn empty_all_trash(state: State<'_, AppState>) -> Result<PurgeReport, String> {
    let s4 = state.require_s4()?;
    let store = Arc::clone(&state.store);
    tauri::async_runtime::spawn_blocking(move || {
        empty_trash(&mut &*store, s4.as_ref(), Utc::now()).map_err(|e| e.to_string())
    })
    .await
    .map_err(|e| e.to_string())?
}

// =============================================================================
// Import & Demo Library Seeding Commands
// =============================================================================

#[tauri::command]
async fn import_from_paths(
    paths: Vec<String>,
    album_id: Option<String>,
    app: AppHandle,
    state: State<'_, AppState>,
) -> Result<ImportRunReport, String> {
    if state.import_control.is_running() {
        return Err("An import is already running.".into());
    }
    let s4 = state.require_s4()?;
    let store_arc = Arc::clone(&state.store);
    let cache_arc = Arc::clone(&state.cache);
    let control = Arc::clone(&state.import_control);

    tauri::async_runtime::spawn_blocking(move || {
        run_import_concurrent(
            paths,
            album_id.as_deref(),
            store_arc,
            s4,
            cache_arc,
            control,
            Utc::now(),
            |rep| {
                let _ = app.emit("frames://import-progress", rep);
            },
        )
        .map_err(|e| e.to_string())
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command(async)]
fn pause_import(state: State<'_, AppState>) -> Result<ImportStatusView, String> {
    state.import_control.pause();
    get_import_status(state)
}

#[tauri::command(async)]
fn resume_import(state: State<'_, AppState>) -> Result<ImportStatusView, String> {
    state.import_control.resume();
    get_import_status(state)
}

#[tauri::command(async)]
fn cancel_import(state: State<'_, AppState>) -> Result<ImportStatusView, String> {
    state.import_control.cancel();
    get_import_status(state)
}

#[tauri::command(async)]
fn reset_import(state: State<'_, AppState>) -> Result<ImportStatusView, String> {
    state.import_control.force_reset();
    get_import_status(state)
}

#[tauri::command(async)]
fn clear_import_queue(all: Option<bool>, state: State<'_, AppState>) -> Result<ImportStatusView, String> {
    {
        let mut store = state.store.lock().map_err(|e| e.to_string())?;
        if all.unwrap_or(false) {
            store.clear_all_import_queue().map_err(|e| e.to_string())?;
        } else {
            store.clear_finished_imports().map_err(|e| e.to_string())?;
        }
    }
    get_import_status(state)
}

#[tauri::command(async)]
fn get_import_status(state: State<'_, AppState>) -> Result<ImportStatusView, String> {
    let store = state.store.lock().map_err(|e| e.to_string())?;
    let progress = store.import_progress().map_err(|e| e.to_string())?;
    let queue = store.list_import_queue().unwrap_or_default();
    Ok(ImportStatusView {
        running: state.import_control.is_running(),
        paused: state.import_control.is_paused(),
        cancelled: state.import_control.is_cancelled(),
        progress,
        queue,
    })
}

/// Sample assets get content-addressed ids like real ones (spec section 4).
fn sample_id(name: &str) -> String {
    frames_core::media::sha256_bytes(name.as_bytes())
}

#[tauri::command(async)]
fn seed_sample_library(state: State<'_, AppState>) -> Result<usize, String> {
    // Explicitly switch to Local Sandbox Mode when seeding the demo library if real S4 is unconfigured
    {
        let mut st = state.settings.lock().unwrap();
        if st.access_key_id.is_empty() {
            st.use_real_s4 = false;
        }
    }

    let s4 = state.require_s4()?;
    let mut store = state.store.lock().map_err(|e| e.to_string())?;
    let mut cache = state.cache.lock().map_err(|e| e.to_string())?;
    let now = Utc::now();

    let bali_id = "11111111-2222-3333-4444-555555555555".to_string();
    let family_id = "22222222-3333-4444-5555-666666666666".to_string();
    let _ = store.create_album(bali_id.clone(), "Bali 2024".into(), now);
    let _ = store.create_album(family_id.clone(), "Family".into(), now);

    let samples: Vec<(AssetRecord, [u8; 3], Option<&str>)> = vec![
        (
            AssetRecord {
                id: sample_id("a101_bali_sunset_heic"),
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
                motion_id: Some(sample_id("m101_bali_sunset_mov")),
                motion_key: Some("originals/2024/05/IMG_4821.MOV".into()),
                motion_size: Some(6_120_400),
                thumb_failed: false,
            },
            [232, 122, 72],
            Some(&bali_id),
        ),
        (
            AssetRecord {
                id: sample_id("a102_bali_surf_video"),
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
                id: sample_id("a103_rice_terraces_dng"),
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
                id: sample_id("a104_april_garden_jpg"),
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
                id: sample_id("a105_family_dinner_mkv"),
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
                id: sample_id("a106_old_scan_guessed"),
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
        let _ = s4.put_object(&rec.key, &webp, "image/webp");
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

    drop(cache);
    drop(store);
    let _ = sync_once(&mut &*state.store, s4.as_ref(), now);
    Ok(added)
}

// =============================================================================
// Sync, Maintenance & Settings Commands
// =============================================================================

#[tauri::command]
async fn sync_now(app: AppHandle, state: State<'_, AppState>) -> Result<SyncReport, String> {
    let s4 = state.require_s4()?;
    let store_arc = Arc::clone(&state.store);

    // Record album asset counts for albums with mode = 'album' AND source = 'auto' before sync
    let pre_sync_counts: Vec<(String, usize)> = {
        if let Ok(store) = store_arc.lock() {
            let conn = store.connection();
            let mut stmt = match conn.prepare(
                "SELECT album_id FROM album_theme WHERE mode = 'album' AND source = 'auto'"
            ) {
                Ok(s) => s,
                Err(_) => return Err("failed to prepare album_theme query".into()),
            };
            let alb_ids: Vec<String> = stmt
                .query_map([], |r| r.get(0))
                .map_err(|e| e.to_string())?
                .filter_map(|r| r.ok())
                .collect();
            alb_ids
                .into_iter()
                .map(|id| {
                    let cnt = store.album_asset_count(&id).unwrap_or(0);
                    (id, cnt)
                })
                .collect()
        } else {
            Vec::new()
        }
    };

    let rep = tauri::async_runtime::spawn_blocking(move || {
        sync_once(&mut &*store_arc, s4.as_ref(), Utc::now()).map_err(|e| e.to_string())
    })
    .await
    .map_err(|e| e.to_string())??;

    *state.offline_mode.lock().unwrap() = rep.offline;
    if !rep.offline {
        state.start_thumb_backfill(&app);

        // Check if any album's file count changed by > 10% after sync (Task 4-f #7)
        if let Ok(store) = state.store.lock() {
            for (alb_id, old_cnt) in pre_sync_counts {
                let new_cnt = store.album_asset_count(&alb_id).unwrap_or(0);
                let changed = if old_cnt == 0 {
                    new_cnt > 0
                } else {
                    (new_cnt as f64 - old_cnt as f64).abs() / (old_cnt as f64) > 0.10
                };
                if changed {
                    let app_handle = app.clone();
                    let store_arc_clone = Arc::clone(&state.store);
                    let cache_root = state.thumbs.cache_root().to_path_buf();
                    let alb_id_clone = alb_id.clone();
                    tauri::async_runtime::spawn_blocking(move || {
                        if let Ok(store) = store_arc_clone.lock() {
                            if frames_core::album_palette::generate_and_save_album_palette(store.connection(), &cache_root, &alb_id_clone).is_ok() {
                                let _ = app_handle.emit("album-palette-ready", serde_json::json!({ "album_id": alb_id_clone }));
                            }
                        }
                    });
                }
            }
        }
    }
    Ok(rep)
}

#[tauri::command(async)]
fn toggle_offline_mode(offline: bool, state: State<'_, AppState>) -> Result<bool, String> {
    state.demo_s4.set_offline(offline);
    *state.offline_mode.lock().unwrap() = offline;
    Ok(offline)
}

#[tauri::command(async)]
fn save_settings(
    input: SettingsInput,
    app: AppHandle,
    state: State<'_, AppState>,
) -> Result<SettingsView, String> {
    let access_key = input.access_key_id.trim();
    if let Some(ref secret) = input.secret_access_key {
        let trimmed = secret.trim();
        if !trimmed.is_empty() {
            if input.use_real_s4 {
                if access_key.is_empty() {
                    return Err(
                        "Access Key ID is required when saving a Secret Access Key.".into(),
                    );
                }
                // Never ignore Keychain save errors in real S4 mode (Blocker 1):
                // if Keychain persistence fails, return an explicit error so the secret
                // is never silently lost on next launch.
                save_secret_in_os_keychain(access_key, trimmed).map_err(|e| {
                    format!("Failed to save Secret Access Key to OS Keychain: {e}")
                })?;
            } else if !access_key.is_empty() {
                let _ = save_secret_in_os_keychain(access_key, trimmed);
            }
            *state.secret_override.lock().unwrap() = Some(trimmed.to_string());
        }
    }

    let new_settings = PersistedSettings {
        endpoint: input.endpoint.trim().to_string(),
        region: input.region.trim().to_string(),
        bucket: input.bucket.trim().to_string(),
        access_key_id: access_key.to_string(),
        space_limit_gb: input.space_limit_gb.max(1),
        cache_location: input.cache_location.trim().to_string(),
        cache_limit_gb: input.cache_limit_gb.max(1),
        use_real_s4: input.use_real_s4,
    };

    let settings_path = state.data_dir.join("settings.json");
    let json = serde_json::to_string_pretty(&new_settings)
        .map_err(|e| format!("Failed to serialize settings: {e}"))?;
    fs::write(&settings_path, json)
        .map_err(|e| format!("Failed to write settings.json: {e}"))?;

    if let Ok(mut cache) = state.cache.lock() {
        let _ = cache.set_limit_bytes(new_settings.cache_limit_gb * 1_000_000_000);
    }

    *state.settings.lock().unwrap() = new_settings;
    state.reset_s4();
    state.start_thumb_backfill(&app);
    resume_imports_in_background(&app);

    let store = state.store.lock().map_err(|e| e.to_string())?;
    let dev_id = store.device_id().map_err(|e| e.to_string())?;
    Ok(state.settings_view(dev_id))
}

#[derive(Debug, serde::Serialize, serde::Deserialize)]
pub struct WorkerStatus {
    pub uploading_count: usize,
    pub thumb_generating_count: usize,
}

#[tauri::command(async)]
fn get_worker_status(state: State<'_, AppState>) -> Result<WorkerStatus, String> {
    let uploading_count = if state.import_control.is_running() {
        let store = state.store.lock().map_err(|e| e.to_string())?;
        let p = store.import_progress().map_err(|e| e.to_string())?;
        p.total_files
            .saturating_sub(p.completed_files + p.skipped_duplicates + p.failed_files)
    } else {
        0
    };
    Ok(WorkerStatus {
        uploading_count,
        thumb_generating_count: state.thumbs.pending_count(),
    })
}

#[tauri::command(async)]
fn clear_local_cache(state: State<'_, AppState>) -> Result<(u64, CacheStats), String> {
    let mut cache = state.cache.lock().map_err(|e| e.to_string())?;
    let freed = cache.clear_lru_cache().map_err(|e| e.to_string())?;
    let stats = cache.stats().map_err(|e| e.to_string())?;
    Ok((freed, stats))
}

#[tauri::command]
async fn run_check_library(state: State<'_, AppState>) -> Result<LibraryCheckReport, String> {
    let s4 = state.require_s4()?;
    let store_arc = Arc::clone(&state.store);
    tauri::async_runtime::spawn_blocking(move || {
        check_library(&mut &*store_arc, s4.as_ref()).map_err(|e| e.to_string())
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
async fn add_unknown_s4_files(
    keys: Vec<String>,
    state: State<'_, AppState>,
) -> Result<usize, String> {
    let s4 = state.require_s4()?;
    let store_arc = Arc::clone(&state.store);
    let cache_arc = Arc::clone(&state.cache);
    tauri::async_runtime::spawn_blocking(move || {
        import_unknown_s4_keys(&mut &*store_arc, s4.as_ref(), &cache_arc, &keys, Utc::now())
            .map_err(|e| e.to_string())
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
async fn run_rebuild_database(app: AppHandle, state: State<'_, AppState>) -> Result<SyncReport, String> {
    let s4 = state.require_s4()?;
    let store_arc = Arc::clone(&state.store);
    let rep = tauri::async_runtime::spawn_blocking(move || {
        rebuild_from_s4(&mut &*store_arc, s4.as_ref()).map_err(|e| e.to_string())
    })
    .await
    .map_err(|e| e.to_string())??;

    *state.db_damaged.lock().unwrap() = None;
    state.start_thumb_backfill(&app);
    Ok(rep)
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .manage(AppState::init())
        .register_asynchronous_uri_scheme_protocol("frames", |ctx, request, responder| {
            let handle = ctx.app_handle().clone();
            tauri::async_runtime::spawn_blocking(move || {
                responder.respond(protocol::handle(&handle, request));
            });
        })
        .setup(|app| {
            // Non-blocking background trash purge (Spec Section 11, Blocker 5):
            // Runs 5 seconds after window launch and every 24 hours thereafter
            // without holding `store.lock()` across S4 network DELETE requests.
            let handle = app.handle().clone();
            std::thread::Builder::new()
                .name("frames-daily-trash-purge".into())
                .spawn(move || {
                    std::thread::sleep(Duration::from_secs(5));
                    loop {
                        let state = handle.state::<AppState>();
                        if let Ok(s4) = state.require_s4() {
                            let _ = purge_expired_trash(&mut &*state.store, s4.as_ref(), Utc::now());
                        }
                        std::thread::sleep(Duration::from_secs(24 * 3600));
                    }
                })
                .ok();

            app.state::<AppState>().start_thumb_backfill(app.handle());

            // The UI syncs once at launch; this picks up unfinished imports
            // shortly after, then keeps syncing every few minutes.
            let handle = app.handle().clone();
            std::thread::Builder::new()
                .name("frames-background-sync".into())
                .spawn(move || {
                    std::thread::sleep(Duration::from_secs(5));
                    resume_imports_in_background(&handle);
                    loop {
                        std::thread::sleep(BACKGROUND_SYNC_INTERVAL);
                        background_sync(&handle);
                    }
                })
                .ok();
            Ok(())
        })
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
            pick_folder_dialog,
            list_albums,
            album_asset_count,
            debug_album,
            get_album_assets,
            create_album,
            rename_album,
            delete_album,
            add_to_album,
            remove_from_album,
            move_to_album,
            get_search_suggestions,
            search_assets,
            query_assets,
            count_assets,
            get_trash,
            trash_assets,
            restore_assets,
            delete_trash_now,
            empty_all_trash,
            import_from_paths,
            pause_import,
            resume_import,
            cancel_import,
            reset_import,
            clear_import_queue,
            get_import_status,
            seed_sample_library,
            sync_now,
            toggle_offline_mode,
            save_settings,
            get_worker_status,
            clear_local_cache,
            run_check_library,
            add_unknown_s4_files,
            run_rebuild_database,
            get_palette,
            get_album_theme,
            set_album_theme,
            regenerate_album_palette,
            get_asset_album_id,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn app_state_initialises_in_isolated_temp_dir_and_rejects_unconfigured_real_s4() {
        let tmp = std::env::temp_dir().join(format!("frames-shell-test-{}", fastrand::u64(..)));
        let state = AppState::init_with_dir(tmp.clone());
        let dev_id = state.store.lock().unwrap().device_id().unwrap();
        assert!(!dev_id.is_empty());
        assert!(state.db_damaged.lock().unwrap().is_none());

        // By default `use_real_s4` is true and no S4 credentials are set:
        // `require_s4` MUST return an error and NEVER silently fall back to `demo_s4`.
        assert!(state.require_s4().is_err());
        assert!(!state.settings_view(dev_id.clone()).s4_configured);

        // Explicitly switching `use_real_s4 = false` (Local Sandbox Mode) enables `demo_s4`.
        state.settings.lock().unwrap().use_real_s4 = false;
        state.reset_s4();
        assert!(state.require_s4().is_ok());
        assert!(state.settings_view(dev_id).s4_configured);

        let _ = fs::remove_dir_all(&tmp);
    }
}
