//! Persistent analyzer queue.
//!
//! Backs `analyzer::AnalysisQueue`. Each song hash gets one row carrying its
//! current status (`queued`, `analyzing` with a percentage, or `failed` with a
//! message).

use rusqlite::params;

use super::connection::{with_conn, with_conn_mut};

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

pub fn analysis_queue_load_rows()
-> rusqlite::Result<Vec<(String, String, Option<i64>, Option<String>)>> {
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

pub fn analysis_queue_save_rows(
    rows: &[(String, String, Option<i64>, Option<String>)],
) -> rusqlite::Result<()> {
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
