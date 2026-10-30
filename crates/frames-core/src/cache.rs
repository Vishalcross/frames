//! Local thumbnail store and size-capped LRU preview/original cache (spec section 10):
//! - All `s.webp` small thumbnails are kept permanently on disk (`thumbs_s/<aa>/<id>.webp`).
//!   A new computer downloads missing small thumbnails in the background.
//! - `m.webp` previews and downloaded originals go into a size-capped LRU cache
//!   (default 10 GB), dropping the least-recently-used items first.

use std::collections::HashMap;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::keys::{thumb_preview_key, thumb_small_key};
use crate::s4::{ObjectStore, Result as S4Result};

/// Default LRU cap for `m.webp` previews and originals: 10 GB (spec section 9 & 10).
pub const DEFAULT_CACHE_LIMIT_BYTES: u64 = 10 * 1_000_000_000;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
struct LruEntry {
    rel_path: String,
    size_bytes: u64,
    last_access_seq: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CacheStats {
    pub small_thumb_count: usize,
    pub small_thumb_bytes: u64,
    pub lru_entry_count: usize,
    pub lru_used_bytes: u64,
    pub lru_limit_bytes: u64,
}

pub struct LocalCache {
    root: PathBuf,
    limit_bytes: u64,
    access_counter: u64,
    lru_entries: HashMap<String, LruEntry>,
    lru_used_bytes: u64,
}

impl LocalCache {
    pub fn open(root: impl AsRef<Path>, limit_bytes: u64) -> io::Result<Self> {
        let root = root.as_ref().to_path_buf();
        fs::create_dir_all(root.join("thumbs_s"))?;
        fs::create_dir_all(root.join("lru/previews"))?;
        fs::create_dir_all(root.join("lru/originals"))?;

        let mut cache = Self {
            root,
            limit_bytes,
            access_counter: 0,
            lru_entries: HashMap::new(),
            lru_used_bytes: 0,
        };
        cache.scan_existing_lru()?;
        Ok(cache)
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    pub fn set_limit_bytes(&mut self, limit_bytes: u64) -> io::Result<()> {
        self.limit_bytes = limit_bytes;
        self.evict_until_within(0)
    }

    fn scan_existing_lru(&mut self) -> io::Result<()> {
        for sub in ["lru/previews", "lru/originals"] {
            let dir = self.root.join(sub);
            if !dir.exists() {
                continue;
            }
            for entry in fs::read_dir(&dir)? {
                let entry = entry?;
                let meta = entry.metadata()?;
                if !meta.is_file() {
                    continue;
                }
                let fname = entry.file_name().to_string_lossy().to_string();
                let rel_path = format!("{sub}/{fname}");
                let size_bytes = meta.len();
                self.access_counter += 1;
                self.lru_used_bytes += size_bytes;
                self.lru_entries.insert(
                    rel_path.clone(),
                    LruEntry {
                        rel_path,
                        size_bytes,
                        last_access_seq: self.access_counter,
                    },
                );
            }
        }
        Ok(())
    }

    // =========================================================================
    // Permanent Small Thumbnails (`s.webp`)
    // =========================================================================

    fn small_thumb_path(&self, id: &str) -> PathBuf {
        let prefix: String = id.chars().take(2).collect();
        let sub = if prefix.is_empty() { "00" } else { &prefix };
        self.root.join("thumbs_s").join(sub).join(format!("{id}.webp"))
    }

    pub fn has_small_thumb(&self, id: &str) -> bool {
        self.small_thumb_path(id).is_file()
    }

    pub fn get_small_thumb_path(&self, id: &str) -> Option<PathBuf> {
        let p = self.small_thumb_path(id);
        if p.is_file() {
            Some(p)
        } else {
            None
        }
    }

    pub fn put_small_thumb(&self, id: &str, bytes: &[u8]) -> io::Result<PathBuf> {
        let path = self.small_thumb_path(id);
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        fs::write(&path, bytes)?;
        Ok(path)
    }

    /// Return the subset of `asset_ids` whose `s.webp` is not yet stored locally.
    pub fn missing_small_thumbs(&self, asset_ids: &[String]) -> Vec<String> {
        asset_ids
            .iter()
            .filter(|id| !self.has_small_thumb(id))
            .cloned()
            .collect()
    }

    /// Background downloader for a new computer (spec section 10): fetches any
    /// missing `thumbs/<id>/s.webp` from S4 and stores them locally.
    pub fn download_missing_small_thumbs(
        &self,
        s4: &dyn ObjectStore,
        asset_ids: &[String],
    ) -> S4Result<usize> {
        let mut downloaded = 0usize;
        for id in self.missing_small_thumbs(asset_ids) {
            let key = thumb_small_key(&id);
            if let Some(bytes) = s4.get_object(&key)? {
                self.put_small_thumb(&id, &bytes)?;
                downloaded += 1;
            }
        }
        Ok(downloaded)
    }

    // =========================================================================
    // Size-Capped LRU Preview (`m.webp`) & Original Cache
    // =========================================================================

    fn preview_rel_path(id: &str) -> String {
        format!("lru/previews/{id}.webp")
    }

    fn original_rel_path(id: &str, filename: &str) -> String {
        let clean_name: String = filename
            .chars()
            .filter(|c| c.is_ascii_alphanumeric() || *c == '.' || *c == '_' || *c == '-')
            .collect();
        let suffix = if clean_name.is_empty() {
            "file".to_string()
        } else {
            clean_name
        };
        format!("lru/originals/{id}-{suffix}")
    }

    pub fn get_preview_path(&mut self, id: &str) -> Option<PathBuf> {
        let rel = Self::preview_rel_path(id);
        self.touch_lru(&rel)
    }

    pub fn put_preview(&mut self, id: &str, bytes: &[u8]) -> io::Result<PathBuf> {
        let rel = Self::preview_rel_path(id);
        self.put_lru_entry(&rel, bytes)
    }

    /// Fetch `m.webp` from local LRU cache, or download from S4 and cache it on demand.
    pub fn get_or_fetch_preview(&mut self, s4: &dyn ObjectStore, id: &str) -> S4Result<Option<PathBuf>> {
        if let Some(p) = self.get_preview_path(id) {
            return Ok(Some(p));
        }
        let key = thumb_preview_key(id);
        if let Some(bytes) = s4.get_object(&key)? {
            let p = self.put_preview(id, &bytes)?;
            return Ok(Some(p));
        }
        Ok(None)
    }

    pub fn get_original_path(&mut self, id: &str, filename: &str) -> Option<PathBuf> {
        let rel = Self::original_rel_path(id, filename);
        self.touch_lru(&rel)
    }

    pub fn put_original(&mut self, id: &str, filename: &str, bytes: &[u8]) -> io::Result<PathBuf> {
        let rel = Self::original_rel_path(id, filename);
        self.put_lru_entry(&rel, bytes)
    }

    /// Fetch an original file from local LRU cache, or stream it directly from S4
    /// (`s4_key`) to disk without buffering the entire file in RAM.
    pub fn get_or_fetch_original(
        &mut self,
        s4: &dyn ObjectStore,
        id: &str,
        filename: &str,
        s4_key: &str,
    ) -> S4Result<Option<PathBuf>> {
        if let Some(p) = self.get_original_path(id, filename) {
            return Ok(Some(p));
        }
        let rel = Self::original_rel_path(id, filename);
        let full = self.root.join(&rel);
        if s4.get_object_to_file(s4_key, &full)? {
            let new_size = fs::metadata(&full).map(|m| m.len()).unwrap_or(0);
            if let Some(old) = self.lru_entries.remove(&rel) {
                self.lru_used_bytes = self.lru_used_bytes.saturating_sub(old.size_bytes);
            }
            self.evict_until_within(new_size)?;
            self.access_counter += 1;
            self.lru_used_bytes += new_size;
            self.lru_entries.insert(
                rel.clone(),
                LruEntry {
                    rel_path: rel,
                    size_bytes: new_size,
                    last_access_seq: self.access_counter,
                },
            );
            return Ok(Some(full));
        }
        Ok(None)
    }

    fn touch_lru(&mut self, rel_path: &str) -> Option<PathBuf> {
        let full = self.root.join(rel_path);
        if !full.is_file() {
            if let Some(removed) = self.lru_entries.remove(rel_path) {
                self.lru_used_bytes = self.lru_used_bytes.saturating_sub(removed.size_bytes);
            }
            return None;
        }
        self.access_counter += 1;
        if let Some(entry) = self.lru_entries.get_mut(rel_path) {
            entry.last_access_seq = self.access_counter;
        }
        Some(full)
    }

    fn put_lru_entry(&mut self, rel_path: &str, bytes: &[u8]) -> io::Result<PathBuf> {
        let new_size = bytes.len() as u64;
        if let Some(old) = self.lru_entries.remove(rel_path) {
            self.lru_used_bytes = self.lru_used_bytes.saturating_sub(old.size_bytes);
        }

        self.evict_until_within(new_size)?;

        let full = self.root.join(rel_path);
        if let Some(parent) = full.parent() {
            fs::create_dir_all(parent)?;
        }
        fs::write(&full, bytes)?;

        self.access_counter += 1;
        self.lru_used_bytes += new_size;
        self.lru_entries.insert(
            rel_path.to_string(),
            LruEntry {
                rel_path: rel_path.to_string(),
                size_bytes: new_size,
                last_access_seq: self.access_counter,
            },
        );
        Ok(full)
    }

    fn evict_until_within(&mut self, incoming_bytes: u64) -> io::Result<()> {
        if self.limit_bytes == 0 {
            return Ok(());
        }
        while !self.lru_entries.is_empty()
            && self.lru_used_bytes.saturating_add(incoming_bytes) > self.limit_bytes
        {
            // Find entry with minimum `last_access_seq`
            let Some(oldest_key) = self
                .lru_entries
                .values()
                .min_by_key(|e| e.last_access_seq)
                .map(|e| e.rel_path.clone())
            else {
                break;
            };
            if let Some(evicted) = self.lru_entries.remove(&oldest_key) {
                self.lru_used_bytes = self.lru_used_bytes.saturating_sub(evicted.size_bytes);
                let _ = fs::remove_file(self.root.join(&evicted.rel_path));
            }
        }
        Ok(())
    }

    /// Clear all LRU-cached previews and originals (Settings -> "Clear cache", spec section 9).
    /// Permanent `s.webp` thumbnails are preserved so offline timeline browsing continues to work.
    pub fn clear_lru_cache(&mut self) -> io::Result<u64> {
        let freed = self.lru_used_bytes;
        for entry in self.lru_entries.values() {
            let _ = fs::remove_file(self.root.join(&entry.rel_path));
        }
        self.lru_entries.clear();
        self.lru_used_bytes = 0;
        Ok(freed)
    }

    pub fn stats(&self) -> io::Result<CacheStats> {
        let mut small_thumb_count = 0usize;
        let mut small_thumb_bytes = 0u64;
        let thumbs_dir = self.root.join("thumbs_s");
        if thumbs_dir.exists() {
            for sub in fs::read_dir(thumbs_dir)? {
                let sub = sub?;
                if sub.file_type()?.is_dir() {
                    for f in fs::read_dir(sub.path())? {
                        let f = f?;
                        if let Ok(m) = f.metadata() {
                            if m.is_file() {
                                small_thumb_count += 1;
                                small_thumb_bytes += m.len();
                            }
                        }
                    }
                }
            }
        }
        Ok(CacheStats {
            small_thumb_count,
            small_thumb_bytes,
            lru_entry_count: self.lru_entries.len(),
            lru_used_bytes: self.lru_used_bytes,
            lru_limit_bytes: self.limit_bytes,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::s4::{InMemoryObjectStore, S4Config};

    struct TempCacheDir(PathBuf);
    impl TempCacheDir {
        fn new() -> Self {
            let p = std::env::temp_dir().join(format!("frames-cache-{}", fastrand::u64(..)));
            fs::create_dir_all(&p).unwrap();
            Self(p)
        }
    }
    impl Drop for TempCacheDir {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    #[test]
    fn lru_evicts_least_recently_used_preview_while_keeping_small_thumbs() {
        let dir = TempCacheDir::new();
        // Cap LRU at 25 bytes
        let mut cache = LocalCache::open(&dir.0, 25).unwrap();

        // Small thumb does not count against LRU and is never evicted
        cache.put_small_thumb("a1", &[1u8; 100]).unwrap();
        assert!(cache.has_small_thumb("a1"));

        // Put two 10-byte previews (total 20 <= 25)
        cache.put_preview("p1", &[10u8; 10]).unwrap();
        cache.put_preview("p2", &[20u8; 10]).unwrap();

        // Touch p1 so p2 becomes the least-recently-used
        assert!(cache.get_preview_path("p1").is_some());

        // Add p3 (10 bytes): 20 + 10 = 30 > 25 -> p2 must be evicted, p1 and p3 stay!
        cache.put_preview("p3", &[30u8; 10]).unwrap();
        assert!(cache.get_preview_path("p2").is_none());
        assert!(cache.get_preview_path("p1").is_some());
        assert!(cache.get_preview_path("p3").is_some());
        assert!(cache.has_small_thumb("a1"));

        // Clear cache removes LRU entries but preserves small thumbs
        let freed = cache.clear_lru_cache().unwrap();
        assert_eq!(freed, 20);
        assert!(cache.get_preview_path("p1").is_none());
        assert!(cache.has_small_thumb("a1"));
    }

    #[test]
    fn background_downloads_missing_small_thumbs_for_new_device() {
        let dir = TempCacheDir::new();
        let cache = LocalCache::open(&dir.0, DEFAULT_CACHE_LIMIT_BYTES).unwrap();
        let s4 = InMemoryObjectStore::new(S4Config::default());

        s4.put_object("thumbs/ab12/s.webp", b"webp-small-1", "image/webp")
            .unwrap();
        s4.put_object("thumbs/cd34/s.webp", b"webp-small-2", "image/webp")
            .unwrap();

        let ids = vec!["ab12".to_string(), "cd34".to_string()];
        assert_eq!(cache.missing_small_thumbs(&ids).len(), 2);

        let count = cache.download_missing_small_thumbs(&s4, &ids).unwrap();
        assert_eq!(count, 2);
        assert!(cache.missing_small_thumbs(&ids).is_empty());
        assert!(cache.has_small_thumb("ab12"));
        assert!(cache.has_small_thumb("cd34"));
    }
}
