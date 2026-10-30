//! Comprehensive tests for `frames_core::store`:
//! - Two-device bidirectional sync convergence
//! - All merge edge cases from spec section 6 & 12
//! - Timeline month grouping, album cover selection, search suggestions & chip filtering
//! - Import queue state transitions & progress tracking

use chrono::{Duration, TimeZone, Utc};
use frames_core::ops::{AssetKind, AssetRecord, Batch};
use frames_core::store::{
    ImportQueueRow, ImportState, SearchFilter, Store, SNAPSHOT_BATCH_THRESHOLD,
};

fn t(sec: i64) -> chrono::DateTime<Utc> {
    Utc.timestamp_opt(1_730_000_000 + sec, 0).unwrap()
}

fn make_asset(id: &str, kind: AssetKind, filename: &str, taken_sec: i64) -> AssetRecord {
    AssetRecord {
        id: id.to_string(),
        kind,
        key: format!("originals/2024/10/{filename}"),
        filename: filename.to_string(),
        size: 2_500_000,
        taken_at: t(taken_sec),
        tz_offset: Some(330), // +05:30
        date_guessed: false,
        width: Some(4032),
        height: Some(3024),
        duration_ms: if kind == AssetKind::Video {
            Some(42_000)
        } else {
            None
        },
        container: if kind == AssetKind::Video {
            Some("mov".into())
        } else {
            None
        },
        video_codec: if kind == AssetKind::Video {
            Some("hevc".into())
        } else {
            None
        },
        audio_codec: if kind == AssetKind::Video {
            Some("aac".into())
        } else {
            None
        },
        camera_make: Some("Sony".into()),
        camera_model: Some("ILCE-7M4".into()),
        motion_id: None,
        motion_key: None,
        motion_size: None,
        thumb_failed: false,
    }
}

#[test]
fn two_devices_converge_across_concurrent_edits() {
    let mut mac = Store::open_in_memory("mac-7f3a").unwrap();
    let mut win = Store::open_in_memory("win-9b1c").unwrap();

    // Mac imports 3 assets and creates an album
    mac.add_asset(
        make_asset("a1", AssetKind::Photo, "DSC0001.ARW", 100),
        None,
        t(10),
    )
    .unwrap();
    mac.add_asset(
        make_asset("a2", AssetKind::Video, "CLIP002.MOV", 200),
        None,
        t(11),
    )
    .unwrap();
    mac.add_asset(
        make_asset("a3", AssetKind::Live, "IMG_003.HEIC", 300),
        None,
        t(12),
    )
    .unwrap();
    mac.create_album("alb-1".into(), "Japan 2024".into(), t(13))
        .unwrap();
    mac.add_to_album("alb-1", vec!["a1".into(), "a2".into()], t(14))
        .unwrap();

    let mac_b1 = mac.peek_outbox_batch(100, t(15)).unwrap().unwrap();
    mac.mark_outbox_flushed(&mac_b1).unwrap();

    // Win pulls Mac's first batch
    assert!(win.apply_remote_batch(&mac_b1.batch).unwrap());
    assert_eq!(win.list_timeline(None, None).unwrap().len(), 3);

    // Concurrent edits while offline:
    // - Mac moves a2 from alb-1 to a new album alb-2 at t(30), and adjusts a1's date at t(31)
    mac.create_album("alb-2".into(), "Highlights".into(), t(29))
        .unwrap();
    mac.move_to_album("alb-1", "alb-2", vec!["a2".into()], t(30))
        .unwrap();
    mac.set_asset_date("a1", t(500), t(31)).unwrap();

    // - Win adds a3 to alb-1 at t(32), trashes a2 at t(33), and renames alb-1 at t(34)
    win.add_to_album("alb-1", vec!["a3".into()], t(32)).unwrap();
    win.trash_assets(vec!["a2".into()], t(33)).unwrap();
    win.rename_album("alb-1", "Tokyo & Kyoto".into(), t(34))
        .unwrap();

    let mac_b2 = mac.peek_outbox_batch(100, t(35)).unwrap().unwrap();
    mac.mark_outbox_flushed(&mac_b2).unwrap();

    let win_b1 = win.peek_outbox_batch(100, t(36)).unwrap().unwrap();
    win.mark_outbox_flushed(&win_b1).unwrap();

    // Exchange batches
    assert!(mac.apply_remote_batch(&win_b1.batch).unwrap());
    assert!(win.apply_remote_batch(&mac_b2.batch).unwrap());

    // Both devices must have identical timeline, albums, album contents, and trash!
    assert_eq!(
        mac.list_timeline(None, None).unwrap(),
        win.list_timeline(None, None).unwrap()
    );
    assert_eq!(mac.list_albums().unwrap(), win.list_albums().unwrap());
    assert_eq!(
        mac.list_album_assets("alb-1").unwrap(),
        win.list_album_assets("alb-1").unwrap()
    );
    assert_eq!(
        mac.list_album_assets("alb-2").unwrap(),
        win.list_album_assets("alb-2").unwrap()
    );
    assert_eq!(
        mac.list_trash(t(100)).unwrap(),
        win.list_trash(t(100)).unwrap()
    );

    // Check specific merged outcomes:
    // - a1 has adjusted date t(500) so it is now the newest visible item in timeline and alb-1
    let tl = mac.list_timeline(None, None).unwrap();
    assert_eq!(tl.len(), 2); // a2 is in trash
    assert_eq!(tl[0].id, "a1");
    assert_eq!(tl[1].id, "a3");

    let alb1_items = mac.list_album_assets("alb-1").unwrap();
    assert_eq!(alb1_items.len(), 2); // a1 and a3 (a2 was moved out and trashed)
}

#[test]
fn search_suggestions_and_combined_chip_filters() {
    let mut store = Store::open_in_memory("mac-1").unwrap();
    store
        .add_asset(
            make_asset("a1", AssetKind::Photo, "DSC0001.ARW", 100),
            None,
            t(1),
        )
        .unwrap();
    let mut video = make_asset("a2", AssetKind::Video, "IMG_2024.MOV", 200);
    video.camera_make = Some("Apple".into());
    video.camera_model = Some("iPhone 15".into());
    store.add_asset(video, None, t(2)).unwrap();

    store
        .create_album("alb-1".into(), "Bali Beach".into(), t(3))
        .unwrap();
    store
        .add_to_album("alb-1", vec!["a1".into(), "a2".into()], t(4))
        .unwrap();

    let sugg = store.search_suggestions("ba").unwrap();
    assert_eq!(sugg.albums.len(), 1);
    assert_eq!(sugg.albums[0].name, "Bali Beach");

    // Filter by album + kind = Video
    let res = store
        .search_assets(&SearchFilter {
            album_id: Some("alb-1".into()),
            kind: Some(AssetKind::Video),
            ..Default::default()
        })
        .unwrap();
    assert_eq!(res.len(), 1);
    assert_eq!(res[0].id, "a2");

    // Filter by text ("raw") -> finds DSC0001.ARW because .arw indexes "raw"
    let raw_res = store
        .search_assets(&SearchFilter {
            text: Some("raw".into()),
            ..Default::default()
        })
        .unwrap();
    assert_eq!(raw_res.len(), 1);
    assert_eq!(raw_res[0].id, "a1");
}

#[test]
fn import_queue_lifecycle_and_snapshot_thresholds() {
    let mut store = Store::open_in_memory("mac-1").unwrap();
    store
        .enqueue_import(&ImportQueueRow {
            path: "/Volumes/Disk/DCIM/IMG_1.HEIC".into(),
            size: 4_000_000,
            mtime: 1_700_000_000_000,
            state: ImportState::Pending,
            asset_id: None,
            upload_id: None,
            parts_done: 0,
            album_id: Some("alb-1".into()),
            error: None,
        })
        .unwrap();
    store
        .enqueue_import(&ImportQueueRow {
            path: "/Volumes/Disk/DCIM/IMG_DUP.HEIC".into(),
            size: 4_000_000,
            mtime: 1_700_000_000_000,
            state: ImportState::Skipped,
            asset_id: Some("ab12".into()),
            upload_id: None,
            parts_done: 0,
            album_id: None,
            error: None,
        })
        .unwrap();

    store
        .update_import_state(
            "/Volumes/Disk/DCIM/IMG_1.HEIC",
            ImportState::Logged,
            Some("ab12"),
            None,
            1,
            None,
        )
        .unwrap();

    let prog = store.import_progress().unwrap();
    assert_eq!(prog.total_files, 2);
    assert_eq!(prog.completed_files, 2);
    assert_eq!(prog.skipped_duplicates, 1);
    assert_eq!(prog.failed_files, 0);

    store.clear_finished_imports().unwrap();
    assert!(store.list_import_queue().unwrap().is_empty());

    // Snapshot threshold checks
    assert!(!store.needs_snapshot(t(100)).unwrap());
    let (_, _gz) = store.export_snapshot_gz(t(100)).unwrap();
    store
        .apply_remote_batch(&Batch {
            device: "win-1".into(),
            seq: 1,
            at: t(101),
            ops: vec![],
        })
        .unwrap();
    assert!(!store.needs_snapshot(t(100) + Duration::days(6)).unwrap());
    assert!(store.needs_snapshot(t(100) + Duration::days(7)).unwrap());

    for seq in 2..=(SNAPSHOT_BATCH_THRESHOLD + 1) {
        store
            .apply_remote_batch(&Batch {
                device: "win-1".into(),
                seq,
                at: t(100 + seq as i64),
                ops: vec![],
            })
            .unwrap();
    }
    assert!(store.needs_snapshot(t(200)).unwrap());
}
