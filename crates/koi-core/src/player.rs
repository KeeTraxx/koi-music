//! Player service: the play queue combined with an [`AudioOutput`].
//!
//! The player runs as a tokio task that owns the [`Queue`] and reacts to user
//! commands and audio events. The UI sends [`PlayerCmd`]s and reads
//! [`PlayerState`] snapshots from a `watch` channel.

use std::sync::Arc;
use std::time::Duration;

use tokio::sync::{mpsc, watch};

use crate::audio::{AudioEvent, AudioInput, AudioOutput, LoadId};
use crate::error::{Error, Result};
use crate::library::{Library, keys};
use crate::model::Track;
use crate::notify::Notifier;
use crate::queue::{ItemId, Previous, Queue, QueueItem, RepeatMode};
use crate::ytm::MusicSource;

/// A track counts as played (history) after this much playback.
pub const HISTORY_THRESHOLD: Duration = Duration::from_secs(30);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Status {
    #[default]
    Stopped,
    /// Resolving the stream or opening the file.
    Loading,
    Playing,
    Paused,
}

/// A playback error to show to the user. `seq` increases with every new error.
#[derive(Debug, Clone, PartialEq)]
pub struct PlayerError {
    pub seq: u64,
    pub message: String,
}

/// Snapshot of the player published after every change.
#[derive(Debug, Clone, PartialEq)]
pub struct PlayerState {
    pub status: Status,
    pub queue: Vec<QueueItem>,
    pub current: Option<usize>,
    pub position: Duration,
    pub duration: Option<Duration>,
    /// Whether the current track plays from a downloaded file.
    pub local: bool,
    /// 0.0..=1.0
    pub volume: f32,
    pub muted: bool,
    pub repeat: RepeatMode,
    pub shuffle: bool,
    pub last_error: Option<PlayerError>,
}

impl Default for PlayerState {
    fn default() -> Self {
        Self {
            status: Status::Stopped,
            queue: Vec::new(),
            current: None,
            position: Duration::ZERO,
            duration: None,
            local: false,
            volume: 1.0,
            muted: false,
            repeat: RepeatMode::Off,
            shuffle: false,
            last_error: None,
        }
    }
}

impl PlayerState {
    pub fn current_track(&self) -> Option<&Track> {
        self.current.and_then(|i| self.queue.get(i)).map(|i| &i.track)
    }

    pub fn current_item_id(&self) -> Option<ItemId> {
        self.current.and_then(|i| self.queue.get(i)).map(|i| i.id)
    }
}

#[derive(Debug, Clone)]
pub enum PlayerCmd {
    /// Replace the queue with `tracks` and play from `start`.
    PlayTracks { tracks: Vec<Track>, start: usize },
    /// Like `PlayTracks`, with shuffle switched on.
    PlayShuffled(Vec<Track>),
    Append(Vec<Track>),
    PlayNext(Track),
    Remove(usize),
    Move { from: usize, to: usize },
    Clear,
    /// Play the queue item at this index.
    Jump(usize),
    TogglePlay,
    Play,
    Pause,
    Next,
    Previous,
    Seek(Duration),
    SetVolume(f32),
    SetMuted(bool),
    SetRepeat(RepeatMode),
    SetShuffle(bool),
}

/// Handle to the player task. Cheap to clone.
#[derive(Clone)]
pub struct Player {
    tx: mpsc::UnboundedSender<PlayerCmd>,
    state: watch::Receiver<Arc<PlayerState>>,
}

impl Player {
    /// Spawn the player task on the current tokio runtime.
    pub fn start(
        source: Arc<dyn MusicSource>,
        library: Arc<Library>,
        audio: Arc<dyn AudioOutput>,
        events: mpsc::UnboundedReceiver<AudioEvent>,
        notifier: Notifier,
    ) -> Self {
        let (tx, rx) = mpsc::unbounded_channel();
        let mut initial = PlayerState::default();
        if let Ok(Some(v)) = library.setting::<f32>(keys::VOLUME) {
            initial.volume = v.clamp(0.0, 1.0);
        }
        if let Ok(Some(m)) = library.setting::<bool>(keys::MUTED) {
            initial.muted = m;
        }
        audio.set_volume(effective_volume(&initial));
        let (state_tx, state) = watch::channel(Arc::new(initial.clone()));
        let (internal_tx, internal_rx) = mpsc::unbounded_channel();
        let actor = Actor {
            source,
            library,
            audio,
            notifier,
            state_tx,
            internal_tx,
            queue: Queue::new(),
            st: initial,
            load: 0,
            want_paused: false,
            retried: false,
            played: Duration::ZERO,
            recorded: false,
            consecutive_failures: 0,
            error_seq: 0,
        };
        tokio::spawn(actor.run(rx, events, internal_rx));
        Self { tx, state }
    }

    pub fn send(&self, cmd: PlayerCmd) {
        let _ = self.tx.send(cmd);
    }

    pub fn state(&self) -> Arc<PlayerState> {
        self.state.borrow().clone()
    }

    /// A receiver to await state changes (e.g. for OS media controls).
    pub fn subscribe(&self) -> watch::Receiver<Arc<PlayerState>> {
        self.state.clone()
    }
}

fn effective_volume(st: &PlayerState) -> f32 {
    if st.muted { 0.0 } else { st.volume }
}

/// Messages from helper tasks back to the actor.
enum Internal {
    Resolved { load: LoadId, result: Result<(AudioInput, bool)> },
}

struct Actor {
    source: Arc<dyn MusicSource>,
    library: Arc<Library>,
    audio: Arc<dyn AudioOutput>,
    notifier: Notifier,
    state_tx: watch::Sender<Arc<PlayerState>>,
    internal_tx: mpsc::UnboundedSender<Internal>,
    queue: Queue,
    st: PlayerState,
    /// Id of the current load; events of older loads are ignored.
    load: LoadId,
    want_paused: bool,
    /// The stream URL of the current load was already re-resolved once.
    retried: bool,
    /// Playback time of the current track (excluding seeks), for history.
    played: Duration,
    recorded: bool,
    consecutive_failures: usize,
    error_seq: u64,
}

impl Actor {
    async fn run(
        mut self,
        mut cmds: mpsc::UnboundedReceiver<PlayerCmd>,
        mut events: mpsc::UnboundedReceiver<AudioEvent>,
        mut internal: mpsc::UnboundedReceiver<Internal>,
    ) {
        loop {
            tokio::select! {
                cmd = cmds.recv() => match cmd {
                    Some(cmd) => self.on_cmd(cmd),
                    None => break,
                },
                Some(ev) = events.recv() => self.on_audio(ev),
                Some(msg) = internal.recv() => self.on_internal(msg),
            }
            self.publish();
        }
        self.audio.stop();
    }

    fn publish(&mut self) {
        self.st.queue = self.queue.items().to_vec();
        self.st.current = self.queue.current_index();
        self.st.repeat = self.queue.repeat();
        self.st.shuffle = self.queue.shuffle();
        let changed = **self.state_tx.borrow() != self.st;
        if changed {
            self.state_tx.send_replace(Arc::new(self.st.clone()));
            self.notifier.notify();
        }
    }

    fn report(&mut self, message: String) {
        tracing::warn!("{message}");
        self.error_seq += 1;
        self.st.last_error = Some(PlayerError { seq: self.error_seq, message });
    }

    fn on_cmd(&mut self, cmd: PlayerCmd) {
        let mut rng = rand::rng();
        match cmd {
            PlayerCmd::PlayTracks { tracks, start } => {
                self.queue.set(tracks, start, &mut rng);
                self.consecutive_failures = 0;
                self.start_current(false);
            }
            PlayerCmd::PlayShuffled(tracks) => {
                self.queue.set_shuffle(false, &mut rng);
                self.queue.set(tracks, 0, &mut rng);
                // Shuffle everything, including the first track.
                self.queue.jump(0);
                let n = self.queue.len();
                if n > 0 {
                    let first = rand::RngExt::random_range(&mut rng, 0..n);
                    self.queue.move_item(first, 0);
                    self.queue.jump(0);
                }
                self.queue.set_shuffle(true, &mut rng);
                self.consecutive_failures = 0;
                self.start_current(false);
            }
            PlayerCmd::Append(tracks) => {
                let was_empty = self.queue.is_empty();
                for t in tracks {
                    self.queue.append(t);
                }
                if was_empty && !self.queue.is_empty() {
                    self.queue.jump(0);
                    self.start_current(false);
                }
            }
            PlayerCmd::PlayNext(track) => {
                let was_empty = self.queue.is_empty();
                self.queue.play_next(track);
                if was_empty {
                    self.queue.jump(0);
                    self.start_current(false);
                }
            }
            PlayerCmd::Remove(index) => {
                if self.queue.remove(index) {
                    if self.queue.current().is_some() && self.st.status != Status::Stopped {
                        self.start_current(self.st.status == Status::Paused);
                    } else {
                        self.stop();
                    }
                }
            }
            PlayerCmd::Move { from, to } => self.queue.move_item(from, to),
            PlayerCmd::Clear => {
                self.queue.clear();
                self.stop();
            }
            PlayerCmd::Jump(index) => {
                if index < self.queue.len() {
                    self.queue.jump(index);
                    self.consecutive_failures = 0;
                    self.start_current(false);
                }
            }
            PlayerCmd::TogglePlay => match self.st.status {
                Status::Playing => self.pause(),
                Status::Loading if !self.want_paused => self.pause(),
                _ => self.play(),
            },
            PlayerCmd::Play => self.play(),
            PlayerCmd::Pause => self.pause(),
            PlayerCmd::Next => {
                if self.queue.advance().is_some() {
                    self.consecutive_failures = 0;
                    self.start_current(self.st.status == Status::Paused);
                }
            }
            PlayerCmd::Previous => match self.queue.previous(self.st.position) {
                Some(Previous::Restart) => self.seek(Duration::ZERO),
                Some(Previous::Play(_)) => {
                    self.consecutive_failures = 0;
                    self.start_current(self.st.status == Status::Paused);
                }
                None => {}
            },
            PlayerCmd::Seek(pos) => self.seek(pos),
            PlayerCmd::SetVolume(v) => {
                self.st.volume = v.clamp(0.0, 1.0);
                self.st.muted = false;
                self.apply_volume();
            }
            PlayerCmd::SetMuted(m) => {
                self.st.muted = m;
                self.apply_volume();
            }
            PlayerCmd::SetRepeat(mode) => self.queue.set_repeat(mode),
            PlayerCmd::SetShuffle(on) => self.queue.set_shuffle(on, &mut rng),
        }
    }

    fn apply_volume(&mut self) {
        self.audio.set_volume(effective_volume(&self.st));
        let (lib, volume, muted) = (self.library.clone(), self.st.volume, self.st.muted);
        tokio::task::spawn_blocking(move || {
            if let Err(e) = lib.set_setting(keys::VOLUME, &volume).and_then(|_| lib.set_setting(keys::MUTED, &muted)) {
                tracing::warn!("cannot save volume: {e}");
            }
        });
    }

    fn play(&mut self) {
        match self.st.status {
            Status::Paused => {
                self.audio.play();
                self.st.status = Status::Playing;
            }
            Status::Loading => self.want_paused = false,
            Status::Stopped if self.queue.current().is_some() => self.start_current(false),
            _ => {}
        }
    }

    fn pause(&mut self) {
        match self.st.status {
            Status::Playing => {
                self.audio.pause();
                self.st.status = Status::Paused;
            }
            Status::Loading => self.want_paused = true,
            _ => {}
        }
    }

    fn seek(&mut self, pos: Duration) {
        if matches!(self.st.status, Status::Playing | Status::Paused) {
            let pos = self.st.duration.map_or(pos, |d| pos.min(d));
            self.audio.seek(pos);
            self.st.position = pos;
        } else if self.st.status == Status::Stopped && self.queue.current().is_some() && pos.is_zero() {
            self.start_current(false);
        }
    }

    fn stop(&mut self) {
        self.load += 1; // invalidate pending loads
        self.audio.stop();
        self.st.status = Status::Stopped;
        self.st.position = Duration::ZERO;
        if self.queue.current().is_none() {
            self.st.duration = None;
            self.st.local = false;
        }
    }

    /// Start loading the current queue item.
    fn start_current(&mut self, paused: bool) {
        let Some(item) = self.queue.current().cloned() else {
            self.stop();
            return;
        };
        self.load += 1;
        self.want_paused = paused;
        self.retried = false;
        self.played = Duration::ZERO;
        self.recorded = false;
        self.st.status = Status::Loading;
        self.st.position = Duration::ZERO;
        self.st.duration = item.track.duration_s.map(|s| Duration::from_secs(s.into()));
        self.st.local = false;
        self.audio.stop();
        self.resolve(self.load, item.track);
    }

    /// Find the input for a track: the downloaded file if present, else a stream.
    fn resolve(&self, load: LoadId, track: Track) {
        let (source, library, tx) = (self.source.clone(), self.library.clone(), self.internal_tx.clone());
        tokio::spawn(async move {
            let id = track.video_id.clone();
            let lib = library.clone();
            let local = tokio::task::spawn_blocking(move || lib.downloaded_path(&id))
                .await
                .map_err(|e| Error::audio(e.to_string()))
                .and_then(|r| r);
            let result = match local {
                Ok(Some(path)) => Ok((AudioInput::File(path), true)),
                Ok(None) | Err(_) => source.audio_stream(&track.video_id).await.map(|s| {
                    let size = (s.size > 0).then_some(s.size);
                    (AudioInput::Stream { url: s.url, user_agent: s.user_agent, size }, false)
                }),
            };
            let _ = tx.send(Internal::Resolved { load, result });
        });
    }

    fn on_internal(&mut self, msg: Internal) {
        match msg {
            Internal::Resolved { load, result } if load == self.load => match result {
                Ok((input, local)) => {
                    self.st.local = local;
                    self.audio.load(load, input, self.want_paused);
                }
                Err(e) => self.fail_current(e),
            },
            Internal::Resolved { .. } => {}
        }
    }

    fn on_audio(&mut self, ev: AudioEvent) {
        match ev {
            AudioEvent::Started { load, duration } if load == self.load => {
                self.consecutive_failures = 0;
                if duration.is_some() {
                    self.st.duration = duration;
                }
                if self.want_paused {
                    self.st.status = Status::Paused;
                } else {
                    self.st.status = Status::Playing;
                }
            }
            AudioEvent::Position { load, position } if load == self.load => {
                let delta = position.saturating_sub(self.st.position);
                // Small forward steps are playback; larger jumps are seeks.
                if self.st.status == Status::Playing && delta <= Duration::from_millis(1500) {
                    self.played += delta;
                }
                self.st.position = position;
                self.maybe_record_history();
            }
            AudioEvent::Ended { load } if load == self.load => {
                if self.queue.on_track_end().is_some() {
                    self.start_current(false);
                } else {
                    self.stop();
                }
            }
            AudioEvent::Failed { load, error, stream_rejected } if load == self.load => {
                if stream_rejected && !self.retried && !self.st.local {
                    // The cached URL probably expired: resolve again, once.
                    if let Some(item) = self.queue.current().cloned() {
                        self.retried = true;
                        self.source.invalidate_stream(&item.track.video_id);
                        self.resolve(load, item.track);
                        return;
                    }
                }
                self.fail_current(error);
            }
            _ => {} // stale event of a previous load
        }
    }

    fn maybe_record_history(&mut self) {
        if self.recorded || self.played < HISTORY_THRESHOLD {
            return;
        }
        let Some(track) = self.queue.current().map(|i| i.track.clone()) else { return };
        self.recorded = true;
        let lib = self.library.clone();
        tokio::task::spawn_blocking(move || {
            if let Err(e) = lib.record_play(&track) {
                tracing::warn!("cannot record history: {e}");
            }
        });
    }

    /// The current track cannot be played: report it and skip to the next one.
    fn fail_current(&mut self, error: Error) {
        let title = self.queue.current().map(|i| i.track.title.clone()).unwrap_or_default();
        self.report(format!("Cannot play \"{title}\": {error}"));
        self.consecutive_failures += 1;
        // Stop after every queue item failed in a row (e.g. offline with repeat-all).
        if self.consecutive_failures < self.queue.len() && self.queue.advance().is_some() {
            self.start_current(false);
        } else {
            self.consecutive_failures = 0;
            self.stop();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ytm::fake::{FakeSource, track};
    use std::sync::Mutex;

    #[derive(Debug, Clone, PartialEq)]
    enum Call {
        Load(LoadId, AudioInput, bool),
        Play,
        Pause,
        Seek(Duration),
        Volume(f32),
        Stop,
    }

    /// Records calls; the test drives audio events by hand.
    struct FakeOutput {
        calls: Mutex<Vec<Call>>,
        tx: mpsc::UnboundedSender<Call>,
    }

    impl FakeOutput {
        fn record(&self, c: Call) {
            self.calls.lock().unwrap().push(c.clone());
            let _ = self.tx.send(c);
        }
    }

    impl AudioOutput for FakeOutput {
        fn load(&self, load: LoadId, input: AudioInput, paused: bool) {
            self.record(Call::Load(load, input, paused));
        }
        fn play(&self) {
            self.record(Call::Play);
        }
        fn pause(&self) {
            self.record(Call::Pause);
        }
        fn seek(&self, position: Duration) {
            self.record(Call::Seek(position));
        }
        fn set_volume(&self, volume: f32) {
            self.record(Call::Volume(volume));
        }
        fn stop(&self) {
            self.record(Call::Stop);
        }
    }

    struct Harness {
        player: Player,
        source: Arc<FakeSource>,
        library: Arc<Library>,
        events: mpsc::UnboundedSender<AudioEvent>,
        calls: mpsc::UnboundedReceiver<Call>,
        output: Arc<FakeOutput>,
        state: watch::Receiver<Arc<PlayerState>>,
    }

    fn harness_with(source: FakeSource, library: Library) -> Harness {
        let (calls_tx, calls) = mpsc::unbounded_channel();
        let out = Arc::new(FakeOutput { calls: Mutex::default(), tx: calls_tx });
        let (events, events_rx) = mpsc::unbounded_channel();
        let source = Arc::new(source);
        let library = Arc::new(library);
        let player = Player::start(source.clone(), library.clone(), out.clone(), events_rx, Notifier::noop());
        let state = player.subscribe();
        Harness { player, source, library, events, calls, output: out, state }
    }

    fn harness() -> Harness {
        harness_with(FakeSource::new(), Library::open_in_memory().unwrap())
    }

    fn tracks(ids: &[&str]) -> Vec<Track> {
        ids.iter().map(|i| track(i)).collect()
    }

    impl Harness {
        /// Wait for the next `load` call and return its id and input.
        async fn next_load(&mut self) -> (LoadId, AudioInput) {
            tokio::time::timeout(Duration::from_secs(5), async {
                loop {
                    if let Call::Load(id, input, _) = self.calls.recv().await.unwrap() {
                        return (id, input);
                    }
                }
            })
            .await
            .expect("load call")
        }

        async fn wait(&mut self, what: &str, pred: impl Fn(&PlayerState) -> bool) -> Arc<PlayerState> {
            tokio::time::timeout(Duration::from_secs(5), self.state.wait_for(|s| pred(s)))
                .await
                .unwrap_or_else(|_| panic!("timed out waiting for {what}: {:?}", self.player.state()))
                .unwrap()
                .clone()
        }

        /// Wait until a track is loaded and report it started.
        async fn start(&mut self) -> (LoadId, AudioInput) {
            let (load, input) = self.next_load().await;
            self.events.send(AudioEvent::Started { load, duration: Some(Duration::from_secs(200)) }).unwrap();
            self.wait("playing", |s| s.status == Status::Playing).await;
            (load, input)
        }

        fn current(&self) -> Option<String> {
            self.player.state().current_track().map(|t| t.video_id.clone())
        }
    }

    fn stream_url(input: &AudioInput) -> &str {
        match input {
            AudioInput::Stream { url, .. } => url,
            AudioInput::File(p) => p.to_str().unwrap(),
        }
    }

    #[tokio::test]
    async fn plays_and_auto_advances_then_stops_at_end() {
        let mut h = harness();
        h.player.send(PlayerCmd::PlayTracks { tracks: tracks(&["a", "b"]), start: 0 });
        let (load, input) = h.start().await;
        assert_eq!(stream_url(&input), "https://stream/a");
        h.events.send(AudioEvent::Ended { load }).unwrap();
        let (load, input) = h.start().await;
        assert_eq!(stream_url(&input), "https://stream/b");
        h.events.send(AudioEvent::Ended { load }).unwrap();
        let s = h.wait("stopped", |s| s.status == Status::Stopped).await;
        assert_eq!(s.current_track().unwrap().video_id, "b", "keeps showing the last track");
    }

    #[tokio::test]
    async fn repeat_one_replays_and_repeat_all_wraps() {
        let mut h = harness();
        h.player.send(PlayerCmd::SetRepeat(RepeatMode::One));
        h.player.send(PlayerCmd::PlayTracks { tracks: tracks(&["a", "b"]), start: 0 });
        let (load, _) = h.start().await;
        h.events.send(AudioEvent::Ended { load }).unwrap();
        let (load, input) = h.start().await;
        assert_eq!(stream_url(&input), "https://stream/a");
        h.player.send(PlayerCmd::SetRepeat(RepeatMode::All));
        h.player.send(PlayerCmd::Jump(1));
        let (load2, _) = h.start().await;
        assert!(load2 > load);
        h.events.send(AudioEvent::Ended { load: load2 }).unwrap();
        let (_, input) = h.start().await;
        assert_eq!(stream_url(&input), "https://stream/a");
    }

    #[tokio::test]
    async fn unavailable_track_is_reported_and_skipped() {
        let src = FakeSource::new();
        src.unavailable.lock().unwrap().push("b".into());
        let mut h = harness_with(src, Library::open_in_memory().unwrap());
        h.player.send(PlayerCmd::PlayTracks { tracks: tracks(&["b", "c"]), start: 0 });
        let (_, input) = h.start().await;
        assert_eq!(stream_url(&input), "https://stream/c");
        let s = h.player.state();
        let err = s.last_error.as_ref().expect("error shown");
        assert!(err.message.contains("Title b"), "{}", err.message);
    }

    #[tokio::test]
    async fn all_unavailable_stops_instead_of_looping() {
        let src = FakeSource::new();
        src.unavailable.lock().unwrap().extend(["a".to_string(), "b".to_string()]);
        let mut h = harness_with(src, Library::open_in_memory().unwrap());
        h.player.send(PlayerCmd::SetRepeat(RepeatMode::All));
        h.player.send(PlayerCmd::PlayTracks { tracks: tracks(&["a", "b"]), start: 0 });
        h.wait("stopped with error", |s| s.status == Status::Stopped && s.last_error.is_some()).await;
        tokio::time::sleep(Duration::from_millis(100)).await;
        assert_eq!(h.player.state().status, Status::Stopped);
    }

    #[tokio::test]
    async fn rejected_stream_is_re_resolved_once() {
        let mut h = harness();
        h.player.send(PlayerCmd::PlayTracks { tracks: tracks(&["a", "b"]), start: 0 });
        let (load, _) = h.next_load().await;
        h.events.send(AudioEvent::Failed { load, error: Error::network("403"), stream_rejected: true }).unwrap();
        let (load2, input) = h.next_load().await;
        assert_eq!(load2, load, "same load retried");
        assert_eq!(stream_url(&input), "https://stream/a");
        assert_eq!(h.source.stream_calls.lock().unwrap()["a"], 2);
        // A second rejection skips the track.
        h.events.send(AudioEvent::Failed { load, error: Error::network("403"), stream_rejected: true }).unwrap();
        let (_, input) = h.next_load().await;
        assert_eq!(stream_url(&input), "https://stream/b");
    }

    #[tokio::test]
    async fn prefers_downloaded_file_and_clears_missing() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("a.m4a");
        std::fs::write(&file, b"x").unwrap();
        let lib = Library::open_in_memory().unwrap();
        lib.add_download(&track("a"), &file, 1).unwrap();
        lib.add_download(&track("b"), &dir.path().join("missing.m4a"), 1).unwrap();
        let mut h = harness_with(FakeSource::new(), lib);
        h.player.send(PlayerCmd::PlayTracks { tracks: tracks(&["a", "b"]), start: 0 });
        let (load, input) = h.start().await;
        assert_eq!(input, AudioInput::File(file));
        assert!(h.player.state().local);
        assert!(!h.source.stream_calls.lock().unwrap().contains_key("a"), "no network for downloaded track");
        h.events.send(AudioEvent::Ended { load }).unwrap();
        let (_, input) = h.start().await;
        assert_eq!(stream_url(&input), "https://stream/b");
        assert!(!h.library.snapshot().is_downloaded("b"), "missing file cleared");
    }

    #[tokio::test]
    async fn pause_resume_seek_and_previous() {
        let mut h = harness();
        h.player.send(PlayerCmd::PlayTracks { tracks: tracks(&["a", "b"]), start: 1 });
        let (load, _) = h.start().await;
        h.player.send(PlayerCmd::TogglePlay);
        h.wait("paused", |s| s.status == Status::Paused).await;
        h.player.send(PlayerCmd::TogglePlay);
        h.wait("playing", |s| s.status == Status::Playing).await;
        h.player.send(PlayerCmd::Seek(Duration::from_secs(90)));
        h.wait("seeked", |s| s.position == Duration::from_secs(90)).await;
        // More than 3 s in: previous restarts.
        h.player.send(PlayerCmd::Previous);
        h.wait("restarted", |s| s.position.is_zero()).await;
        assert_eq!(h.current().as_deref(), Some("b"));
        h.events.send(AudioEvent::Position { load, position: Duration::from_secs(1) }).unwrap();
        h.wait("position 1s", |s| s.position == Duration::from_secs(1)).await;
        // Within 3 s: previous goes to the preceding item.
        h.player.send(PlayerCmd::Previous);
        let (_, input) = h.next_load().await;
        assert_eq!(stream_url(&input), "https://stream/a");
        let calls = h.calls_snapshot();
        assert!(calls.contains(&Call::Pause) && calls.contains(&Call::Play));
        assert!(calls.contains(&Call::Seek(Duration::from_secs(90))) && calls.contains(&Call::Seek(Duration::ZERO)));
    }

    impl Harness {
        /// Every call made to the output so far.
        fn calls_snapshot(&self) -> Vec<Call> {
            self.output.calls.lock().unwrap().clone()
        }
    }

    #[tokio::test]
    async fn play_next_and_queue_editing() {
        let mut h = harness();
        h.player.send(PlayerCmd::PlayTracks { tracks: tracks(&["a", "b"]), start: 0 });
        let (load, _) = h.start().await;
        h.player.send(PlayerCmd::PlayNext(track("n")));
        h.player.send(PlayerCmd::Append(tracks(&["z"])));
        h.wait("4 items", |s| s.queue.len() == 4).await;
        let ids: Vec<_> = h.player.state().queue.iter().map(|i| i.track.video_id.clone()).collect();
        assert_eq!(ids, ["a", "n", "b", "z"]);
        h.events.send(AudioEvent::Ended { load }).unwrap();
        let (_, input) = h.start().await;
        assert_eq!(stream_url(&input), "https://stream/n");
        h.player.send(PlayerCmd::Clear);
        let s = h.wait("cleared", |s| s.queue.is_empty()).await;
        assert_eq!(s.status, Status::Stopped);
    }

    #[tokio::test]
    async fn volume_is_persisted() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("library.db");
        {
            let mut h = harness_with(FakeSource::new(), Library::open(&path).unwrap());
            h.player.send(PlayerCmd::SetVolume(0.4));
            h.wait("volume", |s| s.volume == 0.4).await;
            tokio::time::sleep(Duration::from_millis(100)).await; // saved on a blocking thread
        }
        let h = harness_with(FakeSource::new(), Library::open(&path).unwrap());
        assert_eq!(h.player.state().volume, 0.4);
    }

    #[tokio::test]
    async fn history_recorded_after_30s_of_playback() {
        let mut h = harness();
        h.player.send(PlayerCmd::PlayTracks { tracks: tracks(&["a"]), start: 0 });
        let (load, _) = h.start().await;
        // A seek to 0:40 does not count as listening.
        h.events.send(AudioEvent::Position { load, position: Duration::from_secs(40) }).unwrap();
        for ms in (40_250..=69_000).step_by(250) {
            h.events.send(AudioEvent::Position { load, position: Duration::from_millis(ms) }).unwrap();
        }
        h.wait("position", |s| s.position == Duration::from_secs(69)).await;
        tokio::time::sleep(Duration::from_millis(100)).await;
        assert!(h.library.snapshot().history.is_empty(), "29 s played");
        for ms in (69_250..=71_000).step_by(250) {
            h.events.send(AudioEvent::Position { load, position: Duration::from_millis(ms) }).unwrap();
        }
        h.wait("position", |s| s.position == Duration::from_secs(71)).await;
        tokio::time::timeout(Duration::from_secs(5), async {
            while h.library.snapshot().history.is_empty() {
                tokio::time::sleep(Duration::from_millis(20)).await;
            }
        })
        .await
        .expect("history recorded");
        assert_eq!(h.library.snapshot().history[0].track.video_id, "a");
    }
}
