//! Swift bindings for frames-core (frames_swift_spec.md section 5). The only
//! crate the Mac app links. Everything here is a thin wrapper: the logic stays
//! in frames-core, shared with the Windows app.

use std::path::{Path, PathBuf};
use std::sync::mpsc;
use std::sync::{Arc, Mutex, RwLock};
use std::time::{Duration, Instant};

use base64::engine::general_purpose::STANDARD as BASE64;
use base64::Engine;
use chrono::{TimeZone, Utc};
use frames_core::cache::LocalCache;
use frames_core::keys::clash_filename;
use frames_core::lock::LibraryLock;
use frames_core::ops::AssetKind;
use frames_core::qos::{self, WorkClass};
use frames_core::s4::{load_secret_from_os_keychain, InMemoryObjectStore, ObjectStore, S4Client, S4Config};
use frames_core::store::{SearchFilter, Store};
use frames_core::sync::{
    export_originals, flush_outbox, resume_unfinished_imports, sync_once, unfinished_imports, ImportControl,
    ImportRunReport,
};
use frames_core::thumbnails::ThumbService;
use frames_core::trash::{delete_trash_items_now, empty_trash, purge_expired_trash, trash_summary};

mod manage;
mod video;
pub use manage::*;
pub use video::*;

uniffi::setup_scaffolding!();

/// How often the library syncs with S4 (spec section 6).
const SYNC_INTERVAL: Duration = Duration::from_secs(5 * 60);
/// Background work waits this long after opening, so the first screen reads
/// the database and the disk alone (spec 7.1: launch budget).
const STARTUP_QUIET: Duration = Duration::from_secs(3);
/// Quiet time after the last edit before it is uploaded (spec section 6:
/// "local edits flush after a ~2 s debounce").
const EDIT_PUSH_DEBOUNCE: Duration = Duration::from_secs(2);
/// How often items past their 30 days in Recently Deleted are purged.
const PURGE_INTERVAL: Duration = Duration::from_secs(24 * 60 * 60);

#[derive(Debug, thiserror::Error, uniffi::Error)]
pub enum FramesError {
    #[error("Frames is already open in another app. Quit it, then try again.")]
    AlreadyOpen,
    #[error("The library database could not be opened: {message}")]
    DamagedDatabase { message: String },
    #[error("Not connected to MEGA S4.")]
    NotConnected,
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
    /// An import started, moved on, or finished: ask for `import_status`.
    fn import_changed(&self);
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

impl From<MediaKind> for AssetKind {
    fn from(kind: MediaKind) -> Self {
        match kind {
            MediaKind::Photo => Self::Photo,
            MediaKind::Video => Self::Video,
            MediaKind::Live => Self::Live,
        }
    }
}

/// Which assets a timeline holds. All `None` and `false` is the whole
/// library; the search fields combine as the search field's tokens do.
#[derive(Debug, Clone, Default, uniffi::Record)]
pub struct TimelineFilter {
    pub album_id: Option<String>,
    pub kind: Option<MediaKind>,
    /// Recently Deleted instead of the library, newest deletion first.
    pub trash: bool,
    pub text: Option<String>,
    pub year: Option<i32>,
    pub month: Option<u32>,
    pub camera: Option<String>,
    pub file_ext: Option<String>,
}

/// Search suggestions for what has been typed, grouped as in spec section 9.
#[derive(Debug, Clone, uniffi::Record)]
pub struct SearchSuggestions {
    pub albums: Vec<AlbumRef>,
    /// `YYYY-MM`.
    pub dates: Vec<String>,
    pub cameras: Vec<String>,
    /// `photo`, `video`, `live`, or a file extension such as `heic`.
    pub file_types: Vec<String>,
    pub filenames: Vec<String>,
}

#[derive(Debug, Clone, uniffi::Record)]
pub struct AlbumRef {
    pub id: String,
    pub name: String,
}

/// For the "Delete now" and "Empty Recently Deleted" confirmations.
#[derive(Debug, Clone, uniffi::Record)]
pub struct TrashSummary {
    pub count: u64,
    pub bytes: u64,
}

#[derive(Debug, Clone, uniffi::Record)]
pub struct ExportReport {
    pub exported: u32,
    /// Assets whose original is neither on this Mac nor reachable in S4.
    pub missing: u32,
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
    /// Recently Deleted only: u16 per asset, days until it is purged.
    pub days_left: Vec<u8>,
}

impl TimelineSnapshot {
    fn with_capacity(count: usize) -> Self {
        Self {
            count: 0,
            ids: Vec::with_capacity(count * 64),
            id_ends: Vec::with_capacity(count * 4),
            local_ms: Vec::with_capacity(count * 8),
            flags: Vec::with_capacity(count),
            duration_s: Vec::with_capacity(count * 4),
            thumbhashes: Vec::with_capacity(count * 25),
            thumbhash_ends: Vec::with_capacity(count * 4),
            days_left: Vec::new(),
        }
    }

    fn push(&mut self, id: &str, kind: AssetKind, local_ms: i64, date_guessed: bool, duration_ms: Option<u64>, thumb_failed: bool, thumbhash: Option<&str>) {
        self.count += 1;
        self.ids.extend_from_slice(id.as_bytes());
        self.id_ends.extend_from_slice(&(self.ids.len() as u32).to_le_bytes());
        self.local_ms.extend_from_slice(&local_ms.to_le_bytes());
        let mut flags = 0;
        if kind == AssetKind::Video {
            flags |= FLAG_VIDEO;
        }
        if kind == AssetKind::Live {
            flags |= FLAG_LIVE;
        }
        if date_guessed {
            flags |= FLAG_DATE_GUESSED;
        }
        if thumb_failed {
            flags |= FLAG_THUMB_FAILED;
        }
        self.flags.push(flags);
        let seconds = duration_ms.map_or(0, |ms| (ms / 1000) as u32);
        self.duration_s.extend_from_slice(&seconds.to_le_bytes());
        if let Some(hash) = thumbhash {
            let before = self.thumbhashes.len();
            if BASE64.decode_vec(hash, &mut self.thumbhashes).is_err() {
                self.thumbhashes.truncate(before);
            }
        }
        self.thumbhash_ends.extend_from_slice(&(self.thumbhashes.len() as u32).to_le_bytes());
    }
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

/// `settings.json`, in the format the Tauri app reads and writes too.
#[derive(Debug, Clone, serde::Deserialize, serde::Serialize)]
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

    fn save(&self, data_dir: &Path) -> Result<(), FramesError> {
        let json = serde_json::to_string_pretty(self).map_err(FramesError::other)?;
        std::fs::write(data_dir.join("settings.json"), json).map_err(FramesError::other)
    }

    /// The S4 client these settings describe, or why there is none. `secret`
    /// overrides the Keychain, for testing a connection before saving it.
    fn s4(&self, secret: Option<&str>) -> Result<Arc<dyn ObjectStore>, String> {
        if !self.use_real_s4 {
            return Err("Not connected to MEGA S4. Connect in Settings.".into());
        }
        if self.endpoint.trim().is_empty() || self.bucket.trim().is_empty() || self.access_key_id.trim().is_empty() {
            return Err("The MEGA S4 settings are incomplete. Finish them in Settings.".into());
        }
        let secret = secret
            .map(str::to_string)
            .or_else(|| load_secret_from_os_keychain(&self.access_key_id).ok().flatten())
            .filter(|s| !s.trim().is_empty())
            .ok_or("The MEGA S4 secret key is missing. Enter it in Settings.")?;
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

/// The S4 connection, which Settings can change while the library is open.
/// Background work reads it afresh each time it needs it.
#[derive(Default)]
struct Connection {
    s4: Option<Arc<dyn ObjectStore>>,
    problem: Option<String>,
    /// Work Offline: S4 stays configured but nothing reaches it.
    offline: bool,
}

type SharedConnection = Arc<RwLock<Connection>>;

/// S4, unless it isn't configured or the user is working offline.
fn current_s4(connection: &SharedConnection) -> Option<Arc<dyn ObjectStore>> {
    connection.read().ok().filter(|c| !c.offline).and_then(|c| c.s4.clone())
}

/// An open library: the local database, thumbnails and the S4 connection,
/// held under the library lock for as long as this object lives.
#[derive(uniffi::Object)]
pub struct Library {
    data_dir: PathBuf,
    store: Arc<Mutex<Store>>,
    thumbs: Arc<ThumbService>,
    cache: Arc<Mutex<LocalCache>>,
    settings: Mutex<Settings>,
    connection: SharedConnection,
    imports: Imports,
    observer: Arc<dyn LibraryObserver>,
    sync_requests: Mutex<mpsc::Sender<()>>,
    edit_pushes: Mutex<mpsc::Sender<()>>,
    /// Test libraries upload to memory, never to a real bucket.
    in_memory: bool,
    /// Streams for videos the Mac's player can't open directly, started on first use.
    hls: Mutex<Option<Arc<frames_core::hls::HlsServer>>>,
    /// Whether going offline paused the import, so going online resumes it.
    paused_for_offline: Mutex<bool>,
    _lock: LibraryLock,
}

/// The import that is running, if any, and what the last one did.
#[derive(Clone, Default)]
struct Imports {
    control: Arc<ImportControl>,
    last_report: Arc<Mutex<Option<ImportRunReport>>>,
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
        Self::open_with(data_dir, cache_dir, observer, false)
    }

    /// Open a copy of a library for tests: uploads and S4 reads go to an
    /// empty bucket in memory, so nothing reaches a real one.
    #[uniffi::constructor]
    pub fn open_for_tests(
        data_dir: String,
        cache_dir: Option<String>,
        observer: Arc<dyn LibraryObserver>,
    ) -> Result<Arc<Self>, FramesError> {
        Self::open_with(data_dir, cache_dir, observer, true)
    }
}

impl Library {
    fn open_with(
        data_dir: String,
        cache_dir: Option<String>,
        observer: Arc<dyn LibraryObserver>,
        in_memory: bool,
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
        let cache = Arc::new(Mutex::new(cache));
        let thumbs = ThumbService::new(Arc::clone(&cache), &db_path);
        let connection: SharedConnection = Arc::new(RwLock::new(Connection::default()));
        let store = Arc::new(Mutex::new(store));
        let (sync_requests, wakeups) = mpsc::channel();
        let (edit_pushes, edits) = mpsc::channel();
        let library = Arc::new(Self {
            data_dir: dir,
            store: Arc::clone(&store),
            thumbs: Arc::clone(&thumbs),
            cache: Arc::clone(&cache),
            settings: Mutex::new(settings),
            connection: Arc::clone(&connection),
            imports: Imports::default(),
            observer: Arc::clone(&observer),
            sync_requests: Mutex::new(sync_requests),
            edit_pushes: Mutex::new(edit_pushes),
            in_memory,
            hls: Mutex::new(None),
            paused_for_offline: Mutex::new(false),
            _lock: lock,
        });
        library.connect(None);

        let pusher = (Arc::clone(&store), Arc::clone(&connection));
        spawn("frames-edit-push", WorkClass::Utility, move || push_edits_when_quiet(&pusher.0, &pusher.1, edits))?;
        let background = Background {
            store,
            thumbs,
            cache,
            connection,
            imports: library.imports.clone(),
            observer,
        };
        spawn("frames-background", WorkClass::Background, move || background.run(wakeups))?;
        Ok(library)
    }

    /// Connect to S4 as the settings say, or record why not, then sync.
    fn connect(&self, secret: Option<&str>) {
        let result = if self.in_memory {
            Ok(Arc::new(InMemoryObjectStore::new(S4Config::default())) as Arc<dyn ObjectStore>)
        } else {
            self.settings.lock().map_err(|e| e.to_string()).and_then(|s| s.s4(secret))
        };
        if let Ok(mut connection) = self.connection.write() {
            let offline = connection.offline;
            *connection = match result {
                Ok(s4) => Connection { s4: Some(s4), problem: None, offline },
                Err(problem) => Connection { s4: None, problem: Some(problem), offline },
            };
        }
        self.thumbs.set_s4(self.s4());
        self.sync_now();
    }

    fn s4(&self) -> Option<Arc<dyn ObjectStore>> {
        current_s4(&self.connection)
    }
}

#[uniffi::export]
impl Library {
    /// Why the library isn't syncing with S4, if it isn't.
    pub fn connection_problem(&self) -> Option<String> {
        self.connection.read().ok().and_then(|c| c.problem.clone())
    }

    /// Work Offline, or go back online. Offline, nothing reaches S4: no
    /// sync, uploads, downloads or purges, and a running import pauses.
    /// Edits wait on this Mac and upload when you go back online.
    pub fn set_offline(&self, offline: bool) {
        if let Ok(mut connection) = self.connection.write() {
            connection.offline = offline;
        }
        self.thumbs.set_s4(self.s4());
        let mut paused = self.paused_for_offline.lock().unwrap_or_else(|e| e.into_inner());
        if offline && self.imports.control.is_running() && !self.imports.control.is_paused() {
            self.imports.control.pause();
            *paused = true;
        } else if !offline && *paused {
            self.imports.control.resume();
            *paused = false;
        }
        drop(paused);
        self.observer.import_changed();
        if !offline {
            self.sync_now();
        }
    }

    pub fn is_offline(&self) -> bool {
        self.connection.read().is_ok_and(|c| c.offline)
    }

    /// Sync now instead of waiting for the next round.
    pub fn sync_now(&self) {
        let _ = self.sync_requests.lock().map(|tx| tx.send(()));
    }

    pub fn timeline(&self, filter: TimelineFilter) -> Result<TimelineSnapshot, FramesError> {
        let store = self.store.lock().map_err(FramesError::other)?;
        if filter.trash {
            let items = store.list_trash(Utc::now()).map_err(FramesError::other)?;
            let mut snapshot = TimelineSnapshot::with_capacity(items.len());
            for item in items {
                let a = item.asset;
                let local_ms = a.taken_at.timestamp_millis() + i64::from(a.tz_offset.unwrap_or(0)) * 60_000;
                snapshot.push(&a.id, a.kind, local_ms, a.date_guessed, a.duration_ms, a.thumb_failed, None);
                snapshot.days_left.extend_from_slice(&(item.days_left.clamp(0, 999) as u16).to_le_bytes());
            }
            return Ok(snapshot);
        }
        let filter = SearchFilter {
            text: filter.text.filter(|t| !t.trim().is_empty()),
            album_id: filter.album_id,
            year: filter.year,
            month: filter.month,
            camera: filter.camera,
            kind: filter.kind.map(Into::into),
            file_ext: filter.file_ext,
        };
        let count = store.count_assets(&filter).map_err(FramesError::other)?;
        let mut snapshot = TimelineSnapshot::with_capacity(count);
        store
            .for_each_timeline_tile(&filter, |t| {
                snapshot.push(t.id, t.kind, t.local_ms, t.date_guessed, t.duration_ms, t.thumb_failed, t.thumbhash)
            })
            .map_err(FramesError::other)?;
        Ok(snapshot)
    }

    pub fn search_suggestions(&self, text: String) -> Result<SearchSuggestions, FramesError> {
        let store = self.store.lock().map_err(FramesError::other)?;
        let s = store.search_suggestions(&text).map_err(FramesError::other)?;
        Ok(SearchSuggestions {
            albums: s.albums.into_iter().map(|a| AlbumRef { id: a.id, name: a.name }).collect(),
            dates: s.dates,
            cameras: s.cameras,
            file_types: s.file_types,
            filenames: s.filenames,
        })
    }

    // Edits. Each is applied to the database at once and uploaded about 2 s
    // after the last one (spec section 6).

    /// Create an album holding `asset_ids`, and return its id.
    pub fn create_album(&self, name: String, asset_ids: Vec<String>) -> Result<String, FramesError> {
        let name = album_name(&name)?;
        let id = uuid::Uuid::new_v4().to_string();
        self.edit(|store, now| {
            store.create_album(id.clone(), name, now)?;
            if !asset_ids.is_empty() {
                store.add_to_album(&id, asset_ids, now)?;
            }
            Ok(())
        })?;
        Ok(id)
    }

    pub fn rename_album(&self, album_id: String, name: String) -> Result<(), FramesError> {
        let name = album_name(&name)?;
        self.edit(|store, now| store.rename_album(&album_id, name, now))
    }

    /// Delete an album. Its photos stay in the library.
    pub fn delete_album(&self, album_id: String) -> Result<(), FramesError> {
        self.edit(|store, now| store.delete_album(&album_id, now))
    }

    pub fn add_to_album(&self, album_id: String, asset_ids: Vec<String>) -> Result<(), FramesError> {
        self.edit(|store, now| store.add_to_album(&album_id, asset_ids, now))
    }

    pub fn remove_from_album(&self, album_id: String, asset_ids: Vec<String>) -> Result<(), FramesError> {
        self.edit(|store, now| store.remove_from_album(&album_id, asset_ids, now))
    }

    pub fn move_to_album(&self, from_album_id: String, to_album_id: String, asset_ids: Vec<String>) -> Result<(), FramesError> {
        self.edit(|store, now| store.move_to_album(&from_album_id, &to_album_id, asset_ids, now))
    }

    /// Move to Recently Deleted, where items stay 30 days.
    pub fn trash(&self, asset_ids: Vec<String>) -> Result<(), FramesError> {
        self.edit(|store, now| store.trash_assets(asset_ids, now))
    }

    pub fn restore(&self, asset_ids: Vec<String>) -> Result<(), FramesError> {
        self.edit(|store, now| store.restore_assets(asset_ids, now))
    }

    /// Change when a photo was taken (the index only; the file is untouched).
    pub fn set_date(&self, asset_id: String, taken_utc_ms: i64) -> Result<(), FramesError> {
        let taken = Utc.timestamp_millis_opt(taken_utc_ms).single().ok_or_else(|| FramesError::other("invalid date"))?;
        self.edit(|store, now| store.set_asset_date(&asset_id, taken, now))
    }

    pub fn trash_summary(&self) -> Result<TrashSummary, FramesError> {
        let store = self.store.lock().map_err(FramesError::other)?;
        let summary = trash_summary(&store, Utc::now()).map_err(FramesError::other)?;
        Ok(TrashSummary { count: summary.item_count as u64, bytes: summary.total_bytes })
    }

    /// Delete items in Recently Deleted from S4 now. Returns how many went.
    pub fn delete_now(&self, asset_ids: Vec<String>) -> Result<u64, FramesError> {
        let s4 = self.s4().ok_or(FramesError::NotConnected)?;
        let report = delete_trash_items_now(&mut &*self.store, s4.as_ref(), &asset_ids, Utc::now()).map_err(FramesError::other)?;
        self.push_edits_soon();
        Ok(report.purged_asset_ids.len() as u64)
    }

    pub fn empty_trash(&self) -> Result<u64, FramesError> {
        let s4 = self.s4().ok_or(FramesError::NotConnected)?;
        let report = empty_trash(&mut &*self.store, s4.as_ref(), Utc::now()).map_err(FramesError::other)?;
        self.push_edits_soon();
        Ok(report.purged_asset_ids.len() as u64)
    }

    /// Copy originals (and Live Photo movies) into `folder`, from this Mac
    /// when they are still where they were imported from, otherwise from S4.
    pub fn export_originals(&self, asset_ids: Vec<String>, folder: String) -> Result<ExportReport, FramesError> {
        let folder = PathBuf::from(folder);
        std::fs::create_dir_all(&folder).map_err(FramesError::other)?;
        let mut report = ExportReport { exported: 0, missing: 0 };
        let mut from_s4 = Vec::new();
        for id in asset_ids {
            match self.thumbs.local_original(&id) {
                Some(local) => {
                    for file in std::iter::once(local).chain(self.thumbs.local_motion(&id)) {
                        copy_without_clash(&file, &folder, &id)?;
                    }
                    report.exported += 1;
                }
                None => from_s4.push(id),
            }
        }
        if !from_s4.is_empty() {
            match &self.s4() {
                Some(s4) => {
                    let before = from_s4.len() as u32;
                    let files = export_originals(&from_s4, &folder, &mut &*self.store, s4.as_ref(), &self.cache)
                        .map_err(FramesError::other)?;
                    let exported = (files.len() as u32).min(before);
                    report.exported += exported;
                    report.missing += before - exported;
                }
                None => report.missing += from_s4.len() as u32,
            }
        }
        Ok(report)
    }

    /// Write one asset's original to exactly `path` (dragging to Finder).
    pub fn export_original(&self, asset_id: String, path: String) -> Result<(), FramesError> {
        let target = PathBuf::from(path);
        if let Some(local) = self.thumbs.local_original(&asset_id) {
            std::fs::copy(local, &target).map_err(FramesError::other)?;
            return Ok(());
        }
        let s4 = self.s4().ok_or(FramesError::NotConnected)?;
        let staging = self.thumbs.cache_root().join("export").join(&asset_id);
        let files = export_originals(&[asset_id], &staging, &mut &*self.store, s4.as_ref(), &self.cache)
            .map_err(FramesError::other)?;
        let first = files.first().ok_or_else(|| FramesError::other("The original is not in S4."))?;
        std::fs::copy(first, &target).map_err(FramesError::other)?;
        let _ = std::fs::remove_dir_all(&staging);
        Ok(())
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

    /// Original file names, in order; empty where an id is unknown.
    pub fn filenames(&self, asset_ids: Vec<String>) -> Result<Vec<String>, FramesError> {
        let store = self.store.lock().map_err(FramesError::other)?;
        asset_ids
            .iter()
            .map(|id| {
                let detail = store.get_asset(id).map_err(FramesError::other)?;
                Ok(detail.map(|d| d.record.filename).unwrap_or_default())
            })
            .collect()
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

impl Library {
    /// Apply one edit to the database and schedule its upload.
    fn edit(&self, change: impl FnOnce(&mut Store, chrono::DateTime<Utc>) -> frames_core::store::Result<()>) -> Result<(), FramesError> {
        let mut store = self.store.lock().map_err(FramesError::other)?;
        change(&mut store, Utc::now()).map_err(FramesError::other)?;
        drop(store);
        self.push_edits_soon();
        Ok(())
    }

    fn push_edits_soon(&self) {
        let _ = self.edit_pushes.lock().map(|tx| tx.send(()));
    }
}

fn spawn(name: &str, class: WorkClass, work: impl FnOnce() + Send + 'static) -> Result<(), FramesError> {
    std::thread::Builder::new()
        .name(name.into())
        .spawn(move || qos::run_as(class, work))
        .map(|_| ())
        .map_err(FramesError::other)
}

/// Upload local edits a moment after they stop, so a burst of them becomes
/// one log batch. Without S4, or if it can't be reached, they wait for the
/// next sync.
fn push_edits_when_quiet(store: &Mutex<Store>, connection: &SharedConnection, edits: mpsc::Receiver<()>) {
    while edits.recv().is_ok() {
        while edits.recv_timeout(EDIT_PUSH_DEBOUNCE).is_ok() {}
        if let Some(s4) = current_s4(connection) {
            let _ = flush_outbox(&mut &*store, s4.as_ref(), Utc::now());
        }
    }
}

/// What the background thread works with.
struct Background {
    store: Arc<Mutex<Store>>,
    thumbs: Arc<ThumbService>,
    cache: Arc<Mutex<LocalCache>>,
    connection: SharedConnection,
    imports: Imports,
    observer: Arc<dyn LibraryObserver>,
}

impl Background {
    /// Fill missing thumbnails, then sync on open, every `SYNC_INTERVAL` and
    /// whenever asked: push and pull changes, purge Recently Deleted once a
    /// day, and carry on with imports a previous run left unfinished.
    fn run(self, wakeups: mpsc::Receiver<()>) {
        std::thread::sleep(STARTUP_QUIET);
        let ready = Arc::clone(&self.observer);
        self.thumbs.start_backfill(Some(Arc::new(move |id: &str| ready.thumb_ready(id.to_string()))));
        let mut last_purge: Option<Instant> = None;
        loop {
            if let Some(s4) = current_s4(&self.connection) {
                match sync_once(&mut &*self.store, s4.as_ref(), Utc::now()) {
                    Ok(report) => {
                        self.observer.sync_state(!report.offline);
                        if report.batches_pulled > 0 {
                            self.thumbs.start_backfill(None);
                            self.observer.library_changed();
                        }
                        if !report.offline {
                            if last_purge.is_none_or(|t| t.elapsed() >= PURGE_INTERVAL) {
                                let _ = purge_expired_trash(&mut &*self.store, s4.as_ref(), Utc::now());
                                last_purge = Some(Instant::now());
                            }
                            resume_imports(&self.store, &s4, &self.cache, &self.thumbs, &self.imports, &self.observer);
                        }
                    }
                    Err(_) => self.observer.sync_state(false),
                }
            }
            match wakeups.recv_timeout(SYNC_INTERVAL) {
                Ok(()) | Err(mpsc::RecvTimeoutError::Timeout) => {}
                Err(mpsc::RecvTimeoutError::Disconnected) => return,
            }
        }
    }
}

/// Carry on with files an earlier import queued but didn't finish, on a
/// thread of their own, unless an import is already running.
fn resume_imports(
    store: &Arc<Mutex<Store>>,
    s4: &Arc<dyn ObjectStore>,
    cache: &Arc<Mutex<LocalCache>>,
    thumbs: &Arc<ThumbService>,
    imports: &Imports,
    observer: &Arc<dyn LibraryObserver>,
) {
    let waiting = store.lock().ok().and_then(|s| unfinished_imports(&s).ok()).is_some_and(|g| !g.is_empty());
    if !waiting || imports.control.is_running() {
        return;
    }
    let (store, s4, cache, thumbs) = (Arc::clone(store), Arc::clone(s4), Arc::clone(cache), Arc::clone(thumbs));
    let (imports, observer) = (imports.clone(), Arc::clone(observer));
    let _ = spawn("frames-import", WorkClass::Utility, move || {
        let progress = Arc::clone(&observer);
        let report = resume_unfinished_imports(store, s4, cache, Arc::clone(&imports.control), Utc::now(), move |_| {
            progress.import_changed()
        });
        finish_import(report, &imports, &thumbs, observer.as_ref());
    });
}

/// Record how an import ended and refresh what it changed.
fn finish_import(
    report: Result<Option<ImportRunReport>, frames_core::sync::SyncError>,
    imports: &Imports,
    thumbs: &Arc<ThumbService>,
    observer: &dyn LibraryObserver,
) {
    if let (Ok(Some(report)), Ok(mut last)) = (report, imports.last_report.lock()) {
        *last = Some(report);
    }
    thumbs.start_backfill(None);
    observer.library_changed();
    observer.import_changed();
}

fn album_name(name: &str) -> Result<String, FramesError> {
    let trimmed = name.trim();
    if trimmed.is_empty() {
        return Err(FramesError::other("An album needs a name."));
    }
    Ok(trimmed.to_string())
}

/// Copy `file` into `folder`, renamed with the asset's hash if the name is taken.
fn copy_without_clash(file: &Path, folder: &Path, id: &str) -> Result<(), FramesError> {
    let name = file.file_name().and_then(|n| n.to_str()).unwrap_or("original").to_string();
    let target = if folder.join(&name).exists() { folder.join(clash_filename(&name, id)) } else { folder.join(&name) };
    std::fs::copy(file, target).map(|_| ()).map_err(FramesError::other)
}

fn path_string(path: PathBuf) -> String {
    path.to_string_lossy().to_string()
}
