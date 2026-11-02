//! The one thumbnail pipeline (spec sections 7, 8 and 10).
//!
//! Every `s.webp` and `m.webp` the UI shows comes through [`ThumbService`],
//! which resolves an asset id in this order:
//!
//! 1. the local cache (`thumbs_s/` for `s.webp`, the LRU for `m.webp`);
//! 2. the original on this computer (`assets.local_path`, or the path the
//!    importer read it from), decoded once into both sizes and cached;
//! 3. `thumbs/<id>/s.webp` or `m.webp` in S4, downloaded and cached.
//!
//! Asset rows are read through the service's own SQLite connections, never
//! the app's `Store` mutex, so a long import or sync cannot stall thumbnails.
//! Concurrent requests for one asset share a single decode or download, and an
//! asset with no thumbnail anywhere is remembered for the session instead of
//! being retried in a loop.

use std::collections::HashSet;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Condvar, Mutex, RwLock};
use std::time::Duration;

use rusqlite::{Connection, OptionalExtension};

use crate::cache::{small_thumb_path_in, LocalCache};
use crate::keys::{thumb_preview_key, thumb_small_key};
use crate::media::{generate_thumbnails, read_metadata, MediaMetadata};
use crate::ops::AssetKind;
use crate::s4::ObjectStore;

/// Called with an asset id when the background backfill makes its `s.webp`
/// available, so the UI can reload that tile.
pub type ThumbReadyCallback = Arc<dyn Fn(&str) + Send + Sync>;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ThumbSize {
    /// `s.webp`, short edge ~300 px. Kept permanently.
    Small,
    /// `m.webp`, long edge ~1600 px. Kept in the size-capped LRU.
    Preview,
}

enum Outcome {
    Ready(PathBuf),
    /// No source has this thumbnail.
    Missing,
    /// S4 could not be reached; worth retrying later.
    Unavailable,
}

/// Background threads that fill in missing `s.webp` files. Kept below the
/// decode slot count so on-screen tiles always get a slot.
const BACKFILL_WORKERS: usize = 2;
const MAX_DOWNLOADS: usize = 6;
const MAX_POOLED_READERS: usize = 8;

/// Where an asset's original lives in S4, for serving it when this computer
/// has no copy.
#[derive(Debug, Clone)]
pub struct OriginalInfo {
    pub kind: AssetKind,
    pub key: String,
    pub filename: String,
    pub size: u64,
}

/// The fields of an `assets` row the pipeline needs.
struct AssetSource {
    kind: AssetKind,
    size: u64,
    local_path: Option<String>,
    duration_ms: Option<u64>,
}

pub struct ThumbService {
    cache: Arc<Mutex<LocalCache>>,
    cache_root: PathBuf,
    db_path: PathBuf,
    readers: Mutex<Vec<Connection>>,
    s4: RwLock<Option<Arc<dyn ObjectStore>>>,
    decode_slots: Slots,
    download_slots: Slots,
    in_flight: Mutex<HashSet<String>>,
    in_flight_done: Condvar,
    missing: Mutex<HashSet<(String, ThumbSize)>>,
    backfill_running: AtomicBool,
    backfill_requested: AtomicBool,
    backfill_pending: AtomicUsize,
}

impl ThumbService {
    pub fn new(cache: Arc<Mutex<LocalCache>>, db_path: impl Into<PathBuf>) -> Arc<Self> {
        let cache_root = cache
            .lock()
            .map(|c| c.root().to_path_buf())
            .unwrap_or_default();
        let cores = std::thread::available_parallelism().map_or(4, |n| n.get());
        Arc::new(Self {
            cache,
            cache_root,
            db_path: db_path.into(),
            readers: Mutex::new(Vec::new()),
            s4: RwLock::new(None),
            decode_slots: Slots::new((cores / 2).clamp(BACKFILL_WORKERS + 1, 4)),
            download_slots: Slots::new(MAX_DOWNLOADS),
            in_flight: Mutex::new(HashSet::new()),
            in_flight_done: Condvar::new(),
            missing: Mutex::new(HashSet::new()),
            backfill_running: AtomicBool::new(false),
            backfill_requested: AtomicBool::new(false),
            backfill_pending: AtomicUsize::new(0),
        })
    }

    /// Root of the local cache; `s.webp` files live under `thumbs_s/`.
    pub fn cache_root(&self) -> &Path {
        &self.cache_root
    }

    /// Swap the S4 client (after settings change). Forgets which assets had
    /// no thumbnail, since a new bucket or key may have them.
    pub fn set_s4(&self, s4: Option<Arc<dyn ObjectStore>>) {
        *self.s4.write().unwrap() = s4;
        self.missing.lock().unwrap().clear();
    }

    pub fn small_thumb_path(&self, id: &str) -> PathBuf {
        small_thumb_path_in(&self.cache_root, id)
    }

    /// `s.webp` for `id`, produced from the local original or S4 if needed.
    pub fn small(&self, id: &str) -> Option<PathBuf> {
        match self.ensure(id, ThumbSize::Small) {
            Outcome::Ready(p) => Some(p),
            Outcome::Missing | Outcome::Unavailable => None,
        }
    }

    /// `m.webp` for `id`, falling back to `s.webp` when no preview exists.
    pub fn preview(&self, id: &str) -> Option<PathBuf> {
        match self.ensure(id, ThumbSize::Preview) {
            Outcome::Ready(p) => Some(p),
            Outcome::Missing | Outcome::Unavailable => self.small(id),
        }
    }

    /// The original on this computer, if its recorded path still holds a file
    /// of the right size: `assets.local_path` first, then the import source.
    pub fn local_original(&self, id: &str) -> Option<PathBuf> {
        let asset = self.asset_source(id)?;
        self.local_original_for(id, &asset)
    }

    /// The S4 key, file name and size of `id`'s original.
    pub fn original_info(&self, id: &str) -> Option<OriginalInfo> {
        if !is_asset_id(id) {
            return None;
        }
        self.with_reader(|c| {
            c.query_row(
                "SELECT kind, key, filename, size FROM assets WHERE id = ?1 AND purged_at IS NULL",
                [id],
                |r| {
                    Ok(OriginalInfo {
                        kind: crate::store::str_to_kind(&r.get::<_, String>(0)?),
                        key: r.get(1)?,
                        filename: r.get(2)?,
                        size: r.get::<_, i64>(3)?.max(0) as u64,
                    })
                },
            )
            .optional()
        })
        .flatten()
        .filter(|info| !info.key.is_empty())
    }

    /// Where a Live Photo's motion file lives in S4, for streaming it when this
    /// computer has no copy.
    pub fn motion_info(&self, id: &str) -> Option<OriginalInfo> {
        let (key, size) = self.motion_row(id)?.1?;
        let filename = key.rsplit('/').next().unwrap_or("motion.MOV").to_string();
        Some(OriginalInfo {
            kind: AssetKind::Video,
            key,
            filename,
            size,
        })
    }

    /// A Live Photo's motion file on this computer: the MOV the importer read,
    /// if it is still there with the recorded size.
    pub fn local_motion(&self, id: &str) -> Option<PathBuf> {
        let (motion_id, remote) = self.motion_row(id)?;
        let size = remote.map(|(_, size)| size).unwrap_or(0);
        let paths: Vec<String> = self
            .with_reader(|c| {
                let mut stmt = c.prepare("SELECT path FROM import_queue WHERE asset_id = ?1")?;
                let rows = stmt.query_map([&motion_id], |r| r.get::<_, String>(0))?;
                rows.collect()
            })
            .unwrap_or_default();
        paths.into_iter().map(PathBuf::from).find(|p| {
            fs::metadata(p)
                .map(|m| m.is_file() && (size == 0 || m.len() == size))
                .unwrap_or(false)
        })
    }

    /// `(motion_id, Some((motion_key, motion_size)))` for a Live Photo.
    #[allow(clippy::type_complexity)]
    fn motion_row(&self, id: &str) -> Option<(String, Option<(String, u64)>)> {
        if !is_asset_id(id) {
            return None;
        }
        self.with_reader(|c| {
            c.query_row(
                "SELECT motion_id, motion_key, motion_size FROM assets
                 WHERE id = ?1 AND kind = 'live' AND purged_at IS NULL AND motion_id IS NOT NULL",
                [id],
                |r| {
                    let motion_id: String = r.get(0)?;
                    let key: Option<String> = r.get(1)?;
                    let size = r.get::<_, Option<i64>>(2)?.unwrap_or(0).max(0) as u64;
                    Ok((motion_id, key.filter(|k| !k.is_empty()).map(|k| (k, size))))
                },
            )
            .optional()
        })
        .flatten()
    }

    /// Assets still waiting for the background backfill.
    pub fn pending_count(&self) -> usize {
        self.backfill_pending.load(Ordering::SeqCst)
    }

    /// Make sure every visible asset has its `s.webp` on this computer, newest
    /// first (spec section 10: "A new computer downloads them in the
    /// background"). Calling it during a run schedules one more pass, so it is
    /// safe to call after every import, sync or settings change.
    pub fn start_backfill(self: &Arc<Self>, on_ready: Option<ThumbReadyCallback>) {
        self.backfill_requested.store(true, Ordering::SeqCst);
        if self.backfill_running.swap(true, Ordering::SeqCst) {
            return;
        }
        let me = Arc::clone(self);
        let spawned = std::thread::Builder::new()
            .name("frames-thumb-backfill".into())
            .spawn(move || {
                while me.backfill_requested.swap(false, Ordering::SeqCst) {
                    me.backfill_pass(on_ready.as_ref());
                }
                me.backfill_running.store(false, Ordering::SeqCst);
                // A request that arrived between the last check and the store
                // above found the flag still set; pick it up now.
                if me.backfill_requested.load(Ordering::SeqCst) {
                    me.start_backfill(on_ready);
                }
            });
        if spawned.is_err() {
            self.backfill_running.store(false, Ordering::SeqCst);
        }
    }

    fn backfill_pass(&self, on_ready: Option<&ThumbReadyCallback>) {
        let ids: Vec<String> = self
            .with_reader(|c| {
                let mut stmt = c.prepare(
                    "SELECT id FROM assets
                     WHERE trashed_at IS NULL AND purged_at IS NULL
                     ORDER BY taken_at DESC, id ASC",
                )?;
                let rows = stmt.query_map([], |r| r.get::<_, String>(0))?;
                rows.collect()
            })
            .unwrap_or_default();
        let todo: Vec<String> = {
            let missing = self.missing.lock().unwrap();
            ids.into_iter()
                .filter(|id| {
                    !missing.contains(&(id.clone(), ThumbSize::Small))
                        && !self.small_thumb_path(id).is_file()
                })
                .collect()
        };
        self.backfill_pending.store(todo.len(), Ordering::SeqCst);

        let queue = Mutex::new(todo.into_iter());
        let offline = AtomicBool::new(false);
        std::thread::scope(|s| {
            for _ in 0..BACKFILL_WORKERS {
                s.spawn(|| loop {
                    if offline.load(Ordering::SeqCst) {
                        break;
                    }
                    let Some(id) = queue.lock().unwrap().next() else {
                        break;
                    };
                    match self.ensure(&id, ThumbSize::Small) {
                        Outcome::Ready(_) => {
                            if let Some(cb) = on_ready {
                                cb(&id);
                            }
                        }
                        Outcome::Missing => {}
                        // Stop this pass; the next sync or reconnect starts another.
                        Outcome::Unavailable => offline.store(true, Ordering::SeqCst),
                    }
                    let _ = self.backfill_pending.try_update(
                        Ordering::SeqCst,
                        Ordering::SeqCst,
                        |n| Some(n.saturating_sub(1)),
                    );
                });
            }
        });
        self.backfill_pending.store(0, Ordering::SeqCst);
    }

    fn cached(&self, id: &str, size: ThumbSize) -> Option<PathBuf> {
        match size {
            ThumbSize::Small => {
                let p = self.small_thumb_path(id);
                p.is_file().then_some(p)
            }
            ThumbSize::Preview => self.cache.lock().ok()?.get_preview_path(id),
        }
    }

    /// Return the cached thumbnail, or produce it with at most one producer per
    /// asset: later callers wait for the first and then re-check the cache.
    fn ensure(&self, id: &str, size: ThumbSize) -> Outcome {
        // Ids become file names; accept only the SHA-256 form (spec section 4).
        if !is_asset_id(id) {
            return Outcome::Missing;
        }
        {
            let mut flight = self.in_flight.lock().unwrap();
            loop {
                if let Some(p) = self.cached(id, size) {
                    return Outcome::Ready(p);
                }
                if self.missing.lock().unwrap().contains(&(id.to_string(), size)) {
                    return Outcome::Missing;
                }
                if flight.insert(id.to_string()) {
                    break;
                }
                flight = self
                    .in_flight_done
                    .wait_timeout(flight, Duration::from_secs(30))
                    .unwrap()
                    .0;
            }
        }

        let outcome = self.produce(id, size);
        if matches!(outcome, Outcome::Missing) {
            self.missing.lock().unwrap().insert((id.to_string(), size));
        }
        self.in_flight.lock().unwrap().remove(id);
        self.in_flight_done.notify_all();
        outcome
    }

    fn produce(&self, id: &str, size: ThumbSize) -> Outcome {
        let Some(asset) = self.asset_source(id) else {
            return Outcome::Missing;
        };

        if let Some(src) = self.local_original_for(id, &asset) {
            if let Some(path) = self.generate_from_local(id, &asset, &src, size) {
                return Outcome::Ready(path);
            }
        }

        let Some(s4) = self.s4.read().unwrap().clone() else {
            return Outcome::Missing;
        };
        let key = match size {
            ThumbSize::Small => thumb_small_key(id),
            ThumbSize::Preview => thumb_preview_key(id),
        };
        let fetched = {
            let _slot = self.download_slots.acquire();
            s4.get_object(&key)
        };
        match fetched {
            Ok(Some(bytes)) if !bytes.is_empty() => match self.store_bytes(id, size, &bytes) {
                Some(p) => Outcome::Ready(p),
                None => Outcome::Missing,
            },
            Ok(_) => Outcome::Missing,
            Err(_) => Outcome::Unavailable,
        }
    }

    /// Decode the local original once, cache both sizes, return the one asked for.
    fn generate_from_local(
        &self,
        id: &str,
        asset: &AssetSource,
        src: &Path,
        size: ThumbSize,
    ) -> Option<PathBuf> {
        let pair = {
            let _slot = self.decode_slots.acquire();
            let meta = if asset.kind == AssetKind::Video {
                MediaMetadata {
                    duration_ms: asset.duration_ms,
                    ..Default::default()
                }
            } else {
                // Header-only read; supplies RAW orientation.
                read_metadata(src).unwrap_or_default()
            };
            generate_thumbnails(src, asset.kind, &meta)
        };
        if pair.thumb_failed {
            return None;
        }
        let small = self.store_bytes(id, ThumbSize::Small, &pair.small_webp)?;
        let preview = self.store_bytes(id, ThumbSize::Preview, &pair.preview_webp)?;
        Some(match size {
            ThumbSize::Small => small,
            ThumbSize::Preview => preview,
        })
    }

    fn store_bytes(&self, id: &str, size: ThumbSize, bytes: &[u8]) -> Option<PathBuf> {
        let mut cache = self.cache.lock().ok()?;
        match size {
            ThumbSize::Small => cache.put_small_thumb(id, bytes).ok(),
            ThumbSize::Preview => cache.put_preview(id, bytes).ok(),
        }
    }

    fn local_original_for(&self, id: &str, asset: &AssetSource) -> Option<PathBuf> {
        // A file at the recorded path with a different size is not this asset.
        let is_this_asset = |p: &Path| {
            fs::metadata(p)
                .map(|m| m.is_file() && (asset.size == 0 || m.len() == asset.size))
                .unwrap_or(false)
        };
        if let Some(lp) = asset.local_path.as_deref().filter(|s| !s.is_empty()) {
            let p = PathBuf::from(lp);
            if is_this_asset(&p) {
                return Some(p);
            }
        }
        let import_paths: Vec<String> = self
            .with_reader(|c| {
                let mut stmt =
                    c.prepare("SELECT path FROM import_queue WHERE asset_id = ?1 ORDER BY mtime DESC")?;
                let rows = stmt.query_map([id], |r| r.get::<_, String>(0))?;
                rows.collect()
            })
            .unwrap_or_default();
        import_paths
            .into_iter()
            .map(PathBuf::from)
            .find(|p| is_this_asset(p))
    }

    fn asset_source(&self, id: &str) -> Option<AssetSource> {
        self.with_reader(|c| {
            c.query_row(
                "SELECT kind, size, local_path, duration_ms FROM assets
                 WHERE id = ?1 AND purged_at IS NULL",
                [id],
                |r| {
                    Ok(AssetSource {
                        kind: crate::store::str_to_kind(&r.get::<_, String>(0)?),
                        size: r.get::<_, i64>(1)?.max(0) as u64,
                        local_path: r.get(2)?,
                        duration_ms: r.get::<_, Option<i64>>(3)?.map(|v| v.max(0) as u64),
                    })
                },
            )
            .optional()
        })
        .flatten()
    }

    /// Run `f` on a pooled read connection. WAL mode lets these read while the
    /// app's `Store` connection writes.
    fn with_reader<T>(&self, f: impl FnOnce(&Connection) -> rusqlite::Result<T>) -> Option<T> {
        let pooled = self.readers.lock().unwrap().pop();
        let conn = match pooled {
            Some(c) => c,
            None => {
                let c = Connection::open(&self.db_path).ok()?;
                c.busy_timeout(Duration::from_secs(5)).ok()?;
                c
            }
        };
        let out = f(&conn).ok();
        let mut pool = self.readers.lock().unwrap();
        if pool.len() < MAX_POOLED_READERS {
            pool.push(conn);
        }
        out
    }
}

/// Lowercase hex SHA-256, the only form an asset id takes (spec section 4).
pub fn is_asset_id(id: &str) -> bool {
    id.len() == 64 && id.bytes().all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f'))
}

/// Counting semaphore for blocking threads.
struct Slots {
    used: Mutex<usize>,
    freed: Condvar,
    max: usize,
}

struct SlotGuard<'a>(&'a Slots);

impl Slots {
    fn new(max: usize) -> Self {
        Self {
            used: Mutex::new(0),
            freed: Condvar::new(),
            max: max.max(1),
        }
    }

    fn acquire(&self) -> SlotGuard<'_> {
        let mut used = self.used.lock().unwrap();
        while *used >= self.max {
            used = self.freed.wait(used).unwrap();
        }
        *used += 1;
        SlotGuard(self)
    }
}

impl Drop for SlotGuard<'_> {
    fn drop(&mut self) {
        *self.0.used.lock().unwrap() -= 1;
        self.0.freed.notify_one();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cache::DEFAULT_CACHE_LIMIT_BYTES;
    use crate::media::parse_metadata_from_bytes;
    use crate::s4::{InMemoryObjectStore, S4Config};
    use crate::store::Store;

    struct Fixture {
        dir: PathBuf,
        store: Store,
        service: Arc<ThumbService>,
    }

    impl Drop for Fixture {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.dir);
        }
    }

    fn fixture() -> Fixture {
        let dir = std::env::temp_dir().join(format!("frames-thumbs-{}", fastrand::u64(..)));
        fs::create_dir_all(&dir).unwrap();
        let db_path = dir.join("frames.sqlite3");
        let store = Store::open(&db_path).unwrap();
        let cache = LocalCache::open(dir.join("cache"), DEFAULT_CACHE_LIMIT_BYTES).unwrap();
        let service = ThumbService::new(Arc::new(Mutex::new(cache)), &db_path);
        Fixture { dir, store, service }
    }

    fn insert_asset(store: &Store, id: &str, size: u64, local_path: Option<&Path>) {
        store
            .connection()
            .execute(
                "INSERT INTO assets (id, kind, filename, size, taken_at, date_at, state_at, local_path)
                 VALUES (?1, 'photo', 'p.jpg', ?2, '2024-05-01T00:00:00.000Z', 's', 's', ?3)",
                rusqlite::params![
                    id,
                    size as i64,
                    local_path.map(|p| p.to_string_lossy().to_string())
                ],
            )
            .unwrap();
    }

    fn write_jpeg(path: &Path, w: u32, h: u32) -> u64 {
        let img = image::RgbImage::from_fn(w, h, |x, y| image::Rgb([(x % 256) as u8, (y % 256) as u8, 90]));
        img.save_with_format(path, image::ImageFormat::Jpeg).unwrap();
        fs::metadata(path).unwrap().len()
    }

    fn id(n: u8) -> String {
        format!("{:064x}", n)
    }

    #[test]
    fn generates_both_sizes_from_the_local_original() {
        let f = fixture();
        let src = f.dir.join("IMG_0001.jpg");
        let size = write_jpeg(&src, 2400, 1600);
        insert_asset(&f.store, &id(1), size, Some(&src));

        let small = f.service.small(&id(1)).expect("small thumbnail");
        let bytes = fs::read(&small).unwrap();
        assert_eq!(&bytes[0..4], b"RIFF");
        let meta = parse_metadata_from_bytes(&bytes, "webp");
        assert_eq!((meta.width, meta.height), (Some(450), Some(300)));
        assert!(bytes.len() < 60_000, "lossy s.webp, got {} bytes", bytes.len());

        // The same decode cached the preview.
        let preview = f.service.preview(&id(1)).expect("preview");
        let meta = parse_metadata_from_bytes(&fs::read(preview).unwrap(), "webp");
        assert_eq!((meta.width, meta.height), (Some(1600), Some(1067)));
    }

    #[test]
    fn ignores_a_local_file_whose_size_does_not_match() {
        let f = fixture();
        let src = f.dir.join("replaced.jpg");
        let size = write_jpeg(&src, 400, 400);
        insert_asset(&f.store, &id(2), size + 1, Some(&src));
        assert!(f.service.local_original(&id(2)).is_none());
        assert!(f.service.small(&id(2)).is_none());
    }

    #[test]
    fn downloads_from_s4_when_there_is_no_local_original() {
        let f = fixture();
        insert_asset(&f.store, &id(3), 10, None);
        let s4 = InMemoryObjectStore::new(S4Config::default());
        s4.put_object(&thumb_small_key(&id(3)), b"RIFF-small", "image/webp").unwrap();
        f.service.set_s4(Some(Arc::new(s4)));

        let small = f.service.small(&id(3)).expect("downloaded");
        assert_eq!(fs::read(small).unwrap(), b"RIFF-small");
        // No m.webp in S4: preview falls back to the small thumbnail.
        assert_eq!(f.service.preview(&id(3)), Some(f.service.small_thumb_path(&id(3))));
    }

    #[test]
    fn remembers_assets_with_no_thumbnail_anywhere() {
        let f = fixture();
        insert_asset(&f.store, &id(4), 10, None);
        let s4 = Arc::new(InMemoryObjectStore::new(S4Config::default()));
        f.service.set_s4(Some(s4.clone()));
        assert!(f.service.small(&id(4)).is_none());

        // Uploaded later in this session: still remembered as missing...
        s4.put_object(&thumb_small_key(&id(4)), b"RIFF", "image/webp").unwrap();
        assert!(f.service.small(&id(4)).is_none());
        // ...until the S4 client is replaced.
        f.service.set_s4(Some(s4));
        assert!(f.service.small(&id(4)).is_some());
    }

    #[test]
    fn rejects_ids_that_are_not_sha256() {
        let f = fixture();
        assert!(f.service.small("../../etc/passwd").is_none());
        assert!(f.service.small("b453d5eb-f191-4ef8-b44c-b087f84cff27").is_none());
    }

    #[test]
    fn backfill_fills_in_every_missing_small_thumbnail() {
        let f = fixture();
        for n in 10..16 {
            let src = f.dir.join(format!("{n}.jpg"));
            let size = write_jpeg(&src, 640, 480);
            insert_asset(&f.store, &id(n), size, Some(&src));
        }
        let ready = Arc::new(Mutex::new(Vec::new()));
        let sink = Arc::clone(&ready);
        f.service.start_backfill(Some(Arc::new(move |id: &str| {
            sink.lock().unwrap().push(id.to_string());
        })));
        for _ in 0..200 {
            if !f.service.backfill_running.load(Ordering::SeqCst) {
                break;
            }
            std::thread::sleep(Duration::from_millis(20));
        }
        assert_eq!(ready.lock().unwrap().len(), 6);
        assert!((10..16).all(|n| f.service.small_thumb_path(&id(n)).is_file()));
        assert_eq!(f.service.pending_count(), 0);
    }

    #[test]
    fn finds_a_live_photos_motion_locally_or_in_s4() {
        let f = fixture();
        let mov = f.dir.join("IMG_0001.MOV");
        fs::write(&mov, b"motion-bytes").unwrap();
        let motion_id = id(41);
        f.store
            .connection()
            .execute(
                "INSERT INTO assets (id, kind, filename, size, taken_at, date_at, state_at,
                                     motion_id, motion_key, motion_size)
                 VALUES (?1, 'live', 'IMG_0001.HEIC', 1, '2024-05-01T00:00:00.000Z', 's', 's',
                         ?2, 'originals/2024/05/IMG_0001.MOV', 12)",
                rusqlite::params![id(40), motion_id],
            )
            .unwrap();
        f.store
            .connection()
            .execute(
                "INSERT INTO import_queue (path, size, mtime, state, asset_id)
                 VALUES (?1, 12, 0, 'logged', ?2)",
                rusqlite::params![mov.to_string_lossy(), motion_id],
            )
            .unwrap();

        assert_eq!(f.service.local_motion(&id(40)), Some(mov.clone()));
        let info = f.service.motion_info(&id(40)).unwrap();
        assert_eq!((info.key.as_str(), info.size), ("originals/2024/05/IMG_0001.MOV", 12));
        assert_eq!(info.filename, "IMG_0001.MOV");

        // Replaced on disk by something else: not this motion clip any more.
        fs::write(&mov, b"different length").unwrap();
        assert!(f.service.local_motion(&id(40)).is_none());
        // Not a Live Photo: no motion at all.
        insert_asset(&f.store, &id(42), 10, None);
        assert!(f.service.motion_info(&id(42)).is_none());
    }
}
