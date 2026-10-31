//! Local SQLite store and sync merge engine (spec sections 5, 6, 9, 11).
//!
//! Responsibilities:
//! - Initialise the local SQLite database and own device id / sequence number.
//! - Apply change-log batches (`Batch` / `Op`) using the deterministic
//!   latest-wins merge rules from spec section 6.
//! - Apply local mutations immediately to SQLite while queuing `Op`s in `outbox`.
//! - Pack `outbox` into bounded `Batch`es (~100 assets max) and mark them flushed.
//! - Export and import `.json.gz` snapshots (`index/snapshot-*.json.gz`).
//! - Keep the `search_fts` FTS5 virtual table synchronised with active assets and albums.
//! - Serve queries for the Timeline, Albums, Trash, Search, and Import Queue.

use std::collections::BTreeMap;
use std::io::{Read, Write};
use std::path::Path;

use chrono::{DateTime, Datelike, Duration, SecondsFormat, Utc};
use flate2::read::GzDecoder;
use flate2::write::GzEncoder;
use flate2::Compression;
use rusqlite::{params, Connection, OptionalExtension, Transaction};
use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::dates::{ shift_utc_minutes, folder_year_month, DateSource, Taken };
use crate::keys::{is_valid_device_id, snapshot_key};
use crate::ops::{AssetKind, AssetRecord, Batch, Op};
use crate::SCHEMA_SQL;

const META_DEVICE_ID: &str = "device_id";
const META_NEXT_SEQ: &str = "next_seq";
const META_BATCHES_SINCE_SNAPSHOT: &str = "batches_since_snapshot";
const META_LAST_SNAPSHOT_AT: &str = "last_snapshot_at";
const META_LOCAL_ROOTS: &str = "local_roots";

pub const SNAPSHOT_BATCH_THRESHOLD: u64 = 500;
pub const SNAPSHOT_MAX_AGE_DAYS: i64 = 7;
pub const TRASH_RETENTION_DAYS: i64 = 30;
pub const DEFAULT_OUTBOX_MAX_ASSETS: usize = 100;

#[derive(Debug, Error)]
pub enum StoreError {
    #[error("sqlite error: {0}")]
    Sqlite(#[from] rusqlite::Error),
    #[error("json error: {0}")]
    Json(#[from] serde_json::Error),
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),
    #[error("invalid device id: {0}")]
    InvalidDeviceId(String),
    #[error("invalid timestamp in database: {0}")]
    InvalidTimestamp(String),
}

pub type Result<T> = std::result::Result<T, StoreError>;

/// Format a UTC timestamp as fixed-width RFC 3339 with milliseconds and `Z`,
/// e.g. `2026-11-02T03:14:15.926Z`.
pub fn format_utc(dt: DateTime<Utc>) -> String {
    dt.to_rfc3339_opts(SecondsFormat::Millis, true)
}

/// Parse an RFC 3339 UTC timestamp from SQLite.
pub fn parse_utc(s: &str) -> Result<DateTime<Utc>> {
    DateTime::parse_from_rfc3339(s)
        .map(|dt| dt.with_timezone(&Utc))
        .map_err(|_| StoreError::InvalidTimestamp(s.to_string()))
}

/// Build a merge stamp `<RFC 3339 UTC with ms and Z>|<device-id>`.
/// Because the timestamp has fixed width, plain string comparison orders by
/// batch `at` first and `device` id as tie-breaker (spec section 6).
pub fn make_stamp(at: DateTime<Utc>, device: &str) -> String {
    format!("{}|{device}", format_utc(at))
}

fn kind_to_str(kind: AssetKind) -> &'static str {
    match kind {
        AssetKind::Photo => "photo",
        AssetKind::Video => "video",
        AssetKind::Live => "live",
    }
}

fn str_to_kind(s: &str) -> AssetKind {
    match s {
        "video" => AssetKind::Video,
        "live" => AssetKind::Live,
        _ => AssetKind::Photo,
    }
}

fn generate_device_id() -> String {
    let prefix = if cfg!(target_os = "macos") {
        "mac"
    } else if cfg!(target_os = "windows") {
        "win"
    } else {
        "dev"
    };
    let rand_hex: u32 = fastrand::u32(..);
    format!("{prefix}-{rand_hex:08x}")
}

/// Summary row for timeline, album, and search grids.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AssetSummary {
    pub id: String,
    pub kind: AssetKind,
    pub key: String,
    pub filename: String,
    pub size: u64,
    pub taken_at: DateTime<Utc>,
    pub tz_offset: Option<i32>,
    pub date_guessed: bool,
    pub width: Option<u32>,
    pub height: Option<u32>,
    pub duration_ms: Option<u64>,
    pub thumb_failed: bool,
}

/// Full asset detail for the viewer info panel.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AssetDetail {
    pub record: AssetRecord,
    pub trashed_at: Option<DateTime<Utc>>,
    pub albums: Vec<AlbumRef>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AlbumRef {
    pub id: String,
    pub name: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MonthGroup {
    pub year: i32,
    pub month: u32,
    pub count: usize,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AlbumSummary {
    pub id: String,
    pub name: String,
    pub created_at: DateTime<Utc>,
    pub asset_count: usize,
    pub cover_asset_id: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MatchedAlbumFolder {
    pub album_id: String,
    pub album_name: String,
    pub folder_path: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TrashItem {
    pub asset: AssetSummary,
    pub trashed_at: DateTime<Utc>,
    pub days_left: i64,
    pub motion_size: Option<u64>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PurgeCandidate {
    pub id: String,
    pub key: String,
    pub motion_key: Option<String>,
}

/// A batch of local ops packed from `outbox`, ready to be uploaded to S4.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PendingOutboxBatch {
    pub batch: Batch,
    pub outbox_ids: Vec<i64>,
}

/// Search autocomplete suggestions grouped by category (spec section 9).
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct SearchSuggestions {
    pub albums: Vec<AlbumRef>,
    pub dates: Vec<String>,
    pub cameras: Vec<String>,
    pub file_types: Vec<String>,
    pub filenames: Vec<String>,
}

/// Structured filter chips + free-text query for local search (spec section 9).
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct SearchFilter {
    pub text: Option<String>,
    pub album_id: Option<String>,
    pub year: Option<i32>,
    pub month: Option<u32>,
    pub camera: Option<String>,
    pub kind: Option<AssetKind>,
    pub file_ext: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ImportState {
    Pending,
    Hashed,
    Uploading,
    Uploaded,
    Logged,
    Skipped,
    Failed,
}

impl ImportState {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Pending => "pending",
            Self::Hashed => "hashed",
            Self::Uploading => "uploading",
            Self::Uploaded => "uploaded",
            Self::Logged => "logged",
            Self::Skipped => "skipped",
            Self::Failed => "failed",
        }
    }

    #[allow(clippy::should_implement_trait)]
    pub fn from_str(s: &str) -> Self {
        match s {
            "hashed" => Self::Hashed,
            "uploading" => Self::Uploading,
            "uploaded" => Self::Uploaded,
            "logged" => Self::Logged,
            "skipped" => Self::Skipped,
            "failed" => Self::Failed,
            _ => Self::Pending,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ImportQueueRow {
    pub path: String,
    pub size: u64,
    pub mtime: i64,
    pub state: ImportState,
    pub asset_id: Option<String>,
    pub upload_id: Option<String>,
    pub parts_done: u32,
    pub album_id: Option<String>,
    pub error: Option<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ImportProgress {
    pub total_files: usize,
    pub total_bytes: u64,
    pub completed_files: usize,
    pub completed_bytes: u64,
    pub skipped_duplicates: usize,
    pub failed_files: usize,
    #[serde(default)]
    pub hashed_files: usize,
    #[serde(default)]
    pub uploading_files: usize,
}

/// Full-state snapshot payload written to `index/snapshot-<ts>-<device>.json.gz`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Snapshot {
    pub created_at: DateTime<Utc>,
    pub device: String,
    pub sync_state: BTreeMap<String, u64>,
    pub assets: Vec<SnapshotAsset>,
    pub albums: Vec<SnapshotAlbum>,
    pub album_items: Vec<SnapshotAlbumItem>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SnapshotAsset {
    pub record: AssetRecord,
    pub date_at: String,
    pub trashed_at: Option<String>,
    pub purged_at: Option<String>,
    pub state_at: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SnapshotAlbum {
    pub id: String,
    pub name: String,
    pub name_at: String,
    pub created_at: String,
    pub deleted_at: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SnapshotAlbumItem {
    pub album_id: String,
    pub asset_id: String,
    pub in_album: bool,
    pub at: String,
}

pub struct Store {
    conn: Connection,
}

impl Store {
    /// Open or create the SQLite store at `path`.
    pub fn open(path: impl AsRef<Path>) -> Result<Self> {
        let conn = Connection::open(path)?;
        let _ = conn.pragma_update(None, "journal_mode", "WAL");
        Self::init(conn, None)
    }

    /// Open or create the SQLite store at `path` with an explicit `device_id`.
    pub fn open_with_device(path: impl AsRef<Path>, device_id: &str) -> Result<Self> {
        if !is_valid_device_id(device_id) {
            return Err(StoreError::InvalidDeviceId(device_id.to_string()));
        }
        let conn = Connection::open(path)?;
        let _ = conn.pragma_update(None, "journal_mode", "WAL");
        Self::init(conn, Some(device_id))
    }

    /// Open an in-memory database (used in unit and integration tests).
    pub fn open_in_memory(device_id: &str) -> Result<Self> {
        if !is_valid_device_id(device_id) {
            return Err(StoreError::InvalidDeviceId(device_id.to_string()));
        }
        let conn = Connection::open_in_memory()?;
        Self::init(conn, Some(device_id))
    }

    fn init(conn: Connection, explicit_device_id: Option<&str>) -> Result<Self> {
        let version: i64 = conn.pragma_query_value(None, "user_version", |r| r.get(0))?;
        if version == 0 {
            conn.execute_batch(SCHEMA_SQL)?;
        }
        Self::run_migrations(&conn)?;

        let existing_dev: Option<String> = conn
            .query_row(
                "SELECT value FROM local_meta WHERE key = ?1",
                [META_DEVICE_ID],
                |r| r.get(0),
            )
            .optional()?;

        if existing_dev.is_none() {
            let dev = explicit_device_id
                .map(|s| s.to_string())
                .unwrap_or_else(generate_device_id);
            conn.execute(
                "INSERT INTO local_meta (key, value) VALUES (?1, ?2)",
                params![META_DEVICE_ID, dev],
            )?;
        } else if let Some(explicit) = explicit_device_id {
            conn.execute(
                "UPDATE local_meta SET value = ?2 WHERE key = ?1",
                params![META_DEVICE_ID, explicit],
            )?;
        }

        conn.execute(
            "INSERT OR IGNORE INTO local_meta (key, value) VALUES (?1, '1')",
            [META_NEXT_SEQ],
        )?;
        conn.execute(
            "INSERT OR IGNORE INTO local_meta (key, value) VALUES (?1, '0')",
            [META_BATCHES_SINCE_SNAPSHOT],
        )?;

        Ok(Self { conn })
    }

    pub fn connection(&self) -> &Connection {
        &self.conn
    }

    /// Migrates the database to the latest schema safely and idempotently.
    pub fn run_migrations(conn: &Connection) -> Result<()> {
        let assets_exists: bool = conn
            .query_row(
                "SELECT count(*) FROM sqlite_master WHERE type = 'table' AND name = 'assets'",
                [],
                |r| r.get::<_, i64>(0),
            )
            .map(|c| c > 0)
            .unwrap_or(false);

        if !assets_exists {
            conn.execute_batch(SCHEMA_SQL)?;
        }

        let mut stmt = conn.prepare("PRAGMA table_info(assets)")?;
        let existing_cols: std::collections::HashSet<String> = stmt
            .query_map([], |row| row.get::<_, String>(1))?
            .filter_map(|r| r.ok())
            .collect();

        if !existing_cols.contains("album_id") {
            conn.execute("ALTER TABLE assets ADD COLUMN album_id TEXT", [])?;
        }
        if !existing_cols.contains("relative_path") {
            conn.execute("ALTER TABLE assets ADD COLUMN relative_path TEXT", [])?;
        }
        if !existing_cols.contains("local_path") {
            conn.execute("ALTER TABLE assets ADD COLUMN local_path TEXT", [])?;
        }
        if !existing_cols.contains("remote_key") {
            conn.execute("ALTER TABLE assets ADD COLUMN remote_key TEXT", [])?;
        }
        if !existing_cols.contains("thumb_state") {
            conn.execute("ALTER TABLE assets ADD COLUMN thumb_state TEXT NOT NULL DEFAULT 'none'", [])?;
        }
        if !existing_cols.contains("content_hash") {
            conn.execute("ALTER TABLE assets ADD COLUMN content_hash TEXT", [])?;
        }

        conn.execute(
            "CREATE UNIQUE INDEX IF NOT EXISTS assets_album_relpath ON assets (album_id, relative_path)",
            [],
        )?;

        Ok(())
    }

    pub fn set_local_roots(&self, paths: Vec<String>) -> Result<()> {
        let json = serde_json::to_string(&paths)?;
        self.conn.execute(
            "INSERT INTO local_meta (key, value) VALUES (?1, ?2)
             ON CONFLICT(key) DO UPDATE SET value = excluded.value",
            params![META_LOCAL_ROOTS, json],
        )?;
        Ok(())
    }

    pub fn get_local_roots(&self) -> Result<Vec<String>> {
        let res: Option<String> = self.conn
            .query_row(
                "SELECT value FROM local_meta WHERE key = ?1",
                [META_LOCAL_ROOTS],
                |r| r.get(0),
            )
            .optional()?;
        match res {
            Some(s) => Ok(serde_json::from_str(&s)?),
            None => Ok(Vec::new()),
        }
    }

    pub fn match_local_albums(&self) -> Result<Vec<MatchedAlbumFolder>> {
        let roots = self.get_local_roots()?;
        self.match_local_albums_for_roots(&roots)
    }

    pub fn match_local_albums_for_roots(&self, roots: &[String]) -> Result<Vec<MatchedAlbumFolder>> {
        let mut stmt = self.conn.prepare("SELECT id, name FROM albums WHERE deleted_at IS NULL")?;
        let albums: Vec<(String, String)> = stmt
            .query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?)))?
            .filter_map(|r| r.ok())
            .collect();

        let mut matches = Vec::new();
        for root in roots {
            let root_path = std::path::Path::new(root);
            if let Ok(entries) = std::fs::read_dir(root_path) {
                for entry in entries.flatten() {
                    if let Ok(ft) = entry.file_type() {
                        if ft.is_dir() {
                            let folder_name = entry.file_name().to_string_lossy().to_string();
                            for (album_id, album_name) in &albums {
                                if album_name.eq_ignore_ascii_case(&folder_name) {
                                    matches.push(MatchedAlbumFolder {
                                        album_id: album_id.clone(),
                                        album_name: album_name.clone(),
                                        folder_path: entry.path().to_string_lossy().to_string(),
                                    });
                                }
                            }
                        }
                    }
                }
            }
        }
        Ok(matches)
    }

    /// Own device id stored in `local_meta`.
    pub fn device_id(&self) -> Result<String> {
        self.conn
            .query_row(
                "SELECT value FROM local_meta WHERE key = ?1",
                [META_DEVICE_ID],
                |r| r.get(0),
            )
            .map_err(Into::into)
    }

    /// Own next change-log sequence number.
    pub fn next_seq(&self) -> Result<u64> {
        let s: String = self.conn.query_row(
            "SELECT value FROM local_meta WHERE key = ?1",
            [META_NEXT_SEQ],
            |r| r.get(0),
        )?;
        Ok(s.parse().unwrap_or(1))
    }

    /// Highest applied sequence number for `device` (0 if none applied yet).
    pub fn applied_seq(&self, device: &str) -> Result<u64> {
        let seq: Option<i64> = self
            .conn
            .query_row(
                "SELECT applied_seq FROM sync_state WHERE device = ?1",
                [device],
                |r| r.get(0),
            )
            .optional()?;
        Ok(seq.unwrap_or(0) as u64)
    }

    /// All devices and their highest applied sequence numbers.
    pub fn all_sync_states(&self) -> Result<BTreeMap<String, u64>> {
        let mut stmt = self
            .conn
            .prepare("SELECT device, applied_seq FROM sync_state ORDER BY device")?;
        let rows = stmt.query_map([], |r| {
            Ok((r.get::<_, String>(0)?, r.get::<_, i64>(1)? as u64))
        })?;
        let mut map = BTreeMap::new();
        for row in rows {
            let (k, v) = row?;
            map.insert(k, v);
        }
        Ok(map)
    }

    // =========================================================================
    // Local operations & Outbox
    // =========================================================================

    /// Apply `ops` immediately to local SQLite and append each to `outbox`.
    pub fn record_local_ops(&mut self, ops: &[Op], now: DateTime<Utc>) -> Result<()> {
        if ops.is_empty() {
            return Ok(());
        }
        let device = self.device_id()?;
        let stamp = make_stamp(now, &device);
        let now_str = format_utc(now);
        let tx = self.conn.transaction()?;
        for op in ops {
            apply_op_in_tx(&tx, op, now, &now_str, &stamp)?;
            let op_json = serde_json::to_string(op)?;
            tx.execute(
                "INSERT INTO outbox (op_json, created_at) VALUES (?1, ?2)",
                params![op_json, now_str],
            )?;
        }
        tx.commit()?;
        Ok(())
    }

    pub fn add_asset(
        &mut self,
        asset: AssetRecord,
        album_id: Option<&str>,
        now: DateTime<Utc>,
    ) -> Result<()> {
        let mut ops = Vec::with_capacity(2);
        let id = asset.id.clone();
        ops.push(Op::AssetAdd { asset });
        if let Some(alb) = album_id {
            ops.push(Op::AlbumAdd {
                id: alb.to_string(),
                ids: vec![id],
            });
        }
        self.record_local_ops(&ops, now)
    }

    pub fn set_asset_date(
        &mut self,
        id: &str,
        taken_at: DateTime<Utc>,
        now: DateTime<Utc>,
    ) -> Result<()> {
        self.record_local_ops(
            &[Op::AssetSetDate {
                id: id.to_string(),
                taken_at,
            }],
            now,
        )
    }

    pub fn trash_assets(&mut self, ids: Vec<String>, now: DateTime<Utc>) -> Result<()> {
        self.record_local_ops(&[Op::AssetTrash { ids }], now)
    }

    pub fn restore_assets(&mut self, ids: Vec<String>, now: DateTime<Utc>) -> Result<()> {
        self.record_local_ops(&[Op::AssetRestore { ids }], now)
    }

    pub fn purge_assets(&mut self, ids: Vec<String>, now: DateTime<Utc>) -> Result<()> {
        self.record_local_ops(&[Op::AssetPurge { ids }], now)
    }

    pub fn create_album(&mut self, id: String, name: String, now: DateTime<Utc>) -> Result<()> {
        self.record_local_ops(&[Op::AlbumCreate { id, name }], now)
    }

    pub fn rename_album(&mut self, id: &str, name: String, now: DateTime<Utc>) -> Result<()> {
        self.record_local_ops(
            &[Op::AlbumRename {
                id: id.to_string(),
                name,
            }],
            now,
        )
    }

    pub fn delete_album(&mut self, id: &str, now: DateTime<Utc>) -> Result<()> {
        self.record_local_ops(&[Op::AlbumDelete { id: id.to_string() }], now)
    }

    pub fn add_to_album(
        &mut self,
        album_id: &str,
        asset_ids: Vec<String>,
        now: DateTime<Utc>,
    ) -> Result<()> {
        self.record_local_ops(
            &[Op::AlbumAdd {
                id: album_id.to_string(),
                ids: asset_ids,
            }],
            now,
        )
    }

    pub fn remove_from_album(
        &mut self,
        album_id: &str,
        asset_ids: Vec<String>,
        now: DateTime<Utc>,
    ) -> Result<()> {
        self.record_local_ops(
            &[Op::AlbumRemove {
                id: album_id.to_string(),
                ids: asset_ids,
            }],
            now,
        )
    }

    /// Spec section 5: "A move is `album.remove` + `album.add` in one batch."
    pub fn move_to_album(
        &mut self,
        from_album: &str,
        to_album: &str,
        asset_ids: Vec<String>,
        now: DateTime<Utc>,
    ) -> Result<()> {
        self.record_local_ops(
            &[
                Op::AlbumRemove {
                    id: from_album.to_string(),
                    ids: asset_ids.clone(),
                },
                Op::AlbumAdd {
                    id: to_album.to_string(),
                    ids: asset_ids,
                },
            ],
            now,
        )
    }

    pub fn outbox_len(&self) -> Result<usize> {
        let n: i64 = self
            .conn
            .query_row("SELECT count(*) FROM outbox", [], |r| r.get(0))?;
        Ok(n as usize)
    }

    /// Pack the next batch from `outbox` with at most `max_assets` `asset.add` ops
    /// (spec section 6: "at most ~100 assets per batch during import").
    pub fn peek_outbox_batch(
        &self,
        max_assets: usize,
        now: DateTime<Utc>,
    ) -> Result<Option<PendingOutboxBatch>> {
        let cap = max_assets.max(1);
        let mut stmt = self
            .conn
            .prepare("SELECT id, op_json FROM outbox ORDER BY id ASC")?;
        let rows = stmt.query_map([], |r| {
            Ok((r.get::<_, i64>(0)?, r.get::<_, String>(1)?))
        })?;

        let mut ops = Vec::new();
        let mut outbox_ids = Vec::new();
        let mut assets_in_batch = 0usize;

        for row in rows {
            let (id, op_json) = row?;
            let op: Op = serde_json::from_str(&op_json)?;
            let is_asset_add = matches!(op, Op::AssetAdd { .. });
            if is_asset_add && assets_in_batch >= cap && !ops.is_empty() {
                break;
            }
            if is_asset_add {
                assets_in_batch += 1;
            }
            ops.push(op);
            outbox_ids.push(id);
        }

        if ops.is_empty() {
            return Ok(None);
        }

        let device = self.device_id()?;
        let seq = self.next_seq()?;
        Ok(Some(PendingOutboxBatch {
            batch: Batch {
                device,
                seq,
                at: now,
                ops,
            },
            outbox_ids,
        }))
    }

    /// Mark a pending outbox batch as uploaded to S4: remove its rows from
    /// `outbox`, align local stamps with the flushed `batch.at`, record
    /// `sync_state` for own device, and advance `next_seq`.
    pub fn mark_outbox_flushed(&mut self, pending: &PendingOutboxBatch) -> Result<()> {
        let stamp = make_stamp(pending.batch.at, &pending.batch.device);
        let at_str = format_utc(pending.batch.at);
        let tx = self.conn.transaction()?;
        for op in &pending.batch.ops {
            apply_op_in_tx(&tx, op, pending.batch.at, &at_str, &stamp)?;
        }
        for id in &pending.outbox_ids {
            tx.execute("DELETE FROM outbox WHERE id = ?1", [id])?;
        }
        tx.execute(
            "INSERT INTO sync_state (device, applied_seq) VALUES (?1, ?2)
             ON CONFLICT(device) DO UPDATE SET applied_seq = excluded.applied_seq
             WHERE excluded.applied_seq > sync_state.applied_seq",
            params![pending.batch.device, pending.batch.seq as i64],
        )?;
        let next = pending.batch.seq + 1;
        tx.execute(
            "UPDATE local_meta SET value = ?2 WHERE key = ?1",
            params![META_NEXT_SEQ, next.to_string()],
        )?;
        increment_batches_since_snapshot(&tx)?;
        tx.commit()?;
        Ok(())
    }

    // =========================================================================
    // Remote batch merge
    // =========================================================================

    /// Apply a change-log `Batch` from another device (or replayed during rebuild).
    /// Returns `Ok(true)` if applied, or `Ok(false)` if `batch.seq` was already applied.
    pub fn apply_remote_batch(&mut self, batch: &Batch) -> Result<bool> {
        let already = self.applied_seq(&batch.device)?;
        if batch.seq <= already {
            return Ok(false);
        }

        let stamp = make_stamp(batch.at, &batch.device);
        let at_str = format_utc(batch.at);
        let tx = self.conn.transaction()?;
        for op in &batch.ops {
            apply_op_in_tx(&tx, op, batch.at, &at_str, &stamp)?;
        }
        tx.execute(
            "INSERT INTO sync_state (device, applied_seq) VALUES (?1, ?2)
             ON CONFLICT(device) DO UPDATE SET applied_seq = excluded.applied_seq",
            params![batch.device, batch.seq as i64],
        )?;
        increment_batches_since_snapshot(&tx)?;
        tx.commit()?;
        Ok(true)
    }

    // =========================================================================
    // Snapshots & Rebuild
    // =========================================================================

    pub fn batches_since_snapshot(&self) -> Result<u64> {
        let s: String = self
            .conn
            .query_row(
                "SELECT value FROM local_meta WHERE key = ?1",
                [META_BATCHES_SINCE_SNAPSHOT],
                |r| r.get(0),
            )
            .optional()?
            .unwrap_or_else(|| "0".to_string());
        Ok(s.parse().unwrap_or(0))
    }

    /// True if >= 500 batches have been applied since the last snapshot, or if
    /// at least 1 batch was applied and the last snapshot was >= 7 days ago.
    pub fn needs_snapshot(&self, now: DateTime<Utc>) -> Result<bool> {
        let count = self.batches_since_snapshot()?;
        if count >= SNAPSHOT_BATCH_THRESHOLD {
            return Ok(true);
        }
        if count == 0 {
            return Ok(false);
        }
        let last: Option<String> = self
            .conn
            .query_row(
                "SELECT value FROM local_meta WHERE key = ?1",
                [META_LAST_SNAPSHOT_AT],
                |r| r.get(0),
            )
            .optional()?;
        match last {
            Some(ts) => {
                let dt = parse_utc(&ts)?;
                Ok(now.signed_duration_since(dt) >= Duration::days(SNAPSHOT_MAX_AGE_DAYS))
            }
            None => Ok(false),
        }
    }

    /// Export full database state (excluding local-only tables) as a gzipped JSON
    /// snapshot and reset the snapshot counters. Returns `(s4_key, gz_bytes)`.
    pub fn export_snapshot_gz(&mut self, now: DateTime<Utc>) -> Result<(String, Vec<u8>)> {
        let device = self.device_id()?;
        let sync_state = self.all_sync_states()?;

        let mut asset_stmt = self.conn.prepare(
            "SELECT id, kind, key, filename, size, taken_at, tz_offset, date_guessed,
                    date_at, width, height, duration_ms, container, video_codec, audio_codec,
                    camera_make, camera_model, motion_id, motion_key, motion_size,
                    thumb_failed, trashed_at, purged_at, state_at
             FROM assets ORDER BY id",
        )?;
        let assets = asset_stmt
            .query_map([], |r| {
                Ok(SnapshotAsset {
                    record: AssetRecord {
                        id: r.get(0)?,
                        kind: str_to_kind(&r.get::<_, String>(1)?),
                        key: r.get(2)?,
                        filename: r.get(3)?,
                        size: r.get::<_, i64>(4)? as u64,
                        taken_at: parse_utc(&r.get::<_, String>(5)?).unwrap_or(now),
                        tz_offset: r.get(6)?,
                        date_guessed: r.get::<_, i64>(7)? != 0,
                        width: r.get::<_, Option<i64>>(9)?.map(|v| v as u32),
                        height: r.get::<_, Option<i64>>(10)?.map(|v| v as u32),
                        duration_ms: r.get::<_, Option<i64>>(11)?.map(|v| v as u64),
                        container: r.get(12)?,
                        video_codec: r.get(13)?,
                        audio_codec: r.get(14)?,
                        camera_make: r.get(15)?,
                        camera_model: r.get(16)?,
                        motion_id: r.get(17)?,
                        motion_key: r.get(18)?,
                        motion_size: r.get::<_, Option<i64>>(19)?.map(|v| v as u64),
                        thumb_failed: r.get::<_, i64>(20)? != 0,
                    },
                    date_at: r.get(8)?,
                    trashed_at: r.get(21)?,
                    purged_at: r.get(22)?,
                    state_at: r.get(23)?,
                })
            })?
            .collect::<std::result::Result<Vec<_>, _>>()?;

        let mut album_stmt = self.conn.prepare(
            "SELECT id, name, name_at, created_at, deleted_at FROM albums ORDER BY id",
        )?;
        let albums = album_stmt
            .query_map([], |r| {
                Ok(SnapshotAlbum {
                    id: r.get(0)?,
                    name: r.get(1)?,
                    name_at: r.get(2)?,
                    created_at: r.get(3)?,
                    deleted_at: r.get(4)?,
                })
            })?
            .collect::<std::result::Result<Vec<_>, _>>()?;

        let mut item_stmt = self.conn.prepare(
            "SELECT album_id, asset_id, in_album, at FROM album_items ORDER BY album_id, asset_id",
        )?;
        let album_items = item_stmt
            .query_map([], |r| {
                Ok(SnapshotAlbumItem {
                    album_id: r.get(0)?,
                    asset_id: r.get(1)?,
                    in_album: r.get::<_, i64>(2)? != 0,
                    at: r.get(3)?,
                })
            })?
            .collect::<std::result::Result<Vec<_>, _>>()?;

        let snap = Snapshot {
            created_at: now,
            device: device.clone(),
            sync_state,
            assets,
            albums,
            album_items,
        };

        let json_bytes = serde_json::to_vec(&snap)?;
        let mut encoder = GzEncoder::new(Vec::new(), Compression::default());
        encoder.write_all(&json_bytes)?;
        let gz_bytes = encoder.finish()?;

        let now_str = format_utc(now);
        self.conn.execute(
            "INSERT INTO local_meta (key, value) VALUES (?1, '0')
             ON CONFLICT(key) DO UPDATE SET value = '0'",
            [META_BATCHES_SINCE_SNAPSHOT],
        )?;
        self.conn.execute(
            "INSERT INTO local_meta (key, value) VALUES (?1, ?2)
             ON CONFLICT(key) DO UPDATE SET value = excluded.value",
            params![META_LAST_SNAPSHOT_AT, now_str],
        )?;

        Ok((snapshot_key(now, &device), gz_bytes))
    }

    /// Load a `.json.gz` snapshot into the local database and rebuild `search_fts`.
    pub fn import_snapshot_gz(&mut self, gz_bytes: &[u8]) -> Result<()> {
        let mut decoder = GzDecoder::new(gz_bytes);
        let mut json_bytes = Vec::new();
        decoder.read_to_end(&mut json_bytes)?;
        let snap: Snapshot = serde_json::from_slice(&json_bytes)?;

        let tx = self.conn.transaction()?;
        tx.execute("DELETE FROM assets", [])?;
        tx.execute("DELETE FROM albums", [])?;
        tx.execute("DELETE FROM album_items", [])?;
        tx.execute("DELETE FROM search_fts", [])?;
        tx.execute("DELETE FROM sync_state", [])?;

        for (dev, seq) in &snap.sync_state {
            tx.execute(
                "INSERT INTO sync_state (device, applied_seq) VALUES (?1, ?2)",
                params![dev, *seq as i64],
            )?;
        }

        for a in &snap.assets {
            let r = &a.record;
            tx.execute(
                "INSERT INTO assets (
                    id, kind, key, filename, size, taken_at, tz_offset, date_guessed,
                    date_at, width, height, duration_ms, container, video_codec, audio_codec,
                    camera_make, camera_model, motion_id, motion_key, motion_size,
                    thumb_failed, trashed_at, purged_at, state_at
                ) VALUES (
                    ?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8,
                    ?9, ?10, ?11, ?12, ?13, ?14, ?15,
                    ?16, ?17, ?18, ?19, ?20,
                    ?21, ?22, ?23, ?24
                )",
                params![
                    r.id,
                    kind_to_str(r.kind),
                    r.key,
                    r.filename,
                    r.size as i64,
                    format_utc(r.taken_at),
                    r.tz_offset,
                    i32::from(r.date_guessed),
                    a.date_at,
                    r.width.map(i64::from),
                    r.height.map(i64::from),
                    r.duration_ms.map(|v| v as i64),
                    r.container,
                    r.video_codec,
                    r.audio_codec,
                    r.camera_make,
                    r.camera_model,
                    r.motion_id,
                    r.motion_key,
                    r.motion_size.map(|v| v as i64),
                    i32::from(r.thumb_failed),
                    a.trashed_at,
                    a.purged_at,
                    a.state_at,
                ],
            )?;
        }

        for alb in &snap.albums {
            tx.execute(
                "INSERT INTO albums (id, name, name_at, created_at, deleted_at)
                 VALUES (?1, ?2, ?3, ?4, ?5)",
                params![alb.id, alb.name, alb.name_at, alb.created_at, alb.deleted_at],
            )?;
        }

        for item in &snap.album_items {
            tx.execute(
                "INSERT INTO album_items (album_id, asset_id, in_album, at)
                 VALUES (?1, ?2, ?3, ?4)",
                params![
                    item.album_id,
                    item.asset_id,
                    i32::from(item.in_album),
                    item.at
                ],
            )?;
        }

        // Advance own next_seq if snapshot knows about a higher seq for own device
        let own_dev: String = tx.query_row(
            "SELECT value FROM local_meta WHERE key = ?1",
            [META_DEVICE_ID],
            |r| r.get(0),
        )?;
        if let Some(&applied) = snap.sync_state.get(&own_dev) {
            let cur_next: u64 = tx
                .query_row(
                    "SELECT value FROM local_meta WHERE key = ?1",
                    [META_NEXT_SEQ],
                    |r| r.get::<_, String>(0),
                )?
                .parse()
                .unwrap_or(1);
            if applied + 1 > cur_next {
                tx.execute(
                    "UPDATE local_meta SET value = ?2 WHERE key = ?1",
                    params![META_NEXT_SEQ, (applied + 1).to_string()],
                )?;
            }
        }

        // Rebuild FTS5 index for all visible assets
        for a in &snap.assets {
            if a.trashed_at.is_none() && a.purged_at.is_none() {
                refresh_fts_for_asset(&tx, &a.record.id)?;
            }
        }

        tx.execute(
            "INSERT INTO local_meta (key, value) VALUES (?1, '0')
             ON CONFLICT(key) DO UPDATE SET value = '0'",
            [META_BATCHES_SINCE_SNAPSHOT],
        )?;
        tx.commit()?;
        Ok(())
    }

    /// Clear synced state so the database can be rebuilt from S4 snapshot + logs
    /// (spec section 11: "Rebuild deletes it and reloads from the snapshot plus logs").
    pub fn reset_for_rebuild(&mut self) -> Result<()> {
        let tx = self.conn.transaction()?;
        tx.execute("DELETE FROM assets", [])?;
        tx.execute("DELETE FROM albums", [])?;
        tx.execute("DELETE FROM album_items", [])?;
        tx.execute("DELETE FROM search_fts", [])?;
        tx.execute("DELETE FROM sync_state", [])?;
        tx.execute(
            "UPDATE local_meta SET value = '0' WHERE key = ?1",
            [META_BATCHES_SINCE_SNAPSHOT],
        )?;
        tx.commit()?;
        Ok(())
    }

    // =========================================================================
    // Timeline, Viewer, Album & Trash Queries
    // =========================================================================

    /// True if an asset with `id` is already in the index (and not purged).
    /// Used by the importer to skip exact duplicates (spec section 7, step 2).
    pub fn has_active_or_trashed_asset(&self, id: &str) -> Result<bool> {
        let n: i64 = self.conn.query_row(
            "SELECT count(*) FROM assets WHERE id = ?1 AND purged_at IS NULL",
            [id],
            |r| r.get(0),
        )?;
        Ok(n > 0)
    }

    /// Timeline assets sorted newest first (`taken_at DESC, id ASC`), excluding trashed and purged.
    pub fn list_timeline(
        &self,
        limit: Option<usize>,
        offset: Option<usize>,
    ) -> Result<Vec<AssetSummary>> {
        let lim = limit.unwrap_or(usize::MAX) as i64;
        let off = offset.unwrap_or(0) as i64;
        let mut stmt = self.conn.prepare(
            "SELECT id, kind, key, filename, size, taken_at, tz_offset, date_guessed,
                    width, height, duration_ms, thumb_failed
             FROM assets
             WHERE trashed_at IS NULL AND purged_at IS NULL
             ORDER BY taken_at DESC, id ASC
             LIMIT ?1 OFFSET ?2",
        )?;
        let rows = stmt.query_map(params![lim, off], row_to_summary)?;
        rows.collect::<std::result::Result<Vec<_>, _>>()
            .map_err(Into::into)
    }

    /// Group visible timeline assets into `(year, month, count)` in descending order
    /// using local wall-clock time (`taken_at` + `tz_offset`), matching sticky month headers.
    pub fn timeline_month_groups(&self) -> Result<Vec<MonthGroup>> {
        let mut stmt = self.conn.prepare(
            "SELECT taken_at, tz_offset
             FROM assets
             WHERE trashed_at IS NULL AND purged_at IS NULL
             ORDER BY taken_at DESC, id ASC",
        )?;
        let rows = stmt.query_map([], |r| {
            Ok((r.get::<_, String>(0)?, r.get::<_, Option<i32>>(1)?))
        })?;

        let mut groups: Vec<MonthGroup> = Vec::new();
        for row in rows {
            let (taken_str, tz_offset) = row?;
            let utc = parse_utc(&taken_str)?;
            let taken = Taken {
                utc,
                tz_offset_minutes: tz_offset,
                source: DateSource::Exif,
                guessed: false,
            };
            let (year, month) = folder_year_month(&taken);
            if let Some(last) = groups.last_mut() {
                if last.year == year && last.month == month {
                    last.count += 1;
                    continue;
                }
            }
            groups.push(MonthGroup {
                year,
                month,
                count: 1,
            });
        }
        Ok(groups)
    }

    /// Detailed metadata + album memberships for a single asset.
    pub fn get_asset(&self, id: &str) -> Result<Option<AssetDetail>> {
        let row = self
            .conn
            .query_row(
                "SELECT id, kind, key, filename, size, taken_at, tz_offset, date_guessed,
                        width, height, duration_ms, container, video_codec, audio_codec,
                        camera_make, camera_model, motion_id, motion_key, motion_size,
                        thumb_failed, trashed_at
                 FROM assets
                 WHERE id = ?1 AND purged_at IS NULL",
                [id],
                |r| {
                    let taken_str: String = r.get(5)?;
                    let trashed_str: Option<String> = r.get(20)?;
                    Ok((
                        AssetRecord {
                            id: r.get(0)?,
                            kind: str_to_kind(&r.get::<_, String>(1)?),
                            key: r.get(2)?,
                            filename: r.get(3)?,
                            size: r.get::<_, i64>(4)? as u64,
                            taken_at: parse_utc(&taken_str).unwrap_or_else(|_| Utc::now()),
                            tz_offset: r.get(6)?,
                            date_guessed: r.get::<_, i64>(7)? != 0,
                            width: r.get::<_, Option<i64>>(8)?.map(|v| v as u32),
                            height: r.get::<_, Option<i64>>(9)?.map(|v| v as u32),
                            duration_ms: r.get::<_, Option<i64>>(10)?.map(|v| v as u64),
                            container: r.get(11)?,
                            video_codec: r.get(12)?,
                            audio_codec: r.get(13)?,
                            camera_make: r.get(14)?,
                            camera_model: r.get(15)?,
                            motion_id: r.get(16)?,
                            motion_key: r.get(17)?,
                            motion_size: r.get::<_, Option<i64>>(18)?.map(|v| v as u64),
                            thumb_failed: r.get::<_, i64>(19)? != 0,
                        },
                        trashed_str,
                    ))
                },
            )
            .optional()?;

        let Some((record, trashed_str)) = row else {
            return Ok(None);
        };
        let trashed_at = match trashed_str {
            Some(s) => Some(parse_utc(&s)?),
            None => None,
        };

        let mut alb_stmt = self.conn.prepare(
            "SELECT a.id, a.name
             FROM album_items ai
             JOIN albums a ON a.id = ai.album_id
             WHERE ai.asset_id = ?1 AND ai.in_album = 1 AND a.deleted_at IS NULL
             ORDER BY a.name COLLATE NOCASE ASC",
        )?;
        let albums = alb_stmt
            .query_map([id], |r| {
                Ok(AlbumRef {
                    id: r.get(0)?,
                    name: r.get(1)?,
                })
            })?
            .collect::<std::result::Result<Vec<_>, _>>()?;

        Ok(Some(AssetDetail {
            record,
            trashed_at,
            albums,
        }))
    }

    /// List all non-deleted albums with item counts and newest cover photo id
    /// (spec section 9: "Album contents are sorted by date taken; the cover is the newest photo").
    pub fn list_albums(&self) -> Result<Vec<AlbumSummary>> {
        let mut stmt = self.conn.prepare(
            "SELECT a.id, a.name, a.created_at,
                    (SELECT count(*)
                     FROM album_items ai
                     JOIN assets s ON s.id = ai.asset_id
                     WHERE ai.album_id = a.id AND ai.in_album = 1
                       AND s.trashed_at IS NULL AND s.purged_at IS NULL) AS cnt,
                    (SELECT s.id
                     FROM album_items ai
                     JOIN assets s ON s.id = ai.asset_id
                     WHERE ai.album_id = a.id AND ai.in_album = 1
                       AND s.trashed_at IS NULL AND s.purged_at IS NULL
                     ORDER BY s.taken_at DESC, s.id ASC
                     LIMIT 1) AS cover_id
             FROM albums a
             WHERE a.deleted_at IS NULL
             ORDER BY a.name COLLATE NOCASE ASC, a.id ASC",
        )?;
        let rows = stmt.query_map([], |r| {
            let created_str: String = r.get(2)?;
            Ok(AlbumSummary {
                id: r.get(0)?,
                name: r.get(1)?,
                created_at: parse_utc(&created_str).unwrap_or_else(|_| Utc::now()),
                asset_count: r.get::<_, i64>(3)? as usize,
                cover_asset_id: r.get(4)?,
            })
        })?;
        rows.collect::<std::result::Result<Vec<_>, _>>()
            .map_err(Into::into)
    }

    /// List visible assets in an album, sorted by `taken_at DESC, id ASC`.
    pub fn list_album_assets(&self, album_id: &str) -> Result<Vec<AssetSummary>> {
        let mut stmt = self.conn.prepare(
            "SELECT s.id, s.kind, s.key, s.filename, s.size, s.taken_at, s.tz_offset,
                    s.date_guessed, s.width, s.height, s.duration_ms, s.thumb_failed
             FROM album_items ai
             JOIN albums a ON a.id = ai.album_id
             JOIN assets s ON s.id = ai.asset_id
             WHERE ai.album_id = ?1 AND ai.in_album = 1
               AND a.deleted_at IS NULL
               AND s.trashed_at IS NULL AND s.purged_at IS NULL
             ORDER BY s.taken_at DESC, s.id ASC",
        )?;
        let rows = stmt.query_map([album_id], row_to_summary)?;
        rows.collect::<std::result::Result<Vec<_>, _>>()
            .map_err(Into::into)
    }

    /// List trashed (not yet purged) assets with days remaining until the 30-day purge.
    pub fn list_trash(&self, now: DateTime<Utc>) -> Result<Vec<TrashItem>> {
        let mut stmt = self.conn.prepare(
            "SELECT id, kind, key, filename, size, taken_at, tz_offset, date_guessed,
                    width, height, duration_ms, thumb_failed, trashed_at, motion_size
             FROM assets
             WHERE trashed_at IS NOT NULL AND purged_at IS NULL
             ORDER BY trashed_at DESC, id ASC",
        )?;
        let rows = stmt.query_map([], |r| {
            let summary = row_to_summary(r)?;
            let trashed_str: String = r.get(12)?;
            let motion_size: Option<u64> = r.get::<_, Option<i64>>(13)?.map(|v| v as u64);
            let trashed_at = parse_utc(&trashed_str).unwrap_or(now);
            let elapsed_days = now.signed_duration_since(trashed_at).num_days();
            let days_left = (TRASH_RETENTION_DAYS - elapsed_days).max(0);
            Ok(TrashItem {
                asset: summary,
                trashed_at,
                days_left,
                motion_size,
            })
        })?;
        rows.collect::<std::result::Result<Vec<_>, _>>()
            .map_err(Into::into)
    }

    /// Assets trashed more than 30 days ago (`trashed_at <= now - 30 days`) and not yet purged.
    pub fn assets_due_for_purge(&self, now: DateTime<Utc>) -> Result<Vec<PurgeCandidate>> {
        let cutoff = format_utc(now - Duration::days(TRASH_RETENTION_DAYS));
        let mut stmt = self.conn.prepare(
            "SELECT id, key, motion_key
             FROM assets
             WHERE trashed_at IS NOT NULL AND purged_at IS NULL AND trashed_at <= ?1
             ORDER BY trashed_at ASC",
        )?;
        let rows = stmt.query_map([cutoff], |r| {
            Ok(PurgeCandidate {
                id: r.get(0)?,
                key: r.get(1)?,
                motion_key: r.get(2)?,
            })
        })?;
        rows.collect::<std::result::Result<Vec<_>, _>>()
            .map_err(Into::into)
    }

    // =========================================================================
    // Search & Autocomplete Suggestions (Spec Section 9)
    // =========================================================================

    /// Grouped search suggestions for a partial query string (Albums, Dates, Cameras, Types, Filenames).
    pub fn search_suggestions(&self, raw_query: &str) -> Result<SearchSuggestions> {
        let q = raw_query.trim().to_lowercase();
        if q.is_empty() {
            return Ok(SearchSuggestions::default());
        }
        let like_pattern = format!("%{q}%");

        // 1. Albums
        let mut alb_stmt = self.conn.prepare(
            "SELECT id, name FROM albums
             WHERE deleted_at IS NULL AND lower(name) LIKE ?1
             ORDER BY name COLLATE NOCASE ASC LIMIT 5",
        )?;
        let albums = alb_stmt
            .query_map([&like_pattern], |r| {
                Ok(AlbumRef {
                    id: r.get(0)?,
                    name: r.get(1)?,
                })
            })?
            .collect::<std::result::Result<Vec<_>, _>>()?;

        // 2. Dates (distinct YYYY or YYYY-MM matching query)
        let mut date_stmt = self.conn.prepare(
            "SELECT DISTINCT substr(taken_at, 1, 7) AS ym
             FROM assets
             WHERE trashed_at IS NULL AND purged_at IS NULL AND ym LIKE ?1
             ORDER BY ym DESC LIMIT 5",
        )?;
        let dates = date_stmt
            .query_map([&like_pattern], |r| r.get::<_, String>(0))?
            .collect::<std::result::Result<Vec<_>, _>>()?;

        // 3. Cameras
        let mut cam_stmt = self.conn.prepare(
            "SELECT DISTINCT trim(coalesce(camera_make, '') || ' ' || coalesce(camera_model, '')) AS cam
             FROM assets
             WHERE trashed_at IS NULL AND purged_at IS NULL
               AND cam != '' AND lower(cam) LIKE ?1
             ORDER BY cam ASC LIMIT 5",
        )?;
        let cameras = cam_stmt
            .query_map([&like_pattern], |r| r.get::<_, String>(0))?
            .collect::<std::result::Result<Vec<_>, _>>()?;

        // 4. Types (photo, video, live, plus file extensions like heic, jpg, mov)
        let mut types = Vec::new();
        for t in ["photo", "video", "live", "heic", "raw", "jpg", "png", "mov", "mp4", "mkv", "avi"] {
            if t.contains(&q) {
                types.push(t.to_string());
            }
        }

        // 5. Filenames
        let mut fn_stmt = self.conn.prepare(
            "SELECT DISTINCT filename FROM assets
             WHERE trashed_at IS NULL AND purged_at IS NULL AND lower(filename) LIKE ?1
             ORDER BY filename ASC LIMIT 5",
        )?;
        let filenames = fn_stmt
            .query_map([&like_pattern], |r| r.get::<_, String>(0))?
            .collect::<std::result::Result<Vec<_>, _>>()?;

        Ok(SearchSuggestions {
            albums,
            dates,
            cameras,
            file_types: types,
            filenames,
        })
    }

    /// Search active assets combining FTS5 text tokens and structured filter chips.
    pub fn search_assets(&self, filter: &SearchFilter) -> Result<Vec<AssetSummary>> {
        let fts_match = filter
            .text
            .as_deref()
            .and_then(build_fts_prefix_query);

        let mut stmt = if fts_match.is_some() {
            self.conn.prepare(
                "SELECT s.id, s.kind, s.key, s.filename, s.size, s.taken_at, s.tz_offset,
                        s.date_guessed, s.width, s.height, s.duration_ms, s.thumb_failed,
                        s.camera_make, s.camera_model
                 FROM search_fts f
                 JOIN assets s ON s.id = f.asset_id
                 WHERE f.search_fts MATCH ?1
                   AND s.trashed_at IS NULL AND s.purged_at IS NULL
                 ORDER BY s.taken_at DESC, s.id ASC",
            )?
        } else {
            self.conn.prepare(
                "SELECT s.id, s.kind, s.key, s.filename, s.size, s.taken_at, s.tz_offset,
                        s.date_guessed, s.width, s.height, s.duration_ms, s.thumb_failed,
                        s.camera_make, s.camera_model
                 FROM assets s
                 WHERE s.trashed_at IS NULL AND s.purged_at IS NULL
                 ORDER BY s.taken_at DESC, s.id ASC",
            )?
        };

        let album_members: Option<std::collections::HashSet<String>> =
            if let Some(alb_id) = &filter.album_id {
                let mut a_stmt = self.conn.prepare(
                    "SELECT ai.asset_id
                     FROM album_items ai
                     JOIN albums a ON a.id = ai.album_id
                     WHERE ai.album_id = ?1 AND ai.in_album = 1 AND a.deleted_at IS NULL",
                )?;
                let ids = a_stmt
                    .query_map([alb_id], |r| r.get::<_, String>(0))?
                    .collect::<std::result::Result<std::collections::HashSet<_>, _>>()?;
                Some(ids)
            } else {
                None
            };

        let map_row = |r: &rusqlite::Row<'_>| -> rusqlite::Result<(AssetSummary, Option<String>, Option<String>)> {
            Ok((row_to_summary(r)?, r.get(12)?, r.get(13)?))
        };

        let rows: Vec<(AssetSummary, Option<String>, Option<String>)> = if let Some(ref q) = fts_match {
            stmt.query_map([q], map_row)?
                .collect::<std::result::Result<Vec<_>, _>>()?
        } else {
            stmt.query_map([], map_row)?
                .collect::<std::result::Result<Vec<_>, _>>()?
        };

        let mut results = Vec::new();
        for (summary, cam_make, cam_model) in rows {
            if let Some(ref members) = album_members {
                if !members.contains(&summary.id) {
                    continue;
                }
            }
            if let Some(kind) = filter.kind {
                if summary.kind != kind {
                    continue;
                }
            }
            if let Some(ref ext) = filter.file_ext {
                let want = ext.trim_start_matches('.').to_lowercase();
                let actual = file_extension_lower(&summary.filename);
                if actual != want {
                    continue;
                }
            }
            if let Some(ref want_cam) = filter.camera {
                let full_cam = format!(
                    "{} {}",
                    cam_make.as_deref().unwrap_or(""),
                    cam_model.as_deref().unwrap_or("")
                )
                .to_lowercase();
                if !full_cam.contains(&want_cam.to_lowercase()) {
                    continue;
                }
            }
            if filter.year.is_some() || filter.month.is_some() {
                let local_dt = match summary.tz_offset {
                    Some(m) => shift_utc_minutes(summary.taken_at, m),
                    None => summary.taken_at,
                };
                if let Some(y) = filter.year {
                    if local_dt.year() != y {
                        continue;
                    }
                }
                if let Some(mo) = filter.month {
                    if local_dt.month() != mo {
                        continue;
                    }
                }
            }
            results.push(summary);
        }

        Ok(results)
    }

    // =========================================================================
    // Import Queue Persistence (Spec Section 5 & 7)
    // =========================================================================

    pub fn enqueue_import(&mut self, item: &ImportQueueRow) -> Result<()> {
        self.conn.execute(
            "INSERT INTO import_queue (
                path, size, mtime, state, asset_id, upload_id, parts_done, album_id, error
             ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)
             ON CONFLICT(path) DO UPDATE SET
                size = excluded.size,
                mtime = excluded.mtime,
                state = excluded.state,
                asset_id = excluded.asset_id,
                upload_id = excluded.upload_id,
                parts_done = excluded.parts_done,
                album_id = excluded.album_id,
                error = excluded.error",
            params![
                item.path,
                item.size as i64,
                item.mtime,
                item.state.as_str(),
                item.asset_id,
                item.upload_id,
                item.parts_done as i64,
                item.album_id,
                item.error,
            ],
        )?;
        Ok(())
    }

    pub fn update_import_state(
        &mut self,
        path: &str,
        state: ImportState,
        asset_id: Option<&str>,
        upload_id: Option<&str>,
        parts_done: u32,
        error: Option<&str>,
    ) -> Result<()> {
        self.conn.execute(
            "UPDATE import_queue
             SET state = ?2, asset_id = ?3, upload_id = ?4, parts_done = ?5, error = ?6
             WHERE path = ?1",
            params![
                path,
                state.as_str(),
                asset_id,
                upload_id,
                parts_done as i64,
                error,
            ],
        )?;
        Ok(())
    }

    pub fn get_import_queue_row(&self, path: &str) -> Result<Option<ImportQueueRow>> {
        self.conn
            .query_row(
                "SELECT path, size, mtime, state, asset_id, upload_id, parts_done, album_id, error
                 FROM import_queue WHERE path = ?1",
                [path],
                |r| {
                    Ok(ImportQueueRow {
                        path: r.get(0)?,
                        size: r.get::<_, i64>(1)? as u64,
                        mtime: r.get(2)?,
                        state: ImportState::from_str(&r.get::<_, String>(3)?),
                        asset_id: r.get(4)?,
                        upload_id: r.get(5)?,
                        parts_done: r.get::<_, i64>(6)? as u32,
                        album_id: r.get(7)?,
                        error: r.get(8)?,
                    })
                },
            )
            .optional()
            .map_err(Into::into)
    }

    /// Reconcile a scanned file against `import_queue` so interrupted imports
    /// resume from `import_queue` (`logged`, `skipped`, `uploading` with `upload_id`
    /// and `parts_done`, or `hashed`) if `size` and `mtime` match (spec section 5 & 7).
    pub fn reconcile_import_candidate(
        &mut self,
        path: &str,
        size: u64,
        mtime: i64,
        album_id: Option<&str>,
    ) -> Result<ImportQueueRow> {
        if let Some(mut existing) = self.get_import_queue_row(path)? {
            if existing.size == size
                && existing.mtime == mtime
                && existing.state != ImportState::Failed
            {
                if album_id.is_some() && existing.album_id.is_none() {
                    existing.album_id = album_id.map(str::to_string);
                    self.enqueue_import(&existing)?;
                }
                return Ok(existing);
            }
        }
        let fresh = ImportQueueRow {
            path: path.to_string(),
            size,
            mtime,
            state: ImportState::Pending,
            asset_id: None,
            upload_id: None,
            parts_done: 0,
            album_id: album_id.map(str::to_string),
            error: None,
        };
        self.enqueue_import(&fresh)?;
        Ok(fresh)
    }

    /// Reconcile a batch of `(path, size, mtime)` candidates in a single SQLite
    /// transaction so scanning 850+ files takes milliseconds instead of 850 disk fsyncs.
    pub fn reconcile_import_candidates_batch(
        &mut self,
        candidates: &[(String, u64, i64)],
        album_id: Option<&str>,
    ) -> Result<Vec<ImportQueueRow>> {
        self.conn.execute_batch("BEGIN IMMEDIATE;")?;
        let mut out = Vec::with_capacity(candidates.len());
        for (path, size, mtime) in candidates {
            match self.reconcile_import_candidate(path, *size, *mtime, album_id) {
                Ok(row) => out.push(row),
                Err(e) => {
                    let _ = self.conn.execute_batch("ROLLBACK;");
                    return Err(e);
                }
            }
        }
        self.conn.execute_batch("COMMIT;")?;
        Ok(out)
    }

    pub fn list_import_queue(&self) -> Result<Vec<ImportQueueRow>> {
        let mut stmt = self.conn.prepare(
            "SELECT path, size, mtime, state, asset_id, upload_id, parts_done, album_id, error
             FROM import_queue ORDER BY path ASC",
        )?;
        let rows = stmt.query_map([], |r| {
            Ok(ImportQueueRow {
                path: r.get(0)?,
                size: r.get::<_, i64>(1)? as u64,
                mtime: r.get(2)?,
                state: ImportState::from_str(&r.get::<_, String>(3)?),
                asset_id: r.get(4)?,
                upload_id: r.get(5)?,
                parts_done: r.get::<_, i64>(6)? as u32,
                album_id: r.get(7)?,
                error: r.get(8)?,
            })
        })?;
        rows.collect::<std::result::Result<Vec<_>, _>>()
            .map_err(Into::into)
    }

    pub fn import_progress(&self) -> Result<ImportProgress> {
        let mut stmt = self
            .conn
            .prepare("SELECT size, state, parts_done FROM import_queue")?;
        let rows = stmt.query_map([], |r| {
            Ok((
                r.get::<_, i64>(0)? as u64,
                ImportState::from_str(&r.get::<_, String>(1)?),
                r.get::<_, i64>(2)? as u32,
            ))
        })?;
        let mut prog = ImportProgress::default();
        for row_res in rows {
            let (size, state, parts_done) = row_res?;
            prog.total_files += 1;
            prog.total_bytes += size;
            match state {
                ImportState::Hashed => {
                    prog.hashed_files += 1;
                }
                ImportState::Uploading => {
                    prog.hashed_files += 1;
                    prog.uploading_files += 1;
                    let partial = (u64::from(parts_done) * 16 * 1024 * 1024).min(size);
                    prog.completed_bytes += partial;
                }
                ImportState::Uploaded => {
                    prog.hashed_files += 1;
                    prog.completed_bytes += size;
                }
                ImportState::Logged => {
                    prog.hashed_files += 1;
                    prog.completed_files += 1;
                    prog.completed_bytes += size;
                }
                ImportState::Skipped => {
                    prog.hashed_files += 1;
                    prog.completed_files += 1;
                    prog.completed_bytes += size;
                    prog.skipped_duplicates += 1;
                }
                ImportState::Failed => {
                    prog.failed_files += 1;
                }
                ImportState::Pending => {}
            }
        }
        Ok(prog)
    }

    pub fn clear_finished_imports(&mut self) -> Result<()> {
        self.conn.execute(
            "DELETE FROM import_queue WHERE state IN ('logged', 'skipped')",
            [],
        )?;
        Ok(())
    }
}

// =============================================================================
// Internal Merge & FTS Helpers
// =============================================================================

fn row_to_summary(r: &rusqlite::Row<'_>) -> rusqlite::Result<AssetSummary> {
    let taken_str: String = r.get(5)?;
    Ok(AssetSummary {
        id: r.get(0)?,
        kind: str_to_kind(&r.get::<_, String>(1)?),
        key: r.get(2)?,
        filename: r.get(3)?,
        size: r.get::<_, i64>(4)? as u64,
        taken_at: parse_utc(&taken_str).unwrap_or_else(|_| Utc::now()),
        tz_offset: r.get(6)?,
        date_guessed: r.get::<_, i64>(7)? != 0,
        width: r.get::<_, Option<i64>>(8)?.map(|v| v as u32),
        height: r.get::<_, Option<i64>>(9)?.map(|v| v as u32),
        duration_ms: r.get::<_, Option<i64>>(10)?.map(|v| v as u64),
        thumb_failed: r.get::<_, i64>(11)? != 0,
    })
}

fn increment_batches_since_snapshot(tx: &Transaction<'_>) -> Result<()> {
    let cur: String = tx
        .query_row(
            "SELECT value FROM local_meta WHERE key = ?1",
            [META_BATCHES_SINCE_SNAPSHOT],
            |r| r.get(0),
        )
        .optional()?
        .unwrap_or_else(|| "0".to_string());
    let next = cur.parse::<u64>().unwrap_or(0) + 1;
    tx.execute(
        "INSERT INTO local_meta (key, value) VALUES (?1, ?2)
         ON CONFLICT(key) DO UPDATE SET value = excluded.value",
        params![META_BATCHES_SINCE_SNAPSHOT, next.to_string()],
    )?;
    Ok(())
}

/// Apply one `Op` inside an active SQLite transaction according to spec section 6.
fn apply_op_in_tx(
    tx: &Transaction<'_>,
    op: &Op,
    _batch_at: DateTime<Utc>,
    batch_at_str: &str,
    stamp: &str,
) -> Result<()> {
    match op {
        Op::AssetAdd { asset } => {
            let existing: Option<(String, String, Option<String>)> = tx
                .query_row(
                    "SELECT state_at, date_at, purged_at FROM assets WHERE id = ?1",
                    [&asset.id],
                    |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
                )
                .optional()?;

            match existing {
                None => {
                    tx.execute(
                        "INSERT INTO assets (
                            id, kind, key, filename, size, taken_at, tz_offset, date_guessed,
                            date_at, width, height, duration_ms, container, video_codec, audio_codec,
                            camera_make, camera_model, motion_id, motion_key, motion_size,
                            thumb_failed, trashed_at, purged_at, state_at
                        ) VALUES (
                            ?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8,
                            ?9, ?10, ?11, ?12, ?13, ?14, ?15,
                            ?16, ?17, ?18, ?19, ?20,
                            ?21, NULL, NULL, ?22
                        )",
                        params![
                            asset.id,
                            kind_to_str(asset.kind),
                            asset.key,
                            asset.filename,
                            asset.size as i64,
                            format_utc(asset.taken_at),
                            asset.tz_offset,
                            i32::from(asset.date_guessed),
                            stamp,
                            asset.width.map(i64::from),
                            asset.height.map(i64::from),
                            asset.duration_ms.map(|v| v as i64),
                            asset.container,
                            asset.video_codec,
                            asset.audio_codec,
                            asset.camera_make,
                            asset.camera_model,
                            asset.motion_id,
                            asset.motion_key,
                            asset.motion_size.map(|v| v as i64),
                            i32::from(asset.thumb_failed),
                            stamp,
                        ],
                    )?;
                    refresh_fts_for_asset(tx, &asset.id)?;
                }
                Some((state_at, date_at, purged_at)) => {
                    // Spec: "asset.add: idempotent. After a purge, a newer asset.add brings the asset back (re-import)."
                    if purged_at.is_some() && stamp > state_at.as_str() {
                        let (new_taken, new_date_at) = if stamp >= date_at.as_str() {
                            (format_utc(asset.taken_at), stamp.to_string())
                        } else {
                            let old_taken: String = tx.query_row(
                                "SELECT taken_at FROM assets WHERE id = ?1",
                                [&asset.id],
                                |r| r.get(0),
                            )?;
                            (old_taken, date_at)
                        };
                        tx.execute(
                            "UPDATE assets SET
                                kind = ?2, key = ?3, filename = ?4, size = ?5,
                                taken_at = ?6, tz_offset = ?7, date_guessed = ?8, date_at = ?9,
                                width = ?10, height = ?11, duration_ms = ?12,
                                container = ?13, video_codec = ?14, audio_codec = ?15,
                                camera_make = ?16, camera_model = ?17,
                                motion_id = ?18, motion_key = ?19, motion_size = ?20,
                                thumb_failed = ?21,
                                trashed_at = NULL, purged_at = NULL, state_at = ?22
                             WHERE id = ?1",
                            params![
                                asset.id,
                                kind_to_str(asset.kind),
                                asset.key,
                                asset.filename,
                                asset.size as i64,
                                new_taken,
                                asset.tz_offset,
                                i32::from(asset.date_guessed),
                                new_date_at,
                                asset.width.map(i64::from),
                                asset.height.map(i64::from),
                                asset.duration_ms.map(|v| v as i64),
                                asset.container,
                                asset.video_codec,
                                asset.audio_codec,
                                asset.camera_make,
                                asset.camera_model,
                                asset.motion_id,
                                asset.motion_key,
                                asset.motion_size.map(|v| v as i64),
                                i32::from(asset.thumb_failed),
                                stamp,
                            ],
                        )?;
                        refresh_fts_for_asset(tx, &asset.id)?;
                    }
                }
            }
        }

        Op::AssetSetDate { id, taken_at } => {
            let row: Option<(String, Option<String>)> = tx
                .query_row(
                    "SELECT date_at, purged_at FROM assets WHERE id = ?1",
                    [id],
                    |r| Ok((r.get(0)?, r.get(1)?)),
                )
                .optional()?;
            if let Some((date_at, None)) = row {
                if stamp >= date_at.as_str() {
                    tx.execute(
                        "UPDATE assets SET taken_at = ?2, date_guessed = 0, date_at = ?3 WHERE id = ?1",
                        params![id, format_utc(*taken_at), stamp],
                    )?;
                }
            }
        }

        Op::AssetTrash { ids } => {
            for id in ids {
                let row: Option<(String, Option<String>)> = tx
                    .query_row(
                        "SELECT state_at, purged_at FROM assets WHERE id = ?1",
                        [id],
                        |r| Ok((r.get(0)?, r.get(1)?)),
                    )
                    .optional()?;
                if let Some((state_at, None)) = row {
                    if stamp >= state_at.as_str() {
                        tx.execute(
                            "UPDATE assets SET trashed_at = ?2, state_at = ?3 WHERE id = ?1",
                            params![id, batch_at_str, stamp],
                        )?;
                        delete_fts_for_asset(tx, id)?;
                    }
                }
            }
        }

        Op::AssetRestore { ids } => {
            for id in ids {
                let row: Option<(String, Option<String>)> = tx
                    .query_row(
                        "SELECT state_at, purged_at FROM assets WHERE id = ?1",
                        [id],
                        |r| Ok((r.get(0)?, r.get(1)?)),
                    )
                    .optional()?;
                if let Some((state_at, None)) = row {
                    if stamp >= state_at.as_str() {
                        tx.execute(
                            "UPDATE assets SET trashed_at = NULL, state_at = ?2 WHERE id = ?1",
                            params![id, stamp],
                        )?;
                        refresh_fts_for_asset(tx, id)?;
                    }
                }
            }
        }

        Op::AssetPurge { ids } => {
            for id in ids {
                let row: Option<String> = tx
                    .query_row("SELECT state_at FROM assets WHERE id = ?1", [id], |r| {
                        r.get(0)
                    })
                    .optional()?;
                if let Some(state_at) = row {
                    if stamp >= state_at.as_str() {
                        tx.execute(
                            "UPDATE assets SET purged_at = ?2, state_at = ?3 WHERE id = ?1",
                            params![id, batch_at_str, stamp],
                        )?;
                        delete_fts_for_asset(tx, id)?;
                    }
                }
            }
        }

        Op::AlbumCreate { id, name } => {
            let existing: Option<(String, Option<String>)> = tx
                .query_row(
                    "SELECT name_at, deleted_at FROM albums WHERE id = ?1",
                    [id],
                    |r| Ok((r.get(0)?, r.get(1)?)),
                )
                .optional()?;
            match existing {
                None => {
                    tx.execute(
                        "INSERT INTO albums (id, name, name_at, created_at, deleted_at)
                         VALUES (?1, ?2, ?3, ?4, NULL)",
                        params![id, name, stamp, batch_at_str],
                    )?;
                }
                Some((name_at, None)) => {
                    if stamp >= name_at.as_str() {
                        tx.execute(
                            "UPDATE albums SET name = ?2, name_at = ?3, created_at = ?4 WHERE id = ?1",
                            params![id, name, stamp, batch_at_str],
                        )?;
                    }
                }
                Some((_, Some(_))) => {}
            }
        }

        Op::AlbumRename { id, name } => {
            let row: Option<(String, Option<String>)> = tx
                .query_row(
                    "SELECT name_at, deleted_at FROM albums WHERE id = ?1",
                    [id],
                    |r| Ok((r.get(0)?, r.get(1)?)),
                )
                .optional()?;
            if let Some((name_at, None)) = row {
                if stamp >= name_at.as_str() {
                    tx.execute(
                        "UPDATE albums SET name = ?2, name_at = ?3 WHERE id = ?1",
                        params![id, name, stamp],
                    )?;
                    refresh_fts_for_album_members(tx, id)?;
                }
            }
        }

        Op::AlbumDelete { id } => {
            // Spec: "album.delete: final. Later ops on that album are ignored. Its photos are untouched."
            let exists: Option<Option<String>> = tx
                .query_row("SELECT deleted_at FROM albums WHERE id = ?1", [id], |r| {
                    r.get(0)
                })
                .optional()?;
            match exists {
                None => {
                    tx.execute(
                        "INSERT INTO albums (id, name, name_at, created_at, deleted_at)
                         VALUES (?1, '', ?2, ?3, ?3)",
                        params![id, stamp, batch_at_str],
                    )?;
                }
                Some(None) => {
                    tx.execute(
                        "UPDATE albums SET deleted_at = ?2, name_at = ?3 WHERE id = ?1",
                        params![id, batch_at_str, stamp],
                    )?;
                    refresh_fts_for_album_members(tx, id)?;
                }
                Some(Some(_)) => {}
            }
        }

        Op::AlbumAdd { id, ids } => {
            apply_album_membership(tx, id, ids, true, stamp)?;
        }

        Op::AlbumRemove { id, ids } => {
            apply_album_membership(tx, id, ids, false, stamp)?;
        }

        Op::Unknown => {}
    }
    Ok(())
}

fn apply_album_membership(
    tx: &Transaction<'_>,
    album_id: &str,
    asset_ids: &[String],
    in_album: bool,
    stamp: &str,
) -> Result<()> {
    // Ignore if album does not exist or is deleted
    let alb_deleted: Option<Option<String>> = tx
        .query_row(
            "SELECT deleted_at FROM albums WHERE id = ?1",
            [album_id],
            |r| r.get(0),
        )
        .optional()?;
    let Some(None) = alb_deleted else {
        return Ok(());
    };

    for asset_id in asset_ids {
        // Ignore if asset is unknown or purged
        let asset_purged: Option<Option<String>> = tx
            .query_row(
                "SELECT purged_at FROM assets WHERE id = ?1",
                [asset_id],
                |r| r.get(0),
            )
            .optional()?;
        let Some(None) = asset_purged else {
            continue;
        };

        let existing_at: Option<String> = tx
            .query_row(
                "SELECT at FROM album_items WHERE album_id = ?1 AND asset_id = ?2",
                params![album_id, asset_id],
                |r| r.get(0),
            )
            .optional()?;

        if existing_at.as_deref().is_none_or(|old| stamp >= old) {
            tx.execute(
                "INSERT INTO album_items (album_id, asset_id, in_album, at)
                 VALUES (?1, ?2, ?3, ?4)
                 ON CONFLICT(album_id, asset_id) DO UPDATE SET
                    in_album = excluded.in_album,
                    at = excluded.at",
                params![album_id, asset_id, i32::from(in_album), stamp],
            )?;
            refresh_fts_for_asset(tx, asset_id)?;
        }
    }
    Ok(())
}

fn delete_fts_for_asset(tx: &Transaction<'_>, asset_id: &str) -> Result<()> {
    tx.execute("DELETE FROM search_fts WHERE asset_id = ?1", [asset_id])?;
    Ok(())
}

fn refresh_fts_for_album_members(tx: &Transaction<'_>, album_id: &str) -> Result<()> {
    let mut stmt = tx.prepare("SELECT asset_id FROM album_items WHERE album_id = ?1")?;
    let ids = stmt
        .query_map([album_id], |r| r.get::<_, String>(0))?
        .collect::<std::result::Result<Vec<_>, _>>()?;
    drop(stmt);
    for id in ids {
        refresh_fts_for_asset(tx, &id)?;
    }
    Ok(())
}

type FtsAssetRow = (
    String,
    String,
    Option<String>,
    Option<String>,
    Option<String>,
    Option<String>,
);

fn refresh_fts_for_asset(tx: &Transaction<'_>, asset_id: &str) -> Result<()> {
    delete_fts_for_asset(tx, asset_id)?;

    let row: Option<FtsAssetRow> = tx
        .query_row(
            "SELECT kind, filename, camera_make, camera_model, trashed_at, purged_at
             FROM assets WHERE id = ?1",
            [asset_id],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?, r.get(5)?)),
        )
        .optional()?;

    let Some((kind_str, filename, cam_make, cam_model, None, None)) = row else {
        return Ok(());
    };

    let camera = format!(
        "{} {}",
        cam_make.as_deref().unwrap_or(""),
        cam_model.as_deref().unwrap_or("")
    )
    .trim()
    .to_string();

    let ext = file_extension_lower(&filename);
    let raw_word = if matches!(ext.as_str(), "dng" | "cr2" | "cr3" | "nef" | "arw") {
        " raw"
    } else {
        ""
    };
    let file_type = format!("{kind_str} {ext}{raw_word}").trim().to_string();

    let mut alb_stmt = tx.prepare(
        "SELECT a.name
         FROM album_items ai
         JOIN albums a ON a.id = ai.album_id
         WHERE ai.asset_id = ?1 AND ai.in_album = 1 AND a.deleted_at IS NULL",
    )?;
    let album_names = alb_stmt
        .query_map([asset_id], |r| r.get::<_, String>(0))?
        .collect::<std::result::Result<Vec<_>, _>>()?
        .join(" ");
    drop(alb_stmt);

    tx.execute(
        "INSERT INTO search_fts (asset_id, filename, camera, file_type, albums)
         VALUES (?1, ?2, ?3, ?4, ?5)",
        params![asset_id, filename, camera, file_type, album_names],
    )?;
    Ok(())
}

fn file_extension_lower(filename: &str) -> String {
    match filename.rfind('.') {
        Some(i) if i > 0 && i + 1 < filename.len() => filename[i + 1..].to_lowercase(),
        _ => String::new(),
    }
}

fn build_fts_prefix_query(text: &str) -> Option<String> {
    let tokens: Vec<String> = text
        .split(|c: char| !c.is_alphanumeric())
        .filter(|s| !s.is_empty())
        .map(|s| format!("\"{}\"*", s.to_lowercase()))
        .collect();
    if tokens.is_empty() {
        None
    } else {
        Some(tokens.join(" AND "))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;

    fn t(sec: i64) -> DateTime<Utc> {
        Utc.timestamp_opt(1_730_000_000 + sec, 0).unwrap()
    }

    fn sample_asset(id: &str, filename: &str, taken_sec: i64) -> AssetRecord {
        AssetRecord {
            id: id.to_string(),
            kind: AssetKind::Photo,
            key: format!("originals/2024/10/{filename}"),
            filename: filename.to_string(),
            size: 1_000_000,
            taken_at: t(taken_sec),
            tz_offset: Some(0),
            date_guessed: false,
            width: Some(4032),
            height: Some(3024),
            duration_ms: None,
            container: None,
            video_codec: None,
            audio_codec: None,
            camera_make: Some("Apple".to_string()),
            camera_model: Some("iPhone 15 Pro".to_string()),
            motion_id: None,
            motion_key: None,
            motion_size: None,
            thumb_failed: false,
        }
    }

    #[test]
    fn merge_two_devices_adding_to_same_album_both_survive() {
        let mut store = Store::open_in_memory("mac-1").unwrap();
        store.add_asset(sample_asset("a1", "IMG_1.HEIC", 10), None, t(1)).unwrap();
        store.add_asset(sample_asset("a2", "IMG_2.HEIC", 20), None, t(2)).unwrap();
        store.create_album("alb1".into(), "Bali 2024".into(), t(3)).unwrap();

        // Device 1 adds a1 at t(10), Device 2 adds a2 at t(11)
        let batch_dev1 = Batch {
            device: "dev-a".into(),
            seq: 1,
            at: t(10),
            ops: vec![Op::AlbumAdd {
                id: "alb1".into(),
                ids: vec!["a1".into()],
            }],
        };
        let batch_dev2 = Batch {
            device: "dev-b".into(),
            seq: 1,
            at: t(11),
            ops: vec![Op::AlbumAdd {
                id: "alb1".into(),
                ids: vec!["a2".into()],
            }],
        };

        assert!(store.apply_remote_batch(&batch_dev2).unwrap());
        assert!(store.apply_remote_batch(&batch_dev1).unwrap());

        let items = store.list_album_assets("alb1").unwrap();
        assert_eq!(items.len(), 2);
        assert_eq!(items[0].id, "a2"); // newest first
        assert_eq!(items[1].id, "a1");
    }

    #[test]
    fn merge_album_rename_clash_latest_wins_with_device_tiebreaker() {
        let mut store = Store::open_in_memory("mac-1").unwrap();
        store.create_album("alb1".into(), "Initial".into(), t(1)).unwrap();

        // Same timestamp t(20), device "win-b" > "mac-a"
        let b_mac = Batch {
            device: "mac-a".into(),
            seq: 1,
            at: t(20),
            ops: vec![Op::AlbumRename {
                id: "alb1".into(),
                name: "From Mac".into(),
            }],
        };
        let b_win = Batch {
            device: "win-b".into(),
            seq: 1,
            at: t(20),
            ops: vec![Op::AlbumRename {
                id: "alb1".into(),
                name: "From Win".into(),
            }],
        };

        // Regardless of arrival order, "win-b" wins tiebreaker
        store.apply_remote_batch(&b_win).unwrap();
        store.apply_remote_batch(&b_mac).unwrap();

        let albums = store.list_albums().unwrap();
        assert_eq!(albums.len(), 1);
        assert_eq!(albums[0].name, "From Win");
    }

    #[test]
    fn merge_album_delete_is_final_and_keeps_photos() {
        let mut store = Store::open_in_memory("mac-1").unwrap();
        store.add_asset(sample_asset("a1", "IMG_1.HEIC", 10), None, t(1)).unwrap();
        store.create_album("alb1".into(), "Trip".into(), t(2)).unwrap();
        store.add_to_album("alb1", vec!["a1".into()], t(3)).unwrap();
        store.delete_album("alb1", t(10)).unwrap();

        // Later ops on deleted album are ignored
        let later_batch = Batch {
            device: "win-1".into(),
            seq: 1,
            at: t(50),
            ops: vec![
                Op::AlbumRename {
                    id: "alb1".into(),
                    name: "Resurrected".into(),
                },
                Op::AlbumAdd {
                    id: "alb1".into(),
                    ids: vec!["a1".into()],
                },
            ],
        };
        store.apply_remote_batch(&later_batch).unwrap();

        assert!(store.list_albums().unwrap().is_empty());
        // Photo itself is untouched
        assert_eq!(store.list_timeline(None, None).unwrap().len(), 1);
    }

    #[test]
    fn merge_purge_then_reimport_brings_asset_back() {
        let mut store = Store::open_in_memory("mac-1").unwrap();
        let asset = sample_asset("a1", "IMG_1.HEIC", 10);
        store.add_asset(asset.clone(), None, t(1)).unwrap();
        store.trash_assets(vec!["a1".into()], t(5)).unwrap();
        store.purge_assets(vec!["a1".into()], t(10)).unwrap();

        assert!(!store.has_active_or_trashed_asset("a1").unwrap());
        assert!(store.list_timeline(None, None).unwrap().is_empty());

        // Older add before purge is ignored
        let stale_add = Batch {
            device: "win-1".into(),
            seq: 1,
            at: t(8),
            ops: vec![Op::AssetAdd { asset: asset.clone() }],
        };
        store.apply_remote_batch(&stale_add).unwrap();
        assert!(!store.has_active_or_trashed_asset("a1").unwrap());

        // Newer add after purge brings asset back
        let reimport = Batch {
            device: "win-1".into(),
            seq: 2,
            at: t(20),
            ops: vec![Op::AssetAdd { asset }],
        };
        store.apply_remote_batch(&reimport).unwrap();
        assert!(store.has_active_or_trashed_asset("a1").unwrap());
        assert_eq!(store.list_timeline(None, None).unwrap().len(), 1);
    }

    #[test]
    fn outbox_batching_and_flushing_advances_seq() {
        let mut store = Store::open_in_memory("mac-1").unwrap();
        for i in 0..5 {
            store
                .add_asset(sample_asset(&format!("a{i}"), &format!("IMG_{i}.JPG"), i), None, t(i))
                .unwrap();
        }
        assert_eq!(store.outbox_len().unwrap(), 5);

        // Pack at most 2 assets per batch
        let p1 = store.peek_outbox_batch(2, t(100)).unwrap().unwrap();
        assert_eq!(p1.batch.seq, 1);
        assert_eq!(p1.batch.ops.len(), 2);
        store.mark_outbox_flushed(&p1).unwrap();
        assert_eq!(store.outbox_len().unwrap(), 3);
        assert_eq!(store.applied_seq("mac-1").unwrap(), 1);
        assert_eq!(store.next_seq().unwrap(), 2);

        let p2 = store.peek_outbox_batch(100, t(101)).unwrap().unwrap();
        assert_eq!(p2.batch.seq, 2);
        assert_eq!(p2.batch.ops.len(), 3);
        store.mark_outbox_flushed(&p2).unwrap();
        assert_eq!(store.outbox_len().unwrap(), 0);
    }

    #[test]
    fn snapshot_export_and_import_round_trip() {
        let mut store1 = Store::open_in_memory("mac-1").unwrap();
        store1
            .add_asset(sample_asset("a1", "IMG_1.HEIC", 10), None, t(1))
            .unwrap();
        store1.create_album("alb1".into(), "Bali".into(), t(2)).unwrap();
        store1.add_to_album("alb1", vec!["a1".into()], t(3)).unwrap();
        let pending = store1.peek_outbox_batch(100, t(4)).unwrap().unwrap();
        store1.mark_outbox_flushed(&pending).unwrap();

        let (key, gz) = store1.export_snapshot_gz(t(50)).unwrap();
        assert!(key.starts_with("index/snapshot-"));
        assert!(key.ends_with("-mac-1.json.gz"));

        let mut store2 = Store::open_in_memory("win-2").unwrap();
        store2.import_snapshot_gz(&gz).unwrap();

        assert_eq!(store2.applied_seq("mac-1").unwrap(), 1);
        assert_eq!(store2.list_timeline(None, None).unwrap().len(), 1);
        let albums = store2.list_albums().unwrap();
        assert_eq!(albums.len(), 1);
        assert_eq!(albums[0].name, "Bali");
        assert_eq!(albums[0].cover_asset_id.as_deref(), Some("a1"));

        // Search FTS works immediately on imported snapshot
        let found = store2
            .search_assets(&SearchFilter {
                text: Some("bali iphone".into()),
                ..Default::default()
            })
            .unwrap();
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].id, "a1");
    }

    #[test]
    fn trash_retention_and_30_day_purge_candidates() {
        let mut store = Store::open_in_memory("mac-1").unwrap();
        store.add_asset(sample_asset("old", "OLD.JPG", 1), None, t(1)).unwrap();
        store.add_asset(sample_asset("recent", "NEW.JPG", 2), None, t(2)).unwrap();

        let day0 = t(100);
        let day25 = day0 + Duration::days(25);
        let day31 = day0 + Duration::days(31);

        store.trash_assets(vec!["old".into()], day0).unwrap();
        store.trash_assets(vec!["recent".into()], day25).unwrap();

        let trash_list = store.list_trash(day25).unwrap();
        assert_eq!(trash_list.len(), 2);

        let due_at_25 = store.assets_due_for_purge(day25).unwrap();
        assert!(due_at_25.is_empty());

        let due_at_31 = store.assets_due_for_purge(day31).unwrap();
        assert_eq!(due_at_31.len(), 1);
        assert_eq!(due_at_31[0].id, "old");
    }

    #[test]
    fn local_roots_setting_and_album_matching() {
        let mut store = Store::open_in_memory("mac-1").unwrap();
        let now = t(1);
        store.create_album("alb-1".into(), "Summer Vacation".into(), now).unwrap();
        store.create_album("alb-2".into(), "Trip 2024".into(), now).unwrap();

        let temp_dir = std::env::temp_dir().join(format!("frames_test_roots_{}", fastrand::u64(..)));
        std::fs::create_dir_all(&temp_dir).unwrap();
        let sub1 = temp_dir.join("summer vacation");
        let sub2 = temp_dir.join("TRIP 2024");
        let sub3 = temp_dir.join("Unmatched Folder");
        std::fs::create_dir_all(&sub1).unwrap();
        std::fs::create_dir_all(&sub2).unwrap();
        std::fs::create_dir_all(&sub3).unwrap();

        let root_str = temp_dir.to_str().unwrap().to_string();
        store.set_local_roots(vec![root_str.clone()]).unwrap();

        let stored_roots = store.get_local_roots().unwrap();
        assert_eq!(stored_roots, vec![root_str]);

        let matches = store.match_local_albums().unwrap();
        assert_eq!(matches.len(), 2);

        let mut matched_names: Vec<String> = matches.into_iter().map(|m| m.album_name).collect();
        matched_names.sort();
        assert_eq!(matched_names, vec!["Summer Vacation", "Trip 2024"]);

        let _ = std::fs::remove_dir_all(&temp_dir);
    }
}
