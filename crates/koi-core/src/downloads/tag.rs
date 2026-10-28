//! Writing our track metadata into downloaded files with `lofty`.

use std::path::Path;

use lofty::config::WriteOptions;
use lofty::picture::{Picture, PictureType};
use lofty::prelude::*;
use lofty::tag::Tag;

use crate::error::{Error, ErrorKind, Result};
use crate::model::Track;

fn tag_error(e: impl std::fmt::Display) -> Error {
    Error::new(ErrorKind::Io, format!("cannot write tags: {e}"))
}

/// Thumbnail URL to fetch for embedding. MP4 covers must be JPEG or PNG, so
/// Google image URLs (`...=w120-h120-l90-rj`) are asked for a 544 px JPEG.
pub fn cover_url(thumbnail: &str) -> String {
    match thumbnail.rsplit_once('=') {
        Some((base, _)) if thumbnail.contains("googleusercontent.com") => format!("{base}=w544-h544-l90-rj"),
        _ => thumbnail.to_owned(),
    }
}

/// Write title, artist, album and (if decodable as JPEG/PNG) the cover into `path`.
pub fn write_tags(path: &Path, track: &Track, cover: Option<&[u8]>) -> Result<()> {
    let mut file = lofty::read_from_path(path).map_err(tag_error)?;
    let tag_type = file.primary_tag_type();
    if file.tag(tag_type).is_none() {
        file.insert_tag(Tag::new(tag_type));
    }
    let tag = file.tag_mut(tag_type).expect("inserted above");
    tag.set_title(track.title.clone());
    tag.set_artist(track.artist_line());
    if let Some(album) = &track.album {
        tag.set_album(album.name.clone());
    }
    if let Some(data) = cover {
        match Picture::from_reader(&mut &data[..]) {
            Ok(mut pic) => {
                pic.set_pic_type(PictureType::CoverFront);
                tag.remove_picture_type(PictureType::CoverFront);
                tag.push_picture(pic);
            }
            Err(e) => tracing::warn!("skipping cover art for {}: {e}", track.video_id),
        }
    }
    tag.save_to_path(path, WriteOptions::default()).map_err(tag_error)
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use crate::ytm::fake::track;

    pub(crate) fn fixture(name: &str) -> std::path::PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures").join(name)
    }

    #[test]
    fn writes_title_artist_album_and_cover() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("a.m4a");
        std::fs::copy(fixture("tone-5s.m4a"), &path).unwrap();
        let cover = std::fs::read(fixture("cover.jpg")).unwrap();
        write_tags(&path, &track("a"), Some(&cover)).unwrap();

        let file = lofty::read_from_path(&path).unwrap();
        let tag = file.primary_tag().unwrap();
        assert_eq!(tag.title().as_deref(), Some("Title a"));
        assert_eq!(tag.artist().as_deref(), Some("Artist"));
        assert_eq!(tag.album().as_deref(), Some("Album"));
        // MP4 `covr` atoms carry no picture type, so only the data is checked.
        assert_eq!(tag.pictures().len(), 1);
        assert_eq!(tag.pictures()[0].data(), &cover[..]);
    }

    #[test]
    fn undecodable_cover_is_skipped() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("a.m4a");
        std::fs::copy(fixture("tone-5s.m4a"), &path).unwrap();
        write_tags(&path, &track("a"), Some(b"RIFF....WEBPVP8 ")).unwrap();
        let file = lofty::read_from_path(&path).unwrap();
        assert!(file.primary_tag().unwrap().pictures().is_empty());
    }

    #[test]
    fn google_thumbnails_are_requested_as_jpeg() {
        assert_eq!(
            cover_url("https://lh3.googleusercontent.com/abc=w120-h120-l90-rj"),
            "https://lh3.googleusercontent.com/abc=w544-h544-l90-rj"
        );
        assert_eq!(cover_url("https://i.ytimg.com/vi/x/hq.jpg"), "https://i.ytimg.com/vi/x/hq.jpg");
    }
}
