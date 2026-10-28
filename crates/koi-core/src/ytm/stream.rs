use crate::error::{Error, Result};
use crate::model::{AudioCodec, AudioStream};

/// Preferred stream: itag 140 (AAC 128k in M4A).
pub const PREFERRED_ITAG: u32 = 140;

/// Pick the audio stream to play/download.
///
/// Prefers itag 140, then the highest-bitrate AAC stream. Opus-only tracks are
/// reported as unavailable because the decoder has no Opus support.
pub fn select_audio_stream(streams: &[AudioStream]) -> Result<AudioStream> {
    if let Some(s) = streams.iter().find(|s| s.itag == PREFERRED_ITAG) {
        return Ok(s.clone());
    }
    streams
        .iter()
        .filter(|s| s.codec == AudioCodec::Aac)
        .max_by_key(|s| s.bitrate)
        .cloned()
        .ok_or_else(|| Error::unavailable("no AAC audio stream available for this track"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::error::ErrorKind;

    fn stream(itag: u32, codec: AudioCodec, bitrate: u32) -> AudioStream {
        AudioStream { url: format!("https://x/{itag}"), itag, codec, bitrate, size: 1, mime: String::new(), user_agent: String::new() }
    }

    #[test]
    fn prefers_itag_140() {
        let s = [stream(251, AudioCodec::Opus, 160_000), stream(140, AudioCodec::Aac, 128_000), stream(141, AudioCodec::Aac, 256_000)];
        assert_eq!(select_audio_stream(&s).unwrap().itag, 140);
    }

    #[test]
    fn falls_back_to_best_aac() {
        let s = [stream(251, AudioCodec::Opus, 160_000), stream(139, AudioCodec::Aac, 48_000), stream(141, AudioCodec::Aac, 256_000)];
        assert_eq!(select_audio_stream(&s).unwrap().itag, 141);
    }

    #[test]
    fn opus_only_is_unavailable() {
        let s = [stream(251, AudioCodec::Opus, 160_000), stream(250, AudioCodec::Opus, 64_000)];
        assert_eq!(select_audio_stream(&s).unwrap_err().kind, ErrorKind::Unavailable);
        assert_eq!(select_audio_stream(&[]).unwrap_err().kind, ErrorKind::Unavailable);
    }
}
