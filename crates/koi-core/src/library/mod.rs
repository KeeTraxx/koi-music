//! Local library: playlists, history, downloads registry and settings (SQLite).
//!
//! All methods are synchronous; call them from a blocking context
//! (`tokio::task::spawn_blocking`). After every mutation a fresh
//! [`LibrarySnapshot`] is published for the UI to read without touching the DB.

mod db;

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, RwLock};
use std::time::{SystemTime, UNIX_EPOCH};

use rusqlite::{Connection, OptionalExtension, params};
use serde::Serialize;
use serde::de::DeserializeOwned;

use crate::error::{Error, Result};
use crate::model::{AlbumRef, Track};
use crate::notify::Notifier;

/// History keeps at most this many entries.
pub const HISTORY_LIMIT: usize = 200;

pub type PlaylistId = i64;
pub type EntryId = i64;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlaylistEntry {
    pub id: EntryId,
    pub track: Track,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Playlist {
    pub id: PlaylistId,
    pub name: String,
    /// Remote album/playlist id this was imported from.
    pub source_id: Option<String>,
    pub entries: Vec<PlaylistEntry>,
}

impl Playlist {
    pub fn tracks(&self) -> Vec<Track> {
        self.entries.iter().map(|e| e.track.clone()).collect()
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Download {
    pub track: Track,
    pub path: PathBuf,
    pub size_bytes: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HistoryEntry {
    pub track: Track,
    pub played_at: i64,
}

/// Read-only view of the whole library, cheap to clone (`Arc`).
#[derive(Debug, Clone, Default)]
pub struct LibrarySnapshot {
    pub revision: u64,
    pub playlists: Vec<Playlist>,
    /// Downloads by video id.
    pub downloads: HashMap<String, Download>,
    /// Most recent first.
    pub history: Vec<HistoryEntry>,
}

impl LibrarySnapshot {
    pub fn playlist(&self, id: PlaylistId) -> Option<&Playlist> {
        self.playlists.iter().find(|p| p.id == id)
    }

    pub fn is_downloaded(&self, video_id: &str) -> bool {
        self.downloads.contains_key(video_id)
    }
}

pub struct Library {
    conn: Mutex<Connection>,
    snapshot: RwLock<Arc<LibrarySnapshot>>,
    notifier: RwLock<Notifier>,
}

fn now() -> i64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_secs() as i64).unwrap_or_default()
}

fn validate_name(name: &str) -> Result<String> {
    let name = name.trim();
    if name.is_empty() {
        return Err(Error::invalid("playlist name must not be empty"));
    }
    Ok(name.to_owned())
}

fn upsert_track(conn: &Connection, t: &Track) -> Result<()> {
    conn.execute(
        "INSERT INTO tracks (video_id, title, artists_json, album, album_id, duration_s, thumbnail_url, updated_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)
         ON CONFLICT(video_id) DO UPDATE SET
           title = excluded.title, artists_json = excluded.artists_json,
           album = COALESCE(excluded.album, tracks.album), album_id = COALESCE(excluded.album_id, tracks.album_id),
           duration_s = COALESCE(excluded.duration_s, tracks.duration_s),
           thumbnail_url = COALESCE(excluded.thumbnail_url, tracks.thumbnail_url),
           updated_at = excluded.updated_at",
        params![
            t.video_id,
            t.title,
            serde_json::to_string(&t.artists)?,
            t.album.as_ref().map(|a| &a.name),
            t.album.as_ref().map(|a| &a.id),
            t.duration_s,
            t.thumbnail,
            now()
        ],
    )?;
    Ok(())
}

/// Columns: video_id, title, artists_json, album, album_id, duration_s, thumbnail_url starting at `off`.
fn track_from_row(row: &rusqlite::Row<'_>, off: usize) -> rusqlite::Result<Track> {
    let artists_json: String = row.get(off + 2)?;
    let album: Option<String> = row.get(off + 3)?;
    let album_id: Option<String> = row.get(off + 4)?;
    Ok(Track {
        video_id: row.get(off)?,
        title: row.get(off + 1)?,
        artists: serde_json::from_str(&artists_json).unwrap_or_default(),
        album: album.zip(album_id).map(|(name, id)| AlbumRef { id, name }),
        duration_s: row.get(off + 5)?,
        thumbnail: row.get(off + 6)?,
    })
}

const TRACK_COLS: &str = "t.video_id, t.title, t.artists_json, t.album, t.album_id, t.duration_s, t.thumbnail_url";

impl Library {
    pub fn open(path: &Path) -> Result<Self> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        Self::with_connection(Connection::open(path)?)
    }

    pub fn open_in_memory() -> Result<Self> {
        Self::with_connection(Connection::open_in_memory()?)
    }

    fn with_connection(mut conn: Connection) -> Result<Self> {
        db::migrate(&mut conn)?;
        let lib = Self {
            conn: Mutex::new(conn),
            snapshot: RwLock::new(Arc::default()),
            notifier: RwLock::new(Notifier::noop()),
        };
        lib.refresh()?;
        Ok(lib)
    }

    pub fn set_notifier(&self, n: Notifier) {
        *self.notifier.write().unwrap() = n;
    }

    pub fn snapshot(&self) -> Arc<LibrarySnapshot> {
        self.snapshot.read().unwrap().clone()
    }

    fn conn(&self) -> std::sync::MutexGuard<'_, Connection> {
        self.conn.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// Rebuild the snapshot from the database and notify listeners.
    fn refresh(&self) -> Result<()> {
        let conn = self.conn();
        let mut playlists: Vec<Playlist> = conn
            .prepare("SELECT id, name, source_id FROM playlists ORDER BY created_at, id")?
            .query_map([], |r| Ok(Playlist { id: r.get(0)?, name: r.get(1)?, source_id: r.get(2)?, entries: vec![] }))?
            .collect::<rusqlite::Result<_>>()?;
        {
            let mut stmt = conn.prepare(&format!(
                "SELECT i.playlist_id, i.id, {TRACK_COLS} FROM playlist_items i
                 JOIN tracks t ON t.video_id = i.video_id ORDER BY i.playlist_id, i.position"
            ))?;
            let rows = stmt.query_map([], |r| {
                Ok((r.get::<_, PlaylistId>(0)?, PlaylistEntry { id: r.get(1)?, track: track_from_row(r, 2)? }))
            })?;
            let index: HashMap<PlaylistId, usize> = playlists.iter().enumerate().map(|(i, p)| (p.id, i)).collect();
            for row in rows {
                let (pid, entry) = row?;
                if let Some(&i) = index.get(&pid) {
                    playlists[i].entries.push(entry);
                }
            }
        }
        let downloads = conn
            .prepare(&format!(
                "SELECT d.path, d.size_bytes, {TRACK_COLS} FROM downloads d JOIN tracks t ON t.video_id = d.video_id"
            ))?
            .query_map([], |r| {
                let track = track_from_row(r, 2)?;
                Ok((
                    track.video_id.clone(),
                    Download { path: PathBuf::from(r.get::<_, String>(0)?), size_bytes: r.get::<_, i64>(1)? as u64, track },
                ))
            })?
            .collect::<rusqlite::Result<_>>()?;
        let history = conn
            .prepare(&format!(
                "SELECT h.played_at, {TRACK_COLS} FROM history h JOIN tracks t ON t.video_id = h.video_id
                 ORDER BY h.played_at DESC, h.id DESC"
            ))?
            .query_map([], |r| Ok(HistoryEntry { played_at: r.get(0)?, track: track_from_row(r, 1)? }))?
            .collect::<rusqlite::Result<_>>()?;
        drop(conn);

        let mut snap = self.snapshot.write().unwrap();
        let revision = snap.revision + 1;
        *snap = Arc::new(LibrarySnapshot { revision, playlists, downloads, history });
        drop(snap);
        self.notifier.read().unwrap().notify();
        Ok(())
    }

    // ---- playlists ----

    pub fn create_playlist(&self, name: &str) -> Result<PlaylistId> {
        let id = self.insert_playlist(&validate_name(name)?, None)?;
        self.refresh()?;
        Ok(id)
    }

    fn insert_playlist(&self, name: &str, source_id: Option<&str>) -> Result<PlaylistId> {
        let conn = self.conn();
        let t = now();
        conn.execute(
            "INSERT INTO playlists (name, created_at, updated_at, source_id) VALUES (?1, ?2, ?2, ?3)",
            params![name, t, source_id],
        )?;
        Ok(conn.last_insert_rowid())
    }

    pub fn rename_playlist(&self, id: PlaylistId, name: &str) -> Result<()> {
        let name = validate_name(name)?;
        self.conn().execute("UPDATE playlists SET name = ?1, updated_at = ?2 WHERE id = ?3", params![name, now(), id])?;
        self.refresh()
    }

    /// Delete a playlist. Downloaded files are not touched.
    pub fn delete_playlist(&self, id: PlaylistId) -> Result<()> {
        self.conn().execute("DELETE FROM playlists WHERE id = ?1", [id])?;
        self.refresh()
    }

    pub fn playlist_contains(&self, id: PlaylistId, video_id: &str) -> Result<bool> {
        Ok(self.conn().query_row(
            "SELECT EXISTS(SELECT 1 FROM playlist_items WHERE playlist_id = ?1 AND video_id = ?2)",
            params![id, video_id],
            |r| r.get(0),
        )?)
    }

    /// Append tracks to a playlist (duplicates are allowed; check [`Self::playlist_contains`] first).
    pub fn add_tracks(&self, id: PlaylistId, tracks: &[Track]) -> Result<()> {
        self.append_tracks(id, tracks)?;
        self.refresh()
    }

    fn append_tracks(&self, id: PlaylistId, tracks: &[Track]) -> Result<()> {
        let mut conn = self.conn();
        let tx = conn.transaction()?;
        let start: i64 = tx.query_row(
            "SELECT COALESCE(MAX(position) + 1, 0) FROM playlist_items WHERE playlist_id = ?1",
            [id],
            |r| r.get(0),
        )?;
        for (pos, t) in (start..).zip(tracks) {
            upsert_track(&tx, t)?;
            tx.execute(
                "INSERT INTO playlist_items (playlist_id, video_id, position) VALUES (?1, ?2, ?3)",
                params![id, t.video_id, pos],
            )?;
        }
        tx.execute("UPDATE playlists SET updated_at = ?1 WHERE id = ?2", params![now(), id])?;
        tx.commit()?;
        Ok(())
    }

    pub fn remove_entry(&self, id: PlaylistId, entry: EntryId) -> Result<()> {
        self.conn().execute("DELETE FROM playlist_items WHERE playlist_id = ?1 AND id = ?2", params![id, entry])?;
        self.refresh()
    }

    /// Move the entry at index `from` to index `to`, renumbering positions in one transaction.
    pub fn move_entry(&self, id: PlaylistId, from: usize, to: usize) -> Result<()> {
        {
            let mut conn = self.conn();
            let tx = conn.transaction()?;
            let mut ids: Vec<EntryId> = tx
                .prepare("SELECT id FROM playlist_items WHERE playlist_id = ?1 ORDER BY position")?
                .query_map([id], |r| r.get(0))?
                .collect::<rusqlite::Result<_>>()?;
            if from >= ids.len() || to >= ids.len() {
                return Err(Error::invalid("position out of range"));
            }
            let e = ids.remove(from);
            ids.insert(to, e);
            for (pos, entry) in ids.iter().enumerate() {
                tx.execute("UPDATE playlist_items SET position = ?1 WHERE id = ?2", params![pos as i64, entry])?;
            }
            tx.commit()?;
        }
        self.refresh()
    }

    /// Save a remote album/playlist as a new local playlist (snapshot).
    pub fn import_playlist(&self, name: &str, source_id: &str, tracks: &[Track]) -> Result<PlaylistId> {
        let id = self.insert_playlist(&validate_name(name)?, Some(source_id))?;
        self.append_tracks(id, tracks)?;
        self.refresh()?;
        Ok(id)
    }

    // ---- history ----

    pub fn record_play(&self, track: &Track) -> Result<()> {
        {
            let mut conn = self.conn();
            let tx = conn.transaction()?;
            upsert_track(&tx, track)?;
            tx.execute("INSERT INTO history (video_id, played_at) VALUES (?1, ?2)", params![track.video_id, now()])?;
            tx.execute(
                "DELETE FROM history WHERE id NOT IN (SELECT id FROM history ORDER BY played_at DESC, id DESC LIMIT ?1)",
                [HISTORY_LIMIT as i64],
            )?;
            tx.commit()?;
        }
        self.refresh()
    }

    // ---- downloads ----

    pub fn add_download(&self, track: &Track, path: &Path, size_bytes: u64) -> Result<()> {
        {
            let conn = self.conn();
            upsert_track(&conn, track)?;
            conn.execute(
                "INSERT OR REPLACE INTO downloads (video_id, path, size_bytes, completed_at) VALUES (?1, ?2, ?3, ?4)",
                params![track.video_id, path.to_string_lossy(), size_bytes as i64, now()],
            )?;
        }
        self.refresh()
    }

    /// Forget a download (the file itself is handled by the caller).
    pub fn remove_download(&self, video_id: &str) -> Result<()> {
        self.conn().execute("DELETE FROM downloads WHERE video_id = ?1", [video_id])?;
        self.refresh()
    }

    /// Path of a downloaded track if its file still exists; clears the entry otherwise.
    pub fn downloaded_path(&self, video_id: &str) -> Result<Option<PathBuf>> {
        let path: Option<String> = self
            .conn()
            .query_row("SELECT path FROM downloads WHERE video_id = ?1", [video_id], |r| r.get(0))
            .optional()?;
        match path.map(PathBuf::from) {
            Some(p) if p.is_file() => Ok(Some(p)),
            Some(_) => {
                self.remove_download(video_id)?;
                Ok(None)
            }
            None => Ok(None),
        }
    }

    /// Clear entries whose files were removed outside the app. Returns how many were cleared.
    pub fn verify_downloads(&self) -> Result<usize> {
        let missing: Vec<String> = self
            .snapshot()
            .downloads
            .values()
            .filter(|d| !d.path.is_file())
            .map(|d| d.track.video_id.clone())
            .collect();
        if missing.is_empty() {
            return Ok(0);
        }
        {
            let conn = self.conn();
            for id in &missing {
                conn.execute("DELETE FROM downloads WHERE video_id = ?1", [id])?;
            }
        }
        self.refresh()?;
        Ok(missing.len())
    }

    // ---- settings ----

    pub fn setting<T: DeserializeOwned>(&self, key: &str) -> Result<Option<T>> {
        let v: Option<String> =
            self.conn().query_row("SELECT value_json FROM settings WHERE key = ?1", [key], |r| r.get(0)).optional()?;
        Ok(v.and_then(|v| serde_json::from_str(&v).ok()))
    }

    pub fn set_setting<T: Serialize>(&self, key: &str, value: &T) -> Result<()> {
        self.conn().execute(
            "INSERT INTO settings (key, value_json) VALUES (?1, ?2)
             ON CONFLICT(key) DO UPDATE SET value_json = excluded.value_json",
            params![key, serde_json::to_string(value)?],
        )?;
        Ok(())
    }
}

/// Setting keys.
pub mod keys {
    pub const VOLUME: &str = "volume";
    pub const MUTED: &str = "muted";
    pub const DOWNLOAD_DIR: &str = "download_dir";
    pub const DOWNLOAD_CONCURRENCY: &str = "download_concurrency";
    pub const YTDLP_PATH: &str = "ytdlp_path";
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::error::ErrorKind;
    use crate::ytm::fake::track;

    fn lib() -> Library {
        Library::open_in_memory().unwrap()
    }

    fn names(l: &Library, id: PlaylistId) -> Vec<String> {
        l.snapshot().playlist(id).unwrap().entries.iter().map(|e| e.track.video_id.clone()).collect()
    }

    #[test]
    fn create_rename_delete() {
        let l = lib();
        let id = l.create_playlist("  Focus ").unwrap();
        let snap = l.snapshot();
        assert_eq!(snap.playlists.len(), 1);
        assert_eq!(snap.playlists[0].name, "Focus");
        assert!(snap.playlists[0].entries.is_empty());
        l.rename_playlist(id, "Deep Focus").unwrap();
        assert_eq!(l.snapshot().playlists[0].name, "Deep Focus");
        l.delete_playlist(id).unwrap();
        assert!(l.snapshot().playlists.is_empty());
    }

    #[test]
    fn rejects_empty_names() {
        let l = lib();
        assert_eq!(l.create_playlist("   ").unwrap_err().kind, ErrorKind::Invalid);
        let id = l.create_playlist("Focus").unwrap();
        assert_eq!(l.rename_playlist(id, "").unwrap_err().kind, ErrorKind::Invalid);
        assert_eq!(l.snapshot().playlists[0].name, "Focus");
    }

    #[test]
    fn add_detects_duplicates_and_appends() {
        let l = lib();
        let id = l.create_playlist("Focus").unwrap();
        l.add_tracks(id, &[track("a"), track("b")]).unwrap();
        assert!(l.playlist_contains(id, "a").unwrap());
        assert!(!l.playlist_contains(id, "c").unwrap());
        // Confirmed duplicate is appended.
        l.add_tracks(id, &[track("a")]).unwrap();
        assert_eq!(names(&l, id), ["a", "b", "a"]);
    }

    #[test]
    fn remove_and_reorder_persist() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("library.db");
        let id;
        {
            let l = Library::open(&path).unwrap();
            id = l.create_playlist("Focus").unwrap();
            l.add_tracks(id, &[track("a"), track("b"), track("c"), track("d")]).unwrap();
            l.move_entry(id, 2, 0).unwrap(); // third to first
            assert_eq!(names(&l, id), ["c", "a", "b", "d"]);
            let entry = l.snapshot().playlist(id).unwrap().entries[1].id;
            l.remove_entry(id, entry).unwrap();
        }
        // Survives restart, same order.
        let l = Library::open(&path).unwrap();
        assert_eq!(names(&l, id), ["c", "b", "d"]);
        let snap = l.snapshot();
        let t = &snap.playlist(id).unwrap().entries[0].track;
        assert_eq!(t, &track("c"), "track metadata stored offline");
    }

    #[test]
    fn delete_playlist_keeps_downloads() {
        let l = lib();
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("a.m4a");
        std::fs::write(&file, b"x").unwrap();
        let id = l.create_playlist("Focus").unwrap();
        l.add_tracks(id, &[track("a")]).unwrap();
        l.add_download(&track("a"), &file, 1).unwrap();
        l.delete_playlist(id).unwrap();
        assert!(file.exists());
        assert!(l.snapshot().is_downloaded("a"));
    }

    #[test]
    fn import_creates_ordered_snapshot() {
        let l = lib();
        let id = l.import_playlist("Discovery", "MPREb_x", &[track("1"), track("2"), track("3")]).unwrap();
        let snap = l.snapshot();
        let p = snap.playlist(id).unwrap();
        assert_eq!(p.name, "Discovery");
        assert_eq!(p.source_id.as_deref(), Some("MPREb_x"));
        assert_eq!(names(&l, id), ["1", "2", "3"]);
    }

    #[test]
    fn history_most_recent_first_and_trimmed() {
        let l = lib();
        for i in 0..(HISTORY_LIMIT + 5) {
            l.record_play(&track(&i.to_string())).unwrap();
        }
        let snap = l.snapshot();
        assert_eq!(snap.history.len(), HISTORY_LIMIT);
        assert_eq!(snap.history[0].track.video_id, (HISTORY_LIMIT + 4).to_string());
        assert_eq!(snap.history.last().unwrap().track.video_id, "5");
    }

    #[test]
    fn settings_round_trip_across_reopen() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("library.db");
        Library::open(&path).unwrap().set_setting(keys::VOLUME, &0.4f32).unwrap();
        let l = Library::open(&path).unwrap();
        assert_eq!(l.setting::<f32>(keys::VOLUME).unwrap(), Some(0.4));
        assert_eq!(l.setting::<f32>("missing").unwrap(), None);
    }

    #[test]
    fn missing_download_files_are_cleared() {
        let l = lib();
        let dir = tempfile::tempdir().unwrap();
        let a = dir.path().join("a.m4a");
        let b = dir.path().join("b.m4a");
        std::fs::write(&a, b"x").unwrap();
        std::fs::write(&b, b"x").unwrap();
        l.add_download(&track("a"), &a, 1).unwrap();
        l.add_download(&track("b"), &b, 1).unwrap();
        std::fs::remove_file(&a).unwrap();
        assert_eq!(l.verify_downloads().unwrap(), 1);
        assert!(!l.snapshot().is_downloaded("a"));
        std::fs::remove_file(&b).unwrap();
        assert_eq!(l.downloaded_path("b").unwrap(), None);
        assert!(!l.snapshot().is_downloaded("b"));
    }

    #[test]
    fn notifies_on_change() {
        use std::sync::atomic::{AtomicUsize, Ordering};
        let l = lib();
        let n = Arc::new(AtomicUsize::new(0));
        let n2 = n.clone();
        l.set_notifier(Notifier::new(move || {
            n2.fetch_add(1, Ordering::SeqCst);
        }));
        let rev = l.snapshot().revision;
        l.create_playlist("x").unwrap();
        assert_eq!(n.load(Ordering::SeqCst), 1);
        assert_eq!(l.snapshot().revision, rev + 1);
    }
}
