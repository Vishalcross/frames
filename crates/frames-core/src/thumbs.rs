use std::path::PathBuf;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ThumbSize {
    Small = 256,
    Large = 1024,
}

impl ThumbSize {
    pub fn pixels(self) -> u32 {
        self as u32
    }
}

pub fn app_cache_dir() -> PathBuf {
    if let Ok(dir) = std::env::var("FRAMES_CACHE_DIR") {
        PathBuf::from(dir)
    } else {
        dirs::cache_dir()
            .unwrap_or_else(std::env::temp_dir)
            .join("app.frames.desktop")
            .join("cache")
    }
}

pub fn thumb_path(asset_id: &str, size: ThumbSize) -> PathBuf {
    app_cache_dir()
        .join("thumbs")
        .join(size.pixels().to_string())
        .join(format!("{asset_id}.webp"))
}

pub fn thumb_exists(asset_id: &str, size: ThumbSize) -> bool {
    thumb_path(asset_id, size).exists()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn thumb_path_ends_with_expected_format() {
        let id = "test-asset-123";
        let path = thumb_path(id, ThumbSize::Small);
        let path_str = path.to_string_lossy().replace('\\', "/");
        assert!(path_str.ends_with("thumbs/256/test-asset-123.webp"));

        let path_large = thumb_path(id, ThumbSize::Large);
        let path_large_str = path_large.to_string_lossy().replace('\\', "/");
        assert!(path_large_str.ends_with("thumbs/1024/test-asset-123.webp"));
    }
}
