use std::path::Path;
use chrono::{DateTime, Utc};
use rusqlite::{params, Connection};
use walkdir::WalkDir;

use crate::store::{Result, Store};

pub const SUPPORTED_EXTENSIONS: &[&str] = &[
    "jpg", "jpeg", "png", "heic", "heif", "webp", "gif", "tif", "tiff",
    "dng", "cr2", "cr3", "nef", "arw", "raf", "orf", "rw2", "raw",
    "mp4", "mov", "m4v", "avi", "mts", "m2ts", "mkv", "3gp", "wmv", "webm",
];

pub fn is_supported_media_extension(ext: &str) -> bool {
    let lower = ext.to_lowercase();
    SUPPORTED_EXTENSIONS.contains(&lower.as_str())
}

pub fn is_hidden_or_system_file(path_or_name: &str) -> bool {
    let normalized = path_or_name.replace('\\', "/");
    for part in normalized.split('/') {
        if part.is_empty() {
            continue;
        }
        if part.starts_with('.')
            || part.starts_with("._")
            || part.eq_ignore_ascii_case("thumbs.db")
            || part.eq_ignore_ascii_case(".ds_store")
        {
            return true;
        }
    }
    false
}

/// Returns true if a path component is a known system, cache, editor or preset directory name.
pub fn is_ignored_folder_component(name: &str) -> bool {
    let lower = name.trim().to_lowercase();
    if lower.is_empty() {
        return false;
    }

    // 1. Hidden & system prefixes / exact names
    if lower.starts_with('.')
        || lower.starts_with("._")
        || lower.starts_with('$')
        || lower == "thumbs.db"
        || lower == ".ds_store"
        || lower == "desktop.ini"
        || lower == "__macosx"
        || lower == "system volume information"
        || lower == "$recycle.bin"
        || lower == "lost+found"
    {
        return true;
    }

    // 2. Photo editors and catalog software directories (case-insensitive substring)
    let editor_keywords = [
        "luminar",
        "skylum",
        "lightroom",
        "photoshop",
        "camera raw",
        "cameraraw",
        "capture one",
        "captureone",
        "cosession",
        "darktable",
        "rawtherapee",
        "dxo",
        "photolab",
        "pureraw",
        "aperture",
        "photo booth",
        "photobooth",
        "affinity",
        "topaz",
        "pixelmator",
        "photoroom",
        "gimp",
        "digikam",
        "on1",
        "corel",
        "paintshop",
        "nik collection",
        "final cut",
        "fcp",
        "imovie",
        "davinci",
        "premiere",
        "after effects",
    ];
    for kw in &editor_keywords {
        if lower.contains(kw) {
            return true;
        }
    }

    // 3. Apple Photos & photo catalog / library bundles
    let bundle_extensions = [
        ".photoslibrary",
        ".photolibrary",
        ".migratedphotolibrary",
        ".aplibrary",
        ".fcpbundle",
        ".imovielibrary",
        ".lrlibrary",
        ".lrdata",
        ".lrcat",
        ".catalog",
        ".cosessiondb",
        ".colcat",
        ".cocatalog",
        ".pxv",
    ];
    for ext in &bundle_extensions {
        if lower.ends_with(ext) || lower.contains(ext) {
            return true;
        }
    }

    // 4. Token-based matching for tool, preset, template, and cache directories
    let tokens: Vec<&str> = lower
        .split(|c: char| !c.is_alphanumeric())
        .filter(|t| !t.is_empty())
        .collect();

    let tool_tokens = [
        "preset",
        "presets",
        "template",
        "templates",
        "lut",
        "luts",
        "brush",
        "brushes",
        "plugin",
        "plugins",
        "extension",
        "extensions",
        "overlay",
        "overlays",
        "watermark",
        "watermarks",
        "gradient",
        "gradients",
        "swatch",
        "swatches",
    ];
    for t in &tokens {
        if tool_tokens.contains(t) {
            return true;
        }
    }

    // Exact standalone tool folder names
    if lower == "actions" || lower == "looks" || lower == "styles" || lower == "textures" || lower == "patterns" {
        return true;
    }

    // 5. Cache, previews, thumbnails, logs, crash reports, app data, node_modules
    let cache_tokens = [
        "cache",
        "caches",
        "thumbnail",
        "thumbnails",
        "thumbs",
        "preview",
        "previews",
        "temp",
        "tmp",
        "crashreport",
        "crashreports",
        "appdata",
        "node_modules",
    ];
    for t in &tokens {
        if cache_tokens.contains(t) {
            return true;
        }
    }

    let cache_phrases = [
        "smart previews",
        "catalog backups",
        "application support",
        "crash reports",
    ];
    for phrase in &cache_phrases {
        if lower.contains(phrase) {
            return true;
        }
    }

    false
}

/// Returns true if any component in the path matches an ignored directory or system name.
pub fn is_ignored_album_folder(path_or_name: &str) -> bool {
    let normalized = path_or_name.replace('\\', "/");
    for part in normalized.split('/') {
        let trimmed = part.trim();
        if trimmed.is_empty() {
            continue;
        }
        if is_ignored_folder_component(trimmed) {
            return true;
        }
    }
    false
}

pub fn normalize_relpath(relpath: &str) -> String {
    let forward = relpath.replace('\\', "/");
    let trimmed = forward.trim_matches('/');
    crate::keys::normalize_nfc(trimmed)
}

pub fn classify_kind(ext: &str) -> &'static str {
    match ext.to_lowercase().as_str() {
        "mp4" | "mov" | "m4v" | "avi" | "mts" | "m2ts" | "mkv" | "3gp" | "wmv" | "webm" => "video",
        _ => "photo",
    }
}

/// Upserts a local asset record by (album_id, relative_path).
/// If the row exists (e.g. from remote metadata), sets local_path without inserting a duplicate.
/// Returns the asset_id.
pub fn upsert_local_asset(
    conn: &Connection,
    album_id: &str,
    relative_path: &str,
    local_path: &str,
    filename: &str,
    size: u64,
    mtime: DateTime<Utc>,
    device_id: &str,
) -> Result<String> {
    let norm_rel = normalize_relpath(relative_path);
    let ext = Path::new(filename)
        .extension()
        .and_then(|s| s.to_str())
        .unwrap_or("");
    let kind = classify_kind(ext);
    let mtime_str = crate::store::format_utc(mtime);
    let stamp = format!("{mtime_str}|{device_id}");

    let new_id = uuid::Uuid::new_v4().to_string();
    conn.execute(
        "INSERT INTO assets (
            id, kind, key, filename, size, taken_at, date_at, state_at,
            album_id, relative_path, local_path, thumb_state
        ) VALUES (
            ?1, ?2, '', ?3, ?4, ?5, ?6, ?6,
            ?7, ?8, ?9, 'none'
        )
        ON CONFLICT(album_id, relative_path) DO UPDATE SET
            local_path = excluded.local_path,
            size = CASE WHEN assets.size = 0 THEN excluded.size ELSE assets.size END",
        params![
            new_id,
            kind,
            filename,
            size as i64,
            mtime_str,
            stamp,
            album_id,
            norm_rel,
            local_path,
        ],
    )?;

    let asset_id: String = conn.query_row(
        "SELECT id FROM assets WHERE album_id = ?1 AND relative_path = ?2",
        params![album_id, norm_rel],
        |r| r.get(0),
    )?;

    conn.execute(
        "INSERT INTO album_items (album_id, asset_id, in_album, at)
         VALUES (?1, ?2, 1, ?3)
         ON CONFLICT(album_id, asset_id) DO UPDATE SET in_album = 1, at = excluded.at",
        params![album_id, asset_id, stamp],
    )?;

    Ok(asset_id)
}

/// Scans a single matched album directory and upserts rows for all supported media files.
pub fn scan_album_folder(conn: &Connection, album_id: &str, folder: &Path, device_id: &str) -> Result<usize> {
    scan_album_folder_with_depth(conn, album_id, folder, device_id, None)
}

/// Scans a single matched album directory with an optional maximum traversal depth.
/// When scanning a root folder directly, `max_depth = Some(1)` ensures files directly in the root
/// are indexed without recursing into subdirectories that are separate albums.
pub fn scan_album_folder_with_depth(
    conn: &Connection,
    album_id: &str,
    folder: &Path,
    device_id: &str,
    max_depth: Option<usize>,
) -> Result<usize> {
    if !folder.is_dir() {
        return Ok(0);
    }

    let mut count = 0;
    let mut builder = WalkDir::new(folder).follow_links(false);
    if let Some(md) = max_depth {
        builder = builder.max_depth(md);
    }

    for entry in builder
        .into_iter()
        .filter_entry(|e| {
            if e.depth() == 0 {
                return true;
            }
            let name = e.file_name().to_string_lossy();
            !is_ignored_folder_component(&name)
        })
    {
        let entry = match entry {
            Ok(e) => e,
            Err(_) => continue,
        };

        if entry.file_type().is_dir() {
            continue;
        }

        let path = entry.path();
        let filename = entry.file_name().to_string_lossy().to_string();
        if is_hidden_or_system_file(&filename) || is_ignored_folder_component(&filename) {
            continue;
        }

        let ext = match path.extension().and_then(|e| e.to_str()) {
            Some(e) => e,
            None => continue,
        };

        if !is_supported_media_extension(ext) {
            continue;
        }

        let relative_raw = path
            .strip_prefix(folder)
            .unwrap_or(path)
            .to_string_lossy();
        let relative = normalize_relpath(&relative_raw);
        if is_hidden_or_system_file(&relative) || is_ignored_album_folder(&relative) {
            continue;
        }

        let metadata = match entry.metadata() {
            Ok(m) => m,
            Err(_) => continue,
        };

        let size = metadata.len();
        let mtime: DateTime<Utc> = metadata
            .modified()
            .map(DateTime::from)
            .unwrap_or_else(|_| Utc::now());

        let local_path = path
            .canonicalize()
            .unwrap_or_else(|_| path.to_path_buf())
            .to_string_lossy()
            .to_string();

        let asset_id = upsert_local_asset(
            conn,
            album_id,
            &relative,
            &local_path,
            &filename,
            size,
            mtime,
            device_id,
        )?;

        if crate::thumbs::thumb_exists(&asset_id, crate::thumbs::ThumbSize::Small) {
            crate::thumbs::update_thumb_state(conn, &asset_id, "ready");
        }

        let _ = crate::upload_worker::enqueue_asset_upload(conn, &asset_id);
        count += 1;
    }

    Ok(count)
}

/// Scans all local roots configured in the store against matched albums.
pub fn scan_all_local_roots(store: &Store) -> Result<usize> {
    let _ = store.cleanup_ignored_albums();
    let mut roots = store.get_local_roots()?;
    if roots.is_empty() {
        if let Some(pic_dir) = dirs::picture_dir() {
            if pic_dir.exists() {
                roots.push(pic_dir.to_string_lossy().to_string());
            }
        }
    }
    let matches = store.discover_and_match_local_albums_for_roots(&roots)?;
    let device_id = store.device_id().unwrap_or_else(|_| "local".to_string());
    let mut total = 0;

    let roots_canonical: Vec<std::path::PathBuf> = roots
        .iter()
        .map(|r| Path::new(r).canonicalize().unwrap_or_else(|_| std::path::PathBuf::from(r)))
        .collect();

    for m in matches {
        if is_ignored_album_folder(&m.album_name) || is_ignored_album_folder(&m.folder_path) {
            continue;
        }
        let folder = Path::new(&m.folder_path);
        let folder_canon = folder.canonicalize().unwrap_or_else(|_| folder.to_path_buf());
        let is_root_dir = roots_canonical.contains(&folder_canon);

        let n = if is_root_dir {
            scan_album_folder_with_depth(store.connection(), &m.album_id, folder, &device_id, Some(1))?
        } else {
            scan_album_folder(store.connection(), &m.album_id, folder, &device_id)?
        };
        total += n;
    }

    Ok(total)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs::{self, File};
    use std::io::Write;

    #[test]
    fn scan_temp_folder_of_3_files_twice_gives_exactly_3_rows() {
        let store = Store::open_in_memory("mac-1").unwrap();
        let conn = store.connection();

        let temp_dir = std::env::temp_dir().join(format!("frames_scan_test_{}", fastrand::u64(..)));
        fs::create_dir_all(&temp_dir).unwrap();

        // Create 3 supported files and 1 unsupported file
        let f1 = temp_dir.join("photo1.jpg");
        let f2 = temp_dir.join("photo2.PNG");
        let f3 = temp_dir.join("clip.mp4");
        let f4 = temp_dir.join("notes.txt");

        File::create(&f1).unwrap().write_all(b"jpg content").unwrap();
        File::create(&f2).unwrap().write_all(b"png content").unwrap();
        File::create(&f3).unwrap().write_all(b"mp4 content").unwrap();
        File::create(&f4).unwrap().write_all(b"txt content").unwrap();

        let album_id = "album-test-1";
        // Create album first
        conn.execute(
            "INSERT INTO albums (id, name, name_at, created_at) VALUES (?1, 'Test Album', 'now', 'now')",
            [album_id],
        ).unwrap();

        // Scan once
        let count1 = scan_album_folder(conn, album_id, &temp_dir, "mac-1").unwrap();
        assert_eq!(count1, 3);

        let row_count1: i64 = conn
            .query_row("SELECT count(*) FROM assets WHERE album_id = ?1", [album_id], |r| r.get(0))
            .unwrap();
        assert_eq!(row_count1, 3);

        // Scan twice
        let count2 = scan_album_folder(conn, album_id, &temp_dir, "mac-1").unwrap();
        assert_eq!(count2, 3);

        let row_count2: i64 = conn
            .query_row("SELECT count(*) FROM assets WHERE album_id = ?1", [album_id], |r| r.get(0))
            .unwrap();
        assert_eq!(row_count2, 3, "Scanning twice must give exactly 3 rows");

        let _ = fs::remove_dir_all(&temp_dir);
    }

    #[test]
    fn local_scan_and_remote_listing_same_file_gives_one_row() {
        let store = Store::open_in_memory("mac-1").unwrap();
        let conn = store.connection();
        let album_id = "album-dup-test";
        conn.execute(
            "INSERT INTO albums (id, name, name_at, created_at) VALUES (?1, 'Dup Test', 'now', 'now')",
            [album_id],
        ).unwrap();

        // 1. Local scan arrives first (using decomposed NFD filename like macOS APFS)
        let nfd_name = "caf\u{0065}\u{0301}.jpg";
        let id1 = upsert_local_asset(
            conn,
            album_id,
            nfd_name,
            "/photos/café.jpg",
            "café.jpg",
            1024,
            Utc::now(),
            "mac-1",
        ).unwrap();

        let count: i64 = conn.query_row(
            "SELECT count(*) FROM assets WHERE album_id = ?1",
            [album_id],
            |r| r.get(0),
        ).unwrap();
        assert_eq!(count, 1);

        // 2. Remote listing arrives for the NFC composed path
        let nfc_name = "caf\u{00e9}.jpg";
        let norm_rel = normalize_relpath(nfc_name);
        assert_eq!(norm_rel, normalize_relpath(nfd_name), "NFD and NFC must normalize to identical path");

        let new_id = uuid::Uuid::new_v4().to_string();
        conn.execute(
            "INSERT INTO assets (
                id, kind, key, filename, size, taken_at, date_at, state_at,
                album_id, relative_path, remote_key, thumb_state
            ) VALUES (
                ?1, 'photo', 'albums/album-dup-test/café.jpg', 'café.jpg', 1024, 'now', 'now', 'now',
                ?2, ?3, 'albums/album-dup-test/café.jpg', 'none'
            )
            ON CONFLICT(album_id, relative_path) DO UPDATE SET
                remote_key = excluded.remote_key,
                key = CASE WHEN assets.key = '' THEN excluded.key ELSE assets.key END,
                size = CASE WHEN assets.size = 0 THEN excluded.size ELSE assets.size END",
            rusqlite::params![new_id, album_id, norm_rel],
        ).unwrap();

        // Still exactly one row!
        let total_count: i64 = conn.query_row(
            "SELECT count(*) FROM assets WHERE album_id = ?1",
            [album_id],
            |r| r.get(0),
        ).unwrap();
        assert_eq!(total_count, 1, "Must not duplicate row when same file arrives from remote");

        let (id2, local_p, remote_k, key_val): (String, Option<String>, Option<String>, String) = conn.query_row(
            "SELECT id, local_path, remote_key, key FROM assets WHERE album_id = ?1",
            [album_id],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)),
        ).unwrap();
        assert_eq!(id1, id2, "Original ID must be preserved");
        assert_eq!(local_p.as_deref(), Some("/photos/café.jpg"));
        assert_eq!(remote_k.as_deref(), Some("albums/album-dup-test/café.jpg"));
        assert_eq!(key_val, "albums/album-dup-test/café.jpg");

        // Verify hidden files are skipped
        assert!(is_hidden_or_system_file(".DS_Store"));
        assert!(is_hidden_or_system_file("Thumbs.db"));
        assert!(is_hidden_or_system_file("sub/._hidden.jpg"));
        assert!(is_hidden_or_system_file(".hidden_dir/photo.jpg"));
        assert!(!is_hidden_or_system_file("sub/normal.jpg"));

        // Verify editor presets, templates, caches, and system albums are ignored
        assert!(is_ignored_album_folder("Luminar Neo"));
        assert!(is_ignored_album_folder("Luminar Neo Catalog"));
        assert!(is_ignored_album_folder("Lightroom"));
        assert!(is_ignored_album_folder("Lightroom Catalog"));
        assert!(is_ignored_album_folder("Presets"));
        assert!(is_ignored_album_folder("Develop Presets"));
        assert!(is_ignored_album_folder("Templates"));
        assert!(is_ignored_album_folder("Photo Booth Library"));
        assert!(is_ignored_album_folder("Photos Library.photoslibrary"));
        assert!(is_ignored_album_folder("Cache"));
        assert!(is_ignored_album_folder("Previews.lrdata"));
        assert!(is_ignored_album_folder("Capture One"));
        assert!(is_ignored_album_folder("sub/Presets/img.jpg"));
        assert!(is_ignored_album_folder("root/Cache/thumb.png"));
        assert!(is_ignored_album_folder("Topaz Photo AI"));
        assert!(is_ignored_album_folder("Affinity Photo"));
        assert!(is_ignored_album_folder("Pixelmator Pro"));
        assert!(is_ignored_album_folder("3D LUTs"));
        assert!(is_ignored_album_folder("My_Presets_2024"));
        assert!(is_ignored_album_folder("Develop Preset"));
        assert!(is_ignored_album_folder("Custom Brushes"));
        assert!(is_ignored_album_folder("Actions"));
        assert!(is_ignored_album_folder("Lightroom/Export Presets"));
        assert!(is_ignored_album_folder("$Recycle.Bin"));
        assert!(is_ignored_album_folder(".Trash"));

        // Verify genuine user photo folders are NOT ignored
        assert!(!is_ignored_album_folder("Summer Vacation 2024"));
        assert!(!is_ignored_album_folder("Family Christmas"));
        assert!(!is_ignored_album_folder("Japan Trip"));
        assert!(!is_ignored_album_folder("Portraits"));
        assert!(!is_ignored_album_folder("Action Shots"));
        assert!(!is_ignored_album_folder("Street Style"));
        assert!(!is_ignored_album_folder("Overlook Point"));
    }
}

