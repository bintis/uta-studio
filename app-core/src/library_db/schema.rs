//! Current SQLite schema and connection configuration.

use rusqlite::Connection;

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
            status TEXT NOT NULL CHECK (status IN ('staged', 'queued', 'analyzing', 'completed', 'failed')),
            analyzing_pct INTEGER,
            failed_message TEXT,
            request_id TEXT,
            engine_request_json TEXT,
            request_digest TEXT,
            engine_plan_json TEXT,
            source_path TEXT,
            source_sha256 TEXT,
            queued_at_ms INTEGER,
            queue_position INTEGER NOT NULL DEFAULT 0
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
            error_message TEXT,
            log_path TEXT,
            cancelled INTEGER NOT NULL DEFAULT 0
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

        CREATE TABLE IF NOT EXISTS analysis_artifacts (
            id TEXT PRIMARY KEY,
            file_hash TEXT NOT NULL,
            kind TEXT NOT NULL,
            path TEXT NOT NULL,
            content_hash TEXT NOT NULL,
            producer_node TEXT NOT NULL,
            input_revisions TEXT NOT NULL,
            config_hash TEXT NOT NULL,
            algorithm_version TEXT NOT NULL,
            created_at_ms INTEGER NOT NULL,
            byte_size INTEGER NOT NULL,
            active INTEGER NOT NULL,
            legacy INTEGER NOT NULL,
            invalidated INTEGER NOT NULL DEFAULT 0,
            pinned INTEGER NOT NULL DEFAULT 0
        );
        CREATE INDEX IF NOT EXISTS idx_analysis_artifacts_song_kind
            ON analysis_artifacts(file_hash, kind);
        CREATE INDEX IF NOT EXISTS idx_analysis_artifacts_active
            ON analysis_artifacts(file_hash, kind, active);

        CREATE TABLE IF NOT EXISTS song_analysis_profiles (
            file_hash TEXT PRIMARY KEY,
            profile_json TEXT NOT NULL,
            updated_at_ms INTEGER NOT NULL
        );

        CREATE TABLE IF NOT EXISTS song_workflows (
            file_hash TEXT PRIMARY KEY,
            workflow_json TEXT NOT NULL,
            updated_at_ms INTEGER NOT NULL
        );

        -- Phase 2/3 (the immutable artifact contract, phase plan §2.3): one
        -- row per real node id that a completed/failed run's
        -- `stage_routes` recorded (i.e. the emitting native worker call site had
        -- migrated to `progress_node`/`artifact_reused`; routes without a
        -- node_id -- pre-Phase-3 call sites -- don't produce a row). A
        -- separate `analysis_runs` table was in the original phase plan's
        -- text, but `analysis_history` already fills that role (run id,
        -- file hash, status, timing, error) and is already relied on
        -- throughout the desktop UI -- duplicating it risked drifting the
        -- two out of sync for no real benefit, so `run_id` here references
        -- `analysis_history.id` directly instead.
        CREATE TABLE IF NOT EXISTS analysis_node_attempts (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            run_id INTEGER NOT NULL REFERENCES analysis_history(id) ON DELETE CASCADE,
            file_hash TEXT NOT NULL,
            node_id TEXT NOT NULL,
            status TEXT NOT NULL,
            progress INTEGER NOT NULL,
            operation TEXT NOT NULL,
            implementation TEXT NOT NULL,
            model TEXT NOT NULL,
            requested_device TEXT NOT NULL,
            actual_device TEXT NOT NULL,
            fallback_from TEXT,
            fallback_reason TEXT,
            backend_fallback_from TEXT,
            backend_fallback_reason TEXT,
            started_at_ms INTEGER,
            finished_at_ms INTEGER
        );
        CREATE INDEX IF NOT EXISTS idx_analysis_node_attempts_run
            ON analysis_node_attempts(run_id);
        CREATE INDEX IF NOT EXISTS idx_analysis_node_attempts_song_node
            ON analysis_node_attempts(file_hash, node_id);

        CREATE TABLE IF NOT EXISTS analysis_node_artifacts (
            run_id INTEGER NOT NULL REFERENCES analysis_history(id) ON DELETE CASCADE,
            attempt_id INTEGER REFERENCES analysis_node_attempts(id) ON DELETE CASCADE,
            node_id TEXT NOT NULL,
            direction TEXT NOT NULL CHECK (direction IN ('input', 'output')),
            slot TEXT NOT NULL,
            artifact_kind TEXT NOT NULL,
            revision_id TEXT,
            binding_kind TEXT NOT NULL,
            PRIMARY KEY (run_id, node_id, direction, slot)
        );
        CREATE INDEX IF NOT EXISTS idx_analysis_node_artifacts_revision
            ON analysis_node_artifacts(revision_id);
        CREATE INDEX IF NOT EXISTS idx_analysis_node_artifacts_run_node
            ON analysis_node_artifacts(run_id, node_id);

        ",
    )?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::ensure_schema;
    use rusqlite::Connection;

    #[test]
    fn empty_database_creates_the_current_schema_and_is_idempotent() {
        let conn = Connection::open_in_memory().unwrap();
        ensure_schema(&conn).unwrap();
        ensure_schema(&conn).unwrap();
        let sql: String = conn
            .query_row(
                "SELECT sql FROM sqlite_master WHERE type = 'table' AND name = 'analysis_queue'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert!(sql.contains("'staged'"));
        assert!(sql.contains("'completed'"));
        let columns: Vec<String> = conn
            .prepare("PRAGMA table_info(analysis_artifacts)")
            .unwrap()
            .query_map([], |row| row.get(1))
            .unwrap()
            .collect::<Result<_, _>>()
            .unwrap();
        assert!(columns.contains(&"invalidated".to_string()));
        assert!(columns.contains(&"pinned".to_string()));
    }
}
