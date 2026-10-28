use std::path::Path;
use std::sync::Arc;

use rustypipe::client::{RustyPipe, RustyPipeQuery};
use rustypipe::model::{
    self as rp, AlbumItem, ArtistItem, MusicItem, MusicPlaylistItem, Thumbnail, TrackItem, paginator::Paginator,
};
use rustypipe::param::search_filter::MusicSearchFilter;

use super::{BoxFut, MusicSource, select_audio_stream};
use crate::error::{Error, Result};
use crate::model::{
    AlbumDetail, AlbumRef, AlbumSummary, ArtistDetail, ArtistRef, ArtistSummary, AudioCodec, AudioStream,
    Continuation, Page, PlaylistDetail, PlaylistSummary, SearchFilter, SearchItem, SearchResults, Track,
    best_thumbnail,
};

/// [`MusicSource`] backed by the `rustypipe` InnerTube client.
#[derive(Clone)]
pub struct RustyPipeSource {
    rp: RustyPipe,
}

/// Continuation state for the paginators we hand out.
enum Cont {
    Search(Paginator<MusicItem>),
    Tracks(Paginator<TrackItem>),
}

impl RustyPipeSource {
    pub fn new(storage_dir: &Path) -> Result<Self> {
        std::fs::create_dir_all(storage_dir)?;
        let rp = RustyPipe::builder()
            .storage_dir(storage_dir)
            .no_reporter()
            .build()
            .map_err(Error::from)?;
        Ok(Self { rp })
    }

    pub fn client(&self) -> &RustyPipe {
        &self.rp
    }

    fn q(&self) -> RustyPipeQuery {
        self.rp.query()
    }
}

fn thumb(t: &[Thumbnail]) -> Option<String> {
    best_thumbnail(t.iter().map(|t| (t.url.as_str(), t.width)))
}

fn artists(a: &[rp::ArtistId]) -> Vec<ArtistRef> {
    a.iter().map(|a| ArtistRef { id: a.id.clone(), name: a.name.clone() }).collect()
}

pub(crate) fn track(t: TrackItem) -> Track {
    Track {
        thumbnail: thumb(&t.cover),
        artists: artists(&t.artists),
        album: t.album.map(|a| AlbumRef { id: a.id, name: a.name }),
        duration_s: t.duration,
        title: t.name,
        video_id: t.id,
    }
}

/// A song without duration and without any linked artist was not parsed properly.
fn is_incomplete(t: &Track) -> bool {
    t.duration_s.is_none() && t.artists.iter().all(|a| a.id.is_none())
}

fn album_summary(a: AlbumItem) -> AlbumSummary {
    AlbumSummary { thumbnail: thumb(&a.cover), artists: artists(&a.artists), year: a.year, title: a.name, id: a.id }
}

fn artist_summary(a: ArtistItem) -> ArtistSummary {
    ArtistSummary { thumbnail: thumb(&a.avatar), name: a.name, id: a.id }
}

fn playlist_summary(p: MusicPlaylistItem) -> PlaylistSummary {
    PlaylistSummary {
        thumbnail: thumb(&p.thumbnail),
        owner: p.channel.map(|c| c.name),
        track_count: p.track_count,
        title: p.name,
        id: p.id,
    }
}

fn search_item(item: MusicItem) -> Option<SearchItem> {
    Some(match item {
        MusicItem::Track(t) => SearchItem::Song(track(t)),
        MusicItem::Album(a) => SearchItem::Album(album_summary(a)),
        MusicItem::Artist(a) => SearchItem::Artist(artist_summary(a)),
        MusicItem::Playlist(p) => SearchItem::Playlist(playlist_summary(p)),
        _ => return None,
    })
}

/// Take the items out of a paginator and keep the rest as continuation state.
fn split_paginator<T>(mut p: Paginator<T>, wrap: impl FnOnce(Paginator<T>) -> Cont) -> (Vec<T>, Option<Continuation>) {
    let items = std::mem::take(&mut p.items);
    let next = (!p.is_exhausted()).then(|| Continuation(Arc::new(CellCont(std::sync::Mutex::new(Some(wrap(p)))))));
    (items, next)
}

/// `Paginator` is not `Sync`-friendly to share, so wrap it; it is consumed once.
struct CellCont(std::sync::Mutex<Option<Cont>>);

fn take_cont(cont: &Continuation) -> Result<Cont> {
    cont.0
        .downcast_ref::<CellCont>()
        .and_then(|c| c.0.lock().ok()?.take())
        .ok_or_else(|| Error::invalid("continuation already used"))
}

fn map_stream(s: &rp::AudioStream, user_agent: &str) -> AudioStream {
    AudioStream {
        url: s.url.clone(),
        itag: s.itag,
        codec: match s.codec {
            rp::AudioCodec::Mp4a => AudioCodec::Aac,
            rp::AudioCodec::Opus => AudioCodec::Opus,
            _ => AudioCodec::Other,
        },
        bitrate: s.bitrate,
        size: s.size,
        mime: s.mime.clone(),
        user_agent: user_agent.to_owned(),
    }
}

impl MusicSource for RustyPipeSource {
    fn search(&self, query: &str) -> BoxFut<'_, SearchResults> {
        let query = query.to_owned();
        Box::pin(async move {
            let res = self.q().music_search_main(&query).await?;
            let mut out = SearchResults::default();
            for item in res.items.items {
                match search_item(item) {
                    Some(SearchItem::Song(t)) => out.songs.push(t),
                    Some(SearchItem::Album(a)) => out.albums.push(a),
                    Some(SearchItem::Artist(a)) => out.artists.push(a),
                    Some(SearchItem::Playlist(p)) => out.playlists.push(p),
                    None => {}
                }
            }
            // For artist-like queries rustypipe misparses the top songs card (the type
            // label "Song" ends up as the artist, no duration). Use the songs search instead.
            if out.songs.iter().any(is_incomplete) {
                let n = out.songs.len().max(3);
                let res = self.q().music_search::<MusicItem, _>(&query, Some(MusicSearchFilter::Tracks)).await?;
                out.songs = res.items.items.into_iter().filter_map(search_item).filter_map(|i| match i {
                    SearchItem::Song(t) => Some(t),
                    _ => None,
                }).take(n).collect();
            }
            Ok(out)
        })
    }

    fn search_filtered(&self, query: &str, filter: SearchFilter) -> BoxFut<'_, Page<SearchItem>> {
        let query = query.to_owned();
        Box::pin(async move {
            let f = match filter {
                SearchFilter::Songs => MusicSearchFilter::Tracks,
                SearchFilter::Albums => MusicSearchFilter::Albums,
                SearchFilter::Artists => MusicSearchFilter::Artists,
                SearchFilter::Playlists => MusicSearchFilter::YtmPlaylists,
            };
            let res = self.q().music_search::<MusicItem, _>(&query, Some(f)).await?;
            let (items, next) = split_paginator(res.items, Cont::Search);
            Ok(Page { items: items.into_iter().filter_map(search_item).collect(), next })
        })
    }

    fn search_more(&self, cont: Continuation) -> BoxFut<'_, Page<SearchItem>> {
        Box::pin(async move {
            let Cont::Search(p) = take_cont(&cont)? else {
                return Err(Error::invalid("not a search continuation"));
            };
            let Some(p) = p.next(self.q()).await? else { return Ok(Page::last(Vec::new())) };
            let (items, next) = split_paginator(p, Cont::Search);
            Ok(Page { items: items.into_iter().filter_map(search_item).collect(), next })
        })
    }

    fn suggestions(&self, query: &str) -> BoxFut<'_, Vec<String>> {
        let query = query.to_owned();
        Box::pin(async move { Ok(self.q().music_search_suggestion(&query).await?.terms) })
    }

    fn album(&self, id: &str) -> BoxFut<'_, AlbumDetail> {
        let id = id.to_owned();
        Box::pin(async move {
            let a = self.q().music_album(&id).await?;
            let cover = thumb(&a.cover);
            let album_ref = AlbumRef { id: a.id.clone(), name: a.name.clone() };
            let tracks = a
                .tracks
                .into_iter()
                .map(|t| {
                    let mut t = track(t);
                    // Album tracks often come without cover/album info.
                    t.album.get_or_insert_with(|| album_ref.clone());
                    if t.thumbnail.is_none() {
                        t.thumbnail = cover.clone();
                    }
                    t
                })
                .collect();
            Ok(AlbumDetail { id: a.id, title: a.name, artists: artists(&a.artists), year: a.year, thumbnail: cover, tracks })
        })
    }

    fn playlist(&self, id: &str) -> BoxFut<'_, PlaylistDetail> {
        let id = id.to_owned();
        Box::pin(async move {
            let p = self.q().music_playlist(&id).await?;
            let (items, next) = split_paginator(p.tracks, Cont::Tracks);
            Ok(PlaylistDetail {
                thumbnail: thumb(&p.thumbnail),
                owner: p.channel.map(|c| c.name),
                title: p.name,
                id: p.id,
                tracks: Page { items: items.into_iter().map(track).collect(), next },
            })
        })
    }

    fn playlist_more(&self, cont: Continuation) -> BoxFut<'_, Page<Track>> {
        Box::pin(async move {
            let Cont::Tracks(p) = take_cont(&cont)? else {
                return Err(Error::invalid("not a playlist continuation"));
            };
            let Some(p) = p.next(self.q()).await? else { return Ok(Page::last(Vec::new())) };
            let (items, next) = split_paginator(p, Cont::Tracks);
            Ok(Page { items: items.into_iter().map(track).collect(), next })
        })
    }

    fn artist(&self, id: &str) -> BoxFut<'_, ArtistDetail> {
        let id = id.to_owned();
        Box::pin(async move {
            let a = self.q().music_artist(&id, false).await?;
            Ok(ArtistDetail {
                thumbnail: thumb(&a.header_image),
                top_tracks: a.tracks.into_iter().map(track).collect(),
                albums: a.albums.into_iter().map(album_summary).collect(),
                name: a.name,
                id: a.id,
            })
        })
    }

    fn audio_stream(&self, video_id: &str) -> BoxFut<'_, AudioStream> {
        let video_id = video_id.to_owned();
        Box::pin(async move {
            let q = self.q();
            let player = q.player(&video_id).await?;
            let ua = q.user_agent(player.client_type);
            let streams: Vec<AudioStream> = player.audio_streams.iter().map(|s| map_stream(s, &ua)).collect();
            select_audio_stream(&streams)
        })
    }
}

#[cfg(test)]
mod tests {
    //! Network tests; run with `cargo test -p koi-core -- --ignored`.
    use super::*;

    fn source() -> (tempfile::TempDir, RustyPipeSource) {
        let dir = tempfile::tempdir().unwrap();
        let s = RustyPipeSource::new(dir.path()).unwrap();
        (dir, s)
    }

    #[tokio::test]
    #[ignore = "network"]
    async fn search_returns_songs() {
        let (_d, s) = source();
        let res = s.search("beethoven moonlight sonata").await.unwrap();
        assert!(!res.songs.is_empty());
        for song in &res.songs {
            assert!(!song.title.is_empty() && !song.artists.is_empty() && song.thumbnail.is_some(), "{song:?}");
        }
        // The "top result" card can lack a duration; the regular song rows have one.
        assert!(res.songs.iter().any(|s| s.duration_s.is_some()), "{:?}", res.songs);
    }

    #[tokio::test]
    #[ignore = "network"]
    async fn artist_query_songs_have_metadata() {
        let (_d, s) = source();
        let res = s.search("daft punk").await.unwrap();
        assert!(!res.songs.is_empty());
        for song in &res.songs {
            assert!(!is_incomplete(song), "{song:?}");
            assert!(song.artists.iter().all(|a| a.name != "Song"), "{song:?}");
        }
    }

    #[tokio::test]
    #[ignore = "network"]
    async fn filtered_search_paginates() {
        let (_d, s) = source();
        let page = s.search_filtered("daft punk", SearchFilter::Songs).await.unwrap();
        assert!(page.items.iter().all(|i| matches!(i, SearchItem::Song(_))));
        let next = s.search_more(page.next.expect("more pages")).await.unwrap();
        assert!(!next.items.is_empty());
    }

    #[tokio::test]
    #[ignore = "network"]
    async fn suggestions_work() {
        let (_d, s) = source();
        let sug = s.suggestions("radioh").await.unwrap();
        assert!(sug.iter().any(|t| t.contains("radiohead")), "{sug:?}");
    }

    #[tokio::test]
    #[ignore = "network"]
    async fn album_has_ordered_tracks() {
        let (_d, s) = source();
        let res = s.search_filtered("daft punk discovery", SearchFilter::Albums).await.unwrap();
        let Some(SearchItem::Album(a)) = res.items.into_iter().next() else { panic!("no album") };
        let album = s.album(&a.id).await.unwrap();
        assert!(album.tracks.len() >= 10, "{}", album.tracks.len());
        assert!(album.thumbnail.is_some());
    }

    #[tokio::test]
    #[ignore = "network"]
    async fn artist_and_stream() {
        let (_d, s) = source();
        let res = s.search_filtered("daft punk", SearchFilter::Artists).await.unwrap();
        let Some(SearchItem::Artist(a)) = res.items.into_iter().next() else { panic!("no artist") };
        let artist = s.artist(&a.id).await.unwrap();
        assert!(!artist.top_tracks.is_empty());
        assert!(!artist.top_tracks.is_empty());
        // Streams are only resolved for freely licensed content in tests.
        let stream = s.audio_stream(super::super::fake::CC_VIDEO_ID).await.unwrap();
        assert_eq!(stream.codec, AudioCodec::Aac);
    }
}
