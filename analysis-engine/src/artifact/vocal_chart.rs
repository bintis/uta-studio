use std::collections::BTreeMap;

use crate::contract::{EngineError, EngineErrorCode, EngineResult};
use crate::fusion::{
    CanonicalNote, CanonicalSingingTrack, CanonicalWordBoundary, TimeRange,
    validate_canonical_singing_track,
};
use crate::quantization::QuantizationReport;
use utz::{
    LyricJoin, LyricTextToken, LyricToken, NoteBonus, NotePitch, NoteScoring, ScoringMode,
    VocalChart, VocalMode, VocalNote, VocalPhrase, VocalTrack, VocalTrackRole,
};

/// Newly emitted Candidate bytes use the strict UTZ VocalChart 0.3 contract.
/// Studio retains a read-only migration path for legacy Engine candidate/v1
/// cache entries, but the Engine no longer emits that wrapper.
pub type CandidateVocalChart = VocalChart;

pub fn finalize_candidate_vocal_chart(
    track: &CanonicalSingingTrack,
    execution_fingerprint: &str,
    quantization: Option<&QuantizationReport>,
) -> EngineResult<CandidateVocalChart> {
    if track.schema_version != 1 || execution_fingerprint.trim().is_empty() {
        return Err(invalid(
            "candidate graph version or execution fingerprint is invalid",
        ));
    }
    validate_canonical_singing_track(track).map_err(invalid)?;
    if quantization.is_some_and(|report| report.validate().is_err()) {
        return Err(invalid("Candidate quantization report is invalid"));
    }

    let mut notes_by_word = BTreeMap::<&str, Vec<&CanonicalNote>>::new();
    for note in &track.notes {
        if let Some(word_id) = note.word_id.as_deref() {
            notes_by_word.entry(word_id).or_default().push(note);
        }
    }

    // Finalization is a projection, not another note decoder. Preserve every
    // selected duration state, including same-pitch reattacks and hard cuts.
    // Place lyric placeholders around those ranges; a real note may cross a word.
    let word_order = track
        .words
        .iter()
        .enumerate()
        .map(|(index, word)| (word.word_id.as_str(), index))
        .collect::<BTreeMap<_, _>>();
    let mut emitted_notes = track
        .notes
        .iter()
        .map(|note| {
            let order = note
                .word_id
                .as_deref()
                .and_then(|id| word_order.get(id))
                .copied()
                .unwrap_or(usize::MAX);
            (note.id.clone(), note.range, order)
        })
        .collect::<Vec<_>>();
    emitted_notes.sort_by_key(|(_, range, _)| (range.start, range.end));

    // Missing or unresolved lyrics must not erase measured melody. These
    // notes remain editable without fabricated text ownership.
    let mut notes = track
        .notes
        .iter()
        .filter(|note| {
            note.word_id
                .as_deref()
                .is_none_or(|id| !word_order.contains_key(id))
        })
        .map(|note| project_note(note, Vec::new()))
        .collect::<Vec<_>>();
    let mut deferred_lyrics = BTreeMap::<String, Vec<(usize, LyricToken)>>::new();
    for (word_index, word) in track.words.iter().enumerate() {
        let candidates = notes_by_word
            .remove(word.word_id.as_str())
            .unwrap_or_default();
        let join_before = lyric_join_between(
            word_index
                .checked_sub(1)
                .map(|previous| track.words[previous].text.as_str()),
            &word.text,
        );
        let spoken_range = if candidates.is_empty() {
            largest_unoccupied_range(word.range, emitted_notes.iter().map(|(_, range, _)| *range))
        } else {
            None
        };
        if candidates.is_empty() && spoken_range.is_none() {
            let target_id = emitted_notes
                .iter()
                .filter_map(|(id, range, _)| {
                    let overlap = range_overlap(word.range, *range);
                    (overlap > 0).then_some((overlap, id))
                })
                .max_by_key(|(overlap, _)| *overlap)
                .map(|(_, id)| id.clone())
                .ok_or_else(|| invalid(format!("word {} has no lyric interval", word.word_id)))?;
            deferred_lyrics.entry(target_id).or_default().push((
                word_index,
                LyricToken::Text(LyricTextToken {
                    id: word.word_id.clone(),
                    text: word.text.clone(),
                    join_before,
                    reading: None,
                    phonemes: None,
                }),
            ));
            continue;
        }
        append_word_notes(
            &mut notes,
            word_index,
            word,
            candidates,
            spoken_range,
            join_before,
        )?;
    }
    for note in &mut notes {
        let Some(mut attached) = deferred_lyrics.remove(&note.id) else {
            continue;
        };
        let owner_order = emitted_notes
            .iter()
            .find_map(|(id, _, order)| (id == &note.id).then_some(*order))
            .unwrap_or(usize::MAX);
        let mut lyrics = std::mem::take(&mut note.lyrics)
            .into_iter()
            .map(|token| (owner_order, token))
            .collect::<Vec<_>>();
        lyrics.append(&mut attached);
        lyrics.sort_by_key(|(order, _)| *order);
        note.lyrics = lyrics.into_iter().map(|(_, token)| token).collect();
    }
    if !deferred_lyrics.is_empty() {
        return Err(invalid("Candidate lyric ownership did not resolve"));
    }
    notes.sort_by_key(|note| (note.start, note.id.clone()));
    if notes.is_empty() {
        return Err(invalid(
            "Candidate VocalChart contains no measured notes or aligned lyric placeholders",
        ));
    }

    let mut chart = VocalChart::new(vec![VocalTrack {
        id: "lead".to_string(),
        role: VocalTrackRole::Lead,
        part: None,
        singer: None,
        scoring_enabled: true,
        phrases: vec![VocalPhrase {
            id: "phrase-1".to_string(),
            notes,
        }],
    }]);
    chart.language = track.transcript.language.clone();
    chart
        .validate()
        .map_err(|error| invalid(error.to_string()))?;
    Ok(chart)
}

fn append_word_notes(
    output: &mut Vec<VocalNote>,
    word_index: usize,
    word: &CanonicalWordBoundary,
    mut candidates: Vec<&CanonicalNote>,
    spoken_range: Option<TimeRange>,
    join_before: LyricJoin,
) -> EngineResult<()> {
    candidates.sort_by_key(|note| (note.range.start, note.range.end, note.id.as_str()));
    let lyric_id = word.word_id.clone();
    if candidates.is_empty() {
        let range = spoken_range.ok_or_else(|| invalid("spoken word has no available range"))?;
        output.push(VocalNote {
            id: format!("unpitched-{word_index}"),
            start: range.start,
            duration: range.end - range.start,
            pitch: None,
            // Missing pitch evidence is not positive evidence of speech.
            vocal_mode: VocalMode::Freestyle,
            bonus: NoteBonus::Normal,
            scoring: NoteScoring {
                mode: ScoringMode::None,
                weight: 0.0,
            },
            lyrics: vec![LyricToken::Text(LyricTextToken {
                id: lyric_id,
                text: word.text.clone(),
                join_before,
                reading: None,
                phonemes: None,
            })],
        });
        return Ok(());
    }

    for note in &candidates {
        if note.range.end <= note.range.start {
            return Err(invalid("Candidate note has an invalid range"));
        }
    }
    for (index, note) in candidates.into_iter().enumerate() {
        let lyrics = if index == 0 {
            vec![LyricToken::Text(LyricTextToken {
                id: lyric_id.clone(),
                text: word.text.clone(),
                join_before,
                reading: None,
                phonemes: None,
            })]
        } else {
            vec![LyricToken::Continuation {
                continuation_of: lyric_id.clone(),
            }]
        };
        output.push(project_note(note, lyrics));
    }
    Ok(())
}

fn project_note(note: &CanonicalNote, lyrics: Vec<LyricToken>) -> VocalNote {
    VocalNote {
        id: note.id.clone(),
        start: note.range.start,
        duration: note.range.end - note.range.start,
        pitch: Some(NotePitch {
            midi: note.midi_note,
            cents: note.center_offset_cents.round().clamp(-99.0, 99.0) as i8,
        }),
        vocal_mode: VocalMode::Pitched,
        bonus: NoteBonus::Normal,
        scoring: NoteScoring {
            mode: ScoringMode::Pitch,
            weight: 1.0,
        },
        lyrics,
    }
}

fn range_overlap(left: TimeRange, right: TimeRange) -> u64 {
    left.end
        .min(right.end)
        .saturating_sub(left.start.max(right.start))
}

fn largest_unoccupied_range(
    range: TimeRange,
    occupied: impl IntoIterator<Item = TimeRange>,
) -> Option<TimeRange> {
    let mut occupied = occupied.into_iter().collect::<Vec<_>>();
    occupied.sort_by_key(|range| (range.start, range.end));
    let mut cursor = range.start;
    let mut largest = None;
    for occupied in occupied {
        if occupied.end <= cursor || occupied.start >= range.end {
            continue;
        }
        let clipped_start = occupied.start.max(range.start);
        if clipped_start > cursor {
            let gap = TimeRange::new(cursor, clipped_start).ok()?;
            if largest
                .is_none_or(|largest: TimeRange| gap.end - gap.start > largest.end - largest.start)
            {
                largest = Some(gap);
            }
        }
        cursor = cursor.max(occupied.end.min(range.end));
        if cursor >= range.end {
            break;
        }
    }
    if cursor < range.end {
        let gap = TimeRange::new(cursor, range.end).ok()?;
        if largest
            .is_none_or(|largest: TimeRange| gap.end - gap.start > largest.end - largest.start)
        {
            largest = Some(gap);
        }
    }
    largest
}

fn lyric_join_between(previous: Option<&str>, current: &str) -> LyricJoin {
    let ascii_word = |text: &str| {
        text.chars()
            .all(|character| character.is_ascii_alphanumeric() || character.is_ascii_punctuation())
    };
    if previous.is_some_and(ascii_word)
        && current
            .chars()
            .all(|character| character.is_ascii_alphanumeric() || character.is_ascii_punctuation())
    {
        LyricJoin::Space
    } else {
        LyricJoin::None
    }
}

fn invalid(message: impl Into<String>) -> EngineError {
    EngineError::new(EngineErrorCode::OutputValidationFailed, message)
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use super::*;
    use crate::artifact::write_json_artifact;
    use crate::fusion::{
        BoundaryCandidateRole, BoundaryEvidenceKind, CanonicalLyrics, CanonicalNoteEvidence,
        EvidenceProvenance, HarmonyMetadata, LyricsAuthority, TechniqueScores, TimeRange,
    };

    fn track() -> CanonicalSingingTrack {
        let range = TimeRange::new(100_001, 500_003).unwrap();
        CanonicalSingingTrack {
            schema_version: 1,
            transcript: CanonicalLyrics {
                text: "sing".to_string(),
                language: Some("en".to_string()),
                authority: LyricsAuthority::CallerCanonical,
                tokens: Vec::new(),
                confidence: None,
                source_experts: vec!["caller".to_string()],
                alternatives: Vec::new(),
            },
            words: vec![CanonicalWordBoundary {
                word_id: "word-1".to_string(),
                text: "sing".to_string(),
                range,
                confidence: None,
                disagreement: None,
                source_experts: vec!["aligner".to_string()],
            }],
            notes: vec![CanonicalNote {
                id: "note-1".to_string(),
                range,
                midi_note: 69,
                center_pitch_hz: 439.95,
                center_offset_cents: -0.2,
                confidence: None,
                uncertain: false,
                alternatives: Vec::new(),
                f0_curve: Vec::new(),
                pitch_bend: Vec::new(),
                techniques: TechniqueScores::default(),
                word_id: Some("word-1".to_string()),
                evidence: CanonicalNoteEvidence {
                    source_experts: vec!["game".to_string(), "rmvpe".to_string()],
                    decision_trace: Default::default(),
                    boundary_source: "game".to_string(),
                    boundary_kind: BoundaryEvidenceKind::Game,
                    boundary_role: BoundaryCandidateRole::Primary,
                    boundary_fractional_midi: Some(68.992),
                    boundary_decision_parameter: Some(0.2),
                    presence_decision_parameter: Some(0.2),
                    boundary_calibrated_confidence: None,
                    target_pitch_source: "game".to_string(),
                    target_pitch_source_local_score: None,
                    target_pitch_calibrated_confidence: None,
                    rmvpe_center_hz: Some(439.95),
                    rmvpe_confidence: Some(0.9),
                    rmvpe_cents_difference: Some(-0.2),
                    rmvpe_voiced_ratio: Some(1.0),
                    rmvpe_pitch_mad_cents: Some(0.2),
                    fcpe_center_hz: None,
                    fcpe_observed_ratio: None,
                    fcpe_pitch_mad_cents: None,
                    fcpe_cents_from_rmvpe: None,
                    fcpe_supports_rmvpe: None,
                    acoustic: None,
                    basic_pitch: None,
                    boundary_alternatives: Vec::new(),
                    technique_evidence: Vec::new(),
                },
            }],
            f0_curve: Vec::new(),
            harmony_metadata: HarmonyMetadata::default(),
            provenance: Vec::<EvidenceProvenance>::new(),
        }
    }

    #[test]
    fn finalization_emits_strict_utz_candidate_without_continuous_geometry() {
        let track = track();
        let chart = finalize_candidate_vocal_chart(&track, &"a".repeat(64), None).unwrap();
        chart.validate().unwrap();
        assert_eq!(chart.format, utz::VOCAL_CHART_FORMAT);
        assert_eq!(chart.tracks[0].phrases[0].notes[0].id, "note-1");
        assert_eq!(chart.tracks[0].phrases[0].notes[0].start, 100_001);
        assert!(
            serde_json::to_value(&chart)
                .unwrap()
                .get("continuous_pitch")
                .is_none()
        );
    }

    #[test]
    fn projection_preserves_same_pitch_reattacks_and_selected_geometry() {
        // A shared word and touching equal MIDI pitches do not authorize a
        // merge. Only the evidence decoder may decide that this is one hold.
        for gap in [0, 10, 20_000] {
            let mut track = track();
            track.notes[0].range = TimeRange::new(100_001, 300_002).unwrap();
            track.notes[0].center_offset_cents = -10.0;
            let mut next = track.notes[0].clone();
            next.id = "reattack".to_string();
            next.range = TimeRange::new(300_002 + gap, 500_003).unwrap();
            next.center_offset_cents = 10.0;
            track.notes.push(next);

            let chart = finalize_candidate_vocal_chart(&track, &"f".repeat(64), None).unwrap();
            let notes = &chart.tracks[0].phrases[0].notes;
            assert_eq!(notes.len(), track.notes.len());
            for (actual, selected) in notes.iter().zip(&track.notes) {
                assert_eq!(actual.id, selected.id);
                assert_eq!(actual.start, selected.range.start);
                assert_eq!(actual.duration, selected.range.end - selected.range.start);
                assert_eq!(
                    actual.pitch.unwrap().cents,
                    selected.center_offset_cents as i8
                );
            }
            chart.validate().unwrap();
        }
    }

    #[test]
    fn continuous_pitch_setting_never_merges_across_a_real_gap_or_a_pitch_change() {
        let mut track = track();
        track.notes[0].range = TimeRange::new(100_001, 300_002).unwrap();
        let mut far_same_pitch = track.notes[0].clone();
        far_same_pitch.id = "note-2".to_string();
        far_same_pitch.range = TimeRange::new(400_000, 500_003).unwrap(); // ~100ms real gap.
        track.notes.push(far_same_pitch);

        let chart = finalize_candidate_vocal_chart(&track, &"g".repeat(64), None).unwrap();
        assert_eq!(chart.tracks[0].phrases[0].notes.len(), 2);
    }

    #[test]
    fn unpitched_aligned_words_are_unscored_not_claimed_as_speech() {
        let mut track = track();
        track.notes.clear();
        let chart = finalize_candidate_vocal_chart(&track, &"c".repeat(64), None).unwrap();
        let note = &chart.tracks[0].phrases[0].notes[0];
        assert_eq!(note.vocal_mode, VocalMode::Freestyle);
        assert_eq!(note.scoring.mode, ScoringMode::None);
        assert_eq!(note.scoring.weight, 0.0);
        assert!(note.pitch.is_none());
        chart.validate().unwrap();
    }

    #[test]
    fn missing_lyric_timing_does_not_drop_selected_melody() {
        let mut track = track();
        track.words.clear();
        track.notes[0].word_id = None;
        let chart =
            finalize_candidate_vocal_chart(&track, "melody-without-alignment", None).unwrap();
        let note = &chart.tracks[0].phrases[0].notes[0];
        assert_eq!(note.id, track.notes[0].id);
        assert_eq!(note.start, track.notes[0].range.start);
        assert_eq!(
            note.duration,
            track.notes[0].range.end - track.notes[0].range.start
        );
        assert!(note.lyrics.is_empty());
        assert_eq!(note.pitch.unwrap().midi, track.notes[0].midi_note);
        chart.validate().unwrap();
    }

    fn cross_word_track(note_end: u64) -> CanonicalSingingTrack {
        let mut track = track();
        track.transcript.text = "sing now".to_string();
        track.words[0].range = TimeRange::new(0, 1_000_000).unwrap();
        track.words.push(CanonicalWordBoundary {
            word_id: "word-2".to_string(),
            text: "now".to_string(),
            range: TimeRange::new(1_000_000, 2_000_000).unwrap(),
            confidence: None,
            disagreement: None,
            source_experts: vec!["aligner".to_string()],
        });
        track.notes[0].range = TimeRange::new(500_000, note_end).unwrap();
        track
    }

    #[test]
    fn spoken_placeholder_uses_gap_after_a_cross_word_pitched_note() {
        let chart =
            finalize_candidate_vocal_chart(&cross_word_track(1_500_000), &"h".repeat(64), None)
                .unwrap();

        chart.validate().unwrap();
        let notes = &chart.tracks[0].phrases[0].notes;
        assert_eq!(notes.len(), 2);
        assert_eq!(notes[0].start + notes[0].duration, notes[1].start);
        assert_eq!(notes[1].start, 1_500_000);
        assert_eq!(notes[1].duration, 500_000);
    }

    #[test]
    fn fully_covered_word_attaches_to_real_note_without_overlap() {
        let mut track = cross_word_track(2_000_000);
        track.notes[0].range = TimeRange::new(0, 2_000_000).unwrap();
        let chart = finalize_candidate_vocal_chart(&track, &"i".repeat(64), None).unwrap();

        chart.validate().unwrap();
        let notes = &chart.tracks[0].phrases[0].notes;
        assert_eq!(notes.len(), 1);
        assert_eq!(notes[0].lyrics.len(), 2);
        assert!(matches!(
            &notes[0].lyrics[1],
            LyricToken::Text(token)
                if token.text == "now" && token.join_before == LyricJoin::Space
        ));
    }

    #[test]
    fn melisma_notes_preserve_note_ids_and_share_one_resolvable_lyric_identity() {
        let mut track = track();
        track.notes[0].range = TimeRange::new(100_001, 300_002).unwrap();
        let mut continuation = track.notes[0].clone();
        continuation.id = "note-2".to_string();
        continuation.range = TimeRange::new(300_002, 500_003).unwrap();
        continuation.midi_note = 71;
        track.notes.push(continuation);

        let chart = finalize_candidate_vocal_chart(&track, &"e".repeat(64), None).unwrap();
        let notes = &chart.tracks[0].phrases[0].notes;
        assert_eq!(
            notes
                .iter()
                .map(|note| note.id.as_str())
                .collect::<Vec<_>>(),
            ["note-1", "note-2"]
        );
        assert!(matches!(
            &notes[0].lyrics[0],
            LyricToken::Text(token) if token.id == "word-1" && token.text == "sing"
        ));
        assert_eq!(
            notes[1].lyrics,
            [LyricToken::Continuation {
                continuation_of: "word-1".to_string()
            }]
        );
        chart.validate().unwrap();
    }

    #[test]
    fn finalized_artifact_is_strict_utz_with_stable_byte_metadata() {
        let root = std::env::temp_dir().join(format!(
            "uta-candidate-chart-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&root).unwrap();
        let chart = finalize_candidate_vocal_chart(&track(), &"d".repeat(64), None).unwrap();
        let reference = write_json_artifact(
            &root,
            Path::new("candidate/vocal-chart.json"),
            utz::VOCAL_CHART_MEDIA_TYPE,
            &chart,
        )
        .unwrap();
        assert!(reference.bytes > 0);
        let decoded: CandidateVocalChart =
            serde_json::from_slice(&std::fs::read(root.join(reference.path)).unwrap()).unwrap();
        decoded.validate().unwrap();
        assert_eq!(decoded, chart);
        std::fs::remove_dir_all(root).unwrap();
    }
}
