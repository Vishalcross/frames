//! Change-log batch types (spec sections 5 and 6): the JSON each device
//! writes to `index/log/<device>/<seq>.json`. Only the shapes live here; the
//! merge rules come in a later step.
//!
//! Compatibility: unknown fields are ignored, and an unknown `op` becomes
//! `Op::Unknown` instead of failing the whole batch, so an older app can still
//! read logs written by a newer one.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Batch {
    pub device: String,
    pub seq: u64,
    pub at: DateTime<Utc>,
    pub ops: Vec<Op>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum AssetKind {
    Photo,
    Video,
    Live,
}

/// An asset as carried by `asset.add`; mirrors the `assets` table.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AssetRecord {
    pub id: String,
    pub kind: AssetKind,
    pub key: String,
    pub filename: String,
    pub size: u64,
    pub taken_at: DateTime<Utc>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tz_offset: Option<i32>,
    #[serde(default)]
    pub date_guessed: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub width: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub height: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub duration_ms: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub container: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub video_codec: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub audio_codec: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub camera_make: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub camera_model: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub motion_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub motion_key: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub motion_size: Option<u64>,
    #[serde(default)]
    pub thumb_failed: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "op")]
pub enum Op {
    #[serde(rename = "asset.add")]
    AssetAdd { asset: AssetRecord },
    #[serde(rename = "asset.setDate")]
    AssetSetDate { id: String, taken_at: DateTime<Utc> },
    #[serde(rename = "asset.trash")]
    AssetTrash { ids: Vec<String> },
    #[serde(rename = "asset.restore")]
    AssetRestore { ids: Vec<String> },
    #[serde(rename = "asset.purge")]
    AssetPurge { ids: Vec<String> },
    #[serde(rename = "album.create")]
    AlbumCreate { id: String, name: String },
    #[serde(rename = "album.rename")]
    AlbumRename { id: String, name: String },
    #[serde(rename = "album.delete")]
    AlbumDelete { id: String },
    #[serde(rename = "album.add")]
    AlbumAdd { id: String, ids: Vec<String> },
    #[serde(rename = "album.remove")]
    AlbumRemove { id: String, ids: Vec<String> },
    /// Any op this version does not know. Skipped when merging.
    #[serde(other)]
    Unknown,
}

#[cfg(test)]
mod tests {
    use super::*;

    const BATCH: &str = r#"{
      "device": "mac-7f3a",
      "seq": 123,
      "at": "2026-11-02T03:14:15.926Z",
      "ops": [
        {"op": "asset.add", "asset": {
          "id": "ab12", "kind": "live", "key": "originals/2024/05/IMG_1234.HEIC",
          "filename": "IMG_1234.HEIC", "size": 2345678,
          "taken_at": "2024-05-12T09:00:22Z", "tz_offset": 330,
          "width": 4032, "height": 3024,
          "camera_make": "Apple", "camera_model": "iPhone 15",
          "motion_id": "cd34", "motion_key": "originals/2024/05/IMG_1234.MOV", "motion_size": 3456789
        }},
        {"op": "asset.setDate", "id": "ab12", "taken_at": "2024-05-12T10:00:00Z"},
        {"op": "asset.trash", "ids": ["ab12"]},
        {"op": "asset.restore", "ids": ["ab12"]},
        {"op": "asset.purge", "ids": ["ab12"]},
        {"op": "album.create", "id": "11111111-2222-3333-4444-555555555555", "name": "Bali 2024"},
        {"op": "album.rename", "id": "11111111-2222-3333-4444-555555555555", "name": "Bali '24"},
        {"op": "album.add", "id": "11111111-2222-3333-4444-555555555555", "ids": ["ab12"]},
        {"op": "album.remove", "id": "11111111-2222-3333-4444-555555555555", "ids": ["ab12"]},
        {"op": "album.delete", "id": "11111111-2222-3333-4444-555555555555"}
      ]
    }"#;

    #[test]
    fn parses_the_spec_example_with_every_op() {
        let batch: Batch = serde_json::from_str(BATCH).unwrap();
        assert_eq!(batch.device, "mac-7f3a");
        assert_eq!(batch.seq, 123);
        assert_eq!(batch.ops.len(), 10);
        match &batch.ops[0] {
            Op::AssetAdd { asset } => {
                assert_eq!(asset.kind, AssetKind::Live);
                assert_eq!(asset.tz_offset, Some(330));
                assert_eq!(asset.camera_model.as_deref(), Some("iPhone 15"));
                assert!(!asset.date_guessed);
                assert_eq!(asset.container, None);
            }
            other => panic!("expected asset.add, got {other:?}"),
        }
        assert!(matches!(batch.ops[9], Op::AlbumDelete { .. }));
        assert!(!batch.ops.contains(&Op::Unknown));
    }

    #[test]
    fn round_trips() {
        let batch: Batch = serde_json::from_str(BATCH).unwrap();
        let json = serde_json::to_string(&batch).unwrap();
        let again: Batch = serde_json::from_str(&json).unwrap();
        assert_eq!(batch, again);
        assert!(json.contains(r#""op":"asset.setDate""#));
    }

    #[test]
    fn unknown_op_and_fields_do_not_break_the_batch() {
        let json = r#"{"device":"win-01","seq":1,"at":"2026-11-02T03:14:15Z","extra":true,
            "ops":[{"op":"favourite.set","id":"x","value":true},
                   {"op":"asset.trash","ids":["a"],"note":"ignored"}]}"#;
        let batch: Batch = serde_json::from_str(json).unwrap();
        assert_eq!(batch.ops[0], Op::Unknown);
        assert_eq!(batch.ops[1], Op::AssetTrash { ids: vec!["a".to_string()] });
    }
}
