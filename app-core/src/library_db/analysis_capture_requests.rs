use rusqlite::{OptionalExtension, params};

use super::connection::{with_conn, with_conn_mut};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AnalysisCaptureRequestRow {
    pub file_hash: String,
    pub node_id: String,
    pub artifact_kind: String,
    pub persistent: bool,
    pub created_at_ms: i64,
}

pub fn analysis_capture_request_get(
    file_hash: &str,
    node_id: &str,
    artifact_kind: &str,
) -> rusqlite::Result<Option<AnalysisCaptureRequestRow>> {
    with_conn(|connection| {
        connection
            .query_row(
                "SELECT file_hash, node_id, artifact_kind, persistent, created_at_ms
                 FROM analysis_capture_requests
                 WHERE file_hash = ?1 AND node_id = ?2 AND artifact_kind = ?3",
                params![file_hash, node_id, artifact_kind],
                |row| {
                    Ok(AnalysisCaptureRequestRow {
                        file_hash: row.get(0)?,
                        node_id: row.get(1)?,
                        artifact_kind: row.get(2)?,
                        persistent: row.get::<_, i64>(3)? != 0,
                        created_at_ms: row.get(4)?,
                    })
                },
            )
            .optional()
    })
}

pub fn analysis_capture_request_upsert(row: &AnalysisCaptureRequestRow) -> rusqlite::Result<()> {
    with_conn_mut(|connection| {
        connection.execute(
            "INSERT INTO analysis_capture_requests
             (file_hash, node_id, artifact_kind, persistent, created_at_ms)
             VALUES (?1, ?2, ?3, ?4, ?5)
             ON CONFLICT(file_hash, node_id, artifact_kind) DO UPDATE SET
               persistent = excluded.persistent,
               created_at_ms = excluded.created_at_ms",
            params![
                row.file_hash,
                row.node_id,
                row.artifact_kind,
                row.persistent as i64,
                row.created_at_ms,
            ],
        )?;
        Ok(())
    })
}

pub fn analysis_capture_request_delete(
    file_hash: &str,
    node_id: &str,
    artifact_kind: &str,
) -> rusqlite::Result<()> {
    with_conn_mut(|connection| {
        connection.execute(
            "DELETE FROM analysis_capture_requests
             WHERE file_hash = ?1 AND node_id = ?2 AND artifact_kind = ?3",
            params![file_hash, node_id, artifact_kind],
        )?;
        Ok(())
    })
}
