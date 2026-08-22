use std::path::PathBuf;

use serde::{Deserialize, Serialize};
use tracing::{info, warn};
use ts_rs::TS;

use crate::analyzer::{
    AnalysisArtifactCommit, AnalysisProgressSnapshot, AnalysisStageRoute, enqueue_one,
    is_usdx_song, mark_stems_only, prepare_lrc_no_stems, unix_time_ms, update_song_analyzed,
};
use crate::cache::CacheDir;
use crate::library_db;
use crate::lrc::{self, ParsedLrc};
use crate::song::{Song, TranscriptSource, read_transcript_meta};

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct LrclibCandidate {
    #[serde(default, alias = "trackName")]
    pub track_name: String,
    #[serde(default, alias = "artistName")]
    pub artist_name: String,
    #[serde(default, alias = "albumName")]
    pub album_name: String,
    #[serde(default, alias = "duration")]
    pub duration_secs: f64,
    #[serde(skip_deserializing, default)]
    pub lines: Vec<String>,
    /// Raw LRC (line-level synced lyrics) from LRCLIB, when available. Exposed
    /// to the frontend so the editor can offer timed lyrics without alignment.
    /// `alias` (not `rename`) so it deserializes from LRCLIB's `syncedLyrics`
    /// but still serializes as `synced_lyrics` for the frontend type.
    #[serde(default, alias = "syncedLyrics")]
    pub synced_lyrics: Option<String>,
    #[serde(default, rename = "plainLyrics", skip_serializing)]
    #[ts(skip)]
    plain_lyrics: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct LyricsFile {
    pub lines: Vec<String>,
}

pub fn lrclib_candidates(song: &Song) -> Vec<LrclibCandidate> {
    let title = &song.title;
    let artist = &song.artist;

    if title.is_empty() || artist == "Unknown Artist" {
        return Vec::new();
    }

    let agent = ureq::Agent::new_with_defaults();

    info!(
        "[lrclib] Searching: \"{title}\" by \"{artist}\" ({:.0}s, album=\"{}\")",
        song.duration_secs, song.album
    );

    let url = format!(
        "https://lrclib.net/api/search?track_name={}&artist_name={}",
        urlencoding::encode(title),
        urlencoding::encode(artist),
    );
    let resp = match agent
        .get(&url)
        .header("User-Agent", "Uta Studio/1.0")
        .call()
    {
        Ok(r) => r,
        Err(e) => {
            warn!("[lrclib] Search request failed: {e}");
            return Vec::new();
        }
    };
    let results: Vec<LrclibCandidate> = match resp.into_body().read_json() {
        Ok(r) => r,
        Err(e) => {
            warn!("[lrclib] Failed to parse search results: {e}");
            return Vec::new();
        }
    };

    let mut with_lyrics: Vec<_> = results
        .into_iter()
        .filter(|r| {
            !r.plain_lyrics.is_empty()
                || r.synced_lyrics
                    .as_deref()
                    .is_some_and(|s| !s.trim().is_empty())
        })
        .collect();

    info!(
        "[lrclib] Search returned {} results with lyrics",
        with_lyrics.len()
    );

    let album_lower = song.album.to_lowercase();
    with_lyrics.sort_by_key(|r| {
        let album_bonus: i64 = if r.album_name.to_lowercase() == album_lower {
            0
        } else {
            5_000
        };
        let duration_penalty = ((r.duration_secs - song.duration_secs).abs() * 10.0) as i64;
        album_bonus + duration_penalty
    });

    with_lyrics
        .into_iter()
        .filter_map(|mut r| {
            r.lines = r
                .plain_lyrics
                .lines()
                .map(|l| l.trim().to_string())
                .filter(|l| !l.is_empty())
                .collect();
            // Normalize empty synced payloads to `None` so the frontend can
            // treat "has LRC" as a simple presence check.
            if r.synced_lyrics
                .as_deref()
                .is_some_and(|s| s.trim().is_empty())
            {
                r.synced_lyrics = None;
            }
            if r.lines.is_empty() && r.synced_lyrics.is_none() {
                None
            } else {
                Some(r)
            }
        })
        .collect()
}

pub fn search_lrclib_for_hash(file_hash: &str) -> Vec<LrclibCandidate> {
    let Some(song) = library_db::load_song_by_hash(file_hash).ok().flatten() else {
        return Vec::new();
    };
    lrclib_candidates(&song)
}

pub fn load_lyrics_file(file_hash: &str) -> Option<LyricsFile> {
    let cache = CacheDir::new();
    let path = cache.lyrics_path(file_hash);
    if !path.is_file() {
        return None;
    }
    let bytes = std::fs::read(&path).ok()?;
    serde_json::from_slice::<LyricsFile>(&bytes).ok()
}

/// Drops the transcript so it regenerates from the new lyrics source.
/// Preserves the Authored Chart -- editing the lyrics source must not
/// discard chart edits (docs/analysis-dag-redesign.md §6/Phase 5). Shared
/// by every lyrics-source-change entry point in this file.
fn apply_lyrics_edit_reset(cache: &CacheDir, file_hash: &str) {
    let _ = std::fs::remove_file(cache.transcript_path(file_hash));
    let _ = std::fs::remove_file(cache.recognized_text_path(file_hash));
    let _ = std::fs::remove_file(cache.asr_segments_path(file_hash));
    let _ = std::fs::remove_file(cache.timed_transcript_path(file_hash));
    cache.delete_transcript_variants(file_hash);
}

pub fn save_lyrics_and_realign(file_hash: &str, lines: Vec<String>) -> Result<(), String> {
    if is_usdx_song(file_hash) {
        return Err("Cannot edit lyrics for USDX songs".to_string());
    }

    let normalized: Vec<String> = lines
        .into_iter()
        .map(|l| l.trim().to_string())
        .filter(|l| !l.is_empty())
        .collect();

    if normalized.is_empty() {
        return Err("Lyrics cannot be empty".to_string());
    }

    let cache = CacheDir::new();
    let previous_language = library_db::load_song_by_hash(file_hash)
        .ok()
        .flatten()
        .and_then(|song| song.language);
    write_lyrics_file(&cache, file_hash, &normalized)
        .map_err(|e| format!("Failed to write lyrics file: {e}"))?;

    apply_lyrics_edit_reset(&cache, file_hash);

    update_song_analyzed(file_hash, false, previous_language, None, None, None, None);
    enqueue_one(file_hash);
    Ok(())
}

/// Build the editable transcript JSON from parsed LRC segments.
fn build_lrc_transcript(
    parsed: &ParsedLrc,
    language: Option<&str>,
    key: Option<&str>,
    tempo: f64,
    no_stems: bool,
) -> serde_json::Value {
    serde_json::json!({
        // Leave language null when unknown so it isn't later mistaken for a
        // forced alignment language override (native aligner has no "unknown" model).
        "language": language,
        "source": "lrc",
        "key": key,
        "tempo": tempo,
        "no_stems": no_stems,
        "segments": parsed.segments,
    })
}

fn write_transcript_json(
    cache: &CacheDir,
    file_hash: &str,
    value: &serde_json::Value,
) -> std::io::Result<()> {
    let out = cache.transcript_path(file_hash);
    std::fs::write(&out, serde_json::to_string_pretty(value).unwrap())?;
    // §4.4: known-lyrics/Timed-LRC routes never run through the native
    // pipeline's `chart.build_candidate`, so this Rust-side writer is the
    // one place that needs to also produce the dedicated TimedTranscript
    // artifact for those routes.
    let timed = cache.timed_transcript_path(file_hash);
    std::fs::write(&timed, serde_json::to_string_pretty(value).unwrap())
}

/// Provide LRC / Enhanced LRC for a not-yet-analyzed song, building the
/// transcript directly and skipping transcription. When `separate_stems` is
/// true, a stems-only analysis pass is queued (guide vocals + instrumental);
/// otherwise the chart is authored over its original mix.
pub fn provide_lrc(file_hash: &str, lrc_text: &str, separate_stems: bool) -> Result<(), String> {
    if is_usdx_song(file_hash) {
        return Err("Cannot provide lyrics for USDX songs".to_string());
    }

    let parsed = lrc::parse_lrc(lrc_text)?;

    let Some(song) = library_db::load_song_by_hash(file_hash).ok().flatten() else {
        return Err("Song not found".to_string());
    };

    let cache = CacheDir::new();
    apply_lyrics_edit_reset(&cache, file_hash);
    let _ = std::fs::remove_file(cache.lyrics_path(file_hash));

    let language = song.language.clone();

    if separate_stems {
        let value = build_lrc_transcript(&parsed, language.as_deref(), None, 1.0, false);
        write_transcript_json(&cache, file_hash, &value)
            .map_err(|e| format!("Failed to write transcript: {e}"))?;
        // Stays not-analyzed until stem separation finishes.
        update_song_analyzed(file_hash, false, language, None, None, None, None);
        mark_stems_only(file_hash);
        enqueue_one(file_hash);
    } else {
        let value = build_lrc_transcript(&parsed, language.as_deref(), None, 1.0, true);
        write_transcript_json(&cache, file_hash, &value)
            .map_err(|e| format!("Failed to write transcript: {e}"))?;
        // Authoring over the original mix needs no separation. Prepare everything
        // synchronously (materialize audio, detect the key) and only then mark
        // the song ready — no status-queue pass, and no transient window where
        // authoring assets aren't in place yet.
        prepare_lrc_no_stems(file_hash).map_err(|e| e.to_string())?;
    }

    Ok(())
}

/// Apply provided timed LRC to an already-analyzed song, rebuilding the
/// transcript directly (no realignment) while keeping the existing stems.
pub fn apply_timed_lyrics(file_hash: &str, lrc_text: &str) -> Result<(), String> {
    if is_usdx_song(file_hash) {
        return Err("Cannot edit lyrics for USDX songs".to_string());
    }

    let parsed = lrc::parse_lrc(lrc_text)?;

    let Some(song) = library_db::load_song_by_hash(file_hash).ok().flatten() else {
        return Err("Song not found".to_string());
    };

    let cache = CacheDir::new();
    let meta = read_transcript_meta(&cache, file_hash);
    // Base (unshifted) key so timings line up with the canonical stems.
    let key = song.key.clone().or(meta.key);
    let no_stems = song.no_stems;

    // Timing changed: drop any tempo-shifted transcript variants and the plain
    // lyrics sidecar, and reset the song back to its base key/tempo. The
    // Authored Chart is preserved (docs/analysis-dag-redesign.md §6/Phase 5).
    cache.delete_transcript_variants(file_hash);
    let _ = std::fs::remove_file(cache.lyrics_path(file_hash));

    let value = build_lrc_transcript(
        &parsed,
        song.language.as_deref(),
        key.as_deref(),
        1.0,
        no_stems,
    );
    write_transcript_json(&cache, file_hash, &value)
        .map_err(|e| format!("Failed to write transcript: {e}"))?;

    let mut updated = song;
    updated.is_analyzed = true;
    updated.transcript_source = Some(TranscriptSource::Lrc);
    updated.key = key;
    updated.override_key = None;
    updated.tempo = 1.0;
    updated.key_offset = 0;
    updated.no_stems = no_stems;
    library_db::update_song_fields(file_hash, &updated).map_err(|e| e.to_string())?;

    record_timed_lyrics_import(&cache, file_hash, &updated.title, &updated.artist)?;

    Ok(())
}

/// `lyrics.import_timed`'s own real event/history record
/// (docs/analysis-dag-redesign.md Phase 3 status note). This Rust-side
/// Timed LRC import path completes synchronously and entirely outside the
/// native-queue-driven `process_song`/`LIVE_ANALYSIS`/`ANALYSIS_STARTED`
/// machinery -- there is no "in-flight" window for a progress poll to ever
/// observe, so it never produced a run history entry the way a queued
/// analysis does, and "Last successful run" always read "None yet" after a
/// Timed LRC import. This is a separate, minimal, purely-additive pair of
/// inserts (`analysis_history` + `analysis_node_attempts`) rather than
/// routing through the queue's shared mutable state, since there's nothing
/// in-flight to coordinate with. Best-effort: a failure here must not fail
/// the import itself, which is why every fallible step below is silently
/// dropped rather than propagated.
fn record_timed_lyrics_import(
    cache: &CacheDir,
    file_hash: &str,
    title: &str,
    artist: &str,
) -> Result<(), String> {
    let now = unix_time_ms();
    let (immutable_path, content_hash, byte_size) =
        crate::analysis_artifact::ArtifactStore::new(&cache.path)?.capture(
            file_hash,
            crate::analysis_graph::ArtifactKind::TimedTranscript,
            &cache.timed_transcript_path(file_hash),
        )?;
    let route = AnalysisStageRoute {
        stage: "finalizing".to_string(),
        node_id: Some("lyrics.import_timed".to_string()),
        node_event: Some("completed".to_string()),
        binding_kind: None,
        committed_outputs: vec![AnalysisArtifactCommit {
            slot: "output:0".to_string(),
            artifact_kind: "TimedTranscript".to_string(),
            path: cache.timed_transcript_path(file_hash),
            binding_kind: "produced".to_string(),
            config_hash: "timed-lrc-import".to_string(),
            algorithm_version: format!("lrc-parser/app-{}", env!("CARGO_PKG_VERSION")),
            immutable_path: Some(immutable_path),
            content_hash: Some(content_hash),
            byte_size: Some(byte_size),
            capture_error: None,
        }],
        input_revision_ids: vec![None],
        operation: "Imported timed lyrics".to_string(),
        implementation: "Uta Studio LRC parser".to_string(),
        model: "N/A".to_string(),
        stage_progress: 100,
        requested_device: "cpu".to_string(),
        actual_device: "cpu".to_string(),
        fallback_from: None,
        fallback_reason: None,
        backend_fallback_from: None,
        backend_fallback_reason: None,
        started_at_ms: Some(now),
        finished_at_ms: Some(now),
        event_at_ms: Some(now),
        work_units_completed: Some(1),
        work_units_total: Some(1),
    };
    let snapshot = AnalysisProgressSnapshot {
        stage: "complete".to_string(),
        overall_progress: 100,
        stage_progress: 100,
        operation: "Timed lyrics imported".to_string(),
        detail: "Transcript rebuilt directly from the provided timed LRC.".to_string(),
        implementation: "Uta Studio LRC parser".to_string(),
        model: "N/A".to_string(),
        device: "cpu".to_string(),
        requested_device: "cpu".to_string(),
        fallback_from: None,
        fallback_reason: None,
        backend_fallback_from: None,
        backend_fallback_reason: None,
        stage_routes: vec![route],
        node_id: Some("lyrics.import_timed".to_string()),
        node_event: Some("completed".to_string()),
        artifact_reused_reason: None,
        analysis_log_path: None,
    };
    let snapshot_json = serde_json::to_string(&snapshot).map_err(|error| error.to_string())?;
    let run_id = library_db::analysis_history_insert(&library_db::NewAnalysisHistory {
        file_hash,
        title,
        artist,
        status: "completed",
        started_at_ms: now,
        finished_at_ms: now,
        snapshot_json: &snapshot_json,
        error_message: None,
        log_path: None,
    })
    .map_err(|error| error.to_string())?;
    library_db::analysis_node_attempts_insert_batch(
        run_id,
        file_hash,
        &[library_db::NewAnalysisNodeAttempt {
            node_id: "lyrics.import_timed",
            status: "succeeded",
            progress: 100,
            operation: "Imported timed lyrics",
            implementation: "Uta Studio LRC parser",
            model: "N/A",
            requested_device: "cpu",
            actual_device: "cpu",
            fallback_from: None,
            fallback_reason: None,
            backend_fallback_from: None,
            backend_fallback_reason: None,
            started_at_ms: Some(now),
            finished_at_ms: Some(now),
        }],
    )
    .map_err(|error| error.to_string())?;
    crate::artifact_workbench::capture_analysis_run_artifacts_in(cache, run_id, file_hash)
}

pub(crate) fn write_lyrics_file(
    cache: &CacheDir,
    file_hash: &str,
    lines: &[String],
) -> std::io::Result<PathBuf> {
    let out = cache.lyrics_path(file_hash);
    let lyrics_json = serde_json::json!({ "lines": lines });
    std::fs::write(&out, serde_json::to_string_pretty(&lyrics_json).unwrap())?;
    Ok(out)
}

pub(crate) fn fetch_lrclib_lyrics(song: &Song, cache: &CacheDir) -> Option<PathBuf> {
    let existing = cache.lyrics_path(&song.file_hash);
    if existing.is_file() {
        info!(
            "[lrclib] Using existing lyrics file at {}",
            existing.display()
        );
        return Some(existing);
    }

    let candidates = lrclib_candidates(song);
    let pick = candidates.into_iter().next()?;

    info!(
        "[lrclib] Picked \"{}\" from \"{}\" (duration {:.0}s, delta {:.1}s)",
        pick.track_name,
        pick.album_name,
        pick.duration_secs,
        (pick.duration_secs - song.duration_secs).abs()
    );
    info!("[lrclib] Extracted {} lines", pick.lines.len());

    match write_lyrics_file(cache, &song.file_hash, &pick.lines) {
        Ok(out) => {
            info!("[lrclib] Lyrics saved to {}", out.display());
            Some(out)
        }
        Err(e) => {
            warn!("[lrclib] Failed to write lyrics: {e}");
            None
        }
    }
}

#[cfg(test)]
mod chart_protection_tests {
    use super::apply_lyrics_edit_reset;
    use crate::cache::CacheDir;
    use std::sync::atomic::{AtomicU64, Ordering};

    static COUNTER: AtomicU64 = AtomicU64::new(0);

    fn temp_cache() -> CacheDir {
        let nonce = COUNTER.fetch_add(1, Ordering::SeqCst);
        let path = std::env::temp_dir().join(format!(
            "uta-studio-lyrics-reset-test-{}-{nonce}",
            std::process::id()
        ));
        std::fs::create_dir_all(&path).expect("create temp cache dir");
        CacheDir { path }
    }

    #[test]
    fn lyrics_edit_reset_preserves_the_authored_chart() {
        let cache = temp_cache();
        let hash = "songLyricsEdit";
        std::fs::write(cache.vocal_chart_path(hash), b"{}").unwrap();
        std::fs::write(cache.transcript_path(hash), b"{}").unwrap();
        std::fs::write(cache.variant_transcript_path(hash, 1.2), b"{}").unwrap();

        apply_lyrics_edit_reset(&cache, hash);

        assert!(
            cache.vocal_chart_path(hash).is_file(),
            "authored chart must survive a lyrics source change"
        );
        assert!(!cache.transcript_path(hash).is_file());
        assert!(!cache.variant_transcript_path(hash, 1.2).is_file());
        cache.clear_all();
    }

    #[test]
    fn lyrics_edit_reset_also_drops_the_split_transcript_artifacts() {
        // §4.4: a lyrics-source change must invalidate all four transcript
        // files, not just the compatibility one -- otherwise stale ASR
        // artifacts from the previous source would linger and mislead the
        // Artifact Inventory/UI.
        let cache = temp_cache();
        let hash = "songLyricsEditSplit";
        std::fs::write(cache.transcript_path(hash), b"{}").unwrap();
        std::fs::write(cache.recognized_text_path(hash), b"{}").unwrap();
        std::fs::write(cache.asr_segments_path(hash), b"{}").unwrap();
        std::fs::write(cache.timed_transcript_path(hash), b"{}").unwrap();

        apply_lyrics_edit_reset(&cache, hash);

        assert!(!cache.recognized_text_path(hash).is_file());
        assert!(!cache.asr_segments_path(hash).is_file());
        assert!(!cache.timed_transcript_path(hash).is_file());
        cache.clear_all();
    }

    #[test]
    fn write_transcript_json_also_writes_the_dedicated_timed_transcript_file() {
        // §4.4: the known-lyrics/Timed-LRC routes never run through the
        // native pipeline's `chart.build_candidate`, so this Rust-side
        // writer is the one place that has to produce TimedTranscript for
        // those routes.
        let cache = temp_cache();
        let hash = "songWriteTranscriptSplit";
        let value = serde_json::json!({"segments": [], "source": "lrc"});

        super::write_transcript_json(&cache, hash, &value).unwrap();

        assert!(cache.transcript_path(hash).is_file());
        assert!(cache.timed_transcript_path(hash).is_file());
        let transcript: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(cache.transcript_path(hash)).unwrap())
                .unwrap();
        let timed: serde_json::Value = serde_json::from_str(
            &std::fs::read_to_string(cache.timed_transcript_path(hash)).unwrap(),
        )
        .unwrap();
        assert_eq!(transcript, timed);
        assert_eq!(transcript, value);
        cache.clear_all();
    }
}

#[cfg(test)]
mod timed_lyrics_import_history_tests {
    //! §7.3/Phase 3 status note: `lyrics.import_timed` never produced a run
    //! history entry (it's a synchronous Rust-only path with no
    //! `process_song` queue lifecycle to hook into), so "Last successful
    //! run" always read "None yet" after a Timed LRC import. These lock
    //! `record_timed_lyrics_import`'s real INSERT behavior against an
    //! actual DB fixture, not mocks.
    use super::record_timed_lyrics_import;
    use crate::{cache::CacheDir, library_db};
    use std::time::{SystemTime, UNIX_EPOCH};

    fn temp_root(label: &str) -> std::path::PathBuf {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos();
        std::env::temp_dir().join(format!(
            "uta-studio-timed-lyrics-history-test-{label}-{}-{nonce}",
            std::process::id()
        ))
    }

    #[test]
    fn records_a_completed_history_row_and_a_matching_node_attempt() {
        let root = temp_root("basic");
        let _guard = library_db::reconnect_for_test(&root);
        let cache = CacheDir {
            path: root.join("cache"),
        };
        std::fs::create_dir_all(&cache.path).unwrap();
        std::fs::write(
            cache.timed_transcript_path("songTimedLrc"),
            b"{\"segments\":[]}",
        )
        .unwrap();

        record_timed_lyrics_import(&cache, "songTimedLrc", "Test Title", "Test Artist").unwrap();

        let history = library_db::analysis_history_load(50).expect("load history");
        let run = history
            .iter()
            .find(|row| row.file_hash == "songTimedLrc")
            .expect("a history row must exist for this import");
        assert_eq!(run.status, "completed");
        assert_eq!(run.title, "Test Title");

        let attempts = library_db::analysis_node_attempts_load(run.id).expect("load attempts");
        assert_eq!(attempts.len(), 1);
        assert_eq!(attempts[0].node_id, "lyrics.import_timed");
        assert_eq!(attempts[0].status, "succeeded");
        let bindings = library_db::analysis_node_artifacts_load(run.id, "lyrics.import_timed")
            .expect("load exact bindings");
        assert!(bindings.iter().any(|binding| {
            binding.direction == "output"
                && binding.binding_kind == "produced"
                && binding.revision_id.is_some()
        }));

        drop(_guard);
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn the_snapshot_json_round_trips_through_the_real_progress_snapshot_type() {
        // Guards against a silent no-op: if `AnalysisProgressSnapshot`'s
        // shape ever drifts out of sync with what this function hand-builds,
        // `load_analysis_history`'s `serde_json::from_str(..).ok()?` would
        // just drop the row instead of erroring -- this asserts the row
        // parses back into a real snapshot with the fields this feature
        // depends on, not just that a row exists.
        let root = temp_root("roundtrip");
        let _guard = library_db::reconnect_for_test(&root);
        let cache = CacheDir {
            path: root.join("cache"),
        };
        std::fs::create_dir_all(&cache.path).unwrap();
        std::fs::write(
            cache.timed_transcript_path("songTimedLrcRoundtrip"),
            b"{\"segments\":[]}",
        )
        .unwrap();

        record_timed_lyrics_import(&cache, "songTimedLrcRoundtrip", "Title", "Artist").unwrap();

        let history = library_db::analysis_history_load(50).expect("load history");
        let row = history
            .iter()
            .find(|row| row.file_hash == "songTimedLrcRoundtrip")
            .expect("row must exist");
        let snapshot: crate::analyzer::AnalysisProgressSnapshot =
            serde_json::from_str(&row.snapshot_json).expect("snapshot_json must parse");
        assert_eq!(snapshot.node_id.as_deref(), Some("lyrics.import_timed"));
        assert_eq!(snapshot.node_event.as_deref(), Some("completed"));
        assert_eq!(snapshot.stage_routes.len(), 1);

        drop(_guard);
        let _ = std::fs::remove_dir_all(&root);
    }
}
