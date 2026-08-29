use super::*;

pub fn delete_cache(file_hash: &str) {
    if is_usdx_song(file_hash) {
        return;
    }
    CacheDir::new().delete_song_cache(file_hash);
    update_song_analyzed(file_hash, false, None, None, None, None, None);
}

pub fn reanalyze_transcript(file_hash: &str, language: Option<String>) -> Result<(), String> {
    ensure_reanalysis_supported(file_hash)?;
    save_language_override(file_hash, language)?;
    queue_engine_reanalysis(
        file_hash,
        crate::analysis_experience::AnalysisDefaultTarget::Transcript,
    )
}

pub fn reanalyze_full(file_hash: &str) -> Result<(), String> {
    ensure_reanalysis_supported(file_hash)?;
    queue_engine_reanalysis(
        file_hash,
        crate::analysis_experience::AnalysisDefaultTarget::FullCandidate,
    )
}

pub fn reanalyze_pitch(file_hash: &str) -> Result<(), String> {
    ensure_reanalysis_supported(file_hash)?;
    queue_engine_reanalysis(
        file_hash,
        crate::analysis_experience::AnalysisDefaultTarget::PitchEvidence,
    )
}

pub fn realign(file_hash: &str, language: Option<String>) -> Result<(), String> {
    ensure_reanalysis_supported(file_hash)?;
    save_language_override(file_hash, language)?;
    materialize_lyrics_from_transcript(&CacheDir::new(), file_hash);
    queue_engine_reanalysis(
        file_hash,
        crate::analysis_experience::AnalysisDefaultTarget::Alignment,
    )
}

pub fn reanalyze_force_transcribe(file_hash: &str) -> Result<(), String> {
    reanalyze_transcript(file_hash, None)
}

fn ensure_reanalysis_supported(file_hash: &str) -> Result<(), String> {
    if is_usdx_song(file_hash) {
        Err("this action is unavailable for imported USDX charts".to_string())
    } else {
        Ok(())
    }
}

fn save_language_override(file_hash: &str, language: Option<String>) -> Result<(), String> {
    let Some(language) = language.filter(|language| !language.is_empty()) else {
        return Ok(());
    };
    let mut config = AppConfig::load();
    config.set_language_override(file_hash.to_string(), language);
    config
        .save()
        .map_err(|error| format!("Could not save language override: {error}"))
}

fn queue_engine_reanalysis(
    file_hash: &str,
    target: crate::analysis_experience::AnalysisDefaultTarget,
) -> Result<(), String> {
    crate::analysis_engine_adapter::preview_and_queue_engine_run(file_hash, Some(target))
        .map(|_| ())
}

fn materialize_lyrics_from_transcript(cache: &CacheDir, file_hash: &str) {
    if cache.lyrics_path(file_hash).is_file() {
        return;
    }
    let Ok(data) = std::fs::read_to_string(cache.transcript_path(file_hash)) else {
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
    let lines = parsed
        .segments
        .into_iter()
        .map(|segment| segment.text.trim().to_string())
        .filter(|line| !line.is_empty())
        .collect::<Vec<_>>();
    if !lines.is_empty()
        && let Err(error) = write_lyrics_file(cache, file_hash, &lines)
    {
        warn!("[analyzer] Failed to materialize lyrics for {file_hash}: {error}");
    }
}
