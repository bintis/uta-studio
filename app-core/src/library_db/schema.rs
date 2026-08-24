//! Current SQLite schema and connection configuration.

use rusqlite::Connection;

pub(crate) const SCHEMA_VERSION: i32 = 13;

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
            failed_message TEXT,
            request_id TEXT,
            engine_request_json TEXT,
            request_digest TEXT,
            engine_plan_json TEXT,
            source_path TEXT,
            source_sha256 TEXT,
            queued_at_ms INTEGER
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

        -- Phase 2/3 (docs/analysis-dag-redesign.md, phase plan §2.3): one
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

        CREATE TABLE IF NOT EXISTS analysis_capture_requests (
            file_hash TEXT NOT NULL,
            node_id TEXT NOT NULL,
            artifact_kind TEXT NOT NULL,
            persistent INTEGER NOT NULL DEFAULT 0,
            created_at_ms INTEGER NOT NULL,
            PRIMARY KEY (file_hash, node_id, artifact_kind)
        );
        ",
    )?;
    // SCHEMA_VERSION 4 -> 5: Phase 6 `invalidate_analysis_artifact` /
    // Phase 7 §7.6 "Invalidate" needs a per-revision flag distinct from
    // `active`/`legacy`. `analysis_artifacts` already existed for anyone
    // upgrading from an earlier build, so (unlike every table above) a
    // plain `CREATE TABLE IF NOT EXISTS` can't add this column to their
    // existing rows -- `ALTER TABLE ADD COLUMN` is the first schema change
    // in this codebase that isn't a brand new table, hence the explicit
    // existence check (SQLite has no `ADD COLUMN IF NOT EXISTS`).
    if !column_exists(conn, "analysis_artifacts", "invalidated")? {
        conn.execute_batch(
            "ALTER TABLE analysis_artifacts ADD COLUMN invalidated INTEGER NOT NULL DEFAULT 0;",
        )?;
    }
    // SCHEMA_VERSION 5 -> 6: Phase 7 "Duration 检查器字段" gap closed --
    // per-node Start/Finish wall-clock timestamps
    // (native worker progress frames). Nullable (not `DEFAULT 0`, unlike
    // `invalidated` above): 0 would read as a real Unix-epoch timestamp
    // instead of "unknown," and every row recorded before this migration
    // genuinely has no timing data to backfill.
    if !column_exists(conn, "analysis_node_attempts", "started_at_ms")? {
        conn.execute_batch(
            "ALTER TABLE analysis_node_attempts ADD COLUMN started_at_ms INTEGER;
             ALTER TABLE analysis_node_attempts ADD COLUMN finished_at_ms INTEGER;",
        )?;
    }
    // SCHEMA_VERSION 6 -> 7/8: pin protection plus exact node I/O bindings.
    if !column_exists(conn, "analysis_artifacts", "pinned")? {
        conn.execute_batch(
            "ALTER TABLE analysis_artifacts ADD COLUMN pinned INTEGER NOT NULL DEFAULT 0;",
        )?;
    }
    // SCHEMA_VERSION 8 -> 9: bind every new node I/O row to the concrete
    // attempt that consumed or produced it. Nullable preserves old history,
    // which remains explicitly LegacyUntracked rather than fabricated.
    if !column_exists(conn, "analysis_node_artifacts", "attempt_id")? {
        conn.execute_batch(
            "ALTER TABLE analysis_node_artifacts ADD COLUMN attempt_id INTEGER REFERENCES analysis_node_attempts(id) ON DELETE CASCADE;",
        )?;
    }
    // SCHEMA_VERSION 10 -> 11: a log belongs to one durable run even when
    // old snapshot JSON does not carry its path. Cancellation is stored as
    // a separate flag so existing databases keep their original CHECK
    // constraint while callers can expose `cancelled` as a real history
    // status instead of mislabelling it as a model failure.
    if !column_exists(conn, "analysis_history", "log_path")? {
        conn.execute_batch("ALTER TABLE analysis_history ADD COLUMN log_path TEXT;")?;
    }
    if !column_exists(conn, "analysis_history", "cancelled")? {
        conn.execute_batch(
            "ALTER TABLE analysis_history ADD COLUMN cancelled INTEGER NOT NULL DEFAULT 0;",
        )?;
    }
    // SCHEMA_VERSION 12 -> 13: exact Engine preview snapshots are durable
    // queue intent. Nullable columns preserve every legacy queue row.
    for (column, declaration) in [
        ("request_id", "TEXT"),
        ("engine_request_json", "TEXT"),
        ("request_digest", "TEXT"),
        ("engine_plan_json", "TEXT"),
        ("source_path", "TEXT"),
        ("source_sha256", "TEXT"),
        ("queued_at_ms", "INTEGER"),
    ] {
        if !column_exists(conn, "analysis_queue", column)? {
            conn.execute(
                &format!("ALTER TABLE analysis_queue ADD COLUMN {column} {declaration}"),
                [],
            )?;
        }
    }
    conn.execute(&format!("PRAGMA user_version = {SCHEMA_VERSION}"), [])?;
    Ok(())
}

fn column_exists(conn: &Connection, table: &str, column: &str) -> rusqlite::Result<bool> {
    let mut stmt = conn.prepare(&format!("PRAGMA table_info({table})"))?;
    let mut rows = stmt.query([])?;
    while let Some(row) = rows.next()? {
        let name: String = row.get(1)?;
        if name == column {
            return Ok(true);
        }
    }
    Ok(false)
}

#[cfg(test)]
mod tests {
    use super::{column_exists, ensure_schema};
    use rusqlite::Connection;

    /// The real regression this guards: SCHEMA_VERSION 4 -> 5 added
    /// `invalidated` to a table that already existed for anyone who
    /// installed before this change -- `CREATE TABLE IF NOT EXISTS` is a
    /// no-op against their already-created table, so without the explicit
    /// `ALTER TABLE` migration their `analysis_artifacts` would silently
    /// stay on the old shape forever and every `invalidate_artifact_
    /// revision` call would fail with "no such column". This test
    /// reproduces exactly that pre-migration state by hand (the old
    /// `CREATE TABLE` shape, minus `invalidated`) before calling
    /// `ensure_schema`, rather than trusting a fresh DB (which would pass
    /// even if the `ALTER TABLE` step were silently deleted).
    #[test]
    fn ensure_schema_adds_the_invalidated_column_to_a_pre_existing_table() {
        let conn = Connection::open_in_memory().unwrap();
        conn.execute_batch(
            "CREATE TABLE analysis_artifacts (
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
                legacy INTEGER NOT NULL
            );",
        )
        .unwrap();
        conn.execute(
            "INSERT INTO analysis_artifacts (
                id, file_hash, kind, path, content_hash, producer_node,
                input_revisions, config_hash, algorithm_version, created_at_ms,
                byte_size, active, legacy
             ) VALUES ('id1', 'hashA', 'vocal_stem', 'p', 'c', 'n', '[]', 'cfg', '1', 0, 0, 0, 0)",
            [],
        )
        .unwrap();
        assert!(!column_exists(&conn, "analysis_artifacts", "invalidated").unwrap());

        ensure_schema(&conn).unwrap();

        assert!(column_exists(&conn, "analysis_artifacts", "invalidated").unwrap());
        let invalidated: i64 = conn
            .query_row(
                "SELECT invalidated FROM analysis_artifacts WHERE id = 'id1'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(
            invalidated, 0,
            "a pre-existing row must default to not-invalidated"
        );
    }

    #[test]
    fn ensure_schema_is_idempotent_when_the_column_already_exists() {
        let conn = Connection::open_in_memory().unwrap();
        ensure_schema(&conn).unwrap();
        // Calling it again (e.g. every time the app opens the DB) must not
        // error on "duplicate column name".
        ensure_schema(&conn).unwrap();
        assert!(column_exists(&conn, "analysis_artifacts", "invalidated").unwrap());
    }

    /// Same regression shape as the `invalidated` migration test above,
    /// for SCHEMA_VERSION 5 -> 6 (`started_at_ms`/`finished_at_ms` on
    /// `analysis_node_attempts`, Phase 7's Duration field).
    #[test]
    fn ensure_schema_adds_the_timing_columns_to_a_pre_existing_table() {
        let conn = Connection::open_in_memory().unwrap();
        // Deliberately does *not* pre-create `analysis_history`: this
        // connection never runs `configure()`, so `PRAGMA foreign_keys` is
        // off and the FK reference below is never validated -- only
        // `analysis_node_attempts`'s own pre-migration shape matters for
        // this test. Pre-creating a minimal `analysis_history` here would
        // itself be a trap: `ensure_schema`'s own `CREATE TABLE IF NOT
        // EXISTS analysis_history` would then skip creating the *real*
        // one, and its `CREATE INDEX ... (finished_at_ms DESC)` would fail
        // against a fixture table missing that column.
        conn.execute_batch(
            "CREATE TABLE analysis_node_attempts (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                run_id INTEGER NOT NULL,
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
                backend_fallback_reason TEXT
            );",
        )
        .unwrap();
        conn.execute(
            "INSERT INTO analysis_node_attempts (
                run_id, file_hash, node_id, status, progress, operation,
                implementation, model, requested_device, actual_device
             ) VALUES (1, 'hashA', 'pitch.extract', 'succeeded', 100, 'op', 'impl', 'model', 'cpu', 'cpu')",
            [],
        )
        .unwrap();
        assert!(!column_exists(&conn, "analysis_node_attempts", "started_at_ms").unwrap());

        ensure_schema(&conn).unwrap();

        assert!(column_exists(&conn, "analysis_node_attempts", "started_at_ms").unwrap());
        assert!(column_exists(&conn, "analysis_node_attempts", "finished_at_ms").unwrap());
        let started_at_ms: Option<i64> = conn
            .query_row(
                "SELECT started_at_ms FROM analysis_node_attempts WHERE run_id = 1",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(
            started_at_ms, None,
            "a pre-existing row has no timing data to backfill"
        );
    }

    #[test]
    fn ensure_schema_adds_attempt_id_to_schema_v8_bindings_idempotently() {
        let conn = Connection::open_in_memory().unwrap();
        conn.execute_batch(
            "CREATE TABLE analysis_node_artifacts (
                run_id INTEGER NOT NULL,
                node_id TEXT NOT NULL,
                direction TEXT NOT NULL,
                slot TEXT NOT NULL,
                artifact_kind TEXT NOT NULL,
                revision_id TEXT,
                binding_kind TEXT NOT NULL,
                PRIMARY KEY (run_id, node_id, direction, slot)
            );",
        )
        .unwrap();
        assert!(!column_exists(&conn, "analysis_node_artifacts", "attempt_id").unwrap());
        ensure_schema(&conn).unwrap();
        ensure_schema(&conn).unwrap();
        assert!(column_exists(&conn, "analysis_node_artifacts", "attempt_id").unwrap());
    }

    #[test]
    fn ensure_schema_repairs_a_partially_applied_history_log_migration() {
        let conn = Connection::open_in_memory().unwrap();
        conn.execute_batch(
            "CREATE TABLE analysis_history (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                file_hash TEXT NOT NULL,
                title TEXT NOT NULL,
                artist TEXT NOT NULL,
                status TEXT NOT NULL CHECK (status IN ('completed', 'failed')),
                started_at_ms INTEGER NOT NULL,
                finished_at_ms INTEGER NOT NULL,
                snapshot_json TEXT NOT NULL,
                error_message TEXT,
                log_path TEXT
            );",
        )
        .unwrap();
        assert!(column_exists(&conn, "analysis_history", "log_path").unwrap());
        assert!(!column_exists(&conn, "analysis_history", "cancelled").unwrap());

        ensure_schema(&conn).unwrap();
        ensure_schema(&conn).unwrap();

        assert!(column_exists(&conn, "analysis_history", "cancelled").unwrap());
    }
}
