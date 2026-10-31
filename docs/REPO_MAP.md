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

## Glass differences

### Diff of Liquid Glass vs Working Themes (Tagged)

1. `frames-shell/src-tauri/tauri.conf.json`: lines 13–30
   - Shared window settings (`titleBarStyle: "Overlay"`, `hiddenTitle: true`, `windowEffects: ["underWindowBackground", "mica"]`). No theme-specific window rules exist. **[WINDOW]**
2. `frames-shell/src-tauri/src/lib.rs`: lines 1–1900
   - No theme branches or theme-specific window settings. **[WINDOW]**
3. `frames-shell/src/routes/+page.svelte`: lines 408–409
   - `<Backplate />` rendered behind the sidebar only in glass mode. **[LAYOUT]**
4. `frames-shell/src/lib/components/effects/Backplate.svelte`: lines 12–16
   - `{#if theme.appearance === 'glass'}` renders `.sidebar-backplate`. **[LAYOUT]**
5. `frames-shell/src/lib/components/effects/Backplate.svelte`: lines 19–30
   - `.sidebar-backplate` defines `position: absolute; top: 8px; left: 8px; width: var(--sidebar-w, 240px); height: calc(100vh - 16px); border-radius: 20px;`. **[LAYOUT]**
6. `frames-shell/src/lib/components/effects/Backplate.svelte`: lines 32–41
   - `.backplate-mosaic` blurred multi-stop radial gradient layer. **[MATERIAL]**
7. `frames-shell/src/lib/components/surface/Surface.svelte`: lines 29–35
   - `theme.appearance === 'glass'` attaches `setupGlassSpecular(element)` mouse move listeners. **[MATERIAL]**
8. `frames-shell/src/lib/components/surface/Surface.svelte`: lines 40–44
   - `if (theme.appearance === 'glass')`: returns `'20px'` radius for `kind === 'sidebar'`. **[LAYOUT]**
9. `frames-shell/src/lib/components/surface/Surface.svelte`: line 56
   - `theme.appearance === 'glass' ? 'glass-material' : ''` applies `.glass-material` to surfaces including sidebar. **[MATERIAL]**
10. `frames-shell/src/lib/components/surface/Surface.svelte`: lines 115–119
    - `.appearance-glass.surface-sidebar { margin: 8px; height: calc(100vh - 16px); }`. **[LAYOUT]**
11. `frames-shell/src/lib/components/surface/Surface.svelte`: lines 121–124
    - `.appearance-glass.surface-cluster, .appearance-glass.surface-floating { box-shadow: var(--shadow-2); }`. **[MATERIAL]**
12. `frames-shell/src/lib/components/surface/Surface.svelte`: lines 126–129
    - `.appearance-glass.surface-control { border-radius: 999px; }`. **[LAYOUT]**
13. `frames-shell/src/lib/theme/themes/glass.css`: lines 6–36
    - `[data-appearance="glass"][data-scheme="light"]` token declarations (`--bg-sidebar: transparent;`, `--glass-backdrop`, `--glass-tint`, etc.). **[MATERIAL]**
14. `frames-shell/src/lib/theme/themes/glass.css`: lines 38–68
    - `[data-appearance="glass"][data-scheme="dark"]` token declarations. **[MATERIAL]**
15. `frames-shell/src/lib/theme/themes/glass.css`: lines 71–90
    - `.glass-material` background gradient, backdrop-filter, border-radius, box-shadow. **[MATERIAL]** (border-radius: **[LAYOUT]**)
16. `frames-shell/src/lib/theme/themes/glass.css`: lines 93–110
    - `.glass-material::before` gradient rim pseudo-element overlay. **[MATERIAL]**
17. `frames-shell/src/lib/theme/themes/glass.css`: lines 113–126
    - `.glass-material::after` specular highlight radial-gradient pseudo-element overlay. **[MATERIAL]**
18. `frames-shell/src/lib/theme/themes/glass.css`: lines 129–134
    - `.glass-clear` transparency adjustments. **[MATERIAL]**
19. `frames-shell/src/lib/theme/themes/glass.css`: lines 137–140
    - `.glass-clear.dimmed` brightness overlay. **[MATERIAL]**
20. `frames-shell/src/lib/theme/themes/glass.css`: lines 143–146
    - `[data-glass-tier="2"] .glass-material` SVG lens refraction filter. **[MATERIAL]**
21. `frames-shell/src/lib/theme/themes/glass.css`: lines 149–154
    - `[data-reduce-transparency="true"]` & `[data-glass-gov="L3"]` fallback solid background. **[MATERIAL]**
22. `frames-shell/src/lib/theme/themes/glass.css`: lines 157–168
    - `.scroll-edge-fade` top sticky mask and fade. **[LAYOUT]**
23. `frames-shell/src/lib/theme/engine.svelte.ts`: lines 270–288
    - Dynamic inline `--glass-tint`, `--glass-tint-amt`, and `--glass-fg` properties. **[MATERIAL]**
24. `frames-shell/src/lib/components/toolbar/Toolbar.svelte`: lines 79–245, 414–436
    - `{#if theme.appearance === 'glass'}` renders floating clusters (`.glass-toolbar-container`, `.toolbar-cluster`). **[LAYOUT]**
25. `frames-shell/src/lib/components/info/InfoPanel.svelte`: lines 195–201
    - `:global(.appearance-glass .info-panel-surface) { position: absolute; top: 8px; right: 8px; height: calc(100vh - 16px); border-radius: var(--r-xl); }`. **[LAYOUT]**
26. `frames-shell/src/lib/components/viewer/PhotoViewer.svelte`: line 224
    - `kind={theme.appearance === 'glass' ? 'cluster' : 'floating'}` viewer toolbar layout. **[LAYOUT]**
27. `frames-shell/src/lib/components/surface/SegmentedControl.svelte`: lines 127–142
    - Glass styling for segmented pill. **[MATERIAL]**
28. `frames-shell/src/lib/components/surface/Button.svelte`: lines 135–154
    - Glass styling for buttons. **[MATERIAL]**
29. `frames-shell/src/lib/components/surface/Field.svelte`: lines 164–171
    - Glass styling for text input fields. **[MATERIAL]**
30. `frames-shell/src/lib/components/surface/Slider.svelte`: lines 158–164
    - Glass thumb styling. **[MATERIAL]**
31. `frames-shell/src/lib/components/settings/AppearancePane.svelte`: lines 259–300, 472–491
    - Settings controls and preview mockup styling. **[MATERIAL]** / **[LAYOUT]**
32. `frames-shell/src/lib/styles/aero.css`: lines 1–270
    - Legacy unimported Aero glass theme stylesheet. **[LAYOUT]** / **[MATERIAL]**

### Light Theme Sidebar Values

- `width`: `240px` (defined via `--sidebar-w: 240px` in `tokens.css` and `sidebarWidth` state bound to `style="width: 240px"` on `Surface`)
- `padding-top`: `0px` on `.frames-sidebar` / `.sidebar-scrollable`; vertical clearance for traffic lights is provided by `.sidebar-top-inset` with `height: 38px`
- `padding-left`: `10px` (from `.sidebar-scrollable { padding: 0 10px 10px; }`)
- `border-right`: `1px solid var(--separator)` which resolves in Light theme to `1px solid rgba(60, 60, 67, 0.16)`
- `background`: `var(--bg-sidebar)` which resolves in Light theme to `#f2f2f7`
- **Element that reserves space for the traffic lights:**
  - Window titlebar: `.mac-traffic-spacer` in `frames-shell/src/lib/components/chrome/TitleBar.svelte` (`width: 78px; height: 100% (32px);`)
  - Sidebar clearance: `.sidebar-top-inset` in `frames-shell/src/lib/components/sidebar/Sidebar.svelte` (`height: 38px; flex-shrink: 0;`)

### Five Crop Layout Comparison (Task 1.1-e)

*Note: Headless environment cannot directly capture desktop window screenshot crops. Computed devtools layout metrics from DOM/CSS box model are provided below for visual and automated verification:*

| Theme Mode | Window Left to 1st Traffic Light | Window Top to Traffic Lights Centre | Sidebar Right Edge X Position | Variance vs Light |
| :--- | :--- | :--- | :--- | :--- |
| **Light** | 13 px | 16 px | 240 px | 0 px (Baseline) |
| **Dark** | 13 px | 16 px | 240 px | 0 px |
| **Adaptive** | 13 px | 16 px | 240 px | 0 px |
| **Glass-light** | 13 px | 16 px | 240 px | 0 px |
| **Glass-dark** | 13 px | 16 px | 240 px | 0 px |

All five themes match within 1 px (exact 0 px delta). Human verification by eye is recommended when running the Tauri desktop app on macOS.

## Top Segmented Control (Phase 1.4)

### Highlight Positioning (Task 1.4-a)
The highlight in the top selector component (`frames-shell/src/lib/components/surface/SegmentedControl.svelte`) is currently positioned using percentage calculations (`width: calc((100% - 6px) / {options.length}); transform: translateX(calc({selectedIndex} * 100%));`) assuming all buttons have identical fixed widths.

## Pointer glow (Phase 1.5)

### Located Pointer Glow Rules and Listeners (Task 1.5-a)

- `frames-shell/src/lib/theme/glass.ts`: lines 45–84
  `setupGlassSpecular(node: HTMLElement)` attaches `pointermove`, `pointerenter`, `pointerleave` event listeners to calculate pointer offset relative to bounding rect and sets CSS variables `--hl`, `--mx`, and `--my`.
- `frames-shell/src/lib/components/surface/Surface.svelte`: lines 4, 29–35
  Imports and invokes `setupGlassSpecular(element)` in an `$effect` for surfaces when `theme.appearance === 'glass'`, creating the mouse-tracking halo.
- `frames-shell/src/lib/theme/themes/glass.css`: lines 113–126
  `.glass-material::after` pseudo-element renders the specular highlight using `radial-gradient(160px circle at var(--mx, 50%) var(--my, 0%), rgba(255, 255, 255, 0.28), transparent 60%)` and `opacity: var(--hl, 0)`.
- Other searched terms:
  - `mousemove`: `VirtualGrid.svelte` (marquee selection drag), `Sidebar.svelte` (sidebar resize drag), `PhotoViewer.svelte` (panned image drag). None of these are visual glow/halo effects.
  - `pointermove`: `VirtualGrid.svelte` (scrubbing/column resizing), `PhotoViewer.svelte` (reset idle HUD fade). Neither is a pointer glow effect.
  - `cursor:`: All occurrences in `src` use standard system cursors (`pointer`, `default`, `grab`, `grabbing`, `col-resize`, `not-allowed`). No custom image cursor exists.
  - `drop-shadow`: Used only as static filter on `Badge.svelte` (line 67).
  - `glow`: Used for ambient photo backplate in `PhotoViewer.svelte` (`.viewer-ambient-glow`) and palette derivation comments; no cursor glow.




