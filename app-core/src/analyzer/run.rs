use super::*;

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
                        state.active_hash = None;
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

            let mut state = ANALYZER.lock().unwrap();
            state.active_hash = None;
        }
    });
}

pub(crate) fn reject_unversioned_queue_entry(file_hash: &str, reason: &str) {
    let message =
        format!("legacy analyzer execution is retired; rebuild an exact Plan Preview: {reason}");
    update_queue_status(file_hash, QueuedStatus::Failed(message.clone()));
    finish_analysis_history(file_hash, "failed", Some(&message));
    LIVE_ANALYSIS.lock().unwrap().remove(file_hash);
    PENDING_NODE_INTENTS.lock().unwrap().remove(file_hash);
    FROZEN_CONFIGS.lock().unwrap().remove(file_hash);
}

/// Removes only crash/stop leftovers created by this song's atomic writers.
/// Final cache paths and immutable artifact revisions never match this
/// dot-prefixed temporary naming convention and are therefore preserved.
#[cfg(test)]
pub(crate) fn cleanup_unfinished_output_temps(cache: &CacheDir, file_hash: &str) {
    let prefix = format!(".{file_hash}_");
    let Ok(entries) = std::fs::read_dir(&cache.path) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        let Some(name) = path.file_name().and_then(|name| name.to_str()) else {
            continue;
        };
        if name.starts_with(&prefix) && name.contains(".tmp") {
            let _ = std::fs::remove_file(path);
        }
    }
}

#[cfg(test)]
pub(crate) fn process_song(initial_hash: &str, cache: &CacheDir) {
    let started_at_ms = unix_time_ms();
    ANALYSIS_STARTED
        .lock()
        .unwrap()
        .insert(initial_hash.to_string(), started_at_ms);
    let analysis_log_path = create_analysis_log(initial_hash, started_at_ms);
    append_analysis_log_node_event(
        analysis_log_path.as_deref(),
        "preflight",
        "started",
        0,
        "Validating source media",
    );
    update_queue_status(initial_hash, QueuedStatus::Analyzing(0));
    update_live_analysis(
        initial_hash,
        AnalysisProgressSnapshot {
            stage: "preparing".into(),
            overall_progress: 0,
            stage_progress: 0,
            operation: "Validating source media".into(),
            detail: "Checking the source before the analysis runtime starts.".into(),
            implementation: "Uta! Studio native preflight".into(),
            model: "Source validation".into(),
            device: "CPU".into(),
            requested_device: "CPU".into(),
            fallback_from: None,
            fallback_reason: None,
            backend_fallback_from: None,
            backend_fallback_reason: None,
            stage_routes: vec![AnalysisStageRoute {
                stage: "preparing".into(),
                node_id: Some("preflight".into()),
                node_event: Some("started".into()),
                binding_kind: None,
                committed_outputs: Vec::new(),
                input_revision_ids: Vec::new(),
                operation: "Validating source media".into(),
                implementation: "Uta! Studio native preflight".into(),
                model: "Source validation".into(),
                stage_progress: 0,
                requested_device: "cpu".into(),
                actual_device: "cpu".into(),
                fallback_from: None,
                fallback_reason: None,
                backend_fallback_from: None,
                backend_fallback_reason: None,
                started_at_ms: Some(started_at_ms),
                finished_at_ms: None,
                event_at_ms: Some(started_at_ms),
                work_units_completed: None,
                work_units_total: None,
            }],
            node_id: Some("preflight".to_string()),
            node_event: Some("started".to_string()),
            artifact_reused_reason: None,
            analysis_log_path: analysis_log_path.clone(),
            engine: None,
        },
    );
    // Note: a `reanalyze_pitch`-style backup recorded into
    // `PENDING_NODE_INTENTS` (see `resolve_backups` further down) isn't
    // drained or resolved by either early return below -- the song record
    // vanishing from the DB, or the source file failing to prepare, between
    // enqueue and this point. Both require the song to already have had a
    // successful prior analysis (for there to be anything to back up) and
    // then fail in this specific narrow window, which is rare; the residual
    // risk is an orphaned `.bak` file next to the original cache entry, not
    // silent data loss -- strictly better than the pre-fix behavior, even
    // though it isn't auto-restored here.
    let Some(song) = library_db::load_song_by_hash(initial_hash).ok().flatten() else {
        let failed_at_ms = unix_time_ms();
        if let Some(snapshot) = LIVE_ANALYSIS.lock().unwrap().get_mut(initial_hash) {
            snapshot.node_event = Some("failed".into());
            snapshot.detail = "Song record disappeared before analysis could start".into();
            if let Some(route) = snapshot.stage_routes.first_mut() {
                route.node_event = Some("failed".into());
                route.finished_at_ms = Some(failed_at_ms);
                route.event_at_ms = Some(failed_at_ms);
            }
        }
        append_analysis_log_node_event(
            analysis_log_path.as_deref(),
            "preflight",
            "failed",
            0,
            "Song record disappeared before analysis could start",
        );
        append_analysis_log_path(
            analysis_log_path.as_deref(),
            "song record disappeared before analysis could start",
        );
        finish_analysis_history(
            initial_hash,
            "failed",
            Some("song record disappeared before analysis could start"),
        );
        remove_from_queue(initial_hash);
        LIVE_ANALYSIS.lock().unwrap().remove(initial_hash);
        return;
    };

    let (song, local_path, file_hash_owned) = match prepare_audio_for_analysis(&song, cache) {
        Ok(out) => out,
        Err(e) => {
            let failed_at_ms = unix_time_ms();
            if let Some(snapshot) = LIVE_ANALYSIS.lock().unwrap().get_mut(initial_hash) {
                snapshot.node_event = Some("failed".into());
                snapshot.detail = e.to_string();
                if let Some(route) = snapshot.stage_routes.first_mut() {
                    route.node_event = Some("failed".into());
                    route.finished_at_ms = Some(failed_at_ms);
                    route.event_at_ms = Some(failed_at_ms);
                }
            }
            append_analysis_log_node_event(
                analysis_log_path.as_deref(),
                "preflight",
                "failed",
                0,
                &e.to_string(),
            );
            append_analysis_log_path(
                analysis_log_path.as_deref(),
                &format!("source preparation failed: {e}"),
            );
            update_queue_status(
                initial_hash,
                QueuedStatus::Failed(format!("audio prep failed: {e}")),
            );
            finish_analysis_history(initial_hash, "failed", Some(&e.to_string()));
            return;
        }
    };
    let file_hash = file_hash_owned.as_str();

    if file_hash != initial_hash {
        let snapshot = LIVE_ANALYSIS.lock().unwrap().remove(initial_hash);
        if let Some(snapshot) = snapshot {
            LIVE_ANALYSIS
                .lock()
                .unwrap()
                .insert(file_hash.to_string(), snapshot);
        }
        let started = ANALYSIS_STARTED.lock().unwrap().remove(initial_hash);
        if let Some(started) = started {
            ANALYSIS_STARTED
                .lock()
                .unwrap()
                .insert(file_hash.to_string(), started);
        }
    }

    append_analysis_log_path(
        analysis_log_path.as_deref(),
        &format!(
            "starting source={} file_hash={file_hash}",
            local_path.display()
        ),
    );

    update_queue_status(file_hash, QueuedStatus::Analyzing(0));

    // Node targeting for this run. The intent may have been keyed by the
    // pre-rekey hash for remote songs, so both are drained and merged.
    let intent = {
        let mut intents = PENDING_NODE_INTENTS.lock().unwrap();
        let current = intents.remove(file_hash);
        let initial = if file_hash != initial_hash {
            intents.remove(initial_hash)
        } else {
            None
        };
        match (current, initial) {
            (Some(mut a), Some(b)) => {
                a.targets.extend(b.targets);
                a.force_transcribe |= b.force_transcribe;
                a.backup_paths.extend(b.backup_paths);
                a.disabled_nodes.extend(b.disabled_nodes);
                a.frozen_artifacts.extend(b.frozen_artifacts);
                a.bypassed_nodes.extend(b.bypassed_nodes);
                a.run_override = a.run_override.or(b.run_override);
                a.workflow_execution = a.workflow_execution.or(b.workflow_execution);
                Some(a)
            }
            (Some(a), None) | (None, Some(a)) => Some(a),
            (None, None) => None,
        }
    };
    let node_targets = intent
        .as_ref()
        .map(|i| i.targets.clone())
        .unwrap_or_default();
    let disabled_nodes = intent
        .as_ref()
        .map(|i| i.disabled_nodes.clone())
        .unwrap_or_default();
    let frozen_artifacts = intent
        .as_ref()
        .map(|i| i.frozen_artifacts.clone())
        .unwrap_or_default();
    let bypassed_nodes = intent
        .as_ref()
        .map(|i| i.bypassed_nodes.clone())
        .unwrap_or_default();
    let run_override = intent.as_ref().and_then(|i| i.run_override.clone());
    let workflow_execution = intent
        .as_ref()
        .and_then(|intent| intent.workflow_execution.clone());
    let capture_intermediate = intent
        .as_ref()
        .and_then(|intent| intent.capture_intermediate.clone());
    if let Some(request) = capture_intermediate.clone() {
        ACTIVE_CAPTURE_REQUESTS
            .lock()
            .unwrap()
            .insert(file_hash.to_string(), request);
    }
    let force_transcribe = intent.as_ref().map(|i| i.force_transcribe).unwrap_or(false);
    // Resolved (committed or restored) at every exit point below,
    // regardless of outcome -- see `restore_or_commit_backup`.
    let backup_paths = intent
        .as_ref()
        .map(|i| i.backup_paths.clone())
        .unwrap_or_default();
    let run_work_dir = std::env::temp_dir().join(format!(
        "uta-studio-analysis-{}-{}",
        std::process::id(),
        unix_time_ms()
    ));
    let _ = std::fs::create_dir_all(&run_work_dir);
    let resolve_backups = || {
        for (original, backup) in &backup_paths {
            restore_or_commit_backup(original, backup);
        }
        ACTIVE_CAPTURE_REQUESTS.lock().unwrap().remove(file_hash);
        let _ = std::fs::remove_dir_all(&run_work_dir);
    };

    if !node_targets.is_empty() && file_hash != initial_hash {
        // Move the pre-written transcript to the rekeyed hash so the pass can
        // patch it in place.
        let _ = std::fs::rename(
            cache.transcript_path(initial_hash),
            cache.transcript_path(file_hash),
        );
    }

    // Phase 4: real disabled_nodes are threaded through here now, not just
    // targets -- `run_analysis_plan`/`disable_analysis_node_for_run` are the
    // only callers that ever populate `disabled_nodes` for a legacy
    // special-case function (empty set, so this is behavior-preserving for
    // them). The `Err` fallback mirrors `pipeline_flags_for_targets`'s own
    // fail-open: `run_analysis_plan` already rejects an unhonorable disable
    // before it's ever queued, so this should be unreachable in practice.
    let flags = if workflow_execution.is_some() {
        PipelineFlags::default()
    } else {
        pipeline_flags_for_request(
            &node_targets,
            &disabled_nodes,
            &frozen_artifacts,
            &bypassed_nodes,
        )
        .unwrap_or_default()
    };
    let PipelineFlags {
        skip_transcription,
        skip_separation,
        skip_pitch,
        freeze_separation,
        freeze_pitch,
        bypass_separation,
    } = flags;

    // Phase 4 §4.1: the config this job actually runs with is the snapshot
    // frozen at enqueue time (`enqueue_one`/`enqueue_all`), not whatever
    // the user has changed global settings to since then.
    let config = resolve_frozen_config(file_hash, initial_hash, AppConfig::load);

    // Phase 8: the three profile-controlled knobs (separator/asr engine/
    // align backend) now actually resolve through the Global Defaults ->
    // Song Profile -> Run Override chain, instead of reading `config`
    // directly -- `get_song_analysis_profile`/`run_override` used to be
    // decorative (preview-only, see `preview_full_analysis_plan`); this is
    // the one place real execution honors them.
    let profile_global =
        crate::analysis_profile::AnalysisProfileSnapshot::from_app_config(&config, file_hash);
    let song_profile = crate::analysis_profile::get_song_analysis_profile(file_hash);
    let run_override_for = |field: crate::analysis_profile::ProfileField| {
        run_override
            .as_ref()
            .filter(|(f, _)| *f == field)
            .map(|(_, value)| value.as_str())
    };
    let effective_separator = crate::analysis_profile::resolve_profile_field(
        crate::analysis_profile::ProfileField::Separator,
        &profile_global,
        song_profile.as_ref(),
        run_override_for(crate::analysis_profile::ProfileField::Separator),
    )
    .value;
    let effective_asr_engine = crate::analysis_profile::resolve_profile_field(
        crate::analysis_profile::ProfileField::AsrEngine,
        &profile_global,
        song_profile.as_ref(),
        run_override_for(crate::analysis_profile::ProfileField::AsrEngine),
    )
    .value;
    let effective_align_backend = crate::analysis_profile::resolve_profile_field(
        crate::analysis_profile::ProfileField::AlignmentBackend,
        &profile_global,
        song_profile.as_ref(),
        run_override_for(crate::analysis_profile::ProfileField::AlignmentBackend),
    )
    .value;

    let skip_lrclib = skip_transcription || force_transcribe;
    let lyrics_path = if skip_lrclib {
        None
    } else {
        fetch_lrclib_lyrics(&song, cache)
    };

    let audio_settings = config.audio_processing.clone().unwrap_or_else(|| {
        crate::audio_processing::AudioProcessingSettings::from_legacy_separator(
            &effective_separator,
        )
    });
    let audio_processing =
        crate::audio_processing::AudioProcessingPlanSnapshot::from_settings(&audio_settings);
    let mut cmd_json = serde_json::json!({
        "type": "analyze",
        "protocol": crate::native_runtime::NATIVE_WORKER_PROTOCOL_VERSION,
        "audio_path": local_path.to_string_lossy(),
        "cache_path": cache.path.to_string_lossy(),
        "hash": file_hash,
        "model": config.whisper_model(),
        "beam_size": config.beam_size(),
        "batch_size": config.batch_size(),
        "separator": effective_separator,
        "separator_options": {
            "segment_size": config.separator_segment_size,
            "overlap": config.separator_overlap(),
            "batch_size": config.separator_batch_size(),
            "normalization_pct": config.separator_normalization_pct(),
        },
        "audio_processing": audio_processing,
        "run_work_dir": run_work_dir,
        "analysis_log_path": analysis_log_path.clone(),
        "node_weights": historical_progress_weights(),
        "engine": effective_asr_engine,
        "align_backend": effective_align_backend,
        "vocal_detection_threshold_pct": config.vocal_detection_threshold_pct(),
    });

    if skip_transcription {
        cmd_json["skip_transcription"] = serde_json::json!(true);
    }
    if skip_separation {
        cmd_json["skip_separation"] = serde_json::json!(true);
    }
    if skip_pitch {
        cmd_json["skip_pitch"] = serde_json::json!(true);
    }
    if freeze_separation {
        cmd_json["freeze_separation"] = serde_json::json!(true);
    }
    if freeze_pitch {
        cmd_json["freeze_pitch"] = serde_json::json!(true);
    }
    if bypass_separation {
        cmd_json["bypass_separation_with_original_mix"] = serde_json::json!(true);
    }
    if capture_intermediate.is_some() {
        cmd_json["capture_preprocessed_audio"] = serde_json::json!(true);
    }
    if let Some(workflow_execution) = workflow_execution {
        cmd_json["workflow_execution"] =
            serde_json::to_value(workflow_execution).unwrap_or(serde_json::Value::Null);
    }

    if let Some(ref lp) = lyrics_path {
        cmd_json["lyrics"] = serde_json::json!(lp.to_string_lossy());
    }
    let language_hint = config
        .language_override(file_hash)
        .map(str::to_string)
        .or_else(|| lyrics_path.as_ref().and_then(|_| song.language.clone()))
        .map(|language| normalize_analysis_language(&language))
        .filter(|lang| {
            // "unknown"/empty is not a real language: passing it as a forced
            // alignment language crashes native aligner, so let the worker detect it.
            let normalized = lang.trim().to_ascii_lowercase();
            !normalized.is_empty() && normalized != "unknown" && normalized != "und"
        });
    if let Some(lang) = language_hint {
        cmd_json["language"] = serde_json::json!(lang);
    }

    let json_str = serde_json::to_string(&cmd_json).unwrap();
    let mut retried = false;
    let mut attempt = 1;
    append_analysis_log_attempt(analysis_log_path.as_deref(), attempt, None);

    loop {
        let mut guard = ANALYZER_SERVER.lock().unwrap();

        if let Err(e) = ensure_server(&mut guard) {
            append_analysis_log_path(
                analysis_log_path.as_deref(),
                &format!("analyzer service failed to start: {e}"),
            );
            update_queue_status(file_hash, QueuedStatus::Failed(e.to_string()));
            finish_analysis_history(file_hash, "failed", Some(&e.to_string()));
            resolve_backups();
            return;
        }

        let server = guard.as_mut().unwrap();
        match send_and_monitor(server, &json_str, Some(file_hash)) {
            Ok(SongResult::Done) => {
                finalize_song(file_hash, cache);
                resolve_backups();
                return;
            }
            Ok(SongResult::Oom) => {
                append_analysis_log_path(
                    analysis_log_path.as_deref(),
                    "analyzer reported out-of-memory; restarting the service",
                );
                *guard = None;

                if !retried {
                    retried = true;
                    preserve_retry_attempt(file_hash);
                    attempt += 1;
                    append_analysis_log_path(
                        analysis_log_path.as_deref(),
                        "retrying after a clean analyzer restart",
                    );
                    append_analysis_log_attempt(
                        analysis_log_path.as_deref(),
                        attempt,
                        Some("out_of_memory"),
                    );
                    update_queue_status(file_hash, QueuedStatus::Analyzing(0));
                    continue;
                }
                update_queue_status(file_hash, QueuedStatus::Failed("CUDA out of memory".into()));
                finish_analysis_history(file_hash, "failed", Some("CUDA out of memory"));
                resolve_backups();
                return;
            }
            Ok(SongResult::Error(msg)) => {
                append_analysis_log_path(
                    analysis_log_path.as_deref(),
                    &format!("analysis failed: {msg}"),
                );
                update_queue_status(file_hash, QueuedStatus::Failed(msg.clone()));
                finish_analysis_history(file_hash, "failed", Some(&msg));
                resolve_backups();
                return;
            }
            Err(e) => {
                if take_stop_requested(file_hash) {
                    append_analysis_log_path(
                        analysis_log_path.as_deref(),
                        "analysis stopped by the user",
                    );
                    if let Some(snapshot) = LIVE_ANALYSIS.lock().unwrap().get_mut(file_hash) {
                        snapshot.operation = "Analysis stopped".into();
                        snapshot.detail =
                            "Stopped by the user; committed outputs were kept.".into();
                        snapshot.node_event = Some("cancelled".into());
                        if let Some(node_id) = snapshot.node_id.as_deref()
                            && let Some(route) = snapshot
                                .stage_routes
                                .iter_mut()
                                .find(|route| route.node_id.as_deref() == Some(node_id))
                        {
                            route.node_event = Some("cancelled".into());
                            route.finished_at_ms = Some(unix_time_ms());
                        }
                    }
                    *guard = None;
                    cleanup_unfinished_output_temps(cache, file_hash);
                    finish_analysis_history(file_hash, "cancelled", Some("cancelled by user"));
                    remove_from_queue(file_hash);
                    LIVE_ANALYSIS.lock().unwrap().remove(file_hash);
                    resolve_backups();
                    return;
                }
                append_analysis_log_path(
                    analysis_log_path.as_deref(),
                    &format!("analyzer service connection failed: {e}"),
                );
                *guard = None;

                if !retried {
                    retried = true;
                    preserve_retry_attempt(file_hash);
                    attempt += 1;
                    append_analysis_log_path(
                        analysis_log_path.as_deref(),
                        "retrying after analyzer service crash",
                    );
                    append_analysis_log_attempt(
                        analysis_log_path.as_deref(),
                        attempt,
                        Some("analyzer_service_crash"),
                    );
                    update_queue_status(file_hash, QueuedStatus::Analyzing(0));
                    continue;
                }
                update_queue_status(
                    file_hash,
                    QueuedStatus::Failed(format!("Server crashed: {e}")),
                );
                finish_analysis_history(file_hash, "failed", Some(&format!("Server crashed: {e}")));
                resolve_backups();
                return;
            }
        }
    }
}

#[cfg(test)]
pub(crate) fn finalize_song(file_hash: &str, cache: &CacheDir) {
    if cache.transcript_exists(file_hash) {
        let meta = read_transcript_meta(cache, file_hash);
        update_song_analyzed(
            file_hash,
            true,
            meta.language,
            Some(meta.source),
            meta.key,
            meta.bpm,
            Some(meta.tempo),
        );
        if let Some(snapshot) = LIVE_ANALYSIS.lock().unwrap().get_mut(file_hash) {
            snapshot.stage = "complete".into();
            snapshot.overall_progress = 100;
            snapshot.stage_progress = 100;
            snapshot.operation = "Analysis complete".into();
            snapshot.detail = "All requested analysis stages completed successfully.".into();
            if let Some(route) = snapshot
                .stage_routes
                .iter_mut()
                .find(|route| route.stage == "finalizing")
            {
                route.stage_progress = 100;
                route.operation = "Analysis complete".into();
            }
        }
        let log_path = LIVE_ANALYSIS
            .lock()
            .unwrap()
            .get(file_hash)
            .and_then(|snapshot| snapshot.analysis_log_path.clone());
        append_analysis_log_path(log_path.as_deref(), "analysis completed successfully");
        finish_analysis_history(file_hash, "completed", None);
        remove_from_queue(file_hash);
        LIVE_ANALYSIS.lock().unwrap().remove(file_hash);
    } else {
        let message = "Transcript file not found after analysis";
        let log_path = LIVE_ANALYSIS
            .lock()
            .unwrap()
            .get(file_hash)
            .and_then(|snapshot| snapshot.analysis_log_path.clone());
        append_analysis_log_path(log_path.as_deref(), message);
        update_queue_status(file_hash, QueuedStatus::Failed(message.into()));
        finish_analysis_history(file_hash, "failed", Some(message));
    }
}

// ─── LRC (original-mix) preparation ─────────────────────────────────

/// Prepare an LRC-provided song authored over its original mix, without
/// routing it through the analysis status queue.
///
/// This Studio-local work runs synchronously so the song is immediately
/// editable: resolve the local audio and mark the song ready
/// (`source=Lrc`, `no_stems`). Key detection is intentionally not launched
/// through the retired compatibility analyzer; the user may provide musical
/// context explicitly before a future exact Engine request.
pub fn prepare_lrc_no_stems(file_hash: &str) -> Result<(), UtaStudioError> {
    let cache = CacheDir::new();
    let Some(song) = library_db::load_song_by_hash(file_hash).ok().flatten() else {
        return Err(UtaStudioError::Other("Song not found".into()));
    };

    // Resolve the local audio and rekey the row if its content hash changed so
    // all downstream cache files follow the usual layout.
    let (mut song, _local_path, real_hash) = prepare_audio_for_analysis(&song, &cache)?;
    let real_hash = real_hash.to_string();

    // A rekey moves the row — carry the transcript we wrote under the original
    // hash across so the key pass can patch it in place.
    if real_hash != file_hash {
        let _ = std::fs::rename(
            cache.transcript_path(file_hash),
            cache.transcript_path(&real_hash),
        );
    }

    // Mark ready right away (key still unknown) so the original-mix chart is
    // available immediately, before key detection runs.
    song.is_analyzed = true;
    song.transcript_source = Some(TranscriptSource::Lrc);
    song.key = None;
    song.override_key = None;
    song.bpm = None;
    song.tempo = 1.0;
    song.key_offset = 0;
    song.no_stems = true;
    library_db::update_song_fields(&real_hash, &song)
        .map_err(|e| UtaStudioError::Other(e.to_string()))?;
    Ok(())
}

// ─── Local audio preparation ─────────────────────────────────────────

pub(crate) fn validate_analysis_source(path: &Path) -> Result<(), UtaStudioError> {
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

pub(crate) fn prepare_audio_for_analysis(
    song: &Song,
    _cache: &CacheDir,
) -> Result<(Song, PathBuf, String), UtaStudioError> {
    validate_analysis_source(&song.path)?;
    Ok((song.clone(), song.path.clone(), song.file_hash.clone()))
}

// ─── Server communication ────────────────────────────────────────────

#[cfg(test)]
pub(crate) enum SongResult {
    Done,
    Oom,
    Error(String),
}

#[derive(Debug, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
// Keep the adapter's flat JSON wire shape. Boxing the Progress payload would
// either change that contract or require a custom deserializer for no runtime
// benefit: events are consumed one at a time from the child process.
#[allow(clippy::large_enum_variant)]
#[cfg(test)]
pub(crate) enum ServerEvent {
    Progress {
        pct: u32,
        #[serde(default)]
        msg: String,
        #[serde(default)]
        stage: String,
        #[serde(default)]
        stage_progress: usize,
        #[serde(default)]
        operation: String,
        #[serde(default)]
        implementation: String,
        #[serde(default)]
        model: String,
        #[serde(default)]
        device: String,
        #[serde(default)]
        requested_device: String,
        #[serde(default)]
        fallback_from: Option<String>,
        #[serde(default)]
        fallback_reason: Option<String>,
        #[serde(default)]
        backend_fallback_from: Option<String>,
        #[serde(default)]
        backend_fallback_reason: Option<String>,
        #[serde(default)]
        stage_routes: Vec<AnalysisStageRoute>,
        #[serde(default)]
        node_id: Option<String>,
        #[serde(default)]
        event: Option<String>,
        #[serde(default)]
        artifact_reused_reason: Option<String>,
    },
    Done,
    Error {
        #[serde(default)]
        kind: Option<String>,
        #[serde(default)]
        msg: String,
    },
    #[serde(other)]
    Unknown,
}

#[cfg(test)]
#[allow(clippy::items_after_test_module)]
mod node_event_tests {
    use super::*;

    #[test]
    fn progress_event_without_node_fields_still_deserializes() {
        // Legacy Adapter contract (phase plan §3.3): an event from a
        // pipeline call site that hasn't migrated to progress_node must
        // still parse -- node_id/event/artifact_reused_reason all default
        // to None rather than failing the whole event.
        let json = r#"{"type":"progress","pct":4,"msg":"Inspecting source codec..."}"#;
        let event: ServerEvent = serde_json::from_str(json).expect("legacy event must parse");
        match event {
            ServerEvent::Progress {
                node_id,
                event,
                artifact_reused_reason,
                ..
            } => {
                assert_eq!(node_id, None);
                assert_eq!(event, None);
                assert_eq!(artifact_reused_reason, None);
            }
            _ => panic!("expected Progress event"),
        }
    }

    #[test]
    fn progress_event_with_node_fields_parses_them() {
        let json = r#"{"type":"progress","pct":52,"msg":"Extracting reference pitch...",
            "node_id":"pitch.extract","event":"node_started"}"#;
        let event: ServerEvent = serde_json::from_str(json).expect("structured event must parse");
        match event {
            ServerEvent::Progress { node_id, event, .. } => {
                assert_eq!(node_id.as_deref(), Some("pitch.extract"));
                assert_eq!(event.as_deref(), Some("node_started"));
            }
            _ => panic!("expected Progress event"),
        }
    }

    #[test]
    fn artifact_reused_event_carries_its_reason() {
        let json = r#"{"type":"progress","pct":50,"msg":"Stems already cached",
            "node_id":"stems.separate","event":"artifact_reused","artifact_reused_reason":"cache_hit"}"#;
        let event: ServerEvent = serde_json::from_str(json).expect("event must parse");
        match event {
            ServerEvent::Progress {
                artifact_reused_reason,
                ..
            } => {
                assert_eq!(artifact_reused_reason.as_deref(), Some("cache_hit"));
            }
            _ => panic!("expected Progress event"),
        }
    }

    #[test]
    fn committed_output_event_is_captured_before_canonical_overwrite() {
        let root = std::env::temp_dir().join(format!(
            "uta-studio-boundary-capture-{}-{}",
            std::process::id(),
            unix_time_ms()
        ));
        std::fs::create_dir_all(&root).unwrap();
        let _guard = crate::library_db::reconnect_for_test(&root.join("db"));
        let canonical = root.join("song_timed_transcript.json");
        std::fs::write(&canonical, br#"{"segments":[]}"#).unwrap();
        let mut route: AnalysisStageRoute = serde_json::from_value(serde_json::json!({
            "stage": "finalizing",
            "node_id": "lyrics.align",
            "node_event": "node_completed",
            "committed_outputs": [{
                "slot": "output:0",
                "artifact_kind": "TimedTranscript",
                "path": canonical,
                "binding_kind": "produced",
                "algorithm_version": "1"
            }],
            "operation": "Alignment",
            "implementation": "test",
            "model": "test",
            "stage_progress": 100,
            "requested_device": "cpu",
            "actual_device": "cpu",
            "fallback_from": null,
            "fallback_reason": null,
            "backend_fallback_from": null,
            "backend_fallback_reason": null
        }))
        .unwrap();
        capture_committed_outputs_in(
            &CacheDir { path: root.clone() },
            "song-boundary",
            std::slice::from_mut(&mut route),
        );
        let output = &route.committed_outputs[0];
        let immutable = output.immutable_path.as_ref().unwrap();
        assert_eq!(std::fs::read(immutable).unwrap(), br#"{"segments":[]}"#);
        std::fs::write(&canonical, br#"{"segments":[{"text":"later"}]}"#).unwrap();
        assert_eq!(std::fs::read(immutable).unwrap(), br#"{"segments":[]}"#);
        assert!(output.capture_error.is_none());
        drop(_guard);
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn old_history_snapshot_json_without_node_fields_still_deserializes() {
        // Simulates a snapshot_json blob written by a pre-Phase-3 build and
        // stored in analysis_history.snapshot_json. load_analysis_history
        // silently drops any row that fails to deserialize (`.ok()?`), so
        // this must keep working or old runs vanish from history.
        let old_snapshot_json = r#"{
            "stage": "pitch",
            "stage_progress": 40,
            "operation": "Reference pitch extraction",
            "detail": "Extracting reference pitch...",
            "implementation": "RMVPE",
            "model": "RMVPE singing pitch model",
            "device": "cuda",
            "requested_device": "cuda",
            "fallback_from": null,
            "fallback_reason": null,
            "backend_fallback_from": null,
            "backend_fallback_reason": null,
            "stage_routes": []
        }"#;
        let snapshot: AnalysisProgressSnapshot =
            serde_json::from_str(old_snapshot_json).expect("old snapshot json must still parse");
        assert_eq!(snapshot.node_id, None);
        assert_eq!(snapshot.node_event, None);
        assert_eq!(snapshot.artifact_reused_reason, None);
        assert_eq!(snapshot.stage, "pitch");
    }
}

#[cfg(test)]
pub(crate) fn send_and_monitor(
    server: &mut ServerProcess,
    json_cmd: &str,
    progress_hash: Option<&str>,
) -> Result<SongResult, UtaStudioError> {
    server.writer.write_all(json_cmd.as_bytes())?;
    server.writer.write_all(b"\n")?;
    server.writer.flush()?;

    let mut line_buf = String::new();
    loop {
        line_buf.clear();
        let bytes = server.reader.read_line(&mut line_buf)?;

        if bytes == 0 {
            return Err("Server closed connection unexpectedly".into());
        }

        let line = line_buf.trim();
        if line.is_empty() {
            continue;
        }

        let event: ServerEvent = serde_json::from_str(line).map_err(|error| {
            UtaStudioError::Other(format!(
                "native analyzer polluted stdout with a non-protocol line: {error}"
            ))
        })?;

        match event {
            ServerEvent::Progress {
                pct,
                msg,
                stage,
                stage_progress,
                operation,
                implementation,
                model,
                device,
                requested_device,
                fallback_from,
                fallback_reason,
                backend_fallback_from,
                backend_fallback_reason,
                mut stage_routes,
                node_id,
                event,
                artifact_reused_reason,
            } => {
                if let Some(hash) = progress_hash {
                    let analysis_log_path = LIVE_ANALYSIS
                        .lock()
                        .unwrap()
                        .get(hash)
                        .and_then(|snapshot| snapshot.analysis_log_path.clone());
                    capture_committed_outputs(hash, &mut stage_routes);
                    append_analysis_artifacts(analysis_log_path.as_deref(), &stage_routes);
                    update_live_analysis(
                        hash,
                        AnalysisProgressSnapshot {
                            stage,
                            overall_progress: (pct as usize).clamp(0, 100),
                            stage_progress,
                            operation,
                            detail: msg,
                            implementation,
                            model,
                            device,
                            requested_device,
                            fallback_from,
                            fallback_reason,
                            backend_fallback_from,
                            backend_fallback_reason,
                            stage_routes,
                            node_id,
                            node_event: event,
                            artifact_reused_reason,
                            analysis_log_path,
                            engine: None,
                        },
                    );
                    update_queue_status(hash, QueuedStatus::Analyzing(pct as usize));
                }
            }
            ServerEvent::Done => return Ok(SongResult::Done),
            ServerEvent::Error { kind, msg } => {
                let kind_s = kind.as_deref().unwrap_or("generic");
                if kind_s == "oom" {
                    return Ok(SongResult::Oom);
                }
                let msg = if msg.is_empty() {
                    "Unknown error".to_string()
                } else {
                    msg
                };
                return Ok(SongResult::Error(msg));
            }
            ServerEvent::Unknown => {
                warn!("[analyzer] Ignoring unknown event: {line}");
            }
        }
    }
}
