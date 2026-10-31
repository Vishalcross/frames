# Repository Map

A map of key components, paths, and behaviors across Frames.

## Core File Map

- **Tauri config file:** `frames-shell/src-tauri/tauri.conf.json`
  Configures the Tauri 2 desktop app, window properties, security CSP, bundle settings, and build scripts.

- **Svelte entry file:** `frames-shell/src/routes/+page.svelte`
  Primary SvelteKit SPA entry view mounting the app shell, sidebar, titlebar, views, and active overlays.

- **Sidebar component:** `frames-shell/src/lib/components/sidebar/Sidebar.svelte`
  Renders the collapsible navigation sidebar with library links, system views, user albums, sync status, and storage meters.

- **Top segmented selector (Years / Months / All Photos):** `frames-shell/src/lib/components/surface/SegmentedControl.svelte`
  Reusable pill segment control used in `frames-shell/src/lib/components/toolbar/Toolbar.svelte` to switch library granularity between Years, Months, and All items.

- **Photo grid component:** `frames-shell/src/lib/components/grid/VirtualGrid.svelte`
  Virtualized photo grid rendering thumbnail tiles via `frames-shell/src/lib/components/grid/Tile.svelte` with dynamic zoom, marquee selection, and scrubbing.

- **Photo viewer component:** `frames-shell/src/lib/components/viewer/PhotoViewer.svelte`
  Full-screen lightbox viewer for photos and videos with zoom/pan, navigation, metadata inspection, and video player integration.

- **Global CSS file:** `frames-shell/src/lib/theme/tokens.css`
  Defines base CSS tokens, typography, anim durations, and @property declarations for the theme engine, complemented by `frames-shell/src/lib/styles/app.css` for application-wide layout styles.

- **Rust command registration file:** `frames-shell/src-tauri/src/lib.rs`
  Defines `AppState`, registers the custom `frames://` protocol handler, and exposes all Tauri IPC invoke handlers connecting the frontend to `frames-core`.

- **Database schema file:** `crates/frames-core/schema.sql`
  Defines the SQLite schema including `assets`, `albums`, `album_items`, `outbox`, and supporting indices.

- **Code that talks to MEGA S4:** `crates/frames-core/src/s4.rs`
  Implements the S3-compatible MEGA S4 client, SigV4 signing, resumable multipart uploads, bucket operations, and OS keychain integration.

- **Upload code:** `crates/frames-core/src/s4.rs` & `crates/frames-core/src/sync.rs`
  `s4.rs` handles low-level multipart and single-part S3 PUT/upload streams; `sync.rs` coordinates import and sync queues, uploading assets, live photos, and thumbnails to S4.

## Dependency Versions

- `svelte`: `5.57.3` (`npm ls svelte` in `frames-shell`)
- `@tauri-apps/api`: `2.12.4` (`npm ls @tauri-apps/api` in `frames-shell`)
- `tauri` (Rust): `2` in `frames-shell/src-tauri/Cargo.toml` (`tauri v2.12.5` resolved in `Cargo.lock`)

## Current media loading

### 1. What URL does an `<img>` get for a thumbnail?
- **Normal Tauri operation:** Custom protocol URL `frames://localhost/thumb/{asset_id}` (referenced in `frames-shell/src/lib/components/grid/Tile.svelte` lines 27–33 and 76–84).
- **Error fallback:** If the custom protocol URL fails to load (`onerror={handleImgError}` in `Tile.svelte` lines 35–44), it falls back to a base64 Data URL (`data:image/webp;base64,...`) fetched via `tauriRepo.getThumbnailDataUrl(asset.id, false)`.
- **Mock/Non-Tauri fallback:** An inline SVG or base64 Data URL (`data:image/svg+xml;utf8,...`) produced by `getThumbnailDataUrl(asset, 'thumb')` in `frames-shell/src/lib/mock/mockRepo.ts` lines 13–33.

### 2. Where is that URL produced?
- The custom protocol URL `frames://localhost/thumb/{asset_id}` is produced in `frames-shell/src/lib/services/tauriRepo.ts` lines 79–85 by function `getAssetUrl('thumb', asset.id)`.
- The fallback base64 Data URL is produced in `frames-shell/src-tauri/src/lib.rs` lines 770–788 inside Tauri command `get_thumbnail_data_url`, which reads the cached webp file and encodes it with `format!("data:image/webp;base64,{}", base64::engine::general_purpose::STANDARD.encode(&bytes))`. It is requested by `tauriRepo.getThumbnailDataUrl` in `frames-shell/src/lib/services/tauriRepo.ts` lines 133–143.
- The mock fallback data URL is produced in `frames-shell/src/lib/mock/mockRepo.ts` lines 13–33.

### 3. Is the local disk checked before the network?
- **Yes** for the local thumbnail cache; **No** for local original files on disk.
- In `frames-shell/src-tauri/src/lib.rs` lines 400–413 (`fetch_small_thumb_unlocked`):
  - Lines 405–409 first check local disk cache: `cache_mutex.lock().get_small_thumb_path(id)` (calling `LocalCache::get_small_thumb_path` in `crates/frames-core/src/cache.rs` lines 188–193, which performs `p.exists()` on the local cache directory).
  - If the thumbnail exists on local disk, it is returned immediately without contacting the network.
  - If it is not present in local cache, it falls back to the network via S4: `s4.get_object(&thumb_small_key(id))` (lines 410–411).
  - Local original source files on disk (e.g. from imported library folders) are not checked or used to generate thumbnails on the fly when the cache misses.

### 4. Where does the album count shown in the UI come from?
- **A DB count**.
- In `crates/frames-core/src/store.rs` lines 1120–1138 (`Store::list_albums`), the count is queried directly from SQLite:
  ```sql
  (SELECT count(*)
   FROM album_items ai
   JOIN assets s ON s.id = ai.asset_id
   WHERE ai.album_id = a.id AND ai.in_album = 1
     AND s.trashed_at IS NULL AND s.purged_at IS NULL) AS cnt
  ```
- This DB count is returned via Tauri commands `get_app_bootstrap`, `list_albums`, and `create_album` (`frames-shell/src-tauri/src/lib.rs` lines 893–896), and stored as `AlbumItem.assetCount` in `frames-shell/src/lib/services/tauriRepo.ts` lines 153 and 413.
- The UI binds to `album.assetCount` in:
  - `frames-shell/src/lib/components/sidebar/Sidebar.svelte` lines 237 and 253 (`<span class="row-count">{album.assetCount}</span>`)
  - `frames-shell/src/lib/components/collections/AlbumDetailView.svelte` line 62 (`{album.assetCount.toLocaleString()}`)

### 5. Is `assetProtocol` or `csp` set in the Tauri config? Copy the values.
- In `frames-shell/src-tauri/tauri.conf.json`:
  - `assetProtocol`: **Not set** (absent).
  - `csp`: **Set**. Exact value:
    `"default-src 'self'; img-src 'self' data: blob: frames: http://frames.localhost https://frames.localhost; media-src 'self' blob: frames: http://frames.localhost http://127.0.0.1:* https://*.mega.io https://*; style-src 'self' 'unsafe-inline'; connect-src 'self' ipc: http://ipc.localhost frames: http://frames.localhost http://127.0.0.1:*"`

