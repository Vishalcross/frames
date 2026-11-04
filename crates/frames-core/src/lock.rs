//! One app at a time per library. The Tauri app and the native Mac app share
//! the data folder and the device id, so if both ran they could write the
//! same `index/log/<device>/<seq>.json` and one batch would overwrite the
//! other. Whichever opens the library first holds `<data dir>/.lock`.

use std::fs::{File, OpenOptions, TryLockError};
use std::path::Path;

/// Held while the library is open. The OS releases it when the process
/// exits, crash included, so a stale lock never blocks the next launch.
#[derive(Debug)]
pub struct LibraryLock {
    _file: File,
}

impl LibraryLock {
    /// Lock the library in `data_dir`, or `Ok(None)` if another app or
    /// instance already has it open.
    pub fn acquire(data_dir: &Path) -> std::io::Result<Option<LibraryLock>> {
        std::fs::create_dir_all(data_dir)?;
        let file = OpenOptions::new()
            .create(true)
            .truncate(false)
            .write(true)
            .open(data_dir.join(".lock"))?;
        match file.try_lock() {
            Ok(()) => Ok(Some(LibraryLock { _file: file })),
            Err(TryLockError::WouldBlock) => Ok(None),
            Err(TryLockError::Error(err)) => Err(err),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_second_opener_is_refused_until_the_first_closes() {
        let dir = std::env::temp_dir().join(format!("frames-lock-{}", fastrand::u64(..)));
        let first = LibraryLock::acquire(&dir).unwrap();
        assert!(first.is_some());
        assert!(LibraryLock::acquire(&dir).unwrap().is_none());
        drop(first);
        assert!(LibraryLock::acquire(&dir).unwrap().is_some());
        let _ = std::fs::remove_dir_all(&dir);
    }
}
