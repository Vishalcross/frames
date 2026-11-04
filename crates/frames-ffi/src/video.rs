//! Playing videos on the Mac (frames_swift_spec.md section 10): straight
//! from the original when AVFoundation can, otherwise through an HLS stream
//! that repackages or converts it.

use std::sync::Arc;

use frames_core::hls::HlsServer;
use frames_core::player::{decide_playback, detect_best_h264_encoder, PlaybackDecision, PlayerCapabilities};

use crate::{FramesError, Library};

/// How a video plays on this Mac.
#[derive(Debug, Clone, Copy, PartialEq, Eq, uniffi::Enum)]
pub enum VideoPlan {
    /// AVFoundation plays the original as it is.
    Direct,
    /// The codecs play but the container doesn't (H.264 in MKV, say): copy them into a stream.
    Repackage,
    /// Convert to H.264 and AAC while playing.
    Convert,
}

#[uniffi::export]
impl Library {
    /// The plan from the codecs recorded at import. Without them it says
    /// Direct: try the original, and convert only if that fails.
    pub fn video_plan(&self, asset_id: String) -> VideoPlan {
        let Some(record) = self.store.lock().ok().and_then(|s| s.get_asset(&asset_id).ok().flatten()).map(|d| d.record) else {
            return VideoPlan::Direct;
        };
        if record.container.is_none() && record.video_codec.is_none() {
            return VideoPlan::Direct;
        }
        match decide_playback(record.container.as_deref(), record.video_codec.as_deref(), record.audio_codec.as_deref(),
                              &PlayerCapabilities::default()) {
            PlaybackDecision::PlayDirect => VideoPlan::Direct,
            PlaybackDecision::Repackage => VideoPlan::Repackage,
            PlaybackDecision::Convert { .. } => VideoPlan::Convert,
        }
    }

    /// Start a stream of the video and return the playlist's URL for
    /// AVPlayer. `convert` forces a full conversion, for when a repackaged
    /// stream wouldn't play. Any earlier stream stops.
    pub fn start_video_stream(&self, asset_id: String, convert: bool) -> Result<String, FramesError> {
        let source = self
            .video_source(asset_id.clone())
            .ok_or_else(|| FramesError::other("The video isn't on this Mac and MEGA S4 can't be reached."))?;
        let record = self.store.lock().map_err(FramesError::other)?.get_asset(&asset_id).map_err(FramesError::other)?
            .map(|d| d.record)
            .ok_or_else(|| FramesError::other("Unknown video."))?;
        let decision = match decide_playback(record.container.as_deref(), record.video_codec.as_deref(),
                                             record.audio_codec.as_deref(), &PlayerCapabilities::default()) {
            PlaybackDecision::Repackage if !convert => PlaybackDecision::Repackage,
            PlaybackDecision::Convert { copy_video, copy_audio } if !convert => PlaybackDecision::Convert { copy_video, copy_audio },
            _ => PlaybackDecision::Convert { copy_video: false, copy_audio: false },
        };
        let hevc = record.video_codec.as_deref().is_some_and(|c| matches!(c.to_ascii_lowercase().as_str(), "hevc" | "h265" | "hvc1" | "hev1"));
        self.hls_server()?
            .open(&source, decision, detect_best_h264_encoder(), hevc)
            .map_err(|e| FramesError::other(format!("ffmpeg couldn't start: {e}")))
    }

    /// Stop any stream, deleting its temporary files.
    pub fn stop_video_streams(&self) {
        if let Some(server) = self.hls.lock().ok().and_then(|s| s.clone()) {
            server.close_all();
        }
    }
}

impl Library {
    fn hls_server(&self) -> Result<Arc<HlsServer>, FramesError> {
        let mut slot = self.hls.lock().map_err(FramesError::other)?;
        if let Some(server) = slot.as_ref() {
            return Ok(Arc::clone(server));
        }
        let server = HlsServer::start(self.thumbs.cache_root().join("streams")).map_err(FramesError::other)?;
        *slot = Some(Arc::clone(&server));
        Ok(server)
    }
}
