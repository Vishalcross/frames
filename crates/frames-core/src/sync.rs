//! Cloud sync engine, end-to-end S4 importer runner, Check Library, Rebuild
//! Database, and Export Originals (spec sections 6, 7, 9, 11).

use std::collections::HashSet;
use std::fs;
use std::path::{Path, PathBuf};

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::cache::LocalCache;
use crate::importer::{prepare_import_plan, PreparedImportAsset, SkippedFile};
use crate::keys::{
    log_key, newest_snapshot, parse_log_key, thumb_preview_key, thumb_small_key, LOG_PREFIX,
    SNAPSHOT_PREFIX,
};
use crate::ops::Batch;
use crate::s4::{
    retry_with_backoff, upload_file_with_resume, ObjectStore, S4Error, SpaceUsage,
};
use crate::store::{
    ImportQueueRow, ImportState, Store, StoreError, DEFAULT_OUTBOX_MAX_ASSETS,
};

#[derive(Debug, Error)]
pub enum SyncError {
    #[error("store error: {0}")]
    Store(#[from] StoreError),
    #[error("s4 error: {0}")]
    S4(#[from] S4Error),
    #[error("json error: {0}")]
    Json(#[from] serde_json::Error),
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),
}

impl From<rusqlite::Error> for SyncError {
    fn from(e: rusqlite::Error) -> Self {
        Self::Store(StoreError::Sqlite(e))
    }
}

pub type Result<T> = std::result::Result<T, SyncError>;

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct SyncReport {
    pub offline: bool,
    pub batches_pushed: usize,
    pub batches_pulled: usize,
    pub snapshot_written: Option<String>,
}

/// Flush all pending local ops in `outbox` to `index/log/<device>/<seq:09>.json`
/// in batches of at most `DEFAULT_OUTBOX_MAX_ASSETS` (~100 assets).
pub fn flush_outbox(
    store: &mut Store,
    s4: &dyn ObjectStore,
    now: DateTime<Utc>,
) -> Result<usize> {
    let mut pushed = 0usize;
    while let Some(pending) = store.peek_outbox_batch(DEFAULT_OUTBOX_MAX_ASSETS, now)? {
        let key = log_key(&pending.batch.device, pending.batch.seq);
        let payload = serde_json::to_vec(&pending.batch)?;
        retry_with_backoff(3, || {
            s4.put_object(&key, &payload, "application/json").map(|_| ())
        })?;
        store.mark_outbox_flushed(&pending)?;
        pushed += 1;
    }
    Ok(pushed)
}

/// Pull and apply any change-log batches in S4 (`index/log/<device>/<seq>.json`)
/// with `seq > store.applied_seq(device)`, in `(device, seq)` order.
pub fn pull_remote_logs(store: &mut Store, s4: &dyn ObjectStore) -> Result<usize> {
    let objects = retry_with_backoff(3, || s4.list_objects(LOG_PREFIX))?;

    let mut to_fetch: Vec<(String, u64, String)> = Vec::new();
    for obj in objects {
        if let Some((dev, seq)) = parse_log_key(&obj.key) {
            let applied = store.applied_seq(dev)?;
            if seq > applied {
                to_fetch.push((dev.to_string(), seq, obj.key));
            }
        }
    }
    // Sort by device then seq ascending so each device's log is applied in seq order
    to_fetch.sort_by(|a, b| (a.0.as_str(), a.1).cmp(&(b.0.as_str(), b.1)));

    let mut pulled = 0usize;
    for (_dev, _seq, key) in to_fetch {
        let Some(bytes) = retry_with_backoff(3, || s4.get_object(&key))? else {
            continue;
        };
        let batch: Batch = serde_json::from_slice(&bytes)?;
        if store.apply_remote_batch(&batch)? {
            pulled += 1;
        }
    }
    Ok(pulled)
}

/// Write a full-state `.json.gz` snapshot to S4 if `store.needs_snapshot(now)` is true.
pub fn write_snapshot_if_needed(
    store: &mut Store,
    s4: &dyn ObjectStore,
    now: DateTime<Utc>,
) -> Result<Option<String>> {
    if !store.needs_snapshot(now)? {
        return Ok(None);
    }
    let (key, gz_bytes) = store.export_snapshot_gz(now)?;
    retry_with_backoff(3, || {
        s4.put_object(&key, &gz_bytes, "application/gzip").map(|_| ())
    })?;
    Ok(Some(key))
}

/// Perform one full sync cycle (push outbox -> pull remote logs -> write snapshot if due).
/// If S4 is unreachable (`S4Error::Offline`), returns `SyncReport { offline: true, .. }`
/// while leaving local changes safely in `outbox` (spec section 11).
pub fn sync_once(
    store: &mut Store,
    s4: &dyn ObjectStore,
    now: DateTime<Utc>,
) -> Result<SyncReport> {
    let batches_pushed = match flush_outbox(store, s4, now) {
        Ok(n) => n,
        Err(SyncError::S4(S4Error::Offline(_))) => {
            return Ok(SyncReport {
                offline: true,
                ..Default::default()
            });
        }
        Err(e) => return Err(e),
    };

    let batches_pulled = match pull_remote_logs(store, s4) {
        Ok(n) => n,
        Err(SyncError::S4(S4Error::Offline(_))) => {
            return Ok(SyncReport {
                offline: true,
                batches_pushed,
                ..Default::default()
            });
        }
        Err(e) => return Err(e),
    };

    let snapshot_written = write_snapshot_if_needed(store, s4, now)?;
    Ok(SyncReport {
        offline: false,
        batches_pushed,
        batches_pulled,
        snapshot_written,
    })
}

/// Rebuild the local database from the newest S4 snapshot plus subsequent change
/// logs (spec section 6 & section 11: "Rebuild deletes it and reloads from the
/// snapshot plus logs").
pub fn rebuild_from_s4(store: &mut Store, s4: &dyn ObjectStore) -> Result<SyncReport> {
    store.reset_for_rebuild()?;

    let snap_objs = retry_with_backoff(3, || s4.list_objects(SNAPSHOT_PREFIX))?;
    let snap_keys: Vec<&str> = snap_objs.iter().map(|o| o.key.as_str()).collect();
    if let Some(latest_key) = newest_snapshot(snap_keys) {
        if let Some(gz_bytes) = retry_with_backoff(3, || s4.get_object(latest_key))? {
            store.import_snapshot_gz(&gz_bytes)?;
        }
    }

    let batches_pulled = pull_remote_logs(store, s4)?;
    Ok(SyncReport {
        offline: false,
        batches_pushed: 0,
        batches_pulled,
        snapshot_written: None,
    })
}

// =============================================================================
// Check Library (Spec Section 9 & 11)
// =============================================================================

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct LibraryCheckReport {
    /// Asset IDs in SQLite whose original (or Live Photo motion file) is missing in S4.
    pub missing_originals: Vec<String>,
    /// S4 keys under `originals/` that are not in the SQLite index.
    pub unknown_s4_keys: Vec<String>,
    /// Incomplete multipart uploads left over from a crash that were tidied/aborted.
    pub tidied_multipart_uploads: usize,
    pub space_usage: Option<SpaceUsage>,
}

/// Compare the S4 bucket listing with the local SQLite index and tidy crashed
/// multipart uploads (spec section 11: "Check library compares the S4 listing
/// with the index: lists missing originals, offers to add unknown files, tidies
/// uploads left over from a crash").
pub fn check_library(store: &Store, s4: &dyn ObjectStore) -> Result<LibraryCheckReport> {
    let all_objects = s4.list_objects("")?;
    let total_used: u64 = all_objects.iter().map(|o| o.size).sum();
    let space_usage = Some(SpaceUsage::compute(
        total_used,
        s4.config().space_limit_bytes,
    ));

    let s4_original_keys: HashSet<String> = all_objects
        .into_iter()
        .filter(|o| o.key.starts_with("originals/"))
        .map(|o| o.key)
        .collect();

    let mut indexed_keys: HashSet<String> = HashSet::new();
    let mut missing_originals: Vec<String> = Vec::new();

    let mut stmt = store.connection().prepare(
        "SELECT id, key, motion_key FROM assets WHERE purged_at IS NULL ORDER BY id ASC",
    )?;
    let rows = stmt.query_map([], |r| {
        Ok((
            r.get::<_, String>(0)?,
            r.get::<_, String>(1)?,
            r.get::<_, Option<String>>(2)?,
        ))
    })?;

    for row in rows {
        let (id, key, motion_key) = row?;
        let mut missing = !s4_original_keys.contains(&key);
        indexed_keys.insert(key);
        if let Some(m_key) = motion_key {
            if !s4_original_keys.contains(&m_key) {
                missing = true;
            }
            indexed_keys.insert(m_key);
        }
        if missing {
            missing_originals.push(id);
        }
    }

    let mut unknown_s4_keys: Vec<String> = s4_original_keys
        .into_iter()
        .filter(|k| !indexed_keys.contains(k))
        .collect();
    unknown_s4_keys.sort();

    // Tidy incomplete multipart uploads not actively tracked in import_queue
    let active_upload_ids: HashSet<String> = store
        .list_import_queue()?
        .into_iter()
        .filter_map(|r| r.upload_id)
        .collect();

    let incomplete = s4.list_multipart_uploads()?;
    let mut tidied = 0usize;
    for up in incomplete {
        if !active_upload_ids.contains(&up.upload_id) {
            s4.abort_multipart_upload(&up.key, &up.upload_id)?;
            tidied += 1;
        }
    }

    Ok(LibraryCheckReport {
        missing_originals,
        unknown_s4_keys,
        tidied_multipart_uploads: tidied,
        space_usage,
    })
}

// =============================================================================
// End-to-End Importer Pipeline (Spec Section 7, Steps 1–7)
// =============================================================================

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ImportRunReport {
    pub imported_count: usize,
    pub duplicates_skipped: usize,
    pub unknown_files: Vec<SkippedFile>,
    pub failed_files: Vec<SkippedFile>,
    pub paused_offline: bool,
    pub stopped_space_full: bool,
    pub space_usage: Option<SpaceUsage>,
}

/// Run the full import pipeline (spec section 7, steps 1–7):
/// 1. Scan & classify
/// 2. Stream SHA-256 & skip duplicates
/// 3. Extract metadata & date taken
/// 4. Pair Live Photos
/// 5. Generate `s.webp` and `m.webp` thumbnails (caching `s.webp` and `m.webp` locally)
/// 6. Check S4 `HEAD` for name clashes, upload original (multipart >64 MB), motion file, and thumbnails
/// 7. Log `asset.add` (and `album.add` if `album_id` is set) to SQLite + `outbox`, and flush batches
pub fn run_import<I, P>(
    roots: I,
    album_id: Option<&str>,
    store: &mut Store,
    s4: &dyn ObjectStore,
    cache: &mut LocalCache,
    now: DateTime<Utc>,
) -> Result<ImportRunReport>
where
    I: IntoIterator<Item = P>,
    P: AsRef<Path>,
{
    let plan = prepare_import_plan(roots, store, now)?;
    let mut report = ImportRunReport {
        duplicates_skipped: plan.duplicates_skipped,
        unknown_files: plan.unknown_files,
        failed_files: plan.unreadable_files,
        ..Default::default()
    };

    // Enqueue all prepared assets into `import_queue` so progress & resume work across restarts
    for item in &plan.assets {
        let path_str = item.primary_path.to_string_lossy().to_string();
        store.enqueue_import(&ImportQueueRow {
            path: path_str,
            size: item.record.size + item.record.motion_size.unwrap_or(0),
            mtime: item.primary_mtime_ms,
            state: ImportState::Hashed,
            asset_id: Some(item.record.id.clone()),
            upload_id: None,
            parts_done: 0,
            album_id: album_id.map(str::to_string),
            error: None,
        })?;
    }

    for mut item in plan.assets {
        let path_str = item.primary_path.to_string_lossy().to_string();
        match upload_and_log_one_asset(&mut item, album_id, store, s4, cache, now) {
            Ok(()) => {
                report.imported_count += 1;
            }
            Err(SyncError::S4(S4Error::Offline(_))) => {
                report.paused_offline = true;
                break;
            }
            Err(SyncError::S4(S4Error::SpaceFull {
                used_bytes,
                limit_bytes,
            })) => {
                report.stopped_space_full = true;
                report.space_usage = Some(SpaceUsage::compute(used_bytes, limit_bytes));
                break;
            }
            Err(err) => {
                let msg = err.to_string();
                store.update_import_state(
                    &path_str,
                    ImportState::Failed,
                    Some(&item.record.id),
                    None,
                    0,
                    Some(&msg),
                )?;
                report.failed_files.push(SkippedFile {
                    path: item.primary_path,
                    reason: msg,
                });
            }
        }
    }

    // Flush outbox if online
    if !report.paused_offline {
        let _ = flush_outbox(store, s4, now);
    }

    Ok(report)
}

fn upload_and_log_one_asset(
    item: &mut PreparedImportAsset,
    album_id: Option<&str>,
    store: &mut Store,
    s4: &dyn ObjectStore,
    cache: &mut LocalCache,
    now: DateTime<Utc>,
) -> Result<()> {
    let path_str = item.primary_path.to_string_lossy().to_string();

    // Step 4b: Name clash check via S4 HEAD (spec section 4)
    let primary_clashed = s4.head_object(&item.record.key)?.is_some();
    let motion_clashed = match item.record.motion_key.as_deref() {
        Some(mk) => s4.head_object(mk)?.is_some(),
        None => false,
    };
    item.apply_name_clash(primary_clashed, motion_clashed);

    // Step 5: Generate thumbnails & store locally
    let thumbs = item.build_thumbnails();
    let _ = cache.put_small_thumb(&item.record.id, &thumbs.small_webp);
    let _ = cache.put_preview(&item.record.id, &thumbs.preview_webp);

    // Step 6: Upload original (with resumable multipart support)
    store.update_import_state(
        &path_str,
        ImportState::Uploading,
        Some(&item.record.id),
        None,
        0,
        None,
    )?;

    upload_file_with_resume(
        s4,
        &item.primary_path,
        &item.record.key,
        "application/octet-stream",
        None,
        0,
        3,
        |uid, parts_done| {
            let _ = store.update_import_state(
                &path_str,
                ImportState::Uploading,
                Some(&item.record.id),
                uid,
                parts_done,
                None,
            );
        },
    )?;

    // Upload Live Photo motion file if present
    if let (Some(ref mov_path), Some(ref mov_key)) = (&item.motion_path, &item.record.motion_key) {
        upload_file_with_resume(
            s4,
            mov_path,
            mov_key,
            "video/quicktime",
            None,
            0,
            3,
            |_, _| {},
        )?;
    }

    // Upload small and preview thumbnails
    let s_key = thumb_small_key(&item.record.id);
    let m_key = thumb_preview_key(&item.record.id);
    retry_with_backoff(3, || {
        s4.put_object(&s_key, &thumbs.small_webp, "image/webp")
            .map(|_| ())
    })?;
    retry_with_backoff(3, || {
        s4.put_object(&m_key, &thumbs.preview_webp, "image/webp")
            .map(|_| ())
    })?;

    store.update_import_state(
        &path_str,
        ImportState::Uploaded,
        Some(&item.record.id),
        None,
        1,
        None,
    )?;

    // Step 7: Only after original and thumbnails are confirmed in S4, log asset.add
    store.add_asset(item.record.clone(), album_id, now)?;
    store.update_import_state(
        &path_str,
        ImportState::Logged,
        Some(&item.record.id),
        None,
        1,
        None,
    )?;

    Ok(())
}

// =============================================================================
// Export Originals to a Local Folder (Spec Section 2, 8, 9)
// =============================================================================

/// Export the original files (and Live Photo motion `.MOV` files) for `asset_ids`
/// into `destination_dir`, reading from `cache` when available or downloading from S4.
pub fn export_originals(
    asset_ids: &[String],
    destination_dir: impl AsRef<Path>,
    store: &Store,
    s4: &dyn ObjectStore,
    cache: &mut LocalCache,
) -> Result<Vec<PathBuf>> {
    let dest = destination_dir.as_ref();
    fs::create_dir_all(dest)?;
    let mut exported = Vec::new();

    for id in asset_ids {
        let Some(detail) = store.get_asset(id)? else {
            continue;
        };
        let rec = &detail.record;
        if let Some(cached_path) =
            cache.get_or_fetch_original(s4, &rec.id, &rec.filename, &rec.key)?
        {
            let target = dest.join(&rec.filename);
            fs::copy(&cached_path, &target)?;
            exported.push(target);
        }
        if let (Some(ref m_id), Some(ref m_key)) = (&rec.motion_id, &rec.motion_key) {
            let mov_name = m_key
                .rsplit('/')
                .next()
                .unwrap_or("motion.MOV")
                .to_string();
            if let Some(cached_mov) =
                cache.get_or_fetch_original(s4, m_id, &mov_name, m_key)?
            {
                let target = dest.join(&mov_name);
                fs::copy(&cached_mov, &target)?;
                exported.push(target);
            }
        }
    }
    Ok(exported)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cache::DEFAULT_CACHE_LIMIT_BYTES;
    use crate::ops::AssetKind;
    use crate::s4::{InMemoryObjectStore, S4Config};
    use chrono::TimeZone;

    struct TempDir(PathBuf);
    impl TempDir {
        fn new() -> Self {
            let p = std::env::temp_dir().join(format!("frames-sync-{}", fastrand::u64(..)));
            fs::create_dir_all(&p).unwrap();
            Self(p)
        }
    }
    impl Drop for TempDir {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    fn t(sec: i64) -> DateTime<Utc> {
        Utc.timestamp_opt(1_730_000_000 + sec, 0).unwrap()
    }

    #[test]
    fn end_to_end_import_sync_two_computers_and_rebuild_from_snapshot() {
        let src_dir = TempDir::new();
        let cache_dir_mac = TempDir::new();
        let cache_dir_win = TempDir::new();
        let export_dir = TempDir::new();

        // Create a Live Photo pair in source folder (read-only source disk)
        fs::write(
            src_dir.0.join("IMG_20240512_143022.JPG"),
            b"live-photo-still-bytes",
        )
        .unwrap();
        fs::write(
            src_dir.0.join("IMG_20240512_143022.MOV"),
            b"live-photo-motion-bytes",
        )
        .unwrap();

        let s4 = InMemoryObjectStore::new(S4Config::default());
        let mut mac_store = Store::open_in_memory("mac-1").unwrap();
        let mut mac_cache = LocalCache::open(&cache_dir_mac.0, DEFAULT_CACHE_LIMIT_BYTES).unwrap();

        mac_store
            .create_album("alb-1".into(), "May 2024".into(), t(1))
            .unwrap();

        let import_rep = run_import(
            [&src_dir.0],
            Some("alb-1"),
            &mut mac_store,
            &s4,
            &mut mac_cache,
            t(10),
        )
        .unwrap();
        assert_eq!(import_rep.imported_count, 1);
        assert_eq!(mac_store.outbox_len().unwrap(), 0); // flushed!

        // Windows PC syncs from the same S4 bucket
        let mut win_store = Store::open_in_memory("win-1").unwrap();
        let mut win_cache = LocalCache::open(&cache_dir_win.0, DEFAULT_CACHE_LIMIT_BYTES).unwrap();
        let sync_rep = sync_once(&mut win_store, &s4, t(20)).unwrap();
        assert!(!sync_rep.offline);
        assert_eq!(sync_rep.batches_pulled, 1);

        let win_timeline = win_store.list_timeline(None, None).unwrap();
        assert_eq!(win_timeline.len(), 1);
        assert_eq!(win_timeline[0].kind, AssetKind::Live);

        // Windows downloads missing small thumbs in background & exports original + MOV
        let ids = vec![win_timeline[0].id.clone()];
        assert_eq!(win_cache.download_missing_small_thumbs(&s4, &ids).unwrap(), 1);
        let exported = export_originals(&ids, &export_dir.0, &win_store, &s4, &mut win_cache).unwrap();
        assert_eq!(exported.len(), 2);

        // Mac writes a snapshot; a brand-new rebuild loads snapshot + logs cleanly
        let (snap_key, gz) = mac_store.export_snapshot_gz(t(30)).unwrap();
        s4.put_object(&snap_key, &gz, "application/gzip").unwrap();

        rebuild_from_s4(&mut win_store, &s4).unwrap();
        assert_eq!(win_store.list_timeline(None, None).unwrap().len(), 1);

        // Check library confirms 0 missing originals and 0 unknown S4 keys
        let check = check_library(&win_store, &s4).unwrap();
        assert!(check.missing_originals.is_empty());
        assert!(check.unknown_s4_keys.is_empty());
    }

    #[test]
    fn offline_mode_queues_in_outbox_and_syncs_when_back_online() {
        let s4 = InMemoryObjectStore::new(S4Config::default());
        let mut mac = Store::open_in_memory("mac-1").unwrap();
        let mut win = Store::open_in_memory("win-1").unwrap();

        s4.set_offline(true);
        mac.create_album("alb-offline".into(), "Offline Edits".into(), t(10))
            .unwrap();
        let rep_offline = sync_once(&mut mac, &s4, t(11)).unwrap();
        assert!(rep_offline.offline);
        assert_eq!(mac.outbox_len().unwrap(), 1);

        // Come back online -> outbox flushes and win receives it
        s4.set_offline(false);
        let rep_online = sync_once(&mut mac, &s4, t(12)).unwrap();
        assert!(!rep_online.offline);
        assert_eq!(rep_online.batches_pushed, 1);
        assert_eq!(mac.outbox_len().unwrap(), 0);

        sync_once(&mut win, &s4, t(13)).unwrap();
        let albums = win.list_albums().unwrap();
        assert_eq!(albums.len(), 1);
        assert_eq!(albums[0].name, "Offline Edits");
    }
}
