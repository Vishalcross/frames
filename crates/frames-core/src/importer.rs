//! Local import pipeline (spec sections 4, 7, 11):
//! 1. Scan folders/files recursively (read-only), classifying photos/videos,
//!    ignoring `.DS_Store`, `Thumbs.db`, `.AAE`, and listing unknown types.
//! 2. Stream SHA-256 fingerprints and skip duplicates (against SQLite index and
//!    earlier items in the same import).
//! 3. Read EXIF / QuickTime / ffprobe metadata and run the date-taken fallback chain.
//! 4. Pair Live Photos (matching Apple `ContentIdentifier`, or fallback same
//!    folder + same base name for a photo + `.mov`).
//! 5. Generate `s.webp` and `m.webp` thumbnails (with placeholder fallback and
//!    `thumb_failed = true` if decoding fails).

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::time::UNIX_EPOCH;

use chrono::{DateTime, TimeZone, Utc};
use serde::{Deserialize, Serialize};
use walkdir::WalkDir;

use crate::dates::{choose_taken, folder_year_month, Candidates};
use crate::keys::{clash_filename, original_key};
use crate::media::{
    classify_path, generate_thumbnails, read_metadata, sha256_file, FileClassification,
    MediaMetadata, ThumbnailPair,
};
use crate::ops::{AssetKind, AssetRecord};
use crate::store::{Result as StoreResult, Store};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SkippedFile {
    pub path: PathBuf,
    pub reason: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ScannedCandidate {
    pub path: PathBuf,
    pub filename: String,
    pub ext: String,
    pub is_video: bool,
    pub size: u64,
    pub modified_utc: DateTime<Utc>,
    pub mtime_ms: i64,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ScanReport {
    pub candidates: Vec<ScannedCandidate>,
    pub unknown_files: Vec<SkippedFile>,
    pub unreadable_files: Vec<SkippedFile>,
}

/// Recursively scan one or more paths (folders or files) in read-only mode.
pub fn scan_paths<I, P>(roots: I) -> ScanReport
where
    I: IntoIterator<Item = P>,
    P: AsRef<Path>,
{
    let mut report = ScanReport::default();
    let mut seen_paths = HashSet::new();

    for root in roots {
        let root_path = root.as_ref();
        if !root_path.exists() {
            report.unreadable_files.push(SkippedFile {
                path: root_path.to_path_buf(),
                reason: "unreadable: path does not exist".into(),
            });
            continue;
        }

        let walker = WalkDir::new(root_path).follow_links(false).sort_by_file_name();
        for entry_res in walker {
            let entry = match entry_res {
                Ok(e) => e,
                Err(err) => {
                    let p = err
                        .path()
                        .map(Path::to_path_buf)
                        .unwrap_or_else(|| root_path.to_path_buf());
                    report.unreadable_files.push(SkippedFile {
                        path: p,
                        reason: format!("unreadable: {err}"),
                    });
                    continue;
                }
            };

            if entry.file_type().is_dir() {
                continue;
            }
            let path = entry.path().to_path_buf();
            if !seen_paths.insert(path.clone()) {
                continue;
            }

            match classify_path(&path) {
                FileClassification::Ignored => continue,
                FileClassification::Unknown { ext } => {
                    let label = if ext.is_empty() {
                        "skipped: unknown type".to_string()
                    } else {
                        format!("skipped: unknown type (.{ext})")
                    };
                    report.unknown_files.push(SkippedFile {
                        path,
                        reason: label,
                    });
                }
                FileClassification::Photo { ext } => {
                    push_candidate(&path, ext, false, &mut report);
                }
                FileClassification::Video { ext } => {
                    push_candidate(&path, ext, true, &mut report);
                }
            }
        }
    }

    report
}

fn push_candidate(path: &Path, ext: String, is_video: bool, report: &mut ScanReport) {
    let meta = match std::fs::metadata(path) {
        Ok(m) => m,
        Err(err) => {
            report.unreadable_files.push(SkippedFile {
                path: path.to_path_buf(),
                reason: format!("unreadable: {err}"),
            });
            return;
        }
    };
    let filename = path
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("unknown")
        .to_string();
    let mtime_ms = meta
        .modified()
        .ok()
        .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0);
    let modified_utc = Utc
        .timestamp_millis_opt(mtime_ms)
        .single()
        .unwrap_or_else(Utc::now);

    report.candidates.push(ScannedCandidate {
        path: path.to_path_buf(),
        filename,
        ext,
        is_video,
        size: meta.len(),
        modified_utc,
        mtime_ms,
    });
}

/// A hashed & metadata-inspected file before Live Photo pairing.
#[derive(Debug, Clone)]
pub struct InspectedFile {
    pub candidate: ScannedCandidate,
    pub sha256: String,
    pub metadata: MediaMetadata,
}

/// A logical asset ready for thumbnail generation and S4 upload (either a
/// standalone Photo, a standalone Video, or a paired Live Photo).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PreparedImportAsset {
    pub record: AssetRecord,
    pub folder_year: i32,
    pub folder_month: u32,
    pub primary_path: PathBuf,
    pub primary_mtime_ms: i64,
    pub primary_orientation: u16,
    pub primary_metadata: MediaMetadata,
    pub motion_path: Option<PathBuf>,
    pub motion_filename: Option<String>,
}

impl PreparedImportAsset {
    /// Update `record.key` (and `record.motion_key` for Live Photos) if S4 HEAD
    /// reports a name clash at the default key (spec section 4).
    pub fn apply_name_clash(&mut self, primary_clashed: bool, motion_clashed: bool) {
        let use_clash = primary_clashed || motion_clashed;
        if !use_clash {
            return;
        }
        let clashed_primary_name = clash_filename(&self.record.filename, &self.record.id);
        self.record.key = original_key(self.folder_year, self.folder_month, &clashed_primary_name);

        if self.record.kind == AssetKind::Live {
            if let Some(ref mov_name) = self.motion_filename {
                let mov_ext = Path::new(mov_name)
                    .extension()
                    .and_then(|e| e.to_str())
                    .unwrap_or("MOV");
                let stem = Path::new(&clashed_primary_name)
                    .file_stem()
                    .and_then(|s| s.to_str())
                    .unwrap_or(&clashed_primary_name);
                let clashed_mov = format!("{stem}.{mov_ext}");
                self.record.motion_key = Some(original_key(
                    self.folder_year,
                    self.folder_month,
                    &clashed_mov,
                ));
            }
        }
    }

    /// Generate `s.webp` and `m.webp` thumbnails for this asset and update
    /// `record.thumb_failed` if fallback placeholders were used.
    pub fn build_thumbnails(&mut self) -> ThumbnailPair {
        let pair = generate_thumbnails(
            &self.primary_path,
            self.record.kind,
            &self.primary_metadata,
        );
        self.record.thumb_failed = pair.thumb_failed;
        pair
    }
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct ImportPlan {
    pub assets: Vec<PreparedImportAsset>,
    pub duplicates_skipped: usize,
    pub duplicate_paths: Vec<PathBuf>,
    pub unknown_files: Vec<SkippedFile>,
    pub unreadable_files: Vec<SkippedFile>,
}

/// Pair inspected photos and `.mov` videos into Live Photos (spec section 7, step 4):
/// 1. Same non-empty Apple `content_identifier`.
/// 2. Fallback: same parent folder and same case-insensitive base filename stem.
pub fn pair_live_photos(
    files: Vec<InspectedFile>,
    now: DateTime<Utc>,
) -> Vec<PreparedImportAsset> {
    let mut photos: Vec<Option<InspectedFile>> = Vec::new();
    let mut movs: Vec<Option<InspectedFile>> = Vec::new();
    let mut other_videos: Vec<InspectedFile> = Vec::new();

    for f in files {
        if !f.candidate.is_video {
            photos.push(Some(f));
        } else if f.candidate.ext == "mov" {
            movs.push(Some(f));
        } else {
            other_videos.push(f);
        }
    }

    let mut pairs: Vec<(InspectedFile, Option<InspectedFile>)> = Vec::new();

    // Pass 1: Match by Apple ContentIdentifier
    let mut cid_to_mov: HashMap<String, usize> = HashMap::new();
    for (m_idx, mov_opt) in movs.iter().enumerate() {
        if let Some(mov) = mov_opt {
            if let Some(ref cid) = mov.metadata.content_identifier {
                if !cid.is_empty() {
                    cid_to_mov.entry(cid.clone()).or_insert(m_idx);
                }
            }
        }
    }

    for photo_opt in &mut photos {
        let Some(photo) = photo_opt.as_ref() else {
            continue;
        };
        let matched_mov_idx = photo
            .metadata
            .content_identifier
            .as_ref()
            .and_then(|cid| cid_to_mov.get(cid).copied())
            .filter(|&idx| movs[idx].is_some());

        if let Some(m_idx) = matched_mov_idx {
            let p = photo_opt.take().unwrap();
            let m = movs[m_idx].take().unwrap();
            pairs.push((p, Some(m)));
        }
    }

    // Pass 2 (Fallback): Match remaining photos and MOVs with the same folder + base name
    let mut folder_stem_to_mov: HashMap<(Option<PathBuf>, String), usize> = HashMap::new();
    for (m_idx, mov_opt) in movs.iter().enumerate() {
        if let Some(mov) = mov_opt {
            let key = folder_and_stem(&mov.candidate.path);
            if !key.1.is_empty() {
                folder_stem_to_mov.entry(key).or_insert(m_idx);
            }
        }
    }

    for photo_opt in &mut photos {
        let Some(photo) = photo_opt.as_ref() else {
            continue;
        };
        let key = folder_and_stem(&photo.candidate.path);
        let matched_mov_idx = folder_stem_to_mov
            .get(&key)
            .copied()
            .filter(|&idx| movs[idx].is_some());

        if let Some(m_idx) = matched_mov_idx {
            let p = photo_opt.take().unwrap();
            let m = movs[m_idx].take().unwrap();
            pairs.push((p, Some(m)));
        }
    }

    // Remaining unpaired photos
    for photo in photos.into_iter().flatten() {
        pairs.push((photo, None));
    }

    let mut prepared = Vec::new();
    for (photo, mov_opt) in pairs {
        prepared.push(build_prepared_asset(photo, mov_opt, now));
    }
    for mov in movs.into_iter().flatten() {
        prepared.push(build_prepared_asset(mov, None, now));
    }
    for vid in other_videos {
        prepared.push(build_prepared_asset(vid, None, now));
    }

    // Deterministic order by primary path
    prepared.sort_by(|a, b| a.primary_path.cmp(&b.primary_path));
    prepared
}

fn folder_and_stem(path: &Path) -> (Option<PathBuf>, String) {
    let parent = path.parent().map(Path::to_path_buf);
    let stem = path
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("")
        .to_ascii_lowercase();
    (parent, stem)
}

fn build_prepared_asset(
    primary: InspectedFile,
    motion: Option<InspectedFile>,
    now: DateTime<Utc>,
) -> PreparedImportAsset {
    let taken = choose_taken(&Candidates {
        exif_local: primary.metadata.exif_local,
        exif_offset_minutes: primary.metadata.exif_offset_minutes,
        quicktime_utc: primary.metadata.quicktime_utc,
        filename: &primary.candidate.filename,
        modified: primary.candidate.modified_utc,
        now,
    });
    let (year, month) = folder_year_month(&taken);
    let key = original_key(year, month, &primary.candidate.filename);

    let (kind, motion_id, motion_key, motion_size, motion_path, motion_filename) = match motion {
        Some(mov) => {
            // Spec section 4: "A Live Photo's MOV sits next to its photo with the same base name."
            let photo_stem = Path::new(&primary.candidate.filename)
                .file_stem()
                .and_then(|s| s.to_str())
                .unwrap_or(&primary.candidate.filename);
            let mov_ext = Path::new(&mov.candidate.filename)
                .extension()
                .and_then(|e| e.to_str())
                .unwrap_or("MOV");
            let aligned_mov_name = format!("{photo_stem}.{mov_ext}");
            let m_key = original_key(year, month, &aligned_mov_name);
            (
                AssetKind::Live,
                Some(mov.sha256),
                Some(m_key),
                Some(mov.candidate.size),
                Some(mov.candidate.path),
                Some(aligned_mov_name),
            )
        }
        None => {
            let k = if primary.candidate.is_video {
                AssetKind::Video
            } else {
                AssetKind::Photo
            };
            (k, None, None, None, None, None)
        }
    };

    let record = AssetRecord {
        id: primary.sha256,
        kind,
        key,
        filename: primary.candidate.filename,
        size: primary.candidate.size,
        taken_at: taken.utc,
        tz_offset: taken.tz_offset_minutes,
        date_guessed: taken.guessed,
        width: primary.metadata.width,
        height: primary.metadata.height,
        duration_ms: primary.metadata.duration_ms,
        container: primary.metadata.container.clone(),
        video_codec: primary.metadata.video_codec.clone(),
        audio_codec: primary.metadata.audio_codec.clone(),
        camera_make: primary.metadata.camera_make.clone(),
        camera_model: primary.metadata.camera_model.clone(),
        motion_id,
        motion_key,
        motion_size,
        thumb_failed: false,
    };

    PreparedImportAsset {
        record,
        folder_year: year,
        folder_month: month,
        primary_path: primary.candidate.path,
        primary_mtime_ms: primary.candidate.mtime_ms,
        primary_orientation: primary.metadata.orientation,
        primary_metadata: primary.metadata,
        motion_path,
        motion_filename,
    }
}

/// Full local preparation step (steps 1–4 of spec section 7):
/// Scans `roots`, hashes & reads metadata, pairs Live Photos, and filters out
/// exact duplicates against `store` and within the batch itself.
pub fn prepare_import_plan<I, P>(
    roots: I,
    store: &Store,
    now: DateTime<Utc>,
) -> StoreResult<ImportPlan>
where
    I: IntoIterator<Item = P>,
    P: AsRef<Path>,
{
    let mut scan = scan_paths(roots);
    let mut inspected = Vec::with_capacity(scan.candidates.len());

    for cand in scan.candidates.drain(..) {
        let sha = match sha256_file(&cand.path) {
            Ok(s) => s,
            Err(err) => {
                scan.unreadable_files.push(SkippedFile {
                    path: cand.path,
                    reason: format!("unreadable: {err}"),
                });
                continue;
            }
        };
        let meta = match read_metadata(&cand.path) {
            Ok(m) => m,
            Err(err) => {
                scan.unreadable_files.push(SkippedFile {
                    path: cand.path,
                    reason: format!("unreadable: {err}"),
                });
                continue;
            }
        };
        inspected.push(InspectedFile {
            candidate: cand,
            sha256: sha,
            metadata: meta,
        });
    }

    let paired = pair_live_photos(inspected, now);
    let mut seen_ids: HashSet<String> = HashSet::new();
    let mut assets = Vec::new();
    let mut duplicates_skipped = 0usize;
    let mut duplicate_paths = Vec::new();

    for item in paired {
        let is_dup_in_store = store.has_active_or_trashed_asset(&item.record.id)?;
        let is_dup_in_batch = !seen_ids.insert(item.record.id.clone());
        if is_dup_in_store || is_dup_in_batch {
            duplicates_skipped += 1;
            duplicate_paths.push(item.primary_path);
            continue;
        }
        assets.push(item);
    }

    Ok(ImportPlan {
        assets,
        duplicates_skipped,
        duplicate_paths,
        unknown_files: scan.unknown_files,
        unreadable_files: scan.unreadable_files,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    struct TestDir(PathBuf);
    impl TestDir {
        fn new() -> Self {
            let p = std::env::temp_dir().join(format!("frames-test-{}", fastrand::u64(..)));
            fs::create_dir_all(&p).unwrap();
            Self(p)
        }
        fn path(&self) -> &Path {
            &self.0
        }
    }
    impl Drop for TestDir {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    fn tempdir() -> Result<TestDir, std::io::Error> {
        Ok(TestDir::new())
    }

    fn t(sec: i64) -> DateTime<Utc> {
        Utc.timestamp_opt(1_730_000_000 + sec, 0).unwrap()
    }

    #[test]
    fn scan_ignores_system_files_and_flags_unknown_types() {
        let dir = tempdir().unwrap();
        fs::write(dir.path().join(".DS_Store"), b"junk").unwrap();
        fs::write(dir.path().join("Thumbs.db"), b"junk").unwrap();
        fs::write(dir.path().join("IMG_0001.AAE"), b"<plist/>").unwrap();
        fs::write(dir.path().join("readme.txt"), b"hello").unwrap();
        fs::write(dir.path().join("IMG_20240512_143022.JPG"), b"fake-jpeg").unwrap();

        let report = scan_paths([dir.path()]);
        assert_eq!(report.candidates.len(), 1);
        assert_eq!(report.candidates[0].filename, "IMG_20240512_143022.JPG");
        assert_eq!(report.unknown_files.len(), 1);
        assert!(report.unknown_files[0].reason.contains("unknown type"));
    }

    #[test]
    fn live_photo_pairing_by_content_identifier_and_basename_fallback() {
        let dir = tempdir().unwrap();
        let now = t(100);

        // Pair 1: Different filenames, same Apple ContentIdentifier
        let f_photo_cid = InspectedFile {
            candidate: ScannedCandidate {
                path: dir.path().join("IMG_1001.HEIC"),
                filename: "IMG_1001.HEIC".into(),
                ext: "heic".into(),
                is_video: false,
                size: 1000,
                modified_utc: now,
                mtime_ms: 1000,
            },
            sha256: "hash_p1".into(),
            metadata: MediaMetadata {
                content_identifier: Some("CID-ABC".into()),
                ..Default::default()
            },
        };
        let f_mov_cid = InspectedFile {
            candidate: ScannedCandidate {
                path: dir.path().join("DifferentName.MOV"),
                filename: "DifferentName.MOV".into(),
                ext: "mov".into(),
                is_video: true,
                size: 2000,
                modified_utc: now,
                mtime_ms: 1000,
            },
            sha256: "hash_m1".into(),
            metadata: MediaMetadata {
                content_identifier: Some("CID-ABC".into()),
                ..Default::default()
            },
        };

        // Pair 2: No ContentIdentifier, same folder + same base name ("IMG_20240512_120000")
        let f_photo_stem = InspectedFile {
            candidate: ScannedCandidate {
                path: dir.path().join("IMG_20240512_120000.JPG"),
                filename: "IMG_20240512_120000.JPG".into(),
                ext: "jpg".into(),
                is_video: false,
                size: 1500,
                modified_utc: now,
                mtime_ms: 1000,
            },
            sha256: "hash_p2".into(),
            metadata: MediaMetadata::default(),
        };
        let f_mov_stem = InspectedFile {
            candidate: ScannedCandidate {
                path: dir.path().join("IMG_20240512_120000.mov"),
                filename: "IMG_20240512_120000.mov".into(),
                ext: "mov".into(),
                is_video: true,
                size: 2500,
                modified_utc: now,
                mtime_ms: 1000,
            },
            sha256: "hash_m2".into(),
            metadata: MediaMetadata::default(),
        };

        let assets = pair_live_photos(vec![f_photo_cid, f_mov_cid, f_photo_stem, f_mov_stem], now);
        assert_eq!(assets.len(), 2);

        let a1 = &assets[0];
        assert_eq!(a1.record.kind, AssetKind::Live);
        assert_eq!(a1.record.id, "hash_p1");
        assert_eq!(a1.record.motion_id.as_deref(), Some("hash_m1"));
        // Motion key sits next to photo with same base name
        assert_eq!(a1.record.key, "originals/2024/10/IMG_1001.HEIC");
        assert_eq!(
            a1.record.motion_key.as_deref(),
            Some("originals/2024/10/IMG_1001.MOV")
        );

        let a2 = &assets[1];
        assert_eq!(a2.record.kind, AssetKind::Live);
        assert_eq!(a2.record.id, "hash_p2");
        assert_eq!(a2.record.motion_id.as_deref(), Some("hash_m2"));
        assert_eq!(
            a2.record.key,
            "originals/2024/05/IMG_20240512_120000.JPG"
        );
        assert_eq!(
            a2.record.motion_key.as_deref(),
            Some("originals/2024/05/IMG_20240512_120000.mov")
        );
    }

    #[test]
    fn duplicate_skipping_and_name_clash_alignment() {
        let dir = tempdir().unwrap();
        let store = Store::open_in_memory("mac-1").unwrap();
        let now = t(50);

        // Two identical files in different subfolders
        let sub1 = dir.path().join("a");
        let sub2 = dir.path().join("b");
        fs::create_dir_all(&sub1).unwrap();
        fs::create_dir_all(&sub2).unwrap();
        fs::write(sub1.join("IMG_20240512_100000.JPG"), b"same-photo-bytes").unwrap();
        fs::write(sub2.join("IMG_20240512_100000.JPG"), b"same-photo-bytes").unwrap();

        let plan = prepare_import_plan([dir.path()], &store, now).unwrap();
        assert_eq!(plan.assets.len(), 1);
        assert_eq!(plan.duplicates_skipped, 1);

        // Verify name clash suffix keeps Live Photo pair aligned
        let mut live = plan.assets[0].clone();
        live.record.kind = AssetKind::Live;
        live.motion_filename = Some("IMG_20240512_100000.MOV".into());
        live.apply_name_clash(true, false);

        let short_hash: String = live.record.id.chars().take(8).collect();
        assert_eq!(
            live.record.key,
            format!("originals/2024/05/IMG_20240512_100000-{short_hash}.JPG")
        );
        assert_eq!(
            live.record.motion_key.as_deref(),
            Some(format!("originals/2024/05/IMG_20240512_100000-{short_hash}.MOV").as_str())
        );
    }

    #[test]
    fn corrupt_file_generates_placeholder_and_flags_thumb_failed() {
        let dir = tempdir().unwrap();
        let corrupt_path = dir.path().join("CORRUPT_20240512_100000.HEIC");
        fs::write(&corrupt_path, b"not-a-valid-heic-file").unwrap();

        let store = Store::open_in_memory("mac-1").unwrap();
        let mut plan = prepare_import_plan([dir.path()], &store, t(10)).unwrap();
        assert_eq!(plan.assets.len(), 1);

        let thumbs = plan.assets[0].build_thumbnails();
        assert!(thumbs.thumb_failed);
        assert!(plan.assets[0].record.thumb_failed);
        assert_eq!(&thumbs.small_webp[0..4], b"RIFF");
        assert_eq!(&thumbs.preview_webp[0..4], b"RIFF");
    }
}
