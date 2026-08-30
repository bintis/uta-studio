use std::collections::BTreeMap;

use crate::contract::{EngineError, EngineErrorCode, EngineResult};
use crate::fusion::{
    CanonicalNote, CanonicalSingingTrack, CanonicalWordBoundary, TimeRange,
    validate_canonical_singing_track,
};
use crate::quantization::QuantizationReportV1;
use utz::{
    LyricJoin, LyricTextToken, LyricToken, NoteBonus, NotePitch, NoteScoring, ScoringMode,
    VocalChartV1, VocalMode, VocalNote, VocalPhrase, VocalTrack, VocalTrackRole,
};

/// Newly emitted Candidate bytes use the strict UTZ VocalChart 0.3 contract.
/// Studio retains a read-only migration path for legacy Engine candidate/v1
/// cache entries, but the Engine no longer emits that wrapper.
pub type CandidateVocalChartV1 = VocalChartV1;

pub fn finalize_candidate_vocal_chart(
    track: &CanonicalSingingTrack,
    execution_fingerprint: &str,
    preserve_continuous_pitch: bool,
    quantization: Option<&QuantizationReportV1>,
) -> EngineResult<CandidateVocalChartV1> {
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

    let mut notes = Vec::new();
    for (word_index, word) in track.words.iter().enumerate() {
        append_word_notes(
            &mut notes,
            word_index,
            word,
            notes_by_word
                .remove(word.word_id.as_str())
                .unwrap_or_default(),
            preserve_continuous_pitch,
        )?;
    }
    notes.sort_by_key(|note| (note.start, note.id.clone()));
    if notes.is_empty() {
        return Err(invalid(
            "Candidate VocalChart contains no lyric-owned or aligned spoken notes",
        ));
    }

    let mut chart = VocalChartV1::new(vec![VocalTrack {
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

/// Two independently-measured fragments this close together, at the exact
/// same MIDI pitch, are frame-boundary rounding between the boundary
/// experts, not a real gap -- confirmed against real Japanese pop vocal
/// runs where a single held syllable was over-segmented into a dozen
/// back-to-back same-pitch candidates a few milliseconds apart. A real
/// silence, breath, or consonant-driven cut leaves a gap well past this.
const CONTINUOUS_PITCH_MERGE_GAP: crate::contract::CanonicalTime = 20_000;

/// One or more `CanonicalNote` fragments at a continuously held pitch,
/// collapsed into the single note a singer actually sang. This is what
/// `preserve_continuous_pitch` requests: without it, boundary detection's
/// raw over-segmentation (vibrato and frame jitter routinely split one
/// sustained pitch into several near-identical short candidates) publishes
/// straight through to the Candidate chart.
struct MergedNote {
    id: String,
    range: TimeRange,
    midi_note: u8,
    center_offset_cents: f32,
}

fn merge_continuous_pitch_runs(candidates: &[&CanonicalNote]) -> Vec<MergedNote> {
    let mut merged: Vec<MergedNote> = Vec::new();
    for note in candidates {
        if let Some(last) = merged.last_mut()
            && last.midi_note == note.midi_note
            && note.range.start.saturating_sub(last.range.end) <= CONTINUOUS_PITCH_MERGE_GAP
        {
            let previous_duration = (last.range.end - last.range.start) as f64;
            let next_duration = (note.range.end - note.range.start) as f64;
            last.center_offset_cents = ((last.center_offset_cents as f64 * previous_duration
                + note.center_offset_cents as f64 * next_duration)
                / (previous_duration + next_duration)) as f32;
            last.range = TimeRange::new(last.range.start, last.range.end.max(note.range.end))
                .expect("merging two positive-duration ranges keeps a positive-duration range");
            continue;
        }
        merged.push(MergedNote {
            id: note.id.clone(),
            range: note.range,
            midi_note: note.midi_note,
            center_offset_cents: note.center_offset_cents,
        });
    }
    merged
}

fn append_word_notes(
    output: &mut Vec<VocalNote>,
    word_index: usize,
    word: &CanonicalWordBoundary,
    mut candidates: Vec<&CanonicalNote>,
    preserve_continuous_pitch: bool,
) -> EngineResult<()> {
    candidates.sort_by_key(|note| (note.range.start, note.range.end, note.id.as_str()));
    let lyric_id = word.word_id.clone();
    if candidates.is_empty() {
        output.push(VocalNote {
            id: format!("spoken-{word_index}"),
            start: word.range.start,
            duration: word.range.end.saturating_sub(word.range.start),
            pitch: None,
            vocal_mode: VocalMode::Spoken,
            bonus: NoteBonus::Normal,
            scoring: NoteScoring {
                mode: ScoringMode::Rhythm,
                weight: 1.0,
            },
            lyrics: vec![LyricToken::Text(LyricTextToken {
                id: lyric_id,
                text: word.text.clone(),
                join_before: lyric_join_for(output, &word.text),
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
    let notes: Vec<MergedNote> = if preserve_continuous_pitch {
        merge_continuous_pitch_runs(&candidates)
    } else {
        candidates
            .into_iter()
            .map(|note| MergedNote {
                id: note.id.clone(),
                range: note.range,
                midi_note: note.midi_note,
                center_offset_cents: note.center_offset_cents,
            })
            .collect()
    };

    for (index, note) in notes.into_iter().enumerate() {
        let lyrics = if index == 0 {
            vec![LyricToken::Text(LyricTextToken {
                id: lyric_id.clone(),
                text: word.text.clone(),
                join_before: lyric_join_for(output, &word.text),
                reading: None,
                phonemes: None,
            })]
        } else {
            vec![LyricToken::Continuation {
                continuation_of: lyric_id.clone(),
            }]
        };
        output.push(VocalNote {
            id: note.id,
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
        });
    }
    Ok(())
}

fn lyric_join_for(existing: &[VocalNote], text: &str) -> LyricJoin {
    let previous_is_ascii_word = existing
        .iter()
        .rev()
        .flat_map(|note| note.lyrics.iter().rev())
        .find_map(|token| match token {
            LyricToken::Text(token) => Some(token.text.chars().all(|character| {
                character.is_ascii_alphanumeric() || character.is_ascii_punctuation()
            })),
            LyricToken::Continuation { .. } => None,
        })
        == Some(true);
    let current_is_ascii_word = text
        .chars()
        .all(|character| character.is_ascii_alphanumeric() || character.is_ascii_punctuation());
    if previous_is_ascii_word && current_is_ascii_word {
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
        let chart = finalize_candidate_vocal_chart(&track, &"a".repeat(64), true, None).unwrap();
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
    fn explicit_continuous_pitch_setting_only_changes_geometry_when_fragments_exist_to_merge() {
        // The single-note fixture has nothing to merge, so both settings
        // must still agree -- `preserve_continuous_pitch` never invents
        // geometry, it only collapses genuine over-segmentation.
        let track = track();
        let preserved =
            finalize_candidate_vocal_chart(&track, &"b".repeat(64), true, None).unwrap();
        let omitted = finalize_candidate_vocal_chart(&track, &"b".repeat(64), false, None).unwrap();
        assert_eq!(preserved, omitted);
    }

    #[test]
    fn continuous_pitch_setting_merges_touching_same_pitch_fragments_into_one_note() {
        // Real repro this exists for: boundary detection over-segments one
        // sustained pitch (vibrato/frame jitter) into several back-to-back
        // same-MIDI candidates a few milliseconds apart. `preserve_continuous_pitch`
        // should collapse those into the single note a singer actually held.
        let mut track = track();
        track.notes[0].range = TimeRange::new(100_001, 300_002).unwrap();
        track.notes[0].center_offset_cents = -10.0;
        let mut fragment = track.notes[0].clone();
        fragment.id = "note-2".to_string();
        fragment.range = TimeRange::new(300_012, 500_003).unwrap(); // 10us gap: rounding, not a real one.
        fragment.center_offset_cents = 10.0;
        track.notes.push(fragment);

        let merged = finalize_candidate_vocal_chart(&track, &"f".repeat(64), true, None).unwrap();
        let merged_notes = &merged.tracks[0].phrases[0].notes;
        assert_eq!(merged_notes.len(), 1);
        assert_eq!(merged_notes[0].id, "note-1");
        assert_eq!(merged_notes[0].start, 100_001);
        assert_eq!(merged_notes[0].duration, 500_003 - 100_001);
        // Duration-weighted average of two equal-length fragments at -10/+10.
        assert_eq!(merged_notes[0].pitch.unwrap().cents, 0);
        merged.validate().unwrap();

        let unmerged =
            finalize_candidate_vocal_chart(&track, &"f".repeat(64), false, None).unwrap();
        let unmerged_notes = &unmerged.tracks[0].phrases[0].notes;
        assert_eq!(
            unmerged_notes
                .iter()
                .map(|note| note.id.as_str())
                .collect::<Vec<_>>(),
            ["note-1", "note-2"]
        );
    }

    #[test]
    fn continuous_pitch_setting_never_merges_across_a_real_gap_or_a_pitch_change() {
        let mut track = track();
        track.notes[0].range = TimeRange::new(100_001, 300_002).unwrap();
        let mut far_same_pitch = track.notes[0].clone();
        far_same_pitch.id = "note-2".to_string();
        far_same_pitch.range = TimeRange::new(400_000, 500_003).unwrap(); // ~100ms real gap.
        track.notes.push(far_same_pitch);

        let chart = finalize_candidate_vocal_chart(&track, &"g".repeat(64), true, None).unwrap();
        assert_eq!(chart.tracks[0].phrases[0].notes.len(), 2);
    }

    #[test]
    fn unpitched_aligned_words_are_retained_as_spoken_notes() {
        let mut track = track();
        track.notes.clear();
        let chart = finalize_candidate_vocal_chart(&track, &"c".repeat(64), true, None).unwrap();
        let note = &chart.tracks[0].phrases[0].notes[0];
        assert_eq!(note.vocal_mode, VocalMode::Spoken);
        assert!(note.pitch.is_none());
        chart.validate().unwrap();
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

        let chart = finalize_candidate_vocal_chart(&track, &"e".repeat(64), true, None).unwrap();
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
        let chart = finalize_candidate_vocal_chart(&track(), &"d".repeat(64), true, None).unwrap();
        let reference = write_json_artifact(
            &root,
            Path::new("candidate/vocal-chart.json"),
            utz::VOCAL_CHART_MEDIA_TYPE,
            &chart,
        )
        .unwrap();
        assert!(reference.bytes > 0);
        let decoded: CandidateVocalChartV1 =
            serde_json::from_slice(&std::fs::read(root.join(reference.path)).unwrap()).unwrap();
        decoded.validate().unwrap();
        assert_eq!(decoded, chart);
        std::fs::remove_dir_all(root).unwrap();
    }
}
