use std::path::Path;
use chrono::{DateTime, Utc};
use rusqlite::{params, Connection, OptionalExtension};
use walkdir::WalkDir;

use crate::store::{Result, Store};

pub const SUPPORTED_EXTENSIONS: &[&str] = &[
    "jpg", "jpeg", "png", "heic", "webp", "gif", "tif", "tiff",
    "mp4", "mov", "m4v",
];

pub fn is_supported_media_extension(ext: &str) -> bool {
    let lower = ext.to_lowercase();
    SUPPORTED_EXTENSIONS.contains(&lower.as_str())
}

pub fn classify_kind(ext: &str) -> &'static str {
    match ext.to_lowercase().as_str() {
        "mp4" | "mov" | "m4v" => "video",
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
    let ext = Path::new(filename)
        .extension()
        .and_then(|s| s.to_str())
        .unwrap_or("");
    let kind = classify_kind(ext);
    let mtime_str = crate::store::format_utc(mtime);
    let stamp = format!("{mtime_str}|{device_id}");

    let existing: Option<String> = conn
        .query_row(
            "SELECT id FROM assets WHERE album_id = ?1 AND relative_path = ?2",
            params![album_id, relative_path],
            |r| r.get(0),
        )
        .optional()?;

    let asset_id = if let Some(id) = existing {
        conn.execute(
            "UPDATE assets SET local_path = ?1 WHERE id = ?2",
            params![local_path, id],
        )?;
        id
    } else {
        let new_id = uuid::Uuid::new_v4().to_string();
        conn.execute(
            "INSERT INTO assets (
                id, kind, key, filename, size, taken_at, date_at, state_at,
                album_id, relative_path, local_path, thumb_state
            ) VALUES (
                ?1, ?2, '', ?3, ?4, ?5, ?6, ?6,
                ?7, ?8, ?9, 'none'
            )",
            params![
                new_id,
                kind,
                filename,
                size as i64,
                mtime_str,
                stamp,
                album_id,
                relative_path,
                local_path,
            ],
        )?;
        new_id
    };

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
    if !folder.is_dir() {
        return Ok(0);
    }

    let mut count = 0;
    for entry in WalkDir::new(folder).follow_links(true) {
        let entry = match entry {
            Ok(e) => e,
            Err(_) => continue,
        };

        if entry.file_type().is_dir() {
            continue;
        }

        let path = entry.path();
        let ext = match path.extension().and_then(|e| e.to_str()) {
            Some(e) => e,
            None => continue,
        };

        if !is_supported_media_extension(ext) {
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

        let relative = path
            .strip_prefix(folder)
            .unwrap_or(path)
            .to_string_lossy()
            .replace('\\', "/");
        let filename = entry.file_name().to_string_lossy().to_string();
        let local_path = path
            .canonicalize()
            .unwrap_or_else(|_| path.to_path_buf())
            .to_string_lossy()
            .to_string();

        upsert_local_asset(
            conn,
            album_id,
            &relative,
            &local_path,
            &filename,
            size,
            mtime,
            device_id,
        )?;
        count += 1;
    }

    Ok(count)
}

/// Scans all local roots configured in the store against matched albums.
pub fn scan_all_local_roots(store: &Store) -> Result<usize> {
    let matches = store.match_local_albums()?;
    let device_id = store.device_id().unwrap_or_else(|_| "local".to_string());
    let mut total = 0;

    for m in matches {
        let folder = Path::new(&m.folder_path);
        let n = scan_album_folder(store.connection(), &m.album_id, folder, &device_id)?;
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
}
