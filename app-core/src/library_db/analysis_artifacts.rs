//! Persistence for Artifact Revisions (Phase 2 of the analysis DAG
//! redesign). Row-shaped storage only -- domain typing (`ArtifactKind`,
//! `AnalysisNodeId`) lives in `crate::analysis_artifact`, which maps to and
//! from this row shape at the boundary.

use rusqlite::{OptionalExtension, params};

use super::connection::{with_conn, with_conn_mut};

pub struct AnalysisArtifactRow {
    pub id: String,
    pub file_hash: String,
    pub kind: String,
    pub path: String,
    pub content_hash: String,
    pub producer_node: String,
    pub input_revisions: String,
    pub config_hash: String,
    pub algorithm_version: String,
    pub created_at_ms: i64,
    pub byte_size: i64,
    pub active: bool,
    pub legacy: bool,
    pub invalidated: bool,
}

const COLUMNS: &str = "id, file_hash, kind, path, content_hash, producer_node, \
    input_revisions, config_hash, algorithm_version, created_at_ms, byte_size, active, legacy, \
    invalidated";

fn row_from_query(row: &rusqlite::Row) -> rusqlite::Result<AnalysisArtifactRow> {
    Ok(AnalysisArtifactRow {
        id: row.get(0)?,
        file_hash: row.get(1)?,
        kind: row.get(2)?,
        path: row.get(3)?,
        content_hash: row.get(4)?,
        producer_node: row.get(5)?,
        input_revisions: row.get(6)?,
        config_hash: row.get(7)?,
        algorithm_version: row.get(8)?,
        created_at_ms: row.get(9)?,
        byte_size: row.get(10)?,
        active: row.get::<_, i64>(11)? != 0,
        legacy: row.get::<_, i64>(12)? != 0,
        invalidated: row.get::<_, i64>(13)? != 0,
    })
}

/// Insert-or-update by revision id. Deliberately never touches
/// `active`/`invalidated` on conflict: a rerun that reproduces an existing
/// revision id (same content hash) must not flip the Active Revision back
/// on if the user has since moved it elsewhere, nor silently un-invalidate
/// a revision the user explicitly marked as wrong just because the
/// pipeline reproduced byte-identical output.
pub fn analysis_artifact_upsert(row: &AnalysisArtifactRow) -> rusqlite::Result<()> {
    with_conn_mut(|c| {
        c.execute(
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
                row.id,
                row.file_hash,
                row.kind,
                row.path,
                row.content_hash,
                row.producer_node,
                row.input_revisions,
                row.config_hash,
                row.algorithm_version,
                row.created_at_ms,
                row.byte_size,
                row.active as i64,
                row.legacy as i64,
                row.invalidated as i64,
            ],
        )?;
        Ok(())
    })
}

/// Phase 6 `invalidate_analysis_artifact` / Phase 7 §7.6 "Invalidate".
/// Also clears `active` in the same statement when invalidating -- an
/// invalidated revision must never remain the one the rest of the app
/// treats as current, and `analysis_artifact_set_active` separately
/// refuses to re-select an invalidated revision (see
/// `analysis_artifact.rs::set_active_artifact_revision`).
pub fn analysis_artifact_set_invalidated(id: &str, invalidated: bool) -> rusqlite::Result<()> {
    with_conn_mut(|c| {
        c.execute(
            "UPDATE analysis_artifacts \
             SET invalidated = ?1, active = CASE WHEN ?1 = 1 THEN 0 ELSE active END \
             WHERE id = ?2",
            params![invalidated as i64, id],
        )?;
        Ok(())
    })
}

pub fn analysis_artifacts_for_song(file_hash: &str) -> rusqlite::Result<Vec<AnalysisArtifactRow>> {
    with_conn(|c| {
        let mut stmt = c.prepare(&format!(
            "SELECT {COLUMNS} FROM analysis_artifacts WHERE file_hash = ?1 ORDER BY created_at_ms DESC"
        ))?;
        let rows = stmt.query_map([file_hash], row_from_query)?;
        rows.collect()
    })
}

pub fn analysis_artifacts_for_kind(
    file_hash: &str,
    kind: &str,
) -> rusqlite::Result<Vec<AnalysisArtifactRow>> {
    with_conn(|c| {
        let mut stmt = c.prepare(&format!(
            "SELECT {COLUMNS} FROM analysis_artifacts WHERE file_hash = ?1 AND kind = ?2 ORDER BY created_at_ms DESC"
        ))?;
        let rows = stmt.query_map(params![file_hash, kind], row_from_query)?;
        rows.collect()
    })
}

pub fn analysis_active_artifact(
    file_hash: &str,
    kind: &str,
) -> rusqlite::Result<Option<AnalysisArtifactRow>> {
    with_conn(|c| {
        c.query_row(
            &format!(
                "SELECT {COLUMNS} FROM analysis_artifacts \
                 WHERE file_hash = ?1 AND kind = ?2 AND active = 1 LIMIT 1"
            ),
            params![file_hash, kind],
            row_from_query,
        )
        .optional()
    })
}

pub fn analysis_artifact_set_active(
    file_hash: &str,
    kind: &str,
    revision_id: &str,
) -> rusqlite::Result<()> {
    with_conn_mut(|c| {
        let tx = c.transaction()?;
        tx.execute(
            "UPDATE analysis_artifacts SET active = 0 WHERE file_hash = ?1 AND kind = ?2",
            params![file_hash, kind],
        )?;
        tx.execute(
            "UPDATE analysis_artifacts SET active = 1 WHERE id = ?1",
            params![revision_id],
        )?;
        tx.commit()?;
        Ok(())
    })
}

pub fn analysis_artifact_clear_active(file_hash: &str, kind: &str) -> rusqlite::Result<()> {
    with_conn_mut(|connection| {
        connection.execute(
            "UPDATE analysis_artifacts SET active = 0 WHERE file_hash = ?1 AND kind = ?2",
            params![file_hash, kind],
        )?;
        Ok(())
    })
}

pub fn analysis_artifact_set_pinned(id: &str, pinned: bool) -> rusqlite::Result<()> {
    with_conn_mut(|c| {
        c.execute(
            "UPDATE analysis_artifacts SET pinned = ?1 WHERE id = ?2",
            params![pinned as i64, id],
        )?;
        Ok(())
    })
}

pub fn analysis_artifact_is_pinned(id: &str) -> rusqlite::Result<bool> {
    with_conn(|c| {
        c.query_row(
            "SELECT pinned FROM analysis_artifacts WHERE id = ?1",
            [id],
            |row| Ok(row.get::<_, i64>(0)? != 0),
        )
        .optional()
        .map(|value| value.unwrap_or(false))
    })
}

pub fn analysis_artifact_path_is_pinned(path: &std::path::Path) -> rusqlite::Result<bool> {
    let path = path.to_string_lossy();
    with_conn(|c| {
        c.query_row(
            "SELECT 1 FROM analysis_artifacts WHERE path = ?1 AND pinned = 1 LIMIT 1",
            [path.as_ref()],
            |_| Ok(true),
        )
        .optional()
        .map(|value| value.unwrap_or(false))
    })
}

pub fn analysis_artifact_pinned_paths() -> rusqlite::Result<Vec<std::path::PathBuf>> {
    with_conn(|c| {
        let mut stmt =
            c.prepare("SELECT DISTINCT path FROM analysis_artifacts WHERE pinned = 1")?;
        let rows = stmt.query_map([], |row| row.get::<_, String>(0))?;
        rows.map(|row| row.map(std::path::PathBuf::from)).collect()
    })
}

pub fn analysis_artifact_delete(id: &str) -> rusqlite::Result<()> {
    with_conn_mut(|c| {
        c.execute("DELETE FROM analysis_artifacts WHERE id = ?1", [id])?;
        Ok(())
    })
}
