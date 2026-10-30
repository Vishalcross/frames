# Frames — review, fixes, next steps

Reviewed against [frames_design_spec.md](file:///Users/vishal/Downloads/imp/frames/frames_design_spec.md). Static review plus builds and tests. Nothing was run against a real S4 bucket, ffmpeg or a real photo library.

## 1. Verification results

| Check | Result |
|---|---|
| `cargo test --workspace --offline` | **Pass.** 61 tests: 54 core unit, 3 schema, 3 sync, 1 shell. |
| `svelte-check` | **Pass.** 0 errors, 0 warnings. |
| `cargo clippy` | 15 warnings, all style (`map_or`, arg counts, `from_str` naming). |
| README claims | Out of date. It says "57" and "60" tests, and the real count is 61. |

**Overall:** the core logic is solid on the in-memory fake: store, merge rules, sync, trash and the decision matrix. The app is **not yet usable on a real library**. Several parts have only been tested against `InMemoryObjectStore`, and the UI is a prototype. The status line "100% of v1 implemented and verified" overstates this.

## 2. Spec coverage

| Spec area | Status |
|---|---|
| Schema, keys, dates, ops (§4–5) | ✅ Done and tested |
| Merge rules, snapshots (§6) | ✅ Done and tested on two devices |
| Import pipeline (§7) | ⚠️ Works end to end, but it is sequential, has no real resume, and has a hashing pass up front |
| S4 client (§3) | ⚠️ Works through the `curl` CLI, which has bugs (see below) |
| Playback (§8) | ⚠️ Decision logic is good. The server has bugs and ffmpeg is not bundled. |
| Photo viewing on Windows (§8) | ❌ `prepare_photo_for_webview` exists but no command calls it |
| UI (§9) | ⚠️ Most features are present but it is not virtualised. Import, export and drag-and-drop UX differ from the spec. |
| Keychain (§11) | ⚠️ macOS only. Windows load is a stub. |
| Bundled ffmpeg, libheif, vibrancy and Mica (§2, 9) | ❌ Missing |
| MinIO and real-S4 tests (§12) | ❌ Missing |

## 3. Fixes (prioritised)

### P0 — blocks real use / data safety

1. **The app defaults to a throwaway in-memory S4.** `use_real_s4` is false by default, and `AppState` holds `demo_s4`. The SQLite index persists across restarts but the "bucket" does not. After a restart the index points at objects that no longer exist.
   - Fix: make the real S4 the default. Isolate the demo/seed mode behind an explicit dev flag, or drop it and use a temp data dir.
   - Location: [lib.rs](file:///Users/vishal/Downloads/imp/frames/frames-shell/src-tauri/src/lib.rs#L123-L220)
2. **The startup 30-day purge runs against `demo_s4`.** It runs before settings and the real S4 are used. With real S4, expired trash is never purged from the bucket, but it is marked purged in the index. That leaves orphan objects and wastes quota. Fix: run the purge after the real client is built, and then daily (spec §11).
3. **Resuming a multipart upload computes the wrong ETag.** A resumed part's ETag is computed as `sha256(chunk)` ([s4.rs:425](file:///Users/vishal/Downloads/imp/frames/crates/frames-core/src/s4.rs#L425)). Real S3 and S4 part ETags are MD5, so `CompleteMultipartUpload` will fail with `InvalidPart` on resume. The mock accepts anything. Fix: call `ListParts` for the real ETags.
4. **Import never actually resumes.** `run_import` always calls `upload_file_with_resume(.., None, 0, ..)`. It writes `import_queue` rows but never reads them back to continue. Every run re-hashes the whole source, and 550 GB hashed before the first upload is a long time. Spec §7 requires resume after unplugging the disk or quitting. Fix: hash lazily per file in a pipeline, skip rows that are `logged`, and resume rows that are `uploading` using the stored upload id and parts.
5. **Commands block the UI and hold locks for the whole operation.** `import_from_paths`, `sync_now`, `export_assets`, `run_rebuild_database` and the thumbnail command are synchronous. They hold the `store` and `cache` mutexes for the entire run, so a multi-hour import freezes browsing, search and the trash. There is no progress event (spec §9 "Imports: progress, pause/resume"). Fix: run them as async or background tasks and emit Tauri events. Take the lock per file or batch. Add pause and cancel.
6. **The `curl` transport has bugs for the real bucket.** [s4.rs:729](file:///Users/vishal/Downloads/imp/frames/crates/frames-core/src/s4.rs#L729)
   - `HEAD` is sent as `-X HEAD` without `-I`. curl then waits for a body that never comes, so it can hang. The import name-clash check uses HEAD on every file.
   - There are no connect or total timeouts (`--max-time`, `--connect-timeout`).
   - Every request buffers the whole body in RAM, including `get_object` for originals and videos in `cache`/export. A 4 GB video means 4 GB of memory.
   - Request bodies go through stdin and responses are parsed from a combined `-i` dump.
   - Fix: use a real HTTP client (e.g. `ureq` or `reqwest` with `rustls`), or at least `-I` for HEAD, timeouts, and streaming to a file for downloads. This also removes the dependency on system `curl` on Windows.
7. **The S4 secret key is passed on the command line.** `security add-generic-password -w <secret>` and `cmdkey /pass:<secret>` are visible to other local processes in `ps`. The Windows `load_secret_from_os_keychain` is a stub that returns `None`, because `cmdkey` cannot read passwords back. On Windows the key is lost on restart. Fix: use the `keyring` crate, or the native Security Framework and Credential Manager APIs.

### P1 — correctness and robustness

8. **ffmpeg stderr is piped but never read** ([player.rs:567-571](file:///Users/vishal/Downloads/imp/frames/crates/frames-core/src/player.rs#L567-L571)). When the stderr pipe fills, ffmpeg blocks and the stream stalls. Fix: use `Stdio::null()` or drain on a thread, and pass `-loglevel error`.
9. **The player server leaks and has weak isolation.**
   - Sessions are never removed.
   - A thread is spawned per connection with no limit.
   - Seeking opens a new ffmpeg without stopping the old one until its write fails.
   - A single token is shared by all sessions, with `Access-Control-Allow-Origin: *`.
   - There is no `Range`, `Content-Length` or `206` handling, so the scrubber depends on the app restarting at `?t=`.
   - Presigned URLs expire after 1 hour, so long sessions and Live Photos fail.
   - On Windows, `Command::new("ffmpeg")` flashes a console window (needs `CREATE_NO_WINDOW`).
10. **The player server silently falls back to a fake.** If the `LoopbackStreamServer::start` bind fails, the code swaps in `new_virtual_for_test(49152, ..)`. Videos then fail with no explanation. Surface the error.
11. **Windows HEIC and RAW photos won't display.** The viewer only ever shows the `m.webp` preview via a base64 data URL, and never the full original. `prepare_photo_for_webview` ([player.rs:268](file:///Users/vishal/Downloads/imp/frames/crates/frames-core/src/player.rs#L268)) is not wired to any command or custom protocol (spec §8: "then the full original"). Fix: add a Tauri custom protocol such as `frames://original/<id>`.
12. **The timeline isn't virtualised, and thumbnails are slow.** Spec targets are 100k items, under 2 s open, and smooth scrolling.
    - `get_app_bootstrap` returns every asset to the UI, and the Svelte grid renders every cell ([+page.svelte](file:///Users/vishal/Downloads/imp/frames/frames-shell/src/routes/+page.svelte#L1120-L1160)).
    - Thumbnails come one at a time through IPC as base64 data URLs (`get_thumbnail_data_url`), and only the first 120 are prefetched.
    - Fix: a virtualised grid with a layout computed from the month groups, and a custom protocol or `asset:` URLs for thumbnails (`frames://thumb/<id>`). Fetch small thumbs in the background (`download_missing_small_thumbs`).
13. **A failed multi-file `export_assets` loses files on name collision.** `export_originals` copies to `dest.join(filename)`, so two `IMG_0001.JPG` from different months overwrite each other. Add a suffix on clash.
14. **The import ignores the "4 files in parallel" requirement** (§7.6). Uploads are sequential. At ~200 Mbps this can double the 7–9 h estimate.
15. **Import and export UX doesn't match the spec.**
    - Import: a typed path in a text field. There is no folder picker and no drag-and-drop of files from the OS (the sidebar only handles in-app drag to album).
    - Export: a text field defaulting to `/tmp/frames-export`. It needs `tauri-plugin-dialog`, the `onDragDropEvent` webview event, and the matching capabilities.
16. **The shell test writes to the real user data dir.** `app_state_initialises_and_seeds_sample_library` calls `AppState::init()`, which opens the real `frames.sqlite3` and runs a purge. It also doesn't seed anything, despite its name. Make `init` take a data dir and use a temp dir in tests.
17. **Hard-coded `mac-local` fallback and `.expect(...)` on startup.** If the DB can't be opened the app silently switches to an in-memory DB, so changes are lost. Show an error with "Rebuild database". `device_id` prefixes (`mac-`/`win-`) also aren't set on first run from the shell.
18. **CSP is `null`** in [tauri.conf.json](file:///Users/vishal/Downloads/imp/frames/frames-shell/src-tauri/tauri.conf.json). Set a CSP that allows `img-src` for the thumbnail scheme and `media-src` for `127.0.0.1` and the S4 host.
19. **`taken_at` for photos without an offset is stored as UTC.** This was a recorded decision, but it makes search by date and "Select month" differ by timezone for these files. Add a check and document it on the info panel.

### P2 — spec gaps and cleanup

20. **ffmpeg, ffprobe and libheif are not bundled** (§2 #12). The code calls `ffmpeg` from `PATH`. A user without it gets placeholder thumbnails and no video conversion. Configure `bundle.externalBin` and ship per-platform binaries.
21. **Thumbnails are lossless WebP** ([media.rs](file:///Users/vishal/Downloads/imp/frames/crates/frames-core/src/media.rs): `encode_webp_lossless`) when ffmpeg is unavailable. The spec estimates ~25 KB per `s.webp`, but lossless 300 px thumbnails are several times larger. Check on a sample set against the 2–3 GB per 100k estimate.
22. **No vibrancy or Mica sidebar** (§9). Add `window-vibrancy` (`macOSPrivateApi` or effects config).
23. **Unicode filename normalisation** (NFC vs NFD) is noted as unhandled in `spec-notes.md`. A Mac-created name and the same name on Windows become different keys.
24. **Check library** reports unknown keys but doesn't offer to add them (§11).
25. **Pull performance.** `pull_remote_logs` lists the whole `index/log/` prefix and fetches sequentially every 5 minutes. Fine for now. Add compaction as the spec notes.
26. **Docs and repo hygiene.**
    - `docs/design-spec.md` and `frames_design_spec.md` are duplicate copies; keep one.
    - README test counts are wrong.
    - Package name and bundle identifier are still `frames-shell` and `com.vishal.frames-shell`, with the default author "you" and description "A Tauri App".
    - `frames-shell` uses Rust edition 2024 and core 2021.
    - Fix the 15 clippy warnings.

## 4. Next steps (suggested order)

1. **Make it real.** Do P0 items 1, 2, 6 and 7. Swap the transport for a proper HTTP client, use `keyring`, default to real S4, and fix the purge. First action: in the MEGA account, create the S4 bucket and key, and check the open items from spec §13 (region and endpoint, range requests on presigned URLs, egress limits).
2. **Test against real storage.** Add a MinIO (Docker) integration test for the `S4Client`, covering HEAD, multipart, resume (`ListParts`), list pagination and presigned `Range`. Then one smoke test against the real S4 test bucket, as the spec says in §12.
3. **Rework import as a background pipeline.** Lazy hashing, resume from `import_queue`, 4 parallel uploads, progress events, pause and cancel (items 4, 5, 14).
4. **UI performance.** Virtualised grid, thumbnail and original custom protocol, fetch thumbs in the background, then generate a 100k-item fixture and measure against the §12 targets (items 11, 12).
5. **Playback hardening.** Fix stderr draining, session cleanup, URL expiry and Windows console flashing (items 8–10). Bundle ffmpeg.
6. **Native pickers and drag-drop.** Add `tauri-plugin-dialog` and OS file drop (item 15).
7. **Sample set and manual checks (§12).** Build the sample set (HEIC, DNG, a camera RAW, Live Photo, MTS, AVI, MKV, HEVC MOV, no-date and corrupt files). Do a two-computer sync test and the hands-on Mac and Windows checks.
8. **Trial import.** Import one ~5 GB folder and inspect the bucket with another S3 tool before the full 550 GB run.
9. **Build and packaging.** Decide the Mac and Windows build machines and how the repo moves between them. Add signing and notarisation on the Mac. Recommend the Microsoft HEVC add-on on Windows.

> [!IMPORTANT]
> Don't run the full 550 GB import until P0 items 3–6 are fixed. The multipart resume bug and the HEAD hang would only show up on the real bucket, partway through a multi-hour import.
