# Frames

A personal, Apple Photos-style desktop app for Mac and Windows. The photo and
video library lives in MEGA S4 (S3-compatible object storage); each computer
talks to the bucket directly, with no server in between.

The design is in [docs/design-spec.md](docs/design-spec.md) and is the source of
truth. [docs/spec-notes.md](docs/spec-notes.md) records what was resolved, what
the code adds to the spec, and what is still open.

## Layout

```
docs/                    the spec and running notes
crates/frames-core/      pure logic, tested on its own (no I/O, no UI)
  schema.sql               local SQLite schema (spec section 5)
  src/keys.rs              bucket keys, name-clash naming, log/snapshot keys
  src/dates.rs             date-taken fallback chain, filename dates, folder month
  src/ops.rs               change-log batch types (spec sections 5-6)
  tests/schema.rs          loads schema.sql into real SQLite, FTS5 included
```

Coming later, one slice at a time: `src-tauri/` (the Tauri 2 shell) and `ui/`
(Svelte), then the modules in spec section 3 (`store`, `s4`, `sync`,
`importer`, `media`, `player`, `cache`, `trash`).

## Working on it

Needs stable Rust (rustup) and a C compiler for the bundled SQLite (Xcode
command line tools on Mac, MSVC Build Tools on Windows; Tauri needs them too).

```
cargo test -p frames-core
```

Commit `Cargo.lock` after the first build. Local databases and caches are
git-ignored and must stay out of the repo.

## Status

Slice 1: the core crate. `schema.sql` has been loaded and queried in SQLite and
the filename-date pattern checked against its test cases. The Rust itself has
not been compiled yet; run the tests on first checkout and fix whatever the
compiler finds.
