//! Domain types shared between the YouTube Music source, the library and the UI.

use std::any::Any;
use std::fmt;
use std::sync::Arc;

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct ArtistRef {
    pub id: Option<String>,
    pub name: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct AlbumRef {
    pub id: String,
    pub name: String,
}

/// A playable song (a YouTube video id plus music metadata).
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct Track {
    pub video_id: String,
    pub title: String,
    pub artists: Vec<ArtistRef>,
    pub album: Option<AlbumRef>,
    pub duration_s: Option<u32>,
    pub thumbnail: Option<String>,
}

impl Track {
    /// Artist names joined for display, e.g. "Daft Punk, Pharrell Williams".
    pub fn artist_line(&self) -> String {
        let names: Vec<&str> = self.artists.iter().map(|a| a.name.as_str()).collect();
        if names.is_empty() {
            "Unknown artist".to_owned()
        } else {
            names.join(", ")
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AlbumSummary {
    pub id: String,
    pub title: String,
    pub artists: Vec<ArtistRef>,
    pub year: Option<u16>,
    pub thumbnail: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ArtistSummary {
    pub id: String,
    pub name: String,
    pub thumbnail: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlaylistSummary {
    pub id: String,
    pub title: String,
    pub owner: Option<String>,
    pub track_count: Option<u64>,
    pub thumbnail: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AlbumDetail {
    pub id: String,
    pub title: String,
    pub artists: Vec<ArtistRef>,
    pub year: Option<u16>,
    pub thumbnail: Option<String>,
    pub tracks: Vec<Track>,
}

#[derive(Debug, Clone)]
pub struct PlaylistDetail {
    pub id: String,
    pub title: String,
    pub owner: Option<String>,
    pub thumbnail: Option<String>,
    pub tracks: Page<Track>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ArtistDetail {
    pub id: String,
    pub name: String,
    pub thumbnail: Option<String>,
    pub top_tracks: Vec<Track>,
    pub albums: Vec<AlbumSummary>,
}

/// Result types a search can be restricted to.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum SearchFilter {
    Songs,
    Albums,
    Artists,
    Playlists,
}

impl SearchFilter {
    pub const ALL: [SearchFilter; 4] = [Self::Songs, Self::Albums, Self::Artists, Self::Playlists];

    pub fn label(self) -> &'static str {
        match self {
            Self::Songs => "Songs",
            Self::Albums => "Albums",
            Self::Artists => "Artists",
            Self::Playlists => "Playlists",
        }
    }
}

/// Grouped results of an unfiltered search.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SearchResults {
    pub songs: Vec<Track>,
    pub albums: Vec<AlbumSummary>,
    pub artists: Vec<ArtistSummary>,
    pub playlists: Vec<PlaylistSummary>,
}

impl SearchResults {
    pub fn is_empty(&self) -> bool {
        self.songs.is_empty() && self.albums.is_empty() && self.artists.is_empty() && self.playlists.is_empty()
    }
}

/// One item of a filtered search.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SearchItem {
    Song(Track),
    Album(AlbumSummary),
    Artist(ArtistSummary),
    Playlist(PlaylistSummary),
}

/// Opaque continuation token owned by a [`crate::ytm::MusicSource`] implementation.
#[derive(Clone)]
pub struct Continuation(pub Arc<dyn Any + Send + Sync>);

impl fmt::Debug for Continuation {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("Continuation")
    }
}

/// A page of results with an optional handle to fetch the next page.
#[derive(Debug, Clone)]
pub struct Page<T> {
    pub items: Vec<T>,
    pub next: Option<Continuation>,
}

impl<T> Page<T> {
    pub fn last(items: Vec<T>) -> Self {
        Self { items, next: None }
    }
}

/// Audio codec of a resolved stream.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AudioCodec {
    Aac,
    Opus,
    Other,
}

/// An audio-only stream of a track.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AudioStream {
    pub url: String,
    pub itag: u32,
    pub codec: AudioCodec,
    pub bitrate: u32,
    pub size: u64,
    pub mime: String,
    /// User-Agent the stream URL must be fetched with.
    pub user_agent: String,
}

/// Pick the largest thumbnail URL from a list of `(url, width)` pairs.
pub(crate) fn best_thumbnail<'a>(thumbs: impl IntoIterator<Item = (&'a str, u32)>) -> Option<String> {
    thumbs.into_iter().max_by_key(|(_, w)| *w).map(|(u, _)| u.to_owned())
}
