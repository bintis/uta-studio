//! Exact run/node -> artifact bindings for the Artifact Workbench.

use rusqlite::params;

use super::AnalysisArtifactRow;
use super::connection::{with_conn, with_conn_mut};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AnalysisNodeArtifactRow {
    pub run_id: i64,
    pub attempt_id: Option<i64>,
    pub node_id: String,
    pub direction: String,
    pub slot: String,
    pub artifact_kind: String,
    pub revision_id: Option<String>,
    pub binding_kind: String,
}

fn row_from_query(row: &rusqlite::Row) -> rusqlite::Result<AnalysisNodeArtifactRow> {
    Ok(AnalysisNodeArtifactRow {
        run_id: row.get(0)?,
        attempt_id: row.get(1)?,
        node_id: row.get(2)?,
        direction: row.get(3)?,
        slot: row.get(4)?,
        artifact_kind: row.get(5)?,
        revision_id: row.get(6)?,
        binding_kind: row.get(7)?,
    })
}

pub fn analysis_node_artifact_upsert(row: &AnalysisNodeArtifactRow) -> rusqlite::Result<()> {
    with_conn_mut(|c| {
        c.execute(
            "INSERT INTO analysis_node_artifacts
                (run_id, attempt_id, node_id, direction, slot, artifact_kind, revision_id, binding_kind)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)
             ON CONFLICT(run_id, node_id, direction, slot) DO UPDATE SET
                attempt_id = excluded.attempt_id,
                artifact_kind = excluded.artifact_kind,
                revision_id = excluded.revision_id,
                binding_kind = excluded.binding_kind",
            params![
                row.run_id,
                row.attempt_id,
                row.node_id,
                row.direction,
                row.slot,
                row.artifact_kind,
                row.revision_id,
                row.binding_kind,
            ],
        )?;
        Ok(())
    })
}

/// Atomically records immutable revision metadata and the exact attempt
/// binding that made it observable. The file commit intentionally happens
/// first; if this transaction fails, repair/import can recover the retained
/// immutable bytes without exposing a half-written relation row.
pub fn analysis_artifact_and_node_binding_upsert(
    artifact: &AnalysisArtifactRow,
    binding: &AnalysisNodeArtifactRow,
) -> rusqlite::Result<()> {
    with_conn_mut(|connection| {
        let transaction = connection.transaction()?;
        transaction.execute(
            "INSERT INTO analysis_artifacts (
                id, file_hash, kind, path, content_hash, producer_node,
                input_revisions, config_hash, algorithm_version, created_at_ms,
                byte_size, active, legacy, invalidated
             ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14)
             ON CONFLICT(id) DO UPDATE SET
               path = excluded.path,
               content_hash = excluded.content_hash,
               producer_node = excluded.producer_node,
               input_revisions = excluded.input_revisions,
               config_hash = excluded.config_hash,
               algorithm_version = excluded.algorithm_version,
               byte_size = excluded.byte_size",
            params![
                artifact.id,
                artifact.file_hash,
                artifact.kind,
                artifact.path,
                artifact.content_hash,
                artifact.producer_node,
                artifact.input_revisions,
                artifact.config_hash,
                artifact.algorithm_version,
                artifact.created_at_ms,
                artifact.byte_size,
                artifact.active as i64,
                artifact.legacy as i64,
                artifact.invalidated as i64,
            ],
        )?;
        transaction.execute(
            "INSERT INTO analysis_node_artifacts
                (run_id, attempt_id, node_id, direction, slot, artifact_kind, revision_id, binding_kind)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)
             ON CONFLICT(run_id, node_id, direction, slot) DO UPDATE SET
                attempt_id = excluded.attempt_id,
                artifact_kind = excluded.artifact_kind,
                revision_id = excluded.revision_id,
                binding_kind = excluded.binding_kind",
            params![
                binding.run_id,
                binding.attempt_id,
                binding.node_id,
                binding.direction,
                binding.slot,
                binding.artifact_kind,
                binding.revision_id,
                binding.binding_kind,
            ],
        )?;
        transaction.commit()
    })
}

pub fn analysis_node_artifacts_load(
    run_id: i64,
    node_id: &str,
) -> rusqlite::Result<Vec<AnalysisNodeArtifactRow>> {
    with_conn(|c| {
        let mut stmt = c.prepare(
            "SELECT run_id, attempt_id, node_id, direction, slot, artifact_kind, revision_id, binding_kind
             FROM analysis_node_artifacts
             WHERE run_id = ?1 AND node_id = ?2
             ORDER BY CASE direction WHEN 'input' THEN 0 ELSE 1 END, slot",
        )?;
        let rows = stmt.query_map(params![run_id, node_id], row_from_query)?;
        rows.collect()
    })
}

pub fn analysis_artifact_usage_count(revision_id: &str) -> rusqlite::Result<u64> {
    with_conn(|connection| {
        connection
            .query_row(
                "SELECT COUNT(*) FROM analysis_node_artifacts WHERE revision_id = ?1",
                [revision_id],
                |row| row.get::<_, i64>(0),
            )
            .map(|count| count.max(0) as u64)
    })
}

pub fn analysis_node_artifacts_for_revision(
    revision_id: &str,
) -> rusqlite::Result<Vec<AnalysisNodeArtifactRow>> {
    with_conn(|connection| {
        let mut statement = connection.prepare(
            "SELECT run_id, attempt_id, node_id, direction, slot, artifact_kind, revision_id, binding_kind
             FROM analysis_node_artifacts
             WHERE revision_id = ?1
             ORDER BY run_id DESC, node_id, direction, slot",
        )?;
        let rows = statement.query_map([revision_id], row_from_query)?;
        rows.collect()
    })
}
