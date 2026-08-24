use super::*;
use crate::studio::*;

pub(crate) fn start_key_shift(
    file_hash: &str,
    delta: i8,
    job: &mut NativeAuthoringJob,
    busy: &mut bool,
) -> String {
    if *busy || job.receiver.is_some() {
        return "A key or tempo render is already running.".to_string();
    }
    let Some(song) = app_core::load_song_by_hash(file_hash).ok().flatten() else {
        return format!("Song not found: {file_hash}");
    };
    let Some(original_key) = song.key.as_deref() else {
        return "Analyze the song again to detect its original key.".to_string();
    };
    let offset = (song.key_offset + i32::from(delta)).clamp(-5, 5);
    if offset == song.key_offset {
        return "Key shift is limited to five semitones in either direction.".to_string();
    }
    let (key, pitch_ratio) = calculate_key_shift(original_key, offset);
    let notice_key = key.clone();
    let file_hash = file_hash.to_string();
    let (sender, receiver) = mpsc::channel();
    std::thread::spawn(move || {
        let result = app_core::shift_key(&file_hash, &key, pitch_ratio, offset)
            .map_err(|error| error.to_string());
        let _ = sender.send(AuthoringEvent {
            result,
            kind: "key",
        });
    });
    job.receiver = Some(Mutex::new(receiver));
    *busy = true;
    format!("Rendering key variant {notice_key}…")
}

pub(crate) fn start_tempo_shift(
    file_hash: &str,
    delta: i8,
    job: &mut NativeAuthoringJob,
    busy: &mut bool,
) -> String {
    if *busy || job.receiver.is_some() {
        return "A key or tempo render is already running.".to_string();
    }
    let Some(song) = app_core::load_song_by_hash(file_hash).ok().flatten() else {
        return format!("Song not found: {file_hash}");
    };
    let tempo = ((song.tempo + f64::from(delta) * 0.1) * 10.0).round() / 10.0;
    let tempo = tempo.clamp(0.5, 2.0);
    if (tempo - song.tempo).abs() < f64::EPSILON {
        return "Tempo is limited to 0.5×–2.0×.".to_string();
    }
    let file_hash = file_hash.to_string();
    let (sender, receiver) = mpsc::channel();
    std::thread::spawn(move || {
        let result = app_core::shift_tempo(&file_hash, tempo).map_err(|error| error.to_string());
        let _ = sender.send(AuthoringEvent {
            result,
            kind: "tempo",
        });
    });
    job.receiver = Some(Mutex::new(receiver));
    *busy = true;
    format!("Rendering {tempo:.1}× tempo variant…")
}

pub(crate) fn poll_authoring_job(
    mut job: ResMut<NativeAuthoringJob>,
    mut shell: ResMut<ShellState>,
    mut library: ResMut<LibraryState>,
    mut jobs: ResMut<AsyncJobs>,
    mut invalidated: ResMut<UiInvalidated>,
) {
    let result = job.receiver.as_ref().and_then(|receiver| {
        receiver
            .lock()
            .ok()
            .and_then(|receiver| match receiver.try_recv() {
                Ok(event) => Some(Ok(event)),
                Err(mpsc::TryRecvError::Empty) => None,
                Err(mpsc::TryRecvError::Disconnected) => Some(Err(
                    "Key/tempo render worker exited unexpectedly.".to_string(),
                )),
            })
    });
    let Some(result) = result else {
        return;
    };
    job.receiver = None;
    jobs.authoring_busy = false;
    match result {
        Ok(event) => match event.result {
            Ok(rendered) => {
                shell.notice = Some(format!(
                    "Song {} shifted successfully · key {} · {:.1}× tempo.",
                    event.kind, rendered.key, rendered.tempo
                ));
                library.refresh();
            }
            Err(error) => {
                shell.notice = Some(format!("Could not render {} variant: {error}", event.kind))
            }
        },
        Err(error) => shell.notice = Some(error),
    }
    invalidated.invalidate(UiDirtyRegion::Library);
}

pub(crate) fn poll_lyrics_search_job(
    mut shell: ResMut<ShellState>,
    mut dialogs: ResMut<DialogState>,
    mut jobs: ResMut<AsyncJobs>,
    mut invalidated: ResMut<UiInvalidated>,
) {
    let result = jobs
        .lyrics_search_job
        .receiver
        .as_ref()
        .and_then(|receiver| {
            receiver
                .lock()
                .ok()
                .and_then(|receiver| match receiver.try_recv() {
                    Ok(candidates) => Some(Ok(candidates)),
                    Err(mpsc::TryRecvError::Empty) => None,
                    Err(mpsc::TryRecvError::Disconnected) => {
                        Some(Err("LRCLIB search worker exited unexpectedly.".to_string()))
                    }
                })
        });
    let Some(result) = result else {
        return;
    };
    jobs.lyrics_search_job.receiver = None;
    match result {
        Ok(candidates) => {
            let count = candidates.len();
            if let Some(editor) = dialogs.lyrics_editor.as_mut() {
                editor.searching = false;
                editor.candidates = candidates;
                editor.candidate_index = 0;
                shell.notice = Some(if count == 0 {
                    "LRCLIB did not return a matching lyric.".to_string()
                } else {
                    format!("Found {count} LRCLIB lyric candidate(s). Review before applying.")
                });
            }
        }
        Err(error) => {
            if let Some(editor) = dialogs.lyrics_editor.as_mut() {
                editor.searching = false;
            }
            shell.notice = Some(error);
        }
    }
    invalidated.invalidate(UiDirtyRegion::Library);
}

pub(crate) fn poll_lyrics_waveform_job(
    mut shell: ResMut<ShellState>,
    mut dialogs: ResMut<DialogState>,
    mut jobs: ResMut<AsyncJobs>,
    mut invalidated: ResMut<UiInvalidated>,
) {
    let result = jobs
        .lyrics_waveform_job
        .receiver
        .as_ref()
        .and_then(|receiver| {
            receiver
                .lock()
                .ok()
                .and_then(|receiver| match receiver.try_recv() {
                    Ok(result) => Some(result),
                    Err(mpsc::TryRecvError::Empty) => None,
                    Err(mpsc::TryRecvError::Disconnected) => Some((
                        String::new(),
                        Err("TimedTranscript waveform worker exited unexpectedly.".to_string()),
                    )),
                })
        });
    let Some((file_hash, result)) = result else {
        return;
    };
    jobs.lyrics_waveform_job.receiver = None;
    if let Some(editor) = dialogs
        .lyrics_editor
        .as_mut()
        .filter(|editor| editor.file_hash == file_hash)
    {
        match result {
            Ok(waveform) => editor.waveform = waveform,
            Err(error) => {
                shell.notice = Some(localized_message(
                    &shell.config,
                    UiMessage::TranscriptWaveformFailed,
                    &[("{error}", &error)],
                ))
            }
        }
        invalidated.invalidate(UiDirtyRegion::Library);
    }
}

pub(crate) fn calculate_key_shift(original_key: &str, offset: i32) -> (String, f64) {
    const NOTES: [&str; 12] = [
        "C", "C#", "D", "D#", "E", "F", "F#", "G", "G#", "A", "A#", "B",
    ];
    let (note, quality) = original_key
        .strip_suffix('m')
        .map(|note| (note, "m"))
        .unwrap_or((original_key, ""));
    let key = NOTES
        .iter()
        .position(|candidate| *candidate == note)
        .map(|index| {
            let shifted = (index as i32 + offset).rem_euclid(NOTES.len() as i32) as usize;
            format!("{}{quality}", NOTES[shifted])
        })
        .unwrap_or_else(|| original_key.to_string());
    (key, 2f64.powf(f64::from(offset) / 12.0))
}

pub(crate) fn run_analysis_action(file_hash: &str, action: impl FnOnce()) -> String {
    let Some(song) = app_core::load_song_by_hash(file_hash).ok().flatten() else {
        return format!("Song not found: {file_hash}");
    };
    if matches!(
        song.transcript_source,
        Some(app_core::TranscriptSource::Usdx)
    ) {
        return "This action is unavailable for imported USDX charts.".to_string();
    }
    action();
    format!("Queued analysis for “{}”.", song.title)
}

/// Like `run_analysis_action`, for the Phase 4 executor's `Result`-returning
/// entry points (`run_analysis_node`/`disable_analysis_node_for_run`), which
/// can genuinely refuse a request (e.g. disabling an `AlwaysRequired` node)
/// instead of always succeeding the way every legacy special-case function
/// above does.
pub(crate) fn run_analysis_action_checked(
    file_hash: &str,
    action: impl FnOnce() -> Result<(), String>,
) -> String {
    let Some(song) = app_core::load_song_by_hash(file_hash).ok().flatten() else {
        return format!("Song not found: {file_hash}");
    };
    if matches!(
        song.transcript_source,
        Some(app_core::TranscriptSource::Usdx)
    ) {
        return "This action is unavailable for imported USDX charts.".to_string();
    }
    match action() {
        Ok(()) => format!("Queued analysis for “{}”.", song.title),
        Err(error) => format!("Could not queue analysis: {error}"),
    }
}

pub(crate) fn handle_song_detail_scroll(
    mut wheel: MessageReader<bevy::input::mouse::MouseWheel>,
    shell: Res<ShellState>,
    dialogs: Res<DialogState>,
    mut contents: Query<(&ComputedNode, &mut ScrollPosition), With<SongDetailContent>>,
) {
    if shell.route != StudioRoute::SongDetail || dialogs.lyrics_editor.is_some() {
        return;
    }
    let Ok((computed, mut position)) = contents.single_mut() else {
        wheel.clear();
        return;
    };
    let delta = wheel
        .read()
        .map(|event| {
            let scale = match event.unit {
                bevy::input::mouse::MouseScrollUnit::Line => 22.0,
                bevy::input::mouse::MouseScrollUnit::Pixel => 1.0,
            };
            -event.y * scale
        })
        .sum::<f32>();
    let size = computed.size() * computed.inverse_scale_factor();
    let content = computed.content_size() * computed.inverse_scale_factor();
    position.y = (position.y + delta).clamp(0.0, (content.y - size.y).max(0.0));
}
