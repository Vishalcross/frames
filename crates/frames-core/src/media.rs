//! Media inspection, metadata extraction, orientation, and thumbnail generation
//! (spec sections 4, 7, 8, 12).
//!
//! Supported capabilities:
//! - File extension classification (Photos, Videos, Ignored `.DS_Store` /
//!   `Thumbs.db` / `.AAE`, or Unknown).
//! - Streamed SHA-256 fingerprinting (`<id>`).
//! - Pure-Rust EXIF/TIFF, JPEG, PNG, WebP, and ISOBMFF (MOV, MP4, HEIC, CR3)
//!   metadata reader (DateTimeOriginal, OffsetTimeOriginal, QuickTime creation
//!   date, dimensions, EXIF orientation, camera make/model, video/audio codecs,
//!   duration, and Apple Live Photo `ContentIdentifier`), supplemented by
//!   `ffprobe` when available on the system.
//! - Embedded JPEG preview extraction from RAW files (TIFF/DNG/CR2/NEF/ARW/CR3).
//! - EXIF orientation transformation (`1..=8`), aspect-preserving resize for
//!   `s.webp` (short edge ~300 px) and `m.webp` (long edge ~1600 px), WebP
//!   encoding (`ffmpeg` when present, pure-Rust VP8L lossless WebP fallback),
//!   and placeholder thumbnail generation (`thumb_failed = true`) on decode failure.

use std::fs::File;
use std::io::{self, BufReader, Read};
use std::path::Path;
use std::process::Command;

use byteorder::{BigEndian, ByteOrder, LittleEndian};
use chrono::{DateTime, NaiveDate, NaiveDateTime, NaiveTime, TimeZone, Utc};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::ops::AssetKind;

pub const SMALL_THUMB_SHORT_EDGE: u32 = 300;
pub const PREVIEW_THUMB_LONG_EDGE: u32 = 1600;

/// Supported photo extensions (lowercase, spec section 7 step 1).
pub const PHOTO_EXTENSIONS: &[&str] = &[
    "jpg", "jpeg", "heic", "heif", "png", "gif", "webp", "tiff", "tif", "dng", "cr2", "cr3",
    "nef", "arw",
];

/// Supported video extensions (lowercase, spec section 7 step 1).
pub const VIDEO_EXTENSIONS: &[&str] = &[
    "mov", "mp4", "m4v", "avi", "mts", "m2ts", "mkv", "3gp", "wmv",
];

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum FileClassification {
    Photo { ext: String },
    Video { ext: String },
    Ignored,
    Unknown { ext: String },
}

/// Classify a file path according to spec section 7, step 1:
/// - Ignore `.DS_Store`, `Thumbs.db`, `.AAE` (and AppleDouble `._*` metadata files).
/// - Classify known photo and video extensions.
/// - Everything else is `Unknown` ("skipped: unknown type").
pub fn classify_path(path: &Path) -> FileClassification {
    let Some(file_name) = path.file_name().and_then(|n| n.to_str()) else {
        return FileClassification::Ignored;
    };
    let lower_name = file_name.to_ascii_lowercase();
    if lower_name == ".ds_store" || lower_name == "thumbs.db" || lower_name.starts_with("._") {
        return FileClassification::Ignored;
    }
    let ext = path
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("")
        .to_ascii_lowercase();
    if ext == "aae" {
        return FileClassification::Ignored;
    }
    if PHOTO_EXTENSIONS.contains(&ext.as_str()) {
        return FileClassification::Photo { ext };
    }
    if VIDEO_EXTENSIONS.contains(&ext.as_str()) {
        return FileClassification::Video { ext };
    }
    FileClassification::Unknown { ext }
}

pub fn is_raw_extension(ext: &str) -> bool {
    matches!(
        ext.to_ascii_lowercase().as_str(),
        "dng" | "cr2" | "cr3" | "nef" | "arw"
    )
}

pub fn is_heic_extension(ext: &str) -> bool {
    matches!(ext.to_ascii_lowercase().as_str(), "heic" | "heif")
}

/// Resolve a bundled sidecar or system binary (`"ffmpeg"` or `"ffprobe"`), checking:
/// 1. Adjacent to `std::env::current_exe()` and `../Resources/bin` (bundled app resources).
/// 2. Common macOS Homebrew locations (`/opt/homebrew/bin`, `/usr/local/bin`).
/// 3. Fallback to `PATH`.
pub fn resolve_tool_binary(tool: &str) -> std::path::PathBuf {
    let exe_name = if cfg!(target_os = "windows") && !tool.ends_with(".exe") {
        format!("{tool}.exe")
    } else {
        tool.to_string()
    };

    if let Ok(cur_exe) = std::env::current_exe() {
        if let Some(dir) = cur_exe.parent() {
            for candidate in [
                dir.join(&exe_name),
                dir.join("bin").join(&exe_name),
                dir.join("../Resources").join(&exe_name),
                dir.join("../Resources/bin").join(&exe_name),
            ] {
                if candidate.is_file() {
                    return candidate;
                }
            }
        }
    }

    #[cfg(target_os = "macos")]
    {
        for dir in ["/opt/homebrew/bin", "/usr/local/bin"] {
            let candidate = Path::new(dir).join(&exe_name);
            if candidate.is_file() {
                return candidate;
            }
        }
    }

    std::path::PathBuf::from(exe_name)
}

/// Create a `Command` for `tool` (`"ffmpeg"` or `"ffprobe"`) using `resolve_tool_binary`
/// and setting `CREATE_NO_WINDOW` (`0x0800_0000`) on Windows so console windows never flash.
pub fn spawn_hidden_command(tool: &str) -> Command {
    let bin = resolve_tool_binary(tool);
    #[allow(unused_mut)]
    let mut cmd = Command::new(bin);
    #[cfg(target_os = "windows")]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        cmd.creation_flags(CREATE_NO_WINDOW);
    }
    cmd
}

// =============================================================================
// Streamed SHA-256 Fingerprinting (Spec Section 7, Step 2)
// =============================================================================

/// Compute lowercase hex SHA-256 of a file using a 64 KB streaming buffer.
pub fn sha256_file(path: impl AsRef<Path>) -> io::Result<String> {
    let file = File::open(path)?;
    sha256_reader(BufReader::with_capacity(64 * 1024, file))
}

/// Compute lowercase hex SHA-256 from any `Read` stream.
pub fn sha256_reader(mut reader: impl Read) -> io::Result<String> {
    let mut hasher = Sha256::new();
    let mut buf = [0u8; 64 * 1024];
    loop {
        let n = reader.read(&mut buf)?;
        if n == 0 {
            break;
        }
        hasher.update(&buf[..n]);
    }
    Ok(to_hex_lower(&hasher.finalize()))
}

/// Compute lowercase hex SHA-256 of an in-memory byte slice.
pub fn sha256_bytes(bytes: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(bytes);
    to_hex_lower(&hasher.finalize())
}

fn to_hex_lower(bytes: &[u8]) -> String {
    let mut out = String::with_capacity(bytes.len() * 2);
    for &b in bytes {
        use std::fmt::Write;
        let _ = write!(&mut out, "{b:02x}");
    }
    out
}

// =============================================================================
// Metadata Reading (Spec Section 7, Step 3)
// =============================================================================

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct MediaMetadata {
    /// EXIF DateTimeOriginal (wall-clock, no timezone).
    pub exif_local: Option<NaiveDateTime>,
    /// EXIF OffsetTimeOriginal in minutes east of UTC (e.g. +05:30 -> 330).
    pub exif_offset_minutes: Option<i32>,
    /// QuickTime / MP4 creation date in UTC.
    pub quicktime_utc: Option<DateTime<Utc>>,
    /// Display width after applying EXIF orientation.
    pub width: Option<u32>,
    /// Display height after applying EXIF orientation.
    pub height: Option<u32>,
    /// EXIF orientation tag (1..=8; 1 = normal).
    pub orientation: u16,
    pub camera_make: Option<String>,
    pub camera_model: Option<String>,
    /// Apple Live Photo ContentIdentifier (links photo and MOV).
    pub content_identifier: Option<String>,
    /// Video duration in milliseconds.
    pub duration_ms: Option<u64>,
    /// Container format (e.g. "mov", "mp4", "mkv", "avi", "mts").
    pub container: Option<String>,
    /// Lowercase video codec name (e.g. "h264", "hevc").
    pub video_codec: Option<String>,
    /// Lowercase audio codec name (e.g. "aac", "pcm_s16le").
    pub audio_codec: Option<String>,
}

/// Read metadata from a file on disk. Reads up to the first 2 MB for fast
/// pure-Rust header parsing, and for videos (or when needed) enriches with
/// `ffprobe` if available.
pub fn read_metadata(path: impl AsRef<Path>) -> io::Result<MediaMetadata> {
    let path = path.as_ref();
    let mut file = File::open(path)?;
    let mut header = Vec::with_capacity(256 * 1024);
    (&mut file).take(2 * 1024 * 1024).read_to_end(&mut header)?;

    let ext = path
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("")
        .to_ascii_lowercase();

    let mut meta = parse_metadata_from_bytes(&header, &ext);

    if VIDEO_EXTENSIONS.contains(&ext.as_str()) {
        if meta.container.is_none() && !ext.is_empty() {
            meta.container = Some(ext.clone());
        }
        probe_with_ffprobe(path, &mut meta);
    }

    Ok(meta)
}

/// Pure-Rust metadata parser over file header bytes.
pub fn parse_metadata_from_bytes(bytes: &[u8], ext: &str) -> MediaMetadata {
    let mut meta = MediaMetadata {
        orientation: 1,
        ..Default::default()
    };

    if bytes.starts_with(&[0xFF, 0xD8, 0xFF]) {
        parse_jpeg(bytes, &mut meta);
    } else if bytes.starts_with(b"\x89PNG\r\n\x1a\n") {
        parse_png(bytes, &mut meta);
    } else if bytes.len() >= 12 && &bytes[0..4] == b"RIFF" && &bytes[8..12] == b"WEBP" {
        parse_webp(bytes, &mut meta);
    } else if bytes.starts_with(b"II\x2a\x00") || bytes.starts_with(b"MM\x00\x2a") {
        parse_tiff_exif(bytes, &mut meta);
    } else if bytes.len() >= 8 && is_isobmff_header(bytes) {
        parse_isobmff(bytes, &mut meta);
    }

    // Apply orientation swap to dimensions (orientations 5, 6, 7, 8 rotate 90/270 deg)
    if matches!(meta.orientation, 5..=8) {
        if let (Some(w), Some(h)) = (meta.width, meta.height) {
            meta.width = Some(h);
            meta.height = Some(w);
        }
    }

    if VIDEO_EXTENSIONS.contains(&ext) && meta.container.is_none() {
        meta.container = Some(ext.to_string());
    }

    meta
}

fn is_isobmff_header(bytes: &[u8]) -> bool {
    if bytes.len() < 8 {
        return false;
    }
    matches!(
        &bytes[4..8],
        b"ftyp" | b"moov" | b"mdat" | b"wide" | b"free" | b"skip" | b"meta"
    )
}

// =============================================================================
// JPEG / PNG / WebP Parsers
// =============================================================================

fn parse_jpeg(bytes: &[u8], meta: &mut MediaMetadata) {
    let mut i = 2;
    while i + 4 <= bytes.len() {
        if bytes[i] != 0xFF {
            i += 1;
            continue;
        }
        while i < bytes.len() && bytes[i] == 0xFF {
            i += 1;
        }
        if i >= bytes.len() {
            break;
        }
        let marker = bytes[i];
        i += 1;

        // Standalone markers without length
        if marker == 0xD8 || marker == 0xD9 || (0xD0..=0xD7).contains(&marker) || marker == 0x01 {
            if marker == 0xD9 || marker == 0xDA {
                break;
            }
            continue;
        }
        if i + 2 > bytes.len() {
            break;
        }
        let seg_len = BigEndian::read_u16(&bytes[i..i + 2]) as usize;
        if seg_len < 2 || i + seg_len > bytes.len() {
            break;
        }
        let payload = &bytes[i + 2..i + seg_len];

        if marker == 0xE1 {
            if let Some(tiff_bytes) = payload.strip_prefix(b"Exif\0\0") {
                parse_tiff_exif(tiff_bytes, meta);
            } else if let Some(xmp_bytes) = payload.strip_prefix(b"http://ns.adobe.com/xap/1.0/\0")
            {
                parse_xmp_packet(xmp_bytes, meta);
            }
        } else if matches!(
            marker,
            0xC0 | 0xC1 | 0xC2 | 0xC3 | 0xC5 | 0xC6 | 0xC7 | 0xC9 | 0xCA | 0xCB | 0xCD | 0xCE | 0xCF
        ) && payload.len() >= 5
        {
            let h = BigEndian::read_u16(&payload[1..3]) as u32;
            let w = BigEndian::read_u16(&payload[3..5]) as u32;
            if w > 0 && h > 0 {
                meta.width = Some(w);
                meta.height = Some(h);
            }
        }

        if marker == 0xDA {
            break;
        }
        i += seg_len;
    }
}

fn parse_png(bytes: &[u8], meta: &mut MediaMetadata) {
    let mut i = 8;
    while i + 8 <= bytes.len() {
        let len = BigEndian::read_u32(&bytes[i..i + 4]) as usize;
        let chunk_type = &bytes[i + 4..i + 8];
        let data_start = i + 8;
        let data_end = data_start.saturating_add(len);
        if data_end > bytes.len() {
            break;
        }
        let data = &bytes[data_start..data_end];
        if chunk_type == b"IHDR" && data.len() >= 8 {
            meta.width = Some(BigEndian::read_u32(&data[0..4]));
            meta.height = Some(BigEndian::read_u32(&data[4..8]));
        } else if chunk_type == b"eXIf" {
            parse_tiff_exif(data, meta);
        } else if chunk_type == b"IEND" {
            break;
        }
        i = data_end.saturating_add(4); // skip CRC
    }
}

fn parse_webp(bytes: &[u8], meta: &mut MediaMetadata) {
    let mut i = 12;
    while i + 8 <= bytes.len() {
        let chunk_id = &bytes[i..i + 4];
        let chunk_len = LittleEndian::read_u32(&bytes[i + 4..i + 8]) as usize;
        let data_start = i + 8;
        let data_end = data_start.saturating_add(chunk_len);
        if data_end > bytes.len() {
            break;
        }
        let data = &bytes[data_start..data_end];
        if chunk_id == b"VP8X" && data.len() >= 10 {
            let w = 1 + (u32::from(data[4]) | (u32::from(data[5]) << 8) | (u32::from(data[6]) << 16));
            let h = 1 + (u32::from(data[7]) | (u32::from(data[8]) << 8) | (u32::from(data[9]) << 16));
            meta.width = Some(w);
            meta.height = Some(h);
        } else if chunk_id == b"VP8L" && data.len() >= 5 && data[0] == 0x2f {
            let bits = LittleEndian::read_u32(&data[1..5]);
            let w = (bits & 0x3FFF) + 1;
            let h = ((bits >> 14) & 0x3FFF) + 1;
            meta.width.get_or_insert(w);
            meta.height.get_or_insert(h);
        } else if chunk_id == b"EXIF" {
            let tiff = data.strip_prefix(b"Exif\0\0").unwrap_or(data);
            parse_tiff_exif(tiff, meta);
        }
        i = data_end + (chunk_len & 1);
    }
}

// =============================================================================
// TIFF / EXIF / Apple MakerNote / Embedded RAW JPEG Parser
// =============================================================================

#[derive(Clone, Copy)]
enum Endian {
    Little,
    Big,
}

impl Endian {
    fn u16(self, b: &[u8]) -> u16 {
        match self {
            Self::Little => LittleEndian::read_u16(b),
            Self::Big => BigEndian::read_u16(b),
        }
    }
    fn u32(self, b: &[u8]) -> u32 {
        match self {
            Self::Little => LittleEndian::read_u32(b),
            Self::Big => BigEndian::read_u32(b),
        }
    }
}

fn parse_tiff_exif(tiff: &[u8], meta: &mut MediaMetadata) {
    if tiff.len() < 8 {
        return;
    }
    let endian = if &tiff[0..4] == b"II\x2a\x00" {
        Endian::Little
    } else if &tiff[0..4] == b"MM\x00\x2a" {
        Endian::Big
    } else {
        return;
    };
    let ifd0_offset = endian.u32(&tiff[4..8]) as usize;
    parse_ifd(tiff, endian, ifd0_offset, meta, 0);
}

fn parse_ifd(tiff: &[u8], endian: Endian, offset: usize, meta: &mut MediaMetadata, depth: u8) {
    if depth > 4 || offset + 2 > tiff.len() {
        return;
    }
    let count = endian.u16(&tiff[offset..offset + 2]) as usize;
    let entries_start = offset + 2;
    let mut exif_ifd_offset: Option<usize> = None;

    for idx in 0..count {
        let pos = entries_start + idx * 12;
        if pos + 12 > tiff.len() {
            break;
        }
        let tag = endian.u16(&tiff[pos..pos + 2]);
        let field_type = endian.u16(&tiff[pos + 2..pos + 4]);
        let num_values = endian.u32(&tiff[pos + 4..pos + 8]) as usize;
        let val_bytes = &tiff[pos + 8..pos + 12];

        match tag {
            // ImageWidth / ExifImageWidth
            0x0100 | 0xA002 => {
                if let Some(v) = read_int_tag(tiff, endian, field_type, num_values, val_bytes) {
                    if v > 0 && (tag == 0xA002 || meta.width.is_none()) {
                        meta.width = Some(v);
                    }
                }
            }
            // ImageLength / ExifImageHeight
            0x0101 | 0xA003 => {
                if let Some(v) = read_int_tag(tiff, endian, field_type, num_values, val_bytes) {
                    if v > 0 && (tag == 0xA003 || meta.height.is_none()) {
                        meta.height = Some(v);
                    }
                }
            }
            // Make
            0x010F => {
                if let Some(s) = read_ascii_tag(tiff, endian, field_type, num_values, val_bytes) {
                    if !s.is_empty() {
                        meta.camera_make = Some(s);
                    }
                }
            }
            // Model
            0x0110 => {
                if let Some(s) = read_ascii_tag(tiff, endian, field_type, num_values, val_bytes) {
                    if !s.is_empty() {
                        meta.camera_model = Some(s);
                    }
                }
            }
            // Orientation
            0x0112 => {
                if let Some(v) = read_int_tag(tiff, endian, field_type, num_values, val_bytes) {
                    if (1..=8).contains(&v) {
                        meta.orientation = v as u16;
                    }
                }
            }
            // DateTime (0x0132), DateTimeOriginal (0x9003), DateTimeDigitized (0x9004)
            0x0132 | 0x9003 | 0x9004 => {
                if let Some(s) = read_ascii_tag(tiff, endian, field_type, num_values, val_bytes) {
                    if let Some(dt) = parse_exif_datetime(&s) {
                        if tag == 0x9003 || meta.exif_local.is_none() {
                            meta.exif_local = Some(dt);
                        }
                    }
                }
            }
            // OffsetTime (0x9010), OffsetTimeOriginal (0x9011)
            0x9010 | 0x9011 => {
                if let Some(s) = read_ascii_tag(tiff, endian, field_type, num_values, val_bytes) {
                    if let Some(off) = parse_tz_offset_minutes(&s) {
                        if tag == 0x9011 || meta.exif_offset_minutes.is_none() {
                            meta.exif_offset_minutes = Some(off);
                        }
                    }
                }
            }
            // ExifIFDPointer
            0x8769 => {
                if let Some(ptr) = read_int_tag(tiff, endian, field_type, num_values, val_bytes) {
                    exif_ifd_offset = Some(ptr as usize);
                }
            }
            // MakerNote (Apple Live Photo ContentIdentifier lives at tag 0x0011 in Apple MakerNote)
            0x927C => {
                let offset_val = endian.u32(val_bytes) as usize;
                if offset_val + num_values <= tiff.len() {
                    parse_apple_makernote(&tiff[offset_val..offset_val + num_values], endian, meta);
                }
            }
            // XMP inside TIFF/DNG (tag 0x02BC)
            0x02BC => {
                let offset_val = endian.u32(val_bytes) as usize;
                if offset_val + num_values <= tiff.len() {
                    parse_xmp_packet(&tiff[offset_val..offset_val + num_values], meta);
                }
            }
            _ => {}
        }
    }

    if let Some(exif_off) = exif_ifd_offset {
        parse_ifd(tiff, endian, exif_off, meta, depth + 1);
    }
}

fn parse_apple_makernote(note: &[u8], default_endian: Endian, meta: &mut MediaMetadata) {
    // Apple MakerNote header: "Apple iOS\0" (10 bytes) + 2 bytes version + "MM" or "II" (2 bytes) + IFD count
    let Some(rest) = note.strip_prefix(b"Apple iOS\0") else {
        return;
    };
    if rest.len() < 6 {
        return;
    }
    let endian = if &rest[2..4] == b"MM" {
        Endian::Big
    } else if &rest[2..4] == b"II" {
        Endian::Little
    } else {
        default_endian
    };
    let count = endian.u16(&rest[4..6]) as usize;
    let entries_start = 16; // 10 ("Apple iOS\0") + 2 + 2 + 2
    for idx in 0..count {
        let pos = entries_start + idx * 12;
        if pos + 12 > note.len() {
            break;
        }
        let tag = endian.u16(&note[pos..pos + 2]);
        let field_type = endian.u16(&note[pos + 2..pos + 4]);
        let num_values = endian.u32(&note[pos + 4..pos + 8]) as usize;
        let val_bytes = &note[pos + 8..pos + 12];
        // Tag 0x0011 in Apple MakerNote is ContentIdentifier (ASCII UUID)
        if tag == 0x0011 {
            if let Some(cid) = read_ascii_tag(note, endian, field_type, num_values, val_bytes) {
                if !cid.is_empty() {
                    meta.content_identifier = Some(cid);
                }
            }
        }
    }
}

fn parse_xmp_packet(xmp: &[u8], meta: &mut MediaMetadata) {
    let Ok(s) = std::str::from_utf8(xmp) else {
        return;
    };
    for key in ["ContentIdentifier=\"", "ContentIdentifier>"] {
        if let Some(idx) = s.find(key) {
            let rest = &s[idx + key.len()..];
            let end = rest.find(['"', '<']).unwrap_or(0);
            if end > 0 {
                let val = rest[..end].trim();
                if !val.is_empty() && meta.content_identifier.is_none() {
                    meta.content_identifier = Some(val.to_string());
                }
            }
        }
    }
}

fn read_int_tag(
    tiff: &[u8],
    endian: Endian,
    field_type: u16,
    num_values: usize,
    val_bytes: &[u8],
) -> Option<u32> {
    if num_values == 0 {
        return None;
    }
    match field_type {
        // BYTE
        1 => Some(u32::from(val_bytes[0])),
        // SHORT (2 bytes)
        3 => Some(u32::from(endian.u16(&val_bytes[0..2]))),
        // LONG (4 bytes)
        4 => Some(endian.u32(val_bytes)),
        _ => {
            let _ = tiff;
            None
        }
    }
}

fn read_ascii_tag(
    tiff: &[u8],
    endian: Endian,
    field_type: u16,
    num_values: usize,
    val_bytes: &[u8],
) -> Option<String> {
    if field_type != 2 || num_values == 0 {
        return None;
    }
    let slice = if num_values <= 4 {
        &val_bytes[..num_values]
    } else {
        let off = endian.u32(val_bytes) as usize;
        if off + num_values > tiff.len() {
            return None;
        }
        &tiff[off..off + num_values]
    };
    let trimmed = slice
        .split(|&b| b == 0)
        .next()
        .unwrap_or(slice);
    Some(String::from_utf8_lossy(trimmed).trim().to_string())
}

/// Parse EXIF date `"YYYY:MM:DD HH:MM:SS"` (or ISO `"YYYY-MM-DD HH:MM:SS"`).
pub fn parse_exif_datetime(s: &str) -> Option<NaiveDateTime> {
    let s = s.trim();
    if s.len() < 19 {
        return None;
    }
    let y: i32 = s.get(0..4)?.parse().ok()?;
    let mo: u32 = s.get(5..7)?.parse().ok()?;
    let d: u32 = s.get(8..10)?.parse().ok()?;
    let h: u32 = s.get(11..13)?.parse().ok()?;
    let mi: u32 = s.get(14..16)?.parse().ok()?;
    let sec: u32 = s.get(17..19)?.parse().ok()?;
    let date = NaiveDate::from_ymd_opt(y, mo, d)?;
    let time = NaiveTime::from_hms_opt(h, mi, sec)?;
    Some(date.and_time(time))
}

/// Parse EXIF timezone offset `"+05:30"` or `"-04:00"` or `"Z"` into minutes east of UTC.
pub fn parse_tz_offset_minutes(s: &str) -> Option<i32> {
    let s = s.trim();
    if s == "Z" || s == "+00:00" || s == "-00:00" {
        return Some(0);
    }
    let sign = match s.as_bytes().first()? {
        b'+' => 1,
        b'-' => -1,
        _ => return None,
    };
    let rest = &s[1..];
    let (h_str, m_str) = if let Some(pair) = rest.split_once(':') {
        pair
    } else if rest.len() == 4 {
        (&rest[0..2], &rest[2..4])
    } else {
        return None;
    };
    let h: i32 = h_str.parse().ok()?;
    let m: i32 = m_str.parse().ok()?;
    if h > 14 || m >= 60 {
        return None;
    }
    Some(sign * (h * 60 + m))
}

/// Extract an embedded JPEG preview from a RAW file (DNG, CR2, CR3, NEF, ARW).
/// Spec section 7, step 5: "RAW via its embedded JPEG preview".
pub fn extract_embedded_jpeg(bytes: &[u8]) -> Option<&[u8]> {
    // 1. If TIFF-based RAW, check IFDs for JPEGInterchangeFormat (0x0201) + Length (0x0202)
    //    and SubIFDs (0x014A).
    if bytes.starts_with(b"II\x2a\x00") || bytes.starts_with(b"MM\x00\x2a") {
        let endian = if &bytes[0..4] == b"II\x2a\x00" {
            Endian::Little
        } else {
            Endian::Big
        };
        let ifd0 = endian.u32(&bytes[4..8]) as usize;
        let mut best: Option<&[u8]> = None;
        find_tiff_jpeg_preview(bytes, endian, ifd0, 0, &mut best);
        if best.is_some() {
            return best;
        }
    }

    // 2. Fallback: scan for the largest embedded SOI (\xFF\xD8\xFF) .. EOI (\xFF\xD9) stream.
    let mut best: Option<&[u8]> = None;
    let mut i = 0;
    while i + 4 < bytes.len() {
        if bytes[i] == 0xFF && bytes[i + 1] == 0xD8 && bytes[i + 2] == 0xFF {
            let start = i;
            let mut j = start + 3;
            while j + 1 < bytes.len() {
                if bytes[j] == 0xFF && bytes[j + 1] == 0xD9 {
                    let candidate = &bytes[start..j + 2];
                    if best.is_none_or(|b| candidate.len() > b.len()) {
                        best = Some(candidate);
                    }
                    i = j + 2;
                    break;
                }
                j += 1;
            }
            if j + 1 >= bytes.len() {
                break;
            }
        } else {
            i += 1;
        }
    }
    best
}

fn find_tiff_jpeg_preview<'a>(
    tiff: &'a [u8],
    endian: Endian,
    offset: usize,
    depth: u8,
    best: &mut Option<&'a [u8]>,
) {
    if depth > 4 || offset + 2 > tiff.len() {
        return;
    }
    let count = endian.u16(&tiff[offset..offset + 2]) as usize;
    let entries_start = offset + 2;
    let mut jpeg_off: Option<usize> = None;
    let mut jpeg_len: Option<usize> = None;
    let mut sub_ifds: Vec<usize> = Vec::new();

    for idx in 0..count {
        let pos = entries_start + idx * 12;
        if pos + 12 > tiff.len() {
            break;
        }
        let tag = endian.u16(&tiff[pos..pos + 2]);
        let field_type = endian.u16(&tiff[pos + 2..pos + 4]);
        let num_values = endian.u32(&tiff[pos + 4..pos + 8]) as usize;
        let val_bytes = &tiff[pos + 8..pos + 12];

        match tag {
            0x0201 => {
                jpeg_off = read_int_tag(tiff, endian, field_type, num_values, val_bytes)
                    .map(|v| v as usize);
            }
            0x0202 => {
                jpeg_len = read_int_tag(tiff, endian, field_type, num_values, val_bytes)
                    .map(|v| v as usize);
            }
            0x014A => {
                if num_values == 1 {
                    if let Some(sub) = read_int_tag(tiff, endian, field_type, num_values, val_bytes)
                    {
                        sub_ifds.push(sub as usize);
                    }
                } else {
                    let arr_off = endian.u32(val_bytes) as usize;
                    for k in 0..num_values.min(8) {
                        if arr_off + (k + 1) * 4 <= tiff.len() {
                            sub_ifds.push(endian.u32(&tiff[arr_off + k * 4..arr_off + (k + 1) * 4]) as usize);
                        }
                    }
                }
            }
            _ => {}
        }
    }

    if let (Some(off), Some(len)) = (jpeg_off, jpeg_len) {
        if len > 0 && off + len <= tiff.len() && tiff[off..off + len].starts_with(&[0xFF, 0xD8]) {
            let slice = &tiff[off..off + len];
            if best.is_none_or(|b| slice.len() > b.len()) {
                *best = Some(slice);
            }
        }
    }

    let next_ifd_pos = entries_start + count * 12;
    if next_ifd_pos + 4 <= tiff.len() {
        let next_ifd = endian.u32(&tiff[next_ifd_pos..next_ifd_pos + 4]) as usize;
        if next_ifd > 0 {
            find_tiff_jpeg_preview(tiff, endian, next_ifd, depth + 1, best);
        }
    }
    for sub in sub_ifds {
        find_tiff_jpeg_preview(tiff, endian, sub, depth + 1, best);
    }
}

// =============================================================================
// ISOBMFF (QuickTime MOV / MP4 / HEIC / CR3) Box Parser
// =============================================================================

/// QuickTime epoch (1904-01-01T00:00:00Z) to Unix epoch (1970-01-01T00:00:00Z) in seconds.
const QT_EPOCH_OFFSET_SECS: i64 = 2_082_844_800;

fn parse_isobmff(bytes: &[u8], meta: &mut MediaMetadata) {
    walk_iso_boxes(bytes, meta, 0);
}

fn walk_iso_boxes(data: &[u8], meta: &mut MediaMetadata, depth: u8) {
    if depth > 8 {
        return;
    }
    let mut pos = 0;
    while pos + 8 <= data.len() {
        let size32 = BigEndian::read_u32(&data[pos..pos + 4]) as usize;
        let box_type = &data[pos + 4..pos + 8];
        let (box_len, header_len) = if size32 == 1 {
            if pos + 16 > data.len() {
                break;
            }
            (BigEndian::read_u64(&data[pos + 8..pos + 16]) as usize, 16)
        } else if size32 == 0 {
            (data.len() - pos, 8)
        } else {
            (size32, 8)
        };
        if box_len < header_len || pos + box_len > data.len() {
            break;
        }
        let body = &data[pos + header_len..pos + box_len];

        match box_type {
            b"ftyp" if body.len() >= 4 => {
                let brand = &body[0..4];
                let c = match brand {
                    b"qt  " => Some("mov"),
                    b"mp41" | b"mp42" | b"isom" | b"iso2" | b"avc1" => Some("mp4"),
                    b"M4V " | b"M4VH" | b"M4VP" => Some("m4v"),
                    b"heic" | b"heix" | b"mif1" | b"msf1" => Some("heic"),
                    b"crx " => Some("cr3"),
                    _ => None,
                };
                if let Some(container) = c {
                    meta.container = Some(container.to_string());
                }
            }
            b"moov" | b"trak" | b"mdia" | b"minf" | b"stbl" | b"edts" | b"udta" | b"iprp"
            | b"ipco" => {
                walk_iso_boxes(body, meta, depth + 1);
            }
            b"mvhd" => {
                parse_mvhd(body, meta);
            }
            b"tkhd" => {
                parse_tkhd(body, meta);
            }
            b"stsd" if body.len() >= 8 => {
                parse_stsd(&body[8..], meta);
            }
            b"ispe" if body.len() >= 12 => {
                // HEIC ImageSpatialExtentsProperty: version(1)+flags(3) + width(4) + height(4)
                let w = BigEndian::read_u32(&body[4..8]);
                let h = BigEndian::read_u32(&body[8..12]);
                if w > 0 && h > 0 && meta.width.is_none_or(|cur| w > cur) {
                    meta.width = Some(w);
                    meta.height = Some(h);
                }
            }
            b"irot" if !body.is_empty() => {
                // HEIC ImageRotation: angle * 90 deg counter-clockwise
                let angle = body[0] & 0x03;
                if angle == 1 || angle == 3 {
                    meta.orientation = 6;
                }
            }
            b"meta" => {
                // QuickTime `meta` starts with box list directly; ISO `meta` has 4-byte version/flags
                if body.len() >= 8 && matches!(&body[4..8], b"hdlr" | b"keys" | b"ilst") {
                    parse_qt_meta(body, meta);
                } else if body.len() > 4 {
                    parse_qt_meta(&body[4..], meta);
                    walk_iso_boxes(&body[4..], meta, depth + 1);
                }
            }
            _ => {}
        }

        // Also scan inside unknown/container boxes for embedded "Exif\0\0" TIFF headers (HEIC / CR3)
        if meta.exif_local.is_none() && (box_type == b"mdat" || box_type == b"uuid") {
            if let Some(idx) = find_subslice(body, b"Exif\0\0") {
                parse_tiff_exif(&body[idx + 6..], meta);
            } else if let Some(idx) = find_subslice(body, b"II\x2a\x00") {
                parse_tiff_exif(&body[idx..], meta);
            } else if let Some(idx) = find_subslice(body, b"MM\x00\x2a") {
                parse_tiff_exif(&body[idx..], meta);
            }
        }

        pos += box_len;
    }
}

fn find_subslice(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    haystack.windows(needle.len()).position(|w| w == needle)
}

fn parse_mvhd(body: &[u8], meta: &mut MediaMetadata) {
    if body.len() < 20 {
        return;
    }
    let version = body[0];
    let (creation_time, timescale, duration) = if version == 1 {
        if body.len() < 32 {
            return;
        }
        (
            BigEndian::read_u64(&body[4..12]),
            BigEndian::read_u32(&body[20..24]) as u64,
            BigEndian::read_u64(&body[24..32]),
        )
    } else {
        (
            u64::from(BigEndian::read_u32(&body[4..8])),
            u64::from(BigEndian::read_u32(&body[12..16])),
            u64::from(BigEndian::read_u32(&body[16..20])),
        )
    };

    if creation_time > QT_EPOCH_OFFSET_SECS as u64 {
        let unix_secs = (creation_time as i64) - QT_EPOCH_OFFSET_SECS;
        if let Some(dt) = Utc.timestamp_opt(unix_secs, 0).single() {
            meta.quicktime_utc.get_or_insert(dt);
        }
    }
    if timescale > 0 && duration > 0 {
        meta.duration_ms = Some((duration * 1000) / timescale);
    }
}

fn parse_tkhd(body: &[u8], meta: &mut MediaMetadata) {
    let version = *body.first().unwrap_or(&0);
    let min_len = if version == 1 { 96 } else { 84 };
    if body.len() < min_len {
        return;
    }
    let w_fixed = BigEndian::read_u32(&body[min_len - 8..min_len - 4]);
    let h_fixed = BigEndian::read_u32(&body[min_len - 4..min_len]);
    let w = w_fixed >> 16;
    let h = h_fixed >> 16;
    if w > 0 && h > 0 {
        meta.width = Some(w);
        meta.height = Some(h);
    }
}

fn parse_stsd(entries: &[u8], meta: &mut MediaMetadata) {
    let mut pos = 0;
    while pos + 8 <= entries.len() {
        let len = BigEndian::read_u32(&entries[pos..pos + 4]) as usize;
        if len < 8 || pos + len > entries.len() {
            break;
        }
        let fourcc = &entries[pos + 4..pos + 8];
        match fourcc {
            b"avc1" | b"avc3" => meta.video_codec = Some("h264".into()),
            b"hvc1" | b"hev1" => meta.video_codec = Some("hevc".into()),
            b"vp09" => meta.video_codec = Some("vp9".into()),
            b"av01" => meta.video_codec = Some("av1".into()),
            b"mp4v" => meta.video_codec = Some("mpeg4".into()),
            b"apch" | b"apcn" | b"apcs" | b"apco" => meta.video_codec = Some("prores".into()),
            b"mp4a" => meta.audio_codec = Some("aac".into()),
            b"ac-3" => meta.audio_codec = Some("ac3".into()),
            b"ec-3" => meta.audio_codec = Some("eac3".into()),
            b"Opus" => meta.audio_codec = Some("opus".into()),
            b"sowt" | b"twos" | b"lpcm" => meta.audio_codec = Some("pcm".into()),
            _ => {}
        }
        pos += len;
    }
}

/// Parse QuickTime `meta` (`keys` + `ilst`) for:
/// - `com.apple.quicktime.content.identifier` (Live Photo pairing)
/// - `com.apple.quicktime.creationdate` (with timezone offset)
/// - `com.apple.quicktime.make` and `com.apple.quicktime.model`
fn parse_qt_meta(data: &[u8], meta: &mut MediaMetadata) {
    let mut key_names: Vec<String> = Vec::new();
    let mut ilst_body: Option<&[u8]> = None;

    let mut pos = 0;
    while pos + 8 <= data.len() {
        let len = BigEndian::read_u32(&data[pos..pos + 4]) as usize;
        if len < 8 || pos + len > data.len() {
            break;
        }
        let btype = &data[pos + 4..pos + 8];
        let body = &data[pos + 8..pos + len];
        if btype == b"keys" && body.len() >= 8 {
            let count = BigEndian::read_u32(&body[4..8]) as usize;
            let mut kpos = 8;
            for _ in 0..count {
                if kpos + 8 > body.len() {
                    break;
                }
                let ksize = BigEndian::read_u32(&body[kpos..kpos + 4]) as usize;
                if ksize < 8 || kpos + ksize > body.len() {
                    break;
                }
                let kname = String::from_utf8_lossy(&body[kpos + 8..kpos + ksize]).to_string();
                key_names.push(kname);
                kpos += ksize;
            }
        } else if btype == b"ilst" {
            ilst_body = Some(body);
        }
        pos += len;
    }

    let Some(ilst) = ilst_body else {
        return;
    };
    let mut ipos = 0;
    while ipos + 8 <= ilst.len() {
        let item_len = BigEndian::read_u32(&ilst[ipos..ipos + 4]) as usize;
        if item_len < 8 || ipos + item_len > ilst.len() {
            break;
        }
        // 1-based index into `keys`
        let key_idx = BigEndian::read_u32(&ilst[ipos + 4..ipos + 8]) as usize;
        let item_body = &ilst[ipos + 8..ipos + item_len];
        if key_idx >= 1 && key_idx <= key_names.len() {
            let key = &key_names[key_idx - 1];
            if let Some(val) = extract_ilst_data_utf8(item_body) {
                apply_qt_tag(key, &val, meta);
            }
        }
        ipos += item_len;
    }
}

fn extract_ilst_data_utf8(item_body: &[u8]) -> Option<String> {
    if item_body.len() < 16 || &item_body[4..8] != b"data" {
        return None;
    }
    let len = BigEndian::read_u32(&item_body[0..4]) as usize;
    if len < 16 || len > item_body.len() {
        return None;
    }
    let val = String::from_utf8_lossy(&item_body[16..len])
        .trim()
        .to_string();
    if val.is_empty() {
        None
    } else {
        Some(val)
    }
}

fn apply_qt_tag(key: &str, val: &str, meta: &mut MediaMetadata) {
    match key.to_ascii_lowercase().as_str() {
        "com.apple.quicktime.content.identifier" => {
            meta.content_identifier = Some(val.to_string());
        }
        "com.apple.quicktime.make" => {
            meta.camera_make = Some(val.to_string());
        }
        "com.apple.quicktime.model" => {
            meta.camera_model = Some(val.to_string());
        }
        "com.apple.quicktime.creationdate" | "creation_time" => {
            if let Ok(dt) = DateTime::parse_from_rfc3339(val) {
                meta.quicktime_utc = Some(dt.with_timezone(&Utc));
            }
        }
        _ => {}
    }
}

// =============================================================================
// External ffprobe Enrichment (When Available)
// =============================================================================

#[derive(Deserialize)]
struct FfprobeOutput {
    #[serde(default)]
    streams: Vec<FfprobeStream>,
    format: Option<FfprobeFormat>,
}

#[derive(Deserialize)]
struct FfprobeStream {
    codec_type: Option<String>,
    codec_name: Option<String>,
    width: Option<u32>,
    height: Option<u32>,
}

#[derive(Deserialize)]
struct FfprobeFormat {
    format_name: Option<String>,
    duration: Option<String>,
    #[serde(default)]
    tags: std::collections::HashMap<String, String>,
}

fn probe_with_ffprobe(path: &Path, meta: &mut MediaMetadata) {
    let Ok(output) = spawn_hidden_command("ffprobe")
        .args([
            "-v",
            "quiet",
            "-print_format",
            "json",
            "-show_format",
            "-show_streams",
        ])
        .arg(path)
        .stdin(std::process::Stdio::null())
        .output()
    else {
        return;
    };
    if !output.status.success() {
        return;
    }
    let Ok(parsed) = serde_json::from_slice::<FfprobeOutput>(&output.stdout) else {
        return;
    };
    for stream in &parsed.streams {
        match stream.codec_type.as_deref() {
            Some("video") => {
                if let Some(ref c) = stream.codec_name {
                    meta.video_codec = Some(c.to_ascii_lowercase());
                }
                if let (Some(w), Some(h)) = (stream.width, stream.height) {
                    meta.width.get_or_insert(w);
                    meta.height.get_or_insert(h);
                }
            }
            Some("audio") => {
                if let Some(ref c) = stream.codec_name {
                    meta.audio_codec.get_or_insert_with(|| c.to_ascii_lowercase());
                }
            }
            _ => {}
        }
    }
    if let Some(fmt) = parsed.format {
        if let Some(dur_str) = fmt.duration {
            if let Ok(secs) = dur_str.parse::<f64>() {
                if secs > 0.0 {
                    meta.duration_ms = Some((secs * 1000.0).round() as u64);
                }
            }
        }
        if meta.container.is_none() {
            if let Some(name) = fmt.format_name {
                if let Some(first) = name.split(',').next() {
                    meta.container = Some(first.to_ascii_lowercase());
                }
            }
        }
        for (k, v) in fmt.tags {
            apply_qt_tag(&k, &v, meta);
        }
    }
}

// =============================================================================
// Image Orientation, Resizing, WebP Encoding & Thumbnail Generation
// =============================================================================

/// Simple in-memory RGB image buffer (`3` bytes per pixel: `[R, G, B]`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RgbImage {
    pub width: u32,
    pub height: u32,
    pub data: Vec<u8>,
}

impl RgbImage {
    pub fn new(width: u32, height: u32, fill: [u8; 3]) -> Self {
        let count = (width as usize) * (height as usize);
        let mut data = Vec::with_capacity(count * 3);
        for _ in 0..count {
            data.extend_from_slice(&fill);
        }
        Self {
            width,
            height,
            data,
        }
    }

    #[inline]
    pub fn get_pixel(&self, x: u32, y: u32) -> [u8; 3] {
        let idx = ((y * self.width + x) as usize) * 3;
        [self.data[idx], self.data[idx + 1], self.data[idx + 2]]
    }

    #[inline]
    pub fn set_pixel(&mut self, x: u32, y: u32, rgb: [u8; 3]) {
        let idx = ((y * self.width + x) as usize) * 3;
        self.data[idx] = rgb[0];
        self.data[idx + 1] = rgb[1];
        self.data[idx + 2] = rgb[2];
    }
}

/// Apply EXIF orientation (`1..=8`) to an `RgbImage`.
pub fn apply_exif_orientation(img: &RgbImage, orientation: u16) -> RgbImage {
    let (w, h) = (img.width, img.height);
    match orientation {
        2 => {
            // Flip horizontal
            let mut out = RgbImage::new(w, h, [0; 3]);
            for y in 0..h {
                for x in 0..w {
                    out.set_pixel(w - 1 - x, y, img.get_pixel(x, y));
                }
            }
            out
        }
        3 => {
            // Rotate 180
            let mut out = RgbImage::new(w, h, [0; 3]);
            for y in 0..h {
                for x in 0..w {
                    out.set_pixel(w - 1 - x, h - 1 - y, img.get_pixel(x, y));
                }
            }
            out
        }
        4 => {
            // Flip vertical
            let mut out = RgbImage::new(w, h, [0; 3]);
            for y in 0..h {
                for x in 0..w {
                    out.set_pixel(x, h - 1 - y, img.get_pixel(x, y));
                }
            }
            out
        }
        5 => {
            // Transpose
            let mut out = RgbImage::new(h, w, [0; 3]);
            for y in 0..h {
                for x in 0..w {
                    out.set_pixel(y, x, img.get_pixel(x, y));
                }
            }
            out
        }
        6 => {
            // Rotate 90 CW
            let mut out = RgbImage::new(h, w, [0; 3]);
            for y in 0..h {
                for x in 0..w {
                    out.set_pixel(h - 1 - y, x, img.get_pixel(x, y));
                }
            }
            out
        }
        7 => {
            // Transverse
            let mut out = RgbImage::new(h, w, [0; 3]);
            for y in 0..h {
                for x in 0..w {
                    out.set_pixel(h - 1 - y, w - 1 - x, img.get_pixel(x, y));
                }
            }
            out
        }
        8 => {
            // Rotate 270 CW
            let mut out = RgbImage::new(h, w, [0; 3]);
            for y in 0..h {
                for x in 0..w {
                    out.set_pixel(y, w - 1 - x, img.get_pixel(x, y));
                }
            }
            out
        }
        _ => img.clone(),
    }
}

/// Resize so the short edge is at most `target_short`, preserving aspect ratio (no upscaling).
pub fn resize_short_edge(img: &RgbImage, target_short: u32) -> RgbImage {
    let short = img.width.min(img.height);
    if short <= target_short || short == 0 {
        return img.clone();
    }
    let scale = f64::from(target_short) / f64::from(short);
    let new_w = ((f64::from(img.width) * scale).round() as u32).max(1);
    let new_h = ((f64::from(img.height) * scale).round() as u32).max(1);
    resize_bilinear(img, new_w, new_h)
}

/// Resize so the long edge is at most `target_long`, preserving aspect ratio (no upscaling).
pub fn resize_long_edge(img: &RgbImage, target_long: u32) -> RgbImage {
    let long = img.width.max(img.height);
    if long <= target_long || long == 0 {
        return img.clone();
    }
    let scale = f64::from(target_long) / f64::from(long);
    let new_w = ((f64::from(img.width) * scale).round() as u32).max(1);
    let new_h = ((f64::from(img.height) * scale).round() as u32).max(1);
    resize_bilinear(img, new_w, new_h)
}

fn resize_bilinear(img: &RgbImage, new_w: u32, new_h: u32) -> RgbImage {
    let mut out = RgbImage::new(new_w, new_h, [0; 3]);
    for y in 0..new_h {
        let src_y = (y * img.height) / new_h;
        for x in 0..new_w {
            let src_x = (x * img.width) / new_w;
            out.set_pixel(x, y, img.get_pixel(src_x, src_y));
        }
    }
    out
}

/// Encode a solid-color or general `RgbImage` into a valid `RIFF....WEBPVP8L`
/// lossless WebP byte vector in pure Rust.
pub fn encode_webp_lossless(img: &RgbImage) -> Vec<u8> {
    let w = img.width.clamp(1, 16384);
    let h = img.height.clamp(1, 16384);

    // Check if the image is a solid uniform color (fast single-symbol VP8L code)
    let first = if img.data.len() >= 3 {
        [img.data[0], img.data[1], img.data[2]]
    } else {
        [128, 128, 128]
    };
    let is_solid = img.data.as_chunks::<3>().0.iter().all(|p| *p == first);

    let mut bw = BitWriter::default();
    // VP8L signature byte (0x2f)
    bw.write_bits(0x2f, 8);
    // 14-bit width - 1, 14-bit height - 1, 1-bit alpha=0, 3-bit version=0
    bw.write_bits(w - 1, 14);
    bw.write_bits(h - 1, 14);
    bw.write_bits(0, 1);
    bw.write_bits(0, 3);

    // transform_present = 0, color_cache_present = 0, meta_prefix_codes = 0
    bw.write_bits(0, 1);
    bw.write_bits(0, 1);
    bw.write_bits(0, 1);

    if is_solid {
        // 5 simple 1-symbol prefix trees (0 zero-bits consumed per pixel!)
        write_simple_single_symbol_tree(&mut bw, u32::from(first[1])); // Green
        write_simple_single_symbol_tree(&mut bw, u32::from(first[0])); // Red
        write_simple_single_symbol_tree(&mut bw, u32::from(first[2])); // Blue
        write_simple_single_symbol_tree(&mut bw, 255); // Alpha
        write_simple_single_symbol_tree(&mut bw, 0); // Distance
    } else {
        // General image: use 2-symbol or flat 8-bit prefix trees for G, R, B, and 1-symbol for A, Dist
        write_flat_byte_tree_green(&mut bw);
        write_flat_byte_tree_256(&mut bw);
        write_flat_byte_tree_256(&mut bw);
        write_simple_single_symbol_tree(&mut bw, 255); // Alpha = 255
        write_simple_single_symbol_tree(&mut bw, 0); // Distance unused
        for px in img.data.as_chunks::<3>().0 {
            // Canonical 8-bit code for symbol `s` in a flat 256-entry tree is bit-reversed `s`
            bw.write_bits(u32::from(px[1].reverse_bits()), 8); // G
            bw.write_bits(u32::from(px[0].reverse_bits()), 8); // R
            bw.write_bits(u32::from(px[2].reverse_bits()), 8); // B
        }
    }

    let vp8l_payload = bw.finish();
    let pad = vp8l_payload.len() & 1;
    let riff_size = (4 + 8 + vp8l_payload.len() + pad) as u32;

    let mut out = Vec::with_capacity(8 + riff_size as usize);
    out.extend_from_slice(b"RIFF");
    out.extend_from_slice(&riff_size.to_le_bytes());
    out.extend_from_slice(b"WEBP");
    out.extend_from_slice(b"VP8L");
    out.extend_from_slice(&(vp8l_payload.len() as u32).to_le_bytes());
    out.extend_from_slice(&vp8l_payload);
    if pad == 1 {
        out.push(0);
    }
    out
}

#[derive(Default)]
struct BitWriter {
    bytes: Vec<u8>,
    acc: u64,
    bits: u32,
}

impl BitWriter {
    fn write_bits(&mut self, val: u32, count: u32) {
        self.acc |= u64::from(val) << self.bits;
        self.bits += count;
        while self.bits >= 8 {
            self.bytes.push((self.acc & 0xFF) as u8);
            self.acc >>= 8;
            self.bits -= 8;
        }
    }

    fn finish(mut self) -> Vec<u8> {
        if self.bits > 0 {
            self.bytes.push((self.acc & 0xFF) as u8);
        }
        self.bytes
    }
}

fn write_simple_single_symbol_tree(bw: &mut BitWriter, symbol: u32) {
    bw.write_bits(1, 1); // is_simple = 1
    bw.write_bits(0, 1); // num_symbols - 1 = 0 (1 symbol)
    bw.write_bits(1, 1); // is_first_8bits = 1
    bw.write_bits(symbol & 0xFF, 8);
}

fn write_flat_byte_tree_green(bw: &mut BitWriter) {
    // Normal code length tree for 280 symbols where first 256 have length 8
    bw.write_bits(0, 1); // is_simple = 0
    // num_code_lengths = 19 (4 + 15)
    bw.write_bits(15, 4);
    // Code length order: [17, 18, 0, 1, 2, 3, 4, 5, 16, 6, 7, 8, ...]
    // Index 2 is symbol 0 (code 0, len 1). Index 11 is symbol 8 (code 1, len 1).
    for i in 0..19 {
        let len = if i == 2 || i == 11 { 1 } else { 0 };
        bw.write_bits(len, 3);
    }
    // use_max_symbol = 1, length_nbits = 3 (for 8 bits -> max_symbol = 256)
    bw.write_bits(1, 1);
    bw.write_bits(3, 3); // nbits = 2 + 2*3 = 8
    bw.write_bits(254, 8); // max_symbol = 254 + 2 = 256
    for _ in 0..256 {
        bw.write_bits(1, 1); // symbol `8` has code `1`
    }
}

fn write_flat_byte_tree_256(bw: &mut BitWriter) {
    bw.write_bits(0, 1); // is_simple = 0
    bw.write_bits(15, 4);
    for i in 0..19 {
        let len = if i == 2 || i == 11 { 1 } else { 0 };
        bw.write_bits(len, 3);
    }
    bw.write_bits(0, 1); // use_max_symbol = 0 (all 256 symbols)
    for _ in 0..256 {
        bw.write_bits(1, 1); // symbol `8` has code `1`
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ThumbnailPair {
    /// `thumbs/<id>/s.webp` (short edge ~300 px)
    pub small_webp: Vec<u8>,
    /// `thumbs/<id>/m.webp` (long edge ~1600 px)
    pub preview_webp: Vec<u8>,
    /// True if decoding failed and placeholder thumbnails were generated.
    pub thumb_failed: bool,
}

/// Generate valid placeholder `s.webp` and `m.webp` thumbnails with `thumb_failed = true`
/// (spec section 7, step 5: "If decoding fails, still upload the original, use a
/// placeholder thumbnail and flag the asset").
pub fn placeholder_thumbnails() -> ThumbnailPair {
    let small = RgbImage::new(SMALL_THUMB_SHORT_EDGE, SMALL_THUMB_SHORT_EDGE, [64, 68, 76]);
    let preview = RgbImage::new(600, 600, [64, 68, 76]);
    ThumbnailPair {
        small_webp: encode_webp_lossless(&small),
        preview_webp: encode_webp_lossless(&preview),
        thumb_failed: true,
    }
}

/// Decode a PNG byte slice into an `RgbImage`.
pub fn decode_png_rgb(bytes: &[u8]) -> Option<RgbImage> {
    let decoder = png::Decoder::new(std::io::Cursor::new(bytes));
    let mut reader = decoder.read_info().ok()?;
    let mut buf = vec![0; reader.output_buffer_size()?];
    let info = reader.next_frame(&mut buf).ok()?;
    let w = info.width;
    let h = info.height;
    if w == 0 || h == 0 {
        return None;
    }
    let slice = &buf[..info.buffer_size()];
    let mut rgb = Vec::with_capacity((w as usize) * (h as usize) * 3);
    match info.color_type {
        png::ColorType::Rgb => {
            rgb.extend_from_slice(slice);
        }
        png::ColorType::Rgba => {
            for px in slice.as_chunks::<4>().0 {
                rgb.extend_from_slice(&px[0..3]);
            }
        }
        png::ColorType::Grayscale => {
            for &g in slice {
                rgb.extend_from_slice(&[g, g, g]);
            }
        }
        png::ColorType::GrayscaleAlpha => {
            for px in slice.as_chunks::<2>().0 {
                rgb.extend_from_slice(&[px[0], px[0], px[0]]);
            }
        }
        png::ColorType::Indexed => return None,
    }
    if rgb.len() != (w as usize) * (h as usize) * 3 {
        return None;
    }
    Some(RgbImage {
        width: w,
        height: h,
        data: rgb,
    })
}

/// Generate `s.webp` and `m.webp` thumbnails for `path` (spec section 7, step 5):
/// 1. Try pure-Rust PNG decode (or embedded RAW JPEG preview + `ffmpeg`).
/// 2. Try `ffmpeg` (extracting frame at ~1 s for videos, applying orientation).
/// 3. Fall back to `placeholder_thumbnails()` with `thumb_failed = true`.
pub fn generate_thumbnails(
    path: impl AsRef<Path>,
    kind: AssetKind,
    meta: &MediaMetadata,
) -> ThumbnailPair {
    let path = path.as_ref();
    let ext = path
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("")
        .to_ascii_lowercase();

    // 1. Try pure-Rust PNG path first only for PNG photos (never read multi-GB videos into RAM!)
    if kind == AssetKind::Photo && ext == "png" {
        if let Ok(bytes) = std::fs::read(path) {
            if bytes.starts_with(b"\x89PNG\r\n\x1a\n") {
                if let Some(raw_img) = decode_png_rgb(&bytes) {
                    let oriented = apply_exif_orientation(&raw_img, meta.orientation);
                    let small_img = resize_short_edge(&oriented, SMALL_THUMB_SHORT_EDGE);
                    let preview_img = resize_long_edge(&oriented, PREVIEW_THUMB_LONG_EDGE);
                    return ThumbnailPair {
                        small_webp: encode_webp_lossless(&small_img),
                        preview_webp: encode_webp_lossless(&preview_img),
                        thumb_failed: false,
                    };
                }
            }
        }
    }

    // 2. Try ffmpeg for photos/videos
    if let Some((small_webp, preview_webp)) = generate_thumbnails_via_ffmpeg(path, kind, meta) {
        return ThumbnailPair {
            small_webp,
            preview_webp,
            thumb_failed: false,
        };
    }

    // 3. Fallback placeholder with thumb_failed = true
    placeholder_thumbnails()
}

fn generate_thumbnails_via_ffmpeg(
    path: &Path,
    kind: AssetKind,
    meta: &MediaMetadata,
) -> Option<(Vec<u8>, Vec<u8>)> {
    let seek_arg = if kind == AssetKind::Video {
        let dur_ms = meta.duration_ms.unwrap_or(2000);
        if dur_ms >= 1000 {
            Some("00:00:01")
        } else {
            Some("00:00:00")
        }
    } else {
        None
    };

    let small_filter =
        "scale='if(gt(iw,ih),-1,min(300,iw))':'if(gt(iw,ih),min(300,ih),-1)'";
    let preview_filter =
        "scale='if(gt(iw,ih),min(1600,iw),-1)':'if(gt(iw,ih),-1,min(1600,ih))'";

    // Spec section 7 step 5: "RAW via its embedded JPEG preview".
    let ext = path
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("")
        .to_ascii_lowercase();
    if is_raw_extension(&ext) {
        if let Ok(raw_bytes) = std::fs::read(path) {
            if let Some(jpeg) = extract_embedded_jpeg(&raw_bytes) {
                if let (Some(small), Some(preview)) = (
                    run_ffmpeg_webp_stdin(jpeg, small_filter),
                    run_ffmpeg_webp_stdin(jpeg, preview_filter),
                ) {
                    return Some((small, preview));
                }
            }
        }
    }

    let small = run_ffmpeg_webp(path, seek_arg, small_filter)?;
    let preview = run_ffmpeg_webp(path, seek_arg, preview_filter)?;
    Some((small, preview))
}

fn run_ffmpeg_webp(path: &Path, seek: Option<&str>, vf: &str) -> Option<Vec<u8>> {
    let mut cmd = spawn_hidden_command("ffmpeg");
    cmd.args(["-nostdin", "-hide_banner", "-loglevel", "error"]);
    if let Some(ss) = seek {
        cmd.arg("-ss").arg(ss);
    }
    cmd.arg("-i")
        .arg(path)
        .arg("-frames:v")
        .arg("1")
        .arg("-vf")
        .arg(vf)
        .arg("-quality")
        .arg("80")
        .arg("-compression_level")
        .arg("4")
        .arg("-f")
        .arg("webp")
        .arg("pipe:1")
        .stdin(std::process::Stdio::null());

    let out = cmd.output().ok()?;
    if out.status.success() && out.stdout.len() >= 12 && &out.stdout[0..4] == b"RIFF" {
        Some(out.stdout)
    } else {
        None
    }
}

fn run_ffmpeg_webp_stdin(input_jpeg: &[u8], vf: &str) -> Option<Vec<u8>> {
    let mut child = spawn_hidden_command("ffmpeg")
        .args([
            "-hide_banner",
            "-loglevel",
            "error",
            "-i",
            "pipe:0",
            "-frames:v",
            "1",
            "-vf",
            vf,
            "-quality",
            "80",
            "-compression_level",
            "4",
            "-f",
            "webp",
            "pipe:1",
        ])
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::null())
        .spawn()
        .ok()?;

    if let Some(mut stdin) = child.stdin.take() {
        use std::io::Write;
        let _ = stdin.write_all(input_jpeg);
    }
    let out = child.wait_with_output().ok()?;
    if out.status.success() && out.stdout.len() >= 12 && &out.stdout[0..4] == b"RIFF" {
        Some(out.stdout)
    } else {
        None
    }
}

/// Prepare a cached original photo for display in the webview (spec section 8):
/// - For RAW files (`.dng`, `.cr2`, `.cr3`, `.nef`, `.arw`, `.raf`, `.orf`, `.rw2`),
///   extracts the embedded full-size JPEG preview (or decodes via `ffmpeg`).
/// - On Windows, converts `.heic`/`.heif`/`.tiff` via `ffmpeg` if available so
///   WebView2 can display them without external store codecs.
/// - For standard webview-native formats, returns the original bytes and MIME type.
pub fn prepare_photo_bytes_for_webview(path: impl AsRef<Path>) -> Option<(Vec<u8>, &'static str)> {
    let path = path.as_ref();
    let ext = path
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("")
        .to_ascii_lowercase();

    if is_raw_extension(&ext) {
        let raw_bytes = std::fs::read(path).ok()?;
        if let Some(jpeg) = extract_embedded_jpeg(&raw_bytes) {
            return Some((jpeg.to_vec(), "image/jpeg"));
        }
        if let Some(decoded) = run_ffmpeg_jpeg(path) {
            return Some((decoded, "image/jpeg"));
        }
    }

    #[cfg(target_os = "windows")]
    if matches!(ext.as_str(), "heic" | "heif" | "tif" | "tiff") {
        if let Some(decoded) = run_ffmpeg_jpeg(path) {
            return Some((decoded, "image/jpeg"));
        }
    }

    let bytes = std::fs::read(path).ok()?;
    let mime = match ext.as_str() {
        "png" => "image/png",
        "webp" => "image/webp",
        "gif" => "image/gif",
        "heic" | "heif" => "image/heic",
        "avif" => "image/avif",
        "tif" | "tiff" => "image/tiff",
        _ => "image/jpeg",
    };
    Some((bytes, mime))
}

fn run_ffmpeg_jpeg(path: &Path) -> Option<Vec<u8>> {
    let out = spawn_hidden_command("ffmpeg")
        .args(["-nostdin", "-hide_banner", "-loglevel", "error", "-i"])
        .arg(path)
        .args([
            "-frames:v",
            "1",
            "-q:v",
            "2",
            "-f",
            "image2pipe",
            "-vcodec",
            "mjpeg",
            "pipe:1",
        ])
        .stdin(std::process::Stdio::null())
        .output()
        .ok()?;
    if out.status.success() && out.stdout.starts_with(&[0xFF, 0xD8]) {
        Some(out.stdout)
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    #[test]
    fn classify_supported_ignored_and_unknown_files() {
        assert_eq!(
            classify_path(&PathBuf::from("/a/IMG_1234.HEIC")),
            FileClassification::Photo { ext: "heic".into() }
        );
        assert_eq!(
            classify_path(&PathBuf::from("/a/DSC001.ARW")),
            FileClassification::Photo { ext: "arw".into() }
        );
        assert_eq!(
            classify_path(&PathBuf::from("/a/clip.MTS")),
            FileClassification::Video { ext: "mts".into() }
        );
        assert_eq!(
            classify_path(&PathBuf::from("/a/.DS_Store")),
            FileClassification::Ignored
        );
        assert_eq!(
            classify_path(&PathBuf::from("/a/Thumbs.db")),
            FileClassification::Ignored
        );
        assert_eq!(
            classify_path(&PathBuf::from("/a/IMG_1234.AAE")),
            FileClassification::Ignored
        );
        assert_eq!(
            classify_path(&PathBuf::from("/a/notes.pdf")),
            FileClassification::Unknown { ext: "pdf".into() }
        );
    }

    #[test]
    fn sha256_streaming_matches_known_digest() {
        let digest = sha256_bytes(b"frames-photo-library");
        assert_eq!(digest.len(), 64);
        let reader_digest = sha256_reader(std::io::Cursor::new(b"frames-photo-library")).unwrap();
        assert_eq!(digest, reader_digest);
    }

    #[test]
    fn parses_exif_datetime_and_offset() {
        assert_eq!(
            parse_exif_datetime("2024:05:12 14:30:22"),
            Some(
                NaiveDate::from_ymd_opt(2024, 5, 12)
                    .unwrap()
                    .and_hms_opt(14, 30, 22)
                    .unwrap()
            )
        );
        assert_eq!(parse_tz_offset_minutes("+05:30"), Some(330));
        assert_eq!(parse_tz_offset_minutes("-04:00"), Some(-240));
        assert_eq!(parse_tz_offset_minutes("Z"), Some(0));
    }

    #[test]
    fn parses_synthetic_jpeg_with_exif_and_orientation_swap() {
        // Build a minimal valid JPEG with APP1 Exif (Orientation = 6, Make = "Apple", DateTimeOriginal)
        // and SOF0 (width = 4032, height = 3024 -> swapped to 3024x4032 because Orientation = 6)
        let mut tiff = Vec::new();
        tiff.extend_from_slice(b"MM\x00\x2a\x00\x00\x00\x08"); // Big-endian TIFF, IFD0 at 8
        tiff.extend_from_slice(&3u16.to_be_bytes()); // 3 entries in IFD0
        // Entry 1: Make (0x010F), ASCII (2), count 4, "EOS\0" inline
        tiff.extend_from_slice(&0x010Fu16.to_be_bytes());
        tiff.extend_from_slice(&2u16.to_be_bytes());
        tiff.extend_from_slice(&4u32.to_be_bytes());
        tiff.extend_from_slice(b"EOS\0");
        // Entry 2: Orientation (0x0112), SHORT (3), count 1, value 6 (90 CW)
        tiff.extend_from_slice(&0x0112u16.to_be_bytes());
        tiff.extend_from_slice(&3u16.to_be_bytes());
        tiff.extend_from_slice(&1u32.to_be_bytes());
        tiff.extend_from_slice(&6u16.to_be_bytes());
        tiff.extend_from_slice(&0u16.to_be_bytes());
        // Entry 3: DateTimeOriginal (0x9003), ASCII (2), count 20, offset 50
        tiff.extend_from_slice(&0x9003u16.to_be_bytes());
        tiff.extend_from_slice(&2u16.to_be_bytes());
        tiff.extend_from_slice(&20u32.to_be_bytes());
        tiff.extend_from_slice(&50u32.to_be_bytes());
        tiff.extend_from_slice(&0u32.to_be_bytes()); // next IFD = 0 (offset 46..50)
        tiff.extend_from_slice(b"2024:05:12 14:30:22\0");

        let mut jpeg = vec![0xFF, 0xD8, 0xFF, 0xE1];
        let app1_len = (2 + 6 + tiff.len()) as u16;
        jpeg.extend_from_slice(&app1_len.to_be_bytes());
        jpeg.extend_from_slice(b"Exif\0\0");
        jpeg.extend_from_slice(&tiff);
        // SOF0 marker (0xFFC0): length 11, precision 8, height 3024, width 4032
        jpeg.extend_from_slice(&[0xFF, 0xC0, 0x00, 0x0B, 0x08]);
        jpeg.extend_from_slice(&3024u16.to_be_bytes());
        jpeg.extend_from_slice(&4032u16.to_be_bytes());
        jpeg.extend_from_slice(&[0x01, 0x01, 0x11, 0x00, 0xFF, 0xD9]);

        let meta = parse_metadata_from_bytes(&jpeg, "jpg");
        assert_eq!(meta.camera_make.as_deref(), Some("EOS"));
        assert_eq!(meta.orientation, 6);
        // Swapped because orientation == 6
        assert_eq!(meta.width, Some(3024));
        assert_eq!(meta.height, Some(4032));
        assert_eq!(
            meta.exif_local,
            Some(
                NaiveDate::from_ymd_opt(2024, 5, 12)
                    .unwrap()
                    .and_hms_opt(14, 30, 22)
                    .unwrap()
            )
        );
    }

    #[test]
    fn parses_quicktime_mov_with_live_photo_content_identifier() {
        // Construct a synthetic QuickTime MOV with ftyp, mvhd, and meta (keys + ilst)
        let mut mov = Vec::new();
        // ftyp ("qt  ")
        mov.extend_from_slice(&12u32.to_be_bytes());
        mov.extend_from_slice(b"ftypqt  ");

        // meta box with keys ("com.apple.quicktime.content.identifier") and ilst ("UUID-LIVE-123")
        let key_str = b"com.apple.quicktime.content.identifier";
        let key_entry_len = (8 + key_str.len()) as u32;
        let keys_box_len = 8 + 8 + key_entry_len;
        let mut keys_box = Vec::new();
        keys_box.extend_from_slice(&keys_box_len.to_be_bytes());
        keys_box.extend_from_slice(b"keys");
        keys_box.extend_from_slice(&0u32.to_be_bytes()); // version/flags
        keys_box.extend_from_slice(&1u32.to_be_bytes()); // 1 key
        keys_box.extend_from_slice(&key_entry_len.to_be_bytes());
        keys_box.extend_from_slice(b"mdta");
        keys_box.extend_from_slice(key_str);

        let val_str = b"LIVE-UUID-9999";
        let data_box_len = (16 + val_str.len()) as u32;
        let item_box_len = 8 + data_box_len;
        let ilst_box_len = 8 + item_box_len;
        let mut ilst_box = Vec::new();
        ilst_box.extend_from_slice(&ilst_box_len.to_be_bytes());
        ilst_box.extend_from_slice(b"ilst");
        ilst_box.extend_from_slice(&item_box_len.to_be_bytes());
        ilst_box.extend_from_slice(&1u32.to_be_bytes()); // key index 1
        ilst_box.extend_from_slice(&data_box_len.to_be_bytes());
        ilst_box.extend_from_slice(b"data");
        ilst_box.extend_from_slice(&1u32.to_be_bytes()); // type UTF-8
        ilst_box.extend_from_slice(&0u32.to_be_bytes()); // locale
        ilst_box.extend_from_slice(val_str);

        let meta_box_len = (8 + keys_box.len() + ilst_box.len()) as u32;
        let moov_box_len = 8 + meta_box_len;
        mov.extend_from_slice(&moov_box_len.to_be_bytes());
        mov.extend_from_slice(b"moov");
        mov.extend_from_slice(&meta_box_len.to_be_bytes());
        mov.extend_from_slice(b"meta");
        mov.extend_from_slice(&keys_box);
        mov.extend_from_slice(&ilst_box);

        let meta = parse_metadata_from_bytes(&mov, "mov");
        assert_eq!(meta.container.as_deref(), Some("mov"));
        assert_eq!(meta.content_identifier.as_deref(), Some("LIVE-UUID-9999"));
    }

    #[test]
    fn orientation_and_resize_and_lossless_webp_encoding() {
        let mut img = RgbImage::new(800, 400, [10, 20, 30]);
        img.set_pixel(0, 0, [250, 100, 50]);
        let rotated = apply_exif_orientation(&img, 6); // 90 CW -> 400x800
        assert_eq!(rotated.width, 400);
        assert_eq!(rotated.height, 800);

        let small = resize_short_edge(&rotated, 300);
        assert_eq!(small.width, 300);
        assert_eq!(small.height, 600);

        let webp = encode_webp_lossless(&small);
        assert_eq!(&webp[0..4], b"RIFF");
        assert_eq!(&webp[8..16], b"WEBPVP8L");

        // Parse dimensions back from our generated WebP!
        let parsed = parse_metadata_from_bytes(&webp, "webp");
        assert_eq!(parsed.width, Some(300));
        assert_eq!(parsed.height, Some(600));
    }

    #[test]
    fn extracts_embedded_jpeg_from_raw_tiff_and_apple_makernote_cid() {
        // 1. Build a TIFF/DNG with JPEGInterchangeFormat (0x0201) + Length (0x0202) and Apple MakerNote (0x927C)
        let embedded_jpg = b"\xFF\xD8\xFF\xE0\x00\x10JFIF\x00\x01\x01\x00\x00\x01\x00\x01\x00\x00\xFF\xD9";
        let mut makernote = Vec::new();
        makernote.extend_from_slice(b"Apple iOS\0\x00\x01MM");
        makernote.extend_from_slice(&1u16.to_be_bytes()); // 1 entry
        // Tag 0x0011 (ContentIdentifier), ASCII (2), count 10, offset 28
        makernote.extend_from_slice(&0x0011u16.to_be_bytes());
        makernote.extend_from_slice(&2u16.to_be_bytes());
        makernote.extend_from_slice(&10u32.to_be_bytes());
        makernote.extend_from_slice(&28u32.to_be_bytes());
        makernote.extend_from_slice(b"CID-12345\0");

        let mut dng = Vec::new();
        dng.extend_from_slice(b"MM\x00\x2a\x00\x00\x00\x08"); // IFD0 at 8
        dng.extend_from_slice(&3u16.to_be_bytes()); // 3 entries
        let jpg_offset = (8 + 2 + 3 * 12 + 4 + makernote.len()) as u32;
        let mn_offset = (8 + 2 + 3 * 12 + 4) as u32;

        // Entry 1: 0x0201 JPEGInterchangeFormat
        dng.extend_from_slice(&0x0201u16.to_be_bytes());
        dng.extend_from_slice(&4u16.to_be_bytes());
        dng.extend_from_slice(&1u32.to_be_bytes());
        dng.extend_from_slice(&jpg_offset.to_be_bytes());
        // Entry 2: 0x0202 JPEGInterchangeFormatLength
        dng.extend_from_slice(&0x0202u16.to_be_bytes());
        dng.extend_from_slice(&4u16.to_be_bytes());
        dng.extend_from_slice(&1u32.to_be_bytes());
        dng.extend_from_slice(&(embedded_jpg.len() as u32).to_be_bytes());
        // Entry 3: 0x927C MakerNote
        dng.extend_from_slice(&0x927Cu16.to_be_bytes());
        dng.extend_from_slice(&7u16.to_be_bytes());
        dng.extend_from_slice(&(makernote.len() as u32).to_be_bytes());
        dng.extend_from_slice(&mn_offset.to_be_bytes());
        dng.extend_from_slice(&0u32.to_be_bytes()); // next IFD = 0
        dng.extend_from_slice(&makernote);
        dng.extend_from_slice(embedded_jpg);

        let extracted = extract_embedded_jpeg(&dng).unwrap();
        assert_eq!(extracted, embedded_jpg);

        let meta = parse_metadata_from_bytes(&dng, "dng");
        assert_eq!(meta.content_identifier.as_deref(), Some("CID-12345"));
    }

    #[test]
    fn generates_thumbnails_from_real_png_with_orientation() {
        let path = std::env::temp_dir().join(format!("frames-thumb-{}.png", fastrand::u64(..)));
        {
            let file = File::create(&path).unwrap();
            let mut encoder = png::Encoder::new(file, 600, 400);
            encoder.set_color(png::ColorType::Rgb);
            encoder.set_depth(png::BitDepth::Eight);
            let mut writer = encoder.write_header().unwrap();
            let rgb = vec![120u8; 600 * 400 * 3];
            writer.write_image_data(&rgb).unwrap();
        }
        let mut meta = read_metadata(&path).unwrap();
        assert_eq!(meta.width, Some(600));
        assert_eq!(meta.height, Some(400));
        meta.orientation = 6; // Rotate 90 CW -> 400x600 -> small becomes 300x450

        let pair = generate_thumbnails(&path, AssetKind::Photo, &meta);
        let _ = std::fs::remove_file(&path);

        assert!(!pair.thumb_failed);
        let small_meta = parse_metadata_from_bytes(&pair.small_webp, "webp");
        assert_eq!(small_meta.width, Some(300));
        assert_eq!(small_meta.height, Some(450));
    }
}
