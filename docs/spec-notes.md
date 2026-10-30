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
- **Unicode NFC filename normalisation (`keys::normalize_nfc`).** All scanned
  filenames, S4 object keys (`originals/YYYY/MM/<filename>`), and name-clash
  disambiguations (`<name>-<hash8>.<ext>`) are normalised to Unicode NFC
  (canonical composition, including Hangul Jamo UAX #15 and Latin/Greek/Cyrillic/Kana
  combining marks) so macOS decomposed NFD and Windows composed NFC produce
  identical keys and index rows.

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

### Completed Slices (1–7)

6. **Slice 6 — Tauri 2 Command Bridge & Apple Photos-Style Svelte 5 UI (`frames-shell`) (`a0c4200`)**
   - `frames-shell/src-tauri/src/lib.rs`:
     - `AppState`: Manages `Store` (`frames.sqlite3`), `LocalCache`, `PersistedSettings` (`settings.json` + OS Keychain for secret access key), `S4Client` / `InMemoryObjectStore` fallback, `LoopbackStreamServer`, and runs the 30-day Trash auto-purge on startup.
     - 25 Tauri commands bridging Timeline (`get_app_bootstrap`, `get_timeline`, `get_asset_detail`, `get_thumbnail_data_url`), Video Player (`get_video_playback`, `get_video_seek_url`), Date Adjust & Export (`adjust_asset_date`, `export_assets`), Albums (`list_albums`, `get_album_assets`, `create_album`, `rename_album`, `delete_album`, `add_to_album`, `remove_from_album`, `move_to_album`), Search (`get_search_suggestions`, `search_assets`), Trash (`get_trash`, `trash_assets`, `restore_assets`, `delete_trash_now`, `empty_all_trash`), Imports (`import_from_paths`, `seed_sample_library`), and Sync/Settings (`sync_now`, `toggle_offline_mode`, `save_settings`, `clear_local_cache`, `run_check_library`, `run_rebuild_database`).
   - `frames-shell/src/routes/+page.svelte`:
     - System light/dark mode UI with translucent sidebar (Library, Albums, Imports, Trash, Settings), storage meter with 90% warning banner, and offline banner.
     - Square-thumbnail Timeline grid with sticky month headers, zoom slider (3–15 columns), right-side year scrubber, and badges (`● LIVE`, `▶ duration`, `≈ Date`).
     - Multi-selection (click, Shift-click, Cmd/Ctrl-click, month select, `Cmd/Ctrl+A`), drag-to-album, and bulk action bar.
     - Full-screen Viewer (`Space` quick look / double-click / `Enter`, arrow navigation with neighbour prefetch, scroll/button zoom, `● LIVE` hover/hold motion trigger, video scrubber with seek & `"Can't play here"` + Export fallback, Info panel with Adjust Date & Export).
     - Search box with grouped autocomplete suggestions (Albums, Dates, Cameras, Types, Filenames) and combinable filter chips.
     - Trash view with days remaining and confirmation modal showing item count and GB.

7. **Slice 7 — Adversarial Review Fixes (`frames_review.md`, all 26 items P0–P2)**
   - **P0 Fixes (`s4`, `store`, `sync`, `frames-shell`)**:
     - #1: `PersistedSettings::default()` now defaults `use_real_s4: true` and uses `"app.frames.desktop"` for application data/cache directories.
     - #2: Startup 30-day Trash auto-purge runs against `active_s4()` only when S4 credentials are configured (or demo mode is explicitly active), plus a 24-hour background timer thread (`frames-daily-trash-purge`) runs `purge_expired_trash` while the app stays open.
     - #3: Implemented `ObjectStore::list_parts` (`GET /bucket/key?uploadId=...` + `parse_list_parts_xml`). `upload_file_with_resume` queries `list_parts` when resuming an `existing_upload_id`, reuses server-returned ETags for already-uploaded parts, and falls back to `create_multipart_upload` if the upload ID expired (`404 NoSuchUpload`).
     - #4: Added `Store::reconcile_import_candidate` and updated `run_import` / `run_import_concurrent` to honour `import_queue` state across restarts (skipping `logged` and `skipped` rows when `size` and `mtime` match, and passing `upload_id` + `parts_done` into `upload_file_with_resume`).
     - #5: Refactored `run_import` and `run_import_concurrent` to process folders lazily (one folder at a time instead of hashing 550 GB upfront), upload up to 4 files in parallel (`std::thread::scope`), and support `ImportControl` (`pause_import`, `resume_import`, `cancel_import`, `get_import_status`) with live `"frames://import-progress"` events.
     - #6: Fixed `S4Client` `curl` invocation (`build_curl_args`) to pass `-I` (`--head`) for `HEAD` requests without `--data-binary`, added `--connect-timeout 15 --max-time 300 --speed-time 30 --speed-limit 1024`, and added `get_object_to_file` / `put_object_from_file` so large originals stream directly to/from disk without buffering in RAM.
     - #7: Hardened OS Keychain / Credential Manager integration: macOS pipes `add-generic-password` / `find-generic-password` via stdin to `/usr/bin/security -i` (never exposing secrets in `argv`/`ps`), and Windows uses in-process `Advapi32.dll` `CredWriteW` / `CredReadW` / `CredFree` FFI.
   - **P1 Fixes (`media`, `player`, `sync`, `frames-shell`, `+page.svelte`)**:
     - #8: Added `resolve_tool_binary` (searching sidecar/bundle paths, `/opt/homebrew/bin`, `/usr/local/bin`, and `PATH`) and `spawn_hidden_command` (setting `CREATE_NO_WINDOW = 0x0800_0000` on Windows).
     - #9 & #10: Loopback streaming server (`player.rs`) now sets `.stderr(Stdio::null())` on `ffmpeg` to prevent stderr pipe deadlocks, tracks per-session `active_generation` to kill superseded `ffmpeg` processes immediately on seek, bounds retained sessions (`MAX_ACTIVE_SESSIONS = 16`) and concurrent streams (`MAX_CONCURRENT_STREAMS = 4`), refreshes presigned S4 URLs via `UrlRefresher`, and removes `Access-Control-Allow-Origin: *`.
     - #11: Registered custom `frames://localhost/{thumb,preview,original}/<id>` URI scheme protocol in Tauri to serve binary WebP thumbnails, previews, and `prepare_photo_bytes_for_webview` originals (including RAW embedded JPEGs and Windows HEIC decoding) directly with HTTP caching headers, and upgraded the Viewer to load `frames://localhost/original/<id>` after initial preview display.
     - #12: Implemented row virtualization (`virtualizedGroups`) in the Timeline grid so only visible rows (+ overscan) are mounted in the DOM.
     - #13: Updated `export_originals` to disambiguate colliding filenames using `clash_filename` (`<name>-<hash8>.<ext>`).
     - #14: Converted heavy Tauri commands (`import_from_paths`, `sync_now`, `export_assets`, `run_check_library`, `add_unknown_s4_files`, `run_rebuild_database`) to `async fn` using `spawn_blocking` and short-lived mutex locks.
     - #15: Added `tauri://drag-drop` window listener for OS drag-and-drop import and `pick_folder_dialog` native folder picker buttons.
     - #16 & #17: Added `AppState::init_with_dir` for isolated test runs and `db_damaged` recovery banner in the UI when SQLite fails to open.
     - #18 & #19: Configured an explicit CSP in `tauri.conf.json` and added timezone offset display (`UTC±HH:MM` or `"Camera local time (no timezone offset recorded)"`) in the Viewer Info panel.
   - **P2 Fixes (`media`, `keys`, `sync`, `tauri.conf.json`, `Cargo.toml`)**:
     - #20 & #21: Added lossy WebP flags (`-quality 80 -compression_level 4`) and RAW embedded JPEG `stdin` piping (`run_ffmpeg_webp_stdin`) in `media.rs`.
     - #22 & #26: Configured `productName: "Frames"`, `identifier: "app.frames.desktop"`, `edition = "2021"`, and `windowEffects` (`underWindowBackground`, `mica`).
     - #23: Implemented Unicode NFC filename normalisation (`keys::normalize_nfc` / `clean_filename`).
     - #24: Implemented `import_unknown_s4_keys` and `"Add Unknown Files to Library"` button in Check Library report.
     - #25: Fixed all `cargo clippy --workspace --all-targets` warnings (0 warnings across workspace).

8. **Slice 8 — Second-Round Adversarial Review Blockers & Pre-Flight Hardening (`frames-core` + `frames-shell`)**:
   - **Blocker 1 — Fake S4 fallback & ignored Keychain errors (`frames-shell/src-tauri/src/lib.rs`, `+page.svelte`)**:
     - Added `AppState::require_s4(&self) -> Result<Arc<dyn ObjectStore>, String>`: when `use_real_s4` is `true` and S4 credentials (`endpoint`, `bucket`, `access_key_id`, or `secret`) are missing, all S4-bound commands (`import_from_paths`, `sync_now`, `export_assets`, `run_check_library`, `add_unknown_s4_files`, `run_rebuild_database`, `delete_now_assets`, `empty_trash`, and `frames://` protocol cold fetches) return an explicit error instead of silently falling back to the in-memory `demo_s4` store.
     - `save_settings` now propagates OS Keychain / Credential Manager save errors (`save_secret_in_os_keychain(...)?`) instead of discarding them.
     - `SettingsView` exposes `s4_configured: bool`, and `+page.svelte` renders a prominent warning banner when `use_real_s4 && !s4_configured`.
   - **Blocker 2 — Retry backoff duration & `curl` exit code classification (`crates/frames-core/src/s4.rs`)**:
     - `S4Config::retry_base_delay_ms` defaults to `1_000` ms (`compute_backoff_delay_ms` sleeps `1s, 2s, 4s, 8s, 16s = 31s` total across 5 retries; unit tests use `10` ms on `InMemoryObjectStore`).
     - `classify_curl_exit_code` maps only `curl` exit codes `6` (`Couldn't resolve host`) and `7` (`Failed to connect`) to `S4Error::Offline` (pausing the import as offline), maps transient timeouts/partial transfers (`18`, `28`, `52`, `55`, `56`) to `S4Error::TransientTransport` (retried with exponential backoff), and maps TLS/URL/binary errors (`35`, `51`, `60`, etc.) to `S4Error::Transport` (failing the file without misleadingly pausing the import as offline).
   - **Blocker 3 — Hashing a folder holds the database lock (`crates/frames-core/src/sync.rs`)**:
     - `prepare_folder_assets_concurrent` hashes files (`sha256_file`) and extracts metadata (`read_metadata` / `ffprobe`) outside `store.lock()`, acquiring `store.lock()` only for brief microseconds to check duplicate hashes or update `import_queue`.
   - **Blocker 4 — `frames://` protocol handler blocks UI thread, holds cache lock during S4 downloads, and missing background small-thumb sync (`frames-shell/src-tauri/src/lib.rs`)**:
     - Switched to `.register_asynchronous_uri_scheme_protocol("frames", ...)` running on `tauri::async_runtime::spawn_blocking`.
     - Added `fetch_small_thumb_unlocked`, `fetch_preview_unlocked`, and `fetch_original_unlocked` (using `LocalCache::original_disk_path` and `LocalCache::register_downloaded_original`) so `state.cache.lock()` is never held across S4 network I/O.
     - Added `AppState::spawn_background_small_thumb_sync(&self)` invoked on bootstrap, `sync_now`, and `run_rebuild_database` to download missing `s.webp` thumbnails in the background on a new computer.
   - **Blocker 5 — Startup can hang offline (`frames-shell/src-tauri/src/lib.rs`, `crates/frames-core/src/trash.rs`)**:
     - Removed synchronous `purge_expired_trash` from `AppState::init_with_dir`.
     - Added `purge_expired_trash_concurrent` (releasing `store.lock()` during S4 `DELETE` calls) and scheduled it on a background thread starting 5 seconds after window launch and repeating every 24 hours.
   - **Blocker 6 — Concurrent imports, rigid 4-file batches, and delayed cancel (`crates/frames-core/src/sync.rs`, `crates/frames-core/src/s4.rs`)**:
     - Added `ImportControl::try_start_run()` + `SyncError::AlreadyRunning` + `RunningGuard` RAII drop guard to prevent two imports from running simultaneously.
     - Replaced `assets.chunks(4)` with a 4-worker work-stealing `Mutex<VecDeque<PreparedImportAsset>>` pool in `run_import_concurrent`.
     - Added `upload_file_with_resume_cancel` checking `should_cancel` before each multipart chunk so pause and cancel take effect between 16 MB parts rather than waiting for an entire multi-GB video to finish.
   - **Blocker 7 — Windows drag-and-drop & Local S3/MinIO + Live S4 tests (`tauri.conf.json`, `crates/frames-core/src/s4.rs`)**:
     - Set `"dragDropEnabled": false` in `frames-shell/src-tauri/tauri.conf.json` so HTML5 drag-and-drop of thumbnails onto sidebar albums works on Windows WebView2 alongside external folder drag-and-drop.
     - Added `s4_client_end_to_end_against_local_s3_minio_http_server` and `live_s4_bucket_smoke_test_if_configured` (`FRAMES_TEST_S4_*` env vars) in `crates/frames-core/src/s4.rs`.

9. **Slice 9 — Large Import Pipelining & UI Responsiveness Fixes (`crates/frames-core`, `frames-shell`)**:
   - **Root Causes of App Appearing Stuck During 10 GB / 850+ File Import**:
     1. *Monolithic Upfront Folder Hashing*: In folders with 850+ items (10 GB), `prepare_folder_assets_concurrent` was hashing every single file sequentially before the first file ever began uploading or updating `completed_files`.
     2. *High-Resolution Progress Metrics*: UI previously displayed only `completed_files / total_files` (which stays at 0 until the first file finishes S4 upload), hiding active hashing and active uploads.
     3. *850 Individual SQLite Fsyncs*: `scan.candidates` were reconciled one-by-one with individual transactions.
     4. *Unnecessary Full File Reads on Multi-GB Videos*: `generate_thumbnails` attempted pure-Rust PNG decode on every file regardless of extension, reading multi-GB videos into memory before passing to `ffmpeg`.
     5. *Spawned Child Stdin Contention*: `ffmpeg` and `ffprobe` inherited stdin without `-nostdin` / `Stdio::null()`, triggering OS text/spellcheck timeouts (`NSSpellServer dataFromGeneratingCandidatesForSelectedRange timed out`).
   - **Applied Fixes**:
     - Added `split_folder_into_stem_batches` to stream large folders in stem-aligned chunks of ~16 files so uploading starts within ~1s of launching an import while preserving Live Photo pairs.
     - Multithreaded folder hashing across up to 4 parallel workers, emitting live progress events on each hashed file.
     - Added `Store::reconcile_import_candidates_batch` to reconcile all 850+ files in a single SQLite transaction in a few milliseconds.
     - Guarded `decode_png_rgb` to only run on `.png` files, and added `-nostdin` + `stdin(Stdio::null())` on all `ffmpeg` and `ffprobe` commands.
     - Upgraded `ImportProgress` in `Store` and `+page.svelte` to report both `hashed_files` and `uploading_files` in real time, and added `spellcheck="false"` on the path input.
