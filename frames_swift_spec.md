# Frames for Mac (Swift): spec

Date: 2026-11-03
Status: decisions settled on 2026-11-03 except the signing identity (section 16).
Target machine: the owner's MacBook Air (M3, 8 GB memory, 60 Hz display), macOS 27.
Parity baseline: `main` at 7fa42b7, plus the Adaptive theme changes made after it (photo brightness, immersive palettes).

Related documents. `frames_design_spec.md` sections 4 to 7 (bucket layout, data model, sync, import) remain the contract with the bucket and with the Windows app. This spec does not change them. `Frames Polish, Local-First Media and Packaging Spec.md` Phase 4 describes the album colour settings that are ported here.

---

## 1. Goal

A native macOS version of Frames that feels as smooth as Apple Photos. It uses the same MEGA S4 bucket, the same local library and the same features as the Tauri app.

* **Same library.** The Swift app opens the existing local database, cache and device identity. It does not import again or migrate.
* **Same bucket.** Albums, trash and imports made on the Mac appear on the Windows PC, and the other way round, as they do today.
* **Faster where people notice.** Scrolling, zooming the grid, opening the viewer and moving between photos must hold the display's refresh rate (section 7.1).
* **Windows keeps the Tauri app.** Nothing in this spec changes it, except the data-folder lock in section 6.3, which both apps share.

## 2. Why a native front end, and why not a rewrite of everything

The speed problems in the Tauri app came in two layers:

1. **Backend.** Locks held across network calls, decoding on the wrong threads, and sync work competing with the interface. These are fixed in `frames-core`, which is fast and well tested (106 unit tests).
2. **Web view.** These costs can't be fixed from inside WKWebView:
   * every thumbnail is a request from the web process to Rust; its bytes are copied across processes and then decoded by WebKit, on a schedule the app doesn't control;
   * tile recycling and layout run on the JavaScript main thread;
   * there is no direct access to ImageIO, Core Animation, AVKit or PhotosUI, which is where Apple Photos gets its feel.

So the plan replaces the layer that is slow and keeps the one that is correct. **Swift draws the pixels; Rust owns the data.** Everything that ends up in the bucket or the index stays in `frames-core`. Everything that ends up on screen moves to Apple frameworks.

## 3. Settled decisions

| # | Decision |
|---|---|
| 1 | macOS only, Apple silicon only (arm64). Minimum **macOS 26**: native Liquid Glass (`glassEffect`), the current SwiftUI, and Swift 6 concurrency. |
| 2 | Swift 6 with strict concurrency. SwiftUI for windows, sidebar, toolbar, sheets, settings, inspector and list screens. AppKit and Core Animation for the photo grid and the viewer. |
| 3 | `frames-core` (Rust) stays the single implementation of the store, change log, merge, sync, S4 client, import pipeline, keys, dates, trash and palettes. Swift reaches it through UniFFI, packaged as an XCFramework. |
| 4 | Apple frameworks for pixels: ImageIO to decode (HEIC, RAW, JPEG, WebP), AVFoundation and AVKit for video, PhotosUI for Live Photos, Core Animation for drawing. |
| 5 | Bucket format unchanged. Thumbnails stay WebP, because the Windows app reads them. ImageIO reads WebP but can't write it (checked on this Mac with `CGImageDestinationCopyTypeIdentifiers`), so Rust's libwebp encoder keeps encoding them. |
| 6 | Same local data folder, database and device id as the Tauri app. Only one of the two apps may have the library open at a time (section 6.3). |
| 7 | Not sandboxed and not on the App Store. Signed with a stable identity so Keychain access survives rebuilds; which identity is still open (section 12). |
| 8 | ffmpeg stays bundled, but is used only for formats AVFoundation can't play (MKV, AVI, WMV, MTS/M2TS, old codecs). |
| 9 | Parity first. No new library features in this version. The Tauri app's placeholder sections (Memories, People & Pets, Trips, Days, Duplicates, Hidden, Favourites, Captured by Me, Edit) are **left out** until they do something. |
| 10 | Windows stays on the Tauri app for this version. |

## 4. Architecture

```
┌──────────────────────────── Frames.app (macOS) ─────────────────────────────┐
│ SwiftUI      window · sidebar · toolbar · search · inspector · sheets ·      │
│              Imports · Recently Deleted · Settings · Theme                   │
│ AppKit/CA    GridView (layer pool, layout, selection, zoom)                  │
│              ViewerView (zoomable image, AVPlayerView, PHLivePhotoView)      │
│ Swift media  ImageLoader: memory cache → files from core → ImageIO decode    │
│              MacMediaBackend: decode / video frame / probe for the core      │
│ CoreBridge   async Swift API over FramesCore; change stream as AsyncStream   │
├──────────────────────────── FramesCore.xcframework ─────────────────────────┤
│ frames-ffi (UniFFI)  →  frames-core                                         │
│   store · ops · sync · s4 · importer · keys · dates · trash · palette ·     │
│   thumbnails (file cache + S4 fetch) · player (ffmpeg, loopback server)     │
│   MediaBackend trait ← implemented in Swift on Mac, in Rust on Windows      │
└─────────────────────────────────────┬───────────────────────────────────────┘
                                      │ HTTPS (S3 API)
                                MEGA S4 bucket
```

### Threads and priorities

| Work | Where it runs | Priority (QoS) |
|---|---|---|
| Layout, layer updates, input | main thread | user-interactive |
| Decoding visible tiles; viewer preview | Swift decode queue | user-initiated |
| Prefetch decode | Swift decode queue | utility |
| Calls into the core (queries, edits) | Swift tasks off the main actor | user-initiated |
| Import | Rust worker threads (existing `qos.rs`) | utility |
| Thumbnail backfill, sync, trash purge | Rust worker threads | background |
| Uploading edits (2 s after the last edit) | Rust thread | utility |
| ffmpeg conversion while playing | child process (`taskpolicy`) | user-initiated while visible |

Rule: the main thread never waits on SQLite, the network, a decode or a call into the core.

## 5. The Rust–Swift boundary

### 5.1 Crates and packaging

* New crate `crates/frames-ffi`, using UniFFI proc-macros and depending on `frames-core`. It is the only crate Swift sees.
* `scripts/build-core-xcframework.sh`:
  1. `cargo build --release` for `aarch64-apple-darwin` (no Intel build);
  2. `uniffi-bindgen` generates the Swift bindings;
  3. `xcodebuild -create-xcframework` packages them.
* An Xcode build phase reruns the script when files under `crates/` change.
* The bindings and the XCFramework are build outputs and are never committed.

### 5.2 API shape

One `Library` object owns everything that `AppState` owns in the Tauri app: store, S4 client, thumbnail service, workers and import control.

```
Library.open(dataDir, credentials, observer, mediaBackend) throws -> Library

// Reads (all async on the Swift side)
timeline(filter) -> TimelineSnapshot          // see 5.3
assetDetail(id) -> AssetDetail?
albums() -> [AlbumSummary]
searchSuggestions(text) -> Suggestions
trash() -> TrashPayload
palette(assetId) -> Palette?                  // includes lum
albumTheme(albumId) -> AlbumTheme

// Files for display (the core fetches from S4 or makes them if needed)
requestThumbs(ids, size, priority) -> RequestId   // ready paths arrive through observer.thumbsReady
cancelThumbs(RequestId)
photoFile(id) -> path / originalUrl(id) -> presigned URL
motionFile(id) -> path
videoPlayback(id, canPlayHevc) -> PlaybackPlan    // direct file or URL, or loopback stream URL

// Edits (each schedules the 2 s upload)
createAlbum(name, ids) · renameAlbum · deleteAlbum · addToAlbum · removeFromAlbum · moveToAlbum
trash(ids) · restore(ids) · deleteNow(ids) · emptyTrash() · setDate(id, date)
setAlbumTheme(...) · exportOriginals(ids, folder)

// Import and maintenance
importPaths(paths, albumId?) · pauseImport · resumeImport · cancelImport · importStatus()
syncNow() · checkLibrary() · addUnknownFiles(keys) · rebuildDatabase()
settings() · saveSettings(s) · clearCache() · workerStatus()
```

Shell commands that exist only for the web view, such as the `frames://` protocol, data-URL thumbnails and the debug commands, aren't ported.

### 5.3 Bulk data: one call per screen, never one per tile

`timeline(filter)` returns the whole result as packed arrays, like a Photos fetch result. Each Swift view then holds it in memory.

| Field | Type | Bytes per asset |
|---|---|---|
| `ids` | 32-byte SHA-256, raw, concatenated | 32 |
| `takenAt` | Int64 milliseconds (local wall time for grouping) | 8 |
| `kind` and flags (video, live, date guessed, thumb failed, not uploaded) | UInt8 | 1 |
| `durationS` | UInt32 (videos) | 4 |
| `aspect` | width and height as UInt16 each | 4 |
| `sections` | month boundaries: start index and month key | — |

That is about 49 bytes per asset, or about 5 MB for 100,000. Details (filename, camera, albums) load on demand with `assetDetail`. ThumbHash strings come with `requestThumbs` results, not with the timeline.

After an edit or a sync, the observer reports `libraryChanged(version)`. The view requests a new snapshot and diffs it against the old one (by index ranges, since both are sorted) so it can animate inserts and removals.

### 5.4 Callbacks into Swift

* **`LibraryObserver`** (implemented in Swift, called on Rust threads). Callbacks:
  * `libraryChanged(version)`
  * `importProgress(status)`
  * `thumbsReady([(id, size, path, thumbhash)])`
  * `workerStatus(status)`
  * `syncState(online, pendingEdits)`

  Swift forwards each callback into an `AsyncStream` and handles it on the right actor. Nothing blocks the Rust thread.
* **`MediaBackend`** (implemented in Swift on the Mac; `frames-core` keeps its own Rust implementation for Windows). Methods:
  * `decodeForThumbnail(path, maxPixel) -> RgbaImage`: ImageIO with the orientation applied. It replaces the `sips` subprocess for HEIC and RAW.
  * `videoFrame(path, atMs, maxPixel) -> RgbaImage`: `AVAssetImageGenerator`. It replaces ffmpeg for formats AVFoundation can open.
  * `probeVideo(path) -> VideoInfo?`: `AVURLAsset`. If it returns nothing, the core falls back to ffprobe.

  Metadata (dates, camera, Live Photo pairing) stays in Rust, so both platforms date and pair files the same way.
* **Errors.** One `FramesError` enum: offline, auth, spaceFull, notFound, busy, damagedDatabase, other. Each carries a message the user can read.

### 5.5 Credentials

Swift stores and reads the S4 secret with the Security framework (Keychain) and passes it to `Library.open` and `saveSettings`. The core stops calling `/usr/bin/security` on the Mac. The Windows path doesn't change.

## 6. Compatibility contract

### 6.1 Bucket (unchanged)

* Keys as in `frames_design_spec.md` section 4: `originals/YYYY/MM/<name>`, clash names, `thumbs/<id>/s.webp` (short edge 300 px) and `m.webp` (long edge 1600 px), `index/log/<device>/<seq>.json`, and snapshots.
* Change log batch format and merge rules as in section 6 of that spec. Stamps are `<ISO time, ms, Z>|<device-id>` (`docs/spec-notes.md`).
* Filenames are normalised to NFC. Uploads carry the `x-amz-meta-sha256` tag.
* Unknown ops are skipped. So new op types (for example favourites, if added later) must be additive, and older apps simply ignore them.

### 6.2 Local data (shared with the Tauri app)

* Library folder: `~/Library/Application Support/app.frames.desktop/` holds `frames.sqlite3` and `settings.json`.
* Cache: `~/Library/Caches/app.frames.desktop/cache/` (unless Settings moves it). Small thumbnails are in `thumbs_s/<first 2 hex>/<id>.webp`, previews in `lru/previews/<id>.webp`.
* Schema and migrations stay owned by `frames-core`, so both apps always agree on them.
* The device id and next sequence number come from the database. The Mac keeps its existing identity (`mac-7df742fc`) and its log continues without a gap.

### 6.3 One app at a time (must be built first)

If the Tauri app and the Swift app ran at once with the same device id, both could write the same `<seq>.json`, and one batch would overwrite the other. That loses edits.

* `frames-core` takes an exclusive lock (`flock`) on `<data dir>/.lock` when the library opens and holds it until quit.
* A second app or instance that can't get the lock opens nothing and says: "Frames is already open in another app. Quit it, then try again."
* This change goes into the shared core, so the Tauri app gets it too. Ship it in a Tauri build before the Swift app exists.

## 7. Speed

### 7.1 Budgets

Measure on the owner's MacBook Air (M3, 8 GB, 60 Hz), with both the real library and a generated library of 100,000 items. A budget is met when it holds in Release builds, three runs out of three. On a 60 Hz display a frame lasts 16.7 ms. The code must still be written for 120 Hz (8.3 ms), so it stays smooth on ProMotion and external high-refresh displays.

| Measure | Budget |
|---|---|
| Launch to first grid frame, warm cache | under 400 ms |
| Launch to first grid frame, cold (after reboot) | under 1 s |
| Timeline snapshot, 100k items | under 50 ms in the core, under 10 ms to lay out |
| Scrolling, including a 10-second fling through 100k | no frame over the display interval (16.7 ms on the target Mac; main-thread work per frame under 8 ms); hitch time under 5 ms per second |
| Pinch or slider zoom between column counts | animated at the display rate, no blank tiles at rest |
| Visible tile from the memory cache | same frame |
| Visible tile from a local file | under 50 ms; a ThumbHash placeholder shows meanwhile |
| Viewer opens, preview cached | under 100 ms to the preview, under 300 ms to full resolution (local original) |
| Next or previous photo in the viewer | under 50 ms (neighbours prefetched) |
| Memory while browsing 100k | under 600 MB (the target Mac has 8 GB) |
| CPU when idle (5 s after the last input, no import) | 0 % on the main thread, no repeating timers |
| During an import | scrolling budgets still hold |

### 7.2 Timeline held in memory

* A view keeps its `TimelineSnapshot` (5.3). Layout is arithmetic, with no lookups: square cells, so row = index ÷ columns.
* Month headers are an array of offsets. The visible range comes from a binary search over them.
* Fill and fit change only how each image sits in its square (`contentsGravity`), never the layout.

### 7.3 The grid

The plan is a custom `GridView`: a layer-backed `NSView` inside `NSScrollView`.

* It keeps a pool of `CALayer`s, enough for the visible area plus the overscan, and reuses them. Nothing is allocated per frame.
* Each tile's `contents` is a `CGImage` already decoded at the tile's pixel size, so Core Animation only composites.
* Badges (video length, LIVE, date guessed, not uploaded yet) are sublayers whose text images are cached.
* Layers move inside a `CATransaction` with implicit animations disabled.
* Selection behaves as today: click, Shift, Command, marquee and whole month. Selection is drawn as a layer border plus a check mark, not by re-rendering the tile.
* **Zoom works like Photos.** Pinch or slider changes animate the visible layers around the pointer to the new size, then snap to the new column count.
* **Accessibility.** Every visible tile is an accessibility element ("Photo, 12 May 2024, 14:30"). The arrow keys move a focus ring.
* **Decided in Phase 0: `GridView`.** Both versions held 60 fps through 100k items, but `NSCollectionView` needed 5 to 6 ms of main-thread work per frame (9 ms worst), against about 1.1 ms (2.5 ms worst) for `GridView`. That breaks the 8 ms limit and leaves no room for 120 Hz, badges or selection. Numbers in section 18.

### 7.4 The image pipeline

1. **Placeholder.** The ThumbHash (already stored per asset) is decoded to a tiny image at once. A tile is never empty.
2. **Memory cache.** Decoded `CGImage`s keyed by asset and size class, with a cost limit of about 300 MB. On a memory-pressure warning it drops to 50 %.
3. **File.** `requestThumbs` returns local paths. The core serves them from its cache, fetches them from S4 (up to 6 at a time) or makes them from a local original. Requests go in batches, each carrying a request id.
4. **Decode.** ImageIO `CGImageSourceCreateThumbnailAtIndex` at the exact pixel size needed, with `kCGImageSourceShouldCacheImmediately`, so decoding finishes off the main thread. Concurrency equals the number of performance cores.
5. **Size class.** Use `s.webp`, unless the tile is over 360 pixels on screen (zoomed in, or on a Retina display), in which case use `m.webp`, as in the current app.
6. **Prefetch** follows direction and speed: two screens ahead and half a screen behind. During a fast fling (more than 3 screens a second), only placeholders are shown and decoding waits until the scroll is about to stop.
7. **Cancellation.** Each request carries the generation of the scroll position that asked for it. Requests that are off screen are cancelled before decoding starts, and their file requests are cancelled in the core.
8. **Profile first, then optional.** If decoding is the bottleneck at the densest zoom, add memory-mapped tile atlases of small (64 px) tiles per month. Don't build these before measuring.

### 7.5 The viewer

* **Opening zooms from the tile.** The tile's image is animated from the tile's frame to the viewer, then replaced by the preview and then by the full image. This needs no new decode at the start.
* The full image is decoded at screen size (ImageIO thumbnail at the screen's pixel size), not at the full 48 MP. Full resolution is decoded only past 1:1 zoom, region by region (`CATiledLayer`).
* The ±2 neighbours are prefetched and decoded at screen size, in the direction of travel first.
* Zoom and pan use magnification on `NSScrollView` (pinch, scroll with ⌥, double-click to 1:1).

### 7.6 Measuring

* Every pipeline stage (request, file ready, decode, on screen) and every view transition gets an `os_signpost` interval.
* A debug HUD, off in Release, shows a frame-time histogram (`NSView.displayLink`) and cache hit rates.
* Each phase in section 14 ends with a short performance report against 7.1, using Instruments (Time Profiler, Allocations, Animation Hitches where macOS supports it, Energy).

## 8. Features (parity with the Tauri app)

| Area | Behaviour | Mac implementation |
|---|---|---|
| Window | Sidebar, toolbar, full-screen, resizable sidebar | `NavigationSplitView`, unified toolbar |
| Sidebar | Library; Videos; Live Photos; Albums (with +, rename, delete, drop target); Imports; Recently Deleted; sync and storage status | SwiftUI `List` with sections; drop with `dropDestination` |
| Library | Years, Months and All Photos selector; sticky month headers; 3 to 15 columns; year scrubber | Segmented control in the toolbar; `GridView`; scrubber overlay |
| Years and Months | Cards with a key photo and a count; click to drill in | SwiftUI lazy grid (small item counts) |
| Selection | Click, Shift, Command, marquee, whole month, ⌘A; selection bar with the count and actions | `GridView` plus a toolbar or bottom-bar action group; context menus |
| Viewer | Double-click, Enter or Space to open; arrows; zoom; Esc to close; info panel with Adjust date, Export and Delete | `ViewerView`; SwiftUI `.inspector` |
| Video | Plays natively when possible; repackages or converts otherwise; "Can't play here" with Export | Section 10 |
| Live Photos | LIVE badge; hover over or press and hold to play the motion | `PHLivePhotoView` (section 10) |
| Albums | Create (also from a selection), rename, delete (photos kept); add, move, remove; drag to a sidebar album; newest photo is the cover | Context menus, sheets, drag and drop |
| Search | One field with suggestions grouped as Albums, Dates, Cameras, Types and Filenames, combining as tokens | `.searchable(text:tokens:)` over the core's FTS5 search |
| Recently Deleted | Days left; Restore, Delete now, Empty (each confirms with the count and size) | SwiftUI view plus a `GridView` |
| Imports | Choose a folder or drop files; progress (files, GB, time left), duplicates, errors; pause, resume, cancel; resumes after a restart | `NSOpenPanel`; drop on the window or on an album; SwiftUI progress view |
| Sync | Edits upload 2 s after the last one; pulls every 5 minutes, on wake and when the network returns; offline banner | Core timers plus `NWPathMonitor` and the wake notification, which call `syncNow` |
| Export | Originals to a folder; drag photos to Finder | `NSOpenPanel`; file promises (`NSFilePromiseProvider`) download on demand |
| Settings | Connection (endpoint, bucket, key); storage limit; cache location and size; Clear cache; Check library; Rebuild database; Appearance | SwiftUI `Settings` scene with tabs |
| Menus and keys | File: Import, New Album. Edit: Select All, Delete (to Recently Deleted). View: Zoom In and Out, Years, Months, All, appearance. Window. | `Commands`; same shortcuts as today, including ⌘⇧L to cycle appearance |
| Accessibility | VoiceOver labels, full keyboard use, Reduce Motion, Reduce Transparency, Increase Contrast | System settings read live |

## 9. Appearance

Three appearances, as today.

* **Standard.** The system's light or dark mode and an accent colour.
* **Glass.** Liquid Glass on the sidebar, toolbar and overlays, using `glassEffect` (SwiftUI) and `NSGlassEffectView`. With Reduce Transparency on, solid materials replace it.
* **Adaptive.** A port of the web engine. The TypeScript in `frames-shell/src/lib/theme/palette.ts` and `adaptive.ts` is the reference.
  * **Palette.** From the core (`palette(assetId)`: dominant, second and vivid colours, plus brightness).
  * **Roles.** `immersiveRoles`: canvas in the dominant hue, sidebar in the second hue a shade deeper, accent, hover fills and glows in the vivid colour, at fixed OKLCH tones per scheme (dark canvas L 0.22, light canvas L 0.965). Text contrast is at least 4.5:1, or 7:1 with Increase Contrast.
  * **Scheme.** "Follow photo" picks dark when brightness is below 0.45 and light above 0.55, otherwise it keeps the current scheme. Only an open photo decides; browsing changes colours but never flips light and dark.
  * **Priority.** Open photo first, then album page, then the photo under the pointer after 0.4 s, then the photo centred while scrolling (at most every 0.8 s).
  * **Album settings.** Dynamic, album auto or album manual. Manual colours are shown exactly.
  * **Transitions.** Colours change over 400 ms; instantly with Reduce Motion.
  * **Implementation.** A `Theme` value (`@Observable`) read by SwiftUI views and by the grid's layers. The maths is ported with unit tests against golden values generated from the TypeScript, so both apps produce the same colours.

## 10. Video and Live Photos

* **Native formats** (MOV, MP4, M4V, 3GP with H.264, HEVC, ProRes or AAC) play in `AVPlayerView`. The source is the local cached original if present, otherwise a presigned S4 URL. AVFoundation handles range requests and hardware decoding.
* **Other formats** go through the core's player: an ffmpeg repackage (`-c copy`) or a conversion to H.264 (`h264_videotoolbox`), served on 127.0.0.1 with a per-session token. **Decided in Phase 0: as HLS.**
  * `AVPlayer` rejects today's progressive fragmented-MP4 stream (`AVFoundationErrorDomain` −11850, "server incorrectly configured"), because it requires byte-range support, which a live conversion can't offer.
  * HLS works for both repackage and convert: fMP4 segments of about 2 s and an `EVENT` playlist that grows while ffmpeg writes (`-f hls -hls_segment_type fmp4 -hls_playlist_type event`). The player was ready in 0.2 to 0.3 s, including while the playlist was still growing.
  * Run ffmpeg as fast as it can, not in real time (no `-re`). With stream copy, segments can only end on the source's keyframes, so in real time the first segment can take a whole keyframe interval (8 s for the test file).
  * Segments go to a temporary folder in the cache and are deleted when playback ends or the app quits. This replaces the Tauri design's "nothing written to disk" for this path only.
  * Seeking within written segments uses the playlist. Seeking past them starts a new session at that time (`-ss`), as today.
  * The Windows app keeps its progressive stream, since its web view plays it.
* **Live Photos.** When a Live Photo opens, the photo and its MOV are fetched to the cache and handed to `PHLivePhoto.request(withResourceFileURLs:…)` for `PHLivePhotoView`, which gives Apple's own playback.
  * Fallback if pairing is rejected (for example a missing content identifier): an `AVPlayerLayer` playing the MOV once over the photo.
  * **Deferred to Phase 4** (owner's call, 2026-11-03): there are no Live Photos in the library yet, and a fake pair wouldn't prove Apple's pairing. Confirm both paths when a real one exists.
* The scrubber shows the stored duration while a converted stream is still filling.

## 11. Import

The pipeline in `frames_design_spec.md` section 7 is unchanged and runs in the core: scan, fingerprint, metadata, Live Photo pairing, thumbnails, upload, log. On the Mac:

* Decoding for thumbnails goes through `MediaBackend`: ImageIO for HEIC and RAW, `AVAssetImageGenerator` for video frames. Rust still resizes and encodes the WebP files (quality 80) and the ThumbHash.
* The app isn't sandboxed, so the folders the user picks need no security-scoped bookmarks. The last folder is remembered.
* Importing from the Apple Photos library through PhotoKit is out of scope (section 15).

## 12. Build, signing and packaging

* **Prerequisite:** install Xcode. This Mac has only the Command Line Tools (Swift 6.2.3), which can't run Instruments, sign with a team identity or compile asset catalogs.
* **Layout:**
  * `apps/mac/Frames.xcodeproj`, with sources in `apps/mac/Frames/` (from Phase 1);
  * `apps/mac/Package.swift`: the Phase 0 trial programs (`ffi-smoke`, `grid-spike`, `video-spike`);
  * `crates/frames-ffi` and `scripts/build-core-xcframework.sh` (5.1). The script sets `MACOSX_DEPLOYMENT_TARGET=26.0`; without it the C code inside the core (SQLite, libwebp, ring) targets the build Mac's own macOS version.
* **Bundle:** id `app.frames.mac`. Hardened runtime on; network-client entitlement; no sandbox.
* **ffmpeg and ffprobe** go in `Contents/Helpers/`, signed. The existing `scripts/prepare-binaries.js` provides them, with checksums verified.
* **Signing (still open):**
  * **Apple Development (free Apple ID team).** A stable identity, so Keychain access survives rebuilds, and enough for this Mac.
  * **Developer ID plus notarization (paid Apple Developer Program).** Needed only if the app should open on other Macs without a Gatekeeper warning.
  * **Not ad-hoc**, as the Tauri build is today. Each ad-hoc rebuild changes the code identity and the Keychain asks again.
* **Architecture:** arm64 only.
* **Release checklist:** version and build number, signature check (`codesign --verify --deep --strict`), a launch on a clean user account, and the performance report from 7.6.

## 13. Testing

* **Rust.** Keep the existing tests. Add FFI smoke tests (open a library, take a timeline snapshot, make an edit, receive a callback) and a test for the data-folder lock.
* **Swift (Swift Testing):**
  * theme maths against golden fixtures from `palette.ts`;
  * snapshot diffing and grid layout maths;
  * image pipeline cancellation and priority;
  * `MediaBackend` against the sample set (HEIC, DNG and one camera RAW, a Live Photo pair, MOV with HEVC, MKV, MTS, AVI, a photo with no date, a corrupt file).
* **Performance.** XCTest performance tests with signpost metrics for launch, snapshot and a scripted fling, plus the manual Instruments runs in 7.6.
* **Compatibility.**
  * The Swift app and the Tauri app on one MinIO test bucket: albums and trash made in one appear in the other.
  * The Mac's log sequence continues after switching apps.
  * The lock blocks a second app.
* **Real library.** Each phase ends with a session on the owner's library before the next phase starts.

## 14. Plan

Each phase ends with an app the owner can use on the real library and a performance report.

| Phase | Work | Done when |
|---|---|---|
| **0. Groundwork and spikes** | Data-folder lock in `frames-core`, shipped in a Tauri build. `frames-ffi`, XCFramework, a minimal Swift app listing albums. Grid spike (7.3 decision gate). AVPlayer stream spike and Live Photo spike (section 10). | Lock released; grid choice made with numbers; both video questions answered |
| **1. Browse** | Window, sidebar, Library (All, Years, Months), grid with the full image pipeline, viewer for photos, info panel, sync pull, offline state | Budgets in 7.1 met on the real library |
| **2. Edit** | Albums (all operations, drag and drop), Recently Deleted, Adjust date, Export (including drag to Finder), search, edits uploaded after 2 s | Every edit appears on the Windows app |
| **3. Import** | Folder and drop import, Imports view, pause, resume, cancel, resume after quit | A 5 GB trial folder imports with the scrolling budgets held throughout |
| **4. Video and Live Photos** | Native playback, conversion path, Live Photos | Sample set plays; "Can't play here" works |
| **5. Appearance** | Standard, Glass, Adaptive with album themes | Colours match the Tauri app's golden fixtures; contrast guards pass |
| **6. Settings and release** | Settings scene, Check library, Rebuild database, Clear cache, menus, signing, release checklist | The owner uses the Swift app as the main Mac app |

## 15. Not in this version

* **Windows.** The Tauri app continues.
* **iPhone and iPad.** The core and SwiftUI code are kept portable, so an iPad app is possible later.
* **Library features:**
  * photo editing;
  * favourites and hidden (they need new op types);
  * faces, places and memories;
  * sharing, encryption and any server.
* **Import from the Apple Photos library** (PhotoKit).
* **The placeholder sections** listed in decision 9.

## 16. Decisions

| Question | Answer (2026-11-03) |
|---|---|
| Minimum macOS 26? | Yes |
| Placeholder sections left out until real? | Yes |
| Intel Macs? | No, arm64 only |
| Windows stays on Tauri? | Yes |
| Xcode installed on this Mac? | To do (required before Phase 0) |
| Signing identity | Open. Recommended: free Apple Development, since the app runs only on this Mac |

## 17. Risks

| Risk | Mitigation |
|---|---|
| Two apps writing one log | Data-folder lock first (6.3), in both apps |
| FFI build friction (Rust + Xcode) | One script; bindings generated, never hand-edited; Phase 0 proves it end to end |
| Rust callbacks under Swift 6 strict concurrency | Callbacks only push into `AsyncStream`s; all state lives on actors |
| A custom grid is a lot of code | Decision gate against `NSCollectionView` in Phase 0 |
| AVPlayer and the converted stream | Phase 0 spike, with HLS as the fallback |
| Two copies of the theme maths drift | Golden fixtures generated from the TypeScript, checked in tests |
| Keychain prompts after every build | A stable signing identity (section 12) |

## 18. Phase 0 results (2026-11-03)

| Item | Result |
|---|---|
| Data-folder lock | Done. `frames_core::lock`, with a test. The Tauri app takes it at launch and shows "Frames is already open in another app" if it can't. Shipped in the Tauri build of 20:04. |
| `frames-ffi` and XCFramework | Done. `scripts/build-core-xcframework.sh` builds in about 3 minutes; the static library is 106 MB before linking. `ffi-smoke` opened a copy of the real library from Swift: 216 items, the "Portfolio" album, and a second open refused by the lock. |
| Grid | `GridView` chosen (7.3). Measured below. |
| Video | HLS chosen for non-native formats (section 10). |
| Live Photos | Deferred to Phase 4: no Live Photos in the library yet (section 10). |

**Grid trial.** 100,000 items in 494 months, cycling the 217 real cached thumbnails; window 1280 × 800 points, 7 columns; MacBook Air M3, 60 Hz. Each speed ran for 5 s, scrolled by the display link. "Main" is the main-thread time of one scroll step.

| Speed (screens/s) | `GridView` main p95 / worst | `NSCollectionView` main p95 / worst | Frames over budget |
|---|---|---|---|
| 1 | 1.06 / 2.50 ms | 5.27 / 7.66 ms | none in either |
| 2 | 1.12 / 1.37 ms | 5.96 / 8.05 ms | none in either |
| 6 | 0.75 / 1.11 ms | 6.09 / 9.08 ms | none in either |
| 20 | 1.41 / 2.33 ms | 6.01 / 7.04 ms | none in either |

* Decode latency: 9 to 13 ms median and 13 to 81 ms p95 while scrolling slowly. Above 3 screens a second no decodes start, by design.
* First layout: 211 ms for `GridView` warm (809 ms on a cold first launch); 285 ms for `NSCollectionView`.
* The first, cold `GridView` run had two stalls (163 ms and 50 ms) outside the scroll step; the repeat run had none.

**Follow-ups for Phase 1:**
* Memory ended at about 640 MB in both versions, over the 600 MB budget. It comes from the shared image pipeline, not the grid. Account for the decoded image cost properly and measure with the cache capped at 150 MB.
* Bring the cold first layout under the 1 s budget, and investigate the cold-start stalls with Instruments.
