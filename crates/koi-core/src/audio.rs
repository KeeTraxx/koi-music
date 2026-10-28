//! Audio output: decoding and playing a file or HTTP stream on a dedicated thread.

use std::fs::File;
use std::io::{Read, Seek};
use std::path::PathBuf;
use std::sync::mpsc as std_mpsc;
use std::thread;
use std::time::Duration;

use rodio::{Decoder, Source};
use tokio::sync::mpsc::UnboundedSender;

use crate::error::{Error, Result};
use crate::http_reader::ChunkedHttpReader;

/// Identifies one `load` request so stale events can be ignored.
pub type LoadId = u64;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AudioInput {
    File(PathBuf),
    Stream { url: String, user_agent: String, size: Option<u64> },
}

#[derive(Debug, Clone)]
pub enum AudioEvent {
    /// The source is decoded and playing (or paused, if requested).
    Started { load: LoadId, duration: Option<Duration> },
    Position { load: LoadId, position: Duration },
    /// Playback reached the end of the source.
    Ended { load: LoadId },
    /// Loading or decoding failed. `stream_rejected` means the server refused
    /// the stream URL (e.g. expired), so re-resolving may help.
    Failed { load: LoadId, error: Error, stream_rejected: bool },
}

/// Something that can play audio. Implemented by [`RodioOutput`] and test fakes.
pub trait AudioOutput: Send + Sync + 'static {
    fn load(&self, load: LoadId, input: AudioInput, paused: bool);
    fn play(&self);
    fn pause(&self);
    fn seek(&self, position: Duration);
    /// Linear volume, 0.0..=1.0.
    fn set_volume(&self, volume: f32);
    fn stop(&self);
}

trait ReadSeek: Read + Seek + Send + Sync {}
impl<T: Read + Seek + Send + Sync> ReadSeek for T {}

type BoxDecoder = Decoder<Box<dyn ReadSeek>>;

enum Cmd {
    Load { load: LoadId, input: AudioInput, paused: bool },
    Prepared { load: LoadId, result: std::result::Result<BoxDecoder, (Error, bool)> },
    Play,
    Pause,
    Seek(Duration),
    Volume(f32),
    Stop,
}

/// Plays audio through the default output device using rodio.
pub struct RodioOutput {
    tx: std_mpsc::Sender<Cmd>,
}

const TICK: Duration = Duration::from_millis(250);

impl RodioOutput {
    /// Start the audio thread. `rt` is used for HTTP streaming.
    pub fn start(events: UnboundedSender<AudioEvent>, rt: tokio::runtime::Handle) -> Result<Self> {
        let (tx, rx) = std_mpsc::channel();
        let (ready_tx, ready_rx) = std_mpsc::channel();
        let worker_tx = tx.clone();
        thread::Builder::new()
            .name("koi-music-audio".into())
            .spawn(move || audio_thread(rx, worker_tx, events, rt, ready_tx))?;
        ready_rx.recv().map_err(|_| Error::audio("audio thread exited"))??;
        Ok(Self { tx })
    }

    fn send(&self, cmd: Cmd) {
        let _ = self.tx.send(cmd);
    }
}

impl AudioOutput for RodioOutput {
    fn load(&self, load: LoadId, input: AudioInput, paused: bool) {
        self.send(Cmd::Load { load, input, paused });
    }
    fn play(&self) {
        self.send(Cmd::Play);
    }
    fn pause(&self) {
        self.send(Cmd::Pause);
    }
    fn seek(&self, position: Duration) {
        self.send(Cmd::Seek(position));
    }
    fn set_volume(&self, volume: f32) {
        self.send(Cmd::Volume(volume));
    }
    fn stop(&self) {
        self.send(Cmd::Stop);
    }
}

fn audio_thread(
    rx: std_mpsc::Receiver<Cmd>,
    self_tx: std_mpsc::Sender<Cmd>,
    events: UnboundedSender<AudioEvent>,
    rt: tokio::runtime::Handle,
    ready: std_mpsc::Sender<Result<()>>,
) {
    let mut sink = match rodio::DeviceSinkBuilder::open_default_sink() {
        Ok(s) => s,
        Err(e) => {
            let _ = ready.send(Err(Error::audio(format!("cannot open audio device: {e}"))));
            return;
        }
    };
    sink.log_on_drop(false);
    let _ = ready.send(Ok(()));

    let mut player = rodio::Player::connect_new(sink.mixer());
    let mut volume = 1.0f32;
    // Load currently requested / playing.
    let mut wanted: Option<(LoadId, bool)> = None;
    let mut playing: Option<LoadId> = None;

    loop {
        match rx.recv_timeout(TICK) {
            Ok(Cmd::Load { load, input, paused }) => {
                player.stop();
                playing = None;
                wanted = Some((load, paused));
                let tx = self_tx.clone();
                let rt = rt.clone();
                thread::spawn(move || {
                    let result = open_source(input, &rt);
                    let _ = tx.send(Cmd::Prepared { load, result });
                });
            }
            Ok(Cmd::Prepared { load, result }) => {
                let Some((want, paused)) = wanted else { continue };
                if want != load {
                    continue; // superseded
                }
                match result {
                    Ok(decoder) => {
                        let duration = decoder.total_duration();
                        // A fresh player per track so pause/seek state never leaks between tracks.
                        player = rodio::Player::connect_new(sink.mixer());
                        player.set_volume(volume);
                        if paused {
                            player.pause();
                        }
                        player.append(decoder);
                        playing = Some(load);
                        let _ = events.send(AudioEvent::Started { load, duration });
                    }
                    Err((error, stream_rejected)) => {
                        wanted = None;
                        let _ = events.send(AudioEvent::Failed { load, error, stream_rejected });
                    }
                }
            }
            Ok(Cmd::Play) => player.play(),
            Ok(Cmd::Pause) => player.pause(),
            Ok(Cmd::Seek(pos)) => {
                if let Err(e) = player.try_seek(pos) {
                    tracing::warn!("seek failed: {e}");
                }
                if let Some(load) = playing {
                    let _ = events.send(AudioEvent::Position { load, position: player.get_pos() });
                }
            }
            Ok(Cmd::Volume(v)) => {
                volume = v.clamp(0.0, 1.0);
                player.set_volume(volume);
            }
            Ok(Cmd::Stop) => {
                player.stop();
                playing = None;
                wanted = None;
            }
            Err(std_mpsc::RecvTimeoutError::Timeout) => {}
            Err(std_mpsc::RecvTimeoutError::Disconnected) => break,
        }

        if let Some(load) = playing {
            if player.empty() {
                playing = None;
                wanted = None;
                let _ = events.send(AudioEvent::Ended { load });
            } else if !player.is_paused() {
                let _ = events.send(AudioEvent::Position { load, position: player.get_pos() });
            }
        }
    }
}

/// Open and probe a source. Runs on a worker thread (may block on the network).
fn open_source(
    input: AudioInput,
    rt: &tokio::runtime::Handle,
) -> std::result::Result<BoxDecoder, (Error, bool)> {
    let (reader, len): (Box<dyn ReadSeek>, Option<u64>) = match input {
        AudioInput::File(path) => {
            let file = File::open(&path).map_err(|e| (Error::from(e), false))?;
            let len = file.metadata().ok().map(|m| m.len());
            (Box::new(std::io::BufReader::new(file)), len)
        }
        AudioInput::Stream { url, user_agent, size } => {
            let reader = rt.block_on(ChunkedHttpReader::open(&url, &user_agent, size)).map_err(|e| (e.error, e.rejected))?;
            let len = reader.len();
            (Box::new(reader), Some(len))
        }
    };
    let mut builder = Decoder::builder().with_data(reader).with_seekable(true).with_hint("m4a").with_mime_type("audio/mp4");
    if let Some(len) = len {
        builder = builder.with_byte_len(len);
    }
    builder.build().map_err(|e| (Error::audio(format!("cannot decode audio: {e}")), false))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;
    use tokio::sync::mpsc::UnboundedReceiver;

    fn fixture() -> PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/tone-5s.m4a")
    }

    async fn next_event(rx: &mut UnboundedReceiver<AudioEvent>, pred: impl Fn(&AudioEvent) -> bool) -> AudioEvent {
        tokio::time::timeout(Duration::from_secs(15), async {
            loop {
                let ev = rx.recv().await.expect("audio thread alive");
                if pred(&ev) {
                    return ev;
                }
            }
        })
        .await
        .expect("event in time")
    }

    fn output() -> Option<(RodioOutput, UnboundedReceiver<AudioEvent>, tempfile::TempDir)> {
        let (tx, rx) = tokio::sync::mpsc::unbounded_channel();
        let dir = tempfile::tempdir().unwrap();
        match RodioOutput::start(tx, tokio::runtime::Handle::current()) {
            Ok(o) => Some((o, rx, dir)),
            Err(e) => {
                eprintln!("skipping: no audio device ({e})");
                None
            }
        }
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn plays_pauses_seeks_local_m4a() {
        let Some((out, mut rx, _d)) = output() else { return };
        out.set_volume(0.0);
        out.load(1, AudioInput::File(fixture()), false);
        let ev = next_event(&mut rx, |e| matches!(e, AudioEvent::Started { .. } | AudioEvent::Failed { .. })).await;
        let AudioEvent::Started { duration, .. } = ev else { panic!("{ev:?}") };
        let d = duration.expect("duration known");
        assert!((4..=6).contains(&d.as_secs()), "{d:?}");

        out.seek(Duration::from_secs(3));
        let ev = next_event(&mut rx, |e| matches!(e, AudioEvent::Position { position, .. } if *position >= Duration::from_secs(3))).await;
        assert!(matches!(ev, AudioEvent::Position { load: 1, .. }));

        out.pause();
        tokio::time::sleep(Duration::from_millis(300)).await;
        while rx.try_recv().is_ok() {}
        tokio::time::sleep(Duration::from_millis(600)).await;
        assert!(rx.try_recv().is_err(), "no position updates while paused");

        out.play();
        next_event(&mut rx, |e| matches!(e, AudioEvent::Ended { load: 1 })).await;
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn missing_file_fails() {
        let Some((out, mut rx, _d)) = output() else { return };
        out.load(7, AudioInput::File("/nonexistent.m4a".into()), false);
        let ev = next_event(&mut rx, |_| true).await;
        assert!(matches!(ev, AudioEvent::Failed { load: 7, stream_rejected: false, .. }), "{ev:?}");
    }

    #[tokio::test(flavor = "multi_thread")]
    #[ignore = "network"]
    async fn streams_and_seeks_real_track() {
        use crate::ytm::{HybridSource, MusicSource, RustyPipeSource};
        let Some((out, mut rx, dir)) = output() else { return };
        let source = HybridSource::new(RustyPipeSource::new(&dir.path().join("rp")).unwrap(), crate::ytdlp::YtDlp::new("yt-dlp"));
        let stream = source.audio_stream(crate::ytm::fake::CC_VIDEO_ID).await.unwrap();
        out.set_volume(0.0);
        out.load(1, AudioInput::Stream { url: stream.url, user_agent: stream.user_agent, size: Some(stream.size) }, false);
        let ev = next_event(&mut rx, |e| matches!(e, AudioEvent::Started { .. } | AudioEvent::Failed { .. })).await;
        assert!(matches!(ev, AudioEvent::Started { .. }), "{ev:?}");
        out.seek(Duration::from_secs(60));
        next_event(&mut rx, |e| matches!(e, AudioEvent::Position { position, .. } if *position >= Duration::from_secs(60))).await;
    }
}
