use super::*;

pub fn shutdown_server() {
    let pid = SERVER_PID.swap(0, Ordering::SeqCst);
    if pid != 0 {
        info!("[analyzer] Graceful shutdown of server (pid={pid})");
        // A process killed here must not remain in the singleton.  Otherwise
        // `ensure_server` sees `Some` and the next analysis attempts to reuse
        // a dead connection (or, during setup, an stale native worker).
        if let Ok(mut guard) = ANALYZER_SERVER.try_lock() {
            if let Some(server) = guard.as_mut() {
                let _ = server.writer.write_all(b"{\"type\":\"quit\"}\n");
                let _ = server.writer.flush();
            }
            *guard = None;
            return;
        }
        std::thread::spawn(move || {
            let _ = Command::new("kill").args([&pid.to_string()]).status();
            std::thread::sleep(std::time::Duration::from_secs(3));
            let _ = Command::new("kill").args(["-9", &pid.to_string()]).status();
        });
    }
}

pub fn delete_cache(file_hash: &str) {
    if is_usdx_song(file_hash) {
        return;
    }
    let cache = CacheDir::new();
    cache.delete_song_cache(file_hash);
    update_song_analyzed(file_hash, false, None, None, None, None, None);
}

pub fn reanalyze_transcript(file_hash: &str, language: Option<String>) {
    if is_usdx_song(file_hash) {
        return;
    }

    if let Some(lang) = language
        && !lang.is_empty()
    {
        let mut config = AppConfig::load();
        config.set_language_override(file_hash.to_string(), lang);
        if let Err(error) = config.save() {
            tracing::error!("Could not save language override: {error}");
            return;
        }
    }
    reanalyze(file_hash, false);
}

pub fn reanalyze_full(file_hash: &str) {
    if is_usdx_song(file_hash) {
        return;
    }

    reanalyze(file_hash, true);
}

/// Clears cached pitch evidence so `pitch.extract` regenerates it.
/// Deliberately does **not** touch the Authored Chart
/// (docs/analysis-dag-redesign.md §6/Phase 5): a pitch-only rerun must
/// leave the user's edited chart in place, with new evidence available to
/// compare/merge rather than silently replacing it.
/// Renames `path` to a sibling `.bak` path if a file currently exists
/// there, returning `(original, backup)` for `restore_or_commit_backup` to
/// resolve once the triggered run finishes. Used instead of deleting
/// outright so a run that fails, crashes, or gets OOM-killed doesn't
/// destroy the previous good output for nothing.
pub(crate) fn back_up_before_reset(path: &Path) -> Option<(PathBuf, PathBuf)> {
    if !path.is_file() {
        return None;
    }
    let mut backup_name = path.as_os_str().to_os_string();
    backup_name.push(".bak");
    let backup = PathBuf::from(backup_name);
    // Clear out a stale backup left behind by some earlier run that never
    // got resolved (shouldn't happen -- restore_or_commit_backup always
    // consumes it -- but a leftover .bak must never silently become "the"
    // backup for two different runs).
    let _ = std::fs::remove_file(&backup);
    std::fs::rename(path, &backup)
        .ok()
        .map(|()| (path.to_path_buf(), backup))
}

/// Resolves one backed-up file once its triggering run has finished,
/// regardless of *how* it finished: if a fresh file now exists at the
/// original path, the run produced new output and the backup is no longer
/// needed. Otherwise (failure, crash, or a node that quietly skipped itself
/// -- e.g. `analyze_pitch`'s own exception handler in pipeline.py logs and
/// continues rather than failing the whole run) the old data is restored.
/// Deliberately existence-based rather than gated on `SongResult`, since a
/// single failed node doesn't necessarily surface as an overall run
/// failure.
pub(crate) fn restore_or_commit_backup(original: &Path, backup: &Path) {
    if original.is_file() {
        let _ = std::fs::remove_file(backup);
    } else {
        let _ = std::fs::rename(backup, original);
    }
}

pub(crate) fn apply_pitch_reanalysis_reset(
    cache: &CacheDir,
    file_hash: &str,
) -> Vec<(PathBuf, PathBuf)> {
    [
        cache.pitch_track_path(file_hash),
        cache.pitch_notes_path(file_hash),
    ]
    .into_iter()
    .filter_map(|path| back_up_before_reset(&path))
    .collect()
}

pub fn reanalyze_pitch(file_hash: &str) {
    if is_usdx_song(file_hash) {
        return;
    }

    let cache = CacheDir::new();
    let backups = apply_pitch_reanalysis_reset(&cache, file_hash);
    let mut intents = PENDING_NODE_INTENTS.lock().unwrap();
    let intent = intents.entry(file_hash.to_string()).or_default();
    intent
        .targets
        .insert(crate::analysis_graph::AnalysisNodeId::new("pitch.extract"));
    intent.backup_paths.extend(backups);
    drop(intents);
    enqueue_one(file_hash);
}

/// Drops the transcript so `lyrics.align` regenerates it from the (possibly
/// re-fetched) lyrics source. Preserves the Authored Chart -- realigning
/// must not throw away chart edits just because word timing is about to be
/// recomputed (docs/analysis-dag-redesign.md §6/Phase 5). Same
/// backup-before-delete pattern as `apply_pitch_reanalysis_reset` (Phase 5
/// "失败时保留旧 Pitch"): renames the transcript and each variant aside
/// instead of deleting outright, so a crashed/cancelled realign doesn't
/// destroy the previous good transcript for nothing.
pub(crate) fn apply_realign_reset(cache: &CacheDir, file_hash: &str) -> Vec<(PathBuf, PathBuf)> {
    [
        cache.transcript_path(file_hash),
        cache.recognized_text_path(file_hash),
        cache.asr_segments_path(file_hash),
        cache.timed_transcript_path(file_hash),
    ]
    .into_iter()
    .chain(cache.transcript_variant_paths(file_hash))
    .filter_map(|path| back_up_before_reset(&path))
    .collect()
}

pub fn realign(file_hash: &str, language: Option<String>) {
    if is_usdx_song(file_hash) {
        return;
    }

    if let Some(lang) = language.as_ref().filter(|lang| !lang.is_empty()) {
        let mut config = AppConfig::load();
        config.set_language_override(file_hash.to_string(), lang.clone());
        if let Err(error) = config.save() {
            tracing::error!("Could not save language override: {error}");
            return;
        }
    }

    let cache = CacheDir::new();
    let previous_language = library_db::load_song_by_hash(file_hash)
        .ok()
        .flatten()
        .and_then(|song| song.language);
    materialize_lyrics_from_transcript(&cache, file_hash);
    let backups = apply_realign_reset(&cache, file_hash);
    PENDING_NODE_INTENTS
        .lock()
        .unwrap()
        .entry(file_hash.to_string())
        .or_default()
        .backup_paths
        .extend(backups);
    update_song_analyzed(
        file_hash,
        false,
        language.or(previous_language),
        None,
        None,
        None,
        None,
    );
    enqueue_one(file_hash);
}

pub fn reanalyze_force_transcribe(file_hash: &str) {
    if is_usdx_song(file_hash) {
        return;
    }

    PENDING_NODE_INTENTS
        .lock()
        .unwrap()
        .entry(file_hash.to_string())
        .or_default()
        .force_transcribe = true;

    reanalyze(file_hash, false);
}

/// Full or transcript-only reanalysis reset. Neither branch touches the
/// Authored Chart: "Reanalyze all" regenerating every analysis artifact
/// still must not discard the user's chart edits by default
/// (docs/analysis-dag-redesign.md §6/Phase 5; phase plan Phase 9 test
/// "Full Reanalysis 默认保留 Authored Chart"). Same backup-before-delete
/// pattern as `apply_pitch_reanalysis_reset`/`apply_realign_reset`, made
/// possible for the `full` branch's larger, directory-scanned file set by
/// `CacheDir::analysis_output_paths_keep_chart` -- a real enumeration of
/// what `delete_analysis_outputs_keep_chart` would remove, not a
/// hand-maintained duplicate list that could drift from it.
pub(crate) fn apply_reanalyze_reset(
    cache: &CacheDir,
    file_hash: &str,
    full: bool,
) -> Vec<(PathBuf, PathBuf)> {
    let paths: Vec<PathBuf> = if full {
        cache.analysis_output_paths_keep_chart(file_hash)
    } else {
        [
            cache.transcript_path(file_hash),
            cache.recognized_text_path(file_hash),
            cache.asr_segments_path(file_hash),
            cache.timed_transcript_path(file_hash),
            cache.lyrics_path(file_hash),
        ]
        .into_iter()
        .chain(cache.transcript_variant_paths(file_hash))
        .collect()
    };
    paths
        .into_iter()
        .filter_map(|path| back_up_before_reset(&path))
        .collect()
}

pub(crate) fn reanalyze(file_hash: &str, full: bool) {
    let cache = CacheDir::new();
    let backups = apply_reanalyze_reset(&cache, file_hash, full);
    PENDING_NODE_INTENTS
        .lock()
        .unwrap()
        .entry(file_hash.to_string())
        .or_default()
        .backup_paths
        .extend(backups);
    update_song_analyzed(file_hash, false, None, None, None, None, None);
    enqueue_one(file_hash);
}

pub(crate) fn materialize_lyrics_from_transcript(cache: &CacheDir, file_hash: &str) {
    if cache.lyrics_path(file_hash).is_file() {
        return;
    }

    let transcript_path = cache.transcript_path(file_hash);
    let Ok(data) = std::fs::read_to_string(&transcript_path) else {
        return;
    };

    #[derive(Deserialize)]
    struct Segment {
        #[serde(default)]
        text: String,
    }

    #[derive(Deserialize)]
    struct TranscriptShape {
        #[serde(default)]
        segments: Vec<Segment>,
    }

    let Ok(parsed) = serde_json::from_str::<TranscriptShape>(&data) else {
        return;
    };

    let lines: Vec<String> = parsed
        .segments
        .into_iter()
        .map(|s| s.text.trim().to_string())
        .filter(|l| !l.is_empty())
        .collect();

    if lines.is_empty() {
        return;
    }

    if let Err(e) = write_lyrics_file(cache, file_hash, &lines) {
        warn!("[analyzer] Failed to materialize lyrics from transcript for {file_hash}: {e}");
    }
}

pub(crate) fn normalize_analysis_language(language: &str) -> String {
    let normalized = language.trim().to_ascii_lowercase().replace('_', "-");
    match normalized.as_str() {
        "jp" | "jpn" => "ja".into(),
        "eng" => "en".into(),
        "kor" => "ko".into(),
        "chi" | "zho" | "cn" | "zh-cn" | "zh-tw" => "zh".into(),
        _ => normalized,
    }
}
