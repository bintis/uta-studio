//! Local-folder library source used by Uta Studio.

use std::path::PathBuf;

use crate::cache::CacheDir;
use crate::config::{AppConfig, LibrarySource};
use crate::error::UtaStudioError;
use crate::library_db;
use crate::song::Song;

pub mod folder;

pub use folder::{FolderSource, LibraryFolderEntry, list_library_folder};

/// How many songs we buffer in memory before flushing them to the library DB
/// during a scan. Small enough to keep memory bounded, large enough to avoid
/// the per-transaction overhead of writing rows one-by-one.
pub const SCAN_BATCH_SIZE: usize = 25;

/// Context passed to a source while it is running a scan. Implementations
/// should poll `library_db::scan_generation_is_current` periodically and stop
/// emitting writes once it turns false — the user has triggered a new scan or
/// switched sources.
pub struct ScanContext<'a> {
    pub generation: u64,
    pub cache: &'a CacheDir,
}

pub trait MediaSource: Send + Sync {
    /// Human-readable label that ends up in `library_meta.folder` and the UI.
    fn label(&self) -> String;

    /// Run a full library scan. Implementations are responsible for:
    /// - flushing songs to `library_db` in batches
    /// - removing entries that no longer exist upstream
    /// - updating `library_meta` with the active label + total count
    /// - bailing out when the scan generation has been bumped
    fn scan(&self, ctx: &ScanContext<'_>) -> Result<(), UtaStudioError>;

    /// Return the source media path on local disk.
    fn ensure_local_media(&self, song: &Song, cache: &CacheDir) -> Result<PathBuf, UtaStudioError>;
}

/// Shared by every scan implementation: drain `batch` into the DB if it's
/// non-empty (and the scan generation is still current).
pub(crate) fn flush_batch(batch: &mut Vec<Song>, generation: u64) {
    if batch.is_empty() {
        return;
    }
    let _ = library_db::append_songs_for_scan(batch, generation);
    batch.clear();
}

/// Resolve the configured library source, if any.
pub fn active_source() -> Result<Option<Box<dyn MediaSource>>, UtaStudioError> {
    active_source_from_config(&AppConfig::load())
}

pub fn active_source_from_config(
    config: &AppConfig,
) -> Result<Option<Box<dyn MediaSource>>, UtaStudioError> {
    let Some(src) = config.library_source.as_ref() else {
        return Ok(None);
    };
    let LibrarySource::Folders { paths } = src;
    let paths = paths.clone();
    if paths.is_empty() {
        return Ok(None);
    }
    Ok(Some(Box::new(FolderSource::new_many(paths))))
}
