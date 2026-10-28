//! Seekable reader over an HTTP resource, fetched in ranged chunks in the background.
//!
//! googlevideo.com rejects plain, un-ranged requests for audio streams, so the
//! resource is fetched with the `range` query parameter (or a `Range` header for
//! other hosts) into an in-memory buffer. Like `rustypipe-downloader`, chunks are
//! ~9.5 MB, so a typical track is a single request (googlevideo rejects follow-up
//! requests for small chunks). Each response body is streamed into the buffer as
//! it arrives, so playback starts immediately. Reads block until the requested
//! bytes have arrived. Audio tracks are a few MB, so buffering in memory is fine.

use std::io::{self, Read, Seek, SeekFrom};
use std::sync::{Arc, Condvar, Mutex};

use reqwest::header;

use crate::error::Error;

const CHUNK: u64 = 9_500_000;

#[derive(Default)]
struct State {
    buf: Vec<u8>,
    done: bool,
    error: Option<String>,
    cancelled: bool,
}

struct Shared {
    state: Mutex<State>,
    cv: Condvar,
}

pub struct ChunkedHttpReader {
    shared: Arc<Shared>,
    pos: u64,
    total: u64,
}

fn chunk_request(client: &reqwest::Client, url: &reqwest::Url, start: u64, end: u64) -> reqwest::RequestBuilder {
    let is_gvideo = url.host_str().is_some_and(|h| h.ends_with(".googlevideo.com"));
    let req = if is_gvideo {
        let mut u = url.clone();
        u.query_pairs_mut().append_pair("range", &format!("{start}-{end}"));
        client.get(u)
    } else {
        client.get(url.clone()).header(header::RANGE, format!("bytes={start}-{end}"))
    };
    req.header(header::ORIGIN, "https://www.youtube.com").header(header::REFERER, "https://www.youtube.com/")
}

/// Error opening a stream. `rejected` means the server refused the URL (4xx), e.g. it expired.
#[derive(Debug)]
pub struct OpenError {
    pub error: Error,
    pub rejected: bool,
}

impl ChunkedHttpReader {
    /// Start fetching; returns once the first response headers arrived (so a rejected URL fails here).
    pub async fn open(url: &str, user_agent: &str, size_hint: Option<u64>) -> Result<Self, OpenError> {
        let url: reqwest::Url =
            url.parse().map_err(|_| OpenError { error: Error::invalid("bad stream url"), rejected: false })?;
        let client = reqwest::Client::builder().user_agent(user_agent).build().map_err(net_error)?;
        let clen = url.query_pairs().find(|(k, _)| k == "clen").and_then(|(_, v)| v.parse().ok());

        let first_end = clen.or(size_hint).map_or(CHUNK, |t: u64| t.min(CHUNK)) - 1;
        let res = chunk_request(&client, &url, 0, first_end).send().await.map_err(net_error)?;
        let res = res.error_for_status().map_err(net_error)?;
        let total = clen
            .or_else(|| {
                res.headers()
                    .get(header::CONTENT_RANGE)
                    .and_then(|v| v.to_str().ok())
                    .and_then(|v| v.rsplit('/').next())
                    .and_then(|v| v.parse().ok())
            })
            .or(size_hint)
            .ok_or_else(|| OpenError { error: Error::network("unknown stream length"), rejected: false })?;

        let shared = Arc::new(Shared { state: Mutex::new(State::default()), cv: Condvar::new() });
        shared.state.lock().unwrap().buf.reserve(total as usize);

        let bg = shared.clone();
        tokio::spawn(async move {
            let result = match pump(res, &bg).await {
                Ok(()) => fetch_rest(&client, &url, total, &bg).await,
                Err(e) => Err(e),
            };
            let mut st = bg.state.lock().unwrap();
            match result {
                Ok(()) => st.done = true,
                Err(e) => st.error = Some(e),
            }
            drop(st);
            bg.cv.notify_all();
        });

        Ok(Self { shared, pos: 0, total })
    }

    pub fn len(&self) -> u64 {
        self.total
    }

    pub fn is_empty(&self) -> bool {
        self.total == 0
    }
}

async fn fetch_rest(client: &reqwest::Client, url: &reqwest::Url, total: u64, shared: &Shared) -> Result<(), String> {
    loop {
        let start = {
            let st = shared.state.lock().unwrap();
            if st.cancelled {
                return Ok(());
            }
            st.buf.len() as u64
        };
        if start >= total {
            return Ok(());
        }
        let end = (start + CHUNK - 1).min(total - 1);
        let res = chunk_request(client, url, start, end)
            .send()
            .await
            .and_then(|r| r.error_for_status())
            .map_err(|e| e.to_string())?;
        pump(res, shared).await?;
        if shared.state.lock().unwrap().buf.len() as u64 == start {
            return Err(format!("empty chunk {start}-{end}"));
        }
    }
}

/// Stream a response body into the shared buffer.
async fn pump(mut res: reqwest::Response, shared: &Shared) -> Result<(), String> {
    while let Some(bytes) = res.chunk().await.map_err(|e| e.to_string())? {
        let mut st = shared.state.lock().unwrap();
        if st.cancelled {
            return Ok(());
        }
        st.buf.extend_from_slice(&bytes);
        drop(st);
        shared.cv.notify_all();
    }
    Ok(())
}

fn net_error(e: reqwest::Error) -> OpenError {
    OpenError { rejected: e.status().is_some_and(|s| s.is_client_error()), error: Error::network(e.to_string()) }
}

impl Read for ChunkedHttpReader {
    fn read(&mut self, out: &mut [u8]) -> io::Result<usize> {
        if out.is_empty() || self.pos >= self.total {
            return Ok(0);
        }
        let mut st = self.shared.state.lock().unwrap();
        while (st.buf.len() as u64) <= self.pos && !st.done && st.error.is_none() {
            st = self.shared.cv.wait(st).unwrap();
        }
        let avail = st.buf.len() as u64;
        if avail <= self.pos {
            return match &st.error {
                Some(e) => Err(io::Error::other(e.clone())),
                None => Ok(0),
            };
        }
        let start = self.pos as usize;
        let n = out.len().min(avail as usize - start);
        out[..n].copy_from_slice(&st.buf[start..start + n]);
        self.pos += n as u64;
        Ok(n)
    }
}

impl Seek for ChunkedHttpReader {
    fn seek(&mut self, pos: SeekFrom) -> io::Result<u64> {
        let new = match pos {
            SeekFrom::Start(p) => p as i128,
            SeekFrom::End(d) => self.total as i128 + d as i128,
            SeekFrom::Current(d) => self.pos as i128 + d as i128,
        };
        if new < 0 {
            return Err(io::Error::new(io::ErrorKind::InvalidInput, "seek before start"));
        }
        self.pos = new as u64;
        Ok(self.pos)
    }
}

impl Drop for ChunkedHttpReader {
    fn drop(&mut self) {
        self.shared.state.lock().unwrap().cancelled = true;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test(flavor = "multi_thread")]
    #[ignore = "network"]
    async fn reads_whole_googlevideo_stream() {
        use crate::ytm::{HybridSource, MusicSource, RustyPipeSource};
        let dir = tempfile::tempdir().unwrap();
        let source = HybridSource::new(RustyPipeSource::new(dir.path()).unwrap(), crate::ytdlp::YtDlp::new("yt-dlp"));
        let stream = source.audio_stream(crate::ytm::fake::CC_VIDEO_ID).await.unwrap();
        let mut r = ChunkedHttpReader::open(&stream.url, &stream.user_agent, Some(stream.size)).await.unwrap();
        let total = r.len();
        let buf = tokio::task::spawn_blocking(move || {
            let mut buf = Vec::new();
            r.read_to_end(&mut buf).map(|_| buf)
        })
        .await
        .unwrap()
        .unwrap();
        assert_eq!(buf.len() as u64, total);
        assert_eq!(&buf[4..8], b"ftyp");
    }
}
