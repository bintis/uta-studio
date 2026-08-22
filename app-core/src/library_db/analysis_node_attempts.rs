//! Durable per-node records for a completed or failed analysis run
//! (docs/analysis-dag-redesign.md, phase plan §2.3). One row per real node
//! id a run's `AnalysisStageRoute`s carried a `node_id` for -- routes from
//! a pre-Phase-3 call site (no `node_id`) don't produce a row here, the
//! same "Legacy Adapter" boundary Phase 3 already draws elsewhere.

use rusqlite::params;

use super::connection::{with_conn, with_conn_mut};

pub struct AnalysisNodeAttemptRow {
    pub id: i64,
    pub run_id: i64,
    pub file_hash: String,
    pub node_id: String,
    pub status: String,
    pub progress: i64,
    pub operation: String,
    pub implementation: String,
    pub model: String,
    pub requested_device: String,
    pub actual_device: String,
    pub fallback_from: Option<String>,
    pub fallback_reason: Option<String>,
    pub backend_fallback_from: Option<String>,
    pub backend_fallback_reason: Option<String>,
    pub started_at_ms: Option<i64>,
    pub finished_at_ms: Option<i64>,
}

/// One row to insert, mirroring `AnalysisStageRoute`'s shape without
/// depending on `app_core`'s own types from this low-level storage module
/// (the rest of `library_db` follows the same plain-fields convention).
pub struct NewAnalysisNodeAttempt<'a> {
    pub node_id: &'a str,
    pub status: &'a str,
    pub progress: i64,
    pub operation: &'a str,
    pub implementation: &'a str,
    pub model: &'a str,
    pub requested_device: &'a str,
    pub actual_device: &'a str,
    pub fallback_from: Option<&'a str>,
    pub fallback_reason: Option<&'a str>,
    pub backend_fallback_from: Option<&'a str>,
    pub backend_fallback_reason: Option<&'a str>,
    pub started_at_ms: Option<i64>,
    pub finished_at_ms: Option<i64>,
}

pub fn analysis_node_attempts_insert_batch(
    run_id: i64,
    file_hash: &str,
    attempts: &[NewAnalysisNodeAttempt],
) -> rusqlite::Result<()> {
    if attempts.is_empty() {
        return Ok(());
    }
    with_conn_mut(|connection| {
        for attempt in attempts {
            connection.execute(
                "INSERT INTO analysis_node_attempts (
                    run_id, file_hash, node_id, status, progress, operation,
                    implementation, model, requested_device, actual_device,
                    fallback_from, fallback_reason, backend_fallback_from,
                    backend_fallback_reason, started_at_ms, finished_at_ms
                 ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16)",
                params![
                    run_id,
                    file_hash,
                    attempt.node_id,
                    attempt.status,
                    attempt.progress,
                    attempt.operation,
                    attempt.implementation,
                    attempt.model,
                    attempt.requested_device,
                    attempt.actual_device,
                    attempt.fallback_from,
                    attempt.fallback_reason,
                    attempt.backend_fallback_from,
                    attempt.backend_fallback_reason,
                    attempt.started_at_ms,
                    attempt.finished_at_ms,
                ],
            )?;
        }
        Ok(())
    })
}

pub fn analysis_node_attempts_load(run_id: i64) -> rusqlite::Result<Vec<AnalysisNodeAttemptRow>> {
    with_conn(|connection| {
        let mut statement = connection.prepare(
            "SELECT id, run_id, file_hash, node_id, status, progress, operation,
                    implementation, model, requested_device, actual_device,
                    fallback_from, fallback_reason, backend_fallback_from,
                    backend_fallback_reason, started_at_ms, finished_at_ms
             FROM analysis_node_attempts
             WHERE run_id = ?1
             ORDER BY id ASC",
        )?;
        let rows = statement.query_map(params![run_id], |row| {
            Ok(AnalysisNodeAttemptRow {
                id: row.get(0)?,
                run_id: row.get(1)?,
                file_hash: row.get(2)?,
                node_id: row.get(3)?,
                status: row.get(4)?,
                progress: row.get(5)?,
                operation: row.get(6)?,
                implementation: row.get(7)?,
                model: row.get(8)?,
                requested_device: row.get(9)?,
                actual_device: row.get(10)?,
                fallback_from: row.get(11)?,
                fallback_reason: row.get(12)?,
                backend_fallback_from: row.get(13)?,
                backend_fallback_reason: row.get(14)?,
                started_at_ms: row.get(15)?,
                finished_at_ms: row.get(16)?,
            })
        })?;
        rows.collect()
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::library_db;
    use std::path::Path;

    fn temp_root(label: &str) -> std::path::PathBuf {
        let nonce = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos();
        let path = std::env::temp_dir().join(format!(
            "uta-studio-node-attempts-test-{label}-{}-{nonce}",
            std::process::id()
        ));
        std::fs::create_dir_all(&path).expect("create temp db root");
        path
    }

    #[test]
    fn inserted_attempts_round_trip_for_the_right_run() {
        let root = temp_root("round-trip");
        let _guard = library_db::reconnect_for_test(&root);

        let run_id = library_db::analysis_history_insert(&library_db::NewAnalysisHistory {
            file_hash: "songA",
            title: "Title",
            artist: "Artist",
            status: "completed",
            started_at_ms: 1_000,
            finished_at_ms: 2_000,
            snapshot_json: "{}",
            error_message: None,
            log_path: None,
        })
        .expect("insert run");

        let attempts = vec![
            NewAnalysisNodeAttempt {
                node_id: "stems.separate",
                status: "succeeded",
                progress: 100,
                operation: "Vocal stem separation",
                implementation: "RoFormer",
                model: "mel_band_roformer",
                requested_device: "xpu",
                actual_device: "cpu",
                fallback_from: Some("xpu"),
                fallback_reason: Some("the selected backend is not validated for this model"),
                backend_fallback_from: None,
                backend_fallback_reason: None,
                started_at_ms: None,
                finished_at_ms: None,
            },
            NewAnalysisNodeAttempt {
                node_id: "pitch.extract",
                status: "succeeded",
                progress: 100,
                operation: "Reference pitch extraction",
                implementation: "RMVPE",
                model: "RMVPE singing pitch model",
                requested_device: "cpu",
                actual_device: "cpu",
                fallback_from: None,
                fallback_reason: None,
                backend_fallback_from: None,
                backend_fallback_reason: None,
                started_at_ms: None,
                finished_at_ms: None,
            },
        ];
        analysis_node_attempts_insert_batch(run_id, "songA", &attempts).expect("insert attempts");

        let loaded = analysis_node_attempts_load(run_id).expect("load attempts");
        assert_eq!(loaded.len(), 2);
        assert_eq!(loaded[0].node_id, "stems.separate");
        assert_eq!(loaded[0].file_hash, "songA");
        assert_eq!(loaded[0].fallback_from.as_deref(), Some("xpu"));
        assert_eq!(loaded[1].node_id, "pitch.extract");
        assert_eq!(loaded[1].fallback_from, None);

        cleanup(&root);
    }

    #[test]
    fn loading_a_different_run_id_returns_nothing() {
        let root = temp_root("isolation");
        let _guard = library_db::reconnect_for_test(&root);

        let run_id = library_db::analysis_history_insert(&library_db::NewAnalysisHistory {
            file_hash: "songB",
            title: "Title",
            artist: "Artist",
            status: "completed",
            started_at_ms: 1_000,
            finished_at_ms: 2_000,
            snapshot_json: "{}",
            error_message: None,
            log_path: None,
        })
        .expect("insert run");
        analysis_node_attempts_insert_batch(
            run_id,
            "songB",
            &[NewAnalysisNodeAttempt {
                node_id: "pitch.extract",
                status: "succeeded",
                progress: 100,
                operation: "op",
                implementation: "impl",
                model: "model",
                requested_device: "cpu",
                actual_device: "cpu",
                fallback_from: None,
                fallback_reason: None,
                backend_fallback_from: None,
                backend_fallback_reason: None,
                started_at_ms: None,
                finished_at_ms: None,
            }],
        )
        .unwrap();

        let loaded = analysis_node_attempts_load(run_id + 1).expect("load attempts");
        assert!(loaded.is_empty());

        cleanup(&root);
    }

    #[test]
    fn timing_columns_round_trip_and_default_to_none() {
        let root = temp_root("timing");
        let _guard = library_db::reconnect_for_test(&root);

        let run_id = library_db::analysis_history_insert(&library_db::NewAnalysisHistory {
            file_hash: "songTiming",
            title: "Title",
            artist: "Artist",
            status: "completed",
            started_at_ms: 1_000,
            finished_at_ms: 2_000,
            snapshot_json: "{}",
            error_message: None,
            log_path: None,
        })
        .expect("insert run");
        analysis_node_attempts_insert_batch(
            run_id,
            "songTiming",
            &[
                NewAnalysisNodeAttempt {
                    node_id: "pitch.extract",
                    status: "succeeded",
                    progress: 100,
                    operation: "op",
                    implementation: "impl",
                    model: "model",
                    requested_device: "cpu",
                    actual_device: "cpu",
                    fallback_from: None,
                    fallback_reason: None,
                    backend_fallback_from: None,
                    backend_fallback_reason: None,
                    started_at_ms: Some(1_700_000_000_000),
                    finished_at_ms: Some(1_700_000_004_500),
                },
                NewAnalysisNodeAttempt {
                    node_id: "chart.build_candidate",
                    status: "incomplete",
                    progress: 0,
                    operation: "op",
                    implementation: "impl",
                    model: "model",
                    requested_device: "cpu",
                    actual_device: "cpu",
                    fallback_from: None,
                    fallback_reason: None,
                    backend_fallback_from: None,
                    backend_fallback_reason: None,
                    started_at_ms: None,
                    finished_at_ms: None,
                },
            ],
        )
        .unwrap();

        let loaded = analysis_node_attempts_load(run_id).unwrap();
        let pitch = loaded
            .iter()
            .find(|a| a.node_id == "pitch.extract")
            .unwrap();
        assert_eq!(pitch.started_at_ms, Some(1_700_000_000_000));
        assert_eq!(pitch.finished_at_ms, Some(1_700_000_004_500));
        let never_started = loaded
            .iter()
            .find(|a| a.node_id == "chart.build_candidate")
            .unwrap();
        assert_eq!(never_started.started_at_ms, None);
        assert_eq!(never_started.finished_at_ms, None);

        cleanup(&root);
    }

    #[test]
    fn an_empty_attempts_slice_inserts_nothing_and_does_not_error() {
        let root = temp_root("empty-batch");
        let _guard = library_db::reconnect_for_test(&root);

        let run_id = library_db::analysis_history_insert(&library_db::NewAnalysisHistory {
            file_hash: "songC",
            title: "Title",
            artist: "Artist",
            status: "completed",
            started_at_ms: 1_000,
            finished_at_ms: 2_000,
            snapshot_json: "{}",
            error_message: None,
            log_path: None,
        })
        .expect("insert run");
        analysis_node_attempts_insert_batch(run_id, "songC", &[]).expect("empty batch is ok");

        assert!(analysis_node_attempts_load(run_id).unwrap().is_empty());

        cleanup(&root);
    }

    #[test]
    fn deleting_the_owning_run_cascades_to_its_attempts() {
        let root = temp_root("cascade");
        let _guard = library_db::reconnect_for_test(&root);

        let run_id = library_db::analysis_history_insert(&library_db::NewAnalysisHistory {
            file_hash: "songD",
            title: "Title",
            artist: "Artist",
            status: "completed",
            started_at_ms: 1_000,
            finished_at_ms: 2_000,
            snapshot_json: "{}",
            error_message: None,
            log_path: None,
        })
        .expect("insert run");
        analysis_node_attempts_insert_batch(
            run_id,
            "songD",
            &[NewAnalysisNodeAttempt {
                node_id: "pitch.extract",
                status: "succeeded",
                progress: 100,
                operation: "op",
                implementation: "impl",
                model: "model",
                requested_device: "cpu",
                actual_device: "cpu",
                fallback_from: None,
                fallback_reason: None,
                backend_fallback_from: None,
                backend_fallback_reason: None,
                started_at_ms: None,
                finished_at_ms: None,
            }],
        )
        .unwrap();

        library_db::analysis_history_clear().expect("clear history");

        assert!(analysis_node_attempts_load(run_id).unwrap().is_empty());

        cleanup(&root);
    }

    fn cleanup(root: &Path) {
        let _ = std::fs::remove_dir_all(root);
    }
}
