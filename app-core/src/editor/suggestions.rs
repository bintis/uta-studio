use serde::{Deserialize, Serialize};

use crate::artifact_workbench::ArtifactRef;

use super::evidence::{EvidenceKind, EvidenceTrack, ReviewReason, SingingEvidenceBundle};
use super::{EditorDocument, LyricAddress, TrackRole};

/// Minimum review-region confidence for an evidence disagreement to become a
/// suggestion. Below this, the underlying evidence is too weak to surface as
/// an actionable one-click accept/ignore choice.
const MIN_REGION_CONFIDENCE: f32 = 0.5;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum EditorSuggestionKind {
    ChangePitch {
        note_index: usize,
        midi: f64,
    },
    MoveBoundary {
        note_index: usize,
        start: f64,
        end: f64,
    },
    BindLyric {
        lyric: LyricAddress,
        note_index: usize,
    },
    ChangeTrackRole {
        track_index: usize,
        role: TrackRole,
    },
    InspectEvidence,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct EditorSuggestion {
    pub id: String,
    pub start: f64,
    pub end: f64,
    pub confidence: f32,
    pub suggestion: EditorSuggestionKind,
    #[serde(default)]
    pub evidence_refs: Vec<ArtifactRef>,
}

/// Applies only an explicitly accepted suggestion. Desktop code checkpoints
/// the document first, so this ordinary document mutation participates in the
/// existing undo/redo history rather than creating a model-owned history.
pub fn apply_editor_suggestion(
    document: &mut EditorDocument,
    suggestion: &EditorSuggestion,
) -> Result<bool, String> {
    let changed = match &suggestion.suggestion {
        EditorSuggestionKind::ChangePitch { note_index, midi } => {
            let note = document
                .notes()
                .get(*note_index)
                .cloned()
                .ok_or_else(|| "suggestion note no longer exists".to_string())?;
            document.move_note(*note_index, note.start, note.end, *midi)
        }
        EditorSuggestionKind::MoveBoundary {
            note_index,
            start,
            end,
        } => document.resize_note(*note_index, *start, *end),
        EditorSuggestionKind::BindLyric { lyric, note_index } => {
            document.bind_lyric_to_note(*lyric, *note_index).is_some()
        }
        EditorSuggestionKind::ChangeTrackRole { track_index, role } => {
            document.set_track_role(*track_index, *role)
        }
        EditorSuggestionKind::InspectEvidence => false,
    };
    Ok(changed)
}

impl EditorDocument {
    /// Read-only projection of existing evidence disagreements into
    /// suggestions the user can accept or ignore with one click. Does not
    /// reimplement any fusion/analysis logic — it only compares evidence
    /// numbers that already exist against note numbers that already exist.
    pub fn derive_evidence_suggestions(
        &self,
        evidence: &SingingEvidenceBundle,
    ) -> Vec<EditorSuggestion> {
        let Some(fused_f0) = evidence
            .tracks
            .iter()
            .find(|t| t.kind == EvidenceKind::FusedF0)
        else {
            return Vec::new();
        };
        let notes = self.notes();
        evidence
            .review_regions
            .iter()
            .filter(|r| {
                !r.reviewed
                    && r.confidence
                        .is_some_and(|confidence| confidence >= MIN_REGION_CONFIDENCE)
            })
            .filter(|r| r.reasons.contains(&ReviewReason::PitchDisagreement))
            .filter_map(|region| {
                let confidence = region.confidence?;
                let note = notes
                    .iter()
                    .find(|n| n.pitched && n.start < region.end && n.end > region.start)?;
                let suggested = median_evidence_midi(fused_f0, region.start, region.end)?;
                (suggested.round() != note.midi.round()).then(|| EditorSuggestion {
                    id: format!("evidence-pitch-{}-{}", region.id, note.index),
                    start: region.start,
                    end: region.end,
                    confidence,
                    suggestion: EditorSuggestionKind::ChangePitch {
                        note_index: note.index,
                        midi: suggested.round(),
                    },
                    evidence_refs: region.evidence_refs.clone(),
                })
            })
            .collect()
    }
}

/// `singing_analysis_evidence_bundle` defines every `FusedF0` point's
/// `.value` as measured Hz at the app-owned Engine-artifact boundary.
fn median_evidence_midi(track: &EvidenceTrack, start: f64, end: f64) -> Option<f64> {
    let mut midis: Vec<f64> = track
        .points
        .iter()
        .filter(|p| p.time >= start && p.time <= end)
        .filter_map(|p| {
            let hz = f64::from(p.value);
            (hz.is_finite() && hz > 0.0).then(|| 69.0 + 12.0 * (hz / 440.0).log2())
        })
        .collect();
    if midis.is_empty() {
        return None;
    }
    midis.sort_by(f64::total_cmp);
    Some(midis[midis.len() / 2])
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::analysis_graph::ArtifactKind;
    use crate::editor::evidence::{EvidencePoint, ReviewRegion, ReviewSeverity};
    use utz::{
        DEFAULT_TIMEBASE, LyricJoin, LyricTextToken, LyricToken, NoteBonus, NotePitch, NoteScoring,
        ScoringMode, VocalChartV1, VocalMode, VocalNote, VocalPhrase, VocalTrack, VocalTrackRole,
    };

    fn evidence_point(time: f64, hz: f32) -> EvidencePoint {
        EvidencePoint {
            time,
            value: hz,
            pitch: None,
            label: None,
        }
    }

    fn fused_f0_track(points: Vec<EvidencePoint>) -> EvidenceTrack {
        EvidenceTrack {
            id: "fused-f0".to_string(),
            label: "Fused F0".to_string(),
            kind: EvidenceKind::FusedF0,
            source: ArtifactRef {
                file_hash: "song".to_string(),
                kind: ArtifactKind::PitchEvidence,
                revision_id: "rev".to_string(),
            },
            points,
        }
    }

    fn review_region(id: &str, start: f64, end: f64, confidence: f32) -> ReviewRegion {
        ReviewRegion {
            id: id.to_string(),
            start,
            end,
            severity: ReviewSeverity::Warning,
            reasons: vec![ReviewReason::PitchDisagreement],
            confidence: Some(confidence),
            evidence_refs: Vec::new(),
            reviewed: false,
        }
    }

    // One pitched note spanning [0.0, 1.0] at MIDI 69 (A4, 440.0 Hz).
    fn document_with_one_note() -> EditorDocument {
        let phrase = VocalPhrase {
            id: "phrase-1".into(),
            notes: vec![VocalNote {
                id: "note-1".into(),
                start: 0,
                duration: DEFAULT_TIMEBASE,
                pitch: Some(NotePitch { midi: 69, cents: 0 }),
                vocal_mode: VocalMode::Pitched,
                bonus: NoteBonus::Normal,
                scoring: NoteScoring {
                    mode: ScoringMode::Pitch,
                    weight: 1.0,
                },
                lyrics: vec![LyricToken::Text(LyricTextToken {
                    id: "lyric-1".into(),
                    text: "a".into(),
                    join_before: LyricJoin::Space,
                    reading: None,
                    phonemes: None,
                })],
            }],
        };
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

    #[test]
    fn no_fused_f0_track_yields_empty_result() {
        let document = document_with_one_note();
        let bundle = SingingEvidenceBundle {
            review_regions: vec![review_region("r0", 0.0, 1.0, 0.9)],
            ..Default::default()
        };
        assert!(document.derive_evidence_suggestions(&bundle).is_empty());
    }

    #[test]
    fn pitch_disagreement_at_least_a_semitone_off_produces_change_pitch() {
        let document = document_with_one_note();
        // B4 = 493.88 Hz, roughly two semitones above A4 (note midi 69).
        let track = fused_f0_track(vec![evidence_point(0.5, 493.88)]);
        let bundle = SingingEvidenceBundle {
            tracks: vec![track],
            review_regions: vec![review_region("r0", 0.0, 1.0, 0.9)],
            ..Default::default()
        };
        let suggestions = document.derive_evidence_suggestions(&bundle);
        assert_eq!(suggestions.len(), 1);
        match &suggestions[0].suggestion {
            EditorSuggestionKind::ChangePitch { note_index, midi } => {
                assert_eq!(*note_index, 0);
                assert_eq!(*midi, 71.0);
            }
            other => panic!("expected ChangePitch, got {other:?}"),
        }
        assert_eq!(suggestions[0].evidence_refs.len(), 0);
    }

    #[test]
    fn evidence_pitch_rounding_matching_note_produces_no_suggestion() {
        let document = document_with_one_note();
        // 440.0 Hz rounds to the same MIDI as the note (69).
        let track = fused_f0_track(vec![evidence_point(0.5, 440.0)]);
        let bundle = SingingEvidenceBundle {
            tracks: vec![track],
            review_regions: vec![review_region("r0", 0.0, 1.0, 0.9)],
            ..Default::default()
        };
        assert!(document.derive_evidence_suggestions(&bundle).is_empty());
    }

    #[test]
    fn region_without_pitch_disagreement_reason_is_skipped() {
        let document = document_with_one_note();
        let track = fused_f0_track(vec![evidence_point(0.5, 493.88)]);
        let mut region = review_region("r0", 0.0, 1.0, 0.9);
        region.reasons = vec![ReviewReason::LowConfidence];
        let bundle = SingingEvidenceBundle {
            tracks: vec![track],
            review_regions: vec![region],
            ..Default::default()
        };
        assert!(document.derive_evidence_suggestions(&bundle).is_empty());
    }

    #[test]
    fn already_reviewed_region_is_skipped() {
        let document = document_with_one_note();
        let track = fused_f0_track(vec![evidence_point(0.5, 493.88)]);
        let mut region = review_region("r0", 0.0, 1.0, 0.9);
        region.reviewed = true;
        let bundle = SingingEvidenceBundle {
            tracks: vec![track],
            review_regions: vec![region],
            ..Default::default()
        };
        assert!(document.derive_evidence_suggestions(&bundle).is_empty());
    }

    #[test]
    fn region_below_confidence_threshold_is_skipped() {
        let document = document_with_one_note();
        let track = fused_f0_track(vec![evidence_point(0.5, 493.88)]);
        let region = review_region("r0", 0.0, 1.0, MIN_REGION_CONFIDENCE - 0.01);
        let bundle = SingingEvidenceBundle {
            tracks: vec![track],
            review_regions: vec![region],
            ..Default::default()
        };
        assert!(document.derive_evidence_suggestions(&bundle).is_empty());
    }

    #[test]
    fn region_with_unknown_confidence_never_produces_an_automatic_suggestion() {
        let document = document_with_one_note();
        let track = fused_f0_track(vec![evidence_point(0.5, 493.88)]);
        let mut region = review_region("r0", 0.0, 1.0, 0.9);
        region.confidence = None;
        let bundle = SingingEvidenceBundle {
            tracks: vec![track],
            review_regions: vec![region],
            ..Default::default()
        };
        assert!(document.derive_evidence_suggestions(&bundle).is_empty());
    }

    #[test]
    fn region_with_no_overlapping_note_is_skipped() {
        let document = document_with_one_note();
        let track = fused_f0_track(vec![evidence_point(2.5, 493.88)]);
        let region = review_region("r0", 2.0, 3.0, 0.9);
        let bundle = SingingEvidenceBundle {
            tracks: vec![track],
            review_regions: vec![region],
            ..Default::default()
        };
        assert!(document.derive_evidence_suggestions(&bundle).is_empty());
    }

    #[test]
    fn median_is_robust_to_a_single_outlier_within_the_region() {
        let document = document_with_one_note();
        // Three points near B4 (~493.88 Hz) plus one wild outlier; the
        // median should still land on the B4 cluster, not be dragged by the
        // outlier the way a mean would be.
        let track = fused_f0_track(vec![
            evidence_point(0.1, 493.0),
            evidence_point(0.4, 493.88),
            evidence_point(0.6, 494.5),
            evidence_point(0.9, 5000.0),
        ]);
        let bundle = SingingEvidenceBundle {
            tracks: vec![track],
            review_regions: vec![review_region("r0", 0.0, 1.0, 0.9)],
            ..Default::default()
        };
        let suggestions = document.derive_evidence_suggestions(&bundle);
        assert_eq!(suggestions.len(), 1);
        match &suggestions[0].suggestion {
            EditorSuggestionKind::ChangePitch { midi, .. } => assert_eq!(*midi, 71.0),
            other => panic!("expected ChangePitch, got {other:?}"),
        }
    }
}
