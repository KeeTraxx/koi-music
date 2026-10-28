//! Saving remote YouTube Music albums and playlists as local playlists.

use std::sync::Arc;

use crate::error::{Error, Result};
use crate::library::{Library, PlaylistId};
use crate::model::Track;
use crate::ytm::{MusicSource, playlist_all_tracks};

/// What to import.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RemoteCollection {
    Album(String),
    Playlist(String),
}

/// Fetch all tracks of a remote album or playlist (following pagination).
/// Returns its title and tracks in order.
pub async fn fetch_collection(source: &dyn MusicSource, what: &RemoteCollection) -> Result<(String, Vec<Track>)> {
    match what {
        RemoteCollection::Album(id) => {
            let album = source.album(id).await?;
            Ok((album.title, album.tracks))
        }
        RemoteCollection::Playlist(id) => {
            let (detail, tracks) = playlist_all_tracks(source, id).await?;
            Ok((detail.title, tracks))
        }
    }
}

/// Save a remote album/playlist as a new local playlist named after it.
/// The result is a snapshot: later remote changes are not reflected.
pub async fn import_collection(
    source: &dyn MusicSource,
    library: Arc<Library>,
    what: &RemoteCollection,
) -> Result<PlaylistId> {
    let (title, tracks) = fetch_collection(source, what).await?;
    let source_id = match what {
        RemoteCollection::Album(id) | RemoteCollection::Playlist(id) => id.clone(),
    };
    tokio::task::spawn_blocking(move || library.import_playlist(&title, &source_id, &tracks))
        .await
        .map_err(|e| Error::new(crate::ErrorKind::Db, e.to_string()))?
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ytm::fake::FakeSource;

    fn ids(lib: &Library, id: PlaylistId) -> (String, Vec<String>) {
        let snap = lib.snapshot();
        let p = snap.playlist(id).unwrap();
        (p.name.clone(), p.entries.iter().map(|e| e.track.video_id.clone()).collect())
    }

    #[tokio::test]
    async fn album_becomes_ordered_playlist_named_after_it() {
        let src = FakeSource::new().with_album("MPREb_1", "Discovery", &["t1", "t2", "t3"]);
        let lib = Arc::new(Library::open_in_memory().unwrap());
        let id = import_collection(&src, lib.clone(), &RemoteCollection::Album("MPREb_1".into())).await.unwrap();
        assert_eq!(ids(&lib, id), ("Discovery".into(), vec!["t1".into(), "t2".into(), "t3".into()]));
        assert_eq!(lib.snapshot().playlist(id).unwrap().source_id.as_deref(), Some("MPREb_1"));
    }

    #[tokio::test]
    async fn paginated_playlist_is_imported_completely() {
        // FakeSource pages 2 items at a time.
        let src = FakeSource::new().with_playlist("PL1", "Mix", &["a", "b", "c", "d", "e"]);
        let lib = Arc::new(Library::open_in_memory().unwrap());
        let id = import_collection(&src, lib.clone(), &RemoteCollection::Playlist("PL1".into())).await.unwrap();
        let (name, tracks) = ids(&lib, id);
        assert_eq!(name, "Mix");
        assert_eq!(tracks, ["a", "b", "c", "d", "e"]);
    }

    #[tokio::test]
    async fn missing_album_is_an_error_and_creates_nothing() {
        let src = FakeSource::new();
        let lib = Arc::new(Library::open_in_memory().unwrap());
        let err = import_collection(&src, lib.clone(), &RemoteCollection::Album("nope".into())).await.unwrap_err();
        assert_eq!(err.kind, crate::ErrorKind::NotFound);
        assert!(lib.snapshot().playlists.is_empty());
    }
}
