# Spec notes

How the repo relates to [design-spec.md](design-spec.md). The spec wins; this
file records what has been resolved, what the code adds, and what is open.

## Resolved (spec section 13)

- **The 750 GB is shared** between S4 and the regular MEGA cloud drive
  (confirmed by the owner, 2026-10-30). 40 GB was already in use, so about
  710 GB is free for a ~550 GB library plus thumbnails.

## Still open

- **Egress fair use.** A forum post from S4's launch period mentions free
  egress up to 5x the average monthly stored data. Not confirmed on MEGA's own
  pages.
- **Region and endpoint** for the bucket.
- **Presigned URLs and range requests.** The player (section 8) needs S4 to
  serve presigned GETs with `Range`. Check with a one-object test before
  building the player.
- **Build machines, and how the repo moves between them.** No hosted CI and the
  code stays local, so the Mac app is built on the Mac and the Windows app on
  Windows. The repo is plain git; the transport between the two machines is
  undecided.
- **Library choices** to verify against the sample set (spec section 13).

## Schema additions (marked [+] in schema.sql) — please review

The spec's data model leaves these out, but its rules need them:

| Addition | Why |
|---|---|
| `assets.date_at` | `asset.setDate` is "latest wins" (section 6); the spec only has `state_at`, which covers trash/purge. |
| `assets.purged_at` | After a purge, a newer `asset.add` must bring the asset back, so the purge needs a tombstone to compare against. |
| `assets.thumb_failed` | Section 7 step 5 says to "flag the asset" when thumbnail decoding fails. |
| `import_queue.album_id` | Importing into an album must add `album.add` to the outbox once the asset is logged. |
| `local_meta` table | Home for "own device id" and "own next seq" from `sync_state`. |
| `outbox` as one row per op | Lets the flush pack ops into batches of about 100 assets. |

**Stamps.** Columns that take part in latest-wins merging (`state_at`,
`date_at`, `name_at`, `album_items.at`) hold `<ISO time, ms, Z>|<device-id>`.
Fixed-width time plus device id makes plain string comparison equal to the
spec's order (batch `at`, then device id).

## Decisions made while writing code — please review

- **Folder month uses local wall-clock time** (`taken_at` plus `tz_offset`). A
  photo taken at 00:30 on 1 May local time goes under `2024/05`, not April.
- **EXIF dates without an offset** are stored as if they were UTC, with
  `tz_offset` empty, so the time shown is the time the camera showed.
- **Dates before 1990 or over a day in the future** are skipped as junk when
  choosing the date taken.
- **Snapshot timestamps** use `YYYYMMDDTHHMMSSZ`, so the newest snapshot is the
  alphabetical maximum.
- **Unknown ops** in a change log are skipped, not fatal, so an older app can
  read logs from a newer one.
- **Outbox stamp alignment.** When a local mutation is made, it is applied
  immediately to SQLite with a provisional stamp (`<op-time>|<device>`). When
  `outbox` is flushed into a `Batch`, `mark_outbox_flushed` aligns the local
  stamps and `created_at` / `trashed_at` with the canonical `batch.at`, so all
  devices converge on identical row metadata.
- **Not handled yet:** Unicode normalisation of file names. macOS stores them
  decomposed; the same name typed on Windows is a different key.

## Implementation Progress & Handover Notes (for AI Agents / Developers)

Run all core tests offline at any time with:
```sh
cargo test -p frames-core
```

### Completed Slices

1. **Slice 1 — Core Types, Bucket Keys, Dates & Schema (`a3bf04c`)**
   - `crates/frames-core/schema.sql`: SQLite schema (`assets`, `albums`, `album_items`, `search_fts` FTS5, `sync_state`, `local_meta`, `outbox`, `import_queue`).
   - `crates/frames-core/src/keys.rs`: S4 key builders/parsers (`originals/YYYY/MM/<name>`, `<name>-<hash8>.<ext>`, `thumbs/<id>/{s,m}.webp`, `index/log/<device>/<seq>.json`, `index/snapshot-<ts>-<device>.json.gz`).
   - `crates/frames-core/src/dates.rs`: Date-taken fallback chain (`EXIF` → `QuickTime` → `Filename` → `FileModified` with `date_guessed`), junk-date filtering, and local wall-clock folder month calculation.
   - `crates/frames-core/src/ops.rs`: Serde types for `Batch`, `AssetRecord`, `AssetKind`, and `Op` (with forward-compatible `Op::Unknown`).

2. **Slice 2 — Local SQLite Store, Sync Merge Engine, FTS5 Search & Snapshots (`7e34e8f`)**
   - `crates/frames-core/src/store.rs`:
     - `Store`: SQLite connection wrapper with automatic schema bootstrap, `device_id` generation (`mac-xxxxxxxx` / `win-xxxxxxxx`), `next_seq`, and `sync_state`.
     - Deterministic latest-wins merge engine (`apply_remote_batch`, `record_local_ops`) covering all Section 6 rules (`asset.add` idempotency + re-import after `asset.purge` tombstone, `asset.setDate`, `asset.trash`/`restore`/`purge`, `album.create`/`rename`, final `album.delete`, per-pair `album.add`/`remove`, and `move_to_album`).
     - Automatic `search_fts` FTS5 maintenance on asset/album changes.
     - Outbox batching (`peek_outbox_batch` capped at ~100 assets, `mark_outbox_flushed`).
     - `.json.gz` snapshot export/import (`export_snapshot_gz`, `import_snapshot_gz`, `needs_snapshot` at 500 batches or 7 days, `reset_for_rebuild`).
     - Queries for Timeline (`list_timeline`, `timeline_month_groups`, `get_asset`), Albums (`list_albums` with newest cover photo, `list_album_assets`), Trash (`list_trash`, `assets_due_for_purge`), Search (`search_suggestions`, `search_assets` with FTS5 + filter chips), and Import Queue (`enqueue_import`, `update_import_state`, `import_progress`).
   - `crates/frames-core/tests/sync_and_store.rs`: Multi-device concurrent offline edit convergence tests.

3. **Slice 3 — Media Metadata, Live Photo Pairing, Thumbnails & Import Planner (`02be83c`)**
   - `crates/frames-core/src/media.rs`:
     - `classify_path`: Classifies supported photo/video extensions, ignores `.DS_Store`, `Thumbs.db`, `.AAE`, `._*`, and flags unknown types.
     - `sha256_file` / `sha256_reader`: 64 KB streamed SHA-256 fingerprinting.
     - `read_metadata` & `parse_metadata_from_bytes`: Pure-Rust readers for JPEG, PNG, WebP, TIFF/EXIF (including Apple MakerNote `0x0011` and XMP `ContentIdentifier`, orientation dimension swap for `5..=8`), and ISOBMFF (`MOV`, `MP4`, `HEIC`, `CR3`: `mvhd`, `tkhd`, `ispe`, `stsd` codecs, QuickTime `keys`/`ilst`), supplemented by `ffprobe` when available.
     - `extract_embedded_jpeg`: Extracts embedded JPEG previews from RAW files (`DNG`, `CR2`, `CR3`, `NEF`, `ARW`).
     - `generate_thumbnails`: Applies EXIF orientation (`1..=8`), scales `s.webp` (`300 px` short edge) and `m.webp` (`1600 px` long edge), encodes via `ffmpeg` or pure-Rust lossless VP8L WebP (`encode_webp_lossless`), and falls back to `placeholder_thumbnails()` with `thumb_failed = true` if decoding fails.
   - `crates/frames-core/src/importer.rs`:
     - `scan_paths`, `pair_live_photos` (by Apple `ContentIdentifier` or fallback same folder + base filename stem), and `prepare_import_plan` (with duplicate skipping and S4 name-clash alignment).

4. **Slice 4 — S4 Client, Local Cache, 30-Day Trash Purge & Cloud Sync Engine (`138aa69`)**
   - `crates/frames-core/src/s4.rs`:
     - Pure-Rust AWS Signature V4 (`AWS4-HMAC-SHA256`) request signing (`sign_s4_request`) and `Range`-capable presigned `GET` URLs (`presign_get_url`).
     - `ObjectStore` trait + `S4Client` (native OS `curl` HTTPS transport, XML parsers for `ListObjectsV2` and multipart uploads) + `InMemoryObjectStore` (with fault injection and offline simulation for tests).
     - `upload_file_with_resume`: Resumable multipart upload (>64 MB threshold) with exponential backoff retry.
     - `SpaceUsage`: 90% warning and 100% full check.
     - OS Keychain / Credential Manager helpers (`save_secret_in_os_keychain`, `load_secret_from_os_keychain`).
   - `crates/frames-core/src/cache.rs`:
     - `LocalCache`: Permanent local `s.webp` storage (`thumbs_s/<aa>/<id>.webp`) with background downloader (`download_missing_small_thumbs`), plus size-capped LRU cache (default 10 GB) for `m.webp` previews and originals (`clear_lru_cache`).
   - `crates/frames-core/src/trash.rs`:
     - `purge_expired_trash` (30-day auto-purge), `delete_trash_items_now`, `empty_trash`, and `trash_summary`.
   - `crates/frames-core/src/sync.rs`:
     - `flush_outbox`, `pull_remote_logs`, `write_snapshot_if_needed`, `sync_once`, `rebuild_from_s4`, `check_library`, `run_import`, and `export_originals`.

5. **Slice 5 — `player` (Video Playback Decision, Photo Decoder & Loopback Streaming Server)**
   - `crates/frames-core/src/player.rs`:
     - `decide_playback`: Evaluates `(container, video_codec, audio_codec, PlayerCapabilities)` and returns `PlaybackDecision::PlayDirect`, `PlaybackDecision::Repackage` (`ffmpeg -c copy` to fragmented MP4), or `PlaybackDecision::Convert { copy_video, copy_audio }`.
     - `detect_best_h264_encoder` & `build_ffmpeg_stream_args`: Selects hardware encoder (`h264_videotoolbox`, `h264_nvenc`, `h264_qsv`, `h264_amf`, `h264_mf`) or software `libx264` fallback, placing `-ss <seek>` before `-i <presigned_url>` for HTTP Range seeking and outputting fragmented MP4 (`frag_keyframe+empty_moov+default_base_moof`) to `pipe:1`.
     - `prepare_photo_for_webview`: Extracts embedded JPEG from RAW files (`dng`, `cr2`, `cr3`, `nef`, `arw`) and decodes `heic` to JPEG in memory on platforms where the webview lacks native HEIC support.
     - `LoopbackStreamServer`: Binds `127.0.0.1:0` with a random 128-bit hex session token, validates `/stream/<session_id>?token=<tok>&t=<seek_secs>`, streams `ffmpeg` stdout directly to the player without touching disk/S4, and returns HTTP 500 `"Can't play here"` if conversion fails.

### Remaining Slice

6. **Slice 6 — Tauri 2 Commands & Apple Photos-Style Svelte 5 UI (`frames-shell`)**
   - Wire `Store`, `S4Client` / `InMemoryObjectStore`, `LocalCache`, `sync`, `trash`, and `player` into `frames-shell/src-tauri/src/lib.rs` state and Tauri commands.
   - Build the full Svelte 5 UI in `frames-shell/src/routes/+page.svelte` (Sidebar with Library/Albums/Imports/Trash/Settings, virtualized square-thumbnail Timeline with sticky month headers, zoom slider 3–15 columns, year scrubber, multi-select + bulk action bar, Photo/Video/Live Photo Viewer with neighbour prefetch, zoom, info panel, Adjust Date & Export, Search bar with grouped suggestions & chips, Imports progress, Trash view, and Settings modal).
