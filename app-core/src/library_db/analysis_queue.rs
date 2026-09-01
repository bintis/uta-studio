//! Persistent analyzer queue.
//!
//! Backs `analyzer::AnalysisQueue`. Each song hash gets one row carrying its
//! current status (`staged`, `queued`, `analyzing`, `completed`, or `failed`).
//! Staged requests require an explicit user start; terminal rows remain until removed or rerun.

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
        "INSERT INTO analysis_queue (
             file_hash, status, analyzing_pct, failed_message, queue_position
         )
         VALUES (
             ?1, ?2, ?3, ?4,
             COALESCE((SELECT MAX(queue_position) + 1 FROM analysis_queue), 0)
         )
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
                source_path, source_sha256, queued_at_ms, queue_position
             ) VALUES (
                ?1, ?2, NULL, NULL, ?3, ?4, ?5, ?6, ?7, ?8, ?9,
                COALESCE((SELECT MAX(queue_position) + 1 FROM analysis_queue), 0)
             )
             ON CONFLICT(file_hash) DO UPDATE SET
                status = excluded.status, analyzing_pct = NULL, failed_message = NULL,
                request_id = excluded.request_id,
                engine_request_json = excluded.engine_request_json,
                request_digest = excluded.request_digest,
                engine_plan_json = excluded.engine_plan_json,
                source_path = excluded.source_path,
                source_sha256 = excluded.source_sha256,
                queued_at_ms = excluded.queued_at_ms,
                queue_position = excluded.queue_position
             WHERE analysis_queue.status IN ('failed', 'completed')",
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

/// Replaces the exact request behind a manually staged queue item while
/// retaining its user-selected queue position. A queued/running item cannot
/// be rewritten underneath the scheduler.
pub fn analysis_queue_replace_staged_engine_intent(
    intent: &EngineQueueIntent,
) -> rusqlite::Result<bool> {
    with_conn_mut(|connection| {
        let changed = connection.execute(
            "UPDATE analysis_queue SET
                request_id = ?2,
                engine_request_json = ?3,
                request_digest = ?4,
                engine_plan_json = ?5,
                source_path = ?6,
                source_sha256 = ?7,
                queued_at_ms = ?8,
                analyzing_pct = NULL,
                failed_message = NULL
             WHERE file_hash = ?1 AND status = 'staged'",
            params![
                intent.file_hash,
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
        let mut statement = connection.prepare(
            "SELECT file_hash, status FROM analysis_queue
             ORDER BY queue_position, COALESCE(queued_at_ms, 0), rowid",
        )?;
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

pub fn analysis_queue_ordered_hashes() -> rusqlite::Result<Vec<String>> {
    with_conn(|connection| {
        let mut statement = connection.prepare(
            "SELECT file_hash FROM analysis_queue
             ORDER BY queue_position, COALESCE(queued_at_ms, 0), rowid",
        )?;
        statement
            .query_map([], |row| row.get::<_, String>(0))?
            .collect()
    })
}

pub fn analysis_queue_move(file_hash: &str, earlier: bool) -> rusqlite::Result<bool> {
    with_conn_mut(|connection| {
        let transaction = connection.transaction()?;
        let rows = {
            let mut statement = transaction.prepare(
                "SELECT file_hash, queue_position FROM analysis_queue
                 WHERE status IN ('staged', 'queued')
                 ORDER BY queue_position, COALESCE(queued_at_ms, 0), rowid",
            )?;
            statement
                .query_map([], |row| {
                    Ok((row.get::<_, String>(0)?, row.get::<_, i64>(1)?))
                })?
                .collect::<rusqlite::Result<Vec<_>>>()?
        };
        let Some(index) = rows.iter().position(|(hash, _)| hash == file_hash) else {
            return Ok(false);
        };
        let target = if earlier {
            index.checked_sub(1)
        } else if index + 1 < rows.len() {
            Some(index + 1)
        } else {
            None
        };
        let Some(target) = target else {
            return Ok(false);
        };
        transaction.execute(
            "UPDATE analysis_queue SET queue_position = ?1 WHERE file_hash = ?2",
            params![rows[target].1, rows[index].0],
        )?;
        transaction.execute(
            "UPDATE analysis_queue SET queue_position = ?1 WHERE file_hash = ?2",
            params![rows[index].1, rows[target].0],
        )?;
        transaction.commit()?;
        Ok(true)
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
            "SELECT file_hash, status, analyzing_pct, failed_message FROM analysis_queue
             ORDER BY queue_position, COALESCE(queued_at_ms, 0), rowid",
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
    use super::*;

    fn temp_root(label: &str) -> PathBuf {
        let root = std::env::temp_dir().join(format!(
            "uta-studio-queue-test-{label}-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_nanos()
        ));
        std::fs::create_dir_all(&root).unwrap();
        root
    }

    fn intent(file_hash: &str, queued_at_ms: i64) -> EngineQueueIntent {
        EngineQueueIntent {
            file_hash: file_hash.to_string(),
            request_id: format!("request-{file_hash}"),
            request_json: format!("{{\"file\":\"{file_hash}\"}}"),
            request_digest: format!("identity-{file_hash}"),
            plan_json: "{}".to_string(),
            source_path: PathBuf::from(format!("/{file_hash}.flac")),
            source_sha256: format!("source-{file_hash}"),
            queued_at_ms,
        }
    }

    #[test]
    fn staged_requests_never_resume_until_the_user_starts_them() {
        assert!(!queue_status_resumes("staged"));
        assert!(queue_status_resumes("queued"));
        assert!(queue_status_resumes("analyzing"));
        assert!(!queue_status_resumes("completed"));
        assert!(!queue_status_resumes("failed"));
    }

    #[test]
    fn user_order_and_staged_request_replacement_are_durable() {
        let root = temp_root("order");
        let _guard = crate::library_db::reconnect_for_test(&root);
        analysis_queue_stage_engine_intent(&intent("one", 1)).unwrap();
        analysis_queue_stage_engine_intent(&intent("two", 2)).unwrap();
        analysis_queue_stage_engine_intent(&intent("three", 3)).unwrap();

        assert!(analysis_queue_move("three", true).unwrap());
        assert_eq!(
            analysis_queue_ordered_hashes().unwrap(),
            ["one", "three", "two"]
        );

        let mut edited = intent("three", 99);
        edited.request_id = "request-three-edited".to_string();
        assert!(analysis_queue_replace_staged_engine_intent(&edited).unwrap());
        assert_eq!(
            analysis_queue_ordered_hashes().unwrap(),
            ["one", "three", "two"]
        );
        assert_eq!(
            analysis_queue_engine_intent("three")
                .unwrap()
                .unwrap()
                .request_id,
            "request-three-edited"
        );
    }

    #[test]
    fn completed_row_accepts_a_fresh_exact_rerun_intent() {
        let root = temp_root("completed-rerun");
        let _guard = crate::library_db::reconnect_for_test(&root);
        analysis_queue_upsert_row("song", "completed", None, None).unwrap();

        let mut rerun = intent("song", 2);
        rerun.request_id = "request-song-rerun".to_string();
        assert!(analysis_queue_set_engine_intent(&rerun).unwrap());
        assert_eq!(
            analysis_queue_status("song").unwrap().as_deref(),
            Some("queued")
        );
        assert_eq!(
            analysis_queue_engine_intent("song")
                .unwrap()
                .unwrap()
                .request_id,
            "request-song-rerun"
        );
    }
}
