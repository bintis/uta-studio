//! Persistent analyzer queue.
//!
//! Backs `analyzer::AnalysisQueue`. Each song hash gets one row carrying its
//! current status (`staged`, `queued`, `analyzing` with a percentage, or
//! `failed` with a message). Staged requests require an explicit user start.

use std::path::PathBuf;

use rusqlite::{OptionalExtension, params};

use super::connection::{with_conn, with_conn_mut};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EngineQueueIntent {
    pub file_hash: String,
    pub request_id: String,
    pub request_json: String,
    pub request_digest: String,
    pub plan_json: String,
    pub source_path: PathBuf,
    pub source_sha256: String,
    pub queued_at_ms: i64,
}

type AnalysisQueueRow = (String, String, Option<i64>, Option<String>);

fn upsert_queue_in_tx(
    tx: &rusqlite::Transaction<'_>,
    file_hash: &str,
    status: &str,
    analyzing_pct: Option<i64>,
    failed_message: Option<&str>,
) -> rusqlite::Result<()> {
    tx.execute(
        "INSERT INTO analysis_queue (file_hash, status, analyzing_pct, failed_message)
         VALUES (?1, ?2, ?3, ?4)
         ON CONFLICT(file_hash) DO UPDATE SET
           status = excluded.status,
           analyzing_pct = excluded.analyzing_pct,
           failed_message = excluded.failed_message",
        params![file_hash, status, analyzing_pct, failed_message],
    )?;
    Ok(())
}

pub fn analysis_queue_upsert_row(
    file_hash: &str,
    status: &str,
    analyzing_pct: Option<i64>,
    failed_message: Option<&str>,
) -> rusqlite::Result<()> {
    with_conn_mut(|c| {
        let tx = c.transaction()?;
        upsert_queue_in_tx(&tx, file_hash, status, analyzing_pct, failed_message)?;
        tx.commit()?;
        Ok(())
    })
}

fn analysis_queue_set_engine_intent_with_status(
    intent: &EngineQueueIntent,
    status: &str,
) -> rusqlite::Result<bool> {
    with_conn_mut(|connection| {
        let changed = connection.execute(
            "INSERT INTO analysis_queue (
                file_hash, status, analyzing_pct, failed_message, request_id,
                engine_request_json, request_digest, engine_plan_json,
                source_path, source_sha256, queued_at_ms
             ) VALUES (?1, ?2, NULL, NULL, ?3, ?4, ?5, ?6, ?7, ?8, ?9)
             ON CONFLICT(file_hash) DO UPDATE SET
                status = excluded.status, analyzing_pct = NULL, failed_message = NULL,
                request_id = excluded.request_id,
                engine_request_json = excluded.engine_request_json,
                request_digest = excluded.request_digest,
                engine_plan_json = excluded.engine_plan_json,
                source_path = excluded.source_path,
                source_sha256 = excluded.source_sha256,
                queued_at_ms = excluded.queued_at_ms
             WHERE analysis_queue.status = 'failed'",
            params![
                intent.file_hash,
                status,
                intent.request_id,
                intent.request_json,
                intent.request_digest,
                intent.plan_json,
                intent.source_path.to_string_lossy(),
                intent.source_sha256,
                intent.queued_at_ms,
            ],
        )?;
        Ok(changed == 1)
    })
}

pub fn analysis_queue_set_engine_intent(intent: &EngineQueueIntent) -> rusqlite::Result<bool> {
    analysis_queue_set_engine_intent_with_status(intent, "queued")
}

pub fn analysis_queue_stage_engine_intent(intent: &EngineQueueIntent) -> rusqlite::Result<bool> {
    analysis_queue_set_engine_intent_with_status(intent, "staged")
}

pub fn analysis_queue_status(file_hash: &str) -> rusqlite::Result<Option<String>> {
    with_conn(|connection| {
        connection
            .query_row(
                "SELECT status FROM analysis_queue WHERE file_hash = ?1",
                [file_hash],
                |row| row.get(0),
            )
            .optional()
    })
}

fn queue_status_resumes(status: &str) -> bool {
    matches!(status, "queued" | "analyzing")
}

pub fn analysis_queue_resumable_hashes() -> rusqlite::Result<Vec<String>> {
    with_conn(|connection| {
        let mut statement = connection.prepare("SELECT file_hash, status FROM analysis_queue")?;
        let rows = statement.query_map([], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
        })?;
        Ok(rows
            .collect::<rusqlite::Result<Vec<_>>>()?
            .into_iter()
            .filter_map(|(file_hash, status)| queue_status_resumes(&status).then_some(file_hash))
            .collect())
    })
}

pub fn analysis_queue_engine_intent(
    file_hash: &str,
) -> rusqlite::Result<Option<EngineQueueIntent>> {
    with_conn(|connection| {
        connection
            .query_row(
                "SELECT file_hash, request_id, engine_request_json, request_digest,
                    engine_plan_json, source_path, source_sha256, queued_at_ms
             FROM analysis_queue
             WHERE file_hash = ?1 AND engine_request_json IS NOT NULL",
                [file_hash],
                |row| {
                    Ok(EngineQueueIntent {
                        file_hash: row.get(0)?,
                        request_id: row.get(1)?,
                        request_json: row.get(2)?,
                        request_digest: row.get(3)?,
                        plan_json: row.get(4)?,
                        source_path: PathBuf::from(row.get::<_, String>(5)?),
                        source_sha256: row.get(6)?,
                        queued_at_ms: row.get(7)?,
                    })
                },
            )
            .optional()
    })
}

pub fn analysis_queue_delete(file_hash: &str) -> rusqlite::Result<()> {
    with_conn_mut(|c| {
        c.execute(
            "DELETE FROM analysis_queue WHERE file_hash = ?",
            [file_hash],
        )?;
        Ok(())
    })
}

pub fn analysis_queue_clear() -> rusqlite::Result<()> {
    with_conn_mut(|c| {
        c.execute("DELETE FROM analysis_queue", [])?;
        Ok(())
    })
}

pub fn analysis_queue_load_rows() -> rusqlite::Result<Vec<AnalysisQueueRow>> {
    with_conn(|c| {
        let mut stmt = c.prepare(
            "SELECT file_hash, status, analyzing_pct, failed_message FROM analysis_queue",
        )?;
        let rows = stmt.query_map([], |r| {
            Ok((
                r.get::<_, String>(0)?,
                r.get::<_, String>(1)?,
                r.get::<_, Option<i64>>(2)?,
                r.get::<_, Option<String>>(3)?,
            ))
        })?;
        rows.collect()
    })
}

pub fn analysis_queue_save_rows(rows: &[AnalysisQueueRow]) -> rusqlite::Result<()> {
    with_conn_mut(|c| {
        let tx = c.transaction()?;
        tx.execute("DELETE FROM analysis_queue", [])?;
        for (hash, st, pct, msg) in rows {
            upsert_queue_in_tx(&tx, hash, st.as_str(), *pct, msg.as_deref())?;
        }
        tx.commit()?;
        Ok(())
    })
}

#[cfg(test)]
mod tests {
    use super::queue_status_resumes;

    #[test]
    fn staged_requests_never_resume_until_the_user_starts_them() {
        assert!(!queue_status_resumes("staged"));
        assert!(queue_status_resumes("queued"));
        assert!(queue_status_resumes("analyzing"));
    }
}
