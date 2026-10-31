//! Tauri 2 application state and command bridge connecting the Svelte 5 UI
//! to `frames-core` (`store`, `s4`, `sync`, `importer`, `media`, `player`, `cache`, `trash`).

use std::fs;
use std::io::{Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use base64::Engine;
use chrono::{DateTime, TimeZone, Utc};
use serde::{Deserialize, Serialize};
use tauri::http::{header, Response, StatusCode};
use tauri::{AppHandle, Emitter, Manager, State};
use uuid::Uuid;

use frames_core::cache::{CacheStats, LocalCache, DEFAULT_CACHE_LIMIT_BYTES};
use frames_core::media::{encode_webp_lossless, spawn_hidden_command, RgbImage};
use frames_core::ops::{AssetKind, AssetRecord};
use frames_core::player::{
    detect_best_h264_encoder, H264Encoder, LoopbackStreamServer, PlaybackDecision,
    PlayerCapabilities, VideoPlaybackPlan,
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
    run_import_concurrent, sync_once, ImportControl, ImportRunReport, LibraryCheckReport,
    SyncReport,
};
use std::sync::atomic::{AtomicBool, Ordering};

use frames_core::keys::{thumb_preview_key, thumb_small_key};
use frames_core::trash::{
    delete_trash_items_now, empty_trash, purge_expired_trash_concurrent, trash_summary,
    PurgeReport, TrashSummary,
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
    pub db_damaged: Option<String>,
}

pub struct AppState {
    pub data_dir: PathBuf,
    pub store: Arc<Mutex<Store>>,
    pub cache: Arc<Mutex<LocalCache>>,
    pub settings: Mutex<PersistedSettings>,
    pub demo_s4: InMemoryObjectStore,
    pub secret_override: Mutex<Option<String>>,
    pub player_server: LoopbackStreamServer,
    pub player_error: Option<String>,
    pub h264_encoder: H264Encoder,
    pub offline_mode: Mutex<bool>,
    pub db_damaged: Mutex<Option<String>>,
    pub import_control: Arc<ImportControl>,
    pub thumb_sync_running: Arc<AtomicBool>,
    pub thumb_worker: Arc<frames_core::thumb_worker::ThumbWorker>,
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
        let (store, db_damaged_err) = match Store::open(&db_path) {
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

        let cache_limit = settings.cache_limit_gb.max(1) * 1_000_000_000;
        let cache = LocalCache::open(&settings.cache_location, cache_limit).unwrap_or_else(|_| {
            LocalCache::open(
                data_dir.join("fallback-cache"),
                DEFAULT_CACHE_LIMIT_BYTES,
            )
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
        Self {
            data_dir,
            store: Arc::new(Mutex::new(store)),
            cache: Arc::new(Mutex::new(cache)),
            settings: Mutex::new(settings),
            demo_s4,
            secret_override: Mutex::new(None),
            player_server,
            player_error,
            h264_encoder,
            offline_mode: Mutex::new(false),
            db_damaged: Mutex::new(db_damaged_err),
            import_control: Arc::new(ImportControl::new()),
            thumb_sync_running: Arc::new(AtomicBool::new(false)),
            thumb_worker: Arc::new(frames_core::thumb_worker::ThumbWorker::new()),
        }
    }

    /// Resolve the active S4 object store, or return an explicit error if
    /// `use_real_s4` is enabled without valid MEGA S4 credentials.
    /// NEVER silently falls back to `demo_s4` when `use_real_s4` is `true` (Blocker 1).
    pub fn require_s4(&self) -> Result<Arc<dyn ObjectStore>, String> {
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

    pub fn is_s4_ready_for_purge(&self) -> bool {
        self.require_s4().is_ok()
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

    /// Spawn a non-blocking background worker to download any missing `s.webp`
    /// small thumbnails from S4 without holding `cache.lock()` across network I/O
    /// (Spec Section 10: "A new computer downloads small thumbnails in the background").
    pub fn spawn_background_small_thumb_sync(&self) {
        let Ok(s4) = self.require_s4() else {
            return;
        };
        if self
            .thumb_sync_running
            .compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst)
            .is_err()
        {
            return;
        }
        let store_arc = Arc::clone(&self.store);
        let cache_arc = Arc::clone(&self.cache);
        let running_flag = Arc::clone(&self.thumb_sync_running);

        let _ = std::thread::Builder::new()
            .name("frames-bg-small-thumbs".into())
            .spawn(move || {
                let all_ids: Vec<String> = {
                    let Ok(st) = store_arc.lock() else {
                        running_flag.store(false, Ordering::SeqCst);
                        return;
                    };
                    st.list_timeline(None, None)
                        .unwrap_or_default()
                        .into_iter()
                        .map(|a| a.id)
                        .collect()
                };
                let missing: Vec<String> = {
                    let Ok(c) = cache_arc.lock() else {
                        running_flag.store(false, Ordering::SeqCst);
                        return;
                    };
                    c.missing_small_thumbs(&all_ids)
                };
                for id in missing {
                    let key = thumb_small_key(&id);
                    match s4.get_object(&key) {
                        Ok(Some(bytes)) => {
                            if let Ok(c) = cache_arc.lock() {
                                let _ = c.put_small_thumb(&id, &bytes);
                            }
                        }
                        Ok(None) => {}
                        Err(_) => break,
                    }
                }
                running_flag.store(false, Ordering::SeqCst);
            });
    }
}

/// Fetch `s.webp` small thumbnail path, releasing `cache.lock()` before any S4 download.
fn fetch_small_thumb_unlocked(
    cache_mutex: &Mutex<LocalCache>,
    s4: Option<&dyn ObjectStore>,
    id: &str,
) -> Option<PathBuf> {
    if let Ok(c) = cache_mutex.lock() {
        if let Some(p) = c.get_small_thumb_path(id) {
            return Some(p);
        }
    }
    let s4 = s4?;
    let bytes = s4.get_object(&thumb_small_key(id)).ok().flatten()?;
    let c = cache_mutex.lock().ok()?;
    c.put_small_thumb(id, &bytes).ok()
}

/// Fetch `m.webp` preview path, releasing `cache.lock()` before any S4 download.
fn fetch_preview_unlocked(
    cache_mutex: &Mutex<LocalCache>,
    s4: Option<&dyn ObjectStore>,
    id: &str,
) -> Option<PathBuf> {
    if let Ok(mut c) = cache_mutex.lock() {
        if let Some(p) = c.get_preview_path(id) {
            return Some(p);
        }
    }
    if let Some(s4_store) = s4 {
        if let Ok(Some(bytes)) = s4_store.get_object(&thumb_preview_key(id)) {
            if let Ok(mut c) = cache_mutex.lock() {
                if let Ok(p) = c.put_preview(id, &bytes) {
                    return Some(p);
                }
            }
        }
    }
    fetch_small_thumb_unlocked(cache_mutex, s4, id)
}

/// Fetch original file path, releasing `cache.lock()` before streaming from S4 to disk.
#[allow(dead_code)]
fn fetch_original_unlocked(
    cache_mutex: &Mutex<LocalCache>,
    s4: Option<&dyn ObjectStore>,
    id: &str,
    filename: &str,
    s4_key: &str,
) -> Option<PathBuf> {
    let target_disk_path = {
        let mut c = cache_mutex.lock().ok()?;
        if let Some(p) = c.get_original_path(id, filename) {
            return Some(p);
        }
        c.original_disk_path(id, filename)
    };
    let s4_store = s4?;
    if s4_store.get_object_to_file(s4_key, &target_disk_path).ok()? {
        let mut c = cache_mutex.lock().ok()?;
        return c.register_downloaded_original(id, filename).ok().flatten();
    }
    None
}

fn serve_file_with_range(
    file_path: &Path,
    request_headers: &tauri::http::HeaderMap,
) -> Response<Vec<u8>> {
    let metadata = match fs::metadata(file_path) {
        Ok(m) => m,
        Err(_) => {
            return Response::builder()
                .status(StatusCode::NOT_FOUND)
                .header(header::ACCESS_CONTROL_ALLOW_ORIGIN, "*")
                .body(Vec::new())
                .unwrap();
        }
    };

    let file_len = metadata.len();
    let ext = file_path
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("")
        .to_lowercase();
    let content_type = match ext.as_str() {
        "jpg" | "jpeg" => "image/jpeg",
        "png" => "image/png",
        "webp" => "image/webp",
        "gif" => "image/gif",
        "heic" | "heif" => "image/heic",
        "tif" | "tiff" => "image/tiff",
        "mp4" => "video/mp4",
        "mov" => "video/quicktime",
        "m4v" => "video/x-m4v",
        _ => "application/octet-stream",
    };

    if let Some(range_hdr) = request_headers.get("range").and_then(|v| v.to_str().ok()) {
        if let Some(spec) = range_hdr.strip_prefix("bytes=") {
            let (s_str, e_str) = spec.split_once('-').unwrap_or((spec, ""));
            let start: u64 = s_str.parse().unwrap_or(0);
            let end: u64 = if e_str.is_empty() {
                file_len.saturating_sub(1)
            } else {
                e_str.parse().unwrap_or(file_len.saturating_sub(1)).min(file_len.saturating_sub(1))
            };

            if start <= end && start < file_len {
                let chunk_len = (end - start + 1) as usize;
                let mut f = match fs::File::open(file_path) {
                    Ok(f) => f,
                    Err(_) => {
                        return Response::builder()
                            .status(StatusCode::INTERNAL_SERVER_ERROR)
                            .header(header::ACCESS_CONTROL_ALLOW_ORIGIN, "*")
                            .body(Vec::new())
                            .unwrap();
                    }
                };
                if f.seek(SeekFrom::Start(start)).is_ok() {
                    let mut buf = vec![0u8; chunk_len];
                    if f.read_exact(&mut buf).is_ok() {
                        return Response::builder()
                            .status(StatusCode::PARTIAL_CONTENT)
                            .header(header::CONTENT_TYPE, content_type)
                            .header(header::CONTENT_RANGE, format!("bytes {start}-{end}/{file_len}"))
                            .header(header::CONTENT_LENGTH, chunk_len.to_string())
                            .header(header::ACCEPT_RANGES, "bytes")
                            .header(header::ACCESS_CONTROL_ALLOW_ORIGIN, "*")
                            .body(buf)
                            .unwrap();
                    }
                }
            }
        }
    }

    match fs::read(file_path) {
        Ok(bytes) => Response::builder()
            .status(StatusCode::OK)
            .header(header::CONTENT_TYPE, content_type)
            .header(header::CONTENT_LENGTH, file_len.to_string())
            .header(header::ACCEPT_RANGES, "bytes")
            .header(header::ACCESS_CONTROL_ALLOW_ORIGIN, "*")
            .header(header::CACHE_CONTROL, "public, max-age=3600")
            .body(bytes)
            .unwrap(),
        Err(_) => Response::builder()
            .status(StatusCode::INTERNAL_SERVER_ERROR)
            .header(header::ACCESS_CONTROL_ALLOW_ORIGIN, "*")
            .body(Vec::new())
            .unwrap(),
    }
}

/// Serve `frames://localhost/<kind>/<id>` custom protocol requests asynchronously
/// on a worker thread without holding `cache.lock()` across S4 downloads (Blocker 4).
fn handle_frames_protocol(app: &AppHandle, request: tauri::http::Request<Vec<u8>>) -> Response<Vec<u8>> {
    let state = app.state::<AppState>();

    // Support CORS preflight
    if request.method() == tauri::http::Method::OPTIONS {
        return Response::builder()
            .status(StatusCode::OK)
            .header(header::ACCESS_CONTROL_ALLOW_ORIGIN, "*")
            .header(header::ACCESS_CONTROL_ALLOW_METHODS, "GET, HEAD, OPTIONS")
            .header(header::ACCESS_CONTROL_ALLOW_HEADERS, "*")
            .body(Vec::new())
            .unwrap();
    }

    let uri = request.uri();
    let raw_path = uri.path().trim_start_matches('/');
    let host = uri.host().unwrap_or("");
    let combined = if !host.is_empty() && host != "localhost" && host != "frames.localhost" {
        format!("{host}/{raw_path}")
    } else {
        raw_path.to_string()
    };

    let segments: Vec<&str> = combined.split('/').filter(|s| !s.is_empty()).collect();
    if segments.len() < 2 {
        return Response::builder()
            .status(StatusCode::BAD_REQUEST)
            .header(header::ACCESS_CONTROL_ALLOW_ORIGIN, "*")
            .body(Vec::new())
            .unwrap();
    }

    let kind = segments[0];
    let id = segments[1];

    let s4_opt = state.require_s4().ok();
    let s4_ref = s4_opt.as_deref();

    match kind {
        "thumb" => {
            let size = match segments.get(2).copied() {
                Some("1024") => frames_core::thumbs::ThumbSize::Large,
                _ => frames_core::thumbs::ThumbSize::Small,
            };
            let thumb_file = frames_core::thumbs::thumb_path(id, size);
            if thumb_file.exists() {
                if let Ok(bytes) = fs::read(&thumb_file) {
                    return Response::builder()
                        .status(StatusCode::OK)
                        .header(header::CONTENT_TYPE, "image/webp")
                        .header(header::CACHE_CONTROL, "public, max-age=31536000, immutable")
                        .header(header::ACCESS_CONTROL_ALLOW_ORIGIN, "*")
                        .header(header::ACCESS_CONTROL_ALLOW_METHODS, "GET, HEAD, OPTIONS")
                        .body(bytes)
                        .unwrap();
                }
            }

            // Look up asset in db
            let asset_info: Option<(String, Option<String>, Option<String>)> = {
                let store = state.store.lock().ok();
                store.and_then(|s| {
                    s.connection()
                        .query_row(
                            "SELECT kind, local_path, remote_key FROM assets WHERE id = ?1",
                            [id],
                            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
                        )
                        .ok()
                })
            };

            if let Some((kind, local_path, remote_key)) = asset_info {
                if let Some(lp) = local_path {
                    let src = Path::new(&lp);
                    if src.exists() {
                        let gen_res = if kind == "video" {
                            frames_core::thumbs::generate_video_thumb(src, id, size)
                        } else {
                            frames_core::thumbs::generate_image_thumb(src, id, size)
                        };
                        if let Ok(p) = gen_res {
                            if let Ok(bytes) = fs::read(p) {
                                return Response::builder()
                                    .status(StatusCode::OK)
                                    .header(header::CONTENT_TYPE, "image/webp")
                                    .header(header::CACHE_CONTROL, "public, max-age=31536000, immutable")
                                    .header(header::ACCESS_CONTROL_ALLOW_ORIGIN, "*")
                                    .header(header::ACCESS_CONTROL_ALLOW_METHODS, "GET, HEAD, OPTIONS")
                                    .body(bytes)
                                    .unwrap();
                            }
                        }
                    }
                }

                if let Ok(s4) = state.require_s4() {
                    let fetcher = frames_core::RemoteFetcher::new(s4);
                    let remote_asset = frames_core::RemoteAsset {
                        id: id.to_string(),
                        kind: kind.clone(),
                        remote_key,
                    };
                    if let Ok(p) = tauri::async_runtime::block_on(fetcher.fetch_remote_thumb(&remote_asset, size)) {
                        if let Ok(bytes) = fs::read(p) {
                            return Response::builder()
                                .status(StatusCode::OK)
                                .header(header::CONTENT_TYPE, "image/webp")
                                .header(header::CACHE_CONTROL, "public, max-age=31536000, immutable")
                                .header(header::ACCESS_CONTROL_ALLOW_ORIGIN, "*")
                                .header(header::ACCESS_CONTROL_ALLOW_METHODS, "GET, HEAD, OPTIONS")
                                .body(bytes)
                                .unwrap();
                        }
                    }
                }
            }

            // Fallback: check old small thumb cache / S4
            if let Some(p) = fetch_small_thumb_unlocked(&state.cache, s4_ref, id) {
                if let Ok(bytes) = fs::read(p) {
                    return Response::builder()
                        .status(StatusCode::OK)
                        .header(header::CONTENT_TYPE, "image/webp")
                        .header(header::ACCESS_CONTROL_ALLOW_ORIGIN, "*")
                        .header(header::ACCESS_CONTROL_ALLOW_METHODS, "GET, HEAD, OPTIONS")
                        .header(header::CACHE_CONTROL, "public, max-age=31536000, immutable")
                        .body(bytes)
                        .unwrap();
                }
            }

            Response::builder()
                .status(StatusCode::NOT_FOUND)
                .header(header::ACCESS_CONTROL_ALLOW_ORIGIN, "*")
                .body(Vec::new())
                .unwrap()
        }
        "photo" | "original" => {
            let asset_info: Option<(String, Option<String>, Option<String>)> = {
                let store = state.store.lock().ok();
                store.and_then(|s| {
                    s.connection()
                        .query_row(
                            "SELECT kind, local_path, remote_key FROM assets WHERE id = ?1",
                            [id],
                            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
                        )
                        .ok()
                })
            };

            if let Some((kind, local_path, remote_key)) = asset_info {
                if let Some(lp) = local_path {
                    let src = Path::new(&lp);
                    if src.is_file() {
                        return serve_file_with_range(src, request.headers());
                    }
                }

                let cached_orig = frames_core::thumbs::app_cache_dir().join("originals").join(id);
                if cached_orig.is_file() {
                    return serve_file_with_range(&cached_orig, request.headers());
                }

                if let Ok(s4) = state.require_s4() {
                    let fetcher = frames_core::RemoteFetcher::new(s4);
                    let remote_asset = frames_core::RemoteAsset {
                        id: id.to_string(),
                        kind,
                        remote_key,
                    };
                    if let Ok(p) = tauri::async_runtime::block_on(fetcher.fetch_remote_original(&remote_asset)) {
                        if p.is_file() {
                            return serve_file_with_range(&p, request.headers());
                        }
                    }
                }
            }

            Response::builder()
                .status(StatusCode::NOT_FOUND)
                .header(header::ACCESS_CONTROL_ALLOW_ORIGIN, "*")
                .body(Vec::new())
                .unwrap()
        }
        "preview" => {
            let size = frames_core::thumbs::ThumbSize::Large;
            let thumb_file = frames_core::thumbs::thumb_path(id, size);
            if thumb_file.exists() {
                if let Ok(bytes) = fs::read(&thumb_file) {
                    return Response::builder()
                        .status(StatusCode::OK)
                        .header(header::CONTENT_TYPE, "image/webp")
                        .header(header::CACHE_CONTROL, "public, max-age=3600")
                        .header(header::ACCESS_CONTROL_ALLOW_ORIGIN, "*")
                        .header(header::ACCESS_CONTROL_ALLOW_METHODS, "GET, HEAD, OPTIONS")
                        .body(bytes)
                        .unwrap();
                }
            }
            if let Some(p) = fetch_preview_unlocked(&state.cache, s4_ref, id) {
                if let Ok(bytes) = fs::read(p) {
                    return Response::builder()
                        .status(StatusCode::OK)
                        .header(header::CONTENT_TYPE, "image/webp")
                        .header(header::ACCESS_CONTROL_ALLOW_ORIGIN, "*")
                        .header(header::ACCESS_CONTROL_ALLOW_METHODS, "GET, HEAD, OPTIONS")
                        .header(header::CACHE_CONTROL, "public, max-age=3600")
                        .body(bytes)
                        .unwrap();
                }
            }
            Response::builder()
                .status(StatusCode::NOT_FOUND)
                .header(header::ACCESS_CONTROL_ALLOW_ORIGIN, "*")
                .body(Vec::new())
                .unwrap()
        }
        _ => Response::builder()
            .status(StatusCode::NOT_FOUND)
            .header(header::ACCESS_CONTROL_ALLOW_ORIGIN, "*")
            .body(Vec::new())
            .unwrap(),
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
    let payload = {
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
        let db_damaged = state.db_damaged.lock().unwrap().clone();

        BootstrapPayload {
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
            db_damaged,
        }
    };

    state.spawn_background_small_thumb_sync();
    Ok(payload)
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
    let s4_opt = state.require_s4().ok();
    let s4_ref = s4_opt.as_deref();

    let path_opt = if preview {
        fetch_preview_unlocked(&state.cache, s4_ref, &id)
    } else {
        fetch_small_thumb_unlocked(&state.cache, s4_ref, &id)
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
    let detail = {
        let store = state.store.lock().map_err(|e| e.to_string())?;
        store
            .get_asset(&id)
            .map_err(|e| e.to_string())?
            .ok_or_else(|| format!("Asset not found: {id}"))?
    };

    let s4 = state.require_s4()?;
    let target_key = if detail.record.kind == AssetKind::Live {
        detail
            .record
            .motion_key
            .clone()
            .unwrap_or_else(|| detail.record.key.clone())
    } else {
        detail.record.key.clone()
    };
    let presigned = s4
        .presign_get(&target_key, Utc::now(), 3600)
        .map_err(|e| e.to_string())?;

    let caps = PlayerCapabilities {
        supports_hevc,
        ..Default::default()
    };

    let s4_for_refresher = Arc::clone(&s4);
    let key_for_refresher = target_key.clone();
    let refresher = Arc::new(move || {
        s4_for_refresher
            .presign_get(&key_for_refresher, Utc::now(), 3600)
            .ok()
    });

    let plan = state.player_server.create_playback_plan_with_refresher(
        &id,
        &presigned,
        Some(refresher),
        detail.record.container.as_deref(),
        detail.record.video_codec.as_deref(),
        detail.record.audio_codec.as_deref(),
        detail.record.duration_ms,
        &caps,
        state.h264_encoder,
    );

    if plan.decision != PlaybackDecision::PlayDirect {
        if let Some(ref srv_err) = state.player_error {
            return Err(srv_err.clone());
        }
    }

    Ok(plan)
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
async fn export_assets(
    ids: Vec<String>,
    destination_folder: String,
    state: State<'_, AppState>,
) -> Result<Vec<String>, String> {
    let store_arc = Arc::clone(&state.store);
    let cache_arc = Arc::clone(&state.cache);
    let s4 = state.require_s4()?;

    tauri::async_runtime::spawn_blocking(move || {
        let store = store_arc.lock().map_err(|e| e.to_string())?;
        let mut cache = cache_arc.lock().map_err(|e| e.to_string())?;
        let paths = export_originals(&ids, &destination_folder, &store, s4.as_ref(), &mut cache)
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
            let safe_title = title.replace('"', "\\\"");
            let script = format!("POSIX path of (choose folder with prompt \"{safe_title}\")");
            let out = spawn_hidden_command("osascript")
                .args(["-e", &script])
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
    let s4 = state.require_s4()?;
    let mut store = state.store.lock().map_err(|e| e.to_string())?;
    delete_trash_items_now(&mut store, s4.as_ref(), &ids, Utc::now()).map_err(|e| e.to_string())
}

#[tauri::command]
fn empty_all_trash(state: State<'_, AppState>) -> Result<PurgeReport, String> {
    let s4 = state.require_s4()?;
    let mut store = state.store.lock().map_err(|e| e.to_string())?;
    empty_trash(&mut store, s4.as_ref(), Utc::now()).map_err(|e| e.to_string())
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

#[tauri::command]
fn pause_import(state: State<'_, AppState>) -> Result<ImportStatusView, String> {
    state.import_control.pause();
    get_import_status(state)
}

#[tauri::command]
fn resume_import(state: State<'_, AppState>) -> Result<ImportStatusView, String> {
    state.import_control.resume();
    get_import_status(state)
}

#[tauri::command]
fn cancel_import(state: State<'_, AppState>) -> Result<ImportStatusView, String> {
    state.import_control.cancel();
    get_import_status(state)
}

#[tauri::command]
fn get_import_status(state: State<'_, AppState>) -> Result<ImportStatusView, String> {
    let store = state.store.lock().map_err(|e| e.to_string())?;
    let progress = store.import_progress().map_err(|e| e.to_string())?;
    Ok(ImportStatusView {
        running: state.import_control.is_running(),
        paused: state.import_control.is_paused(),
        cancelled: state.import_control.is_cancelled(),
        progress,
        queue: Vec::new(),
    })
}

#[tauri::command]
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

    let _ = sync_once(&mut store, s4.as_ref(), now);
    Ok(added)
}

// =============================================================================
// Sync, Maintenance & Settings Commands
// =============================================================================

#[tauri::command]
async fn sync_now(state: State<'_, AppState>) -> Result<SyncReport, String> {
    let s4 = state.require_s4()?;
    let store_arc = Arc::clone(&state.store);
    let rep = tauri::async_runtime::spawn_blocking(move || {
        let mut store = store_arc.lock().map_err(|e| e.to_string())?;
        sync_once(&mut store, s4.as_ref(), Utc::now()).map_err(|e| e.to_string())
    })
    .await
    .map_err(|e| e.to_string())??;

    *state.offline_mode.lock().unwrap() = rep.offline;
    if !rep.offline {
        state.spawn_background_small_thumb_sync();
    }
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
    let store = state.store.lock().map_err(|e| e.to_string())?;
    let dev_id = store.device_id().map_err(|e| e.to_string())?;
    Ok(state.settings_view(dev_id))
}

#[tauri::command]
fn set_local_roots(paths: Vec<String>, state: State<'_, AppState>) -> Result<(), String> {
    let store = state.store.lock().map_err(|e| e.to_string())?;
    store.set_local_roots(paths).map_err(|e| e.to_string())
}

#[tauri::command]
fn get_local_roots(state: State<'_, AppState>) -> Result<Vec<String>, String> {
    let store = state.store.lock().map_err(|e| e.to_string())?;
    store.get_local_roots().map_err(|e| e.to_string())
}

#[tauri::command]
fn scan_local_library(state: State<'_, AppState>) -> Result<usize, String> {
    let store = state.store.lock().map_err(|e| e.to_string())?;
    frames_core::scan_local::scan_all_local_roots(&store).map_err(|e| e.to_string())
}

#[tauri::command]
fn prioritize(asset_ids: Vec<String>, state: State<'_, AppState>) -> Result<(), String> {
    state.thumb_worker.prioritize(asset_ids);
    Ok(())
}

#[tauri::command]
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
        let store = store_arc.lock().map_err(|e| e.to_string())?;
        check_library(&store, s4.as_ref()).map_err(|e| e.to_string())
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
        let mut store = store_arc.lock().map_err(|e| e.to_string())?;
        let mut cache = cache_arc.lock().map_err(|e| e.to_string())?;
        import_unknown_s4_keys(&mut store, s4.as_ref(), &mut cache, &keys, Utc::now())
            .map_err(|e| e.to_string())
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
async fn run_rebuild_database(state: State<'_, AppState>) -> Result<SyncReport, String> {
    let s4 = state.require_s4()?;
    let store_arc = Arc::clone(&state.store);
    let rep = tauri::async_runtime::spawn_blocking(move || {
        let mut store = store_arc.lock().map_err(|e| e.to_string())?;
        rebuild_from_s4(&mut store, s4.as_ref()).map_err(|e| e.to_string())
    })
    .await
    .map_err(|e| e.to_string())??;

    *state.db_damaged.lock().unwrap() = None;
    state.spawn_background_small_thumb_sync();
    Ok(rep)
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .manage(AppState::init())
        .register_asynchronous_uri_scheme_protocol("frames", |ctx, request, responder| {
            let handle = ctx.app_handle().clone();
            tauri::async_runtime::spawn_blocking(move || {
                let response = handle_frames_protocol(&handle, request);
                responder.respond(response);
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
                            let _ = purge_expired_trash_concurrent(
                                &state.store,
                                s4.as_ref(),
                                Utc::now(),
                            );
                        }
                        std::thread::sleep(Duration::from_secs(24 * 3600));
                    }
                })
                .ok();

            // Background local library scanner & thumbnail worker (Task 1.2-c, 1.2-g)
            let handle_scan = app.handle().clone();
            std::thread::Builder::new()
                .name("frames-local-scan".into())
                .spawn(move || {
                    let state = handle_scan.state::<AppState>();
                    let db_path = state.data_dir.join("library.sqlite");
                    let worker = Arc::clone(&state.thumb_worker);

                    if let Ok(store) = state.store.lock() {
                        let _ = frames_core::scan_local::scan_all_local_roots(&store);
                    };

                    if let Ok(s4) = state.require_s4() {
                        if let Ok(store) = state.store.lock() {
                            let handle_alb = handle_scan.clone();
                            let _ = frames_core::sync::sync_remote_album_listings(&store, s4.as_ref(), move |album_id| {
                                let _ = handle_alb.emit("album-synced", serde_json::json!({ "album_id": album_id }));
                            });
                        }
                    }

                    let handle_events = handle_scan.clone();
                    let on_ready: frames_core::thumb_worker::ThumbReadyCallback = Arc::new(move |asset_id: &str| {
                        let _ = handle_events.emit("thumb-ready", serde_json::json!({ "asset_id": asset_id }));
                    });
                    worker.start_worker_pool(db_path, Some(on_ready));
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
            pause_import,
            resume_import,
            cancel_import,
            get_import_status,
            seed_sample_library,
            sync_now,
            toggle_offline_mode,
            save_settings,
            set_local_roots,
            get_local_roots,
            scan_local_library,
            prioritize,
            clear_local_cache,
            run_check_library,
            add_unknown_s4_files,
            run_rebuild_database,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_uri_parsing_for_frames_protocol() {
        let req1: tauri::http::Request<Vec<u8>> = tauri::http::Request::builder()
            .uri("frames://thumb/317a28b060bbddd335621ac1934bf3446b32a772d159429480bce3c453a2154d")
            .body(Vec::new())
            .unwrap();
        let uri1 = req1.uri();
        let raw_path1 = uri1.path().trim_start_matches('/');
        let host1 = uri1.host().unwrap_or("");
        println!("REQ1: host='{host1}', path='{}', raw_path='{raw_path1}'", uri1.path());

        let req2: tauri::http::Request<Vec<u8>> = tauri::http::Request::builder()
            .uri("frames://localhost/thumb/317a28b060bbddd335621ac1934bf3446b32a772d159429480bce3c453a2154d")
            .body(Vec::new())
            .unwrap();
        let uri2 = req2.uri();
        let raw_path2 = uri2.path().trim_start_matches('/');
        let host2 = uri2.host().unwrap_or("");
        println!("REQ2: host='{host2}', path='{}', raw_path='{raw_path2}'", uri2.path());
    }

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
        assert!(state.require_s4().is_ok());
        assert!(state.settings_view(dev_id).s4_configured);

        let _ = fs::remove_dir_all(&tmp);
    }
}
