//! Access to YouTube Music behind the [`MusicSource`] trait.

mod hybrid;
mod rustypipe_source;
mod stream;
#[cfg(any(test, feature = "test-util"))]
pub mod fake;

use std::future::Future;
use std::pin::Pin;

pub use hybrid::HybridSource;
pub use rustypipe_source::RustyPipeSource;
pub use stream::select_audio_stream;

use crate::error::Result;
use crate::model::{
    AlbumDetail, ArtistDetail, AudioStream, Continuation, Page, PlaylistDetail, SearchFilter, SearchItem,
    SearchResults, Track,
};

pub type BoxFut<'a, T> = Pin<Box<dyn Future<Output = Result<T>> + Send + 'a>>;

/// Everything the app needs from YouTube Music.
pub trait MusicSource: Send + Sync + 'static {
    /// Unfiltered search, results grouped by type.
    fn search(&self, query: &str) -> BoxFut<'_, SearchResults>;
    /// Search restricted to one result type, paginated.
    fn search_filtered(&self, query: &str, filter: SearchFilter) -> BoxFut<'_, Page<SearchItem>>;
    /// Next page of a filtered search.
    fn search_more(&self, cont: Continuation) -> BoxFut<'_, Page<SearchItem>>;
    fn suggestions(&self, query: &str) -> BoxFut<'_, Vec<String>>;
    fn album(&self, id: &str) -> BoxFut<'_, AlbumDetail>;
    fn playlist(&self, id: &str) -> BoxFut<'_, PlaylistDetail>;
    /// Next page of playlist tracks.
    fn playlist_more(&self, cont: Continuation) -> BoxFut<'_, Page<Track>>;
    fn artist(&self, id: &str) -> BoxFut<'_, ArtistDetail>;
    /// Resolve a playable audio stream for a track (selection per [`select_audio_stream`]).
    fn audio_stream(&self, video_id: &str) -> BoxFut<'_, AudioStream>;
    /// Forget a cached stream (e.g. after the server rejected its URL).
    fn invalidate_stream(&self, _video_id: &str) {}
}

/// Fetch all tracks of a remote playlist, following continuations.
pub async fn playlist_all_tracks(source: &dyn MusicSource, id: &str) -> Result<(PlaylistDetail, Vec<Track>)> {
    let detail = source.playlist(id).await?;
    let mut tracks = detail.tracks.items.clone();
    let mut next = detail.tracks.next.clone();
    while let Some(cont) = next {
        let page = source.playlist_more(cont).await?;
        tracks.extend(page.items);
        next = page.next;
    }
    Ok((detail, tracks))
}
