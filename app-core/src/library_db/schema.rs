//! Current SQLite schema and connection configuration.

use rusqlite::Connection;

const SCHEMA_VERSION: i32 = 2;

pub(super) fn configure(conn: &Connection) -> rusqlite::Result<()> {
    conn.execute_batch(
        "
        PRAGMA journal_mode = WAL;
        PRAGMA synchronous = NORMAL;
        PRAGMA foreign_keys = ON;
        PRAGMA cache_size = -64000;
        PRAGMA mmap_size = 268435456;
    ",
    )
}

pub(super) fn ensure_schema(conn: &Connection) -> rusqlite::Result<()> {
    conn.execute_batch(
        "
        CREATE TABLE IF NOT EXISTS library_meta (
            id INTEGER PRIMARY KEY CHECK (id = 1),
            folder TEXT NOT NULL DEFAULT '',
            scan_count INTEGER NOT NULL DEFAULT 0
        );
        INSERT OR IGNORE INTO library_meta (id, folder, scan_count) VALUES (1, '', 0);

        CREATE TABLE IF NOT EXISTS songs (
            id INTEGER PRIMARY KEY,
            path TEXT NOT NULL UNIQUE,
            file_hash TEXT NOT NULL,
            title TEXT NOT NULL,
            artist TEXT NOT NULL,
            album TEXT NOT NULL,
            duration_secs REAL NOT NULL,
            album_art_path TEXT,
            is_analyzed INTEGER NOT NULL,
            language TEXT,
            transcript_source TEXT,
            is_video INTEGER NOT NULL,
            payload TEXT NOT NULL
        );
        CREATE INDEX IF NOT EXISTS idx_songs_file_hash ON songs(file_hash);
        CREATE INDEX IF NOT EXISTS idx_songs_artist_title
            ON songs(artist COLLATE NOCASE, title COLLATE NOCASE);
        CREATE INDEX IF NOT EXISTS idx_songs_album ON songs(album COLLATE NOCASE);

        CREATE VIRTUAL TABLE IF NOT EXISTS songs_fts USING fts5(
            title,
            artist,
            album,
            content = 'songs',
            content_rowid = 'id'
        );

        CREATE TABLE IF NOT EXISTS analysis_queue (
            file_hash TEXT PRIMARY KEY,
            status TEXT NOT NULL CHECK (status IN ('queued', 'analyzing', 'failed')),
            analyzing_pct INTEGER,
            failed_message TEXT
        );

        CREATE TABLE IF NOT EXISTS analysis_history (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            file_hash TEXT NOT NULL,
            title TEXT NOT NULL,
            artist TEXT NOT NULL,
            status TEXT NOT NULL CHECK (status IN ('completed', 'failed')),
            started_at_ms INTEGER NOT NULL,
            finished_at_ms INTEGER NOT NULL,
            snapshot_json TEXT NOT NULL,
            error_message TEXT
        );
        CREATE INDEX IF NOT EXISTS idx_analysis_history_finished
            ON analysis_history(finished_at_ms DESC);
        CREATE INDEX IF NOT EXISTS idx_analysis_history_song
            ON analysis_history(file_hash, finished_at_ms DESC);

        CREATE TABLE IF NOT EXISTS playlists (
            id TEXT PRIMARY KEY,
            name TEXT NOT NULL
        );
        CREATE TABLE IF NOT EXISTS playlist_songs (
            playlist_id TEXT NOT NULL REFERENCES playlists(id) ON DELETE CASCADE,
            song_id INTEGER NOT NULL REFERENCES songs(id) ON DELETE CASCADE,
            position INTEGER NOT NULL,
            PRIMARY KEY (playlist_id, song_id)
        );
        CREATE INDEX IF NOT EXISTS idx_playlist_songs_order
            ON playlist_songs(playlist_id, position);
        CREATE INDEX IF NOT EXISTS idx_playlist_songs_song
            ON playlist_songs(song_id);
        ",
    )?;
    conn.execute(&format!("PRAGMA user_version = {SCHEMA_VERSION}"), [])?;
    Ok(())
}
