//! Editor audition: chart loading and native audio transport.

use crate::studio::*;

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
    mut session: ResMut<StudioSession>,
    mut invalidated: ResMut<UiInvalidated>,
) {
    let result = session
        .editor_load_job
        .receiver
        .as_ref()
        .and_then(|receiver| {
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
    session.editor_load_job.receiver = None;
    match result {
        Ok(editor) => {
            bevy::log::info!("Switching the native UI to the loaded chart editor");
            let audio_notice = editor.audio_status.error.as_ref().map(|error| {
                format!("Chart editing is available, but native audio is unavailable: {error}")
            });
            session.editor = Some(editor);
            session.route = StudioRoute::Editor;
            session.notice = audio_notice;
        }
        Err(error) => session.notice = Some(error),
    }
    invalidated.0 = true;
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
    editor.artifact_source = source;
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
    if !matches!(source, "vocals" | "instrumental" | "original") {
        return Err("That audition source is not supported.".to_string());
    }
    if source == "vocals" && editor.chart.audio.vocals.is_none() {
        return Err("This chart has no separate vocal source.".to_string());
    }
    let was_playing = editor.audio_status.playing;
    let mut status = audio.load(&editor.chart.file_hash, source)?;
    // The overview waveform is independent of playback source — it's set
    // separately with a right-click on the waveform (`set_editor_waveform_source`).
    if was_playing {
        status = audio.play()?;
    }
    editor.audio_source = source.to_string();
    editor.audio_status = status;
    editor.visible_position = 0.0;
    editor.last_audio_sync = Instant::now();
    Ok(())
}

pub(crate) fn sync_editor_audio(
    time: Res<Time>,
    mut timer: ResMut<EditorAudioSyncTimer>,
    audio: Res<NativeAudio>,
    tones: Res<NativePitchAudition>,
    mut session: ResMut<StudioSession>,
    mut invalidated: ResMut<UiInvalidated>,
) {
    if session.route != StudioRoute::Editor {
        return;
    }
    let mut status_error = None;
    let mut audition_finished = false;
    {
        let Some(editor) = session.editor.as_mut() else {
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
            invalidated.0 = true;
        }
    }
    if audition_finished {
        tones.0.stop();
        if let Ok(status) = audio.0.pause()
            && let Some(editor) = session.editor.as_mut()
        {
            editor.visible_position = status.position_secs;
            editor.audio_status = status;
            editor.last_audio_sync = Instant::now();
        }
        // `PlayNoteVocal` may have temporarily switched playback to the
        // vocal stem; put the user's chosen source back now that it's done.
        let restore = session
            .editor
            .as_ref()
            .and_then(|editor| editor.audition_restore_source.clone());
        if let Some(source) = restore
            && let Some(editor) = session.editor.as_ref()
        {
            let file_hash = editor.chart.file_hash.clone();
            if let Ok(status) = audio.0.load(&file_hash, &source)
                && let Some(editor) = session.editor.as_mut()
            {
                editor.audio_source = source;
                editor.audio_status = status;
                editor.last_audio_sync = Instant::now();
                editor.audition_restore_source = None;
            }
        }
        invalidated.0 = true;
    }
    if status_error.is_some() {
        session.notice = status_error;
    }
}
