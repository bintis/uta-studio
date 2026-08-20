mod tests {
    use super::*;
    use std::collections::BTreeSet;

    use utz::{
        DEFAULT_TIMEBASE, LyricJoin, LyricTextToken, LyricToken, NoteBonus, NotePitch, NoteScoring,
        ScoringMode, VocalChartV1, VocalMode, VocalNote, VocalPhrase, VocalTrack, VocalTrackRole,
    };

    fn document(notes: &[(f64, f64, u8, &str)]) -> EditorDocument {
        let mut phrase = VocalPhrase {
            id: "phrase-1".into(),
            notes: Vec::new(),
        };
        for (index, (start, end, midi, text)) in notes.iter().enumerate() {
            phrase.notes.push(VocalNote {
                id: format!("note-{}", index + 1),
                start: (*start * DEFAULT_TIMEBASE as f64) as u64,
                duration: ((end - start) * DEFAULT_TIMEBASE as f64) as u64,
                pitch: Some(NotePitch {
                    midi: *midi,
                    cents: 0,
                }),
                vocal_mode: VocalMode::Pitched,
                bonus: NoteBonus::Normal,
                scoring: NoteScoring {
                    mode: ScoringMode::Pitch,
                    weight: 1.0,
                },
                lyrics: vec![LyricToken::Text(LyricTextToken {
                    id: format!("lyric-{}", index + 1),
                    text: (*text).into(),
                    join_before: LyricJoin::Space,
                    reading: None,
                    phonemes: None,
                })],
            });
        }
        let mut chart = VocalChartV1::new(vec![VocalTrack {
            id: "lead".into(),
            role: VocalTrackRole::Lead,
            part: None,
            singer: None,
            scoring_enabled: true,
            phrases: vec![phrase],
        }]);
        chart.language = Some("en".into());
        EditorDocument::new(chart)
    }

    fn selection(indices: &[usize]) -> BTreeSet<usize> {
        indices.iter().copied().collect()
    }

    #[test]
    fn move_keeps_the_minimum_duration() {
        let mut document = document(&[(1.0, 2.0, 60, "a")]);
        assert!(document.move_note(0, 3.0, 3.0, 64.0));
        let note = &document.notes()[0];
        assert_eq!(note.start, 3.0);
        assert!((note.end - (3.0 + MIN_NOTE_SECONDS)).abs() < 1e-9);
        assert_eq!(note.midi, 64.0);
    }

    #[test]
    fn insert_places_the_note_in_time_order() {
        let mut document = document(&[(0.0, 1.0, 60, "a"), (2.0, 3.0, 62, "b")]);
        let index = document.insert_note(1.2, 1.6, 65.0).unwrap();
        assert_eq!(index, 1);
        let starts = document
            .notes()
            .iter()
            .map(|note| note.start)
            .collect::<Vec<_>>();
        assert_eq!(starts, [0.0, 1.2, 2.0]);
        document.to_chart().validate().unwrap();
    }

    #[test]
    fn a_syllable_held_across_a_pitch_change_lists_every_note_it_spans() {
        let mut document = document(&[(0.0, 1.0, 60, "hold"), (1.0, 2.0, 62, "gap")]);
        // "gap"'s own note becomes a second continuation of "hold" — the
        // shape a pitch glide partway through one syllable takes.
        let head_id = document.lyrics()[0].address;
        document.delete_lyric(LyricAddress {
            segment: 0,
            word: 1,
        });
        document.extend_lyric_over_note(head_id, 1);
        let lyrics = document.lyrics();
        assert_eq!(lyrics.len(), 1);
        assert_eq!(lyrics[0].note, 0);
        assert_eq!(lyrics[0].continuation_notes, vec![1]);
    }

    #[test]
    fn split_gives_the_tail_a_continuation_and_a_unique_id() {
        let mut document = document(&[(0.0, 1.0, 60, "hold")]);
        let selected = document.split_notes(&selection(&[0]), 0.4);
        assert_eq!(selected, selection(&[0, 1]));
        let notes = document.notes();
        assert_eq!(notes.len(), 2);
        assert_ne!(notes[0].id, notes[1].id);
        assert!((notes[0].end - 0.4).abs() < 1e-9);
        // The lyric lane still shows one syllable, held across both notes.
        let lyrics = document.lyrics();
        assert_eq!(lyrics.len(), 1);
        assert_eq!(lyrics[0].text, "hold");
        assert!((lyrics[0].end - 1.0).abs() < 1e-9);
        document.to_chart().validate().unwrap();
    }

    #[test]
    fn splitting_a_note_with_two_syllables_keeps_both_instead_of_dropping_the_second() {
        let mut document = document(&[(0.0, 1.0, 60, "me")]);
        // Two short syllables sharing one note, with no room for their own
        // (the shape a chart carries when a note's own duration is too
        // short to split further during authoring).
        document.chart.tracks[0].phrases[0].notes[0].lyrics = vec![
            LyricToken::Text(LyricTextToken {
                id: "lyric-1".into(),
                text: "me".into(),
                join_before: LyricJoin::None,
                reading: None,
                phonemes: None,
            }),
            LyricToken::Text(LyricTextToken {
                id: "lyric-2".into(),
                text: "ru".into(),
                join_before: LyricJoin::None,
                reading: None,
                phonemes: None,
            }),
        ];
        document.split_notes(&selection(&[0]), 0.5);
        let lyrics = document.lyrics();
        assert_eq!(
            lyrics
                .iter()
                .map(|lyric| lyric.text.as_str())
                .collect::<Vec<_>>(),
            ["me", "ru"],
            "the second syllable must survive the split, not disappear"
        );
        assert!(lyrics[1].guided, "the second half keeps its own pitch");
        document.to_chart().validate().unwrap();
    }

    #[test]
    fn split_refuses_a_note_that_is_too_short() {
        let mut document = document(&[(0.0, 0.05, 60, "a")]);
        document.split_notes(&selection(&[0]), 0.02);
        assert_eq!(document.note_count(), 1);
    }

    #[test]
    fn unbind_splits_pitch_and_lyric_into_adjacent_notes() {
        let mut document = document(&[(0.0, 1.0, 60, "hi")]);
        let freed = document.unbind_note(0).expect("unbind");
        assert_eq!(
            freed,
            LyricAddress {
                segment: 0,
                word: 0
            }
        );
        let notes = document.notes();
        assert_eq!(notes.len(), 2);
        assert!(notes[0].pitched);
        assert!(notes[0].lyric.is_none());
        assert!(!notes[1].pitched);
        assert_eq!(notes[1].lyric.as_deref(), Some("hi"));
        assert!((notes[0].end - notes[1].start).abs() < 1e-9);
        document.to_chart().validate().unwrap();
    }

    #[test]
    fn unbind_of_a_note_with_two_syllables_defaults_to_the_first_one() {
        let mut document = document(&[(0.0, 1.0, 60, "me")]);
        // Two short syllables sharing one note (same shape as
        // `splitting_a_note_with_two_syllables_keeps_both_instead_of_dropping_the_second`).
        document.chart.tracks[0].phrases[0].notes[0].lyrics = vec![
            LyricToken::Text(LyricTextToken {
                id: "lyric-1".into(),
                text: "me".into(),
                join_before: LyricJoin::None,
                reading: None,
                phonemes: None,
            }),
            LyricToken::Text(LyricTextToken {
                id: "lyric-2".into(),
                text: "ru".into(),
                join_before: LyricJoin::None,
                reading: None,
                phonemes: None,
            }),
        ];
        // Both syllables move onto the same new unpitched note, so the
        // note itself can't distinguish them — only the returned address's
        // `word` can. The plain note-only entry point has no specific
        // token to prefer; it must still resolve to a real, deterministic
        // one (the first) rather than whichever one a stale position
        // search happens to land on.
        let freed = document.unbind_note(0).expect("unbind");
        assert_eq!(
            freed,
            LyricAddress {
                segment: 0,
                word: 0
            }
        );
        assert_eq!(document.lyrics()[freed.word].text, "me");
    }

    #[test]
    fn unbind_selected_keeps_the_specific_syllable_that_was_selected() {
        let mut document = document(&[(0.0, 1.0, 60, "me")]);
        document.chart.tracks[0].phrases[0].notes[0].lyrics = vec![
            LyricToken::Text(LyricTextToken {
                id: "lyric-1".into(),
                text: "me".into(),
                join_before: LyricJoin::None,
                reading: None,
                phonemes: None,
            }),
            LyricToken::Text(LyricTextToken {
                id: "lyric-2".into(),
                text: "ru".into(),
                join_before: LyricJoin::None,
                reading: None,
                phonemes: None,
            }),
        ];
        // The user had the *second* syllable selected, not the note as a
        // whole — unbinding must reselect that same syllable, not just
        // whichever token a note-position lookup happens to land on.
        let selected = LyricAddress {
            segment: 0,
            word: 1,
        };
        let freed = document
            .unbind_selected(Some(selected), None)
            .expect("unbind");
        assert_eq!(document.lyrics()[freed.word].text, "ru");
    }

    #[test]
    fn unbind_selected_by_lyric_alone_keeps_the_text_the_same_as_unbind_by_note() {
        // The common single-syllable case, unbound through the *lyric*
        // selection path (`word: Some(_), note: None`) instead of the note
        // path `unbind_note` already covers -- must behave identically, not
        // just for a note carrying more than one syllable.
        let mut document = document(&[(0.0, 1.0, 60, "hi")]);
        let word = LyricAddress {
            segment: 0,
            word: 0,
        };
        let freed = document.unbind_selected(Some(word), None).expect("unbind");
        let notes = document.notes();
        assert_eq!(notes.len(), 2);
        assert!(notes[0].pitched);
        assert!(notes[0].lyric.is_none());
        assert!(!notes[1].pitched);
        assert_eq!(notes[1].lyric.as_deref(), Some("hi"));
        assert_eq!(document.lyrics()[freed.word].text, "hi");
        document.to_chart().validate().unwrap();
    }

    #[test]
    fn a_failed_unbind_leaves_the_document_untouched() {
        let mut document = document(&[(0.0, 1.0, 60, "a")]);
        document.delete_lyric(LyricAddress {
            segment: 0,
            word: 0,
        });
        let before = document.revision();
        // A note with no lyric can't be unbound — and rejecting it must not
        // itself count as a document change (no take/restore round-trip
        // that bumps the revision on the way to returning `None`).
        assert!(document.unbind_note(0).is_none());
        assert_eq!(document.revision(), before);
    }

    #[test]
    fn dragging_an_unpitched_note_gives_it_pitch_and_promotes_it_to_a_scored_note() {
        let mut document = document(&[(0.0, 1.0, 60, "hi")]);
        let freed = document.unbind_note(0).expect("unbind");
        let index = document.resolve(freed).expect("the freed lyric's note");
        // Before: the note has no pitch, and is a rhythm-only placeholder —
        // dragging it in the pitch canvas should currently do nothing.
        let before = document.notes();
        assert!(!before[index].pitched);
        assert_eq!(
            before[index].kind,
            NoteKind::Rap,
            "reads as unpitched rap/spoken"
        );
        assert!(before[index].placeholder, "unclassified until triaged");

        let (start, end) = (before[index].start, before[index].end);
        assert!(document.move_note(index, start, end, 64.0));

        let after = document.notes();
        assert!(after[index].pitched, "the drag now gives it a real pitch");
        assert_eq!(after[index].midi, 64.0);
        assert_eq!(
            after[index].kind,
            NoteKind::Normal,
            "promoted to a normal scored note instead of staying rhythm-only"
        );
        assert!(
            !after[index].placeholder,
            "triaged once it has a real pitch and mode"
        );
        // A lyric this note owns is guided now that it has a pitch target.
        let lyrics = document.lyrics();
        assert!(
            lyrics
                .iter()
                .any(|lyric| lyric.text == "hi" && lyric.guided)
        );
    }

    #[test]
    fn unbind_refuses_a_note_too_short_to_split() {
        let mut document = document(&[(0.0, 0.04, 60, "a")]);
        assert!(document.unbind_note(0).is_none());
        assert_eq!(document.note_count(), 1);
    }

    #[test]
    fn unbind_refuses_a_note_with_no_lyric() {
        let mut document = document(&[(0.0, 1.0, 60, "a")]);
        document.delete_lyric(LyricAddress {
            segment: 0,
            word: 0,
        });
        assert!(document.unbind_note(0).is_none());
    }

    #[test]
    fn bind_moves_the_lyric_onto_the_target_and_drops_the_placeholder() {
        let mut document = document(&[(0.0, 1.0, 60, "hi")]);
        let freed = document.unbind_note(0).expect("unbind");
        assert_eq!(document.note_count(), 2);
        let bound = document.bind_lyric_to_note(freed, 0).expect("bind");
        assert_eq!(bound, 0);
        assert_eq!(document.note_count(), 1);
        let notes = document.notes();
        assert!(notes[0].pitched);
        assert_eq!(notes[0].lyric.as_deref(), Some("hi"));
        document.to_chart().validate().unwrap();
    }

    #[test]
    fn bind_refuses_a_target_that_already_has_a_lyric() {
        let mut document = document(&[(0.0, 1.0, 60, "hi"), (1.0, 2.0, 62, "there")]);
        let freed = document.unbind_note(0).expect("unbind");
        assert!(document.bind_lyric_to_note(freed, 2).is_none());
        assert_eq!(document.note_count(), 3);
    }

    #[test]
    fn bind_nearest_finds_the_closest_lyric_less_note_in_the_phrase() {
        let mut document = document(&[(0.0, 1.0, 60, "hi"), (5.0, 6.0, 62, "there")]);
        let freed = document.unbind_note(1).expect("unbind");
        // Notes are now: "hi" (0), pitch-only (1), pitch-only-of-split (2)? no:
        // index 0 = "hi" bound note, 1 = pitch half of "there", 2 = lyric half "there".
        let bound = document
            .bind_nearest(Some(freed), None, false)
            .expect("bind nearest");
        assert_eq!(bound, 1);
        assert_eq!(document.note_count(), 2);
    }

    #[test]
    fn bind_nearest_can_keep_the_lyrics_own_timing_instead_of_the_pitch_notes() {
        let mut document = document(&[(0.0, 1.0, 60, "hi"), (5.0, 6.0, 62, "there")]);
        let freed = document.unbind_note(1).expect("unbind");
        let bound = document
            .bind_nearest(Some(freed), None, true)
            .expect("bind nearest");
        let note = &document.notes()[bound];
        assert_eq!(note.start, 5.5);
        assert_eq!(note.end, 6.0);
    }

    #[test]
    fn extending_a_lyric_over_the_next_note_turns_it_into_a_held_continuation() {
        let mut document = document(&[(0.0, 1.0, 60, "hi"), (1.0, 2.0, 62, "there")]);
        // Clear "there"'s own text so its note is a lyric-less pitch target,
        // the shape a pitch glide's second half needs to extend onto.
        document.delete_lyric(LyricAddress {
            segment: 0,
            word: 1,
        });
        let hi = LyricAddress {
            segment: 0,
            word: 0,
        };
        assert_eq!(document.next_extendable_note(hi), Some(1));
        assert!(document.can_extend_lyric_over_note(hi, 1));
        assert!(document.extend_lyric_over_note(hi, 1));
        let lyrics = document.lyrics();
        assert_eq!(lyrics.len(), 1, "the continuation is not its own word");
        assert_eq!(lyrics[0].text, "hi");
        assert_eq!(lyrics[0].start, 0.0);
        assert_eq!(lyrics[0].end, 2.0, "spans through the continuing note");
        // Now the chain's tail is note 1, so a further note it could extend
        // onto would be note 2 — offered by the lyric's own right-click menu
        // without a separate right-click on that note.
        assert_eq!(document.next_extendable_note(hi), None);
    }

    #[test]
    fn unbind_detaches_a_continuation_note_from_its_held_syllable() {
        let mut document = document(&[(0.0, 1.0, 60, "hi"), (1.0, 2.0, 62, "there")]);
        document.delete_lyric(LyricAddress {
            segment: 0,
            word: 1,
        });
        let hi = LyricAddress {
            segment: 0,
            word: 0,
        };
        assert!(document.extend_lyric_over_note(hi, 1));

        let freed = document.unbind_note(1).expect("detach the continuation");
        assert_eq!(
            freed,
            LyricAddress {
                segment: 0,
                word: 1
            },
            "selects the detached note's own new independent copy"
        );
        let notes = document.notes();
        assert_eq!(
            notes.len(),
            2,
            "nothing was split off; the note keeps its pitch"
        );
        assert!(notes[1].pitched);
        // The detach must never make the syllable's text disappear — the
        // detached note gets its own independent copy of it.
        assert_eq!(notes[1].lyric.as_deref(), Some("hi"));
        assert!(!notes[1].continues_lyric);
        assert_eq!(
            notes[0].lyric.as_deref(),
            Some("hi"),
            "the head keeps its own copy too"
        );
        document.to_chart().validate().unwrap();
    }

    #[test]
    fn unbind_refuses_a_continuation_note_that_is_not_the_chains_tail() {
        let mut document = document(&[
            (0.0, 1.0, 60, "hi"),
            (1.0, 2.0, 62, "there"),
            (2.0, 3.0, 64, "you"),
        ]);
        let hi = LyricAddress {
            segment: 0,
            word: 0,
        };
        // Deleted back to front so the second delete's word index isn't
        // thrown off by the first one shifting later words down.
        document.delete_lyric(LyricAddress {
            segment: 0,
            word: 2,
        });
        document.delete_lyric(LyricAddress {
            segment: 0,
            word: 1,
        });
        assert!(document.extend_lyric_over_note(hi, 1));
        assert!(document.extend_lyric_over_note(hi, 2));

        // Note 1 is the middle of the chain (note 2 continues past it);
        // detaching it would strand note 2, so it's refused. The tail,
        // note 2, can be detached.
        assert!(document.unbind_note(1).is_none());
        assert!(document.unbind_note(2).is_some());
        let notes = document.notes();
        assert!(!notes[2].continues_lyric);
        assert!(notes[1].continues_lyric, "the middle note is untouched");
    }

    #[test]
    fn extending_refuses_a_note_that_is_not_immediately_next() {
        let mut document = document(&[
            (0.0, 1.0, 60, "hi"),
            (1.0, 2.0, 62, "mid"),
            (2.0, 3.0, 64, "there"),
        ]);
        document.delete_lyric(LyricAddress {
            segment: 0,
            word: 2,
        });
        let hi = LyricAddress {
            segment: 0,
            word: 0,
        };
        // "mid" still holds its own syllable in between, so "hi" cannot
        // reach past it to claim the third note.
        assert!(!document.can_extend_lyric_over_note(hi, 2));
        assert!(!document.extend_lyric_over_note(hi, 2));
    }

    #[test]
    fn merge_spans_the_selection_and_keeps_every_syllable() {
        let mut document = document(&[(0.0, 1.0, 60, "one"), (1.0, 2.0, 62, "two")]);
        let index = document.merge_notes(&selection(&[0, 1]), None).unwrap();
        assert_eq!(index, 0);
        let notes = document.notes();
        assert_eq!(notes.len(), 1);
        assert_eq!(notes[0].start, 0.0);
        assert_eq!(notes[0].end, 2.0);
        let lyrics = document.lyrics();
        assert_eq!(
            lyrics
                .iter()
                .map(|lyric| lyric.text.as_str())
                .collect::<Vec<_>>(),
            ["one", "two"]
        );
        document.to_chart().validate().unwrap();
    }

    #[test]
    fn quantize_snaps_both_edges_and_holds_the_minimum() {
        let mut document = document(&[(0.31, 0.44, 60, "a")]);
        assert_eq!(document.quantize_notes(None, 0.25), 1);
        let note = &document.notes()[0];
        assert!((note.start - 0.25).abs() < 1e-9);
        assert!((note.end - 0.5).abs() < 1e-9);
    }

    #[test]
    fn shift_never_moves_a_selection_before_zero() {
        let mut document = document(&[(0.2, 1.0, 60, "a"), (1.5, 2.0, 62, "b")]);
        document.shift_notes(&selection(&[0, 1]), -5.0, 0.0, false);
        let notes = document.notes();
        assert_eq!(notes[0].start, 0.0);
        assert!((notes[1].start - 1.3).abs() < 1e-9);
    }

    #[test]
    fn shift_transposes_within_the_midi_range() {
        let mut document = document(&[(0.0, 1.0, 126, "a")]);
        document.shift_notes(&selection(&[0]), 0.0, 4.0, false);
        assert_eq!(document.notes()[0].midi, 127.0);
    }

    #[test]
    fn cycling_a_kind_converges_a_mixed_selection() {
        let mut document = document(&[(0.0, 1.0, 60, "a"), (1.0, 2.0, 62, "b")]);
        document.set_note_kind(&selection(&[1]), NoteKind::Rap);
        document.cycle_note_kinds(&selection(&[0, 1]));
        let kinds = document
            .notes()
            .iter()
            .map(|note| note.kind)
            .collect::<Vec<_>>();
        assert_eq!(kinds, [NoteKind::Golden, NoteKind::Golden]);
    }

    #[test]
    fn a_rhythm_kind_keeps_the_chart_valid_without_a_pitch_target() {
        let mut document = document(&[(0.0, 1.0, 60, "a")]);
        document.set_note_kind(&selection(&[0]), NoteKind::Freestyle);
        document.to_chart().validate().unwrap();
    }

    #[test]
    fn copy_and_paste_reproduce_the_selection_at_the_playhead() {
        let mut document = document(&[(0.0, 0.5, 60, "a"), (1.0, 1.5, 64, "b")]);
        let clipboard = document.copy_notes(&selection(&[0, 1]));
        let pasted = document.paste_notes(&clipboard, 4.0);
        assert_eq!(pasted.len(), 2);
        let notes = document.notes();
        assert_eq!(notes.len(), 4);
        assert!((notes[2].start - 4.0).abs() < 1e-9);
        assert!((notes[3].start - 5.0).abs() < 1e-9);
        assert_eq!(notes[3].midi, 64.0);
        document.to_chart().validate().unwrap();
    }

    #[test]
    fn deleting_a_note_drops_the_continuation_that_referenced_it() {
        let mut document = document(&[(0.0, 1.0, 60, "hold")]);
        document.split_notes(&selection(&[0]), 0.5);
        // Remove the head; the tail's continuation can no longer resolve.
        document.remove_notes(&selection(&[0]));
        assert_eq!(document.note_count(), 1);
        document.to_chart().validate().unwrap();
    }

    #[test]
    fn editing_a_syllable_clears_its_stale_pronunciation() {
        let mut document = document(&[(0.0, 1.0, 60, "a")]);
        let address = LyricAddress {
            segment: 0,
            word: 0,
        };
        if let Some(token) = document.token_mut(address) {
            token.reading = Some("えー".into());
        }
        assert!(document.set_lyric_text(address, "b"));
        assert!(document.token_mut(address).unwrap().reading.is_none());
    }

    #[test]
    fn deleting_a_syllable_keeps_its_note() {
        let mut document = document(&[(0.0, 1.0, 60, "a"), (1.0, 2.0, 62, "b")]);
        let (deleted, next) = document.delete_lyric(LyricAddress {
            segment: 0,
            word: 0,
        });
        assert!(deleted);
        assert_eq!(document.note_count(), 2);
        assert_eq!(document.lyrics().len(), 1);
        assert_eq!(next.unwrap().word, 0);
        document.to_chart().validate().unwrap();
    }

    #[test]
    fn splitting_a_syllable_splits_its_note_and_its_text() {
        let mut document = document(&[(0.0, 1.0, 60, "hello")]);
        let mut addresses = BTreeSet::new();
        addresses.insert(LyricAddress {
            segment: 0,
            word: 0,
        });
        let result = document.split_lyrics(&addresses, 0.5);
        assert_eq!(result.len(), 2);
        let lyrics = document.lyrics();
        assert_eq!(
            lyrics
                .iter()
                .map(|lyric| lyric.text.as_str())
                .collect::<Vec<_>>(),
            ["he", "llo"]
        );
        assert_eq!(document.note_count(), 2);
        document.to_chart().validate().unwrap();
    }

    #[test]
    fn merging_syllables_joins_text_and_notes() {
        let mut document = document(&[(0.0, 1.0, 60, "he"), (1.0, 2.0, 62, "llo")]);
        let mut addresses = BTreeSet::new();
        addresses.insert(LyricAddress {
            segment: 0,
            word: 0,
        });
        addresses.insert(LyricAddress {
            segment: 0,
            word: 1,
        });
        let merged = document.merge_lyrics(&addresses).unwrap();
        assert_eq!(merged.word, 0);
        assert_eq!(document.note_count(), 1);
        assert_eq!(document.lyrics()[0].text, "he llo");
        document.to_chart().validate().unwrap();
    }

    #[test]
    fn a_compact_language_merges_without_a_space() {
        let mut document = document(&[(0.0, 1.0, 60, "歌"), (1.0, 2.0, 62, "詞")]);
        document.set_language(Some("ja".into()));
        let mut addresses = BTreeSet::new();
        addresses.insert(LyricAddress {
            segment: 0,
            word: 0,
        });
        addresses.insert(LyricAddress {
            segment: 0,
            word: 1,
        });
        document.merge_lyrics(&addresses).unwrap();
        assert_eq!(document.lyrics()[0].text, "歌詞");
    }

    #[test]
    fn splitting_a_phrase_creates_a_second_lyric_line() {
        let mut document = document(&[
            (0.0, 1.0, 60, "a"),
            (1.0, 2.0, 62, "b"),
            (2.0, 3.0, 64, "c"),
        ]);
        let next = document
            .split_phrase(LyricAddress {
                segment: 0,
                word: 0,
            })
            .unwrap();
        assert_eq!(next.segment, 1);
        assert_eq!(document.phrase_count(), 2);
        assert_eq!(document.phrase_text(0), "a");
        assert_eq!(document.phrase_text(1), "b c");
        document.to_chart().validate().unwrap();
    }

    #[test]
    fn merging_phrases_restores_one_line() {
        let mut document = document(&[(0.0, 1.0, 60, "a"), (1.0, 2.0, 62, "b")]);
        document
            .split_phrase(LyricAddress {
                segment: 0,
                word: 0,
            })
            .unwrap();
        assert_eq!(document.phrase_count(), 2);
        let address = document
            .merge_phrase_with_next(LyricAddress {
                segment: 0,
                word: 0,
            })
            .unwrap();
        assert_eq!(address.segment, 0);
        assert_eq!(document.phrase_count(), 1);
        assert_eq!(document.phrase_text(0), "a b");
        document.to_chart().validate().unwrap();
    }

    #[test]
    fn inserting_a_lyric_creates_an_unpitched_note() {
        let mut document = document(&[(0.0, 1.0, 60, "a")]);
        let address = document.insert_lyric(None, 5.0).unwrap();
        assert_eq!(document.lyric_text(address).as_deref(), Some("New lyric"));
        let lyric = document
            .lyrics()
            .into_iter()
            .find(|lyric| lyric.address == address)
            .unwrap();
        assert!(!lyric.guided, "a fresh lyric has no pitch guidance yet");
        document.to_chart().validate().unwrap();
    }

    #[test]
    fn add_lyric_to_note_gives_a_lyric_less_note_its_own_text_directly() {
        let mut document = document(&[(0.0, 1.0, 60, "hi")]);
        // `unbind_note` leaves note 0 pitched with no lyric of its own.
        document.unbind_note(0).expect("unbind");
        assert_eq!(document.note_count(), 2, "no new note should be created");

        let address = document.add_lyric_to_note(0).expect("add lyric");
        assert_eq!(document.lyric_text(address).as_deref(), Some("New lyric"));
        assert_eq!(
            document.note_count(),
            2,
            "the text lands on the existing note"
        );
        let notes = document.notes();
        assert_eq!(notes[0].lyric.as_deref(), Some("New lyric"));
        assert!(notes[0].pitched, "the note keeps the pitch it already had");
        document.to_chart().validate().unwrap();

        assert!(
            document.add_lyric_to_note(0).is_none(),
            "refuses a note that already has its own lyric"
        );
    }

    #[test]
    fn revision_advances_only_on_a_real_change() {
        let mut document = document(&[(0.0, 1.0, 60, "a")]);
        let before = document.revision();
        assert!(!document.set_lyric_text(
            LyricAddress {
                segment: 0,
                word: 0
            },
            "a"
        ));
        assert_eq!(document.revision(), before);
        assert!(document.set_lyric_text(
            LyricAddress {
                segment: 0,
                word: 0
            },
            "b"
        ));
        assert!(document.revision() > before);
    }

    #[test]
    fn save_normalizes_note_and_phrase_order() {
        let mut document = document(&[(0.0, 1.0, 60, "a"), (2.0, 3.0, 62, "b")]);
        // Drag the second note before the first; the stored order stays put so
        // the pointer keeps its grip, but saving reorders.
        document.move_note(1, 0.0, 0.5, 62.0);
        let chart = document.to_chart();
        let starts = chart.tracks[0].phrases[0]
            .notes
            .iter()
            .map(|note| note.start)
            .collect::<Vec<_>>();
        assert!(starts.windows(2).all(|pair| pair[0] <= pair[1]));
    }

    #[test]
    fn repair_separates_overlaps_and_makes_the_chart_valid() {
        let mut document = document(&[(0.0, 1.0, 60, "a"), (0.5, 1.5, 62, "b")]);
        assert!(document.to_chart().validate().is_err());
        assert!(document.repair() > 0);
        let notes = document.notes();
        assert!(notes[1].start >= notes[0].end);
        document.to_chart().validate().unwrap();
    }

    #[test]
    fn repair_lifts_a_note_to_the_minimum_duration() {
        let mut document = document(&[(0.0, 0.001, 60, "a")]);
        assert_eq!(document.repair(), 1);
        let note = &document.notes()[0];
        assert!(note.end - note.start >= MIN_NOTE_SECONDS - 1e-9);
    }

    #[test]
    fn repair_leaves_a_clean_chart_untouched() {
        let mut document = document(&[(0.0, 1.0, 60, "a"), (1.0, 2.0, 62, "b")]);
        assert_eq!(document.repair(), 0);
        assert_eq!(document.revision(), 0);
    }

    #[test]
    fn shift_all_moves_every_track() {
        let mut document = document(&[(1.0, 2.0, 60, "a")]);
        assert!(document.shift_all(0.5));
        assert!((document.notes()[0].start - 1.5).abs() < 1e-9);
        assert!(document.shift_all(-10.0));
        assert_eq!(document.notes()[0].start, 0.0);
    }

    #[test]
    fn rolling_moves_a_line_one_note_along_and_back() {
        let mut document = document(&[
            (0.0, 1.0, 60, "one"),
            (1.0, 2.0, 62, "two"),
            (2.0, 3.0, 64, "three"),
        ]);
        assert!(document.roll_lyrics(0, true));
        assert_eq!(
            document
                .notes()
                .iter()
                .map(|note| note.lyric.clone().unwrap_or_default())
                .collect::<Vec<_>>(),
            ["three", "one", "two"]
        );
        assert!(document.roll_lyrics(0, false));
        assert_eq!(
            document
                .notes()
                .iter()
                .map(|note| note.lyric.clone().unwrap_or_default())
                .collect::<Vec<_>>(),
            ["one", "two", "three"]
        );
    }

    #[test]
    fn rolling_needs_something_to_roll() {
        let mut document = document(&[(0.0, 1.0, 60, "only")]);
        assert!(!document.roll_lyrics(0, true));
        assert_eq!(document.revision(), 0);
    }

    #[test]
    fn a_phrase_reads_and_writes_as_its_own_syllables() {
        let mut document = document(&[(0.0, 1.0, 60, "won"), (1.0, 2.0, 62, "der")]);
        // The second syllable joins without a space, so it reads as one word
        // divided by a slash.
        document.set_lyric_text(
            LyricAddress {
                segment: 0,
                word: 1,
            },
            "der",
        );
        let text = document.phrase_token_text(0);
        assert!(text == "won der" || text == "won/der", "{text}");

        assert!(document.set_phrase_token_text(0, "sun/day morn/ing"));
        assert_eq!(document.phrase_token_text(0), "sun/day morn/ing");
        // Four syllables over two notes: the surplus rides on the last note
        // rather than being dropped.
        assert_eq!(document.note_count(), 2);
        assert_eq!(document.lyrics().len(), 4);
        document.to_chart().validate().expect("valid chart");
    }

    #[test]
    fn retyping_a_shorter_line_clears_the_notes_it_no_longer_covers() {
        let mut document = document(&[
            (0.0, 1.0, 60, "one"),
            (1.0, 2.0, 62, "two"),
            (2.0, 3.0, 64, "three"),
        ]);
        assert!(document.set_phrase_token_text(0, "just this"));
        assert_eq!(
            document
                .notes()
                .iter()
                .map(|note| note.lyric.clone().unwrap_or_default())
                .collect::<Vec<_>>(),
            ["just", "this", ""]
        );
        document.to_chart().validate().expect("valid chart");
    }

    #[test]
    fn writing_the_same_text_back_changes_nothing() {
        let mut document = document(&[(0.0, 1.0, 60, "one"), (1.0, 2.0, 62, "two")]);
        let text = document.phrase_token_text(0);
        assert!(!document.set_phrase_token_text(0, &text));
        assert_eq!(document.revision(), 0);
    }

    #[test]
    fn syllabizing_gives_every_syllable_its_own_note() {
        let mut document = document(&[(0.0, 2.0, 60, "wonder"), (2.0, 3.0, 62, "love")]);
        let produced = document.syllabize_lyrics(&BTreeSet::from([LyricAddress {
            segment: 0,
            word: 0,
        }]));
        let notes = document.notes();
        assert_eq!(notes.len(), 3);
        assert_eq!(
            notes
                .iter()
                .filter_map(|note| note.lyric.clone())
                .collect::<Vec<_>>(),
            ["won", "der", "love"]
        );
        assert_eq!(produced.len(), 2);
        // The word keeps its span: the pieces divide it, they do not extend it.
        assert!((notes[0].start - 0.0).abs() < 1e-9);
        assert!((notes[1].end - 2.0).abs() < 1e-9);
        assert!(notes[0].end <= notes[1].start + 1e-9);
        document.to_chart().validate().expect("valid chart");
    }

    #[test]
    fn a_one_syllable_word_is_left_exactly_as_it_was() {
        let mut document = document(&[(0.0, 1.0, 60, "love")]);
        assert!(
            document
                .syllabize_lyrics(&BTreeSet::from([LyricAddress {
                    segment: 0,
                    word: 0
                }]))
                .is_empty()
        );
        assert_eq!(document.note_count(), 1);
        assert_eq!(document.revision(), 0);
    }

    #[test]
    fn a_note_too_short_to_divide_is_not_syllabized() {
        let mut document = document(&[(0.0, 0.04, 60, "wonder")]);
        document.syllabize_lyrics(&BTreeSet::from([LyricAddress {
            segment: 0,
            word: 0,
        }]));
        assert_eq!(document.note_count(), 1);
    }

    #[test]
    fn a_held_word_still_resolves_after_it_is_syllabized() {
        let mut document = document(&[(0.0, 2.0, 60, "wonder"), (2.0, 3.0, 60, "x")]);
        let held = match &document.chart().tracks[0].phrases[0].notes[0].lyrics[0] {
            LyricToken::Text(token) => token.id.clone(),
            LyricToken::Continuation { .. } => panic!("text token"),
        };
        document.chart.tracks[0].phrases[0].notes[1].lyrics = vec![LyricToken::Continuation {
            continuation_of: held,
        }];
        document.syllabize_lyrics(&BTreeSet::from([LyricAddress {
            segment: 0,
            word: 0,
        }]));
        // The hold now follows the last syllable of the word, not the first.
        assert!(document.unresolved_continuations(0).is_empty());
        let notes = document.notes();
        assert!(notes.last().unwrap().continues_lyric);
        document.to_chart().validate().expect("valid chart");
    }

    #[test]
    fn japanese_words_syllabize_by_mora_and_keep_their_reading() {
        let mut document = document(&[(0.0, 3.0, 60, "きょうは")]);
        document.set_language(Some("ja".into()));
        document.syllabize_lyrics(&BTreeSet::from([LyricAddress {
            segment: 0,
            word: 0,
        }]));
        let notes = document.notes();
        assert_eq!(
            notes
                .iter()
                .filter_map(|note| note.lyric.clone())
                .collect::<Vec<_>>(),
            ["きょ", "う", "は"]
        );
        let readings = document.chart().tracks[0].phrases[0]
            .notes
            .iter()
            .filter_map(|note| match note.lyrics.first() {
                Some(LyricToken::Text(token)) => token.reading.clone(),
                _ => None,
            })
            .collect::<Vec<_>>();
        assert_eq!(readings, ["きょ", "う", "は"]);
    }

    #[test]
    fn note_selection_reports_phrase_and_unit_range() {
        let document = document(&[(1.0, 1.5, 60, "a"), (2.0, 2.5, 62, "b")]);
        assert_eq!(document.phrase_index_for_note(0), Some(0));
        assert_eq!(
            document.note_range_units(&BTreeSet::from([0, 1])),
            Some((1_000_000, 2_500_000))
        );
    }

    #[test]
    fn a_new_track_becomes_active_and_starts_empty() {
        let mut document = document(&[(0.0, 1.0, 60, "a")]);
        assert_eq!(document.track_count(), 1);
        let index = document.add_track(TrackRole::Lead);
        assert_eq!(index, 1);
        assert_eq!(document.active_track_index(), 1);
        assert!(document.notes().is_empty());
        let tracks = document.tracks();
        // A second lead track is automatically a duet partner: both lead
        // tracks pick up contiguous UltraStar-style part numbers.
        assert_eq!(tracks[1].role, TrackRole::Lead);
        assert_eq!(tracks[0].part, Some(1));
        assert_eq!(tracks[1].part, Some(2));
        assert!(tracks[1].scoring_enabled);
        // The lead track keeps its material and its coverage.
        assert_eq!(tracks[0].note_count, 1);
        assert!((tracks[0].sung_seconds - 1.0).abs() < 1e-9);
    }

    #[test]
    fn the_last_track_cannot_be_removed() {
        let mut document = document(&[(0.0, 1.0, 60, "a")]);
        assert!(!document.remove_track(0));
        document.add_track(TrackRole::Lead);
        assert!(document.remove_track(1));
        assert_eq!(document.track_count(), 1);
        assert_eq!(document.active_track_index(), 0);
    }

    #[test]
    fn moving_a_selection_to_another_track_legalizes_an_overlap() {
        let mut document = document(&[(0.0, 1.0, 60, "a"), (0.5, 1.5, 62, "b")]);
        assert!(document.problems().blocks_saving());
        document.add_track(TrackRole::Lead);
        document.set_active_track(0);
        assert_eq!(document.move_notes_to_track(&selection(&[1]), 1), 1);
        assert_eq!(document.notes().len(), 1);
        assert_eq!(document.track_notes(1).len(), 1);
        // Both tracks are now single voiced, so the chart saves.
        assert!(!document.problems().blocks_saving());
        document.to_chart().validate().expect("valid chart");
    }

    #[test]
    fn a_held_syllable_follows_its_notes_to_the_new_track() {
        let mut document = document(&[(0.0, 1.0, 60, "held"), (1.0, 2.0, 60, "next")]);
        // Turn the second note into a continuation of the first syllable.
        let held = document.chart().tracks[0].phrases[0].notes[0].lyrics[0].clone();
        let LyricToken::Text(held) = held else {
            panic!("text token");
        };
        document.chart.tracks[0].phrases[0].notes[1].lyrics = vec![LyricToken::Continuation {
            continuation_of: held.id.clone(),
        }];
        document.add_track(TrackRole::Harmony);
        document.set_active_track(0);
        assert_eq!(document.move_notes_to_track(&selection(&[0]), 1), 2);
        assert!(document.notes().is_empty());
        assert_eq!(document.track_notes(1).len(), 2);
        document.set_active_track(1);
        assert!(document.unresolved_continuations(1).is_empty());
    }

    #[test]
    fn an_orphaned_continuation_is_dropped_rather_than_moved_alone() {
        let mut document = document(&[(0.0, 1.0, 60, "held"), (1.0, 2.0, 60, "next")]);
        let held = document.chart().tracks[0].phrases[0].notes[0].lyrics[0].clone();
        let LyricToken::Text(held) = held else {
            panic!("text token");
        };
        document.chart.tracks[0].phrases[0].notes[1].lyrics = vec![LyricToken::Continuation {
            continuation_of: held.id,
        }];
        document.add_track(TrackRole::Harmony);
        document.set_active_track(0);
        // Moving only the holding note would leave a reference across tracks.
        assert_eq!(document.move_notes_to_track(&selection(&[1]), 1), 1);
        assert!(document.track_notes(1)[0].lyric.is_none());
        assert!(document.unresolved_continuations(1).is_empty());
        assert!(document.unresolved_continuations(0).is_empty());
    }

    #[test]
    fn problems_cover_tracks_the_user_is_not_editing() {
        let mut document = document(&[(0.0, 1.0, 60, "a")]);
        document.add_track(TrackRole::Lead);
        document.insert_note(0.0, 1.0, 62.0);
        document.insert_note(0.5, 1.5, 64.0);
        document.set_active_track(0);
        let report = document.problems();
        assert!(report.blocks_saving());
        assert!(report.problems.iter().any(|problem| problem.track == 1
            && problem.kind == super::super::ProblemKind::OverlappingNotes));
    }

    #[test]
    fn an_empty_track_is_not_persisted() {
        let mut document = document(&[(0.0, 1.0, 60, "a")]);
        document.add_track(TrackRole::Backing);
        let chart = document.to_chart();
        assert_eq!(chart.tracks.len(), 1);
        chart.validate().expect("valid chart");
    }

    #[test]
    fn generated_transpose_cases_are_reversible_without_midi_clamping() {
        for midi in (24_u8..=103).step_by(7) {
            for semitones in -12_i8..=12 {
                let mut document = document(&[(0.0, 1.0, midi, "tone")]);
                let original = document.notes()[0].midi;
                document.shift_notes(&selection(&[0]), 0.0, f64::from(semitones), false);
                document.shift_notes(&selection(&[0]), 0.0, -f64::from(semitones), false);
                assert_eq!(document.notes()[0].midi, original, "midi={midi}, shift={semitones}");
                document.to_chart().validate().unwrap();
            }
        }
    }

    #[test]
    fn generated_split_merge_cases_preserve_the_time_span() {
        for duration_tenths in 2_u32..=40 {
            let duration = f64::from(duration_tenths) / 10.0;
            for split_percent in [20_u32, 35, 50, 65, 80] {
                let mut document = document(&[(1.0, 1.0 + duration, 60, "held")]);
                let before = document.notes()[0].clone();
                let split_at = before.start + duration * f64::from(split_percent) / 100.0;
                let selected = document.split_notes(&selection(&[0]), split_at);
                assert_eq!(selected.len(), 2);
                document.merge_notes(&selected, None).unwrap();
                let after = &document.notes()[0];
                assert!((after.start - before.start).abs() < 1e-9);
                assert!((after.end - before.end).abs() < 1e-9);
                document.to_chart().validate().unwrap();
            }
        }
    }

    #[test]
    fn generated_repairs_remove_every_same_track_overlap() {
        for overlap_hundredths in 1_u32..=95 {
            let overlap = f64::from(overlap_hundredths) / 100.0;
            let mut document = document(&[
                (0.0, 1.0, 60, "a"),
                (1.0 - overlap, 2.0, 62, "b"),
                (2.0 - overlap, 3.0, 64, "c"),
            ]);
            document.repair();
            let notes = document.notes();
            assert!(notes.windows(2).all(|pair| pair[0].end <= pair[1].start));
            document.to_chart().validate().unwrap();
        }
    }
}
