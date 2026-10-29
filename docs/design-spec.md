# Frames — design spec

Date: 2026-10-28
Status: design approved section by section; awaiting review of this written spec.

## 1. Goal

A personal, Apple Photos-style desktop app for Mac and Windows that stores a ~550 GB photo and video library in MEGA S4 (S3-compatible object storage, included in the MEGA Pro Lite plan, 750 GB). No server: each desktop app talks to S4 directly.

## 2. Scope

### In v1

* Timeline browsing of photos and videos, sorted by date taken.
* Photo viewer and native video playback, including videos the OS cannot play, via on-the-fly conversion.
* Many photo and video types, including HEIC, RAW and Live Photos.
* Import from a local folder (the external disk) or by drag-and-drop, with duplicate skipping.
* Albums: create, rename, delete; bulk add / move / remove; drag to album.
* Search by filename, date, album, camera, file type.
* Trash with 30-day auto-delete.
* Adjust date (index only), Export originals to a local folder.
* Lightroom-style local thumbnail cache; offline browsing.
* Two computers (one Mac, one Windows PC) kept in sync through S4.

### Out of v1

Phones (Android/iOS), sharing or multiple users, photo editing, Favourites, place-name or content ("dog", "beach") search, faces, map, encryption, any server, HLS streaming, nested album folders, custom album sort, using the external disk as a backup of S4.

### Settled decisions

| # | Decision |
|---|---|
| 1 | No server. |
| 2 | Mac and Windows only. |
| 3 | Source library is on an external disk; the app only ever reads it. Disk is the primary copy for now and will later become the backup of S4. |
| 4 | Single user, no sharing. |
| 5 | Plain (unencrypted) files in S4, browsable by any S3 tool. |
| 6 | An album is a list of asset ids; "copy" adds to another album, "move" removes from the current one and adds to the other. |
| 7 | Search option (a) only: filename, date, album, camera, type. |
| 8 | No stored converted copies of videos. Convert while playing when needed. |
| 9 | Warn at 90% of the configured space limit. |
| 10 | Skip exact duplicates. |
| 11 | Small thumbnails always cached locally; previews and originals cached on demand. |
| 12 | Tech: Tauri 2 (Rust core + Svelte UI), ffmpeg bundled in the installer. |

## 3. Architecture

```
┌────────────────────── Frames app (one per computer) ──────────────────────┐
│ UI (Svelte, in the OS web view: WKWebView on Mac, WebView2 on Windows)     │
│    │ Tauri commands / events                                              │
│ Rust core                                                                 │
│   ├─ store     local SQLite: index, search, import queue, outbox          │
│   ├─ s4        S3 client: get/put/list/delete, multipart, presigned URLs   │
│   ├─ sync      write own change log, read other logs, snapshots, merge    │
│   ├─ importer  scan → fingerprint → metadata → pair → thumbs → upload → log │
│   ├─ media     metadata reading, HEIC/RAW decode, thumbnail encode        │
│   ├─ player    play-direct / repackage / convert decision; ffmpeg process │
│   ├─ cache     local thumbnail store + size-capped preview/original cache │
│   └─ trash     30-day purge                                               │
│ Bundled: ffmpeg, ffprobe, libheif                                         │
└─────────────────────────────────────┬─────────────────────────────────────┘
                                      │ HTTPS (S3 API)
                                MEGA S4 bucket
```

Each Rust module has one job and is tested on its own. The UI only calls Tauri commands; it never talks to S4 itself, except that video elements may load presigned S4 URLs directly.

## 4. S4 bucket layout

```
originals/YYYY/MM/<original filename>         e.g. originals/2024/05/IMG_1234.HEIC
originals/YYYY/MM/<name>-<hash8>.<ext>        name clash with different content
thumbs/<id>/s.webp                            small, short edge ~300 px
thumbs/<id>/m.webp                            preview, long edge ~1600 px
index/log/<device-id>/<seq 9 digits>.json     change log batch, e.g. 000000123.json
index/snapshot-<UTC timestamp>-<device-id>.json.gz
```

* `YYYY/MM` is the date taken at import time. Files are never moved or renamed after upload; date changes live in the index only.
* A Live Photo's MOV sits next to its photo with the same base name.
* `<id>` = lowercase hex SHA-256 of the original file's bytes. For a Live Photo, the id is the photo's hash; the MOV's hash is stored on the same asset.
* Name clash check: before upload, HEAD the target key. If it exists, use `<name>-<first 8 hex of id>.<ext>`.

## 5. Data model

### Local SQLite (per computer; a rebuildable cache of S4 state)

* `assets`: id, kind (`photo` | `video` | `live`), key, filename, size, taken_at (UTC), tz_offset, date_guessed, width, height, duration_ms, container, video_codec, audio_codec, camera_make, camera_model, motion_id, motion_key, motion_size, trashed_at, state_at (timestamp of the last change to existence/trash state).
* `albums`: id (UUID), name, name_at, created_at, deleted_at.
* `album_items`: album_id, asset_id, in_album (bool), at — one row per pair; the latest add or remove wins.
* `search_fts`: FTS5 over filename, camera make/model, file type words, album names.
* `sync_state`: per device, highest seq applied; own device id; own next seq.
* `outbox`: local change batches not yet uploaded.
* `import_queue`: path, size, mtime, state (`pending` | `hashed` | `uploading` | `uploaded` | `logged` | `skipped` | `failed`), asset_id, multipart upload id, parts done, error.

### Change log batch (`index/log/<device>/<seq>.json`)

```json
{
  "device": "mac-7f3a",
  "seq": 123,
  "at": "2026-11-02T03:14:15.926Z",
  "ops": [
    {"op": "asset.add", "asset": {"id": "...", "kind": "photo", "key": "...", "taken_at": "...", "...": "..."}},
    {"op": "asset.setDate", "id": "...", "taken_at": "..."},
    {"op": "asset.trash", "ids": ["..."]},
    {"op": "asset.restore", "ids": ["..."]},
    {"op": "asset.purge", "ids": ["..."]},
    {"op": "album.create", "id": "...", "name": "Bali 2024"},
    {"op": "album.rename", "id": "...", "name": "Bali '24"},
    {"op": "album.delete", "id": "..."},
    {"op": "album.add", "id": "...", "ids": ["..."]},
    {"op": "album.remove", "id": "...", "ids": ["..."]}
  ]
}
```

A move is `album.remove` + `album.add` in one batch.

## 6. Sync (no server)

* Device id: random, created on first run, stored locally.
* Every local change is applied to SQLite at once and appended to the outbox. The outbox is flushed to S4 as one batch file (debounced ~2 s; at most ~100 assets per batch during import). PUT is atomic, so a batch is all or nothing.
* Each device writes only under its own `index/log/<device-id>/`, so writes never collide.
* Pull: on start, every 5 minutes and after coming back online. List other devices' log folders after the last applied seq, apply batches in seq order.
* Merge rules (ordering key = batch `at`, then device id as a tie-breaker):
  * `asset.add`: idempotent. After a purge, a newer `asset.add` brings the asset back (re-import).
  * `asset.trash` / `asset.restore` / `asset.purge`: the latest one wins for that asset.
  * `asset.setDate`, `album.rename`: the latest one wins.
  * `album.add` / `album.remove`: per (album, asset) pair, the latest one wins. Two devices adding different photos to the same album both survive.
  * `album.delete`: final. Later ops on that album are ignored. Its photos are untouched.
  * Ops referencing an unknown or purged asset are ignored.
* Snapshot: a device writes a full-state snapshot (all tables except local-only ones, plus each device's highest applied seq) after applying ~500 batches since the last snapshot, or weekly. A new device loads the newest snapshot and then the logs after its seq marks.
* shortcut: old log files are never deleted in v1; add compaction if the log folder grows past a few thousand files.
* shortcut: "latest wins" trusts the computers' clocks (auto-set by the OS); add a hybrid logical clock if devices are ever offline with wrong clocks.

## 7. Import

1. **Scan** the chosen folder recursively. Photos: JPG/JPEG, HEIC/HEIF, PNG, GIF, WebP, TIFF, DNG, CR2, CR3, NEF, ARW. Videos: MOV, MP4, M4V, AVI, MTS, M2TS, MKV, 3GP, WMV. Ignore `.DS_Store`, `Thumbs.db`, `.AAE`. Anything else is listed as "skipped: unknown type".
2. **Fingerprint** (streamed SHA-256). If the id exists in the index or earlier in this import → skip as a duplicate and count it.
3. **Metadata.** Date taken from EXIF (DateTimeOriginal + offset) or QuickTime creation date; fallback to a date in the filename (e.g. `IMG_20240512_143022`), then the file's modified time with `date_guessed = true`. Also size, dimensions, orientation, camera, and for videos the duration, container and codecs (ffprobe).
4. **Live Photo pairing.** Photo + MOV with the same Apple content identifier; fallback: same base name in the same folder. Stored as one `live` asset.
5. **Thumbnails.** Decode (HEIC via libheif; RAW via its embedded JPEG preview; video via an ffmpeg frame at ~1 s), apply orientation, encode `s.webp` and `m.webp`. If decoding fails, still upload the original, use a placeholder thumbnail and flag the asset.
6. **Upload** the original (multipart above ~64 MB, parts retried on their own), then the thumbnails. Up to 4 files in parallel.
7. **Log.** Only after the original and thumbnails are confirmed in S4, add `asset.add` (and `album.add` when importing into an album) to the outbox.

* The queue lives in SQLite. Unplugging the disk, losing the network or quitting pauses it; the next run resumes, including partly uploaded multipart files.
* Retries use exponential backoff. Persistent failures go to an error list shown at the end; they never stop the import.
* The source folder is opened read-only. The app never writes, moves or deletes there.
* Expected time for 550 GB at 200 Mbps upload: ~6 hours of pure transfer, 7–9 hours in practice.

## 8. Playback and viewing

* **Photos:** show `m.webp` at once, then the full original. Mac shows HEIC directly. On Windows (WebView2 cannot show HEIC) the Rust core decodes the original to JPEG in memory and serves it through a Tauri custom protocol.
* **Video decision**, using the codecs recorded at import and the web view's own `canPlayType` check:
  * Plays as is → the video element loads a presigned S4 URL directly.
  * Codec playable but container not (e.g. H.264 in MKV/MTS) → **repackage**: `ffmpeg -c copy` to fragmented MP4.
  * Codec not playable (e.g. HEVC on Windows without Microsoft's HEVC add-on, old AVI codecs) → **convert** to H.264/AAC fragmented MP4, using the hardware encoder when present (Media Foundation, NVENC, QuickSync or AMF) and software x264 otherwise.
  * ffmpeg reads the original straight from a presigned S4 URL. Its output streams to the player through a loopback HTTP server bound to 127.0.0.1 on a random port, with a per-session token. Nothing is written to S4 or disk.
  * Seeking in a converted video restarts ffmpeg at the new time (`-ss`). The scrubber shows the full length from the stored duration.
  * If conversion fails → "Can't play here" with an Export button.
* **Live Photos:** hover or hold on the LIVE badge to play the motion part.

## 9. Interface

```
┌──────────────┬──────────────────────────────────────────┬──────┐
│ Library      │ 🔍 Search...                    zoom ───● │      │
│ Albums       │ May 2024                                 │ 2024 ┐
│   Bali 2024  │ ▢ ▢ ▢ ▢ ▢ ▢ ▢ ▢  (sticky month headers)  │ 2023 │ date
│   Family     │ ▢ ▢ ▢ ▶0:42 ▢ ●LIVE ▢                    │ 2022 │ scrubber
│ Imports      │ April 2024                               │ ⋯    ┘
│ Trash        │ ▢ ▢ ▢ ▢ ▢ ▢ ▢ ▢                          │      │
└──────────────┴──────────────────────────────────────────┴──────┘
```

* Follows the system light or dark mode. See-through sidebar (macOS vibrancy, Windows Mica).
* **Timeline:** square thumbnails, newest first, sticky month headers, 3–15 columns, a year scrubber. A virtualised grid draws only what's on screen; the layout comes from SQLite. Badges: video length, LIVE, date guessed.
* **Selection:** click, Shift-click, Cmd/Ctrl-click, drag box, select a whole month from its header. A selection bar shows the count and the actions.
* **Viewer:** double-click or Enter to open, arrow keys for previous/next (neighbours prefetched), zoom with scroll or pinch. The info panel shows date, camera, size, filename and albums, plus **Adjust date** and **Export**.
* **Albums:** create, rename, delete (never deletes photos). Bulk: Add to album, Move to album, Remove from album, Move to Trash. Dragging onto a sidebar album adds there. Album contents are sorted by date taken; the cover is the newest photo.
* **Search:** one box with suggestions grouped as Albums, Dates, Cameras, Types, Filenames; choices combine as chips (e.g. `2023` + `video`). Runs on local SQLite, so it works offline.
* **Trash:** shows days left; Restore, Delete now, Empty Trash. Delete now and Empty Trash confirm with the count and GB.
* **Imports:** progress (files, GB, time left), duplicates skipped, errors, pause/resume.
* **Settings:** S4 endpoint, bucket and key; space limit (default 750 GB); cache location and size (default 10 GB); Clear cache; Check library; Rebuild local database.
* **Keyboard:** Space quick look, arrows, Delete → Trash, Cmd/Ctrl+A select all.

## 10. Local cache

* All `s.webp` thumbnails are kept locally (estimate ~25 KB each, ~2–3 GB per 100k items — check against the real library). A new computer downloads them in the background.
* `m.webp` previews and originals go into a size-capped cache (default 10 GB), least recently used dropped first.

## 11. Errors

| Situation | Behaviour |
|---|---|
| Offline | "Offline" banner; browsing, search and album changes still work; changes wait in the outbox; uploads pause. |
| Bad S4 key | Clear message, link to Settings. |
| S4 space full | Stop the import, show space used. |
| S4 busy or 5xx | Retry with exponential backoff. |
| Unreadable file on import | Skip, list it, carry on. |
| Thumbnail fails | Upload the original anyway, placeholder thumbnail, flagged. |
| Video can't be converted | "Can't play here" + Export. |
| Files changed outside the app | **Check library** compares the S4 listing with the index: lists missing originals, offers to add unknown files, tidies uploads left over from a crash. |
| Local database damaged | **Rebuild** deletes it and reloads from the snapshot plus logs. |
| Deletes | Only the trash purge deletes S4 objects, and only that asset's original, motion file and thumbnails. Any open device runs the purge daily for items trashed more than 30 days ago. Deleting twice is harmless. |
| Credentials | S4 key stored in the OS password store (macOS Keychain, Windows Credential Manager), never in a plain file. |

## 12. Testing

* **Rust unit tests:** date fallback chain, Live Photo pairing, duplicate skipping, name clash naming, merge rules (two devices adding to one album, rename clash, delete vs add, purge then re-import), trash timing, the play/repackage/convert decision.
* **Integration tests:** against MinIO in Docker as a stand-in for S4; one smoke test against a real S4 test bucket.
* **Sample set:** HEIC, RAW (at least DNG and one camera format), a Live Photo pair, MTS, AVI, MKV, HEVC MOV, a photo with no date, a corrupt file.
* **Sync test:** two app instances with separate local databases on one bucket.
* **Hands-on checks on a real Mac and Windows PC:** scrolling with 100k generated items, video playback including conversion, Live Photos, offline mode.
* **Before the full import:** a trial import of one ~5 GB folder, then inspect the bucket with another S3 tool.

### Targets

* Timeline opens in under 2 s with a warm cache.
* Smooth scrolling with 100k items.
* A cached preview shows in under 300 ms.

## 13. Open items for the plan stage

* **Build machines.** The Mac app must be built on a Mac and the Windows app on Windows. No GitHub Actions (code stays local). Decide where development and builds happen.
* **Library choices to verify against the sample set:** S3 client, EXIF/QuickTime metadata reader (must handle HEIC and MOV), libheif binding, WebP encoder, RAW embedded-preview extraction, keychain access.
* **S4 facts to confirm in the MEGA account** (mega.io was unreachable while writing this): whether S4 shares the 750 GB with the MEGA cloud drive, the 5× egress fair use, and the chosen region and endpoint.
* **Windows HEVC:** with Microsoft's HEVC add-on, iPhone videos play directly. Without it they always convert (more CPU). Recommend installing it.