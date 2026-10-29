//! yt-dlp integration: stream URL resolution and audio downloads.
//!
//! rustypipe 0.11.4 can only fetch the first ~1 MB of streams (see design D4),
//! so the yt-dlp binary is used for these two operations only.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::sync::{Arc, Mutex, RwLock};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use serde::Deserialize;
use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::process::Command;

use crate::error::{Error, ErrorKind, Result};
use crate::model::{AudioCodec, AudioStream};

/// Format selector: itag 140 (AAC 128k M4A), else best AAC audio.
pub const FORMAT: &str = "140/bestaudio[acodec^=mp4a]";
/// Re-resolve stream URLs this long before they expire.
const EXPIRY_MARGIN: Duration = Duration::from_secs(10 * 60);

pub fn watch_url(video_id: &str) -> String {
    format!("https://music.youtube.com/watch?v={video_id}")
}

/// Handle to the yt-dlp binary. Clones share the configured path.
#[derive(Debug, Clone)]
pub struct YtDlp {
    bin: Arc<RwLock<PathBuf>>,
}

#[derive(Deserialize)]
struct FormatJson {
    url: String,
    format_id: String,
    acodec: Option<String>,
    abr: Option<f64>,
    filesize: Option<u64>,
    filesize_approx: Option<u64>,
    #[serde(default)]
    http_headers: HashMap<String, String>,
}

/// Appended to errors that an outdated yt-dlp commonly causes.
const UPDATE_HINT: &str = "If this keeps happening, update yt-dlp.";

/// Map yt-dlp's stderr to an error, recognizing YouTube's bot check.
fn classify_failure(stderr: &str) -> Error {
    let mut e = classify_failure_kind(stderr);
    if matches!(e.kind, ErrorKind::Unavailable | ErrorKind::BotCheck) {
        e.message = format!("{} {UPDATE_HINT}", e.message);
    }
    e
}

fn classify_failure_kind(stderr: &str) -> Error {
    let line = stderr.lines().rev().find(|l| l.contains("ERROR")).unwrap_or(stderr).trim();
    let lower = line.to_lowercase();
    if lower.contains("not a bot") || lower.contains("sign in to confirm") {
        Error::new(
            ErrorKind::BotCheck,
            "YouTube asks to confirm you're not a bot. Try again later or from another network.",
        )
    } else if lower.contains("unavailable") || lower.contains("requested format is not available") || lower.contains("private video") {
        Error::unavailable(line.trim_start_matches("ERROR: ").to_owned())
    } else if lower.contains("unable to download") || lower.contains("timed out") || lower.contains("getaddrinfo") {
        Error::network(line.to_owned())
    } else {
        Error::unavailable(line.to_owned())
    }
}

/// Result of probing the yt-dlp installation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ToolStatus {
    pub version: String,
    /// JS runtimes yt-dlp found (e.g. `deno-2.8.3`). Empty means YouTube support is degraded.
    pub js_runtimes: Vec<String>,
}

/// Parse the `[debug] JS runtimes: ...` line of `yt-dlp -v` output.
pub(crate) fn parse_js_runtimes(verbose: &str) -> Vec<String> {
    verbose
        .lines()
        .find_map(|l| l.split_once("JS runtimes:").map(|(_, v)| v))
        .map(|v| {
            v.split(',')
                .map(str::trim)
                .filter(|r| !r.is_empty() && *r != "none")
                .map(str::to_owned)
                .collect()
        })
        .unwrap_or_default()
}

/// Parse `koi <downloaded> <total> <estimate>` progress lines.
pub(crate) fn parse_progress(line: &str) -> Option<(u64, Option<u64>)> {
    let mut parts = line.strip_prefix("koi ")?.split_whitespace();
    let done = parts.next()?.parse().ok()?;
    let total = parts.next().and_then(|t| t.parse().ok());
    let estimate = parts.next().and_then(|t| t.parse::<f64>().ok()).map(|e| e as u64);
    Some((done, total.or(estimate)))
}

impl YtDlp {
    pub fn new(bin: impl Into<PathBuf>) -> Self {
        Self { bin: Arc::new(RwLock::new(bin.into())) }
    }

    pub fn bin(&self) -> PathBuf {
        self.bin.read().unwrap().clone()
    }

    /// Use another binary (for all clones of this handle).
    pub fn set_bin(&self, bin: impl Into<PathBuf>) {
        *self.bin.write().unwrap() = bin.into();
    }

    /// A yt-dlp process that ignores the user's yt-dlp config (so our output format and
    /// paths are predictable; plugins such as PO-token providers still load) and, on
    /// Windows, opens no console window.
    fn base_command(&self) -> Command {
        let mut c = Command::new(self.bin());
        c.arg("--ignore-config").stdin(Stdio::null()).kill_on_drop(true);
        #[cfg(windows)]
        {
            /// `CREATE_NO_WINDOW`: a GUI app starting a console program would otherwise flash a console.
            const CREATE_NO_WINDOW: u32 = 0x0800_0000;
            c.creation_flags(CREATE_NO_WINDOW);
        }
        c
    }

    fn command(&self) -> Command {
        let mut c = self.base_command();
        c.args(["--no-playlist", "--no-warnings"]);
        c
    }

    /// `yt-dlp --version`, or `MissingTool` if it cannot be run.
    pub async fn version(&self) -> Result<String> {
        let bin = self.bin();
        let out = self.base_command().arg("--version").output().await.map_err(|e| {
            Error::new(ErrorKind::MissingTool, format!("yt-dlp could not be started ({}): {e}", bin.display()))
        })?;
        if !out.status.success() {
            return Err(Error::new(ErrorKind::MissingTool, "yt-dlp --version failed"));
        }
        Ok(String::from_utf8_lossy(&out.stdout).trim().to_owned())
    }

    /// Version and JS runtimes of the installation, or `MissingTool`.
    pub async fn probe(&self) -> Result<ToolStatus> {
        let version = self.version().await?;
        // Without a URL yt-dlp prints its debug header and exits with an error; no network access.
        let js_runtimes = match self.base_command().arg("-v").output().await {
            Ok(out) => parse_js_runtimes(&String::from_utf8_lossy(&out.stderr)),
            Err(_) => Vec::new(),
        };
        Ok(ToolStatus { version, js_runtimes })
    }

    /// Resolve the audio stream of a video.
    pub async fn resolve(&self, video_id: &str) -> Result<AudioStream> {
        let out = self
            .command()
            .args(["-j", "-f", FORMAT])
            .arg(watch_url(video_id))
            .output()
            .await
            .map_err(|e| Error::new(ErrorKind::MissingTool, format!("yt-dlp could not be started: {e}")))?;
        if !out.status.success() {
            return Err(classify_failure(&String::from_utf8_lossy(&out.stderr)));
        }
        let f: FormatJson = serde_json::from_slice(&out.stdout)
            .map_err(|e| Error::unavailable(format!("unexpected yt-dlp output: {e}")))?;
        let codec = match f.acodec.as_deref() {
            Some(c) if c.starts_with("mp4a") => AudioCodec::Aac,
            Some(c) if c.starts_with("opus") => AudioCodec::Opus,
            _ => AudioCodec::Other,
        };
        Ok(AudioStream {
            itag: f.format_id.parse().unwrap_or_default(),
            codec,
            bitrate: f.abr.map_or(0, |a| (a * 1000.0) as u32),
            size: f.filesize.or(f.filesize_approx).unwrap_or_default(),
            mime: "audio/mp4".into(),
            user_agent: f.http_headers.get("User-Agent").cloned().unwrap_or_default(),
            url: f.url,
        })
    }

    /// Download the audio of a video into `dir` as `<video_id>.<ext>`.
    /// `progress(downloaded, total)` is called for every progress line.
    pub async fn download(
        &self,
        video_id: &str,
        dir: &Path,
        mut progress: impl FnMut(u64, Option<u64>) + Send,
    ) -> Result<PathBuf> {
        tokio::fs::create_dir_all(dir).await?;
        let mut child = self
            .command()
            .args(["-f", FORMAT, "--newline", "--quiet", "--progress", "--no-part", "--no-mtime"])
            .args([
                "--progress-template",
                "download:koi %(progress.downloaded_bytes)s %(progress.total_bytes)s %(progress.total_bytes_estimate)s",
            ])
            .args(["--print", "after_move:filepath", "-o"])
            .arg(dir.join("%(id)s.%(ext)s"))
            .arg(watch_url(video_id))
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .map_err(|e| Error::new(ErrorKind::MissingTool, format!("yt-dlp could not be started: {e}")))?;

        let stderr = child.stderr.take().expect("piped");
        let stderr_task = tokio::spawn(async move {
            let mut buf = String::new();
            let mut lines = BufReader::new(stderr).lines();
            while let Ok(Some(l)) = lines.next_line().await {
                buf.push_str(&l);
                buf.push('\n');
            }
            buf
        });

        let mut final_path = None;
        let mut lines = BufReader::new(child.stdout.take().expect("piped")).lines();
        while let Some(line) = lines.next_line().await? {
            if let Some((done, total)) = parse_progress(&line) {
                progress(done, total);
            } else if !line.trim().is_empty() {
                final_path = Some(PathBuf::from(line.trim()));
            }
        }
        let status = child.wait().await?;
        let stderr = stderr_task.await.unwrap_or_default();
        if !status.success() {
            return Err(classify_failure(&stderr));
        }
        final_path
            .filter(|p| p.is_file())
            .ok_or_else(|| Error::new(ErrorKind::Io, "yt-dlp did not report the downloaded file"))
    }
}

/// Unix timestamp from a googlevideo URL's `expire` parameter.
fn url_expiry(url: &str) -> Option<u64> {
    let query = url.split_once('?')?.1;
    query.split('&').find_map(|kv| kv.strip_prefix("expire=")).and_then(|v| v.parse().ok())
}

fn unix_now() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or_default()
}

/// Caches resolved streams per video id until shortly before they expire.
#[derive(Default)]
pub struct StreamCache {
    entries: Mutex<HashMap<String, (AudioStream, u64)>>,
}

impl StreamCache {
    pub fn get(&self, video_id: &str) -> Option<AudioStream> {
        let mut map = self.entries.lock().unwrap();
        match map.get(video_id) {
            Some((s, valid_until)) if *valid_until > unix_now() => Some(s.clone()),
            Some(_) => {
                map.remove(video_id);
                None
            }
            None => None,
        }
    }

    pub fn insert(&self, video_id: &str, stream: AudioStream) {
        // Unknown expiry: assume a conservative hour.
        let expires = url_expiry(&stream.url).unwrap_or_else(|| unix_now() + 3600);
        let valid_until = expires.saturating_sub(EXPIRY_MARGIN.as_secs());
        self.entries.lock().unwrap().insert(video_id.to_owned(), (stream, valid_until));
    }

    pub fn invalidate(&self, video_id: &str) {
        self.entries.lock().unwrap().remove(video_id);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn stream(url: String) -> AudioStream {
        AudioStream { url, itag: 140, codec: AudioCodec::Aac, bitrate: 0, size: 0, mime: String::new(), user_agent: String::new() }
    }

    #[test]
    fn progress_lines() {
        assert_eq!(parse_progress("koi 1024 3449447 NA"), Some((1024, Some(3449447))));
        assert_eq!(parse_progress("koi 1024 NA 5000.0"), Some((1024, Some(5000))));
        assert_eq!(parse_progress("koi 1024 NA NA"), Some((1024, None)));
        assert_eq!(parse_progress("/tmp/x.m4a"), None);
    }

    #[test]
    fn bot_check_is_recognized() {
        let e = classify_failure("ERROR: [youtube] abc: Sign in to confirm you're not a bot. Use --cookies");
        assert_eq!(e.kind, ErrorKind::BotCheck);
        assert!(e.message.ends_with(UPDATE_HINT));
        let e = classify_failure("ERROR: [youtube] abc: Video unavailable");
        assert_eq!(e.kind, ErrorKind::Unavailable);
        assert!(e.message.ends_with(UPDATE_HINT));
        let e = classify_failure("ERROR: Unable to download webpage: timed out");
        assert_eq!(e.kind, ErrorKind::Network);
        assert!(!e.message.contains(UPDATE_HINT));
    }

    #[test]
    fn js_runtimes_line() {
        let out = "[debug] exe versions: ffmpeg 8.1.2\n[debug] JS runtimes: deno-2.8.3, node-24.1.0\n";
        assert_eq!(parse_js_runtimes(out), ["deno-2.8.3", "node-24.1.0"]);
        assert!(parse_js_runtimes("[debug] JS runtimes: none\n").is_empty());
        assert!(parse_js_runtimes("no such line").is_empty());
    }

    #[tokio::test]
    #[ignore = "needs yt-dlp"]
    async fn probes_installation() {
        let s = YtDlp::new("yt-dlp").probe().await.unwrap();
        assert!(!s.version.is_empty());
        eprintln!("{s:?}");
    }

    #[test]
    fn cache_respects_expiry() {
        let cache = StreamCache::default();
        let fresh = format!("https://x.googlevideo.com/videoplayback?expire={}&itag=140", unix_now() + 6 * 3600);
        cache.insert("a", stream(fresh.clone()));
        assert_eq!(cache.get("a").map(|s| s.url), Some(fresh));
        let stale = format!("https://x.googlevideo.com/videoplayback?expire={}", unix_now() + 60);
        cache.insert("b", stream(stale));
        assert!(cache.get("b").is_none(), "within margin counts as expired");
        cache.invalidate("a");
        assert!(cache.get("a").is_none());
    }

    #[tokio::test]
    #[ignore = "network"]
    async fn resolves_and_downloads() {
        let y = YtDlp::new("yt-dlp");
        assert!(!y.version().await.unwrap().is_empty());
        let s = y.resolve(crate::ytm::fake::CC_VIDEO_ID).await.unwrap();
        assert_eq!(s.codec, AudioCodec::Aac);
        assert!(s.size > 0 && !s.user_agent.is_empty());
        let dir = tempfile::tempdir().unwrap();
        let mut last = 0;
        let path = y.download(crate::ytm::fake::CC_VIDEO_ID, dir.path(), |d, _| last = d).await.unwrap();
        assert!(path.is_file());
        // The ffmpeg container fixup (when ffmpeg is installed) changes the size slightly.
        let size = std::fs::metadata(&path).unwrap().len();
        assert!(last > 0 && size.abs_diff(last) < last / 100, "size {size}, downloaded {last}");
    }

    #[tokio::test]
    async fn missing_binary() {
        let y = YtDlp::new("/nonexistent/yt-dlp");
        assert_eq!(y.version().await.unwrap_err().kind, ErrorKind::MissingTool);
    }
}
