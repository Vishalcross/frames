//! 30-day Trash auto-purge, Delete Now, and Empty Trash (spec sections 3, 9, 11):
//! - Only the trash purge deletes S4 objects, and only that asset's original,
//!   motion file (for Live Photos), and thumbnails (`s.webp`, `m.webp`).
//! - Any open device runs the purge daily for items trashed more than 30 days ago.
//! - Deleting twice is harmless (404s on S4 `DELETE` are ignored).
//! - After the S4 objects are removed, `asset.purge` is recorded in SQLite and
//!   queued to `outbox` so other devices apply the purge tombstone.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::keys::{thumb_preview_key, thumb_small_key};
use crate::s4::{ObjectStore, S4Error};
use crate::store::{PurgeCandidate, Store, StoreError};

#[derive(Debug, Error)]
pub enum TrashError {
    #[error("store error: {0}")]
    Store(#[from] StoreError),
    #[error("s4 error: {0}")]
    S4(#[from] S4Error),
}

pub type Result<T> = std::result::Result<T, TrashError>;

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct TrashSummary {
    pub item_count: usize,
    pub total_bytes: u64,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct PurgeReport {
    pub purged_asset_ids: Vec<String>,
    pub s4_objects_deleted: usize,
}

/// Summary of Trash contents (count and total bytes including Live Photo motion files)
/// used for the "Delete now" / "Empty Trash" confirmation dialog (spec section 9).
pub fn trash_summary(store: &Store, now: DateTime<Utc>) -> Result<TrashSummary> {
    let items = store.list_trash(now)?;
    let mut total_bytes = 0u64;
    for item in &items {
        total_bytes += item.asset.size + item.motion_size.unwrap_or(0);
    }
    Ok(TrashSummary {
        item_count: items.len(),
        total_bytes,
    })
}

/// Run the daily 30-day auto-purge (spec section 11):
/// Finds all assets trashed >= 30 days ago, deletes their original, motion file,
/// and thumbnails (`s.webp`, `m.webp`) from S4, and logs `asset.purge`.
pub fn purge_expired_trash(
    store: &mut Store,
    s4: &dyn ObjectStore,
    now: DateTime<Utc>,
) -> Result<PurgeReport> {
    let candidates = store.assets_due_for_purge(now)?;
    purge_candidates(store, s4, &candidates, now)
}

/// Immediately purge specific trashed assets ("Delete now" in Trash view, spec section 9).
pub fn delete_trash_items_now(
    store: &mut Store,
    s4: &dyn ObjectStore,
    asset_ids: &[String],
    now: DateTime<Utc>,
) -> Result<PurgeReport> {
    let mut candidates = Vec::new();
    for id in asset_ids {
        if let Some(detail) = store.get_asset(id)? {
            if detail.trashed_at.is_some() {
                candidates.push(PurgeCandidate {
                    id: detail.record.id,
                    key: detail.record.key,
                    motion_key: detail.record.motion_key,
                });
            }
        }
    }
    purge_candidates(store, s4, &candidates, now)
}

/// Immediately purge all items currently in the Trash ("Empty Trash", spec section 9).
pub fn empty_trash(
    store: &mut Store,
    s4: &dyn ObjectStore,
    now: DateTime<Utc>,
) -> Result<PurgeReport> {
    let items = store.list_trash(now)?;
    let ids: Vec<String> = items.into_iter().map(|i| i.asset.id).collect();
    delete_trash_items_now(store, s4, &ids, now)
}

fn purge_candidates(
    store: &mut Store,
    s4: &dyn ObjectStore,
    candidates: &[PurgeCandidate],
    now: DateTime<Utc>,
) -> Result<PurgeReport> {
    if candidates.is_empty() {
        return Ok(PurgeReport::default());
    }

    let mut purged_ids = Vec::with_capacity(candidates.len());
    let mut s4_deleted = 0usize;

    for cand in candidates {
        // Delete original
        s4.delete_object(&cand.key)?;
        s4_deleted += 1;

        // Delete Live Photo motion file if present
        if let Some(ref m_key) = cand.motion_key {
            s4.delete_object(m_key)?;
            s4_deleted += 1;
        }

        // Delete small and preview thumbnails
        s4.delete_object(&thumb_small_key(&cand.id))?;
        s4.delete_object(&thumb_preview_key(&cand.id))?;
        s4_deleted += 2;

        purged_ids.push(cand.id.clone());
    }

    store.purge_assets(purged_ids.clone(), now)?;

    Ok(PurgeReport {
        purged_asset_ids: purged_ids,
        s4_objects_deleted: s4_deleted,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ops::{AssetKind, AssetRecord};
    use crate::s4::{InMemoryObjectStore, S4Config};
    use chrono::{Duration, TimeZone};

    fn t(sec: i64) -> DateTime<Utc> {
        Utc.timestamp_opt(1_730_000_000 + sec, 0).unwrap()
    }

    #[test]
    fn purge_deletes_original_motion_and_thumbs_from_s4_and_logs_tombstone() {
        let mut store = Store::open_in_memory("mac-1").unwrap();
        let s4 = InMemoryObjectStore::new(S4Config::default());

        s4.put_object("originals/2024/10/IMG_1.HEIC", b"photo", "image/heic")
            .unwrap();
        s4.put_object("originals/2024/10/IMG_1.MOV", b"motion", "video/quicktime")
            .unwrap();
        s4.put_object("thumbs/a1/s.webp", b"small", "image/webp")
            .unwrap();
        s4.put_object("thumbs/a1/m.webp", b"preview", "image/webp")
            .unwrap();
        // Another asset's file must remain untouched
        s4.put_object("originals/2024/10/KEEP.JPG", b"keep", "image/jpeg")
            .unwrap();

        let live = AssetRecord {
            id: "a1".into(),
            kind: AssetKind::Live,
            key: "originals/2024/10/IMG_1.HEIC".into(),
            filename: "IMG_1.HEIC".into(),
            size: 5,
            taken_at: t(10),
            tz_offset: None,
            date_guessed: false,
            width: Some(100),
            height: Some(100),
            duration_ms: None,
            container: None,
            video_codec: None,
            audio_codec: None,
            camera_make: None,
            camera_model: None,
            motion_id: Some("m1".into()),
            motion_key: Some("originals/2024/10/IMG_1.MOV".into()),
            motion_size: Some(6),
            thumb_failed: false,
        };
        store.add_asset(live, None, t(10)).unwrap();

        let trash_time = t(100);
        store.trash_assets(vec!["a1".into()], trash_time).unwrap();

        let summary = trash_summary(&store, trash_time).unwrap();
        assert_eq!(summary.item_count, 1);
        assert_eq!(summary.total_bytes, 11); // 5 + 6

        // After 31 days, auto-purge deletes all 4 objects for a1 and leaves KEEP.JPG
        let report = purge_expired_trash(&mut store, &s4, trash_time + Duration::days(31)).unwrap();
        assert_eq!(report.purged_asset_ids, vec!["a1".to_string()]);
        assert_eq!(report.s4_objects_deleted, 4);

        assert!(s4.head_object("originals/2024/10/IMG_1.HEIC").unwrap().is_none());
        assert!(s4.head_object("originals/2024/10/IMG_1.MOV").unwrap().is_none());
        assert!(s4.head_object("thumbs/a1/s.webp").unwrap().is_none());
        assert!(s4.head_object("thumbs/a1/m.webp").unwrap().is_none());
        assert!(s4.head_object("originals/2024/10/KEEP.JPG").unwrap().is_some());

        // Deleting a second time is harmless
        assert!(s4.delete_object("originals/2024/10/IMG_1.HEIC").is_ok());
    }
}
