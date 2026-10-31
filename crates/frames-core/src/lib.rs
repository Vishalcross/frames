//! Frames core logic. Pure functions and plain data types: no network, no
//! disk, no UI. The Tauri app and the future S3, importer, sync and player
//! modules build on this crate, and it is tested on its own.
//!
//! Spec: `docs/design-spec.md`.

pub mod cache;
pub mod dates;
pub mod importer;
pub mod keys;
pub mod media;
pub mod ops;
pub mod player;
pub mod s4;
pub mod store;
pub mod sync;
pub mod trash;

pub use store::MatchedAlbumFolder;

/// The local SQLite schema (spec section 5), applied when a database is created.
pub const SCHEMA_SQL: &str = include_str!("../schema.sql");
