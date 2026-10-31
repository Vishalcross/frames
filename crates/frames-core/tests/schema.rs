//! Proves schema.sql loads on this machine's bundled SQLite, FTS5 included.

use frames_core::SCHEMA_SQL;
use rusqlite::Connection;

fn db() -> Connection {
    let conn = Connection::open_in_memory().unwrap();
    conn.execute_batch(SCHEMA_SQL).unwrap();
    conn
}

#[test]
fn schema_loads_with_the_expected_tables() {
    let conn = db();
    for table in [
        "assets", "albums", "album_items", "search_fts", "sync_state", "local_meta", "outbox",
        "import_queue",
    ] {
        let n: i64 = conn
            .query_row("SELECT count(*) FROM sqlite_master WHERE name = ?1", [table], |r| r.get(0))
            .unwrap();
        assert_eq!(n, 1, "missing table {table}");
    }
}

#[test]
fn full_text_search_finds_camera_and_album_words() {
    let conn = db();
    conn.execute(
        "INSERT INTO search_fts (asset_id, filename, camera, file_type, albums)
         VALUES ('a1', 'IMG_1234.HEIC', 'Apple iPhone 15', 'heic photo', 'Café Bali 2024')",
        [],
    )
    .unwrap();
    for word in ["iphone", "cafe", "bali", "heic"] {
        let n: i64 = conn
            .query_row("SELECT count(*) FROM search_fts WHERE search_fts MATCH ?1", [word], |r| r.get(0))
            .unwrap();
        assert_eq!(n, 1, "no match for {word}");
    }
}

#[test]
fn constraints_reject_bad_rows() {
    let conn = db();
    let bad_kind = conn.execute(
        "INSERT INTO assets (id, kind, key, filename, size, taken_at, date_at, state_at)
         VALUES ('x', 'audio', 'k', 'f', 1, 't', 'd', 's')",
        [],
    );
    assert!(bad_kind.is_err());
    let bad_state = conn.execute(
        "INSERT INTO import_queue (path, size, mtime, state) VALUES ('p', 1, 1, 'weird')",
        [],
    );
    assert!(bad_state.is_err());
}

#[test]
fn fresh_db_migrations_run_twice_succeeds() {
    let conn = Connection::open_in_memory().unwrap();
    // Run migrations twice
    frames_core::store::Store::run_migrations(&conn).unwrap();
    frames_core::store::Store::run_migrations(&conn).unwrap();

    let mut stmt = conn.prepare("PRAGMA table_info(assets)").unwrap();
    let cols: std::collections::HashSet<String> = stmt
        .query_map([], |row| row.get::<_, String>(1))
        .unwrap()
        .filter_map(|r| r.ok())
        .collect();

    assert!(cols.contains("local_path"));
    assert!(cols.contains("remote_key"));
    assert!(cols.contains("thumb_state"));
    assert!(cols.contains("content_hash"));
    assert!(cols.contains("album_id"));
    assert!(cols.contains("relative_path"));

    // Verify unique index on (album_id, relative_path) rejects duplicates
    conn.execute(
        "INSERT INTO assets (id, kind, filename, size, taken_at, date_at, state_at, album_id, relative_path)
         VALUES ('a1', 'photo', 'img1.jpg', 10, '2026-01-01T00:00:00Z', 'stamp', 'stamp', 'alb1', 'sub/img1.jpg')",
        [],
    ).unwrap();

    let dup = conn.execute(
        "INSERT INTO assets (id, kind, filename, size, taken_at, date_at, state_at, album_id, relative_path)
         VALUES ('a2', 'photo', 'img1.jpg', 10, '2026-01-01T00:00:00Z', 'stamp', 'stamp', 'alb1', 'sub/img1.jpg')",
        [],
    );
    assert!(dup.is_err(), "Unique constraint on (album_id, relative_path) must reject duplicates");
}
