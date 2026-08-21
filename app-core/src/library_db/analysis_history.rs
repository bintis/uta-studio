//! Durable records of completed and failed analysis sessions.

use rusqlite::params;
use std::path::{Path, PathBuf};

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
    pub log_path: Option<PathBuf>,
}

pub struct NewAnalysisHistory<'a> {
    pub file_hash: &'a str,
    pub title: &'a str,
    pub artist: &'a str,
    pub status: &'a str,
    pub started_at_ms: i64,
    pub finished_at_ms: i64,
    pub snapshot_json: &'a str,
    pub error_message: Option<&'a str>,
    pub log_path: Option<&'a Path>,
}

/// Returns the new row's id -- needed so a caller can attach
/// `analysis_node_attempts` rows (phase plan §2.3) to the run that
/// produced them.
pub fn analysis_history_insert(run: &NewAnalysisHistory<'_>) -> rusqlite::Result<i64> {
    with_conn_mut(|connection| {
        connection.execute(
            "INSERT INTO analysis_history (
                file_hash, title, artist, status, started_at_ms,
                finished_at_ms, snapshot_json, error_message, log_path, cancelled
             ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)",
            params![
                run.file_hash,
                run.title,
                run.artist,
                if run.status == "cancelled" {
                    "failed"
                } else {
                    run.status
                },
                run.started_at_ms,
                run.finished_at_ms,
                run.snapshot_json,
                run.error_message,
                run.log_path.map(|path| path.to_string_lossy().into_owned()),
                i64::from(run.status == "cancelled"),
            ],
        )?;
        Ok(connection.last_insert_rowid())
    })
}

pub fn analysis_history_load(limit: usize) -> rusqlite::Result<Vec<AnalysisHistoryRow>> {
    with_conn(|connection| {
        let mut statement = connection.prepare(
            "SELECT id, file_hash, title, artist, status, started_at_ms,
                    finished_at_ms, snapshot_json, error_message, log_path, cancelled
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
                status: if row.get::<_, i64>(10)? != 0 {
                    "cancelled".to_string()
                } else {
                    row.get(4)?
                },
                started_at_ms: row.get(5)?,
                finished_at_ms: row.get(6)?,
                snapshot_json: row.get(7)?,
                error_message: row.get(8)?,
                log_path: row.get::<_, Option<String>>(9)?.map(PathBuf::from),
            })
        })?;
        rows.collect()
    })
}

pub fn analysis_history_clear() -> rusqlite::Result<Vec<PathBuf>> {
    with_conn_mut(|connection| {
        let transaction = connection.transaction()?;
        let log_paths = {
            let mut statement = transaction
                .prepare("SELECT log_path FROM analysis_history WHERE log_path IS NOT NULL")?;
            let rows = statement.query_map([], |row| row.get::<_, String>(0))?;
            rows.collect::<rusqlite::Result<Vec<_>>>()?
                .into_iter()
                .map(PathBuf::from)
                .collect()
        };
        transaction.execute("DELETE FROM analysis_history", [])?;
        transaction.commit()?;
        Ok(log_paths)
    })
}

pub fn analysis_history_set_error(run_id: i64, message: &str) -> rusqlite::Result<()> {
    with_conn_mut(|connection| {
        connection.execute(
            "UPDATE analysis_history SET error_message = ?1 WHERE id = ?2",
            params![message, run_id],
        )?;
        Ok(())
    })
}
