//! The `frames://` URI scheme (`http://frames.localhost` on Windows) that
//! serves every thumbnail, preview, photo and video the web view shows
//! (spec section 8).
//!
//! - `thumb/<id>/<size>`: `s.webp`, or `m.webp` when `size` is 1024 or more
//! - `preview/<id>`: `m.webp`, falling back to `s.webp`
//! - `photo/<id>` (alias `original/<id>`): the original, from this computer
//!   first, then the LRU cache, then S4. Remote photos are cached whole;
//!   remote videos are streamed by passing the player's range requests to S4.

use std::fs;
use std::io::{Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};

use tauri::http::{header, HeaderMap, Method, Request, Response, StatusCode, Uri};
use tauri::{AppHandle, Manager};

use frames_core::media::{is_heic_extension, is_raw_extension, prepare_photo_bytes_for_webview};
use frames_core::ops::AssetKind;
use frames_core::s4::ObjectStore;
use frames_core::thumbnails::{is_asset_id, OriginalInfo};

use crate::AppState;

/// Largest body for a video request without an end, so seeking never reads a
/// whole multi-GB file into memory.
const VIDEO_CHUNK: u64 = 8 * 1024 * 1024;

pub fn handle(app: &AppHandle, request: Request<Vec<u8>>) -> Response<Vec<u8>> {
    if request.method() == Method::OPTIONS {
        return base(StatusCode::OK)
            .header(header::ACCESS_CONTROL_ALLOW_METHODS, "GET, HEAD, OPTIONS")
            .header(header::ACCESS_CONTROL_ALLOW_HEADERS, "*")
            .body(Vec::new())
            .unwrap();
    }
    let Some((kind, id, size)) = parse_path(request.uri()) else {
        return empty(StatusCode::BAD_REQUEST);
    };
    if !is_asset_id(&id) {
        return empty(StatusCode::NOT_FOUND);
    }
    let state = app.state::<AppState>();
    let head = request.method() == Method::HEAD;

    match kind.as_str() {
        "thumb" if size.unwrap_or(0) >= 1024 => serve_thumbnail(state.thumbs.preview(&id), head),
        "thumb" => serve_thumbnail(state.thumbs.small(&id), head),
        "preview" => serve_thumbnail(state.thumbs.preview(&id), head),
        "photo" | "original" => serve_original(&state, &id, request.headers(), request.method()),
        _ => empty(StatusCode::NOT_FOUND),
    }
}

/// Split `frames://localhost/thumb/<id>/256` into `("thumb", id, Some(256))`.
/// Also accepts `frames://thumb/<id>/256`, where the kind arrives as the host.
pub(crate) fn parse_path(uri: &Uri) -> Option<(String, String, Option<u32>)> {
    let host = uri.host().unwrap_or("");
    let path = uri.path().trim_start_matches('/');
    let is_local = host.is_empty() || host == "localhost" || host.ends_with(".localhost");
    let full = if is_local {
        path.to_string()
    } else {
        format!("{host}/{path}")
    };
    let mut segments = full.split('/').filter(|s| !s.is_empty());
    let kind = segments.next()?.to_string();
    let id = segments.next()?.to_string();
    let size = segments.next().and_then(|s| s.parse().ok());
    Some((kind, id, size))
}

fn serve_thumbnail(path: Option<PathBuf>, head: bool) -> Response<Vec<u8>> {
    match path.and_then(|p| fs::read(p).ok()) {
        // Ids are content hashes, so a thumbnail never changes.
        Some(bytes) => base(StatusCode::OK)
            .header(header::CONTENT_TYPE, "image/webp")
            .header(header::CONTENT_LENGTH, bytes.len())
            .header(header::CACHE_CONTROL, "public, max-age=31536000, immutable")
            .body(if head { Vec::new() } else { bytes })
            .unwrap(),
        None => empty(StatusCode::NOT_FOUND),
    }
}

fn serve_original(state: &AppState, id: &str, headers: &HeaderMap, method: &Method) -> Response<Vec<u8>> {
    if let Some(src) = state.thumbs.local_original(id) {
        return serve_local_original(state, id, &src, headers, method);
    }
    let Some(info) = state.thumbs.original_info(id) else {
        return empty(StatusCode::NOT_FOUND);
    };
    let cached = state
        .cache
        .lock()
        .ok()
        .and_then(|mut c| c.get_original_path(id, &info.filename));
    if let Some(path) = cached {
        return serve_local_original(state, id, &path, headers, method);
    }
    let Ok(s4) = state.require_s4() else {
        return serve_thumbnail(state.thumbs.preview(id), *method == Method::HEAD);
    };
    if info.kind == AssetKind::Video {
        return serve_remote_range(s4.as_ref(), &info, headers, method);
    }
    match download_original(state, s4.as_ref(), id, &info) {
        Some(path) => serve_local_original(state, id, &path, headers, method),
        None => serve_thumbnail(state.thumbs.preview(id), *method == Method::HEAD),
    }
}

/// Stream a remote photo into the LRU cache, holding the cache lock only to
/// pick the path and to register the finished file.
fn download_original(
    state: &AppState,
    s4: &dyn ObjectStore,
    id: &str,
    info: &OriginalInfo,
) -> Option<PathBuf> {
    let dest = state.cache.lock().ok()?.original_disk_path(id, &info.filename);
    if !s4.get_object_to_file(&info.key, &dest).ok()? {
        return None;
    }
    state
        .cache
        .lock()
        .ok()?
        .register_downloaded_original(id, &info.filename)
        .ok()
        .flatten()
}

/// RAW, TIFF and (on Windows) HEIC need converting before the web view can
/// show them; everything else is served as is, with range support.
fn serve_local_original(
    state: &AppState,
    id: &str,
    src: &Path,
    headers: &HeaderMap,
    method: &Method,
) -> Response<Vec<u8>> {
    let ext = extension(src);
    let needs_conversion =
        is_raw_extension(&ext) || is_heic_extension(&ext) || matches!(ext.as_str(), "tif" | "tiff");
    if !needs_conversion {
        return serve_file_with_range(src, headers, method);
    }
    match prepare_photo_bytes_for_webview(src) {
        Some((bytes, mime)) => base(StatusCode::OK)
            .header(header::CONTENT_TYPE, mime)
            .header(header::CONTENT_LENGTH, bytes.len())
            .header(header::CACHE_CONTROL, "private, max-age=3600")
            .body(if *method == Method::HEAD { Vec::new() } else { bytes })
            .unwrap(),
        None => serve_thumbnail(state.thumbs.preview(id), *method == Method::HEAD),
    }
}

/// Answer a video range request by fetching the same byte range from S4.
fn serve_remote_range(
    s4: &dyn ObjectStore,
    info: &OriginalInfo,
    headers: &HeaderMap,
    method: &Method,
) -> Response<Vec<u8>> {
    let len = info.size;
    if len == 0 {
        return empty(StatusCode::NOT_FOUND);
    }
    let (start, end) = match parse_range(headers, len, true) {
        Ok(Some(range)) => range,
        Ok(None) => (0, (VIDEO_CHUNK - 1).min(len - 1)),
        Err(()) => return not_satisfiable(len),
    };
    let body = if *method == Method::HEAD {
        Vec::new()
    } else {
        match s4.get_object_range(&info.key, start, Some(end)) {
            Ok(Some(bytes)) => bytes,
            Ok(None) => return empty(StatusCode::NOT_FOUND),
            Err(_) => return empty(StatusCode::BAD_GATEWAY),
        }
    };
    partial(content_type(&extension(Path::new(&info.filename))), start, end, len, body)
}

pub(crate) fn serve_file_with_range(path: &Path, headers: &HeaderMap, method: &Method) -> Response<Vec<u8>> {
    let Ok(meta) = fs::metadata(path) else {
        return empty(StatusCode::NOT_FOUND);
    };
    let len = meta.len();
    let mime = content_type(&extension(path));
    let is_image = mime.starts_with("image/");
    let head = *method == Method::HEAD;

    let range = match parse_range(headers, len, !is_image) {
        Ok(range) => range,
        Err(()) => return not_satisfiable(len),
    };
    // Large non-image files are always served in chunks.
    let range = range.or_else(|| (!is_image && len > VIDEO_CHUNK).then(|| (0, VIDEO_CHUNK - 1)));

    match range {
        Some((start, end)) => {
            let body = if head {
                Vec::new()
            } else {
                match read_range(path, start, end) {
                    Some(bytes) => bytes,
                    None => return empty(StatusCode::INTERNAL_SERVER_ERROR),
                }
            };
            partial(mime, start, end, len, body)
        }
        None => {
            let body = if head {
                Vec::new()
            } else {
                match fs::read(path) {
                    Ok(bytes) => bytes,
                    Err(_) => return empty(StatusCode::INTERNAL_SERVER_ERROR),
                }
            };
            base(StatusCode::OK)
                .header(header::CONTENT_TYPE, mime)
                .header(header::CONTENT_LENGTH, len)
                .header(header::ACCEPT_RANGES, "bytes")
                .header(header::CACHE_CONTROL, "private, max-age=3600")
                .body(body)
                .unwrap()
        }
    }
}

/// Parse `Range: bytes=…` against a body of `len` bytes. `Ok(None)` means no
/// usable range was asked for; `Err(())` means 416. With `cap_open_end`, an
/// open-ended range is limited to [`VIDEO_CHUNK`] bytes.
fn parse_range(headers: &HeaderMap, len: u64, cap_open_end: bool) -> Result<Option<(u64, u64)>, ()> {
    let Some(spec) = headers
        .get(header::RANGE)
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.trim().strip_prefix("bytes="))
    else {
        return Ok(None);
    };
    if len == 0 {
        return Err(());
    }
    let (start, end) = spec.trim().split_once('-').ok_or(())?;
    let (start, end) = (start.trim(), end.trim());
    let (start, end) = if start.is_empty() {
        // Suffix range: the last N bytes.
        let n: u64 = end.parse().map_err(|_| ())?;
        (len - n.min(len), len - 1)
    } else {
        let start: u64 = start.parse().map_err(|_| ())?;
        let end = if end.is_empty() {
            if cap_open_end {
                start.saturating_add(VIDEO_CHUNK - 1)
            } else {
                len - 1
            }
        } else {
            end.parse().map_err(|_| ())?
        };
        (start, end.min(len - 1))
    };
    if start > end || start >= len {
        return Err(());
    }
    Ok(Some((start, end)))
}

fn read_range(path: &Path, start: u64, end: u64) -> Option<Vec<u8>> {
    let mut file = fs::File::open(path).ok()?;
    file.seek(SeekFrom::Start(start)).ok()?;
    let mut buf = vec![0u8; (end - start + 1) as usize];
    file.read_exact(&mut buf).ok()?;
    Some(buf)
}

fn extension(path: &Path) -> String {
    path.extension()
        .and_then(|e| e.to_str())
        .unwrap_or("")
        .to_ascii_lowercase()
}

fn content_type(ext: &str) -> &'static str {
    match ext {
        "jpg" | "jpeg" => "image/jpeg",
        "png" => "image/png",
        "webp" => "image/webp",
        "gif" => "image/gif",
        "heic" | "heif" => "image/heic",
        "tif" | "tiff" => "image/tiff",
        "mp4" => "video/mp4",
        "mov" => "video/quicktime",
        "m4v" => "video/x-m4v",
        "avi" => "video/x-msvideo",
        "mkv" => "video/x-matroska",
        "webm" => "video/webm",
        _ => "application/octet-stream",
    }
}

fn base(status: StatusCode) -> tauri::http::response::Builder {
    Response::builder()
        .status(status)
        .header(header::ACCESS_CONTROL_ALLOW_ORIGIN, "*")
}

/// An empty error response. `no-store`, so the web view asks again once the
/// thumbnail or original becomes available.
fn empty(status: StatusCode) -> Response<Vec<u8>> {
    base(status)
        .header(header::CACHE_CONTROL, "no-store")
        .body(Vec::new())
        .unwrap()
}

fn not_satisfiable(len: u64) -> Response<Vec<u8>> {
    base(StatusCode::RANGE_NOT_SATISFIABLE)
        .header(header::CONTENT_RANGE, format!("bytes */{len}"))
        .body(Vec::new())
        .unwrap()
}

fn partial(mime: &str, start: u64, end: u64, len: u64, body: Vec<u8>) -> Response<Vec<u8>> {
    base(StatusCode::PARTIAL_CONTENT)
        .header(header::CONTENT_TYPE, mime)
        .header(header::CONTENT_RANGE, format!("bytes {start}-{end}/{len}"))
        .header(header::CONTENT_LENGTH, end - start + 1)
        .header(header::ACCEPT_RANGES, "bytes")
        .body(body)
        .unwrap()
}

#[cfg(test)]
mod tests {
    use super::*;

    const ID: &str = "317a28b060bbddd335621ac1934bf3446b32a772d159429480bce3c453a2154d";

    #[test]
    fn parses_both_url_shapes() {
        let mac: Uri = format!("frames://localhost/thumb/{ID}/256").parse().unwrap();
        assert_eq!(parse_path(&mac), Some(("thumb".into(), ID.into(), Some(256))));

        let windows: Uri = format!("http://frames.localhost/photo/{ID}").parse().unwrap();
        assert_eq!(parse_path(&windows), Some(("photo".into(), ID.into(), None)));

        let host_as_kind: Uri = format!("frames://thumb/{ID}/1024").parse().unwrap();
        assert_eq!(parse_path(&host_as_kind), Some(("thumb".into(), ID.into(), Some(1024))));
    }

    #[test]
    fn serves_files_with_byte_ranges() {
        let tmp = std::env::temp_dir().join(format!("range-test-{}", fastrand::u64(..)));
        fs::write(&tmp, b"0123456789abcdefghij").unwrap(); // 20 bytes
        let mut headers = HeaderMap::new();

        let resp = serve_file_with_range(&tmp, &headers, &Method::GET);
        assert_eq!(resp.status(), StatusCode::OK);
        assert_eq!(resp.body().len(), 20);

        headers.insert(header::RANGE, "bytes=0-4".parse().unwrap());
        let resp = serve_file_with_range(&tmp, &headers, &Method::GET);
        assert_eq!(resp.status(), StatusCode::PARTIAL_CONTENT);
        assert_eq!(resp.body().as_slice(), b"01234");
        assert_eq!(resp.headers().get(header::CONTENT_RANGE).unwrap(), "bytes 0-4/20");

        headers.insert(header::RANGE, "bytes=-5".parse().unwrap());
        let resp = serve_file_with_range(&tmp, &headers, &Method::GET);
        assert_eq!(resp.body().as_slice(), b"fghij");
        assert_eq!(resp.headers().get(header::CONTENT_RANGE).unwrap(), "bytes 15-19/20");

        headers.insert(header::RANGE, "bytes=50-100".parse().unwrap());
        let resp = serve_file_with_range(&tmp, &headers, &Method::GET);
        assert_eq!(resp.status(), StatusCode::RANGE_NOT_SATISFIABLE);
        assert_eq!(resp.headers().get(header::CONTENT_RANGE).unwrap(), "bytes */20");

        let img = std::env::temp_dir().join(format!("range-img-test-{}.jpg", fastrand::u64(..)));
        fs::write(&img, b"0123456789abcdefghij").unwrap();
        headers.insert(header::RANGE, "bytes=0-".parse().unwrap());
        let resp = serve_file_with_range(&img, &headers, &Method::GET);
        assert_eq!(resp.status(), StatusCode::PARTIAL_CONTENT);
        assert_eq!(resp.body().len(), 20);
        assert_eq!(resp.headers().get(header::CONTENT_RANGE).unwrap(), "bytes 0-19/20");

        let _ = fs::remove_file(&img);
        let _ = fs::remove_file(&tmp);
    }

    #[test]
    fn caps_open_ended_video_ranges() {
        let mut headers = HeaderMap::new();
        headers.insert(header::RANGE, "bytes=100-".parse().unwrap());
        let len = 50 * 1024 * 1024;
        assert_eq!(parse_range(&headers, len, true), Ok(Some((100, 100 + VIDEO_CHUNK - 1))));
        assert_eq!(parse_range(&headers, len, false), Ok(Some((100, len - 1))));
    }
}
