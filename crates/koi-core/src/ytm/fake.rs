//! In-memory [`MusicSource`] for tests.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use super::{BoxFut, MusicSource};
use crate::error::{Error, ErrorKind};
use crate::model::*;

#[derive(Default)]
pub struct FakeSource {
    pub search: Mutex<SearchResults>,
    pub albums: Mutex<HashMap<String, AlbumDetail>>,
    pub playlists: Mutex<HashMap<String, (String, Vec<Track>)>>,
    pub artists: Mutex<HashMap<String, ArtistDetail>>,
    /// Video ids that have no playable stream.
    pub unavailable: Mutex<Vec<String>>,
    /// While set, every stream request fails with YouTube's bot check.
    pub bot_check: Mutex<bool>,
    /// Number of audio_stream calls per video id.
    pub stream_calls: Mutex<HashMap<String, usize>>,
    /// Page size for paginated responses.
    pub page_size: usize,
}

/// Freely licensed video for network tests that stream or download: "Big Buck Bunny"
/// on the Blender Foundation's official channel, licensed Creative Commons Attribution
/// (license checked via yt-dlp metadata on 2026-10-28). Never use commercial recordings here.
pub const CC_VIDEO_ID: &str = "aqz-KE-bpKQ";

/// Metadata for [`CC_VIDEO_ID`].
pub fn cc_track() -> Track {
    Track {
        video_id: CC_VIDEO_ID.to_owned(),
        title: "Big Buck Bunny".to_owned(),
        artists: vec![ArtistRef { id: None, name: "Blender Foundation".into() }],
        album: None,
        duration_s: Some(635),
        thumbnail: Some(format!("https://i.ytimg.com/vi/{CC_VIDEO_ID}/hqdefault.jpg")),
    }
}

pub fn track(id: &str) -> Track {
    Track {
        video_id: id.to_owned(),
        title: format!("Title {id}"),
        artists: vec![ArtistRef { id: Some("artist".into()), name: "Artist".into() }],
        album: Some(AlbumRef { id: "album".into(), name: "Album".into() }),
        duration_s: Some(200),
        thumbnail: Some(format!("https://img/{id}.jpg")),
    }
}

impl FakeSource {
    pub fn new() -> Self {
        Self { page_size: 2, ..Default::default() }
    }

    pub fn with_album(self, id: &str, title: &str, track_ids: &[&str]) -> Self {
        let album = AlbumDetail {
            id: id.into(),
            title: title.into(),
            artists: vec![ArtistRef { id: None, name: "Artist".into() }],
            year: Some(2001),
            thumbnail: None,
            tracks: track_ids.iter().map(|t| track(t)).collect(),
        };
        self.albums.lock().unwrap().insert(id.into(), album);
        self
    }

    pub fn with_playlist(self, id: &str, title: &str, track_ids: &[&str]) -> Self {
        let tracks = track_ids.iter().map(|t| track(t)).collect();
        self.playlists.lock().unwrap().insert(id.into(), (title.into(), tracks));
        self
    }

    fn page(&self, items: Vec<Track>, offset: usize) -> Page<Track> {
        let end = (offset + self.page_size).min(items.len());
        let page = items[offset..end].to_vec();
        let next = (end < items.len()).then(|| Continuation(Arc::new((items, end))));
        Page { items: page, next }
    }
}

fn not_found(id: &str) -> Error {
    Error::new(ErrorKind::NotFound, id.to_owned())
}

impl MusicSource for FakeSource {
    fn search(&self, _query: &str) -> BoxFut<'_, SearchResults> {
        let r = self.search.lock().unwrap().clone();
        Box::pin(async move { Ok(r) })
    }

    fn search_filtered(&self, _query: &str, _filter: SearchFilter) -> BoxFut<'_, Page<SearchItem>> {
        let songs = self.search.lock().unwrap().songs.clone();
        Box::pin(async move { Ok(Page::last(songs.into_iter().map(SearchItem::Song).collect())) })
    }

    fn search_more(&self, _cont: Continuation) -> BoxFut<'_, Page<SearchItem>> {
        Box::pin(async move { Ok(Page::last(Vec::new())) })
    }

    fn suggestions(&self, query: &str) -> BoxFut<'_, Vec<String>> {
        let q = query.to_owned();
        Box::pin(async move { Ok(vec![format!("{q}ead")]) })
    }

    fn album(&self, id: &str) -> BoxFut<'_, AlbumDetail> {
        let r = self.albums.lock().unwrap().get(id).cloned().ok_or_else(|| not_found(id));
        Box::pin(async move { r })
    }

    fn playlist(&self, id: &str) -> BoxFut<'_, PlaylistDetail> {
        let r = self.playlists.lock().unwrap().get(id).cloned();
        let id = id.to_owned();
        Box::pin(async move {
            let (title, tracks) = r.ok_or_else(|| not_found(&id))?;
            Ok(PlaylistDetail { id, title, owner: None, thumbnail: None, tracks: self.page(tracks, 0) })
        })
    }

    fn playlist_more(&self, cont: Continuation) -> BoxFut<'_, Page<Track>> {
        Box::pin(async move {
            let (items, offset) =
                cont.0.downcast_ref::<(Vec<Track>, usize)>().cloned().ok_or_else(|| Error::invalid("bad cont"))?;
            Ok(self.page(items, offset))
        })
    }

    fn artist(&self, id: &str) -> BoxFut<'_, ArtistDetail> {
        let r = self.artists.lock().unwrap().get(id).cloned().ok_or_else(|| not_found(id));
        Box::pin(async move { r })
    }

    fn audio_stream(&self, video_id: &str) -> BoxFut<'_, AudioStream> {
        *self.stream_calls.lock().unwrap().entry(video_id.to_owned()).or_default() += 1;
        let unavailable = self.unavailable.lock().unwrap().iter().any(|v| v == video_id);
        let bot_check = *self.bot_check.lock().unwrap();
        let id = video_id.to_owned();
        Box::pin(async move {
            if bot_check {
                return Err(Error::new(ErrorKind::BotCheck, crate::ytdlp::BOT_CHECK_MESSAGE));
            }
            if unavailable {
                return Err(Error::unavailable(format!("{id} is region-blocked")));
            }
            Ok(AudioStream {
                url: format!("https://stream/{id}"),
                itag: 140,
                codec: AudioCodec::Aac,
                bitrate: 128_000,
                size: 1000,
                mime: "audio/mp4".into(),
                user_agent: "test".into(),
            })
        })
    }
}
