//! Swift bindings for frames-core (frames_swift_spec.md section 5). The only
//! crate the Mac app links. Everything here is a thin wrapper: the logic stays
//! in frames-core, shared with the Windows app.

use std::path::{Path, PathBuf};
use std::sync::mpsc;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use base64::engine::general_purpose::STANDARD as BASE64;
use base64::Engine;
use chrono::Utc;
use frames_core::cache::LocalCache;
use frames_core::lock::LibraryLock;
use frames_core::ops::AssetKind;
use frames_core::qos::{self, WorkClass};
use frames_core::s4::{load_secret_from_os_keychain, ObjectStore, S4Client, S4Config};
use frames_core::store::{SearchFilter, Store};
use frames_core::sync::sync_once;
use frames_core::thumbnails::ThumbService;

uniffi::setup_scaffolding!();

/// How often the library syncs with S4 (spec section 6).
const SYNC_INTERVAL: Duration = Duration::from_secs(5 * 60);
/// Background work waits this long after opening, so the first screen reads
/// the database and the disk alone (spec 7.1: launch budget).
const STARTUP_QUIET: Duration = Duration::from_secs(3);

#[derive(Debug, thiserror::Error, uniffi::Error)]
pub enum FramesError {
    #[error("Frames is already open in another app. Quit it, then try again.")]
    AlreadyOpen,
    #[error("The library database could not be opened: {message}")]
    DamagedDatabase { message: String },
    #[error("{message}")]
    Other { message: String },
}

impl FramesError {
    fn other(err: impl std::fmt::Display) -> Self {
        Self::Other { message: err.to_string() }
    }
}

/// Told about changes that happen in the background. Called on Rust threads;
/// the Swift side must hand each call to the right actor and return at once.
#[uniffi::export(with_foreign)]
pub trait LibraryObserver: Send + Sync {
    /// Edits from another computer arrived: reload what is shown.
    fn library_changed(&self);
    /// A thumbnail that was missing is now on disk.
    fn thumb_ready(&self, asset_id: String);
    /// Whether the last sync reached S4.
    fn sync_state(&self, online: bool);
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, uniffi::Enum)]
pub enum MediaKind {
    Photo,
    Video,
    Live,
}

impl From<AssetKind> for MediaKind {
    fn from(kind: AssetKind) -> Self {
        match kind {
            AssetKind::Photo => Self::Photo,
            AssetKind::Video => Self::Video,
            AssetKind::Live => Self::Live,
        }
    }
}

/// Which assets a timeline holds. All `None` and `false` is the whole library.
#[derive(Debug, Clone, Default, uniffi::Record)]
pub struct TimelineFilter {
    pub album_id: Option<String>,
    pub videos_only: bool,
}

/// A whole timeline as packed arrays, newest first (spec 5.3): one call per
/// screen, never one per tile. Entry `i` of each array describes asset `i`.
/// Numbers are little-endian bytes, which cross into Swift as one copy
/// rather than one conversion per value.
#[derive(Debug, Clone, uniffi::Record)]
pub struct TimelineSnapshot {
    pub count: u32,
    /// Asset ids, UTF-8, concatenated; asset `i` ends at `id_ends[i]`.
    pub ids: Vec<u8>,
    /// u32 per asset.
    pub id_ends: Vec<u8>,
    /// i64 per asset: date taken as local wall-clock time, ms since 1970 read as UTC.
    pub local_ms: Vec<u8>,
    /// One byte per asset: bit 0 video, bit 1 Live Photo, bit 2 date guessed,
    /// bit 3 thumbnail failed.
    pub flags: Vec<u8>,
    /// u32 per asset: video length in whole seconds, 0 for photos.
    pub duration_s: Vec<u8>,
    /// Raw ThumbHash bytes, concatenated; asset `i` ends at `thumbhash_ends[i]`.
    pub thumbhashes: Vec<u8>,
    /// u32 per asset.
    pub thumbhash_ends: Vec<u8>,
}

pub const FLAG_VIDEO: u8 = 1;
pub const FLAG_LIVE: u8 = 2;
pub const FLAG_DATE_GUESSED: u8 = 4;
pub const FLAG_THUMB_FAILED: u8 = 8;

#[derive(Debug, Clone, uniffi::Record)]
pub struct AlbumSummary {
    pub id: String,
    pub name: String,
    pub created_at_ms: i64,
    pub asset_count: u64,
    pub cover_asset_id: Option<String>,
}

/// Everything the info panel shows about one asset.
#[derive(Debug, Clone, uniffi::Record)]
pub struct AssetDetail {
    pub id: String,
    pub kind: MediaKind,
    pub filename: String,
    pub size: u64,
    pub taken_utc_ms: i64,
    pub tz_offset_minutes: Option<i32>,
    pub date_guessed: bool,
    pub width: Option<u32>,
    pub height: Option<u32>,
    pub duration_ms: Option<u64>,
    pub camera: Option<String>,
    pub albums: Vec<String>,
}

/// `settings.json` as the Tauri app writes it. Read only until the Mac app
/// has its own Settings window (Phase 6).
#[derive(Debug, Clone, serde::Deserialize)]
#[serde(default)]
struct Settings {
    endpoint: String,
    region: String,
    bucket: String,
    access_key_id: String,
    space_limit_gb: u64,
    cache_location: String,
    cache_limit_gb: u64,
    use_real_s4: bool,
}

impl Default for Settings {
    fn default() -> Self {
        let cache = dirs::cache_dir()
            .unwrap_or_else(std::env::temp_dir)
            .join("app.frames.desktop")
            .join("cache");
        Self {
            endpoint: "https://s3.eu-central-1.s4.mega.io".to_string(),
            region: "eu-central-1".to_string(),
            bucket: "frames-library".to_string(),
            access_key_id: String::new(),
            space_limit_gb: 750,
            cache_location: cache.to_string_lossy().to_string(),
            cache_limit_gb: 10,
            use_real_s4: false,
        }
    }
}

impl Settings {
    fn load(data_dir: &Path) -> Self {
        std::fs::read_to_string(data_dir.join("settings.json"))
            .ok()
            .and_then(|s| serde_json::from_str(&s).ok())
            .unwrap_or_default()
    }

    /// The S4 client these settings describe, or why there is none.
    fn s4(&self) -> Result<Arc<dyn ObjectStore>, String> {
        if !self.use_real_s4 {
            return Err("Not connected to MEGA S4. Connect in the Frames settings.".into());
        }
        if self.endpoint.trim().is_empty() || self.bucket.trim().is_empty() || self.access_key_id.trim().is_empty() {
            return Err("The MEGA S4 settings are incomplete.".into());
        }
        let secret = load_secret_from_os_keychain(&self.access_key_id)
            .ok()
            .flatten()
            .filter(|s| !s.trim().is_empty())
            .ok_or("The MEGA S4 secret key is missing from the Keychain.")?;
        Ok(Arc::new(S4Client::new(S4Config {
            endpoint: self.endpoint.clone(),
            region: self.region.clone(),
            bucket: self.bucket.clone(),
            access_key_id: self.access_key_id.clone(),
            secret_access_key: secret,
            space_limit_bytes: self.space_limit_gb.max(1) * 1_000_000_000,
            ..Default::default()
        })))
    }
}

/// An open library: the local database, thumbnails and the S4 connection,
/// held under the library lock for as long as this object lives.
#[derive(uniffi::Object)]
pub struct Library {
    store: Arc<Mutex<Store>>,
    thumbs: Arc<ThumbService>,
    connection_problem: Option<String>,
    sync_requests: Mutex<mpsc::Sender<()>>,
    _lock: LibraryLock,
}

#[uniffi::export]
impl Library {
    /// Open the library in `data_dir` (the folder holding `frames.sqlite3`),
    /// with thumbnails in `cache_dir` or where the settings put them. Starts
    /// the background sync and thumbnail fill, which report to `observer`.
    #[uniffi::constructor]
    pub fn open(
        data_dir: String,
        cache_dir: Option<String>,
        observer: Arc<dyn LibraryObserver>,
    ) -> Result<Arc<Self>, FramesError> {
        let dir = PathBuf::from(data_dir);
        let lock = LibraryLock::acquire(&dir)
            .map_err(FramesError::other)?
            .ok_or(FramesError::AlreadyOpen)?;
        let settings = Settings::load(&dir);
        let db_path = dir.join("frames.sqlite3");
        let mut store = Store::open(&db_path)
            .map_err(|e| FramesError::DamagedDatabase { message: e.to_string() })?;
        // The same one-time move to content-addressed assets the Tauri app makes.
        let _ = store.remove_legacy_folder_assets(&dir.join("frames.sqlite3.before-content-addressed.bak"));

        let cache_root = cache_dir.unwrap_or_else(|| settings.cache_location.clone());
        let cache = LocalCache::open(&cache_root, settings.cache_limit_gb.max(1) * 1_000_000_000)
            .map_err(FramesError::other)?;
        let thumbs = ThumbService::new(Arc::new(Mutex::new(cache)), &db_path);
        let s4 = settings.s4();
        thumbs.set_s4(s4.as_ref().ok().cloned());

        let store = Arc::new(Mutex::new(store));
        let (sync_requests, wakeups) = mpsc::channel();
        let library = Arc::new(Self {
            store: Arc::clone(&store),
            thumbs: Arc::clone(&thumbs),
            connection_problem: s4.as_ref().err().cloned(),
            sync_requests: Mutex::new(sync_requests),
            _lock: lock,
        });

        std::thread::Builder::new()
            .name("frames-background".into())
            .spawn(move || {
                qos::run_as(WorkClass::Background, || {
                    std::thread::sleep(STARTUP_QUIET);
                    let ready = Arc::clone(&observer);
                    thumbs.start_backfill(Some(Arc::new(move |id: &str| ready.thumb_ready(id.to_string()))));
                    if let Ok(s4) = s4 {
                        sync_loop(&store, s4.as_ref(), &thumbs, observer.as_ref(), wakeups);
                    }
                })
            })
            .map_err(FramesError::other)?;
        Ok(library)
    }

    /// Why the library isn't syncing with S4, if it isn't.
    pub fn connection_problem(&self) -> Option<String> {
        self.connection_problem.clone()
    }

    /// Sync now instead of waiting for the next round.
    pub fn sync_now(&self) {
        let _ = self.sync_requests.lock().map(|tx| tx.send(()));
    }

    pub fn timeline(&self, filter: TimelineFilter) -> Result<TimelineSnapshot, FramesError> {
        let filter = SearchFilter {
            album_id: filter.album_id,
            kind: filter.videos_only.then_some(AssetKind::Video),
            ..Default::default()
        };
        let store = self.store.lock().map_err(FramesError::other)?;
        let count = store.count_assets(&filter).map_err(FramesError::other)?;
        let mut snapshot = TimelineSnapshot {
            count: 0,
            ids: Vec::with_capacity(count * 64),
            id_ends: Vec::with_capacity(count * 4),
            local_ms: Vec::with_capacity(count * 8),
            flags: Vec::with_capacity(count),
            duration_s: Vec::with_capacity(count * 4),
            thumbhashes: Vec::with_capacity(count * 25),
            thumbhash_ends: Vec::with_capacity(count * 4),
        };
        store
            .for_each_timeline_tile(&filter, |tile| {
                let s = &mut snapshot;
                s.count += 1;
                s.ids.extend_from_slice(tile.id.as_bytes());
                s.id_ends.extend_from_slice(&(s.ids.len() as u32).to_le_bytes());
                s.local_ms.extend_from_slice(&tile.local_ms.to_le_bytes());
                let mut flags = 0;
                if tile.kind == AssetKind::Video {
                    flags |= FLAG_VIDEO;
                }
                if tile.kind == AssetKind::Live {
                    flags |= FLAG_LIVE;
                }
                if tile.date_guessed {
                    flags |= FLAG_DATE_GUESSED;
                }
                if tile.thumb_failed {
                    flags |= FLAG_THUMB_FAILED;
                }
                s.flags.push(flags);
                let seconds = tile.duration_ms.map_or(0, |ms| (ms / 1000) as u32);
                s.duration_s.extend_from_slice(&seconds.to_le_bytes());
                if let Some(hash) = tile.thumbhash {
                    let before = s.thumbhashes.len();
                    if BASE64.decode_vec(hash, &mut s.thumbhashes).is_err() {
                        s.thumbhashes.truncate(before);
                    }
                }
                s.thumbhash_ends.extend_from_slice(&(s.thumbhashes.len() as u32).to_le_bytes());
            })
            .map_err(FramesError::other)?;
        Ok(snapshot)
    }

    pub fn albums(&self) -> Result<Vec<AlbumSummary>, FramesError> {
        let store = self.store.lock().map_err(FramesError::other)?;
        let albums = store.list_albums().map_err(FramesError::other)?;
        Ok(albums
            .into_iter()
            .map(|a| AlbumSummary {
                id: a.id,
                name: a.name,
                created_at_ms: a.created_at.timestamp_millis(),
                asset_count: a.asset_count as u64,
                cover_asset_id: a.cover_asset_id,
            })
            .collect())
    }

    /// Photos and videos in the library, trash excluded.
    pub fn asset_count(&self) -> Result<u64, FramesError> {
        let store = self.store.lock().map_err(FramesError::other)?;
        let count = store.count_assets(&SearchFilter::default()).map_err(FramesError::other)?;
        Ok(count as u64)
    }

    pub fn asset_detail(&self, asset_id: String) -> Result<Option<AssetDetail>, FramesError> {
        let store = self.store.lock().map_err(FramesError::other)?;
        let Some(detail) = store.get_asset(&asset_id).map_err(FramesError::other)? else {
            return Ok(None);
        };
        let r = detail.record;
        let camera = [r.camera_make.as_deref(), r.camera_model.as_deref()]
            .into_iter()
            .flatten()
            .collect::<Vec<_>>()
            .join(" ");
        Ok(Some(AssetDetail {
            id: r.id,
            kind: r.kind.into(),
            filename: r.filename,
            size: r.size,
            taken_utc_ms: r.taken_at.timestamp_millis(),
            tz_offset_minutes: r.tz_offset,
            date_guessed: r.date_guessed,
            width: r.width,
            height: r.height,
            duration_ms: r.duration_ms,
            camera: (!camera.is_empty()).then_some(camera),
            albums: detail.albums.into_iter().map(|a| a.name).collect(),
        }))
    }

    /// The small thumbnail's file, fetched from S4 or made from the local
    /// original if needed. Blocks: call it off the main thread.
    pub fn small_thumb(&self, asset_id: String) -> Option<String> {
        self.thumbs.small(&asset_id).map(path_string)
    }

    /// The preview's file (long edge 1600 px), or the small thumbnail if there
    /// is no preview. Blocks: call it off the main thread.
    pub fn preview(&self, asset_id: String) -> Option<String> {
        self.thumbs.preview(&asset_id).map(path_string)
    }

    /// The original on this computer, if it is still where it was imported from.
    pub fn local_original(&self, asset_id: String) -> Option<String> {
        self.thumbs.local_original(&asset_id).map(path_string)
    }
}

/// Sync on open, then every `SYNC_INTERVAL` or when asked, until the library closes.
fn sync_loop(
    store: &Mutex<Store>,
    s4: &dyn ObjectStore,
    thumbs: &Arc<ThumbService>,
    observer: &dyn LibraryObserver,
    wakeups: mpsc::Receiver<()>,
) {
    loop {
        match sync_once(&mut &*store, s4, Utc::now()) {
            Ok(report) => {
                observer.sync_state(!report.offline);
                if report.batches_pulled > 0 {
                    thumbs.start_backfill(None);
                    observer.library_changed();
                }
            }
            Err(_) => observer.sync_state(false),
        }
        match wakeups.recv_timeout(SYNC_INTERVAL) {
            Ok(()) | Err(mpsc::RecvTimeoutError::Timeout) => {}
            Err(mpsc::RecvTimeoutError::Disconnected) => return,
        }
    }
}

fn path_string(path: PathBuf) -> String {
    path.to_string_lossy().to_string()
}
