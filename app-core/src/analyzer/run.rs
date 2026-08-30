use super::*;

/// Runs only persisted, versioned Analysis Engine intents. Queue rows without
/// an exact request/plan snapshot fail closed and can never enter another
/// execution path.
pub(crate) fn spawn_worker() {
    std::thread::spawn(|| {
        let cache = CacheDir::new();
        loop {
            let file_hash = {
                let mut state = ANALYZER.lock().unwrap();
                match state.queue.pop_front() {
                    Some(hash) => {
                        state.active_hash = Some(hash.clone());
                        hash
                    }
                    None => {
                        state.worker_running = false;
                        if let Some(file_hash) = state.active_hash.take() {
                            clear_force_stop_request(&file_hash);
                        }
                        ANALYZER_STATE_CHANGED.notify_all();
                        return;
                    }
                }
            };

            match library_db::analysis_queue_engine_intent(&file_hash) {
                Ok(Some(intent)) => {
                    super::engine_run::process_engine_queue_intent(&file_hash, &cache, intent)
                }
                Ok(None) => reject_unversioned_queue_entry(
                    &file_hash,
                    "analysis queue entry has no exact Engine request snapshot",
                ),
                Err(error) => reject_unversioned_queue_entry(
                    &file_hash,
                    &format!("could not load exact Engine request snapshot: {error}"),
                ),
            }

            ANALYZER.lock().unwrap().active_hash = None;
            clear_force_stop_request(&file_hash);
            ANALYZER_STATE_CHANGED.notify_all();
        }
    });
}

fn reject_unversioned_queue_entry(file_hash: &str, reason: &str) {
    let message = format!("exact Analysis Engine request required: {reason}");
    update_queue_status(file_hash, QueuedStatus::Failed(message.clone()));
    finish_analysis_history(file_hash, "failed", Some(&message));
    LIVE_ANALYSIS.lock().unwrap().remove(file_hash);
}

/// Makes an imported timed-LRC song immediately editable over its authorized
/// original mix. This is Studio authoring state, not analysis execution.
pub fn prepare_lrc_no_stems(file_hash: &str) -> Result<(), UtaStudioError> {
    let Some(mut song) = library_db::load_song_by_hash(file_hash).ok().flatten() else {
        return Err(UtaStudioError::Other("Song not found".into()));
    };
    validate_analysis_source(&song.path)?;
    song.is_analyzed = true;
    song.transcript_source = Some(TranscriptSource::Lrc);
    song.key = None;
    song.override_key = None;
    song.bpm = None;
    song.tempo = 1.0;
    song.key_offset = 0;
    song.no_stems = true;
    library_db::update_song_fields(file_hash, &song)
        .map_err(|error| UtaStudioError::Other(error.to_string()))
}

fn validate_analysis_source(path: &Path) -> Result<(), UtaStudioError> {
    let metadata = std::fs::metadata(path)?;
    if !metadata.is_file() {
        return Err(UtaStudioError::Other(format!(
            "source media is not a file: {}",
            path.display()
        )));
    }
    if metadata.len() == 0 {
        return Err(UtaStudioError::Other(format!(
            "source media is empty: {}",
            path.display()
        )));
    }
    Ok(())
}
