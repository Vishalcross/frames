//! Frames core logic. Pure functions and plain data types: no network, no
//! disk, no UI. The Tauri app and the future S3, importer, sync and player
//! modules build on this crate, and it is tested on its own.
//!
//! Spec: `docs/design-spec.md`.

pub mod album_palette;
pub mod cache;
pub mod color;
pub mod dates;
pub mod importer;
pub mod keys;
pub mod media;
pub mod ops;
pub mod palette;
pub mod player;
pub mod s4;
pub mod store;
pub mod sync;
pub mod trash;
pub mod scan_local;
pub mod thumbs;
pub mod thumb_worker;
pub mod upload_worker;

pub use store::{AlbumDebugInfo, MatchedAlbumFolder};
pub use s4::{RemoteAsset, RemoteFetcher};
pub use upload_worker::UploadWorker;
pub use palette::{
    extract_palette, get_album_theme, get_palette_from_db, normalise_roles, set_album_theme,
    AlbumTheme, Palette, Tone,
};
pub use album_palette::{derive_album_palette, generate_and_save_album_palette};

/// The local SQLite schema (spec section 5), applied when a database is created.
pub const SCHEMA_SQL: &str = include_str!("../schema.sql");
