//! OS media controls (MPRIS on Linux) via `souvlaki`: media keys and now-playing metadata.

use std::sync::Arc;
use std::time::Duration;

use souvlaki::{MediaControlEvent, MediaControls, MediaMetadata, MediaPlayback, MediaPosition, PlatformConfig, SeekDirection};

use crate::player::{Player, PlayerCmd, PlayerState, Status};

const SEEK_STEP: Duration = Duration::from_secs(10);

/// Register with the OS and keep it in sync with the player until the runtime shuts down.
/// Failures (e.g. no D-Bus session) are logged; the app works without media keys.
pub fn start(player: Player, rt: &tokio::runtime::Handle) {
    let config = PlatformConfig { dbus_name: "koi_music", display_name: "koi-music", hwnd: None };
    let mut controls = match MediaControls::new(config) {
        Ok(c) => c,
        Err(e) => {
            tracing::warn!("media controls unavailable: {e:?}");
            return;
        }
    };
    let handler_player = player.clone();
    if let Err(e) = controls.attach(move |event| on_event(&handler_player, event)) {
        tracing::warn!("cannot attach media controls: {e:?}");
        return;
    }
    let mut state = player.subscribe();
    rt.spawn(async move {
        let mut last: Option<(Option<String>, Status)> = None;
        loop {
            let snap = state.borrow_and_update().clone();
            let key = (snap.current_track().map(|t| t.video_id.clone()), snap.status);
            if last.as_ref() != Some(&key) {
                sync(&mut controls, &snap);
                last = Some(key);
            }
            if state.changed().await.is_err() {
                break;
            }
        }
    });
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

fn sync(controls: &mut MediaControls, s: &Arc<PlayerState>) {
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
