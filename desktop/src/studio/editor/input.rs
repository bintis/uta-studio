//! Editor input: keyboard, wheel, and pointer capture translation.

use crate::studio::*;

/// Ends a held tap when the key comes back up, and rescues one that was left
/// held when playback stopped or focus moved into a text field.
pub(crate) fn handle_tap_release(
    keys: Res<ButtonInput<KeyCode>>,
    focus: Res<InputFocus>,
    editable: Query<(), With<EditableText>>,
    mut session: ResMut<StudioSession>,
    mut invalidated: ResMut<UiInvalidated>,
) {
    let typing = focus.get().is_some_and(|entity| editable.contains(entity));
    let tap_key = chord_key_code(
        app_core::editor_action("tap_note")
            .and_then(|action| action.shortcuts.first())
            .map(|chord| chord.key)
            .unwrap_or_default(),
    );
    let released = tap_key.is_some_and(|key| keys.just_released(key));
    let Some(editor) = session.editor.as_mut() else {
        return;
    };
    if editor.tap.holding.is_none() {
        return;
    }
    if released || typing || !editor.audio_status.playing {
        if finish_tap(editor) {
            let remaining = editor.tap.remaining();
            session.notice = (remaining > 0)
                .then(|| format!("{remaining} note(s) left to re-time."))
                .or_else(|| Some("Tapped the last queued note.".to_string()));
            invalidated.0 = true;
        }
    }
}

pub(crate) fn sync_editor_word_input(
    inputs: Query<(Ref<EditableText>, &EditorWordInput)>,
    mut session: ResMut<StudioSession>,
) {
    let Some(editor) = session.editor.as_mut() else {
        return;
    };
    for (input, marker) in &inputs {
        // `Changed<EditableText>` also fires the instant the component is
        // spawned — including every time this widget respawns for a
        // *different* selection on a UI rebuild (e.g. right after Unbind
        // reselects a freshly detached word). Reacting to that as "the user
        // edited it" wrote the newly spawned widget's seed value back over
        // whatever `current` had briefly become out of sync with by then,
        // corrupting or blanking real text. Only a change to an
        // already-existing entity is a genuine edit.
        if input.is_added() || !input.is_changed() {
            continue;
        }
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

pub(crate) fn handle_editor_wheel(
    mut wheel: MessageReader<bevy::input::mouse::MouseWheel>,
    keys: Res<ButtonInput<KeyCode>>,
    mut session: ResMut<StudioSession>,
    mut invalidated: ResMut<UiInvalidated>,
) {
    if session.route != StudioRoute::Editor
        || session
            .editor
            .as_ref()
            .is_some_and(|editor| editor.problems_panel_open || editor.shortcuts_panel_open)
    {
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
    mut marquee_box: Query<&mut Node, With<EditorMarqueeBox>>,
    mut capture: ResMut<EditorPointerCapture>,
    mut session: ResMut<StudioSession>,
    mut invalidated: ResMut<UiInvalidated>,
) {
    let focus_lost = focus_events.read().any(|event| !event.focused);
    if session.route != StudioRoute::Editor || focus_lost || !mouse.pressed(MouseButton::Left) {
        for mut node in &mut marquee_box {
            node.display = Display::None;
        }
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
    // Locking mid-drag cancels it immediately rather than letting the
    // in-flight move/resize finish.
    if editor.lock_mode
        && matches!(
            capture.drag,
            Some(
                EditorDrag::Note { .. }
                    | EditorDrag::Lyric { .. }
                    | EditorDrag::ResizeNote { .. }
                    | EditorDrag::ResizeLyric { .. }
            )
        )
    {
        capture.drag = None;
    }

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
        let bind_held = keys.pressed(KeyCode::KeyB);
        let unbind_held = keys.pressed(KeyCode::KeyC);
        if bind_held && (pressed_note.is_some() || pressed_lyric.is_some()) {
            let pair = pressed_note
                .and_then(|note_index| editor.selected_word.map(|word| (word, note_index)))
                .or_else(|| {
                    pressed_lyric
                        .and_then(|word| editor.selected_note.map(|note_index| (word, note_index)))
                });
            editor.checkpoint("Bind lyric to note");
            match pair.and_then(|(word, note_index)| {
                bind_editor_lyric(&mut editor.document, word, note_index)
            }) {
                Some(bound) => {
                    editor.select_only_note(bound);
                    editor.dirty = true;
                    session.notice = Some("Bound lyric to note.".to_string());
                }
                None => {
                    editor.undo.pop();
                    session.notice = Some(
                        "Select an unpitched lyric and a lyric-less note, then hold B and click the other one to bind them."
                            .to_string(),
                    );
                }
            }
            invalidated.0 = true;
            return;
        } else if unbind_held && (pressed_note.is_some() || pressed_lyric.is_some()) {
            let note_index = pressed_note.or_else(|| {
                pressed_lyric.and_then(|word| editor_note_for_word(&editor.document, word))
            });
            editor.checkpoint("Unbind note");
            match note_index.and_then(|index| unbind_editor_note(&mut editor.document, index)) {
                Some(freed) => {
                    editor.select_only_word(freed);
                    editor.dirty = true;
                    session.notice = Some("Unbound lyric from note.".to_string());
                }
                None => {
                    editor.undo.pop();
                    session.notice =
                        Some("This note has no separable pitch and lyric to unbind.".to_string());
                }
            }
            invalidated.0 = true;
            return;
        }
        if let Some((selection, edge)) = pressed_lyric_resize {
            if let Some((_, start, end)) = selected_editor_word(&editor.document, selection) {
                editor.select_only_word(selection);
                editor.inspector_open = true;
                if !editor.lock_mode {
                    editor.checkpoint("Resize lyric");
                    capture.drag = Some(EditorDrag::ResizeLyric {
                        selection,
                        edge,
                        pointer_start: pointer,
                        original_start: start,
                        original_end: end,
                        viewport_duration: editor.viewport_duration,
                    });
                }
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
                if !editor.lock_mode {
                    editor.checkpoint("Move lyric");
                    capture.drag = Some(EditorDrag::Lyric {
                        pointer_start: pointer,
                        originals,
                        viewport_duration: editor.viewport_duration,
                    });
                }
            }
            editor.manual_scroll_until = Instant::now() + Duration::from_millis(1400);
            invalidated.0 = true;
        } else if let Some((index, edge)) = pressed_resize {
            if let Some(note) = chart_notes(&editor.document)
                .into_iter()
                .find(|note| note.index == index)
            {
                editor.select_only_note(index);
                if !editor.lock_mode {
                    editor.checkpoint("Resize note");
                    capture.drag = Some(EditorDrag::ResizeNote {
                        index,
                        edge,
                        pointer_start: pointer,
                        original_start: note.start,
                        original_end: note.end,
                        viewport_duration: editor.viewport_duration,
                    });
                }
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
                if !editor.lock_mode {
                    editor.checkpoint("Move note");
                    capture.drag = Some(EditorDrag::Note {
                        pointer_start: pointer,
                        originals,
                        viewport_duration: editor.viewport_duration,
                        pitch_span: editor.pitch_max - editor.pitch_min,
                    });
                }
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
            if editor.note_insert_armed {
                // The "Add note" button armed this: place a minimal note
                // where the pointer went down, then let the drag below size
                // it as the pointer moves, in either direction.
                editor.note_insert_armed = false;
                if let Ok((computed, global_transform)) = timeline.single()
                    && computed.size().x > 1.0
                    && computed.size().y > 1.0
                {
                    let size = computed.size() * computed.inverse_scale_factor();
                    let local = global_transform
                        .affine()
                        .inverse()
                        .transform_point2(pointer);
                    let fraction = (local.x / size.x + 0.5).clamp(0.0, 1.0);
                    let surface_y = (local.y / size.y + 0.5).clamp(0.0, 1.0);
                    let target =
                        editor.viewport_start + f64::from(fraction) * editor.viewport_duration;
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
                    let min_len = editor.snap_seconds.max(0.05);
                    if let Some(index) =
                        insert_chart_note(&mut editor.document, start, start + min_len, midi)
                    {
                        editor.select_only_note(index);
                        editor.dirty = true;
                        capture.drag = Some(EditorDrag::InsertNote {
                            note_index: index,
                            anchor_time: start,
                            pointer_start: pointer,
                            viewport_duration: editor.viewport_duration,
                        });
                        invalidated.0 = true;
                    } else {
                        editor.undo.pop();
                    }
                }
            } else if keys.any_pressed([KeyCode::ShiftLeft, KeyCode::ShiftRight]) {
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
                // A click on empty canvas is also how the workspace clears
                // whatever was selected — it used to leave a selected note
                // in place, only clearing lyric selection.
                editor.selected_note = None;
                editor.selected_notes.clear();
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
    if !matches!(drag, EditorDrag::Marquee { .. }) {
        for mut node in &mut marquee_box {
            node.display = Display::None;
        }
    }
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
            | EditorDrag::Marquee { pointer_start, .. }
            | EditorDrag::InsertNote { pointer_start, .. } => pointer_start,
        };
    capture.alignment_guide = None;

    // A note/lyric move or resize only takes effect once the pointer has
    // actually moved; otherwise a plain click (which frequently jitters a
    // pixel or two) can fall inside the snap radius below and silently
    // resize a bound note. Pan and marquee-select stay responsive from the
    // first frame since neither one edits the chart.
    let is_editing_drag = matches!(
        drag,
        EditorDrag::Note { .. }
            | EditorDrag::ResizeNote { .. }
            | EditorDrag::Lyric { .. }
            | EditorDrag::ResizeLyric { .. }
    );
    if is_editing_drag && delta.length() < EDITOR_DRAG_MIN_PX {
        return;
    }

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
        EditorDrag::InsertNote {
            note_index,
            anchor_time,
            viewport_duration,
            ..
        } => {
            let time_delta = f64::from(delta.x / size.x) * viewport_duration;
            let current_time = (anchor_time + time_delta).max(0.0);
            let start = anchor_time.min(current_time);
            let end = anchor_time.max(current_time).max(start + 0.02);
            if resize_chart_note(&mut editor.document, note_index, start, end) {
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
            let raw_top = (start.y.min(current.y) + 0.5).clamp(0.0, 1.0);
            let raw_bottom = (start.y.max(current.y) + 0.5).clamp(0.0, 1.0);
            if let Ok(mut node) = marquee_box.single_mut() {
                let clamped_left = left.clamp(0.0, 1.0);
                let clamped_right = right.clamp(0.0, 1.0);
                node.display = Display::Flex;
                node.left = percent(clamped_left * 100.0);
                node.width = percent(((clamped_right - clamped_left) * 100.0).max(0.0));
                node.top = percent(raw_top * 100.0);
                node.height = percent(((raw_bottom - raw_top) * 100.0).max(0.0));
            }
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
