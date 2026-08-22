use rusqlite::{OptionalExtension, params};

use super::connection::{with_conn, with_conn_mut};

pub fn song_workflow_get(file_hash: &str) -> rusqlite::Result<Option<(String, i64)>> {
    with_conn(|connection| {
        connection
            .query_row(
                "SELECT workflow_json, updated_at_ms FROM song_workflows WHERE file_hash = ?1",
                [file_hash],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .optional()
    })
}

pub fn song_workflow_set(
    file_hash: &str,
    workflow_json: &str,
    updated_at_ms: i64,
) -> rusqlite::Result<()> {
    with_conn_mut(|connection| {
        connection.execute(
            "INSERT INTO song_workflows (file_hash, workflow_json, updated_at_ms)
             VALUES (?1, ?2, ?3)
             ON CONFLICT(file_hash) DO UPDATE SET
               workflow_json = excluded.workflow_json,
               updated_at_ms = excluded.updated_at_ms",
            params![file_hash, workflow_json, updated_at_ms],
        )?;
        Ok(())
    })
}
