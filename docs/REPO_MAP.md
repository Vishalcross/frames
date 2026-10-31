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
