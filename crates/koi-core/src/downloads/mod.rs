//! Download manager: a queue of track downloads with limited concurrency.
//!
//! Each job fetches the audio into `<download_dir>/.koi-music-tmp/<run>/` via a
//! [`DownloadBackend`], tags it, then moves it to `<download_dir>/<Artist> - <Title>.<ext>`
//! and records it in the [`Library`]. The visible folder never holds partial files.

mod tag;

use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex, RwLock};
use std::time::{Duration, Instant};

use tokio::sync::{Semaphore, watch};
use tokio::task::JoinHandle;

pub use tag::{cover_url, write_tags};

use crate::error::{Error, ErrorKind, Result};
use crate::fsname::{track_stem, unique_path};
use crate::library::Library;
use crate::model::Track;
use crate::notify::Notifier;
use crate::ytdlp::YtDlp;
use crate::ytm::BoxFut;

/// Default number of simultaneous downloads.
pub const DEFAULT_CONCURRENCY: usize = 3;
/// Hidden folder inside the download dir for in-progress files.
pub const TEMP_DIR_NAME: &str = ".koi-music-tmp";
/// Progress updates are published at most this often.
const PROGRESS_INTERVAL: Duration = Duration::from_millis(250);

pub type ProgressFn = Box<dyn FnMut(u64, Option<u64>) + Send>;

/// Fetches the audio of a track into a directory.
pub trait DownloadBackend: Send + Sync + 'static {
    /// Download into `temp_dir` and return the file path. `progress(done, total)` reports bytes.
    fn fetch<'a>(&'a self, video_id: &'a str, temp_dir: &'a Path, progress: ProgressFn) -> BoxFut<'a, PathBuf>;
}

/// Downloads with the yt-dlp binary.
pub struct YtDlpBackend {
    ytdlp: YtDlp,
}

impl YtDlpBackend {
    pub fn new(ytdlp: YtDlp) -> Self {
        Self { ytdlp }
    }
}

impl DownloadBackend for YtDlpBackend {
    fn fetch<'a>(&'a self, video_id: &'a str, temp_dir: &'a Path, progress: ProgressFn) -> BoxFut<'a, PathBuf> {
        Box::pin(self.ytdlp.download(video_id, temp_dir, progress))
    }
}

pub type JobId = u64;

#[derive(Debug, Clone, PartialEq)]
pub enum JobState {
    Queued,
    Downloading { done: u64, total: Option<u64> },
    Failed(String),
    Cancelled,
}

impl JobState {
    pub fn is_active(&self) -> bool {
        matches!(self, Self::Queued | Self::Downloading { .. })
    }

    /// Progress 0.0..=1.0 if known.
    pub fn fraction(&self) -> Option<f32> {
        match self {
            Self::Downloading { done, total: Some(t) } if *t > 0 => Some((*done as f32 / *t as f32).min(1.0)),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct Job {
    pub id: JobId,
    pub track: Track,
    pub state: JobState,
}

/// Snapshot of all jobs, oldest first.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct DownloadsState {
    pub jobs: Vec<Job>,
    /// Latest notice for the user (e.g. downloads stopped by a bot check); `seq` grows per notice.
    pub notice: Option<DownloadNotice>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DownloadNotice {
    pub seq: u64,
    pub message: String,
}

struct Entry {
    job: Job,
    /// Token of the current attempt; results of older (cancelled) attempts are ignored.
    run: u64,
    handle: Option<JoinHandle<()>>,
}

struct Inner {
    backend: Arc<dyn DownloadBackend>,
    library: Arc<Library>,
    http: reqwest::Client,
    notifier: Notifier,
    dir: RwLock<PathBuf>,
    semaphore: Arc<Semaphore>,
    concurrency: Mutex<usize>,
    entries: Mutex<Vec<Entry>>,
    state_tx: watch::Sender<Arc<DownloadsState>>,
    next_id: AtomicU64,
    last_progress: Mutex<Instant>,
    /// A delayed publish is scheduled (so throttled progress is not lost).
    flush_pending: AtomicBool,
    notice: Mutex<Option<DownloadNotice>>,
}

/// Handle to the download manager. Cheap to clone; must be used inside a tokio runtime.
#[derive(Clone)]
pub struct Manager {
    inner: Arc<Inner>,
}

impl Manager {
    pub fn new(
        backend: Arc<dyn DownloadBackend>,
        library: Arc<Library>,
        dir: PathBuf,
        concurrency: usize,
        notifier: Notifier,
    ) -> Self {
        // Leftovers of a previous run (e.g. the app was killed mid-download).
        let _ = std::fs::remove_dir_all(dir.join(TEMP_DIR_NAME));
        let concurrency = concurrency.max(1);
        let (state_tx, _) = watch::channel(Arc::default());
        Self {
            inner: Arc::new(Inner {
                backend,
                library,
                http: reqwest::Client::new(),
                notifier,
                dir: RwLock::new(dir),
                semaphore: Arc::new(Semaphore::new(concurrency)),
                concurrency: Mutex::new(concurrency),
                entries: Mutex::default(),
                state_tx,
                next_id: AtomicU64::new(1),
                last_progress: Mutex::new(Instant::now()),
                flush_pending: AtomicBool::new(false),
                notice: Mutex::new(None),
            }),
        }
    }

    pub fn state(&self) -> Arc<DownloadsState> {
        self.inner.state_tx.borrow().clone()
    }

    pub fn subscribe(&self) -> watch::Receiver<Arc<DownloadsState>> {
        self.inner.state_tx.subscribe()
    }

    pub fn dir(&self) -> PathBuf {
        self.inner.dir.read().unwrap().clone()
    }

    /// Change the folder for new downloads (existing files stay where they are).
    pub fn set_dir(&self, dir: PathBuf) {
        *self.inner.dir.write().unwrap() = dir;
    }

    pub fn concurrency(&self) -> usize {
        *self.inner.concurrency.lock().unwrap()
    }

    /// Change how many downloads run at once. Lowering it takes effect as running jobs finish.
    pub fn set_concurrency(&self, n: usize) {
        let n = n.max(1);
        let mut cur = self.inner.concurrency.lock().unwrap();
        if n > *cur {
            self.inner.semaphore.add_permits(n - *cur);
        } else if n < *cur {
            let sem = self.inner.semaphore.clone();
            let remove = (*cur - n) as u32;
            tokio::spawn(async move {
                if let Ok(p) = sem.acquire_many_owned(remove).await {
                    p.forget();
                }
            });
        }
        *cur = n;
    }

    /// Queue a download unless the track is already downloaded or being downloaded.
    pub fn download(&self, track: Track) -> Option<JobId> {
        if self.inner.library.snapshot().is_downloaded(&track.video_id) {
            return None;
        }
        let mut entries = self.inner.entries.lock().unwrap();
        if entries.iter().any(|e| e.job.track.video_id == track.video_id && e.job.state.is_active()) {
            return None;
        }
        // Replace a finished/failed job for the same track instead of listing it twice.
        entries.retain(|e| e.job.track.video_id != track.video_id);
        let id = self.inner.next_id.fetch_add(1, Ordering::Relaxed);
        entries.push(Entry { job: Job { id, track, state: JobState::Queued }, run: 0, handle: None });
        Inner::start(&self.inner, &mut entries, id);
        drop(entries);
        self.inner.publish();
        Some(id)
    }

    /// Queue all tracks that are not downloaded yet. Returns how many were queued.
    pub fn download_all(&self, tracks: impl IntoIterator<Item = Track>) -> usize {
        let mut seen = HashSet::new();
        tracks.into_iter().filter(|t| seen.insert(t.video_id.clone())).filter_map(|t| self.download(t)).count()
    }

    /// Cancel a queued or running download and remove its temporary files.
    pub fn cancel(&self, id: JobId) {
        let mut entries = self.inner.entries.lock().unwrap();
        let Some(e) = entries.iter_mut().find(|e| e.job.id == id && e.job.state.is_active()) else { return };
        e.job.state = JobState::Cancelled;
        let run = e.run;
        e.run = 0; // ignore anything the aborted attempt still reports
        if let Some(handle) = e.handle.take() {
            handle.abort();
            let temp = self.inner.temp_dir(run);
            tokio::spawn(async move {
                let _ = handle.await; // the yt-dlp child is killed when the task is dropped
                remove_temp(&temp).await;
            });
        }
        drop(entries);
        self.inner.publish();
    }

    /// Restart a failed or cancelled download.
    pub fn retry(&self, id: JobId) {
        let mut entries = self.inner.entries.lock().unwrap();
        let Some(e) = entries.iter_mut().find(|e| e.job.id == id) else { return };
        if !matches!(e.job.state, JobState::Failed(_) | JobState::Cancelled) {
            return;
        }
        e.job.state = JobState::Queued;
        Inner::start(&self.inner, &mut entries, id);
        drop(entries);
        self.inner.publish();
    }

    /// Delete a downloaded file and forget it. The track stays in playlists (and streams again).
    pub async fn delete(&self, video_id: &str) -> Result<()> {
        if let Some(d) = self.inner.library.snapshot().downloads.get(video_id) {
            match tokio::fs::remove_file(&d.path).await {
                Ok(()) => {}
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
                Err(e) => return Err(e.into()),
            }
        }
        let lib = self.inner.library.clone();
        let id = video_id.to_owned();
        tokio::task::spawn_blocking(move || lib.remove_download(&id)).await.map_err(join_error)?
    }
}

fn join_error(e: tokio::task::JoinError) -> Error {
    Error::new(ErrorKind::Io, e.to_string())
}

async fn remove_temp(temp: &Path) {
    let _ = tokio::fs::remove_dir_all(temp).await;
    if let Some(root) = temp.parent() {
        let _ = tokio::fs::remove_dir(root).await; // only succeeds when empty
    }
}

impl Inner {
    fn temp_dir(&self, run: u64) -> PathBuf {
        self.dir.read().unwrap().join(TEMP_DIR_NAME).join(run.to_string())
    }

    /// Spawn a new attempt for job `id`. Caller holds the entries lock.
    fn start(this: &Arc<Self>, entries: &mut [Entry], id: JobId) {
        let run = this.next_id.fetch_add(1, Ordering::Relaxed);
        let e = entries.iter_mut().find(|e| e.job.id == id).expect("job exists");
        e.run = run;
        let track = e.job.track.clone();
        let inner = this.clone();
        e.handle = Some(tokio::spawn(async move {
            let Ok(_permit) = inner.semaphore.clone().acquire_owned().await else { return };
            if !inner.set_state(id, run, JobState::Downloading { done: 0, total: None }) {
                return;
            }
            let temp = inner.temp_dir(run);
            let result = inner.process(id, run, &track, &temp).await;
            remove_temp(&temp).await;
            let e = match result {
                // The track is in the library now and listed with the downloaded tracks.
                Ok(()) => return inner.remove(id, run),
                Err(e) => e,
            };
            tracing::warn!("download of {} failed: {e}", track.video_id);
            let bot_check = e.kind == ErrorKind::BotCheck;
            // Only the first job to hit a bot check gets here; the others are aborted below.
            if inner.set_state(id, run, JobState::Failed(e.to_string())) && bot_check {
                inner.stop_all(&e.to_string());
            }
        }));
    }

    /// YouTube blocks this network for now (bot check): stop every queued and running
    /// download instead of letting each one hit the block, and tell the user once.
    /// Nothing resumes automatically; stopped jobs can be retried.
    fn stop_all(&self, message: &str) {
        {
            let mut entries = self.entries.lock().unwrap();
            for e in entries.iter_mut().filter(|e| e.job.state.is_active()) {
                e.job.state = JobState::Failed(message.to_owned());
                let run = std::mem::replace(&mut e.run, 0); // ignore anything the aborted attempt reports
                if let Some(handle) = e.handle.take() {
                    handle.abort();
                    let temp = self.temp_dir(run);
                    tokio::spawn(async move {
                        let _ = handle.await; // the yt-dlp child is killed when the task is dropped
                        remove_temp(&temp).await;
                    });
                }
            }
            let mut notice = self.notice.lock().unwrap();
            let seq = notice.as_ref().map_or(1, |n| n.seq + 1);
            *notice = Some(DownloadNotice { seq, message: format!("Downloads stopped. {message}") });
        }
        self.publish();
    }

    async fn process(self: &Arc<Self>, id: JobId, run: u64, track: &Track, temp: &Path) -> Result<()> {
        tokio::fs::create_dir_all(temp).await?;
        let progress_inner = self.clone();
        let progress: ProgressFn = Box::new(move |done, total| progress_inner.progress(id, run, done, total));
        let file = self.backend.fetch(&track.video_id, temp, progress).await?;

        let cover = match &track.thumbnail {
            Some(url) => self.fetch_cover(&cover_url(url)).await,
            None => None,
        };
        let (path, t) = (file.clone(), track.clone());
        let tagged = tokio::task::spawn_blocking(move || write_tags(&path, &t, cover.as_deref())).await.map_err(join_error)?;
        if let Err(e) = tagged {
            tracing::warn!("{e}"); // the file is still playable
        }

        let dir = self.dir.read().unwrap().clone();
        let ext = file.extension().and_then(|e| e.to_str()).unwrap_or("m4a").to_owned();
        let target = unique_path(&dir, &track_stem(&track.artist_line(), &track.title), &ext);
        tokio::fs::rename(&file, &target).await?;
        let size = tokio::fs::metadata(&target).await.map(|m| m.len()).unwrap_or_default();

        let (lib, t, p) = (self.library.clone(), track.clone(), target.clone());
        let recorded = tokio::task::spawn_blocking(move || lib.add_download(&t, &p, size)).await.map_err(join_error)?;
        if recorded.is_err() {
            let _ = tokio::fs::remove_file(&target).await;
        }
        recorded
    }

    async fn fetch_cover(&self, url: &str) -> Option<Vec<u8>> {
        let res = self.http.get(url).send().await.and_then(|r| r.error_for_status());
        match res {
            Ok(r) => r.bytes().await.ok().map(|b| b.to_vec()),
            Err(e) => {
                tracing::warn!("cannot fetch cover {url}: {e}");
                None
            }
        }
    }

    /// Update a job if `run` is still its current attempt. Returns false if superseded.
    fn set_state(&self, id: JobId, run: u64, state: JobState) -> bool {
        let mut entries = self.entries.lock().unwrap();
        let Some(e) = entries.iter_mut().find(|e| e.job.id == id && e.run == run) else { return false };
        e.job.state = state;
        if !e.job.state.is_active() {
            e.handle = None;
        }
        drop(entries);
        self.publish();
        true
    }

    /// Drop a finished job from the list (if this attempt is still the current one).
    fn remove(&self, id: JobId, run: u64) {
        let mut entries = self.entries.lock().unwrap();
        let before = entries.len();
        entries.retain(|e| !(e.job.id == id && e.run == run));
        let removed = entries.len() != before;
        drop(entries);
        if removed {
            self.publish();
        }
    }

    fn progress(self: &Arc<Self>, id: JobId, run: u64, done: u64, total: Option<u64>) {
        {
            let mut entries = self.entries.lock().unwrap();
            let Some(e) = entries.iter_mut().find(|e| e.job.id == id && e.run == run) else { return };
            e.job.state = JobState::Downloading { done, total };
        }
        let mut last = self.last_progress.lock().unwrap();
        let elapsed = last.elapsed();
        if elapsed >= PROGRESS_INTERVAL {
            *last = Instant::now();
            drop(last);
            self.publish();
        } else if !self.flush_pending.swap(true, Ordering::AcqRel) {
            // Publish the latest progress once the interval is over.
            let inner = self.clone();
            tokio::spawn(async move {
                tokio::time::sleep(PROGRESS_INTERVAL - elapsed).await;
                inner.flush_pending.store(false, Ordering::Release);
                *inner.last_progress.lock().unwrap() = Instant::now();
                inner.publish();
            });
        }
    }

    fn publish(&self) {
        {
            // Send under the lock so concurrent publishes cannot arrive out of order.
            let entries = self.entries.lock().unwrap();
            let jobs = entries.iter().map(|e| e.job.clone()).collect();
            let notice = self.notice.lock().unwrap().clone();
            self.state_tx.send_replace(Arc::new(DownloadsState { jobs, notice }));
        }
        self.notifier.notify();
    }
}

/// [`DownloadBackend`] for tests: copies the M4A fixture in two halves.
#[cfg(test)]
pub(crate) mod fake {
    use super::*;
    use std::collections::HashMap;
    use tokio::sync::Notify;

    #[derive(Default)]
    pub struct FakeBackend {
        /// Video ids whose download stops at 50% until `release` is notified.
        pub hold: Mutex<HashSet<String>>,
        pub release: Notify,
        /// Video ids that fail with a network error (once each).
        pub fail_once: Mutex<HashSet<String>>,
        /// Video ids that fail with YouTube's bot check at 50%, once `bot_check_now` is notified.
        pub bot_check: Mutex<HashSet<String>>,
        pub bot_check_now: Notify,
        pub calls: Mutex<HashMap<String, usize>>,
    }

    impl DownloadBackend for FakeBackend {
        fn fetch<'a>(&'a self, video_id: &'a str, temp_dir: &'a Path, mut progress: ProgressFn) -> BoxFut<'a, PathBuf> {
            Box::pin(async move {
                *self.calls.lock().unwrap().entry(video_id.to_owned()).or_default() += 1;
                let data = std::fs::read(tag::tests::fixture("tone-5s.m4a"))?;
                let total = data.len() as u64;
                let path = temp_dir.join(format!("{video_id}.m4a"));
                let half = data.len() / 2;
                tokio::fs::write(&path, &data[..half]).await?;
                progress(half as u64, Some(total));
                if self.fail_once.lock().unwrap().remove(video_id) {
                    return Err(Error::network("connection reset"));
                }
                let bot_check = self.bot_check.lock().unwrap().contains(video_id);
                if bot_check {
                    self.bot_check_now.notified().await;
                    return Err(Error::new(ErrorKind::BotCheck, crate::ytdlp::BOT_CHECK_MESSAGE));
                }
                let held = self.hold.lock().unwrap().contains(video_id);
                if held {
                    self.release.notified().await;
                }
                tokio::fs::write(&path, &data).await?;
                progress(total, Some(total));
                Ok(path)
            })
        }
    }
}

#[cfg(test)]
mod tests {
    use super::fake::FakeBackend;
    use super::*;
    use crate::ytm::fake::track;

    struct H {
        mgr: Manager,
        backend: Arc<FakeBackend>,
        lib: Arc<Library>,
        dir: tempfile::TempDir,
        state: watch::Receiver<Arc<DownloadsState>>,
    }

    fn harness(concurrency: usize) -> H {
        let dir = tempfile::tempdir().unwrap();
        let backend = Arc::new(FakeBackend::default());
        let lib = Arc::new(Library::open_in_memory().unwrap());
        let mgr = Manager::new(backend.clone(), lib.clone(), dir.path().to_path_buf(), concurrency, Notifier::noop());
        let state = mgr.subscribe();
        H { mgr, backend, lib, dir, state }
    }

    impl H {
        async fn wait(&mut self, what: &str, pred: impl Fn(&DownloadsState) -> bool) -> Arc<DownloadsState> {
            match tokio::time::timeout(Duration::from_secs(5), self.state.wait_for(|s| pred(s))).await {
                Ok(s) => s.unwrap().clone(),
                Err(_) => panic!("timed out waiting for {what}: {:?}", self.mgr.state()),
            }
        }

        fn hold(&self, ids: &[&str]) {
            self.backend.hold.lock().unwrap().extend(ids.iter().map(|s| s.to_string()));
        }

        /// All files below the download dir, relative.
        fn files(&self) -> Vec<String> {
            fn walk(dir: &Path, base: &Path, out: &mut Vec<String>) {
                // Folders can disappear while cleanup runs.
                let Ok(entries) = std::fs::read_dir(dir) else { return };
                for e in entries.flatten() {
                    let p = e.path();
                    if p.is_dir() {
                        walk(&p, base, out);
                    } else {
                        out.push(p.strip_prefix(base).unwrap().display().to_string());
                    }
                }
            }
            let mut out = Vec::new();
            walk(self.dir.path(), self.dir.path(), &mut out);
            out.sort();
            out
        }
    }

    fn state_of(s: &DownloadsState, id: JobId) -> JobState {
        s.jobs.iter().find(|j| j.id == id).unwrap().state.clone()
    }

    /// A successful download leaves the list.
    fn finished(s: &DownloadsState, id: JobId) -> bool {
        !s.jobs.iter().any(|j| j.id == id)
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn downloads_tags_and_records() {
        let mut h = harness(3);
        let mut t = track("a");
        t.thumbnail = None;
        let id = h.mgr.download(t).unwrap();
        h.wait("completed", |s| finished(s, id)).await;
        assert_eq!(h.files(), ["Artist - Title a.m4a"]);
        let snap = h.lib.snapshot();
        let d = &snap.downloads["a"];
        assert_eq!(d.path, h.dir.path().join("Artist - Title a.m4a"));
        let file = lofty::read_from_path(&d.path).unwrap();
        use lofty::prelude::*;
        assert_eq!(file.primary_tag().unwrap().title().as_deref(), Some("Title a"));
        // Already downloaded: not queued again.
        assert_eq!(h.mgr.download(track("a")), None);
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn name_collision_gets_suffix() {
        let mut h = harness(3);
        std::fs::write(h.dir.path().join("Artist - Title a.m4a"), b"other track").unwrap();
        let mut t = track("a");
        t.thumbnail = None;
        let id = h.mgr.download(t).unwrap();
        h.wait("completed", |s| finished(s, id)).await;
        assert_eq!(h.files(), ["Artist - Title a (2).m4a", "Artist - Title a.m4a"]);
        assert_eq!(std::fs::read(h.dir.path().join("Artist - Title a.m4a")).unwrap(), b"other track");
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn cancel_at_half_leaves_no_files() {
        let mut h = harness(3);
        h.hold(&["a"]);
        let id = h.mgr.download(track("a")).unwrap();
        let s = h.wait("50%", |s| state_of(s, id).fraction().is_some_and(|f| (0.45..0.55).contains(&f))).await;
        assert!(matches!(state_of(&s, id), JobState::Downloading { .. }));
        assert!(!h.files().is_empty(), "partial file in temp dir");
        h.mgr.cancel(id);
        assert_eq!(state_of(&h.mgr.state(), id), JobState::Cancelled);
        tokio::time::timeout(Duration::from_secs(5), async {
            while !h.files().is_empty() || h.dir.path().join(TEMP_DIR_NAME).exists() {
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
        })
        .await
        .expect("temp files removed");
        // A late release must not resurrect the job.
        h.backend.release.notify_waiters();
        tokio::time::sleep(Duration::from_millis(100)).await;
        assert_eq!(state_of(&h.mgr.state(), id), JobState::Cancelled);
        assert!(h.files().is_empty());
        assert!(!h.lib.snapshot().is_downloaded("a"));
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn failed_download_can_be_retried() {
        let mut h = harness(3);
        h.backend.fail_once.lock().unwrap().insert("a".into());
        let mut t = track("a");
        t.thumbnail = None;
        let id = h.mgr.download(t).unwrap();
        let s = h.wait("failed", |s| matches!(state_of(s, id), JobState::Failed(_))).await;
        assert!(matches!(state_of(&s, id), JobState::Failed(m) if m.contains("connection reset")));
        assert!(h.files().is_empty(), "no partial file after failure");
        h.mgr.retry(id);
        h.wait("completed", |s| finished(s, id)).await;
        assert_eq!(h.backend.calls.lock().unwrap()["a"], 2);
        assert!(h.lib.snapshot().is_downloaded("a"));
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn finished_downloads_leave_the_list_failed_ones_stay() {
        let mut h = harness(3);
        h.backend.fail_once.lock().unwrap().insert("a".into());
        let (mut a, mut b) = (track("a"), track("b"));
        a.thumbnail = None;
        b.thumbnail = None;
        let a = h.mgr.download(a).unwrap();
        let b = h.mgr.download(b).unwrap();
        h.wait("b finished, a failed", |s| finished(s, b) && matches!(state_of(s, a), JobState::Failed(_))).await;
        assert!(h.lib.snapshot().is_downloaded("b"), "listed with the downloaded tracks");
        assert_eq!(h.mgr.state().jobs.len(), 1, "only the failed job remains");
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn bot_check_stops_all_downloads_once() {
        let mut h = harness(3);
        h.backend.bot_check.lock().unwrap().insert("a".into());
        // "a" runs first; the others never finish on their own (which two get the free
        // slots is up to the scheduler).
        let others = ["b", "c", "d", "e", "f"];
        h.hold(&others);
        h.mgr.download(track("a"));
        h.wait("a running", |s| matches!(s.jobs[0].state, JobState::Downloading { .. })).await;
        h.mgr.download_all(others.iter().map(|i| track(i)));
        h.wait("3 running", |s| s.jobs.iter().filter(|j| matches!(j.state, JobState::Downloading { .. })).count() == 3).await;
        h.backend.bot_check_now.notify_one(); // stored if "a" is not waiting yet
        let s = h.wait("all failed", |s| s.jobs.iter().all(|j| matches!(j.state, JobState::Failed(_)))).await;
        let message = crate::ytdlp::BOT_CHECK_MESSAGE;
        assert!(s.jobs.iter().all(|j| j.state == JobState::Failed(message.to_owned())), "{:?}", s.jobs);
        assert_eq!(s.notice.as_ref().map(|n| n.seq), Some(1), "one notice");
        assert!(s.notice.as_ref().unwrap().message.contains(message));
        tokio::time::timeout(Duration::from_secs(5), async {
            while !h.files().is_empty() {
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
        })
        .await
        .expect("partial files removed");
        tokio::time::sleep(Duration::from_millis(100)).await;
        let fetched: HashSet<String> = h.backend.calls.lock().unwrap().keys().cloned().collect();
        assert_eq!(fetched.len(), 3, "only the 3 running jobs were fetched, queued ones never started: {fetched:?}");
        assert!(fetched.contains("a"));
        assert!(h.mgr.state().jobs.iter().all(|j| !j.state.is_active()), "nothing resumes");
        // The block is over: Retry works.
        h.backend.hold.lock().unwrap().remove("d");
        let d = s.jobs.iter().find(|j| j.track.video_id == "d").unwrap().id;
        h.mgr.retry(d);
        h.wait("d completed", |s| finished(s, d)).await;
        assert_eq!(h.mgr.state().notice.as_ref().map(|n| n.seq), Some(1), "no new notice");
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn concurrency_is_limited() {
        let mut h = harness(3);
        let ids = ["a", "b", "c", "d", "e"];
        h.hold(&ids);
        h.mgr.download_all(ids.iter().map(|i| track(i)));
        let s = h
            .wait("3 running", |s| s.jobs.iter().filter(|j| matches!(j.state, JobState::Downloading { .. })).count() == 3)
            .await;
        assert_eq!(s.jobs.iter().filter(|j| j.state == JobState::Queued).count(), 2);
        tokio::time::sleep(Duration::from_millis(50)).await;
        let running = h.mgr.state().jobs.iter().filter(|j| matches!(j.state, JobState::Downloading { .. })).count();
        assert_eq!(running, 3);
        h.mgr.set_concurrency(5);
        h.wait("5 running", |s| s.jobs.iter().all(|j| matches!(j.state, JobState::Downloading { .. }))).await;
        h.backend.release.notify_waiters();
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn collection_skips_downloaded_and_duplicates() {
        let h = harness(3);
        let file = h.dir.path().join("x.m4a");
        std::fs::write(&file, b"x").unwrap();
        h.lib.add_download(&track("t1"), &file, 1).unwrap();
        h.lib.add_download(&track("t2"), &file, 1).unwrap();
        let ids: Vec<String> = (1..=12).map(|i| format!("t{i}")).collect();
        h.hold(&ids.iter().map(String::as_str).collect::<Vec<_>>());
        let mut tracks: Vec<Track> = ids.iter().map(|i| track(i)).collect();
        tracks.push(track("t5")); // duplicate in the list
        assert_eq!(h.mgr.download_all(tracks), 10);
        assert_eq!(h.mgr.download_all([track("t3")]), 0, "already queued");
        h.backend.release.notify_waiters();
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn delete_removes_file_keeps_playlist_entry() {
        let mut h = harness(3);
        let pl = h.lib.create_playlist("Focus").unwrap();
        let mut t = track("a");
        t.thumbnail = None;
        h.lib.add_tracks(pl, &[t.clone()]).unwrap();
        let id = h.mgr.download(t).unwrap();
        h.wait("completed", |s| finished(s, id)).await;
        h.mgr.delete("a").await.unwrap();
        assert!(h.files().is_empty());
        let snap = h.lib.snapshot();
        assert!(!snap.is_downloaded("a"));
        assert_eq!(snap.playlist(pl).unwrap().entries.len(), 1);
    }

    #[tokio::test(flavor = "multi_thread")]
    #[ignore = "network"]
    async fn yt_dlp_download_is_tagged_and_playable() {
        use crate::audio::{AudioEvent, AudioInput, AudioOutput, RodioOutput};
        let dir = tempfile::tempdir().unwrap();
        let song = crate::ytm::fake::cc_track();
        let lib = Arc::new(Library::open_in_memory().unwrap());
        let downloads = dir.path().join("downloads");
        let backend = Arc::new(YtDlpBackend::new(YtDlp::new("yt-dlp")));
        let mgr = Manager::new(backend, lib.clone(), downloads, 3, Notifier::noop());
        let id = mgr.download(song.clone()).unwrap();
        let mut rx = mgr.subscribe();
        tokio::time::timeout(Duration::from_secs(120), rx.wait_for(|s| s.jobs.iter().all(|j| j.id != id || !j.state.is_active())))
            .await
            .unwrap()
            .unwrap();
        assert!(finished(&mgr.state(), id), "failed: {:?}", mgr.state());
        let path = lib.snapshot().downloads[&song.video_id].path.clone();
        let file = lofty::read_from_path(&path).unwrap();
        use lofty::prelude::*;
        let tag = file.primary_tag().unwrap();
        assert_eq!(tag.title().as_deref(), Some(song.title.as_str()));
        assert_eq!(tag.artist().as_deref(), Some(song.artist_line().as_str()));
        assert!(!tag.pictures().is_empty(), "cover embedded");

        let (tx, mut events) = tokio::sync::mpsc::unbounded_channel();
        let Ok(out) = RodioOutput::start(tx, tokio::runtime::Handle::current()) else { return };
        out.set_volume(0.0);
        out.load(1, AudioInput::File(path), false);
        let ev = tokio::time::timeout(Duration::from_secs(10), events.recv()).await.unwrap().unwrap();
        assert!(matches!(ev, AudioEvent::Started { .. }), "{ev:?}");
    }
}
