//! Swift bindings for frames-core (frames_swift_spec.md section 5). The only
//! crate the Mac app links. Everything here is a thin wrapper: the logic stays
//! in frames-core, shared with the Windows app.

use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use frames_core::lock::LibraryLock;
use frames_core::store::{SearchFilter, Store};

uniffi::setup_scaffolding!();

#[derive(Debug, thiserror::Error, uniffi::Error)]
pub enum FramesError {
    #[error("Frames is already open in another app. Quit it, then try again.")]
    AlreadyOpen,
    #[error("{message}")]
    Other { message: String },
}

impl FramesError {
    fn other(err: impl std::fmt::Display) -> Self {
        Self::Other { message: err.to_string() }
    }
}

#[derive(Debug, Clone, uniffi::Record)]
pub struct AlbumSummary {
    pub id: String,
    pub name: String,
    pub created_at_ms: i64,
    pub asset_count: u64,
    pub cover_asset_id: Option<String>,
}

/// An open library: the local database, held under the library lock for as
/// long as this object lives.
#[derive(uniffi::Object)]
pub struct Library {
    store: Mutex<Store>,
    _lock: LibraryLock,
}

#[uniffi::export]
impl Library {
    /// Open the library in `data_dir`, the folder holding `frames.sqlite3`.
    #[uniffi::constructor]
    pub fn open(data_dir: String) -> Result<Arc<Self>, FramesError> {
        let dir = PathBuf::from(data_dir);
        let lock = LibraryLock::acquire(&dir)
            .map_err(FramesError::other)?
            .ok_or(FramesError::AlreadyOpen)?;
        let store = Store::open(dir.join("frames.sqlite3")).map_err(FramesError::other)?;
        Ok(Arc::new(Self { store: Mutex::new(store), _lock: lock }))
    }

    /// Photos and videos in the library, trash excluded.
    pub fn asset_count(&self) -> Result<u64, FramesError> {
        let store = self.store.lock().map_err(FramesError::other)?;
        let count = store.count_assets(&SearchFilter::default()).map_err(FramesError::other)?;
        Ok(count as u64)
    }

    pub fn albums(&self) -> Result<Vec<AlbumSummary>, FramesError> {
        let store = self.store.lock().map_err(FramesError::other)?;
        let albums = store.list_albums().map_err(FramesError::other)?;
        Ok(albums
            .into_iter()
            .map(|a| AlbumSummary {
                id: a.id,
                name: a.name,
                created_at_ms: a.created_at.timestamp_millis(),
                asset_count: a.asset_count as u64,
                cover_asset_id: a.cover_asset_id,
            })
            .collect())
    }
}
