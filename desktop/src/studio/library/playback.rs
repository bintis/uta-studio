use super::*;
use crate::studio::*;

pub(crate) fn spawn_song_header(
    parent: &mut ChildSpawnerCommands,
    font: Handle<Font>,
    theme: &StudioTheme,
) {
    parent
        .spawn((
            Node {
                width: percent(100),
                height: px(34),
                align_items: AlignItems::Center,
                padding: UiRect::horizontal(px(22)),
                ..default()
            },
            BackgroundColor(theme.muted.with_alpha(0.42)),
        ))
        .with_children(|row| {
            row.spawn(Node {
                width: px(56),
                flex_shrink: 0.0,
                ..default()
            });
            spawn_text(row, font.clone(), "TRACK", 9.0, theme.muted_foreground);
            row.spawn(Node {
                flex_grow: 1.0,
                ..default()
            });
            row.spawn(Node {
                width: px(150),
                ..default()
            })
            .with_children(|artist| {
                spawn_text(artist, font.clone(), "ARTIST", 9.0, theme.muted_foreground);
            });
            row.spawn(Node {
                width: px(180),
                ..default()
            })
            .with_children(|album| {
                spawn_text(album, font.clone(), "ALBUM", 9.0, theme.muted_foreground);
            });
            row.spawn(Node {
                width: px(64),
                ..default()
            })
            .with_children(|duration| {
                spawn_text(duration, font.clone(), "TIME", 9.0, theme.muted_foreground);
            });
            row.spawn(Node {
                width: px(150),
                justify_content: JustifyContent::FlexEnd,
                ..default()
            })
            .with_children(|status| {
                spawn_text(status, font, "STATUS", 9.0, theme.muted_foreground);
            });
        });
}

pub(crate) fn handle_library_search_keyboard(
    keys: Res<ButtonInput<KeyCode>>,
    focus: Res<InputFocus>,
    inputs: Query<&EditableText, With<LibrarySearchInput>>,
    mut shell: ResMut<ShellState>,
    mut library: ResMut<LibraryState>,
    mut dialogs: ResMut<DialogState>,
    mut invalidated: ResMut<UiInvalidated>,
) {
    let command = keys.pressed(KeyCode::ControlLeft)
        || keys.pressed(KeyCode::ControlRight)
        || keys.pressed(KeyCode::SuperLeft)
        || keys.pressed(KeyCode::SuperRight);
    if command && keys.just_pressed(KeyCode::KeyK) && shell.route != StudioRoute::Editor {
        dialogs.search_open = true;
        dialogs.activity_open = false;
        dialogs.about_open = false;
        invalidated.invalidate(UiDirtyRegion::Library);
        return;
    }
    if keys.just_pressed(KeyCode::Escape) && dialogs.search_open {
        dialogs.search_open = false;
        invalidated.invalidate(UiDirtyRegion::Library);
        return;
    }
    if !dialogs.search_open || !keys.just_pressed(KeyCode::Enter) {
        return;
    }
    let Some(entity) = focus.get() else {
        return;
    };
    let Ok(input) = inputs.get(entity) else {
        return;
    };
    let value = input.value().to_string();
    let value = value.trim();
    library.library_search = (!value.is_empty()).then(|| value.to_string());
    shell.route = StudioRoute::Library;
    library.library_view = LibraryView::All;
    library.library_facet = None;
    dialogs.search_open = false;
    library.refresh();
    invalidated.invalidate(UiDirtyRegion::Library);
}

pub(crate) fn start_export_job(
    file_hash: &str,
    extension: &'static str,
    export_directory: Option<PathBuf>,
    job: &mut NativeExportJob,
) -> String {
    if job.receiver.is_some() {
        return "An export is already in progress.".to_string();
    }
    let file_hash = file_hash.to_string();
    let (sender, receiver) = mpsc::channel();
    std::thread::spawn(move || {
        let result = export_song(&file_hash, extension, export_directory.as_deref());
        let _ = sender.send(result);
    });
    job.receiver = Some(Mutex::new(receiver));
    format!(
        "Choose where to save the {} export…",
        if extension == "utz" {
            "UTZ"
        } else {
            "UltraStar"
        }
    )
}

pub(crate) fn start_export_all_job(
    extension: &'static str,
    export_directory: PathBuf,
    job: &mut NativeExportJob,
) -> String {
    if job.receiver.is_some() {
        return "An export is already in progress.".to_string();
    }
    if !export_directory.is_dir() {
        return format!(
            "The export folder is unavailable: {}. Choose it again in Settings > Storage.",
            export_directory.display()
        );
    }

    let songs = SongsStore::load_all()
        .processed
        .into_iter()
        .filter(|song| song.authoring_ready)
        .collect::<Vec<_>>();
    if songs.is_empty() {
        return "No chart is ready to export. Analyze or import a chart first.".to_string();
    }
    let total = songs.len();
    let (sender, receiver) = mpsc::channel();
    std::thread::spawn(move || {
        let result = export_all_songs(&songs, extension, &export_directory);
        let _ = sender.send(result);
    });
    job.receiver = Some(Mutex::new(receiver));
    format!(
        "Exporting {total} ready chart{} as {}…",
        if total == 1 { "" } else { "s" },
        if extension == "utz" {
            "UTZ"
        } else {
            "UltraStar"
        }
    )
}

pub(crate) fn poll_export_job(
    mut shell: ResMut<ShellState>,
    mut jobs: ResMut<AsyncJobs>,
    mut invalidated: ResMut<UiInvalidated>,
) {
    let result = jobs.export_job.receiver.as_ref().and_then(|receiver| {
        receiver
            .lock()
            .ok()
            .and_then(|receiver| match receiver.try_recv() {
                Ok(result) => Some(result),
                Err(mpsc::TryRecvError::Empty) => None,
                Err(mpsc::TryRecvError::Disconnected) => {
                    Some("Export worker exited unexpectedly.".to_string())
                }
            })
    });
    let Some(result) = result else {
        return;
    };
    jobs.export_job.receiver = None;
    shell.notice = Some(result);
    invalidated.invalidate(UiDirtyRegion::Library);
}

pub(crate) fn library_visible_position(playback: &LibraryPlayback) -> f64 {
    if playback.status.playing {
        (playback.status.position_secs + playback.last_audio_sync.elapsed().as_secs_f64()).min(
            playback
                .status
                .duration_secs
                .max(playback.status.position_secs),
        )
    } else {
        playback.visible_position
    }
}

pub(crate) fn play_library_song(
    audio: &uta_studio_audio::EditorAudioPlayer,
    file_hash: &str,
    playback: &mut LibraryPlayback,
) -> Result<(), String> {
    let song = app_core::load_song_by_hash(file_hash)
        .map_err(|error| error.to_string())?
        .ok_or_else(|| format!("Song not found: {file_hash}"))?;
    if !song.path.is_file() {
        return Err(format!(
            "Source audio is unavailable: {}",
            song.path.display()
        ));
    }
    audio.load_path(&song.path)?;
    audio.set_volume(playback.volume)?;
    let status = audio.play()?;
    if let Some(error) = status.error.as_ref() {
        return Err(format!("Could not play the original source: {error}"));
    }
    playback.file_hash = Some(file_hash.to_string());
    playback.queue_index = playback.queue.iter().position(|hash| hash == file_hash);
    playback.visible_position = status.position_secs;
    playback.status = status;
    playback.last_audio_sync = Instant::now();
    Ok(())
}

/// §7.6 "Play audio artifact": plays one artifact revision's file (a
/// vocal/instrumental stem at whichever revision the user picked) through
/// the same player `play_library_song` uses, but as a one-off preview
/// outside the library queue -- `playback.file_hash`/`queue`/`queue_index`
/// are cleared rather than repurposed, since this isn't "now playing this
/// song," it's "now previewing this artifact revision."
pub(crate) fn play_artifact_revision(
    audio: &uta_studio_audio::EditorAudioPlayer,
    path: &std::path::Path,
    playback: &mut LibraryPlayback,
) -> Result<(), String> {
    if !path.is_file() {
        return Err(format!("Artifact file is unavailable: {}", path.display()));
    }
    audio.load_path(path)?;
    audio.set_volume(playback.volume)?;
    let status = audio.play()?;
    if let Some(error) = status.error.as_ref() {
        return Err(format!("Could not play this artifact: {error}"));
    }
    playback.file_hash = None;
    playback.queue.clear();
    playback.queue_index = None;
    playback.visible_position = status.position_secs;
    playback.status = status;
    playback.last_audio_sync = Instant::now();
    Ok(())
}

pub(crate) fn prepare_library_queue(
    songs: &[Song],
    file_hash: &str,
    playback: &mut LibraryPlayback,
) {
    playback.queue = songs
        .iter()
        .filter(|song| song.path.is_file())
        .map(|song| song.file_hash.clone())
        .collect();
    if !playback.queue.iter().any(|hash| hash == file_hash) {
        playback.queue.push(file_hash.to_string());
    }
    playback.queue_index = playback.queue.iter().position(|hash| hash == file_hash);
}

pub(crate) fn advance_library_queue(
    audio: &uta_studio_audio::EditorAudioPlayer,
    playback: &mut LibraryPlayback,
    direction: i8,
    wrap: bool,
) -> Result<(), String> {
    if playback.queue.is_empty() {
        return Err("The playback queue is empty.".to_string());
    }
    let current = playback
        .queue_index
        .or_else(|| {
            playback
                .file_hash
                .as_ref()
                .and_then(|hash| playback.queue.iter().position(|item| item == hash))
        })
        .unwrap_or(0);
    let len = playback.queue.len();
    let next = if playback.shuffle && len > 1 && direction > 0 {
        playback.shuffle_seed = playback
            .shuffle_seed
            .wrapping_mul(6_364_136_223_846_793_005)
            .wrapping_add(1_442_695_040_888_963_407);
        let candidate = (playback.shuffle_seed as usize) % len;
        if candidate == current {
            (candidate + 1) % len
        } else {
            candidate
        }
    } else if direction < 0 {
        if current > 0 {
            current - 1
        } else if wrap {
            len - 1
        } else {
            return Err("This is the start of the queue.".to_string());
        }
    } else if current + 1 < len {
        current + 1
    } else if wrap {
        0
    } else {
        return Err("This is the end of the queue.".to_string());
    };
    let file_hash = playback.queue[next].clone();
    playback.queue_index = Some(next);
    play_library_song(audio, &file_hash, playback)
}

pub(crate) fn restart_library_song(
    audio: &uta_studio_audio::EditorAudioPlayer,
    playback: &mut LibraryPlayback,
) -> Result<(), String> {
    audio.seek(0.0)?;
    let status = audio.play()?;
    playback.visible_position = status.position_secs;
    playback.status = status;
    playback.last_audio_sync = Instant::now();
    Ok(())
}

pub(crate) fn set_library_volume(
    audio: &uta_studio_audio::EditorAudioPlayer,
    playback: &mut LibraryPlayback,
    volume: f64,
) -> Result<(), String> {
    playback.volume = volume.clamp(0.0, 1.0);
    if playback.volume > 0.0 {
        playback.volume_before_mute = playback.volume;
    }
    if playback.status.loaded {
        let status = audio.set_volume(playback.volume)?;
        playback.visible_position = status.position_secs;
        playback.status = status;
        playback.last_audio_sync = Instant::now();
    }
    Ok(())
}

pub(crate) fn toggle_library_playback(
    audio: &uta_studio_audio::EditorAudioPlayer,
    playback: &mut LibraryPlayback,
) -> Result<(), String> {
    if !playback.status.loaded {
        return Err("Choose a song before starting playback.".to_string());
    }
    let status = if playback.status.playing {
        audio.pause()?
    } else {
        if playback.status.ended {
            audio.seek(0.0)?;
        }
        audio.play()?
    };
    playback.visible_position = status.position_secs;
    playback.status = status;
    playback.last_audio_sync = Instant::now();
    Ok(())
}

pub(crate) fn seek_library_relative(
    audio: &uta_studio_audio::EditorAudioPlayer,
    playback: &mut LibraryPlayback,
    delta_secs: f64,
) -> Result<(), String> {
    if !playback.status.loaded {
        return Err("Choose a song before seeking.".to_string());
    }
    let was_playing = playback.status.playing;
    let target = (library_visible_position(playback) + delta_secs)
        .clamp(0.0, playback.status.duration_secs.max(0.0));
    let mut status = audio.seek(target)?;
    if was_playing {
        status = audio.play()?;
    }
    playback.visible_position = status.position_secs;
    playback.status = status;
    playback.last_audio_sync = Instant::now();
    Ok(())
}

pub(crate) fn handle_library_scroll(
    mut wheel: MessageReader<bevy::input::mouse::MouseWheel>,
    keys: Res<ButtonInput<KeyCode>>,
    windows: Query<&Window, With<PrimaryWindow>>,
    shell: Res<ShellState>,
    mut library: ResMut<LibraryState>,
    mut lists: Query<(&ComputedNode, &mut ScrollPosition), With<LibrarySongList>>,
    graphs: Query<(&ComputedNode, &UiGlobalTransform), With<AnalysisGraphViewport>>,
) {
    if shell.route != StudioRoute::Library || library.library_view == LibraryView::Queue {
        return;
    }
    let shift = keys.any_pressed([KeyCode::ShiftLeft, KeyCode::ShiftRight]);
    if shift
        && let Ok(window) = windows.single()
        && let Some(pointer) = window.cursor_position()
        && graphs
            .iter()
            .any(|(computed, transform)| ui_node_contains_pointer(computed, transform, pointer))
    {
        wheel.clear();
        return;
    }
    let Ok((computed, mut position)) = lists.single_mut() else {
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
    library.library_scroll_offset = position.y;
}

pub(crate) fn sync_library_audio(
    time: Res<Time>,
    mut timer: ResMut<LibraryAudioSyncTimer>,
    audio: Res<NativeLibraryAudio>,
    mut shell: ResMut<ShellState>,
    mut playback: ResMut<PlaybackState>,
    mut invalidated: ResMut<UiInvalidated>,
) {
    if playback.library_playback.file_hash.is_none() {
        return;
    }
    if timer.0.tick(time.delta()).just_finished() {
        let was_playing = playback.library_playback.status.playing;
        let had_ended = playback.library_playback.status.ended;
        match audio.0.status() {
            Ok(status) => {
                if let Some(error) = status.error.clone() {
                    shell.notice = Some(format!("Library playback stopped: {error}"));
                    invalidated.invalidate(UiDirtyRegion::Library);
                }
                playback.library_playback.visible_position = status.position_secs;
                playback.library_playback.status = status;
                playback.library_playback.last_audio_sync = Instant::now();
                if playback.library_playback.status.ended && !had_ended {
                    let repeat = playback.library_playback.repeat;
                    let result = if repeat == LibraryRepeatMode::One {
                        restart_library_song(&audio.0, &mut playback.library_playback)
                    } else {
                        advance_library_queue(
                            &audio.0,
                            &mut playback.library_playback,
                            1,
                            repeat == LibraryRepeatMode::All,
                        )
                    };
                    if let Err(error) = result
                        && error != "This is the end of the queue."
                    {
                        shell.notice = Some(error);
                    }
                    invalidated.invalidate(UiDirtyRegion::Library);
                }
                if was_playing != playback.library_playback.status.playing
                    || had_ended != playback.library_playback.status.ended
                {
                    invalidated.invalidate(UiDirtyRegion::Library);
                }
            }
            Err(error) => {
                shell.notice = Some(error);
                invalidated.invalidate(UiDirtyRegion::Library);
            }
        }
    } else if playback.library_playback.status.playing {
        playback.library_playback.visible_position =
            library_visible_position(&playback.library_playback);
    }
}

pub(crate) fn update_library_player_ui(
    playback: Res<PlaybackState>,
    mut progress: Query<&mut Node, With<LibraryPlayerProgress>>,
    mut clocks: Query<&mut Text, (With<LibraryPlayerClockText>, Without<LibraryPlayerProgress>)>,
) {
    let playback = &playback.library_playback;
    if !playback.status.loaded {
        return;
    }
    let position = library_visible_position(playback);
    let duration = playback.status.duration_secs.max(0.001);
    let width = ((position / duration) * 100.0).clamp(0.0, 100.0) as f32;
    for mut node in &mut progress {
        node.width = percent(width);
    }
    let label = format_editor_clock(position, playback.status.duration_secs);
    for mut text in &mut clocks {
        **text = label.clone();
    }
}

pub(crate) fn validate_source_path(
    path: &std::path::Path,
    config: &AppConfig,
) -> Result<PathBuf, String> {
    let requested = std::fs::canonicalize(path).map_err(|error| error.to_string())?;
    let mut allowed_roots = config.library_paths();
    if let Some(export_path) = config.export_path.as_ref() {
        allowed_roots.push(export_path.clone());
    }
    let allowed = allowed_roots.iter().any(|root| {
        std::fs::canonicalize(root)
            .map(|root| requested.starts_with(root))
            .unwrap_or(false)
    });
    allowed
        .then_some(requested)
        .ok_or_else(|| "Path is outside configured library and output locations".to_string())
}
