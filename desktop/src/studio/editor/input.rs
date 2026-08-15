//! Editor input: keyboard, wheel, and pointer capture translation.

use crate::studio::*;

pub(crate) fn sync_editor_word_input(
    inputs: Query<(&EditableText, &EditorWordInput), Changed<EditableText>>,
    mut session: ResMut<StudioSession>,
) {
    let Some(editor) = session.editor.as_mut() else {
        return;
    };
    for (input, marker) in &inputs {
        let text = input.value().to_string();
        let current = selected_editor_word(&editor.document, marker.0)
            .map(|(text, _, _)| text)
            .unwrap_or_default();
        if text == current {
            continue;
        }
        editor.checkpoint("Edit lyric text");
        if update_editor_word_text(&mut editor.document, marker.0, &text) {
            editor.dirty = true;
        } else {
            editor.undo.pop();
        }
    }
}

pub(crate) fn finish_inline_lyric_edit(
    keys: Res<ButtonInput<KeyCode>>,
    mut focus: ResMut<InputFocus>,
    inline_inputs: Query<(), With<InlineEditorWordInput>>,
    mut session: ResMut<StudioSession>,
    mut invalidated: ResMut<UiInvalidated>,
) {
    if !keys.just_pressed(KeyCode::Enter) && !keys.just_pressed(KeyCode::Escape) {
        return;
    }
    let Some(entity) = focus.get() else {
        return;
    };
    if !inline_inputs.contains(entity) {
        return;
    }
    focus.clear();
    if let Some(editor) = session.editor.as_mut() {
        editor.word_edit_focus = None;
    }
    invalidated.0 = true;
}

pub(crate) fn handle_editor_keyboard(
    keys: Res<ButtonInput<KeyCode>>,
    focus: Res<InputFocus>,
    editable: Query<(), With<EditableText>>,
    actions: Query<(), With<UiAction>>,
    audio: Res<NativeAudio>,
    mut session: ResMut<StudioSession>,
    mut invalidated: ResMut<UiInvalidated>,
) {
    if session.route != StudioRoute::Editor {
        return;
    }
    if focus.get().is_some_and(|entity| editable.contains(entity)) {
        return;
    }
    if focus.get().is_some_and(|entity| actions.contains(entity))
        && (keys.just_pressed(KeyCode::Tab)
            || keys.just_pressed(KeyCode::Enter)
            || keys.just_pressed(KeyCode::ArrowLeft)
            || keys.just_pressed(KeyCode::ArrowRight)
            || keys.just_pressed(KeyCode::ArrowUp)
            || keys.just_pressed(KeyCode::ArrowDown))
    {
        return;
    }
    let control = keys.any_pressed([KeyCode::ControlLeft, KeyCode::ControlRight]);
    let shift = keys.any_pressed([KeyCode::ShiftLeft, KeyCode::ShiftRight]);
    if control && keys.just_pressed(KeyCode::KeyZ) && !shift {
        if let Some(label) = session.editor.as_mut().and_then(NativeEditor::undo) {
            session.notice = Some(format!("Undid: {label}."));
            invalidated.0 = true;
        }
        return;
    }
    if control && (keys.just_pressed(KeyCode::KeyY) || (shift && keys.just_pressed(KeyCode::KeyZ)))
    {
        if let Some(label) = session.editor.as_mut().and_then(NativeEditor::redo) {
            session.notice = Some(format!("Redid: {label}."));
            invalidated.0 = true;
        }
        return;
    }
    if control && keys.just_pressed(KeyCode::KeyA) {
        if let Some(editor) = session.editor.as_mut() {
            if editor.selected_word.is_some() {
                let words = all_editor_word_selections(&editor.document);
                editor.selected_word = words.iter().next().copied();
                editor.selected_words = words;
                editor.selected_note = None;
                editor.selected_notes.clear();
                editor.word_edit_focus = None;
                session.notice = Some(format!(
                    "Selected {} lyric word(s).",
                    editor.selected_words.len()
                ));
                invalidated.0 = true;
                return;
            }
            let count = chart_notes(&editor.document).len();
            editor.selected_notes = (0..count).collect();
            editor.selected_note = (count > 0).then_some(0);
            editor.selected_word = None;
            editor.selected_words.clear();
            editor.word_edit_focus = None;
            session.notice = Some(format!("Selected {count} note(s)."));
            invalidated.0 = true;
        }
        return;
    }
    if control && keys.just_pressed(KeyCode::KeyS) {
        if let Some(editor) = session.editor.as_mut() {
            session.notice = Some(save_editor_chart(editor));
            invalidated.0 = true;
        }
        return;
    }
    if control && keys.just_pressed(KeyCode::KeyC) {
        if let Some(editor) = session.editor.as_mut() {
            let selected = editor.selected_note_indices();
            editor.clipboard_notes = copy_chart_notes(&editor.document, &selected);
            session.notice = Some(format!("Copied {} note(s).", editor.clipboard_notes.len()));
            invalidated.0 = true;
        }
        return;
    }
    if control && keys.just_pressed(KeyCode::KeyX) {
        if let Some(editor) = session.editor.as_mut() {
            let selected = editor.selected_note_indices();
            if selected.is_empty() {
                return;
            }
            editor.clipboard_notes = copy_chart_notes(&editor.document, &selected);
            editor.checkpoint("Cut notes");
            let removed = remove_chart_notes(&mut editor.document, &selected);
            editor.selected_note = None;
            editor.selected_notes.clear();
            editor.dirty |= removed > 0;
            session.notice = Some(format!("Cut {removed} note(s)."));
            invalidated.0 = true;
        }
        return;
    }
    if control && keys.just_pressed(KeyCode::KeyV) {
        if let Some(editor) = session.editor.as_mut()
            && !editor.clipboard_notes.is_empty()
        {
            editor.checkpoint("Cut notes");
            let inserted = paste_chart_notes(
                &mut editor.document,
                &editor.clipboard_notes,
                editor.visible_position,
            );
            editor.selected_note = inserted.iter().next().copied();
            editor.selected_notes = inserted;
            editor.dirty = true;
            session.notice = Some("Pasted note(s) at the playhead.".to_string());
            invalidated.0 = true;
        }
        return;
    }
    if control && keys.just_pressed(KeyCode::KeyD) {
        if let Some(editor) = session.editor.as_mut() {
            let selected = editor.selected_note_indices();
            let clipboard = copy_chart_notes(&editor.document, &selected);
            if clipboard.is_empty() {
                return;
            }
            let selected_end = selected
                .iter()
                .filter_map(|index| {
                    chart_notes(&editor.document)
                        .get(*index)
                        .map(|note| note.end)
                })
                .reduce(f64::max)
                .unwrap_or(editor.visible_position);
            editor.checkpoint("Duplicate notes");
            let inserted = paste_chart_notes(
                &mut editor.document,
                &clipboard,
                selected_end + editor.snap_seconds.max(0.02),
            );
            editor.selected_note = inserted.iter().next().copied();
            editor.selected_notes = inserted;
            editor.dirty = true;
            session.notice = Some("Duplicated selected note(s).".to_string());
            invalidated.0 = true;
        }
        return;
    }
    if keys.just_pressed(KeyCode::Delete) || keys.just_pressed(KeyCode::Backspace) {
        if let Some(editor) = session.editor.as_mut() {
            let selected = editor.selected_note_indices();
            if selected.is_empty() {
                let words = editor.selected_word_indices();
                if !words.is_empty() {
                    editor.checkpoint("Duplicate notes");
                    let deleted = delete_editor_words(&mut editor.document, &words);
                    if deleted > 0 {
                        editor.selected_word = None;
                        editor.selected_words.clear();
                        editor.word_edit_focus = None;
                        editor.dirty = true;
                        session.notice = Some(format!("Deleted {deleted} lyric word(s)."));
                        invalidated.0 = true;
                    } else {
                        editor.undo.pop();
                    }
                }
                return;
            }
            editor.checkpoint("Delete lyrics");
            let removed = remove_chart_notes(&mut editor.document, &selected);
            if removed > 0 {
                editor.selected_note = None;
                editor.selected_notes.clear();
                editor.dirty = true;
                session.notice = Some(format!("Deleted {removed} note(s)."));
                invalidated.0 = true;
            }
        }
        return;
    }
    if keys.just_pressed(KeyCode::KeyS) {
        if let Some(editor) = session.editor.as_mut() {
            let selected = editor.selected_note_indices();
            if !selected.is_empty() {
                editor.checkpoint("Delete notes");
                let next =
                    split_chart_notes(&mut editor.document, &selected, editor.visible_position);
                editor.selected_note = next.iter().next().copied();
                editor.selected_notes = next;
                editor.dirty = true;
                session.notice = Some("Split selected note(s).".to_string());
                invalidated.0 = true;
            }
        }
        return;
    }
    if keys.just_pressed(KeyCode::KeyM) {
        if let Some(editor) = session.editor.as_mut() {
            let selected = editor.selected_note_indices();
            if selected.len() > 1 {
                editor.checkpoint("Split notes");
                if let Some(index) =
                    merge_chart_notes(&mut editor.document, &selected, editor.selected_note)
                {
                    editor.select_only_note(index);
                    editor.dirty = true;
                    session.notice = Some("Merged selected notes.".to_string());
                    invalidated.0 = true;
                }
            }
        }
        return;
    }
    if keys.just_pressed(KeyCode::KeyQ) {
        if let Some(editor) = session.editor.as_mut() {
            let selected = editor.selected_note_indices();
            if !selected.is_empty() && editor.snap_seconds > 0.0 {
                editor.checkpoint("Merge notes");
                quantize_chart_notes(&mut editor.document, Some(&selected), editor.snap_seconds);
                editor.dirty = true;
                session.notice = Some("Quantized selected note(s).".to_string());
                invalidated.0 = true;
            }
        }
        return;
    }
    if keys.just_pressed(KeyCode::Tab) {
        if let Some(editor) = session.editor.as_mut() {
            let count = chart_notes(&editor.document).len();
            if count > 0 {
                let next = editor.selected_note.map_or(0, |index| {
                    if shift {
                        (index + count - 1) % count
                    } else {
                        (index + 1) % count
                    }
                });
                editor.select_only_note(next);
                invalidated.0 = true;
            }
        }
        return;
    }
    if keys.just_pressed(KeyCode::Escape) {
        if let Some(editor) = session.editor.as_mut()
            && editor.inspector_open
        {
            editor.inspector_open = false;
            invalidated.0 = true;
        }
        return;
    }
    if keys.just_pressed(KeyCode::Space) {
        session.notice = toggle_editor_playback(&audio.0, session.editor.as_mut()).err();
        invalidated.0 = true;
        return;
    }
    let left = keys.just_pressed(KeyCode::ArrowLeft);
    let right = keys.just_pressed(KeyCode::ArrowRight);
    let up = keys.just_pressed(KeyCode::ArrowUp);
    let down = keys.just_pressed(KeyCode::ArrowDown);
    let delta = if left {
        Some(-2.0)
    } else if right {
        Some(2.0)
    } else {
        None
    };
    if let Some(editor) = session.editor.as_mut() {
        let selected = editor.selected_note_indices();
        if selected.is_empty() && (left || right) && !editor.selected_word_indices().is_empty() {
            let time_step = if editor.snap_seconds > 0.0 {
                editor.snap_seconds
            } else {
                0.01
            };
            editor.checkpoint("Nudge lyrics");
            let words = editor.selected_word_indices();
            let moved = words
                .iter()
                .filter(|selection| {
                    shift_editor_word(
                        &mut editor.document,
                        **selection,
                        if left { -time_step } else { time_step },
                    )
                })
                .count();
            if moved > 0 {
                editor.dirty = true;
                session.notice = Some(format!(
                    "Moved {moved} lyric word(s) {} by {}.",
                    if left { "earlier" } else { "later" },
                    format_snap_grid(time_step)
                ));
                invalidated.0 = true;
            } else {
                editor.undo.pop();
            }
            return;
        }
        if !selected.is_empty() && (left || right || up || down) {
            editor.checkpoint("Nudge notes");
            let time_step = if editor.snap_seconds > 0.0 {
                editor.snap_seconds
            } else {
                0.01
            };
            let seconds = if left {
                -time_step
            } else if right {
                time_step
            } else {
                0.0
            };
            let semitones = if up {
                if shift { 12.0 } else { 1.0 }
            } else if down {
                if shift { -12.0 } else { -1.0 }
            } else {
                0.0
            };
            shift_chart_notes(
                &mut editor.document,
                &selected,
                seconds,
                semitones,
                shift && (left || right),
            );
            editor.dirty = true;
            session.notice = None;
            invalidated.0 = true;
            return;
        }
    }
    let Some(delta) = delta else {
        return;
    };
    let Some(editor) = session.editor.as_mut() else {
        return;
    };
    let was_playing = editor.audio_status.playing;
    let target = (editor.visible_position + delta).max(0.0);
    match audio.0.seek(target) {
        Ok(mut status) => {
            if was_playing && let Ok(playing) = audio.0.play() {
                status = playing;
            }
            editor.visible_position = status.position_secs;
            editor.audio_status = status;
            editor.last_audio_sync = Instant::now();
            session.notice = None;
        }
        Err(error) => session.notice = Some(error),
    }
    invalidated.0 = true;
}

pub(crate) fn handle_editor_wheel(
    mut wheel: MessageReader<bevy::input::mouse::MouseWheel>,
    keys: Res<ButtonInput<KeyCode>>,
    mut session: ResMut<StudioSession>,
    mut invalidated: ResMut<UiInvalidated>,
) {
    if session.route != StudioRoute::Editor {
        wheel.clear();
        return;
    }
    let delta = wheel.read().map(|event| event.y + event.x).sum::<f32>();
    if delta.abs() < f32::EPSILON {
        return;
    }
    let Some(editor) = session.editor.as_mut() else {
        return;
    };
    let control = keys.any_pressed([KeyCode::ControlLeft, KeyCode::ControlRight]);
    let alt = keys.any_pressed([KeyCode::AltLeft, KeyCode::AltRight]);
    let shift = keys.any_pressed([KeyCode::ShiftLeft, KeyCode::ShiftRight]);
    if control {
        let center = editor.viewport_start + editor.viewport_duration / 2.0;
        editor.viewport_duration =
            (editor.viewport_duration * (1.0 - f64::from(delta) * 0.08)).clamp(2.0, 180.0);
        editor.viewport_start = (center - editor.viewport_duration / 2.0).max(0.0);
    } else if alt {
        let span = ((editor.pitch_max - editor.pitch_min) * (1.0 - f64::from(delta) * 0.08))
            .clamp(8.0, 127.0);
        set_editor_pitch_span(editor, span);
    } else if shift {
        let span = editor.pitch_max - editor.pitch_min;
        let offset = f64::from(delta) * span * 0.05;
        editor.pitch_min = (editor.pitch_min + offset).clamp(0.0, 127.0 - span);
        editor.pitch_max = editor.pitch_min + span;
    } else {
        editor.viewport_start =
            (editor.viewport_start - f64::from(delta) * editor.viewport_duration * 0.08).max(0.0);
    }
    editor.manual_scroll_until = Instant::now() + Duration::from_millis(1400);
    invalidated.0 = true;
}

// Pointer capture coordinates multiple independent ECS inputs in one frame.
#[allow(clippy::too_many_arguments, clippy::type_complexity)]
pub(crate) fn handle_editor_pointer_capture(
    mouse: Res<ButtonInput<MouseButton>>,
    keys: Res<ButtonInput<KeyCode>>,
    windows: Query<&Window, With<PrimaryWindow>>,
    audio: Res<NativeAudio>,
    mut focus_events: MessageReader<bevy::window::WindowFocused>,
    resize_interactions: Query<(&Interaction, &EditorNoteResizeHandle), Changed<Interaction>>,
    lyric_resize_interactions: Query<
        (&Interaction, &EditorLyricResizeHandle),
        Changed<Interaction>,
    >,
    note_interactions: Query<
        (&Interaction, &EditorNoteNode),
        (Changed<Interaction>, Without<EditorTimelineSurface>),
    >,
    lyric_interactions: Query<
        (&Interaction, &EditorLyricNode),
        (Changed<Interaction>, Without<EditorTimelineSurface>),
    >,
    surface_interactions: Query<
        &Interaction,
        (
            Changed<Interaction>,
            With<EditorTimelineSurface>,
            Without<EditorNoteNode>,
        ),
    >,
    timeline: Query<(&ComputedNode, &UiGlobalTransform), With<EditorTimelineSurface>>,
    lyrics_surface: Query<
        (&ComputedNode, &UiGlobalTransform),
        (With<EditorLyricsSurface>, Without<EditorTimelineSurface>),
    >,
    mut capture: ResMut<EditorPointerCapture>,
    mut session: ResMut<StudioSession>,
    mut invalidated: ResMut<UiInvalidated>,
) {
    let focus_lost = focus_events.read().any(|event| !event.focused);
    if session.route != StudioRoute::Editor || focus_lost || !mouse.pressed(MouseButton::Left) {
        let finished = capture.drag.take();
        capture.alignment_guide = None;
        let had_finished = finished.is_some();
        if mouse.just_released(MouseButton::Left)
            && let Some(EditorDrag::Pan { pointer_start, .. }) = finished
            && let Ok(window) = windows.single()
            && let Some(pointer) = window.cursor_position()
            && pointer.distance(pointer_start) <= 5.0
            && let Ok((computed, global_transform)) = timeline.single()
        {
            let size = computed.size() * computed.inverse_scale_factor();
            if size.x > 1.0
                && let Some(editor) = session.editor.as_mut()
            {
                let local = global_transform
                    .affine()
                    .inverse()
                    .transform_point2(pointer);
                let fraction = (local.x / size.x + 0.5).clamp(0.0, 1.0);
                let surface_y = (local.y / size.y + 0.5).clamp(0.0, 1.0);
                let target = editor.viewport_start + f64::from(fraction) * editor.viewport_duration;
                let pitch_surface = surface_y * 100.0 >= EDITOR_PITCH_TOP_PERCENT
                    && surface_y * 100.0 <= EDITOR_PITCH_TOP_PERCENT + EDITOR_PITCH_HEIGHT_PERCENT;
                let double_click = pitch_surface
                    && capture.last_surface_click.is_some_and(|(at, previous)| {
                        at.elapsed() <= Duration::from_millis(360)
                            && previous.distance(pointer) <= 7.0
                    });
                capture.last_surface_click = if double_click {
                    None
                } else {
                    Some((Instant::now(), pointer))
                };
                if double_click {
                    let start = if editor.snap_seconds > 0.0 {
                        (target / editor.snap_seconds).round() * editor.snap_seconds
                    } else {
                        target
                    }
                    .max(0.0);
                    let pitch_fraction = surface_pitch_fraction(surface_y);
                    let midi = (editor.pitch_max
                        - f64::from(pitch_fraction) * (editor.pitch_max - editor.pitch_min))
                        .round()
                        .clamp(0.0, 127.0);
                    editor.checkpoint("Add note");
                    if let Some(index) = insert_chart_note(
                        &mut editor.document,
                        start,
                        start + editor.snap_seconds.max(0.25),
                        midi,
                    ) {
                        editor.select_only_note(index);
                        editor.dirty = true;
                        session.notice = Some("Added note at the pointer.".to_string());
                    }
                    invalidated.0 = true;
                    return;
                }
                let was_playing = editor.audio_status.playing;
                match audio.0.seek(target) {
                    Ok(mut status) => {
                        if was_playing && let Ok(playing) = audio.0.play() {
                            status = playing;
                        }
                        editor.visible_position = status.position_secs;
                        editor.audio_status = status;
                        editor.last_audio_sync = Instant::now();
                        session.notice = None;
                    }
                    Err(error) => session.notice = Some(error),
                }
            }
        }
        if had_finished {
            invalidated.0 = true;
        }
        return;
    }

    let Ok(window) = windows.single() else {
        capture.drag = None;
        return;
    };
    let Some(pointer) = window.cursor_position() else {
        // Keep the logical capture while the pointer is temporarily outside the
        // window. A global release or focus-loss event still clears it above.
        return;
    };
    let Some(editor) = session.editor.as_mut() else {
        capture.drag = None;
        return;
    };

    if capture.drag.is_none() && mouse.just_pressed(MouseButton::Left) {
        let pressed_resize = resize_interactions
            .iter()
            .find_map(|(interaction, handle)| {
                (*interaction == Interaction::Pressed).then_some((handle.index, handle.edge))
            });
        let pressed_lyric_resize =
            lyric_resize_interactions
                .iter()
                .find_map(|(interaction, handle)| {
                    (*interaction == Interaction::Pressed)
                        .then_some((handle.selection, handle.edge))
                });
        let pressed_note = note_interactions.iter().find_map(|(interaction, note)| {
            (*interaction == Interaction::Pressed).then_some(note.0)
        });
        let pressed_lyric = lyric_interactions.iter().find_map(|(interaction, lyric)| {
            (*interaction == Interaction::Pressed).then_some(lyric.selection)
        });
        if let Some((selection, edge)) = pressed_lyric_resize {
            if let Some((_, start, end)) = selected_editor_word(&editor.document, selection) {
                editor.checkpoint("Resize lyric");
                capture.drag = Some(EditorDrag::ResizeLyric {
                    selection,
                    edge,
                    pointer_start: pointer,
                    original_start: start,
                    original_end: end,
                    viewport_duration: editor.viewport_duration,
                });
                editor.select_only_word(selection);
                editor.inspector_open = true;
                editor.manual_scroll_until = Instant::now() + Duration::from_millis(1400);
                invalidated.0 = true;
            }
        } else if let Some(selection) = pressed_lyric {
            let modifier = keys.any_pressed([
                KeyCode::ShiftLeft,
                KeyCode::ShiftRight,
                KeyCode::ControlLeft,
                KeyCode::ControlRight,
            ]);
            let double_click = !modifier
                && capture.last_lyric_click.is_some_and(|(at, previous)| {
                    at.elapsed() <= Duration::from_millis(360) && previous == selection
                });
            capture.last_lyric_click = if double_click {
                None
            } else {
                Some((Instant::now(), selection))
            };
            if double_click {
                capture.drag = None;
                editor.word_edit_focus = Some(selection);
            } else if !modifier {
                let selected = if editor.selected_words.contains(&selection) {
                    editor.selected_word_indices()
                } else {
                    [selection].into_iter().collect()
                };
                let originals = selected
                    .into_iter()
                    .filter_map(|selection| {
                        selected_editor_word(&editor.document, selection).map(|(_, start, end)| {
                            EditorWordOriginal {
                                selection,
                                start,
                                end,
                            }
                        })
                    })
                    .collect::<Vec<_>>();
                editor.checkpoint("Move lyric");
                capture.drag = Some(EditorDrag::Lyric {
                    pointer_start: pointer,
                    originals,
                    viewport_duration: editor.viewport_duration,
                });
            }
            editor.manual_scroll_until = Instant::now() + Duration::from_millis(1400);
            invalidated.0 = true;
        } else if let Some((index, edge)) = pressed_resize {
            if let Some(note) = chart_notes(&editor.document)
                .into_iter()
                .find(|note| note.index == index)
            {
                editor.checkpoint("Resize note");
                capture.drag = Some(EditorDrag::ResizeNote {
                    index,
                    edge,
                    pointer_start: pointer,
                    original_start: note.start,
                    original_end: note.end,
                    viewport_duration: editor.viewport_duration,
                });
                editor.select_only_note(index);
                editor.manual_scroll_until = Instant::now() + Duration::from_millis(1400);
                invalidated.0 = true;
            }
        } else if let Some(index) = pressed_note {
            let shift = keys.any_pressed([KeyCode::ShiftLeft, KeyCode::ShiftRight]);
            if shift {
                if !editor.selected_notes.remove(&index) {
                    editor.selected_notes.insert(index);
                    editor.selected_note = Some(index);
                } else {
                    editor.selected_note = editor.selected_notes.iter().next().copied();
                }
                editor.selected_word = None;
                editor.selected_words.clear();
                editor.word_edit_focus = None;
                invalidated.0 = true;
                return;
            }
            if !editor.selected_notes.contains(&index) {
                editor.select_only_note(index);
            }
            let selected = editor.selected_note_indices();
            let originals = chart_notes(&editor.document)
                .into_iter()
                .filter(|note| selected.contains(&note.index))
                .map(|note| EditorNoteOriginal {
                    index: note.index,
                    start: note.start,
                    end: note.end,
                    midi: note.midi,
                })
                .collect::<Vec<_>>();
            if !originals.is_empty() {
                editor.checkpoint("Move note");
                capture.drag = Some(EditorDrag::Note {
                    pointer_start: pointer,
                    originals,
                    viewport_duration: editor.viewport_duration,
                    pitch_span: editor.pitch_max - editor.pitch_min,
                });
                editor.selected_note = Some(index);
                editor.selected_word = None;
                editor.selected_words.clear();
                editor.word_edit_focus = None;
                editor.manual_scroll_until = Instant::now() + Duration::from_millis(1400);
                invalidated.0 = true;
            }
        } else if surface_interactions
            .iter()
            .any(|interaction| *interaction == Interaction::Pressed)
        {
            if keys.any_pressed([KeyCode::ShiftLeft, KeyCode::ShiftRight]) {
                capture.drag = Some(EditorDrag::Marquee {
                    pointer_start: pointer,
                    base: editor.selected_note_indices(),
                    viewport_start: editor.viewport_start,
                    viewport_duration: editor.viewport_duration,
                    pitch_min: editor.pitch_min,
                    pitch_max: editor.pitch_max,
                });
            } else {
                capture.drag = Some(EditorDrag::Pan {
                    pointer_start: pointer,
                    viewport_start: editor.viewport_start,
                    pitch_min: editor.pitch_min,
                    pitch_max: editor.pitch_max,
                });
                editor.selected_word = None;
                editor.selected_words.clear();
                editor.word_edit_focus = None;
            }
            editor.manual_scroll_until = Instant::now() + Duration::from_millis(1400);
        }
    }

    let Some(drag) = capture.drag.clone() else {
        return;
    };
    let surface = if matches!(
        drag,
        EditorDrag::Lyric { .. } | EditorDrag::ResizeLyric { .. }
    ) {
        lyrics_surface.single()
    } else {
        timeline.single()
    };
    let Ok((computed, global_transform)) = surface else {
        return;
    };
    let size = computed.size() * computed.inverse_scale_factor();
    if size.x <= 1.0 || size.y <= 1.0 {
        return;
    }
    let delta = pointer
        - match drag {
            EditorDrag::Note { pointer_start, .. }
            | EditorDrag::ResizeNote { pointer_start, .. }
            | EditorDrag::Lyric { pointer_start, .. }
            | EditorDrag::ResizeLyric { pointer_start, .. }
            | EditorDrag::Pan { pointer_start, .. }
            | EditorDrag::Marquee { pointer_start, .. } => pointer_start,
        };
    capture.alignment_guide = None;

    match drag {
        EditorDrag::Note {
            originals,
            viewport_duration,
            pitch_span,
            ..
        } => {
            let raw_time_delta = f64::from(delta.x / size.x) * viewport_duration;
            let earliest = originals
                .iter()
                .map(|note| note.start)
                .reduce(f64::min)
                .unwrap_or(0.0);
            let time_delta = raw_time_delta.max(-earliest);
            let pitch_delta =
                -f64::from(delta.y / (size.y * (EDITOR_PITCH_HEIGHT_PERCENT / 100.0))) * pitch_span;
            let mut moved = 0usize;
            for original in &originals {
                let start = (original.start + time_delta).max(0.0);
                let end = start + (original.end - original.start).max(0.03);
                let midi = (original.midi + pitch_delta).round().clamp(0.0, 127.0);
                moved += usize::from(move_chart_note(
                    &mut editor.document,
                    original.index,
                    start,
                    end,
                    midi,
                ));
            }
            if moved > 0 {
                editor.dirty = true;
            } else {
                capture.drag = None;
                invalidated.0 = true;
            }
        }
        EditorDrag::ResizeNote {
            index,
            edge,
            original_start,
            original_end,
            viewport_duration,
            ..
        } => {
            let time_delta = f64::from(delta.x / size.x) * viewport_duration;
            let (start, end) = match edge {
                NoteEdge::Start => (
                    (original_start + time_delta).clamp(0.0, original_end - 0.02),
                    original_end,
                ),
                NoteEdge::End => (
                    original_start,
                    (original_end + time_delta).max(original_start + 0.02),
                ),
            };
            if resize_chart_note(&mut editor.document, index, start, end) {
                editor.dirty = true;
            } else {
                capture.drag = None;
                invalidated.0 = true;
            }
        }
        EditorDrag::Lyric {
            originals,
            viewport_duration,
            ..
        } => {
            let raw_time_delta = f64::from(delta.x / size.x) * viewport_duration;
            let earliest = originals
                .iter()
                .map(|word| word.start)
                .reduce(f64::min)
                .unwrap_or(0.0);
            let proposed_delta = raw_time_delta.max(-earliest);
            let snap_tolerance = (f64::from(EDITOR_LYRIC_NOTE_SNAP_DISTANCE_PX / size.x)
                * viewport_duration)
                .min(EDITOR_LYRIC_NOTE_SNAP_MAX_SECONDS);
            let snap = (!keys.any_pressed([KeyCode::AltLeft, KeyCode::AltRight]))
                .then(|| {
                    snap_lyric_move_to_notes(
                        &originals,
                        proposed_delta,
                        &chart_notes(&editor.document),
                        snap_tolerance,
                    )
                })
                .flatten();
            let time_delta = snap
                .map(|snap| snap.delta)
                .unwrap_or(proposed_delta)
                .max(-earliest);
            capture.alignment_guide = snap.map(|snap| snap.target);
            let moved = originals
                .iter()
                .filter(|word| {
                    set_editor_word_timing(
                        &mut editor.document,
                        word.selection,
                        word.start + time_delta,
                        word.end + time_delta,
                    )
                })
                .count();
            if moved > 0 {
                editor.dirty = true;
            } else {
                capture.drag = None;
                invalidated.0 = true;
            }
        }
        EditorDrag::ResizeLyric {
            selection,
            edge,
            original_start,
            original_end,
            viewport_duration,
            ..
        } => {
            let time_delta = f64::from(delta.x / size.x) * viewport_duration;
            let snap_tolerance = (f64::from(EDITOR_LYRIC_NOTE_SNAP_DISTANCE_PX / size.x)
                * viewport_duration)
                .min(EDITOR_LYRIC_NOTE_SNAP_MAX_SECONDS);
            let note_boundaries = chart_notes(&editor.document);
            let allow_snap = !keys.any_pressed([KeyCode::AltLeft, KeyCode::AltRight]);
            let (start, end, guide) = match edge {
                NoteEdge::Start => {
                    let proposed = (original_start + time_delta).clamp(0.0, original_end - 0.01);
                    let snapped = allow_snap
                        .then(|| nearest_note_boundary(proposed, &note_boundaries, snap_tolerance))
                        .flatten()
                        .filter(|target| *target <= original_end - 0.01);
                    (snapped.unwrap_or(proposed), original_end, snapped)
                }
                NoteEdge::End => {
                    let proposed = (original_end + time_delta).max(original_start + 0.01);
                    let snapped = allow_snap
                        .then(|| nearest_note_boundary(proposed, &note_boundaries, snap_tolerance))
                        .flatten()
                        .filter(|target| *target >= original_start + 0.01);
                    (original_start, snapped.unwrap_or(proposed), snapped)
                }
            };
            capture.alignment_guide = guide;
            if set_editor_word_timing(&mut editor.document, selection, start, end) {
                editor.dirty = true;
            } else {
                capture.drag = None;
                invalidated.0 = true;
            }
        }
        EditorDrag::Pan {
            viewport_start,
            pitch_min,
            pitch_max,
            ..
        } => {
            editor.viewport_start =
                (viewport_start - f64::from(delta.x / size.x) * editor.viewport_duration).max(0.0);
            let pitch_span = pitch_max - pitch_min;
            let pitch_offset =
                f64::from(delta.y / (size.y * (EDITOR_PITCH_HEIGHT_PERCENT / 100.0))) * pitch_span;
            editor.pitch_min = (pitch_min + pitch_offset).clamp(0.0, 127.0 - pitch_span);
            editor.pitch_max = editor.pitch_min + pitch_span;
        }
        EditorDrag::Marquee {
            pointer_start,
            base,
            viewport_start,
            viewport_duration,
            pitch_min,
            pitch_max,
        } => {
            let inverse = global_transform.affine().inverse();
            let start = inverse.transform_point2(pointer_start) / size;
            let current = inverse.transform_point2(pointer) / size;
            let left = start.x.min(current.x) + 0.5;
            let right = start.x.max(current.x) + 0.5;
            let top = surface_pitch_fraction(start.y.min(current.y) + 0.5);
            let bottom = surface_pitch_fraction(start.y.max(current.y) + 0.5);
            let time_start = viewport_start + f64::from(left) * viewport_duration;
            let time_end = viewport_start + f64::from(right) * viewport_duration;
            let pitch_span = pitch_max - pitch_min;
            let midi_max = pitch_max - f64::from(top) * pitch_span;
            let midi_min = pitch_max - f64::from(bottom) * pitch_span;
            let mut selected = base;
            for note in chart_notes(&editor.document) {
                if note.end >= time_start
                    && note.start <= time_end
                    && note.midi >= midi_min
                    && note.midi <= midi_max
                {
                    selected.insert(note.index);
                }
            }
            editor.selected_note = selected.iter().next().copied();
            editor.selected_notes = selected;
            editor.selected_word = None;
            editor.selected_words.clear();
            editor.word_edit_focus = None;
        }
    }
    editor.manual_scroll_until = Instant::now() + Duration::from_millis(1400);
}
