# Frames

A personal, Apple Photos-style desktop app for Mac and Windows. The photo and
video library lives in MEGA S4 (S3-compatible object storage); each computer
talks to the bucket directly, with no server in between.

The design is in [docs/design-spec.md](docs/design-spec.md) and is the source of
truth. [docs/spec-notes.md](docs/spec-notes.md) records what was resolved, what
the code adds to the spec, and what is still open.

## Layout

```
docs/                    the spec and running handover notes (docs/spec-notes.md)
frames_design_spec.md    canonical copy of the design spec
crates/frames-core/      core library, tested on its own (65 unit + integration tests)
  schema.sql               local SQLite schema (spec section 5)
  src/keys.rs              bucket keys, name-clash naming, log/snapshot keys
  src/dates.rs             date-taken fallback chain, filename dates, folder month
  src/ops.rs               change-log batch types (spec sections 5-6)
  src/store.rs             SQLite store, merge engine, outbox, snapshots, FTS5 search
  src/media.rs             EXIF/QuickTime/ffprobe metadata, RAW preview, WebP thumbnails
  src/importer.rs          recursive scanner, SHA-256 fingerprint, Live Photo pairing
  src/s4.rs                S4/S3 client, SigV4 signing, presigned URLs, multipart, keychain
  src/cache.rs             permanent s.webp store + size-capped LRU preview/original cache
  src/trash.rs             30-day trash auto-purge, delete now, empty trash
  src/sync.rs              outbox flush, log pull, snapshots, import runner, check/rebuild
  src/player.rs            video play/repackage/convert decision + 127.0.0.1 stream server
  tests/                   schema and multi-device sync integration tests
frames-shell/            Tauri 2 (Rust shell) + Svelte 5 UI
```

## Working on it

Needs stable Rust (rustup) and a C compiler for the bundled SQLite (Xcode
command line tools on Mac, MSVC Build Tools on Windows; Tauri needs them too).

```
cargo test -p frames-core
```

Commit `Cargo.lock` after the first build. Local databases and caches are
git-ignored and must stay out of the repo.

## Status

All 8 slices (100% of v1 architecture in [docs/design-spec.md](docs/design-spec.md), all 26 first-round adversarial review fixes from [frames_review.md](frames_review.md), and all 7 second-round pre-import blockers) are implemented and verified:
- **Slices 1–5 (`frames-core`)**: `keys`, `dates`, `ops`, `store`, `media`, `importer`, `s4`, `cache`, `trash`, `sync`, and `player` (`65` unit tests + `6` integration tests passing).
- **Slice 6 (`frames-shell`)**: Tauri 2 Rust command bridge + async custom `frames://` binary media protocol + virtualized Apple Photos-style Svelte 5 desktop UI (`1` isolated state test, `0` `svelte-check` warnings).
- **Slices 7–8 (Adversarial Review & Pre-Import Blockers)**: No silent fake-S4 fallback, propagated Keychain errors, 31-second exponential retry backoff, accurate `curl` exit-code classification, lock-free folder hashing, unlocked async `frames://` S4 downloads + background `s.webp` sync, non-blocking startup trash purge, single-import guard + 4-worker work-stealing upload pool + per-part multipart cancel, Windows HTML5 drag-drop compatibility, and local S3/MinIO HTTP + live S4 smoke tests (`0` `cargo clippy --workspace --all-targets` warnings).
See [docs/spec-notes.md](docs/spec-notes.md) for detailed handover notes.
