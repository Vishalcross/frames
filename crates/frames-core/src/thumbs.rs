use std::fs;
use std::path::{Path, PathBuf};
use image::DynamicImage;
use rusqlite::Connection;

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

pub fn update_thumb_state(conn: &Connection, asset_id: &str, state: &str) {
    let _ = conn.execute(
        "UPDATE assets SET thumb_state = ?1 WHERE id = ?2",
        rusqlite::params![state, asset_id],
    );
}

fn decode_heic_or_fallback_via_ffmpeg(src: &Path) -> Result<DynamicImage, String> {
    let out = crate::media::spawn_hidden_command("ffmpeg")
        .arg("-y")
        .arg("-i")
        .arg(src)
        .arg("-frames:v")
        .arg("1")
        .arg("-f")
        .arg("image2pipe")
        .arg("-vcodec")
        .arg("mjpeg")
        .arg("-")
        .output()
        .map_err(|e| format!("failed to invoke ffmpeg: {e}"))?;

    if !out.status.success() || out.stdout.is_empty() {
        return Err("ffmpeg frame extraction failed".to_string());
    }

    image::load_from_memory(&out.stdout)
        .map_err(|e| format!("failed to decode ffmpeg output: {e}"))
}

/// Generates a thumbnail for an image file, resizing preserving aspect ratio.
/// Uses temp-then-rename to ensure atomic file generation.
pub fn generate_image_thumb(src: &Path, asset_id: &str, size: ThumbSize) -> Result<PathBuf, String> {
    generate_image_thumb_with_conn(src, asset_id, size, None)
}

pub fn generate_image_thumb_with_conn(
    src: &Path,
    asset_id: &str,
    size: ThumbSize,
    conn: Option<&Connection>,
) -> Result<PathBuf, String> {
    let result = (|| -> Result<PathBuf, String> {
        if !src.exists() {
            return Err(format!("source file does not exist: {}", src.display()));
        }

        let ext = src
            .extension()
            .and_then(|s| s.to_str())
            .unwrap_or("")
            .to_lowercase();

        let img = if ext == "heic" || ext == "heif" {
            image::open(src).or_else(|_| decode_heic_or_fallback_via_ffmpeg(src))?
        } else {
            image::open(src).or_else(|_| decode_heic_or_fallback_via_ffmpeg(src))?
        };

        let max_dim = size.pixels();
        let resized = img.resize(max_dim, max_dim, image::imageops::FilterType::Triangle);
        let rgb = resized.to_rgb8();
        let media_img = crate::media::RgbImage {
            width: rgb.width(),
            height: rgb.height(),
            data: rgb.into_raw(),
        };
        let webp_bytes = crate::media::encode_webp_lossless(&media_img);

        let dest = thumb_path(asset_id, size);
        if let Some(parent) = dest.parent() {
            fs::create_dir_all(parent).map_err(|e| e.to_string())?;
        }

        let temp_dest = dest.with_extension(format!("tmp.{}", fastrand::u64(..)));
        fs::write(&temp_dest, &webp_bytes).map_err(|e| e.to_string())?;
        fs::rename(&temp_dest, &dest).map_err(|e| e.to_string())?;

        Ok(dest)
    })();

    match &result {
        Ok(path) => {
            if let Some(c) = conn {
                update_thumb_state(c, asset_id, "ready");
            }
            Ok(path.clone())
        }
        Err(err) => {
            if let Some(c) = conn {
                update_thumb_state(c, asset_id, "failed");
            }
            Err(err.clone())
        }
    }
}

/// Generates a thumbnail for a video file by invoking ffmpeg.
/// Retries with -ss 0 if -ss 1 fails.
pub fn generate_video_thumb(src: &Path, asset_id: &str, size: ThumbSize) -> Result<PathBuf, String> {
    generate_video_thumb_with_conn(src, asset_id, size, None)
}

pub fn generate_video_thumb_with_conn(
    src: &Path,
    asset_id: &str,
    size: ThumbSize,
    conn: Option<&Connection>,
) -> Result<PathBuf, String> {
    let result = (|| -> Result<PathBuf, String> {
        if !src.exists() {
            return Err(format!("video file does not exist: {}", src.display()));
        }

        let dest = thumb_path(asset_id, size);
        if let Some(parent) = dest.parent() {
            fs::create_dir_all(parent).map_err(|e| e.to_string())?;
        }

        let scale_vf = format!("scale={}:-2", size.pixels());

        // Extract frame via ffmpeg to pipe as mjpeg.
        // Try -ss 1 first; if clip is shorter than 1s or seek fails, retry with -ss 0.
        let extract_frame = |ss: &str| -> Result<Vec<u8>, String> {
            let out = crate::media::spawn_hidden_command("ffmpeg")
                .arg("-y")
                .arg("-ss")
                .arg(ss)
                .arg("-i")
                .arg(src)
                .arg("-frames:v")
                .arg("1")
                .arg("-vf")
                .arg(&scale_vf)
                .arg("-f")
                .arg("image2pipe")
                .arg("-vcodec")
                .arg("mjpeg")
                .arg("-")
                .output()
                .map_err(|e| format!("failed to invoke ffmpeg: {e}"))?;

            if out.status.success() && !out.stdout.is_empty() {
                Ok(out.stdout)
            } else {
                Err(String::from_utf8_lossy(&out.stderr).to_string())
            }
        };

        let frame_bytes = extract_frame("1").or_else(|_| extract_frame("0"))?;
        let img = image::load_from_memory(&frame_bytes)
            .map_err(|e| format!("failed to decode video frame: {e}"))?;
        let rgb = img.to_rgb8();
        let media_img = crate::media::RgbImage {
            width: rgb.width(),
            height: rgb.height(),
            data: rgb.into_raw(),
        };
        let webp_bytes = crate::media::encode_webp_lossless(&media_img);

        let temp_dest = PathBuf::from(format!("{}.tmp.{}", dest.display(), fastrand::u64(..)));
        fs::write(&temp_dest, &webp_bytes).map_err(|e| e.to_string())?;
        fs::rename(&temp_dest, &dest).map_err(|e| e.to_string())?;

        Ok(dest)
    })();

    match &result {
        Ok(path) => {
            if let Some(c) = conn {
                update_thumb_state(c, asset_id, "ready");
            }
            Ok(path.clone())
        }
        Err(err) => {
            if let Some(c) = conn {
                update_thumb_state(c, asset_id, "failed");
            }
            Err(err.clone())
        }
    }
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

    #[test]
    fn generate_image_thumb_resizes_png_correctly() {
        // Create a 500x300 PNG test fixture
        let temp_dir = std::env::temp_dir().join(format!("frames_thumb_test_{}", fastrand::u64(..)));
        fs::create_dir_all(&temp_dir).unwrap();
        let png_src = temp_dir.join("test_sample.png");

        let img_buf = image::RgbImage::new(500, 300);
        img_buf.save(&png_src).unwrap();

        let asset_id = format!("test-img-{}", fastrand::u64(..));
        let out_path = generate_image_thumb(&png_src, &asset_id, ThumbSize::Small).unwrap();

        assert!(out_path.exists());
        assert!(thumb_exists(&asset_id, ThumbSize::Small));

        let decoded = image::open(&out_path).unwrap();
        let longest_edge = decoded.width().max(decoded.height());
        assert!(longest_edge <= 256, "Longest edge must be <= 256, got {longest_edge}");
        assert_eq!(longest_edge, 256);

        let _ = fs::remove_file(&png_src);
        let _ = fs::remove_file(&out_path);
        let _ = fs::remove_dir_all(&temp_dir);
    }

    #[test]
    fn generate_video_thumb_creates_valid_thumbnail() {
        let temp_dir = std::env::temp_dir().join(format!("frames_vid_test_{}", fastrand::u64(..)));
        fs::create_dir_all(&temp_dir).unwrap();
        let vid_src = temp_dir.join("test_sample.mp4");

        // Synthesize a 1-second 320x240 MP4 with ffmpeg lavfi testsrc
        let gen = crate::media::spawn_hidden_command("ffmpeg")
            .arg("-y")
            .arg("-f")
            .arg("lavfi")
            .arg("-i")
            .arg("testsrc=duration=1:size=320x240:rate=1")
            .arg("-c:v")
            .arg("libx264")
            .arg("-pix_fmt")
            .arg("yuv420p")
            .arg(&vid_src)
            .status();

        if let Ok(st) = gen {
            if st.success() && vid_src.exists() {
                let asset_id = format!("test-vid-{}", fastrand::u64(..));
                let out_path = generate_video_thumb(&vid_src, &asset_id, ThumbSize::Small).unwrap();

                assert!(out_path.exists());
                assert!(thumb_exists(&asset_id, ThumbSize::Small));

                let decoded = image::open(&out_path).unwrap();
                let longest_edge = decoded.width().max(decoded.height());
                assert!(longest_edge <= 256);

                let _ = fs::remove_file(&vid_src);
                let _ = fs::remove_file(&out_path);
            }
        }
        let _ = fs::remove_dir_all(&temp_dir);
    }
}
