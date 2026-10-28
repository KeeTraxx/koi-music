//! Database schema and migrations.

use rusqlite::Connection;
use rusqlite_migration::{M, Migrations};

use crate::error::Result;

const MIGRATIONS: &[M<'static>] = &[M::up(
    "CREATE TABLE tracks (
        video_id      TEXT PRIMARY KEY,
        title         TEXT NOT NULL,
        artists_json  TEXT NOT NULL,
        album         TEXT,
        album_id      TEXT,
        duration_s    INTEGER,
        thumbnail_url TEXT,
        updated_at    INTEGER NOT NULL
    );
    CREATE TABLE playlists (
        id         INTEGER PRIMARY KEY,
        name       TEXT NOT NULL,
        created_at INTEGER NOT NULL,
        updated_at INTEGER NOT NULL,
        source_id  TEXT
    );
    CREATE TABLE playlist_items (
        id          INTEGER PRIMARY KEY,
        playlist_id INTEGER NOT NULL REFERENCES playlists(id) ON DELETE CASCADE,
        video_id    TEXT NOT NULL REFERENCES tracks(video_id),
        position    INTEGER NOT NULL
    );
    CREATE INDEX playlist_items_by_playlist ON playlist_items(playlist_id, position);
    CREATE TABLE downloads (
        video_id     TEXT PRIMARY KEY REFERENCES tracks(video_id),
        path         TEXT NOT NULL,
        size_bytes   INTEGER NOT NULL,
        completed_at INTEGER NOT NULL
    );
    CREATE TABLE history (
        id        INTEGER PRIMARY KEY,
        video_id  TEXT NOT NULL REFERENCES tracks(video_id),
        played_at INTEGER NOT NULL
    );
    CREATE TABLE settings (
        key        TEXT PRIMARY KEY,
        value_json TEXT NOT NULL
    );",
)];

pub fn migrate(conn: &mut Connection) -> Result<()> {
    conn.pragma_update(None, "foreign_keys", "ON")?;
    conn.pragma_update(None, "journal_mode", "WAL")?;
    Migrations::from_slice(MIGRATIONS).to_latest(conn)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn migrations_are_valid() {
        assert!(Migrations::from_slice(MIGRATIONS).validate().is_ok());
    }

    #[test]
    fn migrations_apply_idempotently() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("library.db");
        for _ in 0..2 {
            let mut conn = Connection::open(&path).unwrap();
            migrate(&mut conn).unwrap();
        }
        let conn = Connection::open(&path).unwrap();
        let tables: i64 = conn
            .query_row("SELECT count(*) FROM sqlite_master WHERE type = 'table' AND name NOT LIKE 'sqlite_%' AND name != '_rusqlite_migration'", [], |r| r.get(0))
            .unwrap();
        assert!(tables >= 6, "{tables}");
    }
}
