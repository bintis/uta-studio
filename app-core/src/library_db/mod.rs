//! SQLite-backed library store.
//!
//! Split into focused submodules so each file owns one responsibility:
//!  - [`connection`] — the singleton `Connection` and `with_conn`/`with_conn_mut` guards
//!  - [`schema`] — current SQLite schema and connection configuration
//!  - [`analysis_queue`] — CRUD for the analyzer's persistent queue
//!  - [`songs`] — core song row CRUD, scan-aware inserts, rekey/update helpers
//!  - [`queries`] — search / pagination / library-menu aggregation queries
//!  - [`rebase`] — one-shot path rewrite when the data root moves
//!
//! `mod.rs` is a thin barrel: it owns the small top-of-stack items
//! (`init_library`, `library_db_path`, the scan generation counter) and
//! re-exports everything else so external call sites keep writing
//! `library_db::foo(...)`.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use crate::cache::uta_studio_dir;

mod analysis_artifacts;
mod analysis_capture_requests;
mod analysis_history;
mod analysis_node_artifacts;
mod analysis_node_attempts;
mod analysis_queue;
mod connection;
mod playlists;
mod queries;
mod rebase;
mod schema;
mod song_analysis_profiles;
mod song_workflows;
mod songs;

pub use analysis_artifacts::{
    AnalysisArtifactRow, analysis_active_artifact, analysis_artifact_clear_active,
    analysis_artifact_delete, analysis_artifact_is_pinned, analysis_artifact_path_is_pinned,
    analysis_artifact_pinned_paths, analysis_artifact_set_active,
    analysis_artifact_set_invalidated, analysis_artifact_set_pinned, analysis_artifact_upsert,
    analysis_artifacts_for_kind, analysis_artifacts_for_song, analysis_artifacts_publish_batch,
};
pub use analysis_capture_requests::{
    AnalysisCaptureRequestRow, analysis_capture_request_delete, analysis_capture_request_get,
    analysis_capture_request_upsert,
};
pub use analysis_history::{
    NewAnalysisHistory, analysis_history_clear, analysis_history_insert, analysis_history_load,
    analysis_history_set_error,
};
pub use analysis_node_artifacts::{
    AnalysisNodeArtifactRow, analysis_artifact_and_node_binding_upsert,
    analysis_artifact_usage_count, analysis_node_artifact_upsert,
    analysis_node_artifacts_for_revision, analysis_node_artifacts_load,
};
pub use analysis_node_attempts::{
    NewAnalysisNodeAttempt, analysis_node_attempts_insert_batch, analysis_node_attempts_load,
};
pub use analysis_queue::{
    EngineQueueIntent, analysis_queue_clear, analysis_queue_delete, analysis_queue_engine_intent,
    analysis_queue_load_rows, analysis_queue_save_rows, analysis_queue_set_engine_intent,
    analysis_queue_upsert_row,
};
pub use playlists::{PlaylistDefinition, replace_all_playlists};
pub use queries::{
    iter_file_hashes_filtered_not_analyzed, load_meta_sql, load_songs_page,
    query_library_menu_items,
};
pub use rebase::{rebase_song_album_art_cache_paths, rebase_song_album_art_paths};
pub use song_analysis_profiles::{
    song_analysis_profile_delete, song_analysis_profile_get, song_analysis_profile_set,
};
pub use song_workflows::{song_workflow_get, song_workflow_set};
pub use songs::{
    append_songs_for_scan, delete_songs_not_in_paths, load_all_songs, load_song_by_hash,
    load_song_path_strings, read_library_meta, replace_all_songs_sorted, update_library_meta,
    update_song_fields,
};

/// Incremented at the start of each `start_scan` so in-flight scan threads stop writing
/// after the library is cleared or replaced (folder change / new scan).
static SCAN_GENERATION: AtomicU64 = AtomicU64::new(0);

pub fn bump_scan_generation() -> u64 {
    SCAN_GENERATION.fetch_add(1, Ordering::SeqCst) + 1
}

pub fn scan_generation_is_current(generation: u64) -> bool {
    SCAN_GENERATION.load(Ordering::SeqCst) == generation
}

pub fn library_db_path() -> PathBuf {
    uta_studio_dir().join("songs.db")
}

pub fn init_library() -> rusqlite::Result<()> {
    if connection::is_initialised() {
        return Ok(());
    }
    let conn = connection::open_connection(&library_db_path())?;
    connection::install(conn)?;
    Ok(())
}

pub fn reconnect_library_at_root(root: &Path) -> Result<(), String> {
    let db_path = root.join("songs.db");
    let conn = connection::open_connection(&db_path)
        .map_err(|e| format!("failed opening relocated songs db: {e}"))?;
    connection::replace_or_install(conn)
}

/// Test-only isolation for the process-wide DB singleton. `library_db`'s
/// connection is a single `OnceLock`, so any test module that needs to
/// exercise real SQL must serialize against every *other* such test module
/// in the crate, not just against itself -- hence one shared lock here
/// rather than a per-module `Mutex`, which would only prevent races within
/// one module and still race across modules (e.g. `analysis_artifact`'s
/// tests vs. `analysis_profile`'s tests running on different threads).
/// Reconnects to a caller-provided temp directory -- never the real app
/// data root -- so nothing exercised under this guard can touch a user's
/// actual library database.
#[cfg(test)]
pub(crate) fn reconnect_for_test(root: &Path) -> std::sync::MutexGuard<'static, ()> {
    static GUARD: std::sync::Mutex<()> = std::sync::Mutex::new(());
    let guard = GUARD
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    reconnect_library_at_root(root).expect("reconnect to isolated test db");
    guard
}
