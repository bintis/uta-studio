use super::*;

pub fn delete_cache(file_hash: &str) {
    if is_usdx_song(file_hash) {
        return;
    }
    CacheDir::new().delete_song_cache(file_hash);
    update_song_analyzed(file_hash, false, None, None, None, None, None);
}

/// Removes a song's library entry along with its generated cache, queue, and
/// analysis-profile rows. The indexed source file is never touched -- a
/// later library scan re-discovers it as a fresh, unanalyzed song.
pub fn remove_song_from_library(file_hash: &str) -> Result<(), String> {
    stop_analysis_before_song_removal(file_hash)?;
    CacheDir::new().delete_song_cache(file_hash);
    library_db::analysis_queue_delete(file_hash).map_err(|error| error.to_string())?;
    library_db::song_analysis_profile_delete(file_hash).map_err(|error| error.to_string())?;
    library_db::delete_song_by_hash(file_hash).map_err(|error| error.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::song::{Song, SongOrigin};

    fn temp_root(label: &str) -> std::path::PathBuf {
        let nonce = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos();
        let path = std::env::temp_dir().join(format!(
            "uta-studio-remove-song-test-{label}-{}-{nonce}",
            std::process::id()
        ));
        std::fs::create_dir_all(&path).expect("create temp db root");
        path
    }

    #[test]
    fn removing_a_failed_song_clears_its_rows_but_keeps_the_source_file() {
        let root = temp_root("basic");
        let _guard = library_db::reconnect_for_test(&root);

        let file_hash = "failed-song";
        let source_path = root.join("failed-song.flac");
        std::fs::write(&source_path, b"not real audio").unwrap();

        let song = Song {
            path: source_path.clone(),
            file_hash: file_hash.to_string(),
            title: "Failed".to_string(),
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
        library_db::replace_all_songs_sorted(&[song]).unwrap();
        library_db::analysis_queue_upsert_row(file_hash, "failed", None, Some("boom")).unwrap();
        library_db::song_analysis_profile_set(file_hash, "{}", 1).unwrap();

        remove_song_from_library(file_hash).expect("remove succeeds");

        assert!(library_db::load_song_by_hash(file_hash).unwrap().is_none());
        assert!(
            library_db::analysis_queue_status(file_hash)
                .unwrap()
                .is_none()
        );
        assert!(
            library_db::song_analysis_profile_get(file_hash)
                .unwrap()
                .is_none()
        );
        assert!(source_path.is_file(), "source media must not be deleted");

        std::fs::remove_dir_all(&root).ok();
    }
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
