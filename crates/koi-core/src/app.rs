//! `AppCore`: wires all core services together for the UI (design D3).

use std::path::PathBuf;
use std::sync::{Arc, RwLock};

use tokio::sync::mpsc::UnboundedSender;

use crate::audio::{AudioEvent, AudioInput, AudioOutput, LoadId, RodioOutput};
use crate::dirs::AppDirs;
use crate::downloads::{DEFAULT_CONCURRENCY, Manager, YtDlpBackend};
use crate::error::{Error, Result};
use crate::library::{Library, keys};
use crate::notify::Notifier;
use crate::player::Player;
use crate::ytdlp::{ToolStatus, YtDlp};
use crate::ytm::{HybridSource, MusicSource, RustyPipeSource};

/// Result of probing yt-dlp.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ToolProbe {
    Pending,
    Ready(ToolStatus),
    /// yt-dlp could not be run; streaming and downloads are disabled.
    Missing(String),
}

impl ToolProbe {
    /// Streaming non-downloaded tracks and downloading need yt-dlp.
    pub fn is_usable(&self) -> bool {
        !matches!(self, Self::Missing(_))
    }
}

pub struct AppCore {
    pub rt: tokio::runtime::Handle,
    pub dirs: AppDirs,
    pub library: Arc<Library>,
    pub source: Arc<dyn MusicSource>,
    pub player: Player,
    pub downloads: Manager,
    pub ytdlp: YtDlp,
    /// Why audio output is unavailable, if it is.
    pub audio_error: Option<String>,
    tool: Arc<RwLock<ToolProbe>>,
    notifier: Notifier,
}

impl AppCore {
    /// Open the library, verify downloads, and start all services on `rt`.
    pub fn start(rt: tokio::runtime::Handle, dirs: AppDirs, notifier: Notifier) -> Result<Self> {
        let _guard = rt.enter();
        dirs.create_all()?;
        let library = Arc::new(Library::open(&dirs.database())?);
        library.set_notifier(notifier.clone());
        let cleared = library.verify_downloads()?;
        if cleared > 0 {
            tracing::info!("cleared {cleared} downloads whose files were removed");
        }

        let ytdlp_path: PathBuf = library.setting(keys::YTDLP_PATH)?.unwrap_or_else(|| "yt-dlp".into());
        let ytdlp = YtDlp::new(ytdlp_path);
        let source: Arc<dyn MusicSource> =
            Arc::new(HybridSource::new(RustyPipeSource::new(&dirs.rustypipe_storage())?, ytdlp.clone()));

        let (events_tx, events_rx) = tokio::sync::mpsc::unbounded_channel();
        let (audio, audio_error): (Arc<dyn AudioOutput>, _) = match RodioOutput::start(events_tx.clone(), rt.clone()) {
            Ok(out) => (Arc::new(out), None),
            Err(e) => {
                tracing::error!("{e}");
                let msg = e.to_string();
                (Arc::new(NullOutput { events: events_tx, reason: msg.clone() }), Some(msg))
            }
        };
        let player = Player::start(source.clone(), library.clone(), audio, events_rx, notifier.clone());

        let dir: PathBuf = library.setting(keys::DOWNLOAD_DIR)?.unwrap_or_else(|| dirs.default_downloads.clone());
        let concurrency = library.setting(keys::DOWNLOAD_CONCURRENCY)?.unwrap_or(DEFAULT_CONCURRENCY);
        let backend = Arc::new(YtDlpBackend::new(ytdlp.clone()));
        let downloads = Manager::new(backend, library.clone(), dir, concurrency, notifier.clone());

        let core = Self {
            rt,
            dirs,
            library,
            source,
            player,
            downloads,
            ytdlp,
            audio_error,
            tool: Arc::new(RwLock::new(ToolProbe::Pending)),
            notifier,
        };
        core.probe_tool();
        Ok(core)
    }

    pub fn tool(&self) -> ToolProbe {
        self.tool.read().unwrap().clone()
    }

    /// Probe yt-dlp in the background and publish the result.
    pub fn probe_tool(&self) {
        *self.tool.write().unwrap() = ToolProbe::Pending;
        let (ytdlp, tool, notifier) = (self.ytdlp.clone(), self.tool.clone(), self.notifier.clone());
        self.rt.spawn(async move {
            let probe = match ytdlp.probe().await {
                Ok(s) => {
                    if s.js_runtimes.is_empty() {
                        tracing::warn!("yt-dlp found no JavaScript runtime; YouTube support is degraded");
                    }
                    ToolProbe::Ready(s)
                }
                Err(e) => ToolProbe::Missing(e.to_string()),
            };
            *tool.write().unwrap() = probe;
            notifier.notify();
        });
    }

    /// Change the yt-dlp binary (persisted) and probe it again.
    pub fn set_ytdlp_path(&self, path: PathBuf) -> Result<()> {
        self.library.set_setting(keys::YTDLP_PATH, &path)?;
        self.ytdlp.set_bin(path);
        self.probe_tool();
        Ok(())
    }

    pub fn set_download_dir(&self, dir: PathBuf) -> Result<()> {
        self.library.set_setting(keys::DOWNLOAD_DIR, &dir)?;
        self.downloads.set_dir(dir);
        Ok(())
    }

    pub fn set_download_concurrency(&self, n: usize) -> Result<()> {
        let n = n.clamp(1, 10);
        self.library.set_setting(keys::DOWNLOAD_CONCURRENCY, &n)?;
        let _guard = self.rt.enter();
        self.downloads.set_concurrency(n);
        Ok(())
    }
}

/// Stand-in when no audio device can be opened: every load fails with the reason.
struct NullOutput {
    events: UnboundedSender<AudioEvent>,
    reason: String,
}

impl AudioOutput for NullOutput {
    fn load(&self, load: LoadId, _input: AudioInput, _paused: bool) {
        let error = Error::audio(self.reason.clone());
        let _ = self.events.send(AudioEvent::Failed { load, error, stream_rejected: false });
    }
    fn play(&self) {}
    fn pause(&self) {}
    fn seek(&self, _position: std::time::Duration) {}
    fn set_volume(&self, _volume: f32) {}
    fn stop(&self) {}
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ytm::fake::track;

    #[tokio::test(flavor = "multi_thread")]
    async fn startup_clears_downloads_with_missing_files() {
        let base = tempfile::tempdir().unwrap();
        let dirs = AppDirs::in_dir(base.path());
        dirs.create_all().unwrap();
        let kept = base.path().join("kept.m4a");
        std::fs::write(&kept, b"x").unwrap();
        {
            let lib = Library::open(&dirs.database()).unwrap();
            lib.add_download(&track("kept"), &kept, 1).unwrap();
            lib.add_download(&track("gone"), &base.path().join("gone.m4a"), 1).unwrap();
        }
        let core = AppCore::start(tokio::runtime::Handle::current(), dirs, Notifier::noop()).unwrap();
        let snap = core.library.snapshot();
        assert!(snap.is_downloaded("kept"));
        assert!(!snap.is_downloaded("gone"));
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn missing_ytdlp_is_reported() {
        let base = tempfile::tempdir().unwrap();
        let core = AppCore::start(tokio::runtime::Handle::current(), AppDirs::in_dir(base.path()), Notifier::noop()).unwrap();
        core.set_ytdlp_path("/nonexistent/yt-dlp".into()).unwrap();
        tokio::time::timeout(std::time::Duration::from_secs(5), async {
            while core.tool() == ToolProbe::Pending {
                tokio::time::sleep(std::time::Duration::from_millis(10)).await;
            }
        })
        .await
        .unwrap();
        assert!(!core.tool().is_usable());
    }
}
