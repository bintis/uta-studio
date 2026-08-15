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

mod analysis_queue;
mod analysis_history;
mod connection;
mod playlists;
mod queries;
mod rebase;
mod schema;
mod songs;

pub use analysis_queue::{
    analysis_queue_clear, analysis_queue_delete, analysis_queue_load_rows,
    analysis_queue_save_rows, analysis_queue_upsert_row,
};
pub use analysis_history::{
    AnalysisHistoryRow, analysis_history_clear, analysis_history_insert, analysis_history_load,
};
pub use playlists::{PlaylistDefinition, replace_all_playlists};
pub use queries::{
    iter_file_hashes_filtered_not_analyzed, load_meta_sql, load_songs_page,
    query_library_menu_items,
};
pub use rebase::{rebase_song_album_art_cache_paths, rebase_song_album_art_paths};
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
