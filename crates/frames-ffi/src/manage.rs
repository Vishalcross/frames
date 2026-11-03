//! Imports, settings and library maintenance for the Mac app's Imports view
//! and Settings window (frames_swift_spec.md sections 8 and 11).

use std::path::PathBuf;
use std::sync::Arc;

use chrono::Utc;
use frames_core::s4::save_secret_in_os_keychain;
use frames_core::sync::{check_library, import_unknown_s4_keys, rebuild_from_s4, run_import_concurrent};

use crate::{finish_import, spawn, FramesError, Library, Settings, WorkClass};

/// Where the current or last import stands, for the Imports view.
#[derive(Debug, Clone, uniffi::Record)]
pub struct ImportStatus {
    pub running: bool,
    pub paused: bool,
    /// Across the import queue: every file an import has taken on.
    pub total_files: u64,
    pub total_bytes: u64,
    pub completed_files: u64,
    pub completed_bytes: u64,
    pub duplicates: u64,
    pub failed: u64,
    /// The last run that finished.
    pub last: Option<ImportSummary>,
}

#[derive(Debug, Clone, uniffi::Record)]
pub struct ImportSummary {
    pub imported: u64,
    pub duplicates: u64,
    /// Files that aren't photos or videos, by name.
    pub skipped: Vec<String>,
    /// Files that failed, with why.
    pub failures: Vec<ImportFailure>,
    pub stopped_offline: bool,
    pub stopped_space_full: bool,
}

#[derive(Debug, Clone, uniffi::Record)]
pub struct ImportFailure {
    pub path: String,
    pub reason: String,
}

/// The Settings window's Connection, Storage and Library panes.
#[derive(Debug, Clone, uniffi::Record)]
pub struct LibrarySettings {
    pub use_s4: bool,
    pub endpoint: String,
    pub region: String,
    pub bucket: String,
    pub access_key_id: String,
    /// Whether the Keychain holds a secret for `access_key_id`. The secret
    /// itself never leaves Rust.
    pub has_secret: bool,
    pub space_limit_gb: u64,
    pub cache_location: String,
    pub cache_limit_gb: u64,
    pub device_id: String,
    pub data_folder: String,
}

#[derive(Debug, Clone, uniffi::Record)]
pub struct CacheInfo {
    pub thumbnails: u64,
    pub thumbnail_bytes: u64,
    pub cached_bytes: u64,
    pub cached_limit_bytes: u64,
}

#[derive(Debug, Clone, uniffi::Record)]
pub struct LibraryCheck {
    /// Assets whose original is missing from S4.
    pub missing_originals: u64,
    /// Originals in S4 that the library doesn't know, by key.
    pub unknown_files: Vec<String>,
    /// Uploads left over from a crash, now tidied away.
    pub tidied_uploads: u64,
    pub used_bytes: u64,
    pub limit_bytes: u64,
}

/// An album's colour setting for the Adaptive appearance: each photo's own
/// colours, or one palette for the whole album, generated from its photos or
/// chosen by hand. Kept on this Mac only, as in the Tauri app.
#[derive(Debug, Clone, uniffi::Record)]
pub struct AlbumColours {
    pub one_palette: bool,
    pub chosen: bool,
    pub chosen_colours: Option<ColourSet>,
    pub generated: Option<ColourSet>,
}

/// Background, sidebar and accent, as `#RRGGBB`.
#[derive(Debug, Clone, uniffi::Record)]
pub struct ColourSet {
    pub bg: String,
    pub sidebar: String,
    pub accent: String,
}

/// A photo's colours for the Adaptive appearance, as hex, with its brightness.
#[derive(Debug, Clone, uniffi::Record)]
pub struct PhotoPalette {
    pub bg: String,
    pub sidebar: String,
    pub accent: String,
    pub lum: Option<f32>,
}

#[uniffi::export]
impl Library {
    // Imports (spec section 7). One at a time; progress arrives as
    // `import_changed` calls, and `import_status` says where it stands.

    /// Import files and folders, into an album if given. Returns at once.
    pub fn import_paths(&self, paths: Vec<String>, album_id: Option<String>) -> Result<(), FramesError> {
        let s4 = self.s4().ok_or(FramesError::NotConnected)?;
        if self.imports.control.is_running() {
            return Err(FramesError::other("An import is already running."));
        }
        let (store, cache, thumbs) = (Arc::clone(&self.store), Arc::clone(&self.cache), Arc::clone(&self.thumbs));
        let (imports, observer) = (self.imports.clone(), Arc::clone(&self.observer));
        spawn("frames-import", WorkClass::Utility, move || {
            let progress = Arc::clone(&observer);
            let paths: Vec<PathBuf> = paths.into_iter().map(PathBuf::from).collect();
            let report = run_import_concurrent(paths, album_id.as_deref(), store, s4, cache, Arc::clone(&imports.control),
                                               Utc::now(), move |_| progress.import_changed());
            finish_import(report.map(Some), &imports, &thumbs, observer.as_ref());
        })?;
        self.observer.import_changed();
        Ok(())
    }

    pub fn import_status(&self) -> Result<ImportStatus, FramesError> {
        let progress = self.store.lock().map_err(FramesError::other)?.import_progress().map_err(FramesError::other)?;
        let last = self.imports.last_report.lock().map_err(FramesError::other)?.clone().map(|r| ImportSummary {
            imported: r.imported_count as u64,
            duplicates: r.duplicates_skipped as u64,
            skipped: r.unknown_files.iter().map(|f| f.path.to_string_lossy().to_string()).collect(),
            failures: r.failed_files.iter().map(|f| ImportFailure {
                path: f.path.to_string_lossy().to_string(),
                reason: f.reason.clone(),
            }).collect(),
            stopped_offline: r.paused_offline,
            stopped_space_full: r.stopped_space_full,
        });
        Ok(ImportStatus {
            running: self.imports.control.is_running(),
            paused: self.imports.control.is_paused(),
            total_files: progress.total_files as u64,
            total_bytes: progress.total_bytes,
            completed_files: progress.completed_files as u64,
            completed_bytes: progress.completed_bytes,
            duplicates: progress.skipped_duplicates as u64,
            failed: progress.failed_files as u64,
            last,
        })
    }

    pub fn pause_import(&self) {
        self.imports.control.pause();
        self.observer.import_changed();
    }

    pub fn resume_import(&self) {
        self.imports.control.resume();
        self.observer.import_changed();
    }

    /// Stop the import, and drop the files it hadn't reached so they aren't
    /// resumed later. Files already uploaded stay in the library.
    pub fn cancel_import(&self) -> Result<(), FramesError> {
        self.imports.control.cancel();
        self.store.lock().map_err(FramesError::other)?.cancel_unfinished_imports().map_err(FramesError::other)?;
        self.observer.import_changed();
        Ok(())
    }

    // Settings (spec section 9: Settings).

    pub fn settings(&self) -> Result<LibrarySettings, FramesError> {
        let s = self.settings.lock().map_err(FramesError::other)?.clone();
        let device_id = self.store.lock().map_err(FramesError::other)?.device_id().map_err(FramesError::other)?;
        let has_secret = !s.access_key_id.is_empty()
            && frames_core::s4::load_secret_from_os_keychain(&s.access_key_id).ok().flatten().is_some_and(|k| !k.is_empty());
        Ok(LibrarySettings {
            use_s4: s.use_real_s4,
            endpoint: s.endpoint,
            region: s.region,
            bucket: s.bucket,
            access_key_id: s.access_key_id,
            has_secret,
            space_limit_gb: s.space_limit_gb,
            cache_location: s.cache_location,
            cache_limit_gb: s.cache_limit_gb,
            device_id,
            data_folder: self.data_dir.to_string_lossy().to_string(),
        })
    }

    /// Save the settings (the same `settings.json` the Tauri app uses), put a
    /// new secret in the Keychain, and reconnect. A new cache location takes
    /// effect the next time the library opens.
    pub fn save_settings(&self, settings: LibrarySettings, secret: Option<String>) -> Result<(), FramesError> {
        let new = settings_from(&settings);
        let secret = secret.map(|s| s.trim().to_string()).filter(|s| !s.is_empty());
        if let Some(secret) = &secret {
            if new.access_key_id.is_empty() {
                return Err(FramesError::other("Enter the Access Key ID that goes with this secret."));
            }
            save_secret_in_os_keychain(&new.access_key_id, secret)
                .map_err(|e| FramesError::other(format!("The secret key couldn't be saved in the Keychain: {e}")))?;
        }
        new.save(&self.data_dir)?;
        if let Ok(mut cache) = self.cache.lock() {
            let _ = cache.set_limit_bytes(new.cache_limit_gb * 1_000_000_000);
        }
        *self.settings.lock().map_err(FramesError::other)? = new;
        self.connect(secret.as_deref());
        Ok(())
    }

    /// Try settings before saving them: reach the bucket with them once.
    pub fn test_connection(&self, settings: LibrarySettings, secret: Option<String>) -> Result<(), FramesError> {
        let mut candidate = settings_from(&settings);
        candidate.use_real_s4 = true;
        let s4 = candidate.s4(secret.as_deref().filter(|s| !s.trim().is_empty())).map_err(FramesError::other)?;
        s4.head_object("index/connection-check").map(|_| ()).map_err(|e| FramesError::other(format!("Couldn't reach the bucket: {e}")))
    }

    pub fn cache_info(&self) -> Result<CacheInfo, FramesError> {
        let stats = self.cache.lock().map_err(FramesError::other)?.stats().map_err(FramesError::other)?;
        Ok(CacheInfo {
            thumbnails: stats.small_thumb_count as u64,
            thumbnail_bytes: stats.small_thumb_bytes,
            cached_bytes: stats.lru_used_bytes,
            cached_limit_bytes: stats.lru_limit_bytes,
        })
    }

    /// Empty the cache of previews and originals; thumbnails stay. Returns the bytes freed.
    pub fn clear_cache(&self) -> Result<u64, FramesError> {
        self.cache.lock().map_err(FramesError::other)?.clear_lru_cache().map_err(FramesError::other)
    }

    // Maintenance (spec section 11).

    /// Compare the bucket with the library: missing originals, unknown files,
    /// and uploads left over from a crash (which it tidies).
    pub fn check_library(&self) -> Result<LibraryCheck, FramesError> {
        let s4 = self.s4().ok_or(FramesError::NotConnected)?;
        let report = check_library(&mut &*self.store, s4.as_ref()).map_err(FramesError::other)?;
        let space = report.space_usage;
        Ok(LibraryCheck {
            missing_originals: report.missing_originals.len() as u64,
            unknown_files: report.unknown_s4_keys,
            tidied_uploads: report.tidied_multipart_uploads as u64,
            used_bytes: space.as_ref().map_or(0, |s| s.used_bytes),
            limit_bytes: space.as_ref().map_or(0, |s| s.limit_bytes),
        })
    }

    /// Add originals found in S4 but not in the library. Returns how many.
    pub fn add_unknown_files(&self, keys: Vec<String>) -> Result<u64, FramesError> {
        let s4 = self.s4().ok_or(FramesError::NotConnected)?;
        let added = import_unknown_s4_keys(&mut &*self.store, s4.as_ref(), &self.cache, &keys, Utc::now())
            .map_err(FramesError::other)?;
        self.thumbs.start_backfill(None);
        Ok(added as u64)
    }

    /// Throw the local database away and load it again from S4's snapshot and logs.
    pub fn rebuild_database(&self) -> Result<(), FramesError> {
        let s4 = self.s4().ok_or(FramesError::NotConnected)?;
        rebuild_from_s4(&mut &*self.store, s4.as_ref()).map_err(FramesError::other)?;
        self.thumbs.start_backfill(None);
        Ok(())
    }

    /// Where to play a video from: the original on this Mac, else a link to
    /// it in S4 that works for an hour. A path starts with `/`.
    pub fn video_source(&self, asset_id: String) -> Option<String> {
        if let Some(local) = self.thumbs.local_original(&asset_id) {
            return Some(local.to_string_lossy().to_string());
        }
        let key = self.thumbs.original_info(&asset_id)?.key;
        self.s4()?.presign_get(&key, Utc::now(), 3600).ok()
    }

    /// The same for a Live Photo's movie.
    pub fn motion_source(&self, asset_id: String) -> Option<String> {
        if let Some(local) = self.thumbs.local_motion(&asset_id) {
            return Some(local.to_string_lossy().to_string());
        }
        let key = self.thumbs.motion_info(&asset_id)?.key;
        self.s4()?.presign_get(&key, Utc::now(), 3600).ok()
    }

    pub fn album_colours(&self, album_id: String) -> Result<AlbumColours, FramesError> {
        let store = self.store.lock().map_err(FramesError::other)?;
        let theme = frames_core::palette::get_album_theme(store.connection(), &album_id).map_err(FramesError::other)?;
        Ok(album_colours_from(theme))
    }

    /// Save an album's colour setting. A generated palette is made now if the
    /// album has none yet.
    pub fn set_album_colours(&self, album_id: String, one_palette: bool, chosen: bool,
                             colours: Option<ColourSet>) -> Result<AlbumColours, FramesError> {
        let store = self.store.lock().map_err(FramesError::other)?;
        let conn = store.connection();
        let (bg, sidebar, accent) = match colours {
            Some(c) => (Some(c.bg), Some(c.sidebar), Some(c.accent)),
            None => (None, None, None),
        };
        let mode = if one_palette { "album" } else { "dynamic" };
        let source = if chosen { "manual" } else { "auto" };
        let mut theme = frames_core::palette::set_album_theme(conn, &album_id, mode, source, bg, sidebar, accent)
            .map_err(FramesError::other)?;
        if one_palette && !chosen && theme.auto_bg.is_none() {
            let _ = frames_core::album_palette::generate_and_save_album_palette(conn, self.thumbs.cache_root(), &album_id);
            theme = frames_core::palette::get_album_theme(conn, &album_id).map_err(FramesError::other)?;
        }
        Ok(album_colours_from(theme))
    }

    /// Work the album's palette out again from its photos.
    pub fn regenerate_album_colours(&self, album_id: String) -> Result<AlbumColours, FramesError> {
        let store = self.store.lock().map_err(FramesError::other)?;
        let conn = store.connection();
        frames_core::album_palette::generate_and_save_album_palette(conn, self.thumbs.cache_root(), &album_id)
            .map_err(FramesError::other)?;
        let theme = frames_core::palette::get_album_theme(conn, &album_id).map_err(FramesError::other)?;
        Ok(album_colours_from(theme))
    }

    /// An album the asset is in, whose colour setting applies when the photo
    /// is opened from the library (as in the Tauri app).
    pub fn album_of(&self, asset_id: String) -> Option<String> {
        let store = self.store.lock().ok()?;
        store
            .connection()
            .query_row(
                "SELECT album_id FROM album_items ai JOIN albums a ON a.id = ai.album_id
                 WHERE ai.asset_id = ?1 AND ai.in_album = 1 AND a.deleted_at IS NULL LIMIT 1",
                [&asset_id],
                |r| r.get(0),
            )
            .ok()
    }

    /// The photo's colours for the Adaptive appearance, worked out from its
    /// thumbnail the first time. None until the thumbnail exists.
    pub fn palette(&self, asset_id: String) -> Option<PhotoPalette> {
        let store = self.store.lock().ok()?;
        let conn = store.connection();
        let stored = frames_core::palette::get_palette_from_db(conn, &asset_id).ok().flatten();
        let palette = match stored {
            Some(p) if p.lum.is_some() => p,
            _ if self.thumbs.small_thumb_path(&asset_id).exists() => {
                frames_core::palette::compute_and_store_palette(conn, self.thumbs.cache_root(), &asset_id).ok()?
            }
            other => other?,
        };
        Some(PhotoPalette { bg: palette.bg, sidebar: palette.sidebar, accent: palette.accent, lum: palette.lum })
    }
}

fn album_colours_from(theme: frames_core::AlbumTheme) -> AlbumColours {
    let set = |bg: Option<String>, sidebar: Option<String>, accent: Option<String>| match (bg, sidebar, accent) {
        (Some(bg), Some(sidebar), Some(accent)) => Some(ColourSet { bg, sidebar, accent }),
        _ => None,
    };
    AlbumColours {
        one_palette: theme.mode == "album",
        chosen: theme.source == "manual",
        chosen_colours: set(theme.bg, theme.sidebar, theme.accent),
        generated: set(theme.auto_bg, theme.auto_sidebar, theme.auto_accent),
    }
}

fn settings_from(s: &LibrarySettings) -> Settings {
    Settings {
        endpoint: s.endpoint.trim().to_string(),
        region: s.region.trim().to_string(),
        bucket: s.bucket.trim().to_string(),
        access_key_id: s.access_key_id.trim().to_string(),
        space_limit_gb: s.space_limit_gb.max(1),
        cache_location: s.cache_location.trim().to_string(),
        cache_limit_gb: s.cache_limit_gb.max(1),
        use_real_s4: s.use_s4,
    }
}
