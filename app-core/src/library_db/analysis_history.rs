//! Durable records of completed and failed analysis sessions.

use rusqlite::params;

use super::connection::{with_conn, with_conn_mut};

pub struct AnalysisHistoryRow {
    pub id: i64,
    pub file_hash: String,
    pub title: String,
    pub artist: String,
    pub status: String,
    pub started_at_ms: i64,
    pub finished_at_ms: i64,
    pub snapshot_json: String,
    pub error_message: Option<String>,
}

pub fn analysis_history_insert(
    file_hash: &str,
    title: &str,
    artist: &str,
    status: &str,
    started_at_ms: i64,
    finished_at_ms: i64,
    snapshot_json: &str,
    error_message: Option<&str>,
) -> rusqlite::Result<()> {
    with_conn_mut(|connection| {
        connection.execute(
            "INSERT INTO analysis_history (
                file_hash, title, artist, status, started_at_ms,
                finished_at_ms, snapshot_json, error_message
             ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
            params![
                file_hash,
                title,
                artist,
                status,
                started_at_ms,
                finished_at_ms,
                snapshot_json,
                error_message,
            ],
        )?;
        Ok(())
    })
}

pub fn analysis_history_load(limit: usize) -> rusqlite::Result<Vec<AnalysisHistoryRow>> {
    with_conn(|connection| {
        let mut statement = connection.prepare(
            "SELECT id, file_hash, title, artist, status, started_at_ms,
                    finished_at_ms, snapshot_json, error_message
             FROM analysis_history
             ORDER BY finished_at_ms DESC, id DESC
             LIMIT ?1",
        )?;
        let rows = statement.query_map([limit.clamp(1, 500) as i64], |row| {
            Ok(AnalysisHistoryRow {
                id: row.get(0)?,
                file_hash: row.get(1)?,
                title: row.get(2)?,
                artist: row.get(3)?,
                status: row.get(4)?,
                started_at_ms: row.get(5)?,
                finished_at_ms: row.get(6)?,
                snapshot_json: row.get(7)?,
                error_message: row.get(8)?,
            })
        })?;
        rows.collect()
    })
}

pub fn analysis_history_clear() -> rusqlite::Result<()> {
    with_conn_mut(|connection| {
        connection.execute("DELETE FROM analysis_history", [])?;
        Ok(())
    })
}
