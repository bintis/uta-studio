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

/// Atomically clears the current selection for one artifact kind while
/// retaining every historical revision for explicit recovery. A pinned
/// revision blocks the complete operation so a chart deletion can never
/// partially cross the user's pin boundary. The optional compatibility
/// recovery revision is recorded in the same transaction. The caller may capture immutable
/// bytes before this call; no artifact row becomes visible if the transaction
/// fails or a pin appears concurrently.
pub fn analysis_artifacts_deactivate_kind_with_recovery(
    file_hash: &str,
    kind: &str,
    recovery: Option<&AnalysisArtifactRow>,
) -> rusqlite::Result<bool> {
    with_conn_mut(|connection| {
        let transaction = connection.transaction()?;
        let pinned: i64 = transaction.query_row(
            "SELECT COUNT(*) FROM analysis_artifacts \
             WHERE file_hash = ?1 AND kind = ?2 AND invalidated = 0 AND pinned = 1",
            params![file_hash, kind],
            |row| row.get(0),
        )?;
        if pinned > 0 {
            return Ok(false);
        }
        if let Some(row) = recovery {
            if row.file_hash != file_hash || row.kind != kind || row.active || row.invalidated {
                return Err(rusqlite::Error::InvalidQuery);
            }
            let invalidated: i64 = transaction.query_row(
                "SELECT COUNT(*) FROM analysis_artifacts WHERE id = ?1 AND invalidated = 1",
                [&row.id],
                |query_row| query_row.get(0),
            )?;
            if invalidated > 0 {
                return Err(rusqlite::Error::InvalidQuery);
            }
            transaction.execute(
                "INSERT INTO analysis_artifacts (
                    id, file_hash, kind, path, content_hash, producer_node,
                    input_revisions, config_hash, algorithm_version, created_at_ms,
                    byte_size, active, legacy, invalidated
                 ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, 0, ?12, 0)
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
                    row.legacy as i64,
                ],
            )?;
        }
        transaction.execute(
            "UPDATE analysis_artifacts SET active = 0 \
             WHERE file_hash = ?1 AND kind = ?2 AND invalidated = 0",
            params![file_hash, kind],
        )?;
        transaction.commit()?;
        Ok(true)
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

/// Atomically records a complete Engine result and switches only the
/// selected Candidate/evidence kinds after every output was validated and
/// captured. Authored revisions are never included by the caller.
pub fn analysis_artifacts_publish_batch(
    rows: &[AnalysisArtifactRow],
    activations: &[(String, String, String)],
    analyzed_file_hashes: &[String],
) -> rusqlite::Result<()> {
    with_conn_mut(|connection| {
        let transaction = connection.transaction()?;
        for row in rows {
            transaction.execute(
                "INSERT INTO analysis_artifacts (
                    id, file_hash, kind, path, content_hash, producer_node,
                    input_revisions, config_hash, algorithm_version, created_at_ms,
                    byte_size, active, legacy, invalidated
                 ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, 0, 0, 0)
                 ON CONFLICT(id) DO UPDATE SET
                    path = excluded.path, content_hash = excluded.content_hash,
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
                ],
            )?;
        }
        for (file_hash, kind, revision_id) in activations {
            transaction.execute(
                "UPDATE analysis_artifacts SET active = 0 WHERE file_hash = ?1 AND kind = ?2",
                params![file_hash, kind],
            )?;
            let changed = transaction.execute(
                "UPDATE analysis_artifacts SET active = 1 WHERE id = ?1 AND file_hash = ?2 AND kind = ?3 AND invalidated = 0",
                params![revision_id, file_hash, kind],
            )?;
            if changed != 1 {
                return Err(rusqlite::Error::QueryReturnedNoRows);
            }
        }
        mark_songs_analyzed_in_transaction(&transaction, analyzed_file_hashes)?;
        transaction.commit()
    })
}

fn mark_songs_analyzed_in_transaction(
    transaction: &rusqlite::Transaction<'_>,
    file_hashes: &[String],
) -> rusqlite::Result<()> {
    for file_hash in file_hashes {
        let rows = {
            let mut statement =
                transaction.prepare("SELECT id, payload FROM songs WHERE file_hash = ?1")?;
            statement
                .query_map([file_hash], |row| {
                    Ok((row.get::<_, i64>(0)?, row.get::<_, String>(1)?))
                })?
                .collect::<rusqlite::Result<Vec<_>>>()?
        };
        for (song_id, payload) in rows {
            let mut song: crate::song::Song = serde_json::from_str(&payload).map_err(|error| {
                rusqlite::Error::FromSqlConversionFailure(
                    1,
                    rusqlite::types::Type::Text,
                    Box::new(error),
                )
            })?;
            song.is_analyzed = true;
            let payload = super::songs::song_to_payload(&song)?;
            let changed = transaction.execute(
                "UPDATE songs SET is_analyzed = 1, payload = ?2 WHERE id = ?1",
                params![song_id, payload],
            )?;
            if changed != 1 {
                return Err(rusqlite::Error::QueryReturnedNoRows);
            }
        }
    }
    Ok(())
}

pub fn mark_songs_analyzed(file_hashes: &[String]) -> rusqlite::Result<()> {
    with_conn_mut(|connection| {
        let transaction = connection.transaction()?;
        mark_songs_analyzed_in_transaction(&transaction, file_hashes)?;
        transaction.commit()
    })
}

pub fn active_artifact_file_hashes(kind: &str) -> rusqlite::Result<Vec<String>> {
    with_conn(|connection| {
        let mut statement = connection.prepare(
            "SELECT DISTINCT file_hash FROM analysis_artifacts \
             WHERE kind = ?1 AND active = 1 AND invalidated = 0",
        )?;
        statement
            .query_map([kind], |row| row.get(0))?
            .collect::<rusqlite::Result<Vec<_>>>()
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        analysis_graph::ArtifactKind,
        song::{Song, SongOrigin},
    };
    use std::path::PathBuf;

    fn artifact_row(id: &str, file_hash: &str, kind: &str) -> AnalysisArtifactRow {
        AnalysisArtifactRow {
            id: id.to_string(),
            file_hash: file_hash.to_string(),
            kind: kind.to_string(),
            path: PathBuf::from(format!("{id}.json"))
                .to_string_lossy()
                .into_owned(),
            content_hash: format!("content-{id}"),
            producer_node: "test".to_string(),
            input_revisions: "[]".to_string(),
            config_hash: "config".to_string(),
            algorithm_version: "1".to_string(),
            created_at_ms: 1,
            byte_size: 1,
            active: false,
            legacy: false,
            invalidated: false,
        }
    }

    #[test]
    fn deactivating_authored_kind_is_atomic_recoverable_and_preserves_other_artifacts() {
        let root = std::env::temp_dir().join(format!(
            "uta-studio-delete-chart-test-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_nanos()
        ));
        std::fs::create_dir_all(&root).unwrap();
        let _guard = crate::library_db::reconnect_for_test(&root);
        let file_hash = "delete-chart-song";
        let authored = serde_json::to_string(&ArtifactKind::AuthoredChart).unwrap();
        let candidate = serde_json::to_string(&ArtifactKind::CandidateChart).unwrap();
        let evidence = serde_json::to_string(&ArtifactKind::EvidenceBundle).unwrap();
        let rows = vec![
            artifact_row("authored-old", file_hash, &authored),
            artifact_row("authored-active", file_hash, &authored),
            artifact_row("candidate-active", file_hash, &candidate),
            artifact_row("evidence-active", file_hash, &evidence),
        ];
        analysis_artifacts_publish_batch(
            &rows,
            &[
                (
                    file_hash.to_string(),
                    authored.clone(),
                    "authored-active".to_string(),
                ),
                (
                    file_hash.to_string(),
                    candidate.clone(),
                    "candidate-active".to_string(),
                ),
                (
                    file_hash.to_string(),
                    evidence.clone(),
                    "evidence-active".to_string(),
                ),
            ],
            &[],
        )
        .unwrap();

        let mut recovery = artifact_row("authored-recovery", file_hash, &authored);
        recovery.legacy = true;
        analysis_artifact_set_pinned("authored-old", true).unwrap();
        assert!(
            !analysis_artifacts_deactivate_kind_with_recovery(
                file_hash,
                &authored,
                Some(&recovery)
            )
            .unwrap()
        );
        let authored_rows = analysis_artifacts_for_kind(file_hash, &authored).unwrap();
        assert_eq!(authored_rows.len(), 2);
        assert!(authored_rows.iter().all(|row| row.id != recovery.id));
        assert!(
            analysis_active_artifact(file_hash, &authored)
                .unwrap()
                .is_some()
        );

        analysis_artifact_set_pinned("authored-old", false).unwrap();
        assert!(
            analysis_artifacts_deactivate_kind_with_recovery(file_hash, &authored, Some(&recovery))
                .unwrap()
        );
        let retained = analysis_artifacts_for_kind(file_hash, &authored).unwrap();
        assert_eq!(retained.len(), 3);
        assert!(retained.iter().all(|row| !row.invalidated && !row.active));
        assert!(
            retained
                .iter()
                .any(|row| row.id == recovery.id && row.legacy)
        );
        analysis_artifact_set_active(file_hash, &authored, "authored-old").unwrap();
        assert_eq!(
            analysis_active_artifact(file_hash, &authored)
                .unwrap()
                .unwrap()
                .id,
            "authored-old",
            "retained authored history remains explicitly recoverable"
        );
        assert_eq!(
            analysis_active_artifact(file_hash, &candidate)
                .unwrap()
                .unwrap()
                .id,
            "candidate-active"
        );
        assert_eq!(
            analysis_active_artifact(file_hash, &evidence)
                .unwrap()
                .unwrap()
                .id,
            "evidence-active"
        );
    }

    #[test]
    fn candidate_publication_marks_every_duplicate_hash_row_analyzed() {
        let root = std::env::temp_dir().join(format!(
            "uta-studio-publish-test-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_nanos()
        ));
        std::fs::create_dir_all(&root).unwrap();
        let _guard = crate::library_db::reconnect_for_test(&root);
        let file_hash = "candidate-song";
        let song = Song {
            path: root.join("candidate.flac"),
            file_hash: file_hash.to_string(),
            title: "Candidate".to_string(),
            artist: "Test".to_string(),
            album: "Test".to_string(),
            duration_secs: 1.0,
            album_art_path: None,
            is_analyzed: false,
            language: None,
            transcript_source: None,
            key: None,
            override_key: None,
            bpm: None,
            tempo: 1.0,
            key_offset: 0,
            is_video: false,
            usdx: None,
            origin: SongOrigin::LocalFile,
            no_stems: false,
            authoring_ready: false,
            authoring_missing: Vec::new(),
            editor_ready: false,
            editor_blocked_reason: None,
            override_bpm: None,
            composer: None,
            country: None,
            background_video_path: None,
        };
        let mut duplicate = song.clone();
        duplicate.path = root.join("duplicate.flac");
        duplicate.title = "Duplicate".to_string();
        super::super::songs::replace_all_songs_sorted(&[song, duplicate]).unwrap();

        let kind = serde_json::to_string(&ArtifactKind::CandidateChart).unwrap();
        let revision_id = "candidate-revision";
        let row = AnalysisArtifactRow {
            id: revision_id.to_string(),
            file_hash: file_hash.to_string(),
            kind: kind.clone(),
            path: PathBuf::from("candidate.json")
                .to_string_lossy()
                .into_owned(),
            content_hash: "content".to_string(),
            producer_node: "chart.fuse".to_string(),
            input_revisions: "[]".to_string(),
            config_hash: "config".to_string(),
            algorithm_version: "1".to_string(),
            created_at_ms: 1,
            byte_size: 1,
            active: false,
            legacy: false,
            invalidated: false,
        };
        analysis_artifacts_publish_batch(
            &[row],
            &[(file_hash.to_string(), kind, revision_id.to_string())],
            &[file_hash.to_string()],
        )
        .unwrap();

        let rows = with_conn(|connection| {
            let mut statement = connection.prepare(
                "SELECT is_analyzed, payload FROM songs WHERE file_hash = ?1 ORDER BY path",
            )?;
            statement
                .query_map([file_hash], |row| {
                    Ok((row.get::<_, i64>(0)?, row.get::<_, String>(1)?))
                })?
                .collect::<rusqlite::Result<Vec<_>>>()
        })
        .unwrap();
        assert_eq!(rows.len(), 2);
        for (indexed, payload) in rows {
            let persisted: Song = serde_json::from_str(&payload).unwrap();
            assert_eq!(indexed, 1);
            assert!(persisted.is_analyzed);
        }
    }
}
