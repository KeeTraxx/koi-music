use std::sync::Arc;

use super::{BoxFut, MusicSource};
use crate::model::*;
use crate::ytdlp::{StreamCache, YtDlp};

/// Metadata from one [`MusicSource`] (rustypipe), audio streams from yt-dlp, with a URL cache.
pub struct HybridSource<M> {
    meta: M,
    ytdlp: YtDlp,
    cache: Arc<StreamCache>,
}

impl<M: MusicSource> HybridSource<M> {
    pub fn new(meta: M, ytdlp: YtDlp) -> Self {
        Self { meta, ytdlp, cache: Arc::default() }
    }

    pub fn cache(&self) -> Arc<StreamCache> {
        self.cache.clone()
    }
}

impl<M: MusicSource> MusicSource for HybridSource<M> {
    fn search(&self, query: &str) -> BoxFut<'_, SearchResults> {
        self.meta.search(query)
    }
    fn search_filtered(&self, query: &str, filter: SearchFilter) -> BoxFut<'_, Page<SearchItem>> {
        self.meta.search_filtered(query, filter)
    }
    fn search_more(&self, cont: Continuation) -> BoxFut<'_, Page<SearchItem>> {
        self.meta.search_more(cont)
    }
    fn suggestions(&self, query: &str) -> BoxFut<'_, Vec<String>> {
        self.meta.suggestions(query)
    }
    fn album(&self, id: &str) -> BoxFut<'_, AlbumDetail> {
        self.meta.album(id)
    }
    fn playlist(&self, id: &str) -> BoxFut<'_, PlaylistDetail> {
        self.meta.playlist(id)
    }
    fn playlist_more(&self, cont: Continuation) -> BoxFut<'_, Page<Track>> {
        self.meta.playlist_more(cont)
    }
    fn artist(&self, id: &str) -> BoxFut<'_, ArtistDetail> {
        self.meta.artist(id)
    }
    fn audio_stream(&self, video_id: &str) -> BoxFut<'_, AudioStream> {
        let id = video_id.to_owned();
        Box::pin(async move {
            if let Some(s) = self.cache.get(&id) {
                return Ok(s);
            }
            let s = self.ytdlp.resolve(&id).await?;
            self.cache.insert(&id, s.clone());
            Ok(s)
        })
    }
    fn invalidate_stream(&self, video_id: &str) {
        self.cache.invalidate(video_id);
    }
}
