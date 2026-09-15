use super::lyric_projection::{LyricDisplayGroup, lyric_display_groups, lyric_display_scopes};
use std::collections::BTreeMap;

use crate::contract::{EngineError, EngineErrorCode, EngineResult};
use crate::fusion::{
    CanonicalNote, CanonicalSingingTrack, TimeRange, validate_canonical_singing_track,
};
use crate::quantization::QuantizationReport;
use uta_studio_chart::{
    LyricJoin, LyricTextToken, LyricTiming, LyricToken, NoteBonus, NotePitch, NoteScoring,
    ScoringMode, VocalChart, VocalMode, VocalNote, VocalPhrase, VocalTrack, VocalTrackRole,
};

/// Newly emitted Candidate bytes use Studio's internal chart model, whose
/// standard projection is the UTZ VocalChart 0.3 contract.
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

    let projection_notes = notes_with_lyric_ownership(track);
    let lyric_groups = lyric_display_groups(track);
    let lyric_scopes = lyric_display_scopes(&lyric_groups);
    let mut notes_by_word = BTreeMap::<&str, Vec<&CanonicalNote>>::new();
    for note in &projection_notes {
        if let Some(word_id) = note.word_id.as_deref() {
            notes_by_word.entry(word_id).or_default().push(note);
        }
    }

    // Preserve the selected melody, including same-pitch reattacks and hard
    // cuts. Lyrics carry their own measured or unresolved audition times;
    // projection never creates a melodic boundary from a word timestamp.
    let word_order = lyric_groups
        .iter()
        .enumerate()
        .map(|(index, group)| (group.boundary.word_id.as_str(), index))
        .collect::<BTreeMap<_, _>>();
    let mut emitted_notes = projection_notes
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
    let mut lyric_ids = lyric_groups
        .iter()
        .map(|group| group.boundary.word_id.clone())
        .collect::<std::collections::BTreeSet<_>>();
    let mut unassigned_index = 0usize;
    let mut notes = projection_notes
        .iter()
        .filter(|note| {
            note.word_id
                .as_deref()
                .is_none_or(|id| !word_order.contains_key(id))
        })
        .map(|note| {
            // Note identifiers already own their full UTZ byte budget.
            // Allocate an independent chart-local identity rather than
            // composing another identifier from an arbitrarily long note ID.
            let id = loop {
                let candidate = format!("unassigned-{unassigned_index}");
                unassigned_index += 1;
                if lyric_ids.insert(candidate.clone()) {
                    break candidate;
                }
            };
            project_note(
                note,
                vec![LyricToken::Text(LyricTextToken {
                    timing_unresolved: false,
                    timing: None,
                    id,
                    text: String::new(),
                    join_before: LyricJoin::None,
                    reading: None,
                    phonemes: None,
                })],
            )
        })
        .collect::<Vec<_>>();
    let mut deferred_lyrics = BTreeMap::<String, Vec<(usize, LyricToken)>>::new();
    // Word ownership cannot describe every text token on a shared note. Keep
    // the actual preceding text anchor, including deferred measured words.
    let mut previous_lyric_anchor = None::<String>;
    for (word_index, group) in lyric_groups.iter().enumerate() {
        let word = &group.boundary;
        let candidates = notes_by_word
            .remove(word.word_id.as_str())
            .unwrap_or_default();
        let join_before = lyric_join_between(
            word_index
                .checked_sub(1)
                .map(|previous| lyric_groups[previous].boundary.text.as_str()),
            &word.text,
        );
        let overlap_target = emitted_notes
            .iter()
            .filter_map(|(id, range, _)| {
                let overlap = range_overlap(word.range, *range);
                (overlap > 0).then_some((overlap, id))
            })
            .max_by_key(|(overlap, _)| *overlap)
            .map(|(_, id)| id.clone());
        let spoken_range = if candidates.is_empty() && overlap_target.is_none() {
            let scope = lyric_scopes[word_index];
            (scope.end > scope.start)
                .then(|| {
                    largest_unoccupied_range(
                        scope,
                        emitted_notes.iter().map(|(_, range, _)| *range),
                    )
                })
                .flatten()
        } else {
            None
        };
        if candidates.is_empty() && spoken_range.is_none() {
            let neighbour = (!group.measured)
                .then(|| {
                    previous_lyric_anchor.clone().or_else(|| {
                        emitted_notes
                            .iter()
                            .filter(|(_, _, order)| *order > word_index && *order != usize::MAX)
                            .min_by_key(|(_, range, order)| (*order, range.start))
                            .map(|(id, _, _)| id.clone())
                    })
                })
                .flatten();
            let target_id = neighbour
                .or(overlap_target)
                .ok_or_else(|| invalid(format!("word {} has no lyric interval", word.word_id)))?;
            previous_lyric_anchor = Some(target_id.clone());
            deferred_lyrics.entry(target_id).or_default().push((
                word_index,
                LyricToken::Text(LyricTextToken {
                    timing_unresolved: group.timing_unresolved,
                    timing: Some(lyric_timing(lyric_scopes[word_index])),
                    id: word.word_id.clone(),
                    text: word.text.clone(),
                    join_before,
                    reading: None,
                    phonemes: None,
                }),
            ));
            continue;
        }
        let first_emitted = notes.len();
        append_word_notes(
            &mut notes,
            word_index,
            group,
            candidates,
            spoken_range,
            join_before,
            lyric_timing(lyric_scopes[word_index]),
        )?;
        previous_lyric_anchor = Some(notes[first_emitted].id.clone());
        if let Some(range) = spoken_range {
            emitted_notes.push((notes[first_emitted].id.clone(), range, word_index));
        }
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
        phrases: phrases_by_lyric_line(track, &lyric_groups, notes),
    }]);
    chart.language = track.transcript.language.clone();
    chart
        .validate()
        .map_err(|error| invalid(error.to_string()))?;
    Ok(chart)
}

/// Reconsider stale word ownership using temporal overlap only. This does not
/// move, split, merge or replace any selected note. Multiple words can bind one
/// note because text tokens have independent timing.
fn notes_with_lyric_ownership(track: &CanonicalSingingTrack) -> Vec<CanonicalNote> {
    track
        .notes
        .iter()
        .map(|note| {
            let mut projected = note.clone();
            if note.word_id.is_some() {
                projected.word_id = track
                    .words
                    .iter()
                    .filter_map(|word| {
                        let overlap = range_overlap(word.range, note.range);
                        (overlap > 0).then_some((
                            overlap,
                            note.word_id.as_deref() == Some(word.word_id.as_str()),
                            word,
                        ))
                    })
                    .max_by_key(|(overlap, owner, _)| (*overlap, *owner))
                    .map(|(_, _, word)| word.word_id.clone());
            }
            projected
        })
        .collect()
}

/// Groups the finalized notes into one UTZ phrase per canonical lyric line.
///
/// A note follows the line of the word it carries (a continuation follows its
/// word). Notes without line structure -- unassigned melody, or words from a
/// transcript without line structure -- stay with the phrase in progress, and
/// notes before the first line-owned note join that line. Lines are emitted
/// in transcript order: a word measured earlier than the notes of a preceding
/// line stays in the open phrase instead of reordering phrases, so the result
/// always satisfies the UTZ phrase ordering rule. Without any line-owned word
/// the track remains a single phrase.
fn phrases_by_lyric_line(
    track: &CanonicalSingingTrack,
    groups: &[LyricDisplayGroup],
    notes: Vec<VocalNote>,
) -> Vec<VocalPhrase> {
    let lines = track
        .transcript
        .tokens
        .iter()
        .filter_map(|token| token.id.as_deref())
        .collect::<Vec<_>>();
    let line_order = lines
        .iter()
        .enumerate()
        .map(|(order, id)| (*id, order))
        .collect::<BTreeMap<_, _>>();
    let word_order = groups
        .iter()
        .map(|group| &group.boundary)
        .filter_map(|word| {
            let order = *line_order.get(word.line_id.as_deref()?)?;
            Some((word.word_id.as_str(), order))
        })
        .collect::<BTreeMap<_, _>>();
    let note_order = |note: &VocalNote| {
        note.lyrics.iter().find_map(|token| match token {
            LyricToken::Text(token) => word_order.get(token.id.as_str()).copied(),
            LyricToken::Continuation { continuation_of } => {
                word_order.get(continuation_of.as_str()).copied()
            }
        })
    };
    let Some(first) = notes.iter().find_map(note_order) else {
        return vec![VocalPhrase {
            id: "phrase-1".to_string(),
            notes,
        }];
    };
    let mut current = first;
    let mut phrases = Vec::<VocalPhrase>::new();
    for note in notes {
        let order = note_order(&note).unwrap_or(current);
        if phrases.is_empty() || order > current {
            current = order.max(current);
            phrases.push(VocalPhrase {
                id: phrase_id(current, lines[current]),
                notes: Vec::new(),
            });
        }
        phrases
            .last_mut()
            .expect("a phrase is open")
            .notes
            .push(note);
    }
    phrases
}

/// Phrase ids stay unique through the line order and readable through the
/// caller's line id, trimmed to the UTZ id budget.
fn phrase_id(order: usize, line_id: &str) -> String {
    let prefix = format!("phrase-{}-", order + 1);
    let budget = uta_studio_chart::MAX_ID_BYTES.saturating_sub(prefix.len());
    let mut end = line_id.len().min(budget);
    while !line_id.is_char_boundary(end) {
        end -= 1;
    }
    format!("{prefix}{}", &line_id[..end])
}

fn append_word_notes(
    output: &mut Vec<VocalNote>,
    word_index: usize,
    group: &LyricDisplayGroup,
    mut candidates: Vec<&CanonicalNote>,
    spoken_range: Option<TimeRange>,
    join_before: LyricJoin,
    timing: LyricTiming,
) -> EngineResult<()> {
    let word = &group.boundary;
    let timing_unresolved = group.timing_unresolved;
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
                timing_unresolved,
                timing: Some(timing),
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
                timing_unresolved,
                timing: Some(timing),
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

fn lyric_timing(range: TimeRange) -> LyricTiming {
    LyricTiming {
        start: range.start,
        duration: range.end - range.start,
    }
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

pub(super) fn lyric_join_between(previous: Option<&str>, current: &str) -> LyricJoin {
    let ascii_word = |text: &str| {
        text.chars().all(|character| {
            character.is_ascii_alphanumeric()
                || character.is_ascii_punctuation()
                || character.is_whitespace()
        })
    };
    if previous.is_some_and(ascii_word)
        && current.chars().all(|character| {
            character.is_ascii_alphanumeric()
                || character.is_ascii_punctuation()
                || character.is_whitespace()
        })
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
        CanonicalWordBoundary, EvidenceProvenance, HarmonyMetadata, LyricsAuthority,
        TechniqueScores, TimeRange,
    };

    fn track() -> CanonicalSingingTrack {
        let range = TimeRange::new(100_001, 500_003).unwrap();
        CanonicalSingingTrack {
            schema_version: 1,
            lyric_units: Vec::new(),
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
                line_id: None,
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
        assert_eq!(chart.format, uta_studio_chart::VOCAL_CHART_FORMAT);
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
        assert!(matches!(&note.lyrics[0], LyricToken::Text(token) if token.text.is_empty()));
        assert_eq!(note.pitch.unwrap().midi, track.notes[0].midi_note);
        chart.validate().unwrap();
    }

    #[test]
    fn unowned_notes_keep_full_length_note_ids_and_have_short_distinct_lyric_ids() {
        let mut track = track();
        track.words.clear();
        track.notes[0].word_id = None;
        track.notes[0].id = "n".repeat(64);
        let mut following = track.notes[0].clone();
        following.id = "m".repeat(64);
        following.range = TimeRange::new(600_000, 1_000_000).unwrap();
        track.notes.push(following);

        let chart = finalize_candidate_vocal_chart(&track, "long-note-identifiers", None).unwrap();
        chart.validate().unwrap();
        let notes = chart
            .tracks
            .iter()
            .flat_map(|track| &track.phrases)
            .flat_map(|phrase| &phrase.notes)
            .collect::<Vec<_>>();
        assert_eq!(
            notes
                .iter()
                .map(|note| note.id.as_str())
                .collect::<Vec<_>>(),
            track
                .notes
                .iter()
                .map(|note| note.id.as_str())
                .collect::<Vec<_>>()
        );
        let lyric_ids = notes
            .iter()
            .flat_map(|note| &note.lyrics)
            .filter_map(|token| match token {
                LyricToken::Text(text) => Some(text.id.as_str()),
                LyricToken::Continuation { .. } => None,
            })
            .collect::<std::collections::BTreeSet<_>>();
        assert_eq!(lyric_ids.len(), 2);
        assert!(lyric_ids.iter().all(|id| id.len() <= 64));
        assert_eq!(
            chart,
            finalize_candidate_vocal_chart(&track, "long-note-identifiers", None).unwrap(),
        );
    }

    #[test]
    fn unowned_lyric_ids_do_not_collide_with_original_caller_word_ids() {
        let mut track = track();
        track.words[0].word_id = "unassigned-0".into();
        track.notes[0].word_id = Some("unassigned-0".into());
        let mut following = track.notes[0].clone();
        following.id = "unowned-note".into();
        following.range = TimeRange::new(600_000, 1_000_000).unwrap();
        following.word_id = None;
        track.notes.push(following);
        let chart = finalize_candidate_vocal_chart(&track, "caller-lyric-identity", None).unwrap();
        chart.validate().unwrap();
        let text_tokens = chart
            .tracks
            .iter()
            .flat_map(|track| &track.phrases)
            .flat_map(|phrase| &phrase.notes)
            .flat_map(|note| &note.lyrics)
            .filter_map(|token| match token {
                LyricToken::Text(text) => Some(text),
                LyricToken::Continuation { .. } => None,
            })
            .collect::<Vec<_>>();
        assert!(
            text_tokens
                .iter()
                .any(|token| token.id == "unassigned-0" && token.text == "sing")
        );
        assert_eq!(text_tokens.len(), 2);
        assert_ne!(text_tokens[0].id, text_tokens[1].id);
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
            line_id: None,
        });
        track.notes[0].range = TimeRange::new(500_000, note_end).unwrap();
        track
    }

    #[test]
    fn unresolved_word_between_touching_measured_words_keeps_timeline_order() {
        let mut track = cross_word_track(2_000_000);
        track.transcript.text = "一二三".into();
        track.words[0].text = "一".into();
        track.words[1].text = "三".into();
        track.lyric_units = [
            ("word-1", "一", Some(track.words[0].range)),
            ("missing-middle", "二", None),
            ("word-2", "三", Some(track.words[1].range)),
        ]
        .into_iter()
        .map(
            |(id, text, measured_range)| crate::fusion::CanonicalLyricUnit {
                id: id.into(),
                text: text.into(),
                line_id: None,
                measured_range,
                audition_range: TimeRange::new(0, 2_000_000).unwrap(),
            },
        )
        .collect();
        let original = track.clone();
        let chart = finalize_candidate_vocal_chart(&track, "touching-words", None).unwrap();
        chart.validate().unwrap();
        let notes = &chart.tracks[0].phrases[0].notes;
        assert_eq!(notes.len(), 1);
        assert_eq!(notes[0].start, track.notes[0].range.start);
        assert_eq!(
            notes[0].duration,
            track.notes[0].range.end - track.notes[0].range.start
        );
        let tokens = notes
            .iter()
            .flat_map(|note| &note.lyrics)
            .filter_map(|token| {
                if let LyricToken::Text(text) = token {
                    Some(text)
                } else {
                    None
                }
            })
            .collect::<Vec<_>>();
        assert_eq!(
            tokens
                .iter()
                .map(|token| token.text.as_str())
                .collect::<String>(),
            "一二三"
        );
        assert!(
            tokens
                .windows(2)
                .all(|pair| pair[0].timing.unwrap().start <= pair[1].timing.unwrap().start)
        );
        assert_eq!(
            tokens[1].timing,
            Some(LyricTiming {
                start: 1_000_000,
                duration: 0
            })
        );
        assert!(tokens[1].timing_unresolved);
        assert!(!tokens[0].timing_unresolved && !tokens[2].timing_unresolved);
        assert_eq!(track, original);
        let decoded: VocalChart =
            serde_json::from_slice(&serde_json::to_vec(&chart).unwrap()).unwrap();
        assert_eq!(decoded, chart);
    }

    #[test]
    fn cross_word_pitched_note_keeps_independent_word_times_without_a_new_attack() {
        let track = cross_word_track(1_500_000);
        let chart = finalize_candidate_vocal_chart(&track, "held-word-times", None).unwrap();
        let notes = &chart.tracks[0].phrases[0].notes;
        assert_eq!(notes.len(), 1);
        assert_eq!(notes[0].id, track.notes[0].id);
        assert_eq!(notes[0].start, 500_000);
        assert_eq!(notes[0].duration, 1_000_000);
        let tokens = notes[0]
            .lyrics
            .iter()
            .filter_map(|token| match token {
                LyricToken::Text(token) => Some(token),
                _ => None,
            })
            .collect::<Vec<_>>();
        assert_eq!(tokens.len(), 2);
        for (token, word) in tokens.iter().zip(&track.words) {
            assert_eq!(token.text, word.text);
            assert_eq!(
                token.timing,
                Some(LyricTiming {
                    start: word.range.start,
                    duration: word.range.end - word.range.start,
                })
            );
        }
        let decoded: VocalChart =
            serde_json::from_slice(&serde_json::to_vec(&chart).unwrap()).unwrap();
        decoded.validate().unwrap();
        assert_eq!(decoded, chart);
    }

    #[test]
    fn measured_japanese_words_have_distinct_times_on_one_selected_note() {
        let mut track = cross_word_track(2_000_000);
        track.transcript.text = "切に".into();
        track.words[0].text = "切".into();
        track.words[1].text = "に".into();
        track.notes[0].range = TimeRange::new(0, 2_000_000).unwrap();
        let original = track.clone();
        let chart = finalize_candidate_vocal_chart(&track, "sequential-japanese", None).unwrap();
        let notes = &chart.tracks[0].phrases[0].notes;
        assert_eq!(notes.len(), 1);
        assert_eq!(notes[0].duration, 2_000_000);
        assert_eq!(notes[0].pitch.unwrap().midi, track.notes[0].midi_note);
        assert_eq!(notes[0].lyrics.len(), 2);
        for (token, word) in notes[0].lyrics.iter().zip(&track.words) {
            assert!(matches!(token, LyricToken::Text(token)
            if token.text == word.text && !token.timing_unresolved
            && token.timing == Some(LyricTiming {
                start: word.range.start, duration: word.range.end - word.range.start,
            })));
        }
        assert_eq!(track, original);
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
            uta_studio_chart::VOCAL_CHART_MEDIA_TYPE,
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

    fn lined_track() -> CanonicalSingingTrack {
        let mut track = cross_word_track(1_000_000);
        track.transcript.tokens = ["lrc-0", "lrc-1"]
            .into_iter()
            .zip(["sing", "now"])
            .map(|(id, text)| crate::fusion::TranscriptTokenEvidence {
                id: Some(id.to_string()),
                text: text.to_string(),
                range: None,
                confidence: None,
            })
            .collect();
        track.words[0].line_id = Some("lrc-0".to_string());
        track.words[1].line_id = Some("lrc-1".to_string());
        let mut second = track.notes[0].clone();
        second.id = "note-2".to_string();
        second.range = TimeRange::new(1_300_000, 1_800_000).unwrap();
        second.word_id = Some("word-2".to_string());
        track.notes.push(second);
        track
    }

    #[test]
    fn phrases_follow_caller_lyric_lines_and_keep_unowned_melody_in_the_open_phrase() {
        let mut track = lined_track();
        let mut stray = track.notes[0].clone();
        stray.id = "stray".to_string();
        stray.range = TimeRange::new(1_000_000, 1_200_000).unwrap();
        stray.word_id = None;
        track.notes.insert(1, stray);

        let chart = finalize_candidate_vocal_chart(&track, &"j".repeat(64), None).unwrap();
        chart.validate().unwrap();
        let phrases = &chart.tracks[0].phrases;
        assert_eq!(
            phrases
                .iter()
                .map(|phrase| phrase.id.as_str())
                .collect::<Vec<_>>(),
            ["phrase-1-lrc-0", "phrase-2-lrc-1"]
        );
        let ids = |index: usize| {
            phrases[index]
                .notes
                .iter()
                .map(|note| note.id.as_str())
                .collect::<Vec<_>>()
        };
        assert_eq!(ids(0), ["note-1", "stray"]);
        assert_eq!(ids(1), ["note-2"]);
    }

    #[test]
    fn a_word_measured_before_an_earlier_line_never_reorders_phrases() {
        // The aligner measured line 2's word before line 1's word: canonical
        // words stay time-ordered, so the second line's word comes first.
        let mut track = lined_track();
        track.notes[1].range = TimeRange::new(100_000, 400_000).unwrap();
        track.notes.swap(0, 1);
        track.words[1].range = TimeRange::new(0, 450_000).unwrap();
        track.words[0].range = TimeRange::new(450_000, 1_000_000).unwrap();
        track.words.swap(0, 1);

        let chart = finalize_candidate_vocal_chart(&track, &"k".repeat(64), None).unwrap();
        chart.validate().unwrap();
        let phrases = &chart.tracks[0].phrases;
        assert_eq!(phrases.len(), 1);
        assert_eq!(phrases[0].id, "phrase-2-lrc-1");
        assert_eq!(phrases[0].notes.len(), 2);
    }

    #[test]
    fn words_without_line_structure_stay_in_one_phrase_with_a_bounded_id() {
        let chart = finalize_candidate_vocal_chart(&track(), &"l".repeat(64), None).unwrap();
        assert_eq!(chart.tracks[0].phrases.len(), 1);
        assert_eq!(chart.tracks[0].phrases[0].id, "phrase-1");
        let long = phrase_id(41, &"line".repeat(40));
        assert!(long.len() <= uta_studio_chart::MAX_ID_BYTES);
        assert!(long.starts_with("phrase-42-line"));
    }

    #[test]
    fn held_note_crossing_lines_keeps_geometry_text_and_independent_times() {
        for owner in ["word-1", "word-2"] {
            let mut track = lined_track();
            track.notes.truncate(1);
            track.notes[0].range = TimeRange::new(500_000, 1_800_000).unwrap();
            track.notes[0].word_id = Some(owner.into());
            let original = track.clone();
            let chart = finalize_candidate_vocal_chart(&track, "held-line-note", None).unwrap();
            let notes = chart.tracks[0]
                .phrases
                .iter()
                .flat_map(|phrase| &phrase.notes)
                .collect::<Vec<_>>();
            assert_eq!(notes.len(), 1);
            assert_eq!(notes[0].id, track.notes[0].id);
            assert_eq!(notes[0].start, 500_000);
            assert_eq!(notes[0].duration, 1_300_000);
            assert_eq!(notes[0].pitch.unwrap().midi, track.notes[0].midi_note);
            let tokens = notes[0]
                .lyrics
                .iter()
                .filter_map(|token| match token {
                    LyricToken::Text(token) => Some(token),
                    _ => None,
                })
                .collect::<Vec<_>>();
            assert_eq!(
                tokens
                    .iter()
                    .map(|token| token.text.as_str())
                    .collect::<Vec<_>>(),
                ["sing", "now"]
            );
            assert_eq!(tokens[0].timing.unwrap().start, 0);
            assert_eq!(tokens[1].timing.unwrap().start, 1_000_000);
            // A note belongs to one phrase. Original line IDs and caller scopes
            // stay canonical rather than manufacturing a sentence attack.
            assert_eq!(track, original);
            chart.validate().unwrap();
        }
    }

    #[test]
    fn quantized_sentence_start_reassigns_stale_ownership_without_merging_lines() {
        let mut track = lined_track();
        track.words[0].range = TimeRange::new(950_000, 1_000_000).unwrap();
        track.words[1].range = TimeRange::new(1_000_000, 1_010_000).unwrap();
        track.notes.truncate(1);
        track.notes[0].range = TimeRange::new(980_000, 1_060_000).unwrap();
        let original = track.clone();
        let assert_lines = |chart: &VocalChart| {
            chart.validate().unwrap();
            let lyrics = chart.tracks[0]
                .phrases
                .iter()
                .flat_map(|phrase| &phrase.notes)
                .flat_map(|note| &note.lyrics)
                .filter_map(|token| match token {
                    LyricToken::Text(token) => {
                        Some((token.id.as_str(), token.text.as_str(), token.timing))
                    }
                    _ => None,
                })
                .collect::<Vec<_>>();
            assert_eq!(
                lyrics,
                [
                    (
                        "word-1",
                        "sing",
                        Some(LyricTiming {
                            start: 950_000,
                            duration: 50_000
                        })
                    ),
                    (
                        "word-2",
                        "now",
                        Some(LyricTiming {
                            start: 1_000_000,
                            duration: 10_000
                        })
                    ),
                ]
            );
        };
        let raw = finalize_candidate_vocal_chart(&track, "raw-sentence-owner", None).unwrap();
        assert_lines(&raw);
        assert_eq!(track, original);

        let context = crate::contract::MusicalContext {
            bpm: Some(120.0),
            key: None,
            time_signature: None,
            quantization_grid: Some(crate::contract::QuantizationGrid::Sixteenth),
            authority: crate::contract::ContextAuthority::Hint,
        };
        let report = crate::quantization::quantize_singing_track(
            &mut track,
            &context,
            TimeRange::new(0, 2_000_000).unwrap(),
            &[],
        )
        .unwrap();
        assert_eq!(
            track.notes[0].range,
            TimeRange::new(1_000_000, 1_125_000).unwrap()
        );
        assert_eq!(track.notes[0].word_id.as_deref(), Some("word-1"));
        let quantized = track.clone();
        let mut expected_note = track.notes[0].clone();
        expected_note.word_id = Some("word-2".to_string());
        assert_eq!(notes_with_lyric_ownership(&track), [expected_note]);

        let chart =
            finalize_candidate_vocal_chart(&track, "quantized-sentence-owner", Some(&report))
                .unwrap();
        assert_lines(&chart);
        let phrases = &chart.tracks[0].phrases;
        assert_eq!(phrases[0].notes[0].id, "unpitched-0");
        assert!(phrases[0].notes[0].pitch.is_none());
        assert_eq!(phrases[0].notes[0].start, track.words[0].range.start);
        assert_eq!(phrases[0].notes[0].duration, 50_000);
        let pitched = &phrases[1].notes[0];
        assert_eq!(pitched.id, track.notes[0].id);
        assert_eq!(pitched.start, track.notes[0].range.start);
        assert_eq!(pitched.duration, 125_000);
        assert_eq!(pitched.pitch, raw.tracks[0].phrases[0].notes[0].pitch);
        assert_eq!(track, quantized);

        // Analysis references must describe the final placeholder/pitched IDs,
        // not the pre-quantization note or sentence-fragment list.
        let boundaries = crate::fusion::BoundaryEvidenceSet {
            source_expert: "game".to_string(),
            kind: BoundaryEvidenceKind::Game,
            model_hash: None,
            runtime_identity: None,
            segments: vec![crate::fusion::BoundarySegmentEvidence {
                range: original.notes[0].range,
                fractional_midi: Some(f32::from(original.notes[0].midi_note)),
                boundary_decision_parameter: None,
                presence_decision_parameter: None,
            }],
        };
        let fusion = crate::fusion::fuse_singing_evidence(
            &original.words,
            &boundaries,
            "rmvpe",
            &[],
            None,
            &[],
            None,
            None,
            None,
        )
        .unwrap();
        let selected = fusion
            .candidates
            .iter()
            .find(|candidate| candidate.range == original.notes[0].range)
            .unwrap();
        let decision = crate::contract::FusionDecisionProvenance::Algorithm {
            selector: crate::contract::HSMM_VITERBI_SELECTOR.to_string(),
            selector_version: crate::fingerprint::HSMM_VERSION.to_string(),
            candidate_set_digest: "sentence-owner-fixture".to_string(),
            selected_candidate_ids: vec![selected.id.clone()],
            reuse_policy: crate::contract::AnalysisReusePolicy::Deterministic,
        };
        let analysis = crate::artifact::SingingAnalysis::new(
            &original,
            &chart,
            fusion.candidates,
            fusion.hard_boundaries,
            Vec::new(),
            "quantized-sentence-owner",
            &decision,
        )
        .unwrap();
        assert_eq!(analysis.chart_references.track_id, chart.tracks[0].id);
        assert_eq!(
            analysis.chart_references.phrase_ids,
            [phrases[0].id.clone(), phrases[1].id.clone()]
        );
        assert_eq!(
            analysis.chart_references.note_ids,
            ["note-1", "unpitched-0"]
        );
        assert_eq!(
            analysis.chart_references.lyric_token_ids,
            ["word-1", "word-2"]
        );
    }

    #[test]
    fn a_later_dominant_word_changes_ownership_without_a_note_cut() {
        let mut track = cross_word_track(1_900_000);
        track.notes[0].range.start = 900_000;
        let notes = notes_with_lyric_ownership(&track);
        assert_eq!(notes.len(), 1);
        assert_eq!(notes[0].word_id.as_deref(), Some("word-2"));
        assert_eq!(notes[0].range, track.notes[0].range);
        let chart = finalize_candidate_vocal_chart(&track, "dominant-word", None).unwrap();
        assert_eq!(chart.tracks[0].phrases[0].notes[0].lyrics.len(), 2);
    }

    #[test]
    fn unsplit_unassigned_note_stays_unassigned_despite_measured_word_overlap() {
        let mut track = lined_track();
        track.notes.truncate(1);
        track.notes[0].range = TimeRange::new(1_000_000, 1_200_000).unwrap();
        track.notes[0].word_id = None;
        assert!(range_overlap(track.notes[0].range, track.words[1].range) > 0);
        assert_eq!(notes_with_lyric_ownership(&track), track.notes);
    }

    #[test]
    fn an_aligned_word_edge_does_not_assign_previously_unowned_melody() {
        let mut track = lined_track();
        let mut stray = track.notes[0].clone();
        stray.id = "stray".to_string();
        stray.range = TimeRange::new(1_000_000, 1_200_000).unwrap();
        stray.word_id = None;
        track.notes.insert(1, stray);
        assert_eq!(notes_with_lyric_ownership(&track), track.notes);
    }

    #[test]
    fn unsplit_note_without_any_word_overlap_does_not_keep_a_stale_lyric_owner() {
        let mut track = lined_track();
        track.notes.truncate(1);
        track.notes[0].range = TimeRange::new(2_100_000, 2_500_000).unwrap();
        let mut expected = track.notes[0].clone();
        expected.word_id = None;
        assert_eq!(notes_with_lyric_ownership(&track), [expected]);
        let chart = finalize_candidate_vocal_chart(&track, "unowned-sentence-note", None).unwrap();
        let note = chart.tracks[0]
            .phrases
            .iter()
            .flat_map(|phrase| &phrase.notes)
            .find(|note| note.id == track.notes[0].id)
            .unwrap();
        assert!(matches!(&note.lyrics[0], LyricToken::Text(token) if token.text.is_empty()));
        assert_eq!(note.start, track.notes[0].range.start);
        assert_eq!(note.duration, 400_000);
        assert_eq!(note.pitch.unwrap().midi, track.notes[0].midi_note);
    }

    #[test]
    fn line_search_scope_does_not_split_the_first_words_real_note_leadin() {
        let mut track = lined_track();
        track.words[0].range = TimeRange::new(500_000, 800_000).unwrap();
        track.words[1].range = TimeRange::new(1_000_000, 1_200_000).unwrap();
        track.notes.remove(0);
        track.notes[0].range = TimeRange::new(830_000, 1_200_000).unwrap();
        let original = track.notes[0].clone();
        assert_eq!(
            notes_with_lyric_ownership(&track).as_slice(),
            std::slice::from_ref(&original)
        );
        let chart = finalize_candidate_vocal_chart(&track, "line-leadin", None).unwrap();
        let pitched = chart.tracks[0]
            .phrases
            .iter()
            .flat_map(|phrase| &phrase.notes)
            .filter(|note| note.pitch.is_some())
            .collect::<Vec<_>>();
        assert_eq!(pitched.len(), 1);
        assert_eq!(pitched[0].start, original.range.start);
        assert_eq!(
            pitched[0].duration,
            original.range.end - original.range.start
        );
        assert!(matches!(&pitched[0].lyrics[0], LyricToken::Text(token) if token.text == "now"));
    }

    #[test]
    fn unresolved_imported_characters_stay_visible_in_their_original_order() {
        let mut track = cross_word_track(900_000);
        track.transcript.text = "目覚める".to_string();
        track.words[0].text = "目".to_string();
        track.words[0].range = TimeRange::new(100_000, 400_000).unwrap();
        track.words[1].text = "め".to_string();
        track.words[1].range = TimeRange::new(600_000, 900_000).unwrap();
        track.notes[0].range = track.words[0].range;
        let mut later = track.notes[0].clone();
        later.id = "later-note".to_string();
        later.word_id = Some(track.words[1].word_id.clone());
        later.range = track.words[1].range;
        track.notes.push(later);
        track.lyric_units = [
            ("word-1", "目", Some(track.words[0].range)),
            ("missing-inner", "覚", None),
            ("word-2", "め", Some(track.words[1].range)),
            ("missing-tail", "る", None),
        ]
        .into_iter()
        .map(
            |(id, text, measured_range)| crate::fusion::CanonicalLyricUnit {
                id: id.to_string(),
                text: text.to_string(),
                line_id: None,
                measured_range,
                audition_range: TimeRange::new(0, 1_000_000).unwrap(),
            },
        )
        .collect();
        let original = track.clone();
        let chart = finalize_candidate_vocal_chart(&track, "missing-kana", None).unwrap();
        let notes = &chart.tracks[0].phrases[0].notes;
        assert_eq!(notes.len(), 2);
        let text = notes
            .iter()
            .flat_map(|note| &note.lyrics)
            .filter_map(|token| match token {
                LyricToken::Text(token) => {
                    assert_eq!(token.timing_unresolved, token.id.starts_with("missing-"));
                    assert!(token.timing.is_some());
                    Some(token.text.as_str())
                }
                LyricToken::Continuation { .. } => None,
            })
            .collect::<Vec<_>>();
        assert_eq!(text, ["目", "覚", "め", "る"]);
        assert_eq!(text.concat(), track.transcript.text);
        let tokens = notes
            .iter()
            .flat_map(|note| &note.lyrics)
            .filter_map(|token| match token {
                LyricToken::Text(token) => Some(token),
                _ => None,
            })
            .collect::<Vec<_>>();
        assert_eq!(
            tokens[0].timing,
            Some(LyricTiming {
                start: 100_000,
                duration: 300_000
            })
        );
        assert_eq!(
            tokens[1].timing,
            Some(LyricTiming {
                start: 400_000,
                duration: 200_000
            })
        );
        assert_eq!(
            tokens[2].timing,
            Some(LyricTiming {
                start: 600_000,
                duration: 300_000
            })
        );
        assert_eq!(
            tokens[3].timing,
            Some(LyricTiming {
                start: 900_000,
                duration: 100_000
            })
        );
        for (note, measured) in notes.iter().zip(&track.notes) {
            assert_eq!(note.start, measured.range.start);
            assert_eq!(note.duration, measured.range.end - measured.range.start);
        }
        assert_eq!(track, original);
    }

    #[test]
    fn entirely_unresolved_line_keeps_text_as_an_unscored_audition_placeholder() {
        let mut track = track();
        track.transcript.text = "切に".to_string();
        track.words.clear();
        track.notes.clear();
        let scope = TimeRange::new(2_000_000, 4_000_000).unwrap();
        track.lyric_units = ["切", "に"]
            .into_iter()
            .enumerate()
            .map(|(index, text)| crate::fusion::CanonicalLyricUnit {
                id: format!("unresolved-{index}"),
                text: text.to_string(),
                line_id: None,
                measured_range: None,
                audition_range: scope,
            })
            .collect();
        let chart = finalize_candidate_vocal_chart(&track, "unresolved-line", None).unwrap();
        let notes = &chart.tracks[0].phrases[0].notes;
        assert_eq!(notes.len(), 1);
        assert_eq!(notes[0].start, scope.start);
        assert_eq!(notes[0].duration, scope.end - scope.start);
        assert_eq!(notes[0].scoring.mode, ScoringMode::None);
        assert_eq!(notes[0].scoring.weight, 0.0);
        assert!(notes[0].pitch.is_none());
        assert!(matches!(&notes[0].lyrics[0], LyricToken::Text(token)
            if token.text == "切に" && token.timing_unresolved));
    }

    #[test]
    fn unresolved_line_scope_never_moves_its_text_after_the_next_measured_line() {
        for held in [false, true] {
            let mut track = cross_word_track(2_000_000);
            track.transcript.text = "A\nB\nC".to_string();
            track.words[0].text = "A".to_string();
            track.words[0].range = TimeRange::new(1_000_000, 2_000_000).unwrap();
            track.words[1].text = "C".to_string();
            track.words[1].range = TimeRange::new(3_000_000, 4_000_000).unwrap();
            track.notes[0].range =
                TimeRange::new(1_000_000, if held { 4_000_000 } else { 2_000_000 }).unwrap();
            if !held {
                let mut later = track.notes[0].clone();
                later.id = "later-note".to_string();
                later.range = track.words[1].range;
                later.word_id = Some(track.words[1].word_id.clone());
                track.notes.push(later);
            }
            track.lyric_units = [
                ("word-1", "A", Some(track.words[0].range)),
                ("middle-line", "B", None),
                ("word-2", "C", Some(track.words[1].range)),
            ]
            .into_iter()
            .map(
                |(id, text, measured_range)| crate::fusion::CanonicalLyricUnit {
                    id: id.to_string(),
                    text: text.to_string(),
                    line_id: Some(id.to_string()),
                    measured_range,
                    audition_range: TimeRange::new(0, 10_000_000).unwrap(),
                },
            )
            .collect();
            let chart =
                finalize_candidate_vocal_chart(&track, "unresolved-line-order", None).unwrap();
            let tokens = chart.tracks[0]
                .phrases
                .iter()
                .flat_map(|phrase| &phrase.notes)
                .flat_map(|note| &note.lyrics)
                .filter_map(|token| match token {
                    LyricToken::Text(token) if !token.text.is_empty() => Some(token),
                    _ => None,
                })
                .collect::<Vec<_>>();
            assert_eq!(
                tokens
                    .iter()
                    .map(|token| token.text.as_str())
                    .collect::<Vec<_>>(),
                ["A", "B", "C"]
            );
            assert!(tokens[1].timing_unresolved);
        }
    }

    #[test]
    fn nearby_word_onsets_reuse_real_note_edges_without_micro_fragments() {
        let edge: u64 = 1_000_000;
        for offset in [-40_000_i64, 40_000] {
            for next_pitch in [58, 56] {
                let onset = edge.saturating_add_signed(offset);
                let mut track = cross_word_track(edge);
                track.words[0].range = TimeRange::new(0, onset).unwrap();
                track.words[1].range = TimeRange::new(onset, 2_000_000).unwrap();
                track.notes[0].range = TimeRange::new(0, edge).unwrap();
                track.notes[0].midi_note = 58;
                let mut next = track.notes[0].clone();
                next.id = "next-real-note".to_string();
                next.range = TimeRange::new(edge, 2_000_000).unwrap();
                next.word_id = Some(track.words[1].word_id.clone());
                next.midi_note = next_pitch;
                track.notes.push(next);
                let original = track.clone();
                let chart = finalize_candidate_vocal_chart(&track, "near-word-edge", None).unwrap();
                let notes = &chart.tracks[0].phrases[0].notes;
                assert_eq!(notes.len(), 2);
                for (note, selected) in notes.iter().zip(&track.notes) {
                    assert_eq!(note.id, selected.id);
                    assert_eq!(note.start, selected.range.start);
                    assert_eq!(note.duration, selected.range.end - selected.range.start);
                    assert_eq!(note.pitch.unwrap().midi, selected.midi_note);
                }
                assert!(matches!(&notes[0].lyrics[0], LyricToken::Text(token)
                    if token.text == "sing"));
                assert!(matches!(&notes[1].lyrics[0], LyricToken::Text(token)
                    if token.text == "now"));
                assert_eq!(
                    notes[1].start.abs_diff(track.words[1].range.start),
                    offset.unsigned_abs()
                );
                assert!(
                    notes[1].start.abs_diff(track.words[1].range.start)
                        <= crate::fusion::BOUNDARY_EVIDENCE_TOLERANCE
                );
                assert_eq!(track, original);
            }
        }
    }

    #[test]
    fn separate_gaps_and_distant_unassociated_words_do_not_reuse_note_edges() {
        for (onset, gap) in [(960_000, 10_000), (920_000, 0)] {
            let mut track = cross_word_track(1_000_000);
            track.words[0].range = TimeRange::new(0, onset).unwrap();
            track.words[1].range = TimeRange::new(onset, 2_000_000).unwrap();
            track.notes[0].range = TimeRange::new(0, 1_000_000).unwrap();
            let mut next = track.notes[0].clone();
            next.id = "separate-note".to_string();
            next.range = TimeRange::new(1_000_000 + gap, 2_000_000).unwrap();
            next.word_id = Some(if gap == 0 {
                track.words[0].word_id.clone()
            } else {
                track.words[1].word_id.clone()
            });
            track.notes.push(next);
            let notes = notes_with_lyric_ownership(&track);
            assert_eq!(notes.len(), 2);
            assert_eq!(notes[0].range, track.notes[0].range);
            assert_eq!(notes[1].range, track.notes[1].range);
            let chart = finalize_candidate_vocal_chart(&track, "gap-word-times", None).unwrap();
            let notes = &chart.tracks[0].phrases[0].notes;
            assert_eq!(notes.len(), 2);
            assert!(notes.iter().flat_map(|note| &note.lyrics).any(
                |token| matches!(token, LyricToken::Text(token) if token.id == "word-2"
                    && token.timing.unwrap().start == onset)
            ));
        }
    }
    #[test]
    fn agreeing_word_owners_reuse_their_local_note_edge() {
        let mut track = cross_word_track(2_900_000);
        track.words[0].range = TimeRange::new(2_640_000, 2_800_000).unwrap();
        track.words[1].range = TimeRange::new(2_800_000, 3_180_000).unwrap();
        track.notes[0].range = TimeRange::new(2_640_000, 2_900_000).unwrap();
        let mut after = track.notes[0].clone();
        after.id = "after-edge".to_string();
        after.range = TimeRange::new(2_900_000, 3_180_000).unwrap();
        after.word_id = Some(track.words[1].word_id.clone());
        track.notes.push(after);
        let original = track.clone();
        let projected = notes_with_lyric_ownership(&track);
        assert_eq!(projected, track.notes);
        let chart = finalize_candidate_vocal_chart(&track, "local-word-edge", None).unwrap();
        let notes = &chart.tracks[0].phrases[0].notes;
        assert_eq!(notes.len(), 2);
        assert_eq!(notes[0].duration, 260_000);
        assert_eq!(notes[1].start, 2_900_000);
        assert!(matches!(&notes[0].lyrics[0], LyricToken::Text(token) if token.text == "sing"));
        assert!(matches!(&notes[1].lyrics[0], LyricToken::Text(token) if token.text == "now"));
        assert_eq!(track, original);
    }

    #[test]
    fn internal_syllable_stays_when_the_next_note_has_no_word_overlap() {
        let mut track = cross_word_track(1_000_000);
        track.transcript.text = "切に".to_string();
        track.words[0].text = "切".to_string();
        track.words[0].range = TimeRange::new(0, 860_000).unwrap();
        track.words[1].text = "に".to_string();
        track.words[1].range = TimeRange::new(860_000, 960_000).unwrap();
        track.notes[0].range = TimeRange::new(0, 1_000_000).unwrap();
        let mut after = track.notes[0].clone();
        after.id = "after-word".to_string();
        after.range = TimeRange::new(1_000_000, 2_000_000).unwrap();
        after.word_id = Some(track.words[1].word_id.clone());
        track.notes.push(after);
        let original = track.clone();
        let projected = notes_with_lyric_ownership(&track);
        assert_eq!(projected.len(), 2);
        assert_eq!(projected[0].range, track.notes[0].range);
        assert!(projected[1].word_id.is_none());
        assert_eq!(projected[1].range, track.notes[1].range);
        let chart = finalize_candidate_vocal_chart(&track, "internal-syllable", None).unwrap();
        let notes = &chart.tracks[0].phrases[0].notes;
        assert_eq!(notes.len(), 2);
        assert!(matches!(&notes[0].lyrics[0], LyricToken::Text(token) if token.text == "切"));
        assert!(matches!(&notes[0].lyrics[1], LyricToken::Text(token)
            if token.text == "に" && token.timing.unwrap().start == 860_000));
        assert_eq!(track, original);
    }

    #[test]
    fn nearby_word_edge_corrects_stale_overlap_ownership_without_a_prefix() {
        let edge = 1_000_000;
        let mut track = cross_word_track(edge);
        track.words[0].range = TimeRange::new(0, edge + 20_000).unwrap();
        track.words[1].range = TimeRange::new(edge + 20_000, 2_000_000).unwrap();
        track.notes[0].range = TimeRange::new(0, edge).unwrap();
        let mut after = track.notes[0].clone();
        after.id = "after-edge".to_string();
        after.range = TimeRange::new(edge, 2_000_000).unwrap();
        // A prior largest-overlap association need not already be correct.
        after.word_id = track.notes[0].word_id.clone();
        track.notes.push(after);
        let original = track.clone();
        let chart = finalize_candidate_vocal_chart(&track, "stale-word-owner", None).unwrap();
        let notes = &chart.tracks[0].phrases[0].notes;
        assert_eq!(notes.len(), 2);
        assert_eq!(notes[1].start, edge);
        assert_eq!(notes[1].duration, 1_000_000);
        assert!(matches!(&notes[0].lyrics[0], LyricToken::Text(token) if token.text == "sing"));
        assert!(matches!(&notes[1].lyrics[0], LyricToken::Text(token) if token.text == "now"));
        assert_eq!(track, original);
    }

    #[test]
    fn a_distant_internal_word_time_does_not_split_the_measured_note() {
        let edge = 1_000_000;
        let onset = edge + 140_000;
        let mut track = cross_word_track(edge);
        track.words[0].range = TimeRange::new(0, onset).unwrap();
        track.words[1].range = TimeRange::new(onset, 2_000_000).unwrap();
        track.notes[0].range = TimeRange::new(0, edge).unwrap();
        let mut after = track.notes[0].clone();
        after.id = "after-edge".to_string();
        after.range = TimeRange::new(edge, 2_000_000).unwrap();
        track.notes.push(after);
        let original = track.clone();
        let chart = finalize_candidate_vocal_chart(&track, "internal-word-onset", None).unwrap();
        let notes = &chart.tracks[0].phrases[0].notes;
        assert_eq!(notes.len(), 2);
        assert_eq!(notes[1].start, edge);
        assert_eq!(notes[1].duration, 1_000_000);
        assert!(matches!(&notes[1].lyrics[0], LyricToken::Text(token)
            if token.text == "now" && token.timing.unwrap().start == onset));
        assert_eq!(track, original);
    }

    #[test]
    fn one_note_edge_cannot_collapse_two_short_repeated_words() {
        let edge = 1_000_000;
        let mut track = cross_word_track(edge);
        track.transcript.text = "sing now now".to_string();
        track.words[0].range = TimeRange::new(0, edge - 20_000).unwrap();
        track.words[1].range = TimeRange::new(edge - 20_000, edge + 20_000).unwrap();
        let mut repeated = track.words[1].clone();
        repeated.word_id = "repeated-word".to_string();
        repeated.range = TimeRange::new(edge + 20_000, 2_000_000).unwrap();
        track.words.push(repeated);
        track.notes[0].range = TimeRange::new(0, edge).unwrap();
        let mut after = track.notes[0].clone();
        after.id = "after-edge".to_string();
        after.range = TimeRange::new(edge, 2_000_000).unwrap();
        track.notes.push(after);
        let original = track.clone();
        let chart = finalize_candidate_vocal_chart(&track, "short-repeated-words", None).unwrap();
        let notes = &chart.tracks[0].phrases[0].notes;
        assert_eq!(notes.len(), 2);
        assert_eq!(notes[1].start, edge);
        assert_eq!(notes[1].duration, 1_000_000);
        assert!(notes.iter().flat_map(|note| &note.lyrics).any(
            |token| matches!(token, LyricToken::Text(token) if token.id == "word-2"
                && token.timing == Some(LyricTiming { start: edge - 20_000, duration: 40_000 }))
        ));
        let words = notes
            .iter()
            .flat_map(|note| &note.lyrics)
            .filter_map(|token| match token {
                LyricToken::Text(token) => Some((token.id.as_str(), token.text.as_str())),
                _ => None,
            })
            .collect::<Vec<_>>();
        assert_eq!(
            words,
            [
                ("word-1", "sing"),
                ("word-2", "now"),
                ("repeated-word", "now")
            ]
        );
        assert_eq!(track, original);
    }
    #[test]
    fn a_dense_unresolved_line_uses_one_scope_without_hiding_measured_neighbours() {
        let mut track = cross_word_track(3_000_000);
        track.notes[0].range = TimeRange::new(0, 3_000_000).unwrap();
        track.words[0].range = TimeRange::new(0, 1_000_000).unwrap();
        track.words[1].range = TimeRange::new(2_000_000, 3_000_000).unwrap();
        let scope = TimeRange::new(0, 138_000_000).unwrap();
        let make_unit =
            |id: String, text: String, measured_range| crate::fusion::CanonicalLyricUnit {
                id,
                text,
                line_id: Some("caller-line".into()),
                measured_range,
                audition_range: scope,
            };
        track.lyric_units.push(make_unit(
            "word-1".into(),
            "sing".into(),
            Some(track.words[0].range),
        ));
        for index in 0..186 {
            track
                .lyric_units
                .push(make_unit(format!("missing-{index}"), "字".into(), None));
        }
        track.lyric_units.push(make_unit(
            "word-2".into(),
            "now".into(),
            Some(track.words[1].range),
        ));
        track.transcript.text = format!("sing{}now", "字".repeat(186));
        let original = track.clone();
        let chart = finalize_candidate_vocal_chart(&track, "dense-unresolved-line", None).unwrap();
        let notes = &chart.tracks[0].phrases[0].notes;
        assert_eq!(notes.len(), 1);
        let tokens = notes[0]
            .lyrics
            .iter()
            .filter_map(|token| match token {
                LyricToken::Text(token) => Some(token),
                _ => None,
            })
            .collect::<Vec<_>>();
        assert_eq!(tokens.len(), 3);
        assert_eq!(tokens[1].id, "missing-0");
        assert_eq!(tokens[1].text, "字".repeat(186));
        assert!(tokens[1].timing_unresolved);
        assert_eq!(
            tokens[1].timing,
            Some(LyricTiming {
                start: 1_000_000,
                duration: 1_000_000
            })
        );
        assert!(!tokens[0].timing_unresolved && !tokens[2].timing_unresolved);
        assert_eq!(
            tokens
                .iter()
                .map(|token| token.text.as_str())
                .collect::<String>(),
            track.transcript.text
        );
        assert_eq!(track, original);
        chart.validate().unwrap();
    }
    #[test]
    fn unresolved_suffix_follows_a_measured_word_sharing_another_words_note() {
        let mut track = track();
        track.transcript.text = "계신주께여엉".into();
        track.transcript.language = Some("ko".into());
        track.words[0].text = "계".into();
        track.words[0].range = TimeRange::new(520_000, 600_000).unwrap();
        for (id, text, start, end) in [
            ("shared-word", "신", 1_000_000, 1_160_000),
            ("later-word", "엉", 1_400_000, 2_480_000),
        ] {
            let mut word = track.words[0].clone();
            word.word_id = id.into();
            word.text = text.into();
            word.range = TimeRange::new(start, end).unwrap();
            track.words.push(word);
        }
        track.notes[0].range = TimeRange::new(20_000, 570_000).unwrap();
        let mut middle = track.notes[0].clone();
        middle.id = "prior-word-continuation".into();
        middle.range = TimeRange::new(570_000, 1_000_000).unwrap();
        track.notes.push(middle);
        let mut later = track.notes[0].clone();
        later.id = "shared-note".into();
        later.word_id = Some("later-word".into());
        later.range = TimeRange::new(1_110_000, 1_580_000).unwrap();
        track.notes.push(later);
        track.lyric_units = [
            ("word-1", "계", Some(track.words[0].range)),
            ("shared-word", "신", Some(track.words[1].range)),
            ("missing-words", "주께여", None),
            ("later-word", "엉", Some(track.words[2].range)),
        ]
        .into_iter()
        .map(
            |(id, text, measured_range)| crate::fusion::CanonicalLyricUnit {
                id: id.into(),
                text: text.into(),
                line_id: None,
                measured_range,
                audition_range: TimeRange::new(0, 3_000_000).unwrap(),
            },
        )
        .collect();
        let original = track.clone();
        let chart =
            finalize_candidate_vocal_chart(&track, "shared-word-suffix-order", None).unwrap();
        let notes = &chart.tracks[0].phrases[0].notes;
        assert_eq!(notes.len(), track.notes.len());
        for (note, selected) in notes.iter().zip(&track.notes) {
            assert_eq!(note.id, selected.id);
            assert_eq!(note.start, selected.range.start);
            assert_eq!(note.duration, selected.range.end - selected.range.start);
            assert_eq!(note.pitch.unwrap().midi, selected.midi_note);
        }
        let text_tokens = |note: &VocalNote| {
            note.lyrics
                .iter()
                .filter_map(|token| match token {
                    LyricToken::Text(token) => Some(token.clone()),
                    _ => None,
                })
                .collect::<Vec<_>>()
        };
        let tokens = notes.iter().flat_map(text_tokens).collect::<Vec<_>>();
        assert_eq!(
            tokens
                .iter()
                .map(|token| token.text.as_str())
                .collect::<String>(),
            track.transcript.text
        );
        let shared = text_tokens(&notes[2]);
        assert_eq!(
            shared
                .iter()
                .map(|token| token.text.as_str())
                .collect::<Vec<_>>(),
            ["신", "주께여", "엉"]
        );
        assert_eq!(
            shared[0].timing,
            Some(LyricTiming {
                start: 1_000_000,
                duration: 160_000
            })
        );
        assert!(!shared[0].timing_unresolved);
        assert_eq!(
            shared[1].timing,
            Some(LyricTiming {
                start: 1_160_000,
                duration: 240_000
            })
        );
        assert!(shared[1].timing_unresolved);
        assert_eq!(
            shared[2].timing,
            Some(LyricTiming {
                start: 1_400_000,
                duration: 1_080_000
            })
        );
        assert!(matches!(&notes[1].lyrics[0], LyricToken::Continuation {
            continuation_of
        } if continuation_of == "word-1"));
        assert_eq!(track, original);
        chart.validate().unwrap();
    }
}
