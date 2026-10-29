//! OS media controls via `souvlaki` (MPRIS on Linux, System Media Transport Controls on
//! Windows): media keys and now-playing metadata.
//!
//! The session lives on the UI thread: Windows needs the app's window handle and the
//! controls must be used from the thread that owns the window. The UI calls
//! [`MediaSession::update`] on every repaint; the player requests one on each change.

use std::ffi::c_void;
use std::time::Duration;

use souvlaki::{MediaControlEvent, MediaControls, MediaMetadata, MediaPlayback, MediaPosition, PlatformConfig, SeekDirection};

use crate::player::{Player, PlayerCmd, PlayerState, Status};

const SEEK_STEP: Duration = Duration::from_secs(10);

pub struct MediaSession {
    controls: MediaControls,
    /// Track and status last sent to the OS.
    last: Option<(Option<String>, Status)>,
}

impl MediaSession {
    /// Register with the OS. `hwnd` is the Win32 window handle (required on Windows,
    /// ignored elsewhere). Returns `None` when the platform service is unavailable
    /// (e.g. no D-Bus session); the app works without media keys.
    pub fn new(player: Player, hwnd: Option<*mut c_void>) -> Option<Self> {
        let config = PlatformConfig { dbus_name: "koi_music", display_name: "koi-music", hwnd };
        let mut controls = match MediaControls::new(config) {
            Ok(c) => c,
            Err(e) => {
                tracing::warn!("media controls unavailable: {e:?}");
                return None;
            }
        };
        if let Err(e) = controls.attach(move |event| on_event(&player, event)) {
            tracing::warn!("cannot attach media controls: {e:?}");
            return None;
        }
        Some(Self { controls, last: None })
    }

    /// Publish metadata and playback status when the track or status changed.
    pub fn update(&mut self, s: &PlayerState) {
        let key = (s.current_track().map(|t| t.video_id.clone()), s.status);
        if self.last.as_ref() == Some(&key) {
            return;
        }
        self.last = Some(key);
        sync(&mut self.controls, s);
    }
}

fn on_event(player: &Player, event: MediaControlEvent) {
    let cmd = match event {
        MediaControlEvent::Play => PlayerCmd::Play,
        MediaControlEvent::Pause => PlayerCmd::Pause,
        MediaControlEvent::Toggle => PlayerCmd::TogglePlay,
        MediaControlEvent::Next => PlayerCmd::Next,
        MediaControlEvent::Previous => PlayerCmd::Previous,
        MediaControlEvent::Stop => PlayerCmd::Pause,
        MediaControlEvent::SetPosition(MediaPosition(p)) => PlayerCmd::Seek(p),
        MediaControlEvent::Seek(dir) | MediaControlEvent::SeekBy(dir, _) => {
            let step = match event {
                MediaControlEvent::SeekBy(_, d) => d,
                _ => SEEK_STEP,
            };
            let pos = player.state().position;
            PlayerCmd::Seek(match dir {
                SeekDirection::Forward => pos + step,
                SeekDirection::Backward => pos.saturating_sub(step),
            })
        }
        MediaControlEvent::SetVolume(v) => PlayerCmd::SetVolume(v as f32),
        _ => return,
    };
    player.send(cmd);
}

fn sync(controls: &mut MediaControls, s: &PlayerState) {
    if let Some(t) = s.current_track() {
        let artist = t.artist_line();
        let meta = MediaMetadata {
            title: Some(&t.title),
            artist: Some(&artist),
            album: t.album.as_ref().map(|a| a.name.as_str()),
            cover_url: t.thumbnail.as_deref(),
            duration: s.duration,
        };
        if let Err(e) = controls.set_metadata(meta) {
            tracing::debug!("set_metadata: {e:?}");
        }
    }
    let progress = Some(MediaPosition(s.position));
    let playback = match s.status {
        Status::Playing | Status::Loading => MediaPlayback::Playing { progress },
        Status::Paused => MediaPlayback::Paused { progress },
        Status::Stopped => MediaPlayback::Stopped,
    };
    if let Err(e) = controls.set_playback(playback) {
        tracing::debug!("set_playback: {e:?}");
    }
}
