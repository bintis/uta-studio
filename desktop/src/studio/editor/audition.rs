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

use super::state::{EditorAudioSyncTimer, NativeEditor, NativeEditorLoadJob, WaveformSource};

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

pub(crate) fn start_editor_revision_load_job(
    reference: app_core::ArtifactRef,
    audio: Arc<uta_studio_audio::EditorAudioPlayer>,
    job: &mut NativeEditorLoadJob,
) -> String {
    if job.receiver.is_some() {
        return "The chart editor is already loading.".to_string();
    }
    let (sender, receiver) = mpsc::channel();
    std::thread::spawn(move || {
        let result = load_native_editor_revision(&reference, audio.as_ref());
        let _ = sender.send(result);
    });
    job.receiver = Some(Mutex::new(receiver));
    "Loading the selected immutable revision, audio, and editor evidence…".to_string()
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

fn load_native_editor_revision(
    reference: &app_core::ArtifactRef,
    audio: &uta_studio_audio::EditorAudioPlayer,
) -> Result<NativeEditor, String> {
    let mut chart =
        app_core::load_chart(&reference.file_hash).map_err(|error| error.to_string())?;
    app_core::apply_artifact_revision_to_chart(&mut chart, reference)?;
    finish_native_editor_load(chart, Some(reference.clone()), audio)
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
    let waveform =
        app_core::decode_chart_waveform(std::path::Path::new(waveform_path)).unwrap_or_default();
    bevy::log::info!("Preparing native editor audio");
    // Authoring does not depend on playback initialization. Keep the native
    // audio error on the editor status so transport can explain the problem,
    // while still allowing the chart and decoded waveform to be edited.
    let status =
        editor_audio_status(audio.load_path(std::path::Path::new(&chart.audio.instrumental)));
    bevy::log::info!("Native editor is ready");
    let mut editor = NativeEditor::new(chart, status, waveform, waveform_source, "instrumental");
    editor.artifact_source = source.clone();
    let revisions = app_core::load_analysis_artifacts(&editor.chart.file_hash);
    if let Some(evidence_revision) = revisions
        .iter()
        .filter(|revision| revision.kind == app_core::ArtifactKind::EvidenceBundle)
        .max_by_key(|revision| revision.created_at_ms)
        && let Ok(bytes) = std::fs::read(&evidence_revision.path)
        && let Ok(bundle) = serde_json::from_slice::<app_core::SingingEvidenceBundle>(&bytes)
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
    if let Some(opened_chart) = opened_chart {
        let workflow_revision = app_core::load_song_workflow(&editor.chart.file_hash)
            .ok()
            .map(|workflow| workflow.definition.revision.to_string());
        let audio_artifacts = revisions
            .iter()
            .filter(|revision| {
                revision.active
                    && matches!(
                        revision.kind,
                        app_core::ArtifactKind::AudioStem
                            | app_core::ArtifactKind::VocalStem
                            | app_core::ArtifactKind::InstrumentalStem
                            | app_core::ArtifactKind::AnalysisVocalStem
                    )
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
                label: format!("{:?} · {}", revision.kind, revision.producer_node),
                producer: app_core::WorkflowNodeId::new(revision.producer_node.as_str()),
                model_id: None,
            })
            .collect();
        let evidence_bundle = revisions
            .iter()
            .find(|revision| {
                revision.active && revision.kind == app_core::ArtifactKind::EvidenceBundle
            })
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
    }
    Ok(editor)
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
pub(crate) fn set_editor_waveform_source(editor: &mut NativeEditor, source: WaveformSource) {
    let path = waveform_source_path(&editor.chart.audio, source);
    editor.waveform =
        app_core::decode_chart_waveform(std::path::Path::new(path)).unwrap_or_default();
    editor.waveform_source = source;
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
        let revision = app_core::load_analysis_artifacts(&editor.chart.file_hash)
            .into_iter()
            .find(|revision| revision.id == revision_id)
            .ok_or_else(|| "That artifact revision is no longer available.".to_string())?;
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
                Err(error) => status_error = Some(error),
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
