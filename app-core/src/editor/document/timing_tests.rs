fn word(index: usize) -> LyricAddress {
    LyricAddress {
        segment: 0,
        word: index,
    }
}

fn independent_document() -> EditorDocument {
    let mut document = document(&[(0.0, 2.0, 60, "切れ"), (2.0, 3.0, 62, "た")]);
    let mut extra = document.token_at(word(0)).unwrap().clone();
    extra.id = "lyric-extra".into();
    extra.text = "に".into();
    extra.join_before = LyricJoin::None;
    extra.reading = Some("に".into());
    extra.timing_unresolved = true;
    extra.timing = Some(utz::LyricTiming {
        start: 1_000_000,
        duration: 500_000,
    });
    document.token_mut(word(0)).unwrap().timing = Some(utz::LyricTiming {
        start: 500_000,
        duration: 500_000,
    });
    document.chart.tracks[0].phrases[0].notes[0]
        .lyrics
        .push(LyricToken::Text(extra));
    document
}

fn geometry(document: &EditorDocument) -> Vec<(u64, u64, Option<NotePitch>)> {
    document.chart.tracks[0]
        .phrases
        .iter()
        .flat_map(|phrase| &phrase.notes)
        .map(|note| (note.start, note.duration, note.pitch))
        .collect()
}

#[test]
fn independent_timing_projects_each_word_and_tab_visits_shared_note_tokens() {
    let mut document = independent_document();
    let lyrics = document.lyrics();
    assert_eq!((lyrics[0].start, lyrics[0].end), (0.5, 1.0));
    assert_eq!((lyrics[1].start, lyrics[1].end), (1.0, 1.5));
    assert_eq!(document.advance_lyric_edit(word(0), true), Some(word(1)));
    assert_eq!(document.advance_lyric_edit(word(1), true), Some(word(2)));
    assert_eq!(document.advance_lyric_edit(word(2), false), Some(word(1)));
    assert_eq!(document.advance_lyric_edit(word(1), false), Some(word(0)));
    document.to_chart().validate().unwrap();
}

#[test]
fn independent_timing_edits_do_not_move_notes_or_other_words() {
    let mut document = independent_document();
    let before = geometry(&document);
    assert!(document.shift_lyric(word(1), 0.2));
    assert!(document.lyrics()[1].timing_unresolved);
    assert_eq!(
        (document.lyrics()[1].start, document.lyrics()[1].end),
        (1.2, 1.7)
    );
    assert!(document.set_lyric_timing(word(1), 1.1, 1.6));
    assert!(!document.lyrics()[1].timing_unresolved);
    assert_eq!(document.lyrics()[0].start, 0.5);
    assert_eq!(geometry(&document), before);
}

#[test]
fn independent_timing_note_edits_preserve_words_and_global_shift_moves_both() {
    let mut document = independent_document();
    assert!(document.move_note(0, 0.1, 1.9, 64.0));
    assert!(document.resize_note(0, 0.2, 1.8));
    assert_eq!(
        (document.lyrics()[0].start, document.lyrics()[1].end),
        (0.5, 1.5)
    );
    assert!(document.shift_all(0.5));
    assert_eq!(
        (document.lyrics()[0].start, document.lyrics()[1].end),
        (1.0, 2.0)
    );
    assert_eq!(document.notes()[0].start, 0.7);
    assert!(document.shift_all(-20.0));
    assert_eq!(document.notes()[0].start, 0.0);
    assert_eq!(document.lyrics()[0].start, 0.3);
}

#[test]
fn independent_timing_uses_overlapping_pitch_away_from_its_carrier() {
    let mut document = independent_document();
    document.token_mut(word(0)).unwrap().timing = Some(utz::LyricTiming {
        start: 2_100_000,
        duration: 300_000,
    });
    let lyric = &document.lyrics()[0];
    assert_eq!(lyric.note, 0);
    assert!(lyric.guided);
    assert_eq!(lyric.guidance_notes, vec![1]);
}

#[test]
fn independent_timing_note_split_routes_each_word_without_retiming_it() {
    let mut document = independent_document();
    let original = document
        .lyrics()
        .into_iter()
        .map(|lyric| (lyric.text, lyric.start, lyric.end))
        .collect::<Vec<_>>();
    document.split_notes(&selection(&[0]), 1.0);
    assert_eq!(document.lyrics()[0].note, 0);
    assert_eq!(document.lyrics()[1].note, 1);
    assert_eq!(
        document
            .lyrics()
            .into_iter()
            .map(|lyric| (lyric.text, lyric.start, lyric.end))
            .collect::<Vec<_>>(),
        original
    );
    document.merge_notes(&selection(&[0, 1]), None).unwrap();
    assert_eq!(
        document
            .lyrics()
            .into_iter()
            .map(|lyric| (lyric.text, lyric.start, lyric.end))
            .collect::<Vec<_>>(),
        original
    );
    document.to_chart().validate().unwrap();
}

#[test]
fn independent_timing_continuation_does_not_extend_explicit_range() {
    let mut document = independent_document();
    let id = document.token_id_at(word(1)).unwrap();
    document.chart.tracks[0].phrases[0].notes[1].lyrics = vec![LyricToken::Continuation {
        continuation_of: id,
    }];
    let lyrics = document.lyrics();
    assert_eq!(lyrics[1].continuation_notes, vec![1]);
    assert_eq!(lyrics[1].end, 1.5);
    assert_eq!(lyrics[1].guidance_notes, vec![0]);
}

#[test]
fn independent_timing_split_and_merge_keep_other_shared_word_and_note_geometry() {
    let mut document = independent_document();
    let before = geometry(&document);
    let split = document.split_lyrics(&BTreeSet::from([word(0)]), 0.7);
    assert_eq!(split.len(), 2);
    assert_eq!(
        document
            .lyrics()
            .iter()
            .map(|lyric| lyric.text.as_str())
            .collect::<Vec<_>>(),
        ["切", "れ", "に", "た"]
    );
    assert_eq!(geometry(&document), before);
    document.merge_lyrics(&split).unwrap();
    assert_eq!(
        document
            .lyrics()
            .iter()
            .map(|lyric| lyric.text.as_str())
            .collect::<Vec<_>>(),
        ["切れ", "に", "た"]
    );
    assert_eq!(
        (document.lyrics()[0].start, document.lyrics()[0].end),
        (0.5, 1.0)
    );
    assert_eq!(geometry(&document), before);
    document.to_chart().validate().unwrap();
}

#[test]
fn independent_timing_bound_shared_word_split_does_not_overwrite_neighbor() {
    let mut document = independent_document();
    document.token_mut(word(0)).unwrap().timing = None;
    document.token_mut(word(1)).unwrap().timing = None;
    document.set_lyric_text(word(1), "かな");
    let before = geometry(&document);
    document.split_lyrics(&BTreeSet::from([word(1)]), 1.0);
    assert_eq!(
        document
            .lyrics()
            .iter()
            .map(|lyric| lyric.text.as_str())
            .collect::<Vec<_>>(),
        ["切れ", "か", "な", "た"]
    );
    assert_eq!(geometry(&document), before);
}

#[test]
fn independent_timing_syllabizing_marks_internal_estimates_and_preserves_neighbors() {
    let mut document = independent_document();
    document.set_language(Some("ja".into()));
    document.set_lyric_text(word(0), "めざめる");
    let before = geometry(&document);
    let produced = document.syllabize_lyrics(&BTreeSet::from([word(0)]));
    assert_eq!(produced.len(), 4);
    assert!(
        document.lyrics()[..4]
            .iter()
            .all(|lyric| lyric.timing_unresolved)
    );
    assert_eq!(document.lyrics()[4].text, "に");
    assert_eq!(geometry(&document), before);
    document.to_chart().validate().unwrap();
}

#[test]
fn independent_timing_short_syllable_intervals_are_nonzero() {
    let mut document = independent_document();
    document.set_language(Some("en".into()));
    document.set_lyric_text(word(0), "away");
    document.token_mut(word(0)).unwrap().timing = Some(utz::LyricTiming {
        start: 500_000,
        duration: 2,
    });
    let produced = document.syllabize_lyrics(&BTreeSet::from([word(0)]));
    assert_eq!(produced.len(), 2);
    for address in produced {
        assert_eq!(
            document.token_at(address).unwrap().timing.unwrap().duration,
            1
        );
    }
    document.to_chart().validate().unwrap();
}

#[test]
fn independent_timing_copy_preserves_all_words_metadata_and_offsets() {
    let mut document = independent_document();
    let clipboard = document.copy_notes(&selection(&[0]));
    let pasted = document.paste_notes(&clipboard, 4.0);
    let copied = document
        .lyrics()
        .into_iter()
        .filter(|lyric| pasted.contains(&lyric.note))
        .collect::<Vec<_>>();
    assert_eq!(copied.len(), 2);
    assert_eq!(
        (copied[0].text.as_str(), copied[0].start, copied[0].end),
        ("切れ", 4.5, 5.0)
    );
    assert_eq!(
        (copied[1].text.as_str(), copied[1].start, copied[1].end),
        ("に", 5.0, 5.5)
    );
    assert!(copied[1].timing_unresolved);
    let copied_token = document.token_at(copied[1].address).unwrap();
    assert_eq!(copied_token.reading.as_deref(), Some("に"));
    assert_eq!(copied_token.join_before, LyricJoin::None);
    assert_ne!(copied_token.id, document.token_at(word(1)).unwrap().id);
    document.to_chart().validate().unwrap();
}

#[test]
fn independent_timing_copy_remaps_continuations_and_copied_bound_tails_hold() {
    let mut document = document(&[
        (0.0, 1.0, 60, "hold"),
        (1.0, 2.0, 62, ""),
        (2.0, 3.0, 64, ""),
    ]);
    let id = document.token_id_at(word(0)).unwrap();
    for note in &mut document.chart.tracks[0].phrases[0].notes[1..] {
        note.lyrics = vec![LyricToken::Continuation {
            continuation_of: id.clone(),
        }];
    }
    let clipboard = document.copy_notes(&selection(&[1, 2]));
    let pasted = document.paste_notes(&clipboard, 4.0);
    let lyric = document
        .lyrics()
        .into_iter()
        .find(|lyric| pasted.contains(&lyric.note))
        .unwrap();
    assert_eq!((lyric.start, lyric.end), (4.0, 6.0));
    assert_eq!(lyric.continuation_notes.len(), 1);
    assert_ne!(document.token_id_at(lyric.address).unwrap(), id);
    document.to_chart().validate().unwrap();
}

#[test]
fn independent_timing_nonadjacent_bound_merge_does_not_delete_unselected_text() {
    let mut document = document(&[
        (0.0, 1.0, 60, "A"),
        (1.0, 2.0, 62, "B"),
        (2.0, 3.0, 64, "C"),
    ]);
    let survivor = document
        .merge_lyrics(&BTreeSet::from([word(0), word(2)]))
        .unwrap();
    assert_eq!(document.lyric_text(survivor).as_deref(), Some("A C"));
    assert!(document.lyrics().iter().any(|lyric| lyric.text == "B"));
    assert_eq!(document.lyrics().len(), 2);
}

#[test]
fn independent_timing_invalid_note_merge_does_not_mutate_tokens() {
    let mut document = document(&[(0.0, 1.0, 60, "hold")]);
    assert!(
        document
            .merge_notes(&selection(&[0, usize::MAX]), None)
            .is_none()
    );
    assert!(document.token_at(word(0)).unwrap().timing.is_none());
}

#[test]
fn independent_timing_phrase_retokenizing_keeps_text_and_marks_new_scopes() {
    let mut document = independent_document();
    assert!(document.set_phrase_token_text(0, "切/れ/に/た"));
    assert_eq!(document.phrase_text(0), "切れにた");
    assert!(
        document
            .lyrics()
            .iter()
            .all(|lyric| lyric.timing_unresolved)
    );
}

#[test]
fn independent_timing_note_quantize_and_drag_leave_measured_lyrics_in_place() {
    let mut document = independent_document();
    document.move_note(0, 0.13, 1.89, 60.0);
    let before = document
        .lyrics()
        .into_iter()
        .take(2)
        .map(|lyric| (lyric.start, lyric.end))
        .collect::<Vec<_>>();
    assert_eq!(document.quantize_notes(Some(&selection(&[0])), 0.25), 1);
    assert_eq!(
        (document.notes()[0].start, document.notes()[0].end),
        (0.25, 2.0)
    );
    assert_eq!(document.shift_notes(&selection(&[0]), 0.25, 1.0, false), 1);
    assert_eq!(
        document
            .lyrics()
            .into_iter()
            .take(2)
            .map(|lyric| (lyric.start, lyric.end))
            .collect::<Vec<_>>(),
        before
    );
    assert_eq!(document.note_count(), 2);
}

#[test]
fn independent_timing_multiple_word_split_returns_all_created_tokens() {
    let mut document = independent_document();
    document.set_lyric_text(word(1), "かな");
    let produced = document.split_lyrics(&BTreeSet::from([word(0), word(1)]), f64::NAN);
    assert_eq!(produced.len(), 4);
    assert_eq!(
        produced
            .into_iter()
            .map(|address| document.lyric_text(address).unwrap())
            .collect::<Vec<_>>(),
        ["切", "れ", "か", "な"]
    );
}

#[test]
fn independent_timing_tab_visits_words_that_share_a_note_with_a_continuation() {
    let mut document = independent_document();
    let root = document.token_id_at(word(0)).unwrap();
    document.chart.tracks[0].phrases[0].notes[1].lyrics.insert(
        0,
        LyricToken::Continuation {
            continuation_of: root,
        },
    );
    assert_eq!(document.advance_lyric_edit(word(1), true), Some(word(2)));
    assert_eq!(document.advance_lyric_edit(word(2), false), Some(word(1)));
}

#[test]
fn independent_timing_note_split_keeps_unoccupied_halves_saveable() {
    for (start, duration) in [(100_000, 300_000), (1_200_000, 300_000)] {
        let mut document = document(&[(0.0, 2.0, 60, "word")]);
        document.token_mut(word(0)).unwrap().timing = Some(utz::LyricTiming { start, duration });
        document.split_notes(&selection(&[0]), 1.0);
        let lyrics = document.lyrics();
        let written = lyrics
            .iter()
            .filter(|lyric| !lyric.text.is_empty())
            .collect::<Vec<_>>();
        assert_eq!(written.len(), 1);
        assert_eq!(written[0].text, "word");
        assert_eq!(written[0].start, start as f64 / DEFAULT_TIMEBASE as f64);
        assert!(written[0].continuation_notes.is_empty());
        assert_eq!(
            lyrics.iter().filter(|lyric| lyric.text.is_empty()).count(),
            1
        );
        document.to_chart().validate().unwrap();
    }
}

#[test]
fn independent_timing_roll_explicitly_aligns_all_words_to_destination_notes() {
    let mut document = independent_document();
    let before = geometry(&document);
    assert!(document.roll_lyrics(0, true));
    let lyrics = document.lyrics();
    assert_eq!(
        lyrics
            .iter()
            .map(|lyric| lyric.text.as_str())
            .collect::<Vec<_>>(),
        ["た", "切れ", "に"]
    );
    assert_eq!((lyrics[1].start, lyrics[1].end), (2.0, 3.0));
    assert_eq!((lyrics[2].start, lyrics[2].end), (2.0, 3.0));
    assert!(lyrics[2].timing_unresolved);
    assert_eq!(geometry(&document), before);
}
