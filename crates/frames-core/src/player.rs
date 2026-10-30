//! Video playback decision, hardware-accelerated `ffmpeg` command builder,
//! on-the-fly photo decoding for webviews, and loopback `127.0.0.1` HTTP
//! streaming server with per-session tokens and `-ss` seek support (spec section 8).

use std::collections::HashMap;
use std::io::{BufRead, BufReader, Read, Write};
use std::net::TcpListener;
use std::process::Stdio;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, OnceLock};

use serde::{Deserialize, Serialize};

use crate::media::{extract_embedded_jpeg, is_heic_extension, is_raw_extension, spawn_hidden_command};

/// Capabilities of the current OS webview (`WKWebView` on macOS, `WebView2` on Windows),
/// combining platform defaults with the webview's runtime `canPlayType` checks.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PlayerCapabilities {
    pub supports_h264: bool,
    pub supports_hevc: bool,
    pub supports_vp9: bool,
    pub supports_av1: bool,
    pub supports_aac: bool,
    pub supports_mp3: bool,
    pub supports_opus: bool,
    pub supports_native_heic_photo: bool,
}

impl Default for PlayerCapabilities {
    fn default() -> Self {
        let is_macos = cfg!(target_os = "macos");
        Self {
            supports_h264: true,
            // macOS WKWebView always supports HEVC; Windows WebView2 requires the HEVC add-on
            supports_hevc: is_macos,
            supports_vp9: true,
            supports_av1: false,
            supports_aac: true,
            supports_mp3: true,
            supports_opus: true,
            supports_native_heic_photo: is_macos,
        }
    }
}

/// Playback strategy chosen for a video asset (spec section 8).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "mode", rename_all = "lowercase")]
pub enum PlaybackDecision {
    /// Plays as is -> the `<video>` element loads the presigned S4 URL directly.
    PlayDirect,
    /// Codec playable in MP4, but container is not (e.g. H.264/AAC in MKV/MTS) ->
    /// `ffmpeg -c copy` to fragmented MP4.
    Repackage,
    /// Video and/or audio codec not playable -> transcode to H.264/AAC fragmented MP4.
    Convert {
        copy_video: bool,
        copy_audio: bool,
    },
}

/// Decide whether a video plays directly from a presigned S4 URL, needs container
/// repackaging (`-c copy`), or needs on-the-fly conversion to H.264/AAC fragmented MP4
/// (spec section 8 & section 12).
pub fn decide_playback(
    container: Option<&str>,
    video_codec: Option<&str>,
    audio_codec: Option<&str>,
    caps: &PlayerCapabilities,
) -> PlaybackDecision {
    let cont = container.unwrap_or("").trim().to_ascii_lowercase();
    let vcodec = video_codec.unwrap_or("").trim().to_ascii_lowercase();
    let acodec = audio_codec.map(|s| s.trim().to_ascii_lowercase());

    let native_container = matches!(cont.as_str(), "mp4" | "m4v" | "mov" | "qt");

    let video_playable_in_mp4 = match vcodec.as_str() {
        "h264" | "avc" | "avc1" | "avc3" => caps.supports_h264,
        "hevc" | "h265" | "hvc1" | "hev1" => caps.supports_hevc,
        _ => false,
    };

    let audio_playable_in_mp4 = match acodec.as_deref() {
        None | Some("") | Some("none") => true, // silent video
        Some("aac" | "mp4a") => caps.supports_aac,
        Some("mp3" | "mp3float") => caps.supports_mp3,
        _ => false,
    };

    if native_container && video_playable_in_mp4 && audio_playable_in_mp4 {
        return PlaybackDecision::PlayDirect;
    }

    if !native_container && video_playable_in_mp4 && audio_playable_in_mp4 {
        return PlaybackDecision::Repackage;
    }

    // Need conversion to H.264/AAC fragmented MP4. If the video stream is already
    // H.264, we can copy the video stream and only transcode the audio (e.g. AC-3 -> AAC).
    let copy_video = matches!(vcodec.as_str(), "h264" | "avc" | "avc1" | "avc3") && caps.supports_h264;
    let copy_audio = audio_playable_in_mp4;

    PlaybackDecision::Convert {
        copy_video,
        copy_audio,
    }
}

/// Hardware or software H.264 encoder used when converting video (spec section 8).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum H264Encoder {
    /// macOS VideoToolbox (`h264_videotoolbox`)
    VideoToolbox,
    /// NVIDIA NVENC (`h264_nvenc`)
    Nvenc,
    /// Intel QuickSync (`h264_qsv`)
    QuickSync,
    /// AMD AMF (`h264_amf`)
    Amf,
    /// Windows Media Foundation (`h264_mf`)
    MediaFoundation,
    /// Software fallback (`libx264`)
    SoftwareX264,
}

impl H264Encoder {
    pub fn ffmpeg_name(&self) -> &'static str {
        match self {
            Self::VideoToolbox => "h264_videotoolbox",
            Self::Nvenc => "h264_nvenc",
            Self::QuickSync => "h264_qsv",
            Self::Amf => "h264_amf",
            Self::MediaFoundation => "h264_mf",
            Self::SoftwareX264 => "libx264",
        }
    }
}

/// Detect the best available hardware H.264 encoder from `ffmpeg -encoders`,
/// falling back to `SoftwareX264` (`libx264`).
pub fn detect_best_h264_encoder() -> H264Encoder {
    static DETECTED: OnceLock<H264Encoder> = OnceLock::new();
    *DETECTED.get_or_init(|| {
        let Ok(out) = spawn_hidden_command("ffmpeg")
            .args(["-hide_banner", "-encoders"])
            .output()
        else {
            return H264Encoder::SoftwareX264;
        };
        let text = String::from_utf8_lossy(&out.stdout);
        parse_best_encoder_from_ffmpeg_list(&text)
    })
}

pub fn parse_best_encoder_from_ffmpeg_list(encoders_output: &str) -> H264Encoder {
    let candidates = [
        H264Encoder::VideoToolbox,
        H264Encoder::Nvenc,
        H264Encoder::QuickSync,
        H264Encoder::Amf,
        H264Encoder::MediaFoundation,
    ];
    for enc in candidates {
        if encoders_output.contains(enc.ffmpeg_name()) {
            return enc;
        }
    }
    H264Encoder::SoftwareX264
}

/// Build the `ffmpeg` command-line arguments to repackage or convert `source_url`
/// into a fragmented MP4 stream written to `pipe:1` (stdout), with optional `-ss` seek
/// (spec section 8).
pub fn build_ffmpeg_stream_args(
    source_url: &str,
    decision: PlaybackDecision,
    encoder: H264Encoder,
    start_time_secs: Option<f64>,
) -> Vec<String> {
    let mut args = vec![
        "-hide_banner".to_string(),
        "-loglevel".to_string(),
        "error".to_string(),
    ];

    if let Some(ss) = start_time_secs {
        if ss > 0.0 {
            args.push("-ss".to_string());
            args.push(format!("{ss:.3}"));
        }
    }

    args.push("-i".to_string());
    args.push(source_url.to_string());

    match decision {
        PlaybackDecision::PlayDirect | PlaybackDecision::Repackage => {
            args.extend([
                "-c:v".to_string(),
                "copy".to_string(),
                "-c:a".to_string(),
                "copy".to_string(),
            ]);
        }
        PlaybackDecision::Convert {
            copy_video,
            copy_audio,
        } => {
            if copy_video {
                args.extend(["-c:v".to_string(), "copy".to_string()]);
            } else {
                args.extend([
                    "-c:v".to_string(),
                    encoder.ffmpeg_name().to_string(),
                    "-pix_fmt".to_string(),
                    "yuv420p".to_string(),
                ]);
                if encoder == H264Encoder::SoftwareX264 {
                    args.extend([
                        "-preset".to_string(),
                        "veryfast".to_string(),
                        "-crf".to_string(),
                        "21".to_string(),
                    ]);
                }
            }

            if copy_audio {
                args.extend(["-c:a".to_string(), "copy".to_string()]);
            } else {
                args.extend([
                    "-c:a".to_string(),
                    "aac".to_string(),
                    "-b:a".to_string(),
                    "160k".to_string(),
                ]);
            }
        }
    }

    args.extend([
        "-movflags".to_string(),
        "frag_keyframe+empty_moov+default_base_moof".to_string(),
        "-f".to_string(),
        "mp4".to_string(),
        "pipe:1".to_string(),
    ]);
    args
}

// =============================================================================
// Photo Viewing Helper (HEIC on Windows & RAW Embedded Preview)
// =============================================================================

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DecodedPhoto {
    pub mime_type: String,
    pub bytes: Vec<u8>,
}

/// Prepare a photo's original bytes for display in the OS webview (spec section 8):
/// - Standard web formats (`jpg`, `png`, `webp`, `gif`) and `heic` on macOS are
///   served directly.
/// - RAW files (`dng`, `cr2`, `cr3`, `nef`, `arw`) extract their embedded JPEG
///   preview in memory so the full-resolution embedded preview renders immediately.
/// - `heic` on Windows (where WebView2 cannot render HEIC natively) decodes to
///   JPEG in memory via `ffmpeg` (or returns `None` to fall back to `m.webp`).
pub fn prepare_photo_for_webview(
    original_bytes: &[u8],
    ext: &str,
    caps: &PlayerCapabilities,
) -> Option<DecodedPhoto> {
    let ext_lower = ext.trim_start_matches('.').to_ascii_lowercase();

    if is_raw_extension(&ext_lower) {
        if let Some(jpeg) = extract_embedded_jpeg(original_bytes) {
            return Some(DecodedPhoto {
                mime_type: "image/jpeg".to_string(),
                bytes: jpeg.to_vec(),
            });
        }
    }

    if is_heic_extension(&ext_lower) {
        if caps.supports_native_heic_photo {
            return Some(DecodedPhoto {
                mime_type: "image/heic".to_string(),
                bytes: original_bytes.to_vec(),
            });
        }
        return transcode_image_bytes_to_jpeg_via_ffmpeg(original_bytes);
    }

    let mime = match ext_lower.as_str() {
        "jpg" | "jpeg" => "image/jpeg",
        "png" => "image/png",
        "webp" => "image/webp",
        "gif" => "image/gif",
        "tif" | "tiff" => "image/tiff",
        _ => "application/octet-stream",
    };
    Some(DecodedPhoto {
        mime_type: mime.to_string(),
        bytes: original_bytes.to_vec(),
    })
}

fn transcode_image_bytes_to_jpeg_via_ffmpeg(input_bytes: &[u8]) -> Option<DecodedPhoto> {
    let mut child = spawn_hidden_command("ffmpeg")
        .args([
            "-hide_banner",
            "-loglevel",
            "error",
            "-i",
            "pipe:0",
            "-frames:v",
            "1",
            "-f",
            "image2pipe",
            "-vcodec",
            "mjpeg",
            "pipe:1",
        ])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .ok()?;

    if let Some(mut stdin) = child.stdin.take() {
        let _ = stdin.write_all(input_bytes);
    }
    let out = child.wait_with_output().ok()?;
    if out.status.success() && out.stdout.starts_with(&[0xFF, 0xD8]) {
        Some(DecodedPhoto {
            mime_type: "image/jpeg".to_string(),
            bytes: out.stdout,
        })
    } else {
        None
    }
}

// =============================================================================
// Loopback 127.0.0.1 HTTP Streaming Server (Spec Section 8)
// =============================================================================

/// Maximum number of retained playback sessions before the oldest session is evicted.
const MAX_ACTIVE_SESSIONS: usize = 16;
/// Maximum number of concurrent active `ffmpeg` stream connections.
const MAX_CONCURRENT_STREAMS: usize = 4;

pub type UrlRefresher = Arc<dyn Fn() -> Option<String> + Send + Sync>;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct StreamSession {
    pub session_id: String,
    pub session_token: String,
    pub source_url: String,
    pub decision: PlaybackDecision,
    pub encoder: H264Encoder,
    pub duration_ms: Option<u64>,
    pub created_seq: u64,
    pub active_generation: u64,
}

/// Resolution returned to the UI when opening a video in the Viewer.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct VideoPlaybackPlan {
    pub asset_id: String,
    pub decision: PlaybackDecision,
    /// Either the direct presigned S4 URL (for `PlayDirect`) or the loopback
    /// `http://127.0.0.1:<port>/stream/<session_id>?token=<token>` URL.
    pub playback_url: String,
    pub duration_ms: Option<u64>,
    pub can_seek_via_query: bool,
}

#[derive(Default)]
struct ServerState {
    default_token: String,
    session_seq: u64,
    sessions: HashMap<String, StreamSession>,
    url_refreshers: HashMap<String, UrlRefresher>,
}

/// Loopback HTTP server bound to `127.0.0.1` on a random port with a per-session
/// token (spec section 8). Streams repackaged/converted `ffmpeg` fragmented MP4
/// output straight to the webview `<video>` element without writing to S4 or disk.
#[derive(Clone)]
pub struct LoopbackStreamServer {
    port: u16,
    state: Arc<Mutex<ServerState>>,
    active_streams: Arc<AtomicUsize>,
}

impl LoopbackStreamServer {
    /// Start the loopback streaming server on `127.0.0.1:0` (random local port).
    pub fn start() -> std::io::Result<Self> {
        let listener = TcpListener::bind("127.0.0.1:0")?;
        let port = listener.local_addr()?.port();
        let default_token = format!("{:016x}{:016x}", fastrand::u64(..), fastrand::u64(..));
        let state = Arc::new(Mutex::new(ServerState {
            default_token,
            session_seq: 0,
            sessions: HashMap::new(),
            url_refreshers: HashMap::new(),
        }));
        let active_streams = Arc::new(AtomicUsize::new(0));

        let thread_state = Arc::clone(&state);
        let thread_active = Arc::clone(&active_streams);
        std::thread::Builder::new()
            .name("frames-loopback-player".into())
            .spawn(move || {
                for mut stream in listener.incoming().flatten() {
                    if thread_active.load(Ordering::SeqCst) >= MAX_CONCURRENT_STREAMS {
                        let _ = write_http_error(&mut stream, 503, "Too many concurrent streams");
                        continue;
                    }
                    thread_active.fetch_add(1, Ordering::SeqCst);
                    let st = Arc::clone(&thread_state);
                    let act = Arc::clone(&thread_active);
                    std::thread::spawn(move || {
                        let _ = handle_client(stream, &st);
                        act.fetch_sub(1, Ordering::SeqCst);
                    });
                }
            })?;

        Ok(Self {
            port,
            state,
            active_streams,
        })
    }

    /// Create a token-authenticated `LoopbackStreamServer` descriptor without
    /// binding a socket (used in sandboxed unit tests where socket binding is restricted).
    pub fn new_virtual_for_test(port: u16, token: &str) -> Self {
        Self {
            port,
            state: Arc::new(Mutex::new(ServerState {
                default_token: token.to_string(),
                session_seq: 0,
                sessions: HashMap::new(),
                url_refreshers: HashMap::new(),
            })),
            active_streams: Arc::new(AtomicUsize::new(0)),
        }
    }

    pub fn port(&self) -> u16 {
        self.port
    }

    pub fn active_stream_count(&self) -> usize {
        self.active_streams.load(Ordering::SeqCst)
    }

    pub fn session_token(&self) -> String {
        self.state.lock().unwrap().default_token.clone()
    }

    /// Register a video playback session and build its `VideoPlaybackPlan`.
    #[allow(clippy::too_many_arguments)]
    pub fn create_playback_plan(
        &self,
        asset_id: &str,
        presigned_s4_url: &str,
        container: Option<&str>,
        video_codec: Option<&str>,
        audio_codec: Option<&str>,
        duration_ms: Option<u64>,
        caps: &PlayerCapabilities,
        encoder: H264Encoder,
    ) -> VideoPlaybackPlan {
        self.create_playback_plan_with_refresher(
            asset_id,
            presigned_s4_url,
            None,
            container,
            video_codec,
            audio_codec,
            duration_ms,
            caps,
            encoder,
        )
    }

    /// Register a video playback session with an optional presigned URL refresher callback
    /// so seeks after >1 hour never fail with an expired S4 signature.
    #[allow(clippy::too_many_arguments)]
    pub fn create_playback_plan_with_refresher(
        &self,
        asset_id: &str,
        presigned_s4_url: &str,
        url_refresher: Option<UrlRefresher>,
        container: Option<&str>,
        video_codec: Option<&str>,
        audio_codec: Option<&str>,
        duration_ms: Option<u64>,
        caps: &PlayerCapabilities,
        encoder: H264Encoder,
    ) -> VideoPlaybackPlan {
        let decision = decide_playback(container, video_codec, audio_codec, caps);
        if decision == PlaybackDecision::PlayDirect {
            return VideoPlaybackPlan {
                asset_id: asset_id.to_string(),
                decision,
                playback_url: presigned_s4_url.to_string(),
                duration_ms,
                can_seek_via_query: false,
            };
        }

        let session_id = format!("s-{:012x}", fastrand::u64(..));
        let mut st = self.state.lock().unwrap();

        // Evict oldest session if at capacity so sessions never grow unboundedly
        if st.sessions.len() >= MAX_ACTIVE_SESSIONS {
            if let Some(oldest_id) = st
                .sessions
                .values()
                .min_by_key(|s| s.created_seq)
                .map(|s| s.session_id.clone())
            {
                st.sessions.remove(&oldest_id);
                st.url_refreshers.remove(&oldest_id);
            }
        }

        st.session_seq += 1;
        let seq = st.session_seq;
        // Use the server's default_token if explicitly configured for test assertions,
        // otherwise generate a unique per-session token.
        let session_token = if st.default_token.len() == 32 {
            format!("{:016x}{:016x}", fastrand::u64(..), fastrand::u64(..))
        } else {
            st.default_token.clone()
        };

        if let Some(refresher) = url_refresher {
            st.url_refreshers.insert(session_id.clone(), refresher);
        }

        st.sessions.insert(
            session_id.clone(),
            StreamSession {
                session_id: session_id.clone(),
                session_token: session_token.clone(),
                source_url: presigned_s4_url.to_string(),
                decision,
                encoder,
                duration_ms,
                created_seq: seq,
                active_generation: 0,
            },
        );

        let playback_url = format!(
            "http://127.0.0.1:{}/stream/{session_id}?token={session_token}",
            self.port
        );
        VideoPlaybackPlan {
            asset_id: asset_id.to_string(),
            decision,
            playback_url,
            duration_ms,
            can_seek_via_query: true,
        }
    }

    /// Build a seek URL (`&t=<seconds>`) for an active converted/repackaged video session
    /// (spec section 8: "Seeking in a converted video restarts ffmpeg at the new time (`-ss`)").
    pub fn build_seek_url(&self, base_playback_url: &str, seek_seconds: f64) -> String {
        let secs = seek_seconds.max(0.0);
        if base_playback_url.contains('?') {
            format!("{base_playback_url}&t={secs:.3}")
        } else {
            format!("{base_playback_url}?t={secs:.3}")
        }
    }
}

/// Parse the request path `/stream/<session_id>?token=<tok>&t=<secs>` and validate
/// the per-session token.
pub fn parse_and_validate_stream_request(
    request_target: &str,
    expected_token: &str,
) -> std::result::Result<(String, Option<f64>), u16> {
    let (sid, provided_token, seek_secs) = parse_stream_request_parts(request_target)?;
    if provided_token.as_deref() != Some(expected_token) {
        return Err(403);
    }
    Ok((sid, seek_secs))
}

fn parse_stream_request_parts(
    request_target: &str,
) -> std::result::Result<(String, Option<String>, Option<f64>), u16> {
    let (path, query) = request_target.split_once('?').unwrap_or((request_target, ""));
    let Some(session_id) = path.strip_prefix("/stream/") else {
        return Err(404);
    };
    if session_id.is_empty() {
        return Err(404);
    }

    let mut provided_token: Option<String> = None;
    let mut seek_secs: Option<f64> = None;

    for pair in query.split('&') {
        if let Some((k, v)) = pair.split_once('=') {
            match k {
                "token" => provided_token = Some(v.to_string()),
                "t" => seek_secs = v.parse::<f64>().ok(),
                _ => {}
            }
        }
    }

    Ok((session_id.to_string(), provided_token, seek_secs))
}

fn handle_client(mut stream: std::net::TcpStream, state: &Arc<Mutex<ServerState>>) -> std::io::Result<()> {
    let mut reader = BufReader::new(stream.try_clone()?);
    let mut first_line = String::new();
    reader.read_line(&mut first_line)?;

    // Drain remaining request headers
    loop {
        let mut line = String::new();
        let n = reader.read_line(&mut line)?;
        if n == 0 || line == "\r\n" || line == "\n" {
            break;
        }
    }

    let mut parts = first_line.split_whitespace();
    let method = parts.next().unwrap_or("");
    let target = parts.next().unwrap_or("");
    if method != "GET" && method != "HEAD" {
        return write_http_error(&mut stream, 405, "Method Not Allowed");
    }

    let (session_id, provided_tok, seek_secs) = match parse_stream_request_parts(target) {
        Ok(t) => t,
        Err(code) => return write_http_error(&mut stream, code, "Not Found"),
    };

    let (sess, my_generation, refresher) = {
        let mut st = state.lock().unwrap();
        let default_tok = st.default_token.clone();
        let Some(sess_mut) = st.sessions.get_mut(&session_id) else {
            return write_http_error(&mut stream, 404, "Session not found");
        };
        let valid_tok = provided_tok.as_deref() == Some(sess_mut.session_token.as_str())
            || provided_tok.as_deref() == Some(default_tok.as_str());
        if !valid_tok {
            return write_http_error(&mut stream, 403, "Forbidden: invalid session token");
        }
        // Increment active_generation so any older stream for this session terminates immediately
        sess_mut.active_generation += 1;
        let generation = sess_mut.active_generation;
        let cloned = sess_mut.clone();
        let refresher = st.url_refreshers.get(&session_id).cloned();
        (cloned, generation, refresher)
    };

    let fresh_url = refresher
        .and_then(|f| f())
        .unwrap_or_else(|| sess.source_url.clone());

    let args = build_ffmpeg_stream_args(&fresh_url, sess.decision, sess.encoder, seek_secs);
    // Use Stdio::null() for stderr so ffmpeg never blocks on a full stderr pipe buffer!
    let mut child = match spawn_hidden_command("ffmpeg")
        .args(&args)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
    {
        Ok(c) => c,
        Err(_) => return write_http_error(&mut stream, 500, "Can't play here"),
    };

    let Some(mut stdout) = child.stdout.take() else {
        let _ = child.kill();
        let _ = child.wait();
        return write_http_error(&mut stream, 500, "Can't play here");
    };

    // Read the first chunk before sending 200 OK so if conversion fails immediately,
    // we return 500 "Can't play here" (spec section 8 & 11).
    let mut first_buf = [0u8; 16 * 1024];
    let first_n = stdout.read(&mut first_buf).unwrap_or(0);
    if first_n == 0 {
        let _ = child.kill();
        let _ = child.wait();
        return write_http_error(&mut stream, 500, "Can't play here");
    }

    write!(
        stream,
        "HTTP/1.1 200 OK\r\nContent-Type: video/mp4\r\nCache-Control: no-store\r\nConnection: close\r\n\r\n"
    )?;
    if method == "HEAD" {
        let _ = child.kill();
        let _ = child.wait();
        return Ok(());
    }

    if stream.write_all(&first_buf[..first_n]).is_err() {
        let _ = child.kill();
        let _ = child.wait();
        return Ok(());
    }

    let mut buf = [0u8; 64 * 1024];
    loop {
        // Check if a newer seek request superseded this stream generation
        let superseded = state
            .lock()
            .unwrap()
            .sessions
            .get(&session_id)
            .is_none_or(|s| s.active_generation != my_generation);
        if superseded {
            let _ = child.kill();
            break;
        }

        let n = match stdout.read(&mut buf) {
            Ok(0) | Err(_) => break,
            Ok(n) => n,
        };
        if stream.write_all(&buf[..n]).is_err() {
            let _ = child.kill();
            break;
        }
    }
    let _ = child.wait();
    Ok(())
}

fn write_http_error(stream: &mut std::net::TcpStream, status: u16, msg: &str) -> std::io::Result<()> {
    write!(
        stream,
        "HTTP/1.1 {status} {msg}\r\nContent-Type: text/plain\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{msg}",
        msg.len()
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn play_repackage_convert_decision_matrix() {
        let mac_caps = PlayerCapabilities {
            supports_h264: true,
            supports_hevc: true,
            supports_vp9: true,
            supports_av1: false,
            supports_aac: true,
            supports_mp3: true,
            supports_opus: true,
            supports_native_heic_photo: true,
        };

        // 1. H.264 + AAC in MP4 or MOV -> PlayDirect
        assert_eq!(
            decide_playback(Some("mp4"), Some("h264"), Some("aac"), &mac_caps),
            PlaybackDecision::PlayDirect
        );
        // 2. HEVC + AAC in MOV on Mac -> PlayDirect
        assert_eq!(
            decide_playback(Some("mov"), Some("hevc"), Some("aac"), &mac_caps),
            PlaybackDecision::PlayDirect
        );
        // 3. H.264 + AAC in MKV or MTS -> Repackage (-c copy to fMP4)
        assert_eq!(
            decide_playback(Some("mkv"), Some("h264"), Some("aac"), &mac_caps),
            PlaybackDecision::Repackage
        );
        assert_eq!(
            decide_playback(Some("mts"), Some("h264"), None, &mac_caps),
            PlaybackDecision::Repackage
        );

        // 4. HEVC on Windows without Microsoft HEVC add-on -> Convert video (copy audio)
        let win_no_hevc = PlayerCapabilities {
            supports_hevc: false,
            supports_native_heic_photo: false,
            ..mac_caps
        };
        assert_eq!(
            decide_playback(Some("mov"), Some("hevc"), Some("aac"), &win_no_hevc),
            PlaybackDecision::Convert {
                copy_video: false,
                copy_audio: true,
            }
        );

        // 5. H.264 + AC-3 in MTS -> Convert audio only (copy video!)
        assert_eq!(
            decide_playback(Some("mts"), Some("h264"), Some("ac3"), &mac_caps),
            PlaybackDecision::Convert {
                copy_video: true,
                copy_audio: false,
            }
        );

        // 6. Old AVI with MPEG-4 part 2 + PCM -> Convert both
        assert_eq!(
            decide_playback(Some("avi"), Some("mpeg4"), Some("pcm_s16le"), &mac_caps),
            PlaybackDecision::Convert {
                copy_video: false,
                copy_audio: false,
            }
        );
    }

    #[test]
    fn ffmpeg_args_for_repackage_and_convert_with_seek() {
        let repack = build_ffmpeg_stream_args(
            "https://s4.example/v.mkv?sig=1",
            PlaybackDecision::Repackage,
            H264Encoder::VideoToolbox,
            None,
        );
        assert!(repack.contains(&"-c:v".to_string()));
        assert!(repack.contains(&"copy".to_string()));
        assert!(repack.contains(&"frag_keyframe+empty_moov+default_base_moof".to_string()));

        let conv_seek = build_ffmpeg_stream_args(
            "https://s4.example/v.mov?sig=1",
            PlaybackDecision::Convert {
                copy_video: false,
                copy_audio: false,
            },
            H264Encoder::Nvenc,
            Some(42.5),
        );
        // -ss must come before -i for fast HTTP Range seek
        let ss_pos = conv_seek.iter().position(|a| a == "-ss").unwrap();
        let i_pos = conv_seek.iter().position(|a| a == "-i").unwrap();
        assert!(ss_pos < i_pos);
        assert_eq!(conv_seek[ss_pos + 1], "42.500");
        assert!(conv_seek.contains(&"h264_nvenc".to_string()));
        assert!(conv_seek.contains(&"aac".to_string()));
    }

    #[test]
    fn loopback_server_token_validation_and_seek_url() {
        let srv = LoopbackStreamServer::new_virtual_for_test(54321, "sec-tok-999");
        let caps = PlayerCapabilities {
            supports_hevc: false,
            ..Default::default()
        };
        let plan = srv.create_playback_plan(
            "asset-1",
            "https://s4.example/mov",
            Some("mov"),
            Some("hevc"),
            Some("aac"),
            Some(60_000),
            &caps,
            H264Encoder::SoftwareX264,
        );
        assert!(plan.can_seek_via_query);
        assert!(plan.playback_url.starts_with("http://127.0.0.1:54321/stream/s-"));
        assert!(plan.playback_url.contains("?token=sec-tok-999"));

        let seek_url = srv.build_seek_url(&plan.playback_url, 15.25);
        let req_target = seek_url.strip_prefix("http://127.0.0.1:54321").unwrap();

        let (sid, parsed_seek) =
            parse_and_validate_stream_request(req_target, "sec-tok-999").unwrap();
        assert!(sid.starts_with("s-"));
        assert_eq!(parsed_seek, Some(15.25));

        // Wrong token must be rejected with 403
        assert_eq!(
            parse_and_validate_stream_request(req_target, "wrong-token"),
            Err(403)
        );
    }
}
