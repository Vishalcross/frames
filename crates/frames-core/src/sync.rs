//! Cloud sync engine, end-to-end S4 importer runner, Check Library, Rebuild
//! Database, and Export Originals (spec sections 6, 7, 9, 11).

use std::collections::{BTreeMap, HashMap, HashSet, VecDeque};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::cache::LocalCache;
use crate::dates::{choose_taken, Candidates};
use crate::importer::{
    pair_live_photos, scan_paths, InspectedFile, PreparedImportAsset, ScannedCandidate,
    SkippedFile,
};
use crate::keys::{
    clash_filename, clean_filename, log_key, newest_snapshot, parse_log_key, thumb_preview_key,
    thumb_small_key, LOG_PREFIX, SNAPSHOT_PREFIX,
};
use crate::media::{
    classify_path, generate_thumbnails, read_metadata, sha256_file, FileClassification,
};
use crate::ops::{AssetKind, AssetRecord, Batch};
use crate::s4::{
    retry_with_backoff_cfg, upload_file_with_resume, upload_file_with_resume_cancel, ObjectStore,
    S4Error, SpaceUsage,
};
use crate::store::{
    ImportQueueRow, ImportState, Store, StoreError, DEFAULT_OUTBOX_MAX_ASSETS,
};

#[derive(Debug, Error)]
pub enum SyncError {
    #[error("an import is already running")]
    AlreadyRunning,
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

#[derive(Debug, Default)]
pub struct ImportControl {
    paused: AtomicBool,
    cancelled: AtomicBool,
    running: AtomicBool,
}

impl ImportControl {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn pause(&self) {
        self.paused.store(true, Ordering::SeqCst);
    }

    pub fn resume(&self) {
        self.paused.store(false, Ordering::SeqCst);
    }

    pub fn cancel(&self) {
        self.cancelled.store(true, Ordering::SeqCst);
        self.paused.store(false, Ordering::SeqCst);
    }

    pub fn is_paused(&self) -> bool {
        self.paused.load(Ordering::SeqCst)
    }

    pub fn is_cancelled(&self) -> bool {
        self.cancelled.load(Ordering::SeqCst)
    }

    pub fn is_running(&self) -> bool {
        self.running.load(Ordering::SeqCst)
    }

    pub fn set_running(&self, val: bool) {
        self.running.store(val, Ordering::SeqCst);
    }

    /// Atomically claim the single-import lock. Returns `false` if another import
    /// is already running.
    pub fn try_start_run(&self) -> bool {
        if self
            .running
            .compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst)
            .is_ok()
        {
            self.paused.store(false, Ordering::SeqCst);
            self.cancelled.store(false, Ordering::SeqCst);
            true
        } else {
            false
        }
    }

    pub fn reset_for_new_run(&self) {
        self.paused.store(false, Ordering::SeqCst);
        self.cancelled.store(false, Ordering::SeqCst);
        self.running.store(true, Ordering::SeqCst);
    }

    pub fn force_reset(&self) {
        self.paused.store(false, Ordering::SeqCst);
        self.cancelled.store(false, Ordering::SeqCst);
        self.running.store(false, Ordering::SeqCst);
    }
}

struct RunningGuard<'a>(&'a ImportControl);

impl Drop for RunningGuard<'_> {
    fn drop(&mut self) {
        self.0.set_running(false);
    }
}

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
    let base_delay = s4.config().retry_base_delay_ms;
    let mut pushed = 0usize;
    while let Some(pending) = store.peek_outbox_batch(DEFAULT_OUTBOX_MAX_ASSETS, now)? {
        let key = log_key(&pending.batch.device, pending.batch.seq);
        let payload = serde_json::to_vec(&pending.batch)?;
        retry_with_backoff_cfg(5, base_delay, || {
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
    let base_delay = s4.config().retry_base_delay_ms;
    let objects = retry_with_backoff_cfg(5, base_delay, || s4.list_objects(LOG_PREFIX))?;

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
        let Some(bytes) = retry_with_backoff_cfg(5, base_delay, || s4.get_object(&key))? else {
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
    let base_delay = s4.config().retry_base_delay_ms;
    let (key, gz_bytes) = store.export_snapshot_gz(now)?;
    retry_with_backoff_cfg(5, base_delay, || {
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

    let base_delay = s4.config().retry_base_delay_ms;
    let snap_objs = retry_with_backoff_cfg(5, base_delay, || s4.list_objects(SNAPSHOT_PREFIX))?;
    let snap_keys: Vec<&str> = snap_objs.iter().map(|o| o.key.as_str()).collect();
    if let Some(latest_key) = newest_snapshot(snap_keys) {
        if let Some(gz_bytes) = retry_with_backoff_cfg(5, base_delay, || s4.get_object(latest_key))? {
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

/// Add unknown `originals/...` S4 objects (discovered by `check_library`) into the
/// library by streaming each object to a temporary file, hashing it, extracting
/// metadata and thumbnails, uploading thumbnails, and logging `asset.add` (spec §11).
pub fn import_unknown_s4_keys(
    store: &mut Store,
    s4: &dyn ObjectStore,
    cache: &mut LocalCache,
    keys: &[String],
    now: DateTime<Utc>,
) -> Result<usize> {
    let mut added = 0usize;
    let tmp_dir = std::env::temp_dir().join(format!("frames-unknown-{}", fastrand::u64(..)));
    fs::create_dir_all(&tmp_dir)?;

    for (idx, key) in keys.iter().enumerate() {
        let raw_name = key.rsplit('/').next().unwrap_or("recovered.jpg");
        let filename = clean_filename(raw_name);
        let is_video = match classify_path(Path::new(&filename)) {
            FileClassification::Photo { .. } => false,
            FileClassification::Video { .. } => true,
            _ => continue,
        };
        let tmp_file = tmp_dir.join(format!("{idx}_{filename}"));
        let downloaded = s4.get_object_to_file(key, &tmp_file)?;
        if !downloaded {
            continue;
        }
        let hash = sha256_file(&tmp_file)?;
        if store.has_active_or_trashed_asset(&hash)? {
            let _ = fs::remove_file(&tmp_file);
            continue;
        }
        let size = fs::metadata(&tmp_file).map(|m| m.len()).unwrap_or(0);
        let meta = read_metadata(&tmp_file).unwrap_or_default();
        let kind = if is_video {
            AssetKind::Video
        } else {
            AssetKind::Photo
        };
        let thumbs = generate_thumbnails(&tmp_file, kind, &meta);
        let _ = cache.put_small_thumb(&hash, &thumbs.small_webp);
        let _ = cache.put_preview(&hash, &thumbs.preview_webp);

        let s_key = thumb_small_key(&hash);
        let m_key = thumb_preview_key(&hash);
        let base_delay = s4.config().retry_base_delay_ms;
        let _ = retry_with_backoff_cfg(5, base_delay, || {
            s4.put_object(&s_key, &thumbs.small_webp, "image/webp")
                .map(|_| ())
        });
        let _ = retry_with_backoff_cfg(5, base_delay, || {
            s4.put_object(&m_key, &thumbs.preview_webp, "image/webp")
                .map(|_| ())
        });

        let taken = choose_taken(&Candidates {
            exif_local: meta.exif_local,
            exif_offset_minutes: meta.exif_offset_minutes,
            quicktime_utc: meta.quicktime_utc,
            filename: &filename,
            modified: now,
            now,
        });

        let record = AssetRecord {
            id: hash,
            kind,
            key: key.clone(),
            filename,
            size,
            taken_at: taken.utc,
            tz_offset: taken.tz_offset_minutes,
            date_guessed: taken.guessed,
            width: meta.width,
            height: meta.height,
            duration_ms: meta.duration_ms,
            container: meta.container,
            video_codec: meta.video_codec,
            audio_codec: meta.audio_codec,
            camera_make: meta.camera_make,
            camera_model: meta.camera_model,
            motion_id: None,
            motion_key: None,
            motion_size: None,
            thumb_failed: thumbs.thumb_failed,
        };
        store.add_asset(record, None, now)?;
        let _ = fs::remove_file(&tmp_file);
        added += 1;
    }

    let _ = fs::remove_dir_all(&tmp_dir);
    if added > 0 {
        let _ = flush_outbox(store, s4, now);
    }
    Ok(added)
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

/// Prepare one folder's scanned candidates lazily (hashing only this folder's files,
/// skipping duplicates in `store` or `seen_hashes`, pairing Live Photos within the folder,
/// and attaching any resumable `upload_id` and `parts_done` from `resume_map`).
fn prepare_folder_assets(
    folder_candidates: Vec<ScannedCandidate>,
    album_id: Option<&str>,
    store: &mut Store,
    seen_hashes: &mut HashSet<String>,
    resume_map: &HashMap<PathBuf, (Option<String>, u32)>,
    report: &mut ImportRunReport,
    now: DateTime<Utc>,
) -> Result<Vec<PreparedImportAsset>> {
    let mut inspected_files: Vec<InspectedFile> = Vec::new();

    for cand in folder_candidates {
        let path_str = cand.path.to_string_lossy().to_string();
        let hash = match sha256_file(&cand.path) {
            Ok(h) => h,
            Err(err) => {
                let msg = format!("unreadable: {err}");
                let _ = store.update_import_state(
                    &path_str,
                    ImportState::Failed,
                    None,
                    None,
                    0,
                    Some(&msg),
                );
                report.failed_files.push(SkippedFile {
                    path: cand.path,
                    reason: msg,
                });
                continue;
            }
        };

        if seen_hashes.contains(&hash) || store.has_active_or_trashed_asset(&hash)? {
            let _ = store.update_import_state(
                &path_str,
                ImportState::Skipped,
                Some(&hash),
                None,
                0,
                None,
            );
            report.duplicates_skipped += 1;
            continue;
        }
        seen_hashes.insert(hash.clone());

        let metadata = match read_metadata(&cand.path) {
            Ok(m) => m,
            Err(err) => {
                let msg = format!("unreadable: {err}");
                let _ = store.update_import_state(
                    &path_str,
                    ImportState::Failed,
                    Some(&hash),
                    None,
                    0,
                    Some(&msg),
                );
                report.failed_files.push(SkippedFile {
                    path: cand.path,
                    reason: msg,
                });
                continue;
            }
        };
        inspected_files.push(InspectedFile {
            candidate: cand,
            sha256: hash,
            metadata,
        });
    }

    let paired_items = pair_live_photos(inspected_files, now);
    let mut prepared = Vec::with_capacity(paired_items.len());
    for mut item in paired_items {
        if let Some((existing_uid, parts_done)) = resume_map.get(&item.primary_path) {
            item.existing_upload_id = existing_uid.clone();
            item.parts_already_done = *parts_done;
        }
        // The companion MOV travels with its photo and is logged with it.
        set_motion_import_state(store, &item, ImportState::Hashed);
        let path_str = item.primary_path.to_string_lossy().to_string();
        let state = if item.existing_upload_id.is_some() {
            ImportState::Uploading
        } else {
            ImportState::Hashed
        };
        store.enqueue_import(&ImportQueueRow {
            path: path_str,
            size: item.record.size,
            mtime: item.primary_mtime_ms,
            state,
            asset_id: Some(item.record.id.clone()),
            upload_id: item.existing_upload_id.clone(),
            parts_done: item.parts_already_done,
            album_id: album_id.map(str::to_string),
            error: None,
        })?;
        prepared.push(item);
    }

    Ok(prepared)
}

/// Run the full import pipeline (spec section 7, steps 1–7) lazily folder-by-folder,
/// honouring existing `import_queue` state so interrupted imports resume without
/// re-hashing or re-uploading completed parts.
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
    let scan = scan_paths(roots);
    let mut report = ImportRunReport {
        unknown_files: scan.unknown_files,
        failed_files: scan.unreadable_files,
        ..Default::default()
    };

    let mut by_folder: BTreeMap<PathBuf, Vec<ScannedCandidate>> = BTreeMap::new();
    let mut resume_map: HashMap<PathBuf, (Option<String>, u32)> = HashMap::new();

    for cand in scan.candidates {
        let path_str = cand.path.to_string_lossy().to_string();
        let row = store.reconcile_import_candidate(&path_str, cand.size, cand.mtime_ms, album_id)?;
        match row.state {
            ImportState::Logged => {
                // Already imported and logged in a prior run
                continue;
            }
            ImportState::Skipped => {
                report.duplicates_skipped += 1;
                continue;
            }
            _ => {
                if row.upload_id.is_some() || row.parts_done > 0 {
                    resume_map.insert(cand.path.clone(), (row.upload_id, row.parts_done));
                }
                let parent = cand
                    .path
                    .parent()
                    .map(Path::to_path_buf)
                    .unwrap_or_default();
                by_folder.entry(parent).or_default().push(cand);
            }
        }
    }

    let mut seen_hashes: HashSet<String> = HashSet::new();

    'folders: for (_folder, folder_candidates) in by_folder {
        let assets = prepare_folder_assets(
            folder_candidates,
            album_id,
            store,
            &mut seen_hashes,
            &resume_map,
            &mut report,
            now,
        )?;

        for mut item in assets {
            let path_str = item.primary_path.to_string_lossy().to_string();
            match upload_and_log_one_asset(&mut item, album_id, store, s4, cache, now) {
                Ok(()) => {
                    report.imported_count += 1;
                }
                Err(SyncError::S4(S4Error::Offline(_))) => {
                    report.paused_offline = true;
                    break 'folders;
                }
                Err(SyncError::S4(S4Error::SpaceFull {
                    used_bytes,
                    limit_bytes,
                })) => {
                    report.stopped_space_full = true;
                    report.space_usage = Some(SpaceUsage::compute(used_bytes, limit_bytes));
                    break 'folders;
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
    }

    // Flush outbox if online
    if !report.paused_offline {
        let _ = flush_outbox(store, s4, now);
    }

    Ok(report)
}

/// Split a folder's scanned candidates into stem-aligned batches of ~32 files so
/// large folders (e.g. 850+ files / 10 GB) begin uploading within ~1 second instead
/// of waiting for all 850 files to be hashed upfront, while keeping same-stem
/// Live Photo pairs (`IMG_1234.HEIC` + `IMG_1234.MOV`) in the same batch.
fn split_folder_into_stem_batches(mut candidates: Vec<ScannedCandidate>) -> Vec<Vec<ScannedCandidate>> {
    if candidates.len() <= 36 {
        return vec![candidates];
    }
    candidates.sort_by(|a, b| {
        let sa = a
            .path
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("")
            .to_ascii_lowercase();
        let sb = b
            .path
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("")
            .to_ascii_lowercase();
        sa.cmp(&sb).then_with(|| a.path.cmp(&b.path))
    });

    let mut batches: Vec<Vec<ScannedCandidate>> = Vec::new();
    let mut current: Vec<ScannedCandidate> = Vec::with_capacity(36);
    let mut last_stem = String::new();

    for cand in candidates {
        let stem = cand
            .path
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("")
            .to_ascii_lowercase();
        if current.len() >= 32 && stem != last_stem {
            batches.push(std::mem::take(&mut current));
        }
        last_stem = stem;
        current.push(cand);
    }
    if !current.is_empty() {
        batches.push(current);
    }
    batches
}

/// Prepare a batch of scanned candidates in parallel across up to 4 worker threads
/// WITHOUT holding `store.lock()` across `sha256_file` or `read_metadata` (`ffprobe`),
/// firing `on_progress` as each file is hashed.
#[allow(clippy::too_many_arguments)]
fn prepare_folder_assets_concurrent<F>(
    folder_candidates: Vec<ScannedCandidate>,
    album_id: Option<&str>,
    store: &Arc<Mutex<Store>>,
    control: &ImportControl,
    seen_hashes: &mut HashSet<String>,
    resume_map: &HashMap<PathBuf, (Option<String>, u32)>,
    report: &mut ImportRunReport,
    now: DateTime<Utc>,
    on_progress: &F,
) -> Result<Vec<PreparedImportAsset>>
where
    F: Fn(&ImportRunReport) + Send + Sync,
{
    let worker_count = folder_candidates.len().clamp(1, 4);
    let cand_queue = Mutex::new(VecDeque::from(folder_candidates));
    let outcomes: Mutex<Vec<std::result::Result<InspectedFile, (PathBuf, String)>>> =
        Mutex::new(Vec::new());
    let report_snapshot = report.clone();

    std::thread::scope(|s| {
        for _ in 0..worker_count {
            let q_ref = &cand_queue;
            let out_ref = &outcomes;
            let store_ref = Arc::clone(store);
            let snap_ref = &report_snapshot;
            s.spawn(move || loop {
                if control.is_cancelled() {
                    break;
                }
                while control.is_paused() && !control.is_cancelled() {
                    std::thread::sleep(Duration::from_millis(100));
                }
                if control.is_cancelled() {
                    break;
                }
                let next_cand = {
                    let mut q = q_ref.lock().unwrap();
                    q.pop_front()
                };
                let Some(cand) = next_cand else {
                    break;
                };
                let path_str = cand.path.to_string_lossy().to_string();
                let hash = match sha256_file(&cand.path) {
                    Ok(h) => h,
                    Err(err) => {
                        let msg = format!("unreadable: {err}");
                        if let Ok(mut st) = store_ref.lock() {
                            let _ = st.update_import_state(
                                &path_str,
                                ImportState::Failed,
                                None,
                                None,
                                0,
                                Some(&msg),
                            );
                        }
                        on_progress(snap_ref);
                        out_ref.lock().unwrap().push(Err((cand.path, msg)));
                        continue;
                    }
                };

                let metadata = match read_metadata(&cand.path) {
                    Ok(m) => m,
                    Err(err) => {
                        let msg = format!("unreadable: {err}");
                        if let Ok(mut st) = store_ref.lock() {
                            let _ = st.update_import_state(
                                &path_str,
                                ImportState::Failed,
                                Some(&hash),
                                None,
                                0,
                                Some(&msg),
                            );
                        }
                        on_progress(snap_ref);
                        out_ref.lock().unwrap().push(Err((cand.path, msg)));
                        continue;
                    }
                };

                if let Ok(mut st) = store_ref.lock() {
                    let _ = st.update_import_state(
                        &path_str,
                        ImportState::Hashed,
                        Some(&hash),
                        None,
                        0,
                        None,
                    );
                }
                on_progress(snap_ref);

                out_ref.lock().unwrap().push(Ok(InspectedFile {
                    candidate: cand,
                    sha256: hash,
                    metadata,
                }));
            });
        }
    });

    if control.is_cancelled() {
        return Ok(Vec::new());
    }

    let mut inspected_files: Vec<InspectedFile> = Vec::new();
    for outcome in outcomes.into_inner().unwrap() {
        match outcome {
            Err((path, reason)) => {
                report.failed_files.push(SkippedFile { path, reason });
            }
            Ok(insp) => {
                let path_str = insp.candidate.path.to_string_lossy().to_string();
                let is_dup = if seen_hashes.contains(&insp.sha256) {
                    true
                } else {
                    let st = store.lock().unwrap();
                    st.has_active_or_trashed_asset(&insp.sha256)?
                };
                if is_dup {
                    if let Ok(mut st) = store.lock() {
                        let _ = st.update_import_state(
                            &path_str,
                            ImportState::Skipped,
                            Some(&insp.sha256),
                            None,
                            0,
                            None,
                        );
                    }
                    report.duplicates_skipped += 1;
                    continue;
                }
                seen_hashes.insert(insp.sha256.clone());
                inspected_files.push(insp);
            }
        }
    }

    let paired_items = pair_live_photos(inspected_files, now);
    let mut prepared = Vec::with_capacity(paired_items.len());
    {
        let mut st = store.lock().unwrap();
        for mut item in paired_items {
            if let Some((existing_uid, parts_done)) = resume_map.get(&item.primary_path) {
                item.existing_upload_id = existing_uid.clone();
                item.parts_already_done = *parts_done;
            }
            let path_str = item.primary_path.to_string_lossy().to_string();
            let state = if item.existing_upload_id.is_some() {
                ImportState::Uploading
            } else {
                ImportState::Hashed
            };
            // The companion MOV travels with its photo and is logged with it.
            set_motion_import_state(&mut st, &item, ImportState::Hashed);
            st.enqueue_import(&ImportQueueRow {
                path: path_str,
                size: item.record.size,
                mtime: item.primary_mtime_ms,
                state,
                asset_id: Some(item.record.id.clone()),
                upload_id: item.existing_upload_id.clone(),
                parts_done: item.parts_already_done,
                album_id: album_id.map(str::to_string),
                error: None,
            })?;
            prepared.push(item);
        }
    }
    on_progress(report);

    Ok(prepared)
}

/// Concurrent background import runner (work-stealing 4-worker pool per stem batch)
/// with fine-grained `Mutex` locking on `Store` and `LocalCache` so UI timeline reads
/// never stall during hashing or uploading, plus single-run enforcement and immediate
/// pause/resume/cancel via `ImportControl`.
#[allow(clippy::too_many_arguments)]
pub fn run_import_concurrent<I, P, F>(
    roots: I,
    album_id: Option<&str>,
    store: Arc<Mutex<Store>>,
    s4: Arc<dyn ObjectStore>,
    cache: Arc<Mutex<LocalCache>>,
    control: Arc<ImportControl>,
    now: DateTime<Utc>,
    on_progress: F,
) -> Result<ImportRunReport>
where
    I: IntoIterator<Item = P>,
    P: AsRef<Path>,
    F: Fn(&ImportRunReport) + Send + Sync,
{
    if !control.try_start_run() {
        return Err(SyncError::AlreadyRunning);
    }
    let _running_guard = RunningGuard(&control);

    let scan = scan_paths(roots);
    let mut report = ImportRunReport {
        unknown_files: scan.unknown_files,
        failed_files: scan.unreadable_files,
        ..Default::default()
    };

    let mut by_folder: BTreeMap<PathBuf, Vec<ScannedCandidate>> = BTreeMap::new();
    let mut resume_map: HashMap<PathBuf, (Option<String>, u32)> = HashMap::new();

    // Batch-reconcile all scanned candidates in a single SQLite transaction
    let batch_tuples: Vec<(String, u64, i64)> = scan
        .candidates
        .iter()
        .map(|c| (c.path.to_string_lossy().to_string(), c.size, c.mtime_ms))
        .collect();
    let reconciled_rows = {
        let mut st = store.lock().unwrap();
        st.reconcile_import_candidates_batch(&batch_tuples, album_id)?
    };

    for (cand, row) in scan.candidates.into_iter().zip(reconciled_rows) {
        if control.is_cancelled() {
            break;
        }
        match row.state {
            ImportState::Logged => continue,
            ImportState::Skipped => {
                report.duplicates_skipped += 1;
                continue;
            }
            _ => {
                if row.upload_id.is_some() || row.parts_done > 0 {
                    resume_map.insert(cand.path.clone(), (row.upload_id, row.parts_done));
                }
                let parent = cand
                    .path
                    .parent()
                    .map(Path::to_path_buf)
                    .unwrap_or_default();
                by_folder.entry(parent).or_default().push(cand);
            }
        }
    }
    on_progress(&report);

    let mut seen_hashes: HashSet<String> = HashSet::new();

    'folders: for (_folder, folder_candidates) in by_folder {
        let batches = split_folder_into_stem_batches(folder_candidates);
        for batch_candidates in batches {
            if control.is_cancelled() {
                break 'folders;
            }
            while control.is_paused() && !control.is_cancelled() {
                std::thread::sleep(Duration::from_millis(100));
            }
            if control.is_cancelled() {
                break 'folders;
            }

            let assets = prepare_folder_assets_concurrent(
                batch_candidates,
                album_id,
                &store,
                &control,
                &mut seen_hashes,
                &resume_map,
                &mut report,
                now,
                &on_progress,
            )?;

            if assets.is_empty() || control.is_cancelled() {
                if control.is_cancelled() {
                    break 'folders;
                }
                continue;
            }

            // Work-stealing pool of up to 10 concurrent upload workers so fast photo
            // uploads never stall waiting for a slow video upload in the same batch.
            let worker_count = assets.len().clamp(1, 10);
            let work_queue = Mutex::new(VecDeque::from(assets));
            let shared_report = Mutex::new(report);
            let stop_folder_loop = AtomicBool::new(false);

            std::thread::scope(|s| {
                let mut handles = Vec::with_capacity(worker_count);
                for _ in 0..worker_count {
                    let queue_ref = &work_queue;
                    let report_ref = &shared_report;
                    let stop_ref = &stop_folder_loop;
                    let store_ref = Arc::clone(&store);
                    let s4_ref = Arc::clone(&s4);
                    let cache_ref = Arc::clone(&cache);
                    let control_ref = Arc::clone(&control);
                    let alb = album_id.map(str::to_string);
                    let progress_cb = &on_progress;

                    handles.push(s.spawn(move || loop {
                        if control_ref.is_cancelled() || stop_ref.load(Ordering::SeqCst) {
                            break;
                        }
                        while control_ref.is_paused() && !control_ref.is_cancelled() {
                            std::thread::sleep(Duration::from_millis(100));
                        }
                        if control_ref.is_cancelled() || stop_ref.load(Ordering::SeqCst) {
                            break;
                        }

                        let next_item = {
                            let mut q = queue_ref.lock().unwrap();
                            q.pop_front()
                        };
                        let Some(mut item) = next_item else {
                            break;
                        };

                        let res = upload_and_log_concurrent(
                            &mut item,
                            alb.as_deref(),
                            &store_ref,
                            s4_ref.as_ref(),
                            &cache_ref,
                            &control_ref,
                            now,
                            || {
                                if let Ok(rep) = report_ref.lock() {
                                    progress_cb(&rep);
                                }
                            },
                        );

                        match res {
                            Ok(()) => {
                                if let Ok(mut rep) = report_ref.lock() {
                                    rep.imported_count += 1;
                                    progress_cb(&rep);
                                }
                            }
                            Err(SyncError::S4(S4Error::Cancelled)) => {
                                stop_ref.store(true, Ordering::SeqCst);
                                break;
                            }
                            Err(SyncError::S4(S4Error::Offline(_))) => {
                                if let Ok(mut rep) = report_ref.lock() {
                                    rep.paused_offline = true;
                                    progress_cb(&rep);
                                }
                                stop_ref.store(true, Ordering::SeqCst);
                                break;
                            }
                            Err(SyncError::S4(S4Error::SpaceFull {
                                used_bytes,
                                limit_bytes,
                            })) => {
                                if let Ok(mut rep) = report_ref.lock() {
                                    rep.stopped_space_full = true;
                                    rep.space_usage =
                                        Some(SpaceUsage::compute(used_bytes, limit_bytes));
                                    progress_cb(&rep);
                                }
                                stop_ref.store(true, Ordering::SeqCst);
                                break;
                            }
                            Err(err) => {
                                let msg = err.to_string();
                                let path_str = item.primary_path.to_string_lossy().to_string();
                                if let Ok(mut st) = store_ref.lock() {
                                    let _ = st.update_import_state(
                                        &path_str,
                                        ImportState::Failed,
                                        Some(&item.record.id),
                                        None,
                                        0,
                                        Some(&msg),
                                    );
                                }
                                if let Ok(mut rep) = report_ref.lock() {
                                    rep.failed_files.push(SkippedFile {
                                        path: item.primary_path,
                                        reason: msg,
                                    });
                                    progress_cb(&rep);
                                }
                            }
                        }
                    }));
                }
                for h in handles {
                    let _ = h.join();
                }
            });

            report = shared_report.into_inner().unwrap();

            if !report.paused_offline && !control.is_cancelled() {
                if let Ok(mut st) = store.lock() {
                    let _ = flush_outbox(&mut st, s4.as_ref(), now);
                }
            }
            if stop_folder_loop.load(Ordering::SeqCst) || control.is_cancelled() {
                break 'folders;
            }
        }
    }

    if !report.paused_offline && !control.is_cancelled() {
        if let Ok(mut st) = store.lock() {
            let _ = flush_outbox(&mut st, s4.as_ref(), now);
        }
    }
    if control.is_cancelled() {
        if let Ok(mut st) = store.lock() {
            let _ = st.cancel_unfinished_imports();
        }
    }
    on_progress(&report);
    Ok(report)
}

/// Largest untagged object compared byte for byte; bigger ones count as a
/// clash rather than download the whole file to check.
const UNTAGGED_VERIFY_MAX_BYTES: u64 = 256 * 1024 * 1024;

/// What is already at `key`: nothing (`None`), this very file (`Some(true)`)
/// or a different file (`Some(false)`). Uploads carry their SHA-256 as a tag;
/// objects written before tags existed, or by another S3 tool, are compared
/// by size and then by content.
fn object_at_key(s4: &dyn ObjectStore, key: &str, local: &Path, sha256: &str) -> Result<Option<bool>> {
    let Some(meta) = s4.head_object(key)? else {
        return Ok(None);
    };
    if let Some(tag) = meta.content_sha256 {
        return Ok(Some(tag == sha256));
    }
    let local_size = std::fs::metadata(local)?.len();
    if meta.size != local_size || local_size > UNTAGGED_VERIFY_MAX_BYTES {
        return Ok(Some(false));
    }
    Ok(s4
        .get_object(key)?
        .map(|bytes| crate::media::sha256_bytes(&bytes) == sha256))
}

/// Settle where this asset's files go and which still need uploading (spec
/// section 4 name clash). An object at the planned key may be this very file,
/// left by an interrupted run; otherwise it is another file with the same name
/// and this one moves to `<name>-<hash8>.<ext>`. Returns
/// `(upload_primary, upload_motion)`.
fn plan_original_uploads(item: &mut PreparedImportAsset, s4: &dyn ObjectStore) -> Result<(bool, bool)> {
    let check = |item: &PreparedImportAsset| -> Result<(Option<bool>, Option<bool>)> {
        let primary = object_at_key(s4, &item.record.key, &item.primary_path, &item.record.id)?;
        let motion = match (&item.record.motion_key, &item.motion_path, &item.record.motion_id) {
            (Some(key), Some(path), Some(sha)) => object_at_key(s4, key, path, sha)?,
            _ => None,
        };
        Ok((primary, motion))
    };

    let (mut primary, mut motion) = check(item)?;
    let primary_clash = primary == Some(false);
    let motion_clash = motion == Some(false);
    if primary_clash || motion_clash {
        item.apply_name_clash(primary_clash, motion_clash);
        // The renamed keys may hold this file from an interrupted run too.
        (primary, motion) = check(item)?;
    }
    Ok((
        primary != Some(true),
        item.record.motion_key.is_some() && motion != Some(true),
    ))
}

/// Mark a Live Photo's MOV with the same import state as its photo, so an
/// interrupted pair resumes as a pair.
fn set_motion_import_state(store: &mut Store, item: &PreparedImportAsset, state: ImportState) {
    if let Some(ref mov_path) = item.motion_path {
        let _ = store.update_import_state(
            &mov_path.to_string_lossy(),
            state,
            item.record.motion_id.as_deref(),
            None,
            0,
            None,
        );
    }
}

/// Files an earlier import left unfinished, grouped by the album they were
/// going into. Files that are no longer on disk (an unplugged drive) stay
/// queued for the next time they appear.
pub fn unfinished_imports(store: &Store) -> Result<Vec<(Option<String>, Vec<PathBuf>)>> {
    let mut groups: BTreeMap<Option<String>, Vec<PathBuf>> = BTreeMap::new();
    for (path, album_id) in store.unfinished_import_rows()? {
        let path = PathBuf::from(path);
        if path.is_file() {
            groups.entry(album_id).or_default().push(path);
        }
    }
    Ok(groups.into_iter().collect())
}

/// Resume every unfinished import (spec section 7: "Unplugging the disk,
/// losing the network or quitting pauses it; the next run resumes").
/// Returns the report of the last run, or `None` if there was nothing to do.
#[allow(clippy::too_many_arguments)]
pub fn resume_unfinished_imports<F>(
    store: Arc<Mutex<Store>>,
    s4: Arc<dyn ObjectStore>,
    cache: Arc<Mutex<LocalCache>>,
    control: Arc<ImportControl>,
    now: DateTime<Utc>,
    on_progress: F,
) -> Result<Option<ImportRunReport>>
where
    F: Fn(&ImportRunReport) + Send + Sync,
{
    let groups = unfinished_imports(&store.lock().unwrap())?;
    let mut last = None;
    for (album_id, paths) in groups {
        let rep = run_import_concurrent(
            paths,
            album_id.as_deref(),
            Arc::clone(&store),
            Arc::clone(&s4),
            Arc::clone(&cache),
            Arc::clone(&control),
            now,
            &on_progress,
        )?;
        let stop = rep.paused_offline || rep.stopped_space_full || control.is_cancelled();
        last = Some(rep);
        if stop {
            break;
        }
    }
    Ok(last)
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
    let base_delay = s4.config().retry_base_delay_ms;

    // Step 4b: Name clash check via S4 HEAD (spec section 4)
    let (upload_primary, upload_motion) = plan_original_uploads(item, s4)?;

    // Step 5: Generate thumbnails & store locally
    let thumbs = item.build_thumbnails();
    let _ = cache.put_small_thumb(&item.record.id, &thumbs.small_webp);
    let _ = cache.put_preview(&item.record.id, &thumbs.preview_webp);

    // Step 6: Upload original (with resumable multipart support)
    let initial_uid = item.existing_upload_id.clone();
    let initial_parts = item.parts_already_done;
    store.update_import_state(
        &path_str,
        ImportState::Uploading,
        Some(&item.record.id),
        initial_uid.as_deref(),
        initial_parts,
        None,
    )?;

    if upload_primary {
        upload_file_with_resume(
            s4,
            &item.primary_path,
            &item.record.key,
            "application/octet-stream",
            Some(&item.record.id),
            initial_uid.as_deref(),
            initial_parts,
            5,
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
    }

    // Upload Live Photo motion file if present
    if let (true, Some(mov_path), Some(mov_key)) =
        (upload_motion, &item.motion_path, &item.record.motion_key)
    {
        upload_file_with_resume(
            s4,
            mov_path,
            mov_key,
            "video/quicktime",
            item.record.motion_id.as_deref(),
            None,
            0,
            5,
            |_, _| {},
        )?;
    }

    // Upload small and preview thumbnails
    let s_key = thumb_small_key(&item.record.id);
    let m_key = thumb_preview_key(&item.record.id);
    retry_with_backoff_cfg(5, base_delay, || {
        s4.put_object(&s_key, &thumbs.small_webp, "image/webp")
            .map(|_| ())
    })?;
    retry_with_backoff_cfg(5, base_delay, || {
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
    set_motion_import_state(store, item, ImportState::Logged);

    // Remember where the original lives so thumbnails and viewing stay local
    let _ = store.connection().execute(
        "UPDATE assets SET local_path = ?1 WHERE id = ?2",
        rusqlite::params![path_str, item.record.id],
    );

    Ok(())
}

#[allow(clippy::too_many_arguments)]
fn upload_and_log_concurrent<G>(
    item: &mut PreparedImportAsset,
    album_id: Option<&str>,
    store: &Arc<Mutex<Store>>,
    s4: &dyn ObjectStore,
    cache: &Arc<Mutex<LocalCache>>,
    control: &ImportControl,
    now: DateTime<Utc>,
    on_state_change: G,
) -> Result<()>
where
    G: Fn(),
{
    let path_str = item.primary_path.to_string_lossy().to_string();
    let base_delay = s4.config().retry_base_delay_ms;

    let should_cancel = || {
        while control.is_paused() && !control.is_cancelled() {
            std::thread::sleep(Duration::from_millis(100));
        }
        control.is_cancelled()
    };

    if should_cancel() {
        return Err(SyncError::S4(S4Error::Cancelled));
    }

    // Step 4b: Name clash check via S4 HEAD (no lock held!)
    let (upload_primary, upload_motion) = plan_original_uploads(item, s4)?;

    // Step 5: Generate thumbnails outside the DB lock
    let thumbs = item.build_thumbnails();
    if let Ok(mut c) = cache.lock() {
        let _ = c.put_small_thumb(&item.record.id, &thumbs.small_webp);
        let _ = c.put_preview(&item.record.id, &thumbs.preview_webp);
    }

    let initial_uid = item.existing_upload_id.clone();
    let initial_parts = item.parts_already_done;
    if let Ok(mut st) = store.lock() {
        st.update_import_state(
            &path_str,
            ImportState::Uploading,
            Some(&item.record.id),
            initial_uid.as_deref(),
            initial_parts,
            None,
        )?;
    }
    on_state_change();

    // Step 6: Upload original without holding the DB lock across network I/O
    if upload_primary {
        upload_file_with_resume_cancel(
            s4,
            &item.primary_path,
            &item.record.key,
            "application/octet-stream",
            Some(&item.record.id),
            initial_uid.as_deref(),
            initial_parts,
            5,
            |uid, parts_done| {
                if let Ok(mut st) = store.lock() {
                    let _ = st.update_import_state(
                        &path_str,
                        ImportState::Uploading,
                        Some(&item.record.id),
                        uid,
                        parts_done,
                        None,
                    );
                }
                on_state_change();
            },
            should_cancel,
        )?;
    }

    if let (true, Some(mov_path), Some(mov_key)) =
        (upload_motion, &item.motion_path, &item.record.motion_key)
    {
        upload_file_with_resume_cancel(
            s4,
            mov_path,
            mov_key,
            "video/quicktime",
            item.record.motion_id.as_deref(),
            None,
            0,
            5,
            |_, _| {},
            should_cancel,
        )?;
    }

    let s_key = thumb_small_key(&item.record.id);
    let m_key = thumb_preview_key(&item.record.id);
    retry_with_backoff_cfg(5, base_delay, || {
        if should_cancel() {
            return Err(S4Error::Cancelled);
        }
        s4.put_object(&s_key, &thumbs.small_webp, "image/webp")
            .map(|_| ())
    })?;
    retry_with_backoff_cfg(5, base_delay, || {
        if should_cancel() {
            return Err(S4Error::Cancelled);
        }
        s4.put_object(&m_key, &thumbs.preview_webp, "image/webp")
            .map(|_| ())
    })?;

    // Step 7: Short lock to record Uploaded + add_asset + Logged
    if let Ok(mut st) = store.lock() {
        st.update_import_state(
            &path_str,
            ImportState::Uploaded,
            Some(&item.record.id),
            None,
            1,
            None,
        )?;
        st.add_asset(item.record.clone(), album_id, now)?;
        st.update_import_state(
            &path_str,
            ImportState::Logged,
            Some(&item.record.id),
            None,
            1,
            None,
        )?;
        set_motion_import_state(&mut st, item, ImportState::Logged);

        // Remember where the original lives so thumbnails and viewing stay local
        let _ = st.connection().execute(
            "UPDATE assets SET local_path = ?1 WHERE id = ?2",
            rusqlite::params![path_str, item.record.id],
        );
    }

    Ok(())
}

// =============================================================================
// Export Originals to a Local Folder (Spec Section 2, 8, 9)
// =============================================================================

/// Export the original files (and Live Photo motion `.MOV` files) for `asset_ids`
/// into `destination_dir`, reading from `cache` when available or downloading from S4.
/// Disambiguates colliding filenames with `-<hash8>` (spec section 4) so no exported
/// file overwrites another file in the destination folder.
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
    let mut used_names: HashSet<String> = HashSet::new();

    for id in asset_ids {
        let Some(detail) = store.get_asset(id)? else {
            continue;
        };
        let rec = &detail.record;
        if let Some(cached_path) =
            cache.get_or_fetch_original(s4, &rec.id, &rec.filename, &rec.key)?
        {
            let chosen_name = if used_names.contains(&rec.filename)
                || dest.join(&rec.filename).exists()
            {
                clash_filename(&rec.filename, &rec.id)
            } else {
                rec.filename.clone()
            };
            used_names.insert(chosen_name.clone());
            let target = dest.join(&chosen_name);
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
                let chosen_mov = if used_names.contains(&mov_name)
                    || dest.join(&mov_name).exists()
                {
                    clash_filename(&mov_name, m_id)
                } else {
                    mov_name.clone()
                };
                used_names.insert(chosen_mov.clone());
                let target = dest.join(&chosen_mov);
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
    use crate::keys::original_key;
    use crate::media::sha256_bytes;
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

    #[test]
    fn export_originals_disambiguates_colliding_filenames() {
        let dir1 = TempDir::new();
        let dir2 = TempDir::new();
        let cache_dir = TempDir::new();
        let export_dir = TempDir::new();

        fs::write(dir1.0.join("IMG_20240512_143022.JPG"), b"camera-one-bytes").unwrap();
        fs::write(dir2.0.join("IMG_20240512_143022.JPG"), b"camera-two-bytes").unwrap();

        let s4 = InMemoryObjectStore::new(S4Config::default());
        let mut store = Store::open_in_memory("mac-1").unwrap();
        let mut cache = LocalCache::open(&cache_dir.0, DEFAULT_CACHE_LIMIT_BYTES).unwrap();

        let rep = run_import([&dir1.0, &dir2.0], None, &mut store, &s4, &mut cache, t(10)).unwrap();
        assert_eq!(rep.imported_count, 2);

        let timeline = store.list_timeline(None, None).unwrap();
        assert_eq!(timeline.len(), 2);
        let ids: Vec<String> = timeline.into_iter().map(|a| a.id).collect();

        let exported = export_originals(&ids, &export_dir.0, &store, &s4, &mut cache).unwrap();
        assert_eq!(exported.len(), 2);
        assert_ne!(exported[0], exported[1]);
        assert!(exported[0].exists());
        assert!(exported[1].exists());
        let b0 = fs::read(&exported[0]).unwrap();
        let b1 = fs::read(&exported[1]).unwrap();
        assert_ne!(b0, b1);
    }

    #[test]
    fn import_unknown_s4_keys_adds_unindexed_objects_to_library() {
        let cache_dir = TempDir::new();
        let s4 = InMemoryObjectStore::new(S4Config::default());
        let mut store = Store::open_in_memory("mac-1").unwrap();
        let mut cache = LocalCache::open(&cache_dir.0, DEFAULT_CACHE_LIMIT_BYTES).unwrap();

        let key = "originals/2024/05/IMG_20240512_143022.JPG";
        let bytes = b"unindexed-s4-photo-bytes";
        s4.put_object(key, bytes, "image/jpeg").unwrap();

        let check = check_library(&store, &s4).unwrap();
        assert_eq!(check.unknown_s4_keys, vec![key.to_string()]);

        let added = import_unknown_s4_keys(
            &mut store,
            &s4,
            &mut cache,
            &check.unknown_s4_keys,
            t(50),
        )
        .unwrap();
        assert_eq!(added, 1);

        let expected_id = sha256_bytes(bytes);
        assert!(store.has_active_or_trashed_asset(&expected_id).unwrap());

        let check_after = check_library(&store, &s4).unwrap();
        assert!(check_after.unknown_s4_keys.is_empty());
    }

    #[test]
    fn run_import_resumes_interrupted_upload_and_skips_already_logged_files() {
        let src_dir = TempDir::new();
        let cache_dir = TempDir::new();
        let p1 = src_dir.0.join("IMG_20240512_100000.JPG");
        let p2 = src_dir.0.join("IMG_20240512_100001.JPG");
        fs::write(&p1, b"already-logged-photo-bytes").unwrap();
        fs::write(&p2, b"pending-photo-bytes").unwrap();

        let s4 = InMemoryObjectStore::new(S4Config::default());
        let mut store = Store::open_in_memory("mac-1").unwrap();
        let mut cache = LocalCache::open(&cache_dir.0, DEFAULT_CACHE_LIMIT_BYTES).unwrap();

        // First import only p1
        let rep1 = run_import([&p1], None, &mut store, &s4, &mut cache, t(10)).unwrap();
        assert_eq!(rep1.imported_count, 1);

        // Now run import on the whole directory: p1 is already 'logged' in import_queue
        // with matching size & mtime, so only p2 is hashed and uploaded.
        let rep2 = run_import([&src_dir.0], None, &mut store, &s4, &mut cache, t(20)).unwrap();
        assert_eq!(rep2.imported_count, 1);
        assert_eq!(store.list_timeline(None, None).unwrap().len(), 2);
    }

    #[test]
    fn concurrent_import_enforces_single_run_and_imports_across_worker_pool() {
        let src_dir = TempDir::new();
        let cache_dir = TempDir::new();
        for i in 0..6 {
            fs::write(
                src_dir.0.join(format!("IMG_20240512_10000{i}.JPG")),
                format!("photo-payload-{i}").as_bytes(),
            )
            .unwrap();
        }

        let s4: Arc<dyn ObjectStore> = Arc::new(InMemoryObjectStore::new(S4Config::default()));
        let store = Arc::new(Mutex::new(Store::open_in_memory("mac-1").unwrap()));
        let cache = Arc::new(Mutex::new(
            LocalCache::open(&cache_dir.0, DEFAULT_CACHE_LIMIT_BYTES).unwrap(),
        ));
        let control = Arc::new(ImportControl::new());

        // Simulate an import already running: second call must fail with AlreadyRunning
        assert!(control.try_start_run());
        let dup_err = run_import_concurrent(
            [&src_dir.0],
            None,
            Arc::clone(&store),
            Arc::clone(&s4),
            Arc::clone(&cache),
            Arc::clone(&control),
            t(10),
            |_| {},
        )
        .unwrap_err();
        assert!(matches!(dup_err, SyncError::AlreadyRunning));

        // Release and run for real across the 4-worker pool
        control.set_running(false);
        let rep = run_import_concurrent(
            [&src_dir.0],
            None,
            Arc::clone(&store),
            Arc::clone(&s4),
            Arc::clone(&cache),
            Arc::clone(&control),
            t(10),
            |_| {},
        )
        .unwrap();
        assert_eq!(rep.imported_count, 6);
        assert!(!control.is_running());
        assert_eq!(store.lock().unwrap().list_timeline(None, None).unwrap().len(), 6);
    }

    #[test]
    fn resume_continues_an_interrupted_import_without_duplicate_uploads() {
        let src_dir = TempDir::new();
        let cache_dir = TempDir::new();
        let names = [
            "IMG_20240512_100000.JPG",
            "IMG_20240512_100001.JPG",
            "IMG_20240512_100002.JPG",
        ];
        for (i, name) in names.iter().enumerate() {
            fs::write(src_dir.0.join(name), format!("photo-{i}").as_bytes()).unwrap();
        }

        let mem = InMemoryObjectStore::new(S4Config::default());
        let s4: Arc<dyn ObjectStore> = Arc::new(mem.clone());
        let store = Arc::new(Mutex::new(Store::open_in_memory("mac-1").unwrap()));
        let cache = Arc::new(Mutex::new(
            LocalCache::open(&cache_dir.0, DEFAULT_CACHE_LIMIT_BYTES).unwrap(),
        ));
        let control = Arc::new(ImportControl::new());
        let run = |roots: Vec<PathBuf>| {
            run_import_concurrent(
                roots,
                None,
                Arc::clone(&store),
                Arc::clone(&s4),
                Arc::clone(&cache),
                Arc::clone(&control),
                t(10),
                |_| {},
            )
            .unwrap()
        };

        // The network drops: everything is hashed, nothing is logged.
        mem.set_offline(true);
        assert!(run(vec![src_dir.0.clone()]).paused_offline);
        let pending = unfinished_imports(&store.lock().unwrap()).unwrap();
        assert_eq!(pending.len(), 1);
        assert_eq!(pending[0].1.len(), 3);
        mem.set_offline(false);

        // Photo 0 had finished uploading just before the drop (tagged as itself);
        // photo 1's name is taken by a different file.
        let key0 = original_key(2024, 5, names[0]);
        let photo0 = src_dir.0.join(names[0]);
        mem.put_object_from_file(&key0, &photo0, "image/jpeg", Some(&sha256_file(&photo0).unwrap()))
            .unwrap();
        mem.put_object(&original_key(2024, 5, names[1]), b"someone else", "image/jpeg")
            .unwrap();
        // Photo 2 finished uploading before content tags existed.
        mem.put_object(&original_key(2024, 5, names[2]), b"photo-2", "image/jpeg")
            .unwrap();

        let rep = resume_unfinished_imports(
            Arc::clone(&store),
            Arc::clone(&s4),
            Arc::clone(&cache),
            Arc::clone(&control),
            t(20),
            |_| {},
        )
        .unwrap()
        .expect("there was work to resume");
        assert_eq!(rep.imported_count, 3);
        assert!(unfinished_imports(&store.lock().unwrap()).unwrap().is_empty());
        assert_eq!(store.lock().unwrap().list_timeline(None, None).unwrap().len(), 3);

        // Photos 0 and 2 were not uploaded a second time; photo 1 moved aside.
        let originals: Vec<String> = mem
            .list_objects("originals/")
            .unwrap()
            .into_iter()
            .map(|o| o.key)
            .collect();
        assert_eq!(originals.len(), 4, "{originals:?}");
        assert!(originals.contains(&key0));
        assert_eq!(originals.iter().filter(|k| k.contains("IMG_20240512_100001")).count(), 2);

        // Nothing left: a second resume is a no-op.
        let again = resume_unfinished_imports(store, s4, cache, control, t(30), |_| {}).unwrap();
        assert!(again.is_none());
    }

    #[test]
    fn live_photo_pair_resumes_as_a_pair() {
        let src_dir = TempDir::new();
        let cache_dir = TempDir::new();
        let photo = src_dir.0.join("IMG_20240512_120000.HEIC");
        let motion = src_dir.0.join("IMG_20240512_120000.MOV");
        fs::write(&photo, b"still-bytes").unwrap();
        fs::write(&motion, b"motion-bytes").unwrap();

        let mem = InMemoryObjectStore::new(S4Config::default());
        let s4: Arc<dyn ObjectStore> = Arc::new(mem.clone());
        let store = Arc::new(Mutex::new(Store::open_in_memory("mac-1").unwrap()));
        let cache = Arc::new(Mutex::new(
            LocalCache::open(&cache_dir.0, DEFAULT_CACHE_LIMIT_BYTES).unwrap(),
        ));
        let control = Arc::new(ImportControl::new());

        mem.set_offline(true);
        let rep = run_import_concurrent(
            [&src_dir.0],
            None,
            Arc::clone(&store),
            Arc::clone(&s4),
            Arc::clone(&cache),
            Arc::clone(&control),
            t(10),
            |_| {},
        )
        .unwrap();
        assert!(rep.paused_offline);
        // Both halves are still queued; the MOV was not written off as skipped.
        let pending = unfinished_imports(&store.lock().unwrap()).unwrap();
        assert_eq!(pending[0].1, vec![photo.clone(), motion.clone()]);

        mem.set_offline(false);
        resume_unfinished_imports(Arc::clone(&store), s4, cache, control, t(20), |_| {})
            .unwrap();

        let st = store.lock().unwrap();
        let assets = st.list_timeline(None, None).unwrap();
        assert_eq!(assets.len(), 1);
        assert_eq!(assets[0].kind, AssetKind::Live);
        let queue = st.list_import_queue().unwrap();
        assert!(queue.iter().all(|row| row.state == ImportState::Logged), "{queue:?}");
    }

    #[test]
    fn cancelled_import_is_not_resumed_automatically() {
        let src_dir = TempDir::new();
        let cache_dir = TempDir::new();
        for i in 0..4 {
            fs::write(
                src_dir.0.join(format!("IMG_20240512_11000{i}.JPG")),
                format!("cancel-{i}").as_bytes(),
            )
            .unwrap();
        }
        let s4: Arc<dyn ObjectStore> = Arc::new(InMemoryObjectStore::new(S4Config::default()));
        let store = Arc::new(Mutex::new(Store::open_in_memory("mac-1").unwrap()));
        let cache = Arc::new(Mutex::new(
            LocalCache::open(&cache_dir.0, DEFAULT_CACHE_LIMIT_BYTES).unwrap(),
        ));
        let control = Arc::new(ImportControl::new());

        // The user presses Cancel as soon as the run reports progress.
        let canceller = Arc::clone(&control);
        run_import_concurrent(
            [&src_dir.0],
            None,
            Arc::clone(&store),
            s4,
            cache,
            Arc::clone(&control),
            t(10),
            move |_| canceller.cancel(),
        )
        .unwrap();

        let st = store.lock().unwrap();
        assert!(unfinished_imports(&st).unwrap().is_empty());
        let queue = st.list_import_queue().unwrap();
        assert!(queue
            .iter()
            .filter(|row| row.state != ImportState::Logged)
            .all(|row| row.state == ImportState::Failed
                && row.error.as_deref().is_some_and(|e| e.starts_with("Cancelled"))));
    }
}
