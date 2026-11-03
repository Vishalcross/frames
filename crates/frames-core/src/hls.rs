//! Video the Mac's player can't open directly (frames_swift_spec.md section
//! 10). AVPlayer refuses a progressive stream without byte ranges, so ffmpeg
//! repackages or converts the video into fragmented-MP4 segments and a
//! growing EVENT playlist in a temporary folder, and a small server on
//! 127.0.0.1 serves them to the player. Nothing goes to S4.

use std::collections::HashMap;
use std::io::{BufRead, BufReader, Write};
use std::net::{TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::process::{Child, Stdio};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use crate::media::spawn_hidden_command;
use crate::player::{codec_args, H264Encoder, PlaybackDecision};

/// How long a request for a file waits for ffmpeg to write it.
const FILE_WAIT: Duration = Duration::from_secs(20);

/// Serves HLS sessions from `root`, one folder per session.
pub struct HlsServer {
    port: u16,
    token: String,
    root: PathBuf,
    sessions: Mutex<HashMap<String, Child>>,
}

impl HlsServer {
    /// Listen on a free port on 127.0.0.1. Session folders go under `root`,
    /// which is emptied first: anything there is left from an earlier run.
    pub fn start(root: PathBuf) -> std::io::Result<Arc<Self>> {
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root)?;
        let listener = TcpListener::bind("127.0.0.1:0")?;
        let server = Arc::new(Self {
            port: listener.local_addr()?.port(),
            token: format!("{:016x}{:016x}", fastrand::u64(..), fastrand::u64(..)),
            root,
            sessions: Mutex::new(HashMap::new()),
        });
        let serving = Arc::downgrade(&server);
        std::thread::Builder::new().name("frames-hls".into()).spawn(move || {
            for stream in listener.incoming().flatten() {
                let Some(server) = serving.upgrade() else { return };
                std::thread::spawn(move || {
                    let _ = server.answer(stream);
                });
            }
        })?;
        Ok(server)
    }

    /// Start ffmpeg on `source` (a file path or URL) and return the
    /// playlist's URL. `hevc` says the video is HEVC, so a copied stream is
    /// tagged the way Apple's player needs. Other sessions are stopped: the
    /// viewer plays one video at a time.
    pub fn open(&self, source: &str, decision: PlaybackDecision, encoder: H264Encoder, hevc: bool) -> std::io::Result<String> {
        self.close_all();
        let id = format!("{:012x}", fastrand::u64(..) & 0xffff_ffff_ffff);
        let dir = self.root.join(&id);
        std::fs::create_dir_all(&dir)?;
        let child = spawn_hidden_command("ffmpeg")
            .args(hls_args(source, decision, encoder, hevc, &dir))
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()?;
        self.sessions.lock().unwrap_or_else(|e| e.into_inner()).insert(id.clone(), child);
        Ok(format!("http://127.0.0.1:{}/{}/{id}/index.m3u8", self.port, self.token))
    }

    /// Stop every session and delete its files.
    pub fn close_all(&self) {
        let sessions: Vec<(String, Child)> = self.sessions.lock().unwrap_or_else(|e| e.into_inner()).drain().collect();
        for (id, mut child) in sessions {
            let _ = child.kill();
            let _ = child.wait();
            let _ = std::fs::remove_dir_all(self.root.join(id));
        }
    }

    fn answer(&self, stream: TcpStream) -> std::io::Result<()> {
        let mut reader = BufReader::new(stream.try_clone()?);
        let mut request = String::new();
        reader.read_line(&mut request)?;
        // Read the headers to the blank line; nothing in them matters here.
        let mut line = String::new();
        while reader.read_line(&mut line)? > 2 {
            line.clear();
        }
        let path = request.split_whitespace().nth(1).unwrap_or("");
        let parts: Vec<&str> = path.trim_start_matches('/').split('/').collect();
        let mut out = stream;
        let file = match parts.as_slice() {
            [token, id, name] if *token == self.token && safe_name(id) && safe_name(name) => {
                let known = self.sessions.lock().unwrap_or_else(|e| e.into_inner()).contains_key(*id);
                known.then(|| self.root.join(id).join(name))
            }
            _ => None,
        };
        let Some(file) = file else {
            return out.write_all(b"HTTP/1.1 404 Not Found\r\nContent-Length: 0\r\nConnection: close\r\n\r\n");
        };
        match wait_for(&file) {
            Some(body) => {
                let kind = if file.extension().is_some_and(|e| e == "m3u8") { "application/vnd.apple.mpegurl" } else { "video/mp4" };
                write!(out, "HTTP/1.1 200 OK\r\nContent-Type: {kind}\r\nContent-Length: {}\r\nCache-Control: no-store\r\nConnection: close\r\n\r\n", body.len())?;
                out.write_all(&body)
            }
            None => out.write_all(b"HTTP/1.1 404 Not Found\r\nContent-Length: 0\r\nConnection: close\r\n\r\n"),
        }
    }
}

impl Drop for HlsServer {
    fn drop(&mut self) {
        self.close_all();
    }
}

/// ffmpeg arguments for an HLS session written into `dir`.
fn hls_args(source: &str, decision: PlaybackDecision, encoder: H264Encoder, hevc: bool, dir: &Path) -> Vec<String> {
    let mut args: Vec<String> = ["-hide_banner", "-loglevel", "error", "-i", source].map(String::from).to_vec();
    let codecs = codec_args(decision, encoder);
    let copies_video = codecs.windows(2).any(|w| w[0] == "-c:v" && w[1] == "copy");
    args.extend(codecs);
    if copies_video && hevc {
        args.extend(["-tag:v".to_string(), "hvc1".to_string()]);
    }
    if !copies_video && encoder == H264Encoder::VideoToolbox {
        args.extend(["-b:v".to_string(), "10M".to_string()]);
    }
    args.extend(
        [
            "-f", "hls",
            "-hls_time", "2",
            "-hls_playlist_type", "event",
            "-hls_segment_type", "fmp4",
            "-hls_flags", "independent_segments+temp_file",
            "-hls_fmp4_init_filename", "init.mp4",
        ]
        .map(String::from),
    );
    args.push("-hls_segment_filename".to_string());
    args.push(dir.join("seg%05d.m4s").to_string_lossy().to_string());
    args.push(dir.join("index.m3u8").to_string_lossy().to_string());
    args
}

/// The file's bytes once ffmpeg has written it, or None after `FILE_WAIT`.
fn wait_for(file: &Path) -> Option<Vec<u8>> {
    let started = Instant::now();
    loop {
        if let Ok(bytes) = std::fs::read(file) {
            return Some(bytes);
        }
        if started.elapsed() > FILE_WAIT {
            return None;
        }
        std::thread::sleep(Duration::from_millis(100));
    }
}

fn safe_name(name: &str) -> bool {
    !name.is_empty() && name.chars().all(|c| c.is_ascii_alphanumeric() || c == '.' || c == '_' || c == '-') && !name.starts_with('.')
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Read;

    fn get(url: &str) -> (String, Vec<u8>) {
        let rest = url.trim_start_matches("http://");
        let (host, path) = rest.split_once('/').unwrap();
        let mut stream = TcpStream::connect(host).unwrap();
        write!(stream, "GET /{path} HTTP/1.1\r\nHost: {host}\r\n\r\n").unwrap();
        let mut response = Vec::new();
        stream.read_to_end(&mut response).unwrap();
        let split = response.windows(4).position(|w| w == b"\r\n\r\n").unwrap();
        (String::from_utf8_lossy(&response[..split]).to_string(), response[split + 4..].to_vec())
    }

    #[test]
    fn serves_a_repackaged_mkv_as_hls() {
        let dir = std::env::temp_dir().join(format!("frames-hls-{}", fastrand::u64(..)));
        std::fs::create_dir_all(&dir).unwrap();
        let src = dir.join("clip.mkv");
        let made = spawn_hidden_command("ffmpeg")
            .args(["-hide_banner", "-loglevel", "error", "-f", "lavfi", "-i", "testsrc=size=160x120:rate=10:duration=4"])
            .args(["-f", "lavfi", "-i", "sine=duration=4", "-c:v", "libx264", "-g", "10", "-c:a", "aac", "-shortest"])
            .arg(&src)
            .status()
            .is_ok_and(|s| s.success());
        if !made {
            let _ = std::fs::remove_dir_all(&dir);
            return; // no usable ffmpeg on this machine
        }
        let server = HlsServer::start(dir.join("hls")).unwrap();
        let playlist = server.open(src.to_str().unwrap(), PlaybackDecision::Repackage, H264Encoder::SoftwareX264, false).unwrap();

        let (head, body) = get(&playlist);
        assert!(head.starts_with("HTTP/1.1 200"), "{head}");
        let text = String::from_utf8_lossy(&body);
        assert!(text.starts_with("#EXTM3U") && text.contains("init.mp4"), "{text}");
        let (head, init) = get(&playlist.replace("index.m3u8", "init.mp4"));
        assert!(head.contains("video/mp4") && init.windows(4).any(|w| w == b"ftyp"));

        // Wrong token, unknown session or a path outside the folder: nothing.
        assert!(get(&playlist.replace(&server.token, "nope")).0.starts_with("HTTP/1.1 404"));
        assert!(get(&playlist.replace("index.m3u8", "..%2Fx")).0.starts_with("HTTP/1.1 404"));
        server.close_all();
        let _ = std::fs::remove_dir_all(&dir);
    }
}
