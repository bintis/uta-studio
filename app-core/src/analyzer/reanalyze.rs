use super::*;

pub fn shutdown_server() {
    // AnalysisCliClient owns and reaps each uta-analyze process. There is no
    // shared compatibility analyzer server to stop at application shutdown.
}

pub fn delete_cache(file_hash: &str) {
    if is_usdx_song(file_hash) {
        return;
    }
    let cache = CacheDir::new();
    cache.delete_song_cache(file_hash);
    update_song_analyzed(file_hash, false, None, None, None, None, None);
}

pub fn reanalyze_transcript(file_hash: &str, language: Option<String>) -> Result<(), String> {
    if is_usdx_song(file_hash) {
        return Err("this action is unavailable for imported USDX charts".to_string());
    }

    if let Some(lang) = language
        && !lang.is_empty()
    {
        let mut config = AppConfig::load();
        config.set_language_override(file_hash.to_string(), lang);
        config
            .save()
            .map_err(|error| format!("Could not save language override: {error}"))?;
    }
    queue_engine_reanalysis(
        file_hash,
        crate::analysis_experience::AnalysisDefaultTarget::Transcript,
    )
}

pub fn reanalyze_full(file_hash: &str) -> Result<(), String> {
    if is_usdx_song(file_hash) {
        return Err("this action is unavailable for imported USDX charts".to_string());
    }
    queue_engine_reanalysis(
        file_hash,
        crate::analysis_experience::AnalysisDefaultTarget::FullCandidate,
    )
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
#[cfg(test)]
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
#[cfg(test)]
pub(crate) fn restore_or_commit_backup(original: &Path, backup: &Path) {
    if original.is_file() {
        let _ = std::fs::remove_file(backup);
    } else {
        let _ = std::fs::rename(backup, original);
    }
}

#[cfg(test)]
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

pub fn reanalyze_pitch(file_hash: &str) -> Result<(), String> {
    if is_usdx_song(file_hash) {
        return Err("this action is unavailable for imported USDX charts".to_string());
    }
    queue_engine_reanalysis(
        file_hash,
        crate::analysis_experience::AnalysisDefaultTarget::PitchEvidence,
    )
}

/// Drops the transcript so `lyrics.align` regenerates it from the (possibly
/// re-fetched) lyrics source. Preserves the Authored Chart -- realigning
/// must not throw away chart edits just because word timing is about to be
/// recomputed (docs/analysis-dag-redesign.md §6/Phase 5). Same
/// backup-before-delete pattern as `apply_pitch_reanalysis_reset` (Phase 5
/// "失败时保留旧 Pitch"): renames the transcript and each variant aside
/// instead of deleting outright, so a crashed/cancelled realign doesn't
/// destroy the previous good transcript for nothing.
#[cfg(test)]
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

pub fn realign(file_hash: &str, language: Option<String>) -> Result<(), String> {
    if is_usdx_song(file_hash) {
        return Err("this action is unavailable for imported USDX charts".to_string());
    }

    if let Some(lang) = language.as_ref().filter(|lang| !lang.is_empty()) {
        let mut config = AppConfig::load();
        config.set_language_override(file_hash.to_string(), lang.clone());
        config
            .save()
            .map_err(|error| format!("Could not save language override: {error}"))?;
    }
    materialize_lyrics_from_transcript(&CacheDir::new(), file_hash);
    queue_engine_reanalysis(
        file_hash,
        crate::analysis_experience::AnalysisDefaultTarget::Alignment,
    )
}

pub fn reanalyze_force_transcribe(file_hash: &str) -> Result<(), String> {
    if is_usdx_song(file_hash) {
        return Err("this action is unavailable for imported USDX charts".to_string());
    }
    queue_engine_reanalysis(
        file_hash,
        crate::analysis_experience::AnalysisDefaultTarget::Transcript,
    )
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
#[cfg(test)]
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

#[cfg(test)]
pub(crate) fn reanalyze(file_hash: &str, full: bool) -> Result<(), String> {
    queue_engine_reanalysis(
        file_hash,
        if full {
            crate::analysis_experience::AnalysisDefaultTarget::FullCandidate
        } else {
            crate::analysis_experience::AnalysisDefaultTarget::Transcript
        },
    )
}

fn queue_engine_reanalysis(
    file_hash: &str,
    target: crate::analysis_experience::AnalysisDefaultTarget,
) -> Result<(), String> {
    crate::analysis_engine_adapter::preview_and_queue_engine_run(file_hash, Some(target))
        .map(|_| ())
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

#[cfg(test)]
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
