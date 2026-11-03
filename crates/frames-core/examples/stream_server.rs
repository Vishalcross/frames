//! Phase 0 video trial (frames_swift_spec.md section 10): serve one video
//! through the real loopback stream server and print its URLs, so a native
//! player can be pointed at them. Runs until killed.
//! Usage: cargo run --example stream_server -- <video file> <duration ms>

use frames_core::player::{H264Encoder, LoopbackStreamServer, PlaybackDecision};

fn main() {
    let mut args = std::env::args().skip(1);
    let path = args.next().expect("usage: stream_server <video file> <duration ms>");
    let duration_ms = args.next().and_then(|d| d.parse().ok());
    let server = LoopbackStreamServer::start().expect("start the loopback server");
    let session = |decision| {
        server.create_stream_session("trial", &path, None, decision, duration_ms, H264Encoder::VideoToolbox)
    };
    let repackage = session(PlaybackDecision::Repackage);
    let convert = session(PlaybackDecision::Convert { copy_video: false, copy_audio: false });
    println!("repackage {}", repackage.playback_url);
    println!("repackage-at-10s {}", server.build_seek_url(&repackage.playback_url, 10.0));
    println!("convert {}", convert.playback_url);
    loop {
        std::thread::sleep(std::time::Duration::from_secs(3600));
    }
}
