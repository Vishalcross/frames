-- Frames local database (one per computer). A rebuildable cache of S4 state.
-- Source: docs/design-spec.md section 5. Anything beyond the spec is marked [+]
-- and explained in docs/spec-notes.md.
--
-- Stamps. Every column that takes part in "latest wins" merging
-- (assets.state_at, assets.date_at, albums.name_at, album_items.at) holds
--     <RFC 3339 UTC with milliseconds and Z>|<device-id>
-- e.g. 2026-11-02T03:14:15.926Z|mac-7f3a. The timestamp has a fixed width, so
-- plain string comparison gives the spec's order: batch `at`, then device id.

PRAGMA user_version = 1;

CREATE TABLE assets (
  id            TEXT PRIMARY KEY,                 -- lowercase hex SHA-256 of the original
  kind          TEXT NOT NULL CHECK (kind IN ('photo', 'video', 'live')),
  key           TEXT NOT NULL DEFAULT '',         -- S4 key of the original
  filename      TEXT NOT NULL,
  size          INTEGER NOT NULL,
  taken_at      TEXT NOT NULL,                    -- UTC, RFC 3339
  tz_offset     INTEGER,                          -- minutes east of UTC; NULL = unknown
  date_guessed  INTEGER NOT NULL DEFAULT 0,
  date_at       TEXT NOT NULL,                    -- [+] stamp of the last date change
  width         INTEGER,
  height        INTEGER,
  duration_ms   INTEGER,
  container     TEXT,
  video_codec   TEXT,
  audio_codec   TEXT,
  camera_make   TEXT,
  camera_model  TEXT,
  motion_id     TEXT,                             -- Live Photo: hash of the MOV
  motion_key    TEXT,
  motion_size   INTEGER,
  thumb_failed  INTEGER NOT NULL DEFAULT 0,       -- [+] placeholder thumbnail in use
  trashed_at    TEXT,                             -- UTC time of trashing (30-day clock)
  purged_at     TEXT,                             -- [+] set by asset.purge; row stays as a tombstone
  state_at      TEXT NOT NULL,                    -- stamp of the last existence/trash change
  album_id      TEXT,
  relative_path TEXT,
  local_path    TEXT,
  remote_key    TEXT,
  thumb_state   TEXT NOT NULL DEFAULT 'none' CHECK (thumb_state IN ('none', 'queued', 'ready', 'failed')),
  content_hash  TEXT
);

-- Timeline: visible assets by date.
CREATE INDEX assets_timeline ON assets (taken_at)
  WHERE trashed_at IS NULL AND purged_at IS NULL;
-- Trash view and the daily purge.
CREATE INDEX assets_trash ON assets (trashed_at)
  WHERE trashed_at IS NOT NULL AND purged_at IS NULL;
-- Unique asset per album and relative path.
CREATE UNIQUE INDEX IF NOT EXISTS assets_album_relpath ON assets (album_id, relative_path);

CREATE TABLE albums (
  id          TEXT PRIMARY KEY,                   -- UUID
  name        TEXT NOT NULL,
  name_at     TEXT NOT NULL,                      -- stamp of the last rename
  created_at  TEXT NOT NULL,
  deleted_at  TEXT                                -- album.delete is final
);

-- One row per (album, asset); the latest add or remove wins.
CREATE TABLE album_items (
  album_id  TEXT NOT NULL,
  asset_id  TEXT NOT NULL,
  in_album  INTEGER NOT NULL CHECK (in_album IN (0, 1)),
  at        TEXT NOT NULL,                        -- stamp
  PRIMARY KEY (album_id, asset_id)
) WITHOUT ROWID;
CREATE INDEX album_items_asset ON album_items (asset_id);

-- Search over filename, camera make/model, file type words and album names.
CREATE VIRTUAL TABLE search_fts USING fts5(
  asset_id UNINDEXED,
  filename,
  camera,
  file_type,
  albums,
  tokenize = 'unicode61 remove_diacritics 2'
);

-- Highest change-log seq applied, per device.
CREATE TABLE sync_state (
  device       TEXT PRIMARY KEY,
  applied_seq  INTEGER NOT NULL DEFAULT 0
);

-- [+] Own device id and next seq, plus other small local-only values.
CREATE TABLE local_meta (
  key    TEXT PRIMARY KEY,
  value  TEXT NOT NULL
);

-- Local changes not yet uploaded; flushed to S4 as batch files.
CREATE TABLE outbox (
  id          INTEGER PRIMARY KEY AUTOINCREMENT,
  op_json     TEXT NOT NULL,
  created_at  TEXT NOT NULL
);

CREATE TABLE import_queue (
  path       TEXT PRIMARY KEY,
  size       INTEGER NOT NULL,
  mtime      INTEGER NOT NULL,                    -- Unix milliseconds
  state      TEXT NOT NULL DEFAULT 'pending'
             CHECK (state IN ('pending', 'hashed', 'uploading', 'uploaded',
                              'logged', 'skipped', 'failed')),
  asset_id   TEXT,
  upload_id  TEXT,                                -- multipart upload id
  parts_done INTEGER NOT NULL DEFAULT 0,
  album_id   TEXT,                                -- [+] album to add to once logged
  error      TEXT
);

-- Persistent background upload jobs (Task 1.2-n)
CREATE TABLE IF NOT EXISTS upload_jobs (
  id          TEXT PRIMARY KEY,
  asset_id    TEXT NOT NULL,
  state       TEXT NOT NULL CHECK (state IN ('queued', 'running', 'done', 'failed')),
  error       TEXT,
  retry_count INTEGER NOT NULL DEFAULT 0,
  updated_at  TEXT NOT NULL
);
CREATE INDEX IF NOT EXISTS upload_jobs_state ON upload_jobs(state);

-- Adaptive photo colour palettes (Task 4-d)
CREATE TABLE IF NOT EXISTS photo_palettes (
  asset_id    TEXT PRIMARY KEY,
  bg          TEXT NOT NULL,
  sidebar     TEXT NOT NULL,
  accent      TEXT NOT NULL,
  on_bg       TEXT NOT NULL,
  on_sidebar  TEXT NOT NULL,
  computed_at INTEGER NOT NULL
);

-- Album colour theme settings (Task 4-e)
CREATE TABLE IF NOT EXISTS album_theme (
  album_id        TEXT PRIMARY KEY,
  mode            TEXT NOT NULL DEFAULT 'dynamic',
  source          TEXT NOT NULL DEFAULT 'auto',
  bg              TEXT,
  sidebar         TEXT,
  accent          TEXT,
  auto_bg         TEXT,
  auto_sidebar    TEXT,
  auto_accent     TEXT,
  auto_on_bg      TEXT,
  auto_on_sidebar TEXT
);
