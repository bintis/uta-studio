//! Editor audition: chart loading and native audio transport.

use std::{
    sync::{Arc, Mutex, mpsc},
    time::Instant,
};

use bevy::prelude::{Res, ResMut, Time, default};

use crate::studio::{
    session::{NativeAudio, NativePitchAudition, StudioRoute},
    state::{AsyncJobs, EditorUiState, ShellState},
    ui_invalidation::{UiDirtyRegion, UiInvalidated},
};

use super::state::{
    ArtifactAuditionSelection, ArtifactAuditionSlot, EditorAudioSyncTimer, NativeEditor,
    NativeEditorLoadJob, WaveformSource,
};

pub(crate) fn start_editor_load_job(
    file_hash: &str,
    audio: Arc<uta_studio_audio::EditorAudioPlayer>,
    job: &mut NativeEditorLoadJob,
) -> String {
    if job.receiver.is_some() {
        return "The chart editor is already loading.".to_string();
    }
    let file_hash = file_hash.to_string();
    let (sender, receiver) = mpsc::channel();
    std::thread::spawn(move || {
        let result = load_native_editor(&file_hash, audio.as_ref());
        let _ = sender.send(result);
    });
    job.receiver = Some(Mutex::new(receiver));
    "Loading chart, audio, and waveform…".to_string()
}

pub(crate) fn start_editor_merge_load_job(
    candidate: app_core::ArtifactRef,
    authored: app_core::ArtifactRef,
    mode: app_core::ChartRevisionMergeMode,
    audio: Arc<uta_studio_audio::EditorAudioPlayer>,
    job: &mut NativeEditorLoadJob,
) -> String {
    if job.receiver.is_some() {
        return "The chart editor is already loading.".to_string();
    }
    let (sender, receiver) = mpsc::channel();
    std::thread::spawn(move || {
        let result = (|| {
            let mut chart =
                app_core::load_chart(&candidate.file_hash).map_err(|error| error.to_string())?;
            chart.vocal_chart = app_core::merge_chart_revisions(&candidate, &authored, mode)?;
            finish_native_editor_load(chart, Some(candidate), audio.as_ref())
        })();
        let _ = sender.send(result);
    });
    job.receiver = Some(Mutex::new(receiver));
    "Building a validated chart merge working copy…".to_string()
}

pub(crate) fn poll_editor_load_job(
    mut shell: ResMut<ShellState>,
    mut editor_state: ResMut<EditorUiState>,
    mut jobs: ResMut<AsyncJobs>,
    mut invalidated: ResMut<UiInvalidated>,
) {
    let result = jobs.editor_load_job.receiver.as_ref().and_then(|receiver| {
        receiver
            .lock()
            .ok()
            .and_then(|receiver| match receiver.try_recv() {
                Ok(result) => Some(result),
                Err(mpsc::TryRecvError::Empty) => None,
                Err(mpsc::TryRecvError::Disconnected) => {
                    Some(Err("Chart editor loader exited unexpectedly.".to_string()))
                }
            })
    });
    let Some(result) = result else {
        return;
    };
    jobs.editor_load_job.receiver = None;
    match result {
        Ok(editor) => {
            bevy::log::info!("Switching the native UI to the loaded chart editor");
            let audio_notice = editor.audio_status.error.as_ref().map(|error| {
                format!("Chart editing is available, but native audio is unavailable: {error}")
            });
            editor_state.editor = Some(editor);
            shell.route = StudioRoute::Editor;
            shell.notice = audio_notice;
        }
        Err(error) => shell.notice = Some(error),
    }
    invalidated.invalidate(UiDirtyRegion::Editor);
}

pub(crate) fn load_native_editor(
    file_hash: &str,
    audio: &uta_studio_audio::EditorAudioPlayer,
) -> Result<NativeEditor, String> {
    bevy::log::info!("Loading chart for the native editor");
    let chart = app_core::load_chart(file_hash).map_err(|error| error.to_string())?;
    finish_native_editor_load(chart, None, audio)
}

fn project_singing_analysis_for_editor(
    bytes: &[u8],
    source: app_core::ArtifactRef,
) -> Result<app_core::SingingEvidenceBundle, String> {
    app_core::singing_analysis_evidence_bundle(bytes, source)
}

fn decode_initial_editor_waveform(
    stop_result: Result<uta_studio_audio::EditorAudioStatus, String>,
    path: &std::path::Path,
) -> (app_core::ChartWaveform, bool, Option<String>) {
    match stop_result {
        Ok(status) if !status.playing => (
            app_core::decode_chart_waveform(path).unwrap_or_default(),
            false,
            None,
        ),
        Ok(_) => (
            app_core::ChartWaveform::default(),
            true,
            Some(
                "Could not confirm playback was stopped before reading the initial waveform: stop reported that playback was still running"
                    .to_string(),
            ),
        ),
        Err(error) => (
            app_core::ChartWaveform::default(),
            true,
            Some(format!(
                "Could not confirm playback was stopped before reading the initial waveform: {error}"
            )),
        ),
    }
}

fn finish_native_editor_load(
    chart: app_core::ChartDocument,
    source: Option<app_core::ArtifactRef>,
    audio: &uta_studio_audio::EditorAudioPlayer,
) -> Result<NativeEditor, String> {
    bevy::log::info!("Decoding the bounded editor waveform while playback is stopped");
    // The overview waveform is a lyric/pitch alignment aid, so it defaults to
    // the voice, not whatever instrumental happens to be auditioned — but
    // it's independent of `audio_source` and can be repointed at any stem
    // with a right-click on the waveform itself.
    let waveform_source = if chart.audio.vocals.is_some() {
        WaveformSource::Vocals
    } else {
        WaveformSource::Instrumental
    };
    let waveform_path = waveform_source_path(&chart.audio, waveform_source);
    let (waveform, waveform_fallback_pending, waveform_warning) = decode_initial_editor_waveform(
        confirm_editor_audio_stopped(audio),
        std::path::Path::new(waveform_path),
    );
    bevy::log::info!("Preparing native editor audio");
    // Authoring does not depend on playback initialization. Keep the native
    // audio error on the editor status so transport can explain the problem,
    // while still allowing the chart and decoded waveform to be edited.
    let mut status =
        editor_audio_status(audio.load_path(std::path::Path::new(&chart.audio.instrumental)));
    if let Some(warning) = waveform_warning {
        status.error = Some(match status.error.take() {
            Some(error) => format!("{warning}; {error}"),
            None => warning,
        });
    }
    bevy::log::info!("Native editor is ready");
    let mut editor = NativeEditor::new(chart, status, waveform, waveform_source, "instrumental");
    editor.artifact_audition.waveform_fallback_pending = waveform_fallback_pending;
    editor.artifact_source = source.clone();
    let revisions = app_core::load_analysis_artifacts(&editor.chart.file_hash);
    if let Some(evidence_revision) = revisions
        .iter()
        .filter(|revision| revision.kind == app_core::ArtifactKind::EvidenceBundle)
        .max_by_key(|revision| revision.created_at_ms)
        && let Ok(bytes) = std::fs::read(&evidence_revision.path)
        && let Ok(bundle) = project_singing_analysis_for_editor(
            &bytes,
            app_core::ArtifactRef {
                file_hash: evidence_revision.file_hash.clone(),
                kind: evidence_revision.kind,
                revision_id: evidence_revision.id.clone(),
            },
        )
    {
        editor.evidence = bundle;
        editor.review_index = (!editor.evidence.review_regions.is_empty()).then_some(0);
    }
    if let Some(technique_revision) = revisions
        .iter()
        .filter(|revision| revision.kind == app_core::ArtifactKind::TechniqueEvidence)
        .max_by_key(|revision| revision.created_at_ms)
        && let Ok(bytes) = std::fs::read(&technique_revision.path)
    {
        let source = app_core::ArtifactRef {
            file_hash: technique_revision.file_hash.clone(),
            kind: technique_revision.kind,
            revision_id: technique_revision.id.clone(),
        };
        if let Ok(track) = app_core::technique_evidence_track(&bytes, source) {
            editor
                .evidence
                .tracks
                .retain(|existing| existing.kind != app_core::EvidenceKind::StarsTechnique);
            editor.evidence.tracks.push(track);
            editor
                .visible_evidence
                .insert(app_core::EvidenceKind::StarsTechnique);
        }
    }
    editor.suggestions = editor
        .document
        .derive_evidence_suggestions(&editor.evidence);
    let opened_chart = source.or_else(|| {
        revisions
            .iter()
            .find(|revision| {
                revision.active
                    && matches!(
                        revision.kind,
                        app_core::ArtifactKind::AuthoredChart
                            | app_core::ArtifactKind::CandidateChart
                    )
            })
            .map(|revision| app_core::ArtifactRef {
                file_hash: revision.file_hash.clone(),
                kind: revision.kind,
                revision_id: revision.id.clone(),
            })
    });
    let workflow_revision = app_core::load_song_workflow(&editor.chart.file_hash)
        .ok()
        .map(|workflow| workflow.definition.revision.to_string());
    let audio_artifacts = editor_audio_artifacts(&revisions);
    let evidence_bundle = revisions
        .iter()
        .find(|revision| revision.active && revision.kind == app_core::ArtifactKind::EvidenceBundle)
        .map(|revision| app_core::ArtifactRef {
            file_hash: revision.file_hash.clone(),
            kind: revision.kind,
            revision_id: revision.id.clone(),
        });
    let newer_candidate = revisions
        .iter()
        .filter(|revision| {
            revision.active && revision.kind == app_core::ArtifactKind::CandidateChart
        })
        .max_by_key(|revision| revision.created_at_ms)
        .map(|revision| app_core::ArtifactRef {
            file_hash: revision.file_hash.clone(),
            kind: revision.kind,
            revision_id: revision.id.clone(),
        });
    editor.source_context = Some(app_core::EditorSourceContext {
        opened_chart,
        workflow_revision,
        run_id: None,
        evidence_bundle,
        audio_artifacts,
        newer_candidate,
    });
    Ok(editor)
}

fn is_selectable_editor_audio_revision(revision: &app_core::ArtifactRevision) -> bool {
    !revision.invalidated
        && matches!(
            revision.kind,
            app_core::ArtifactKind::AudioStem
                | app_core::ArtifactKind::VocalStem
                | app_core::ArtifactKind::InstrumentalStem
                | app_core::ArtifactKind::AnalysisVocalStem
        )
}

fn editor_audio_artifacts(
    revisions: &[app_core::ArtifactRevision],
) -> Vec<app_core::EditorAudioArtifact> {
    revisions
        .iter()
        .filter(|revision| {
            is_selectable_editor_audio_revision(revision)
                && app_core::validate_artifact_revision_file(revision).is_ok()
        })
        .map(|revision| app_core::EditorAudioArtifact {
            revision: app_core::ArtifactRef {
                file_hash: revision.file_hash.clone(),
                kind: revision.kind,
                revision_id: revision.id.clone(),
            },
            role: match revision.kind {
                app_core::ArtifactKind::InstrumentalStem => app_core::AudioRole::Instrumental,
                app_core::ArtifactKind::AnalysisVocalStem => app_core::AudioRole::LeadVocal,
                _ => app_core::AudioRole::Vocal,
            },
            label: if revision.active {
                format!("{:?} · {}", revision.kind, revision.producer_node)
            } else {
                format!(
                    "{:?} · {} · Revision {}",
                    revision.kind, revision.producer_node, revision.id
                )
            },
            producer: app_core::WorkflowNodeId::new(revision.producer_node.as_str()),
            model_id: None,
        })
        .collect()
}

fn confirm_editor_audio_stopped(
    audio: &uta_studio_audio::EditorAudioPlayer,
) -> Result<uta_studio_audio::EditorAudioStatus, String> {
    match audio.pause() {
        Ok(status) if !status.playing => Ok(status),
        pause_result => {
            let pause_error = pause_result
                .err()
                .unwrap_or_else(|| "pause reported that playback was still running".to_string());
            match audio.stop() {
                Ok(status) if !status.playing => Ok(status),
                Ok(_) => Err(format!(
                    "{pause_error}; stop reported that playback was still running"
                )),
                Err(error) => Err(format!("{pause_error}; stop failed: {error}")),
            }
        }
    }
}

fn reconcile_editor_artifact_audition(
    audio: &uta_studio_audio::EditorAudioPlayer,
    editor: &mut NativeEditor,
) -> Result<Option<String>, String> {
    let revisions = app_core::load_analysis_artifacts(&editor.chart.file_hash);
    let audio_artifacts = editor_audio_artifacts(&revisions);
    let authorized = audio_artifacts
        .iter()
        .map(|artifact| artifact.revision.clone())
        .collect::<Vec<_>>();
    if let Some(context) = editor.source_context.as_mut() {
        context.audio_artifacts = audio_artifacts;
    }
    let direct_source_invalidated =
        editor
            .audio_source
            .strip_prefix("artifact:")
            .is_some_and(|revision_id| {
                !authorized
                    .iter()
                    .any(|artifact| artifact.revision_id == revision_id)
            });
    let result = editor.artifact_audition.reconcile(&authorized);
    if result.waveform_cleared {
        editor.waveform = app_core::ChartWaveform::default();
    }
    let mut waveform_notice = None;
    if editor.artifact_audition.waveform_fallback_pending {
        refresh_editor_audio_status(editor, audio.status())?;
        if !editor.audio_status.playing {
            let path = waveform_source_path(&editor.chart.audio, editor.waveform_source);
            match app_core::decode_chart_waveform(std::path::Path::new(path)) {
                Ok(waveform) => {
                    editor.waveform = waveform;
                    editor.artifact_audition.waveform_fallback_pending = false;
                }
                Err(error) => {
                    waveform_notice = Some(format!("Could not decode that waveform: {error}"));
                }
            }
        }
    }
    if !result.active_invalidated && !direct_source_invalidated {
        return Ok(waveform_notice);
    }
    let position = editor.visible_position;
    let stopped_status = match confirm_editor_audio_stopped(audio) {
        Ok(status) => status,
        Err(error) => {
            editor.audio_status = match audio.status() {
                Ok(status) => status,
                Err(status_error) => uta_studio_audio::EditorAudioStatus {
                    playing: true,
                    error: Some(status_error),
                    ..editor.audio_status.clone()
                },
            };
            editor.visible_position = editor.audio_status.position_secs;
            editor.last_audio_sync = Instant::now();
            return Err(format!(
                "Active artifact revision is no longer available, but playback could not be stopped: {error}"
            ));
        }
    };
    let next_artifact = editor.artifact_audition.active_artifact().cloned();
    let (mut source, mut status) = if let Some(artifact) = next_artifact
        && let Ok(revision) = audio_artifact_revision(editor, &artifact)
    {
        (
            format!("artifact:{}", artifact.revision_id),
            audio.load_path(&revision.path),
        )
    } else {
        (
            "instrumental".to_string(),
            audio.load(&editor.chart.file_hash, "instrumental"),
        )
    };
    if source.starts_with("artifact:") && status.is_err() {
        editor.artifact_audition.active = None;
        source = "instrumental".to_string();
        status = audio.load(&editor.chart.file_hash, "instrumental");
    }
    let status = match status {
        Ok(status) => {
            let seek_position = position.min(status.duration_secs);
            Ok(audio.seek(seek_position).unwrap_or(status))
        }
        Err(error) => Err(error),
    };
    match status {
        Ok(status) => {
            editor.audio_source = source;
            editor.audio_status = status;
            editor.visible_position = position.min(editor.audio_status.duration_secs);
            editor.last_audio_sync = Instant::now();
            Ok(Some(
                "Active artifact revision is no longer available; playback stopped".to_string(),
            ))
        }
        Err(error) => {
            editor.audio_source.clear();
            editor.audio_status = stopped_status;
            editor.visible_position = position.min(editor.audio_status.duration_secs);
            editor.last_audio_sync = Instant::now();
            Ok(Some(format!(
                "Active artifact revision is no longer available; playback stopped, but a safe fallback could not be loaded: {error}"
            )))
        }
    }
}

fn audio_artifact_revision(
    editor: &NativeEditor,
    artifact: &app_core::ArtifactRef,
) -> Result<app_core::ArtifactRevision, String> {
    let authorized = artifact.file_hash == editor.chart.file_hash
        && editor.source_context.as_ref().is_some_and(|context| {
            context
                .audio_artifacts
                .iter()
                .any(|candidate| candidate.revision == *artifact)
        });
    if !authorized {
        return Err("That audio artifact is not bound to this editor session.".to_string());
    }
    app_core::load_analysis_artifacts(&editor.chart.file_hash)
        .into_iter()
        .find(|revision| {
            !revision.invalidated
                && revision.file_hash == artifact.file_hash
                && revision.kind == artifact.kind
                && revision.id == artifact.revision_id
        })
        .ok_or_else(|| "That artifact revision is no longer available.".to_string())
        .and_then(|revision| {
            app_core::validate_artifact_revision_file(&revision)
                .map_err(|_| "That artifact revision is no longer available.".to_string())?;
            Ok(revision)
        })
}

pub(crate) fn select_editor_artifact_audition(
    audio: &uta_studio_audio::EditorAudioPlayer,
    editor: &mut NativeEditor,
    slot: ArtifactAuditionSlot,
    artifact: app_core::ArtifactRef,
) -> Result<(), String> {
    let _ = reconcile_editor_artifact_audition(audio, editor)?;
    audio_artifact_revision(editor, &artifact)?;
    let should_activate =
        editor.artifact_audition.active.is_none() || editor.artifact_audition.active == Some(slot);
    if should_activate {
        select_editor_audio_source(audio, editor, &format!("artifact:{}", artifact.revision_id))?;
        editor.artifact_audition.bind(slot, artifact);
        editor.artifact_audition.activate(slot)?;
    } else {
        editor.artifact_audition.bind(slot, artifact);
    }
    Ok(())
}

pub(crate) fn activate_editor_artifact_audition(
    audio: &uta_studio_audio::EditorAudioPlayer,
    editor: &mut NativeEditor,
    slot: ArtifactAuditionSlot,
) -> Result<(), String> {
    let _ = reconcile_editor_artifact_audition(audio, editor)?;
    let artifact = editor
        .artifact_audition
        .bound(slot)
        .cloned()
        .ok_or_else(|| "That artifact audition slot is not bound yet".to_string())?;
    select_editor_audio_source(audio, editor, &format!("artifact:{}", artifact.revision_id))?;
    editor.artifact_audition.activate(slot)?;
    Ok(())
}

fn refresh_editor_audio_status(
    editor: &mut NativeEditor,
    status: Result<uta_studio_audio::EditorAudioStatus, String>,
) -> Result<(), String> {
    match status {
        Ok(status) => {
            editor.audio_status = status;
            editor.visible_position = editor.audio_status.position_secs;
            editor.last_audio_sync = Instant::now();
            Ok(())
        }
        Err(error) => {
            editor.audio_status.playing = true;
            editor.audio_status.error = Some(error.clone());
            editor.last_audio_sync = Instant::now();
            Err(format!("Could not confirm playback was stopped: {error}"))
        }
    }
}

pub(crate) fn confirm_waveform_status(
    editor: &mut NativeEditor,
    status: Result<uta_studio_audio::EditorAudioStatus, String>,
    playing_error: &str,
) -> Result<(), String> {
    refresh_editor_audio_status(editor, status)?;
    if editor.audio_status.playing {
        Err(playing_error.to_string())
    } else {
        Ok(())
    }
}

fn confirm_waveform_read_stopped(
    audio: &uta_studio_audio::EditorAudioPlayer,
    editor: &mut NativeEditor,
    playing_error: &str,
) -> Result<(), String> {
    if editor.audio_status.playing {
        return Err(playing_error.to_string());
    }
    confirm_waveform_status(editor, audio.status(), playing_error)
}

pub(crate) fn set_editor_artifact_waveform(
    audio: &uta_studio_audio::EditorAudioPlayer,
    editor: &mut NativeEditor,
    artifact: app_core::ArtifactRef,
) -> Result<(), String> {
    confirm_waveform_read_stopped(
        audio,
        editor,
        "Stop playback before reading an artifact waveform",
    )?;
    let _ = reconcile_editor_artifact_audition(audio, editor)?;
    let revision = audio_artifact_revision(editor, &artifact)?;
    editor.waveform = app_core::decode_chart_waveform(&revision.path).map_err(|error| {
        format!("Could not decode that artifact waveform without changing playback: {error}")
    })?;
    editor.artifact_audition.waveform = Some(artifact);
    editor.artifact_audition.waveform_fallback_pending = false;
    Ok(())
}

/// Resolves which file a waveform source refers to, falling back to the
/// instrumental stem when the chart has no separate vocal track.
pub(crate) fn waveform_source_path(audio: &app_core::ChartAudio, source: WaveformSource) -> &str {
    match source {
        WaveformSource::Vocals => audio
            .vocals
            .as_deref()
            .unwrap_or(audio.instrumental.as_str()),
        WaveformSource::Original => audio.original.as_str(),
        WaveformSource::Instrumental => audio.instrumental.as_str(),
    }
}

/// Repoints the overview waveform at a different stem, independent of
/// whatever plays back through `audio_source`.
pub(crate) fn set_editor_waveform_source(
    audio: &uta_studio_audio::EditorAudioPlayer,
    editor: &mut NativeEditor,
    source: WaveformSource,
) -> Result<(), String> {
    confirm_waveform_read_stopped(audio, editor, "Stop playback before reading a waveform")?;
    let _ = reconcile_editor_artifact_audition(audio, editor)?;
    let path = waveform_source_path(&editor.chart.audio, source);
    let waveform = app_core::decode_chart_waveform(std::path::Path::new(path))
        .map_err(|error| format!("Could not decode that waveform: {error}"))?;
    editor.waveform = waveform;
    editor.waveform_source = source;
    editor.artifact_audition.waveform = None;
    editor.artifact_audition.waveform_fallback_pending = false;
    Ok(())
}

pub(crate) fn editor_audio_status(
    result: Result<uta_studio_audio::EditorAudioStatus, String>,
) -> uta_studio_audio::EditorAudioStatus {
    result.unwrap_or_else(|error| uta_studio_audio::EditorAudioStatus {
        error: Some(error),
        ..default()
    })
}

pub(crate) fn toggle_editor_playback(
    audio: &uta_studio_audio::EditorAudioPlayer,
    editor: Option<&mut NativeEditor>,
) -> Result<(), String> {
    let editor = editor.ok_or_else(|| "No chart is open".to_string())?;
    let status = if editor.audio_status.playing {
        audio.pause()?
    } else {
        audio.play()?
    };
    editor.visible_position = status.position_secs;
    editor.audio_status = status;
    editor.last_audio_sync = Instant::now();
    Ok(())
}

pub(crate) fn select_editor_audio_source(
    audio: &uta_studio_audio::EditorAudioPlayer,
    editor: &mut NativeEditor,
    source: &str,
) -> Result<(), String> {
    let was_playing = editor.audio_status.playing;
    let position = editor.visible_position;
    let mut status = if let Some(revision_id) = source.strip_prefix("artifact:") {
        let artifact = editor
            .source_context
            .as_ref()
            .and_then(|context| {
                context
                    .audio_artifacts
                    .iter()
                    .find(|artifact| artifact.revision.revision_id == revision_id)
            })
            .map(|artifact| artifact.revision.clone())
            .ok_or_else(|| {
                "That audio artifact is not bound to this editor session.".to_string()
            })?;
        let revision = audio_artifact_revision(editor, &artifact)?;
        audio.load_path(&revision.path)?
    } else {
        if !matches!(source, "vocals" | "instrumental" | "original") {
            return Err("That audition source is not supported.".to_string());
        }
        if source == "vocals" && editor.chart.audio.vocals.is_none() {
            return Err("This chart has no separate vocal source.".to_string());
        }
        audio.load(&editor.chart.file_hash, source)?
    };
    status = audio.seek(position.min(status.duration_secs))?;
    // The overview waveform is independent of playback source — it's set
    // separately with a right-click on the waveform (`set_editor_waveform_source`).
    if was_playing {
        status = audio.play()?;
    }
    editor.audio_source = source.to_string();
    editor.audio_status = status;
    editor.visible_position = position.min(editor.audio_status.duration_secs);
    editor.last_audio_sync = Instant::now();
    Ok(())
}

fn artifact_reconciliation_needed(
    selection: &ArtifactAuditionSelection,
    audio_source: &str,
) -> bool {
    selection.a.is_some()
        || selection.b.is_some()
        || selection.waveform.is_some()
        || selection.waveform_fallback_pending
        || audio_source.starts_with("artifact:")
}

pub(crate) fn sync_editor_audio(
    time: Res<Time>,
    mut timer: ResMut<EditorAudioSyncTimer>,
    audio: Res<NativeAudio>,
    tones: Res<NativePitchAudition>,
    mut shell: ResMut<ShellState>,
    mut editor_state: ResMut<EditorUiState>,
    mut invalidated: ResMut<UiInvalidated>,
) {
    if shell.route != StudioRoute::Editor {
        return;
    }
    let mut status_error = None;
    let mut audition_finished = false;
    {
        let Some(editor) = editor_state.editor.as_mut() else {
            return;
        };
        if timer.0.tick(time.delta()).just_finished() {
            match audio.0.status() {
                Ok(status) => {
                    if let Some(error) = status.error.clone() {
                        status_error = Some(format!("Editor audio stopped: {error}"));
                    }
                    editor.visible_position = status.position_secs;
                    editor.audio_status = status;
                    editor.last_audio_sync = Instant::now();
                }
                Err(error) => {
                    editor.audio_status.playing = true;
                    editor.audio_status.error = Some(error.clone());
                    editor.last_audio_sync = Instant::now();
                    status_error = Some(error);
                }
            }
            if artifact_reconciliation_needed(&editor.artifact_audition, &editor.audio_source) {
                match reconcile_editor_artifact_audition(audio.0.as_ref(), editor) {
                    Ok(Some(notice)) | Err(notice) => status_error = Some(notice),
                    Ok(None) => {}
                }
            }
        } else if editor.audio_status.playing {
            editor.visible_position = (editor.audio_status.position_secs
                + editor.last_audio_sync.elapsed().as_secs_f64())
            .min(
                editor
                    .audio_status
                    .duration_secs
                    .max(editor.audio_status.position_secs),
            );
        }

        // A ranged audition ends where the user asked it to, not at the end
        // of the song.
        if let Some(until) = editor.audition_until
            && editor.visible_position >= until
        {
            editor.audition_until = None;
            audition_finished = true;
        }

        if editor.audio_status.playing
            && Instant::now() >= editor.manual_scroll_until
            && editor.visible_position >= editor.viewport_start + editor.viewport_duration * 0.82
        {
            editor.viewport_start =
                (editor.visible_position - editor.viewport_duration * 0.28).max(0.0);
            invalidated.invalidate(UiDirtyRegion::Editor);
        }
    }
    if audition_finished {
        tones.0.stop();
        if let Ok(status) = audio.0.pause()
            && let Some(editor) = editor_state.editor.as_mut()
        {
            editor.visible_position = status.position_secs;
            editor.audio_status = status;
            editor.last_audio_sync = Instant::now();
        }
        // `PlayNoteVocal` may have temporarily switched playback to the
        // vocal stem; put the user's chosen source back now that it's done.
        let restore = editor_state
            .editor
            .as_ref()
            .and_then(|editor| editor.audition_restore_source.clone());
        if let Some(source) = restore
            && let Some(editor) = editor_state.editor.as_ref()
        {
            let file_hash = editor.chart.file_hash.clone();
            if let Ok(status) = audio.0.load(&file_hash, &source)
                && let Some(editor) = editor_state.editor.as_mut()
            {
                editor.audio_source = source;
                editor.audio_status = status;
                editor.last_audio_sync = Instant::now();
                editor.audition_restore_source = None;
            }
        }
        invalidated.invalidate(UiDirtyRegion::Editor);
    }
    if status_error.is_some() {
        shell.notice = status_error;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn audio_revision(id: &str, active: bool, invalidated: bool) -> app_core::ArtifactRevision {
        app_core::ArtifactRevision {
            id: id.to_string(),
            file_hash: "song".to_string(),
            kind: app_core::ArtifactKind::AudioStem,
            path: std::path::PathBuf::from(format!("{id}.flac")),
            content_hash: format!("hash-{id}"),
            producer_node: app_core::AnalysisNodeId::new("separate"),
            input_revisions: Vec::new(),
            config_hash: "config".to_string(),
            algorithm_version: "v1".to_string(),
            created_at_ms: 1,
            byte_size: 1,
            active,
            legacy: false,
            invalidated,
        }
    }

    #[test]
    fn artifact_a_b_includes_historical_but_not_invalidated_audio_revisions() {
        assert!(is_selectable_editor_audio_revision(&audio_revision(
            "active", true, false
        )));
        assert!(is_selectable_editor_audio_revision(&audio_revision(
            "historical",
            false,
            false
        )));
        assert!(!is_selectable_editor_audio_revision(&audio_revision(
            "invalidated",
            false,
            true
        )));
    }

    #[test]
    fn direct_artifact_playback_and_pending_waveform_request_reconciliation() {
        let mut selection = ArtifactAuditionSelection::default();
        assert!(artifact_reconciliation_needed(
            &selection,
            "artifact:direct-revision"
        ));
        assert!(!artifact_reconciliation_needed(&selection, "instrumental"));
        selection.waveform_fallback_pending = true;
        assert!(artifact_reconciliation_needed(&selection, "instrumental"));
    }

    #[test]
    fn initial_waveform_decode_is_deferred_when_stopped_state_is_unconfirmed() {
        let missing = std::path::Path::new("this-path-must-not-be-read.wav");
        let (waveform, pending, warning) = decode_initial_editor_waveform(
            Err("transport status unavailable".to_string()),
            missing,
        );
        assert!(waveform.peaks.is_empty());
        assert!(pending);
        assert_eq!(
            warning.as_deref(),
            Some(
                "Could not confirm playback was stopped before reading the initial waveform: transport status unavailable"
            )
        );
    }

    #[test]
    fn current_singing_analysis_loads_nonempty_editor_evidence() {
        let bytes = serde_json::to_vec(&serde_json::json!({
            "contract":"uta.analysis-engine.singing-analysis",
            "version":1,
            "format_version":"0.3.0",
            "timebase":1000000,
            "chart_references":{},
            "candidate_evidence":[{
                "id":"selected",
                "range":{"start":1000000,"end":2000000},
                "target_midi":69,
                "boundary_source":"rmvpe",
                "boundary_kind":"f0_derived",
                "boundary_role":"primary",
                "boundary_hard":false,
                "target_pitch_source":"rmvpe",
                "center_pitch_hz":440.0,
                "boundary_alternatives":[],
                "boundary_constraints":[],
                "technique_evidence":[],
                "techniques":{},
                "alternatives":[]
            }],
            "candidate_hard_boundaries":{},
            "review_regions":[{
                "id":"review-1",
                "range":{"start":1000000,"end":2000000},
                "confidence":0.8,
                "reasons":["pitch_disagreement"],
                "evidence_experts":["rmvpe","fcpe"],
                "reviewed":false
            }],
            "provenance":{
                "execution_fingerprint":"ffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffff",
                "fusion_algorithm":"fusion-v16",
                "fusion_decision":{
                    "decision_mode":"algorithm",
                    "selector":"hsmm_viterbi",
                    "selector_version":"hsmm-v15",
                    "candidate_set_digest":"ff56f669f0e2df22931f619b2d607b080479dfd21bf88198e67da0d4e8101847",
                    "selected_candidate_ids":["selected"],
                    "reuse_policy":"deterministic"
                }
            }
        }))
        .unwrap();
        let bundle = project_singing_analysis_for_editor(
            &bytes,
            app_core::ArtifactRef {
                file_hash: "song".to_string(),
                kind: app_core::ArtifactKind::EvidenceBundle,
                revision_id: "singing-analysis".to_string(),
            },
        )
        .unwrap();
        assert!(bundle.tracks.iter().any(|track| !track.points.is_empty()));
        assert_eq!(bundle.review_regions.len(), 1);
    }
}
