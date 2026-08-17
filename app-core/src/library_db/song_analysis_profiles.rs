//! Per-song override layer of the three-tier parameter inheritance chain
//! (phase plan §8.4: Global Defaults -> Song Profile Overrides -> One-run
//! Overrides). This module only persists the middle tier's raw JSON blob;
//! typing (`AnalysisProfileSnapshot`) lives in `crate::analysis_profile`.

use rusqlite::{OptionalExtension, params};

use super::connection::{with_conn, with_conn_mut};

pub fn song_analysis_profile_set(
    file_hash: &str,
    profile_json: &str,
    updated_at_ms: i64,
) -> rusqlite::Result<()> {
    with_conn_mut(|c| {
        c.execute(
            "INSERT INTO song_analysis_profiles (file_hash, profile_json, updated_at_ms)
             VALUES (?1, ?2, ?3)
             ON CONFLICT(file_hash) DO UPDATE SET
               profile_json = excluded.profile_json,
               updated_at_ms = excluded.updated_at_ms",
            params![file_hash, profile_json, updated_at_ms],
        )?;
        Ok(())
    })
}

pub fn song_analysis_profile_get(file_hash: &str) -> rusqlite::Result<Option<String>> {
    with_conn(|c| {
        c.query_row(
            "SELECT profile_json FROM song_analysis_profiles WHERE file_hash = ?1",
            [file_hash],
            |row| row.get(0),
        )
        .optional()
    })
}

pub fn song_analysis_profile_delete(file_hash: &str) -> rusqlite::Result<()> {
    with_conn_mut(|c| {
        c.execute(
            "DELETE FROM song_analysis_profiles WHERE file_hash = ?1",
            [file_hash],
        )?;
        Ok(())
    })
}
