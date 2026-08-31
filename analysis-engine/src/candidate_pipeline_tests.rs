use std::collections::BTreeMap;

use super::*;
use crate::artifact::{
    ACOUSTIC_EVIDENCE_CONTRACT, ACOUSTIC_EVIDENCE_VERSION, AcousticEvidenceFrameV1,
    BasicPitchFrameV3, GameNoteEvidenceV1,
};
use crate::fingerprint::ACOUSTIC_DSP_VERSION;
use crate::fusion::{LyricsAuthority, SingingReviewReason, TimeRange};

#[cfg(unix)]
fn write_executable(path: &std::path::Path, body: &str) {
    use std::io::Write;
    use std::os::unix::fs::PermissionsExt;

    let staging = path.with_extension("part");
    {
        let mut file = std::fs::File::create(&staging).unwrap();
        file.write_all(body.as_bytes()).unwrap();
        file.sync_all().unwrap();
    }
    std::fs::set_permissions(&staging, std::fs::Permissions::from_mode(0o700)).unwrap();
    std::fs::rename(staging, path).unwrap();
}

fn transcript(authority: TranscriptAuthorityV1) -> TranscriptArtifactV1 {
    let caller = authority == TranscriptAuthorityV1::CallerCanonical;
    TranscriptArtifactV1 {
        contract: "uta.analysis-engine.transcript".to_string(),
        version: 1,
        authority,
        language: Some("en".to_string()),
        text: "sing now".to_string(),
        tokens: if caller {
            vec![
                TranscriptTokenV1 {
                    id: "caller-1".to_string(),
                    text: "sing".to_string(),
                    confidence: None,
                },
                TranscriptTokenV1 {
                    id: "caller-2".to_string(),
                    text: "now".to_string(),
                    confidence: None,
                },
            ]
        } else {
            Vec::new()
        },
        confidence: None,
        source_experts: vec![if caller {
            "caller.canonical_lyrics".to_string()
        } else {
            "qwen3_asr_1_7b".to_string()
        }],
        alternatives: Vec::new(),
        model_sha256: (!caller).then(|| "a".repeat(64)),
        runtime_manifest_sha256: (!caller).then(|| "b".repeat(64)),
        backend: if caller { "caller" } else { "vulkan" }.to_string(),
    }
}

fn alignment() -> AlignmentArtifactV1 {
    AlignmentArtifactV1 {
        contract: "uta.analysis-engine.alignment".to_string(),
        version: 1,
        transcript: "sing now".to_string(),
        language: Some("en".to_string()),
        items: vec![
            AlignmentItemV1 {
                id: "word-0".to_string(),
                text: "sing".to_string(),
                level: BoundaryLevel::Word,
                start: 100_000,
                duration: 300_000,
                confidence: None,
                authority: BoundaryAuthority::Soft,
            },
            AlignmentItemV1 {
                id: "word-1".to_string(),
                text: "now".to_string(),
                level: BoundaryLevel::Word,
                start: 500_000,
                duration: 400_000,
                confidence: None,
                authority: BoundaryAuthority::Soft,
            },
        ],
        source_expert: "qwen3_forced_aligner_0_6b".to_string(),
        model_sha256: "c".repeat(64),
        runtime_manifest_sha256: "d".repeat(64),
        backend: "vulkan".to_string(),
    }
}

fn pitch(octave_disagreement: bool) -> PitchEvidenceV03 {
    let mut frequency_hz = vec![None; 100];
    let mut confidence = vec![Some(0.1); 100];
    for index in 10..40 {
        frequency_hz[index] = Some(if octave_disagreement { 880.0 } else { 440.0 });
        confidence[index] = Some(0.9);
    }
    for index in 50..90 {
        frequency_hz[index] = Some(493.88);
        confidence[index] = Some(0.8);
    }
    PitchEvidenceV03 {
        format: "uta.pitch-evidence".to_string(),
        format_version: "0.3.0".to_string(),
        timebase: 1_000_000,
        start: 0,
        hop: 10_000,
        frequency_hz,
        confidence,
        model: BTreeMap::new(),
    }
}

fn game() -> GameEvidenceV1 {
    GameEvidenceV1 {
        schema_version: 1,
        model_id: "game".to_string(),
        variant: "fixture".to_string(),
        source_asset_sha256: "e".repeat(64),
        source_commit: "fixture".to_string(),
        model_manifest_sha256: "f".repeat(64),
        runtime_manifest_sha256: "1".repeat(64),
        backend: "openvino_gpu".to_string(),
        sample_rate: 44_100,
        timestep_ms: 10,
        d3pm_steps: 8,
        estimator_note_buckets: vec![32],
        notes: vec![
            GameNoteEvidenceV1 {
                range: TimeRange::new(100_000, 400_000).unwrap(),
                midi: 69.25,
                boundary_decision_threshold: 0.2,
                presence_decision_threshold: 0.2,
            },
            GameNoteEvidenceV1 {
                range: TimeRange::new(500_000, 900_000).unwrap(),
                midi: 71.1,
                boundary_decision_threshold: 0.2,
                presence_decision_threshold: 0.2,
            },
        ],
    }
}

fn basic_pitch() -> BasicPitchEvidenceV3 {
    BasicPitchEvidenceV3 {
        frames: vec![
            BasicPitchFrameV3 {
                time: 110_000,
                note_activation: 0.9,
                onset_activation: 0.8,
                contour_class: 42,
                contour_activation: 0.7,
            },
            BasicPitchFrameV3 {
                time: 510_000,
                note_activation: 0.9,
                onset_activation: 0.8,
                contour_class: 44,
                contour_activation: 0.7,
            },
        ],
        model_manifest_sha256: "3".repeat(64),
        runtime_manifest_sha256: "4".repeat(64),
    }
}

fn acoustic() -> AcousticEvidenceV1 {
    AcousticEvidenceV1 {
        contract: ACOUSTIC_EVIDENCE_CONTRACT.to_string(),
        version: ACOUSTIC_EVIDENCE_VERSION,
        algorithm: ACOUSTIC_DSP_VERSION.to_string(),
        timebase: 1_000_000,
        start: 0,
        hop: 10_000,
        sample_rate: 16_000,
        window_samples: 512,
        semantic_audio_role: "lead_vocal".to_string(),
        decoded_audio_sha256: "2".repeat(64),
        frames: (0..100)
            .map(|index| AcousticEvidenceFrameV1 {
                start: index * 10_000,
                rms: 0.2,
                spectral_flux: (index > 0).then_some(if index == 10 || index == 50 {
                    0.3
                } else {
                    0.01
                }),
                periodicity: 0.8,
                snr_db: 20.0,
                fundamental_hz: Some(440.0),
                vibrato_activation: 0.0,
                glide_activation: 0.0,
                ornament_activation: 0.0,
                breath_activation: 0.0,
                voicing_transition_activation: 0.0,
            })
            .collect(),
    }
}

fn fused_inputs(
    octave_disagreement: bool,
) -> (
    TranscriptArtifactV1,
    CanonicalLyrics,
    AlignmentArtifactV1,
    Vec<CanonicalWordBoundary>,
    PitchEvidenceV03,
) {
    let (transcript, lyrics) =
        fuse_transcript_stage(&[transcript(TranscriptAuthorityV1::Generated)], None).unwrap();
    let (alignment, words) = fuse_alignment_stage(&lyrics, &[alignment()], 0, 1_000_000).unwrap();
    (
        transcript,
        lyrics,
        alignment,
        words,
        pitch(octave_disagreement),
    )
}

#[test]
fn baseline_review_uses_the_selected_fcpe_primary() {
    let (transcript, lyrics, alignment, words, pitch) = fused_inputs(false);
    let regions = build_baseline_review_regions(
        &transcript,
        lyrics,
        &alignment,
        words,
        None,
        Some(&pitch),
        &game(),
        &acoustic(),
        0,
        1_000_000,
        "fcpe",
    )
    .unwrap();
    assert!(regions.iter().all(|region| region.range.end <= 1_000_000));
}

#[test]
fn caller_authority_is_distinct_from_unknown_model_confidence() {
    let (artifact, canonical) =
        fuse_transcript_stage(&[transcript(TranscriptAuthorityV1::CallerCanonical)], None).unwrap();
    assert_eq!(artifact.confidence, None);
    assert_eq!(canonical.confidence, None);
    assert_eq!(canonical.authority, LyricsAuthority::CallerCanonical);
    assert!(artifact.model_sha256.is_none());
    assert_eq!(canonical.tokens[0].id.as_deref(), Some("caller-1"));
}

#[test]
fn caller_timed_lyric_ranges_survive_transcript_fusion() {
    let (_, mut canonical) =
        fuse_transcript_stage(&[transcript(TranscriptAuthorityV1::CallerCanonical)], None).unwrap();
    let lyrics = crate::contract::LyricsV1 {
        mode: crate::contract::LyricsMode::Canonical,
        language: Some("en".to_string()),
        tokens: vec![
            crate::contract::LyricTokenV1 {
                id: "caller-1".to_string(),
                text: "sing".to_string(),
                reading: None,
                phonemes: None,
                start: Some(100_000),
                end: Some(500_000),
            },
            crate::contract::LyricTokenV1 {
                id: "caller-2".to_string(),
                text: "now".to_string(),
                reading: None,
                phonemes: None,
                start: Some(500_000),
                end: Some(900_000),
            },
        ],
    };

    attach_caller_lyric_ranges(&mut canonical, &lyrics);

    assert_eq!(
        canonical.tokens[0].range,
        Some(TimeRange::new(100_000, 500_000).unwrap())
    );
    assert_eq!(
        canonical.tokens[1].range,
        Some(TimeRange::new(500_000, 900_000).unwrap())
    );
}

#[test]
fn timed_lyric_line_owns_notes_outside_a_collapsed_alignment_span() {
    let transcript = CanonicalLyrics {
        text: "霞む景色の中に滲んで".to_string(),
        language: Some("ja".to_string()),
        authority: LyricsAuthority::CallerCanonical,
        tokens: vec![TranscriptTokenEvidence {
            id: Some("line-14".to_string()),
            text: "霞む景色の中に滲んで".to_string(),
            range: Some(TimeRange::new(157_860_000, 163_810_000).unwrap()),
            confidence: None,
        }],
        confidence: None,
        source_experts: vec!["caller.canonical_lyrics".to_string()],
        alternatives: Vec::new(),
    };
    let words = vec![CanonicalWordBoundary {
        word_id: "word-122".to_string(),
        text: "霞む景色の中に滲んで".to_string(),
        range: TimeRange::new(159_600_000, 159_760_000).unwrap(),
        confidence: None,
        disagreement: None,
        source_experts: vec!["qwen3_forced_aligner_0_6b".to_string()],
    }];

    assert_eq!(
        timed_lyric_word_owner(
            TimeRange::new(158_200_000, 158_500_000).unwrap(),
            &transcript,
            &words,
        ),
        Some("word-122")
    );
    assert_eq!(
        timed_lyric_word_owner(
            TimeRange::new(161_000_000, 161_250_000).unwrap(),
            &transcript,
            &words,
        ),
        Some("word-122")
    );
    assert_eq!(
        timed_lyric_word_owner(
            TimeRange::new(164_000_000, 164_250_000).unwrap(),
            &transcript,
            &words,
        ),
        None
    );
}

#[test]
fn selected_unlinked_notes_are_published_through_timed_lyric_ownership() {
    let (transcript, mut lyrics) =
        fuse_transcript_stage(&[transcript(TranscriptAuthorityV1::CallerCanonical)], None).unwrap();
    attach_caller_lyric_ranges(
        &mut lyrics,
        &crate::contract::LyricsV1 {
            mode: crate::contract::LyricsMode::Canonical,
            language: Some("en".to_string()),
            tokens: vec![
                crate::contract::LyricTokenV1 {
                    id: "caller-1".to_string(),
                    text: "sing".to_string(),
                    reading: None,
                    phonemes: None,
                    start: Some(0),
                    end: Some(500_000),
                },
                crate::contract::LyricTokenV1 {
                    id: "caller-2".to_string(),
                    text: "now".to_string(),
                    reading: None,
                    phonemes: None,
                    start: Some(500_000),
                    end: Some(1_000_000),
                },
            ],
        },
    );
    let (alignment, words) = fuse_alignment_stage(&lyrics, &[alignment()], 0, 1_000_000).unwrap();
    let mut fusion = execute_singing_fusion_stage(
        &transcript,
        &alignment,
        &words,
        Some(&pitch(false)),
        None,
        None,
        Some(&game()),
        Some(&acoustic()),
        &[],
        &[],
        &[],
        0,
        1_000_000,
        "rmvpe",
    )
    .unwrap();
    for candidate in &mut fusion.fusion.candidates {
        candidate.word_id = None;
    }

    let output =
        execute_candidate_graph_stage(lyrics, words, fusion, FusionDecisionModeV1::Algorithm)
            .unwrap();

    assert!(!output.track.notes.is_empty());
    assert!(output.track.notes.iter().all(|note| note.word_id.is_some()));
}

#[test]
fn generated_unknown_confidence_and_reference_alternative_remain_truthful() {
    let (artifact, canonical) = fuse_transcript_stage(
        &[transcript(TranscriptAuthorityV1::Generated)],
        Some("reference only"),
    )
    .unwrap();
    assert_eq!(canonical.text, "sing now");
    assert_eq!(canonical.confidence, None);
    assert!(canonical.tokens.is_empty());
    assert!(artifact.tokens.is_empty());
    assert_eq!(canonical.alternatives, ["reference only"]);
    assert_eq!(artifact.model_sha256, Some("a".repeat(64)));
}

#[test]
fn reference_sequence_reconciliation_corrects_identity_without_claiming_caller_authority() {
    let (artifact, canonical) = fuse_transcript_stage(
        &[transcript(TranscriptAuthorityV1::Generated)],
        Some("sing know"),
    )
    .unwrap();
    assert_eq!(artifact.text, "sing know");
    assert_eq!(canonical.text, "sing know");
    assert_eq!(canonical.authority, LyricsAuthority::Generated);
    assert!(
        canonical
            .source_experts
            .iter()
            .any(|source| source == "caller.reference")
    );
    assert_eq!(canonical.alternatives, ["sing now"]);
}

#[test]
fn transcript_disagreement_regions_are_typed_and_source_bounded() {
    let mut generated = transcript(TranscriptAuthorityV1::Generated);
    generated.language = Some("en-US".to_string());
    generated.confidence = Some(0.4);
    let regions = build_transcript_disagreement_regions(
        &generated,
        Some("唱歌 现在 很好"),
        Some("zh-CN"),
        TimeRange::new(10, 1_000_010).unwrap(),
    );
    assert_eq!(regions.len(), 1);
    assert_eq!(regions[0].range, TimeRange::new(10, 1_000_010).unwrap());
    assert!(
        regions[0]
            .reasons
            .contains(&SingingReviewReason::TranscriptLowConfidence)
    );
    assert!(
        regions[0]
            .reasons
            .contains(&SingingReviewReason::TranscriptReferenceMismatch)
    );
    assert!(
        regions[0]
            .reasons
            .contains(&SingingReviewReason::TranscriptLanguageMismatch)
    );
}

#[test]
fn transcript_challenger_enters_fusion_without_winning_an_unknown_score_tie() {
    let primary = transcript(TranscriptAuthorityV1::Generated);
    let mut challenger = primary.clone();
    challenger.text = "alpha alternative".to_string();
    challenger.source_experts = vec!["firered_asr2_aed".to_string()];
    let (artifact, canonical) = fuse_transcript_stage(&[primary, challenger], None).unwrap();
    assert_eq!(artifact.text, "sing now");
    assert_eq!(canonical.alternatives, ["alpha alternative"]);
}

#[test]
fn alignment_unknown_confidence_is_preserved_and_overlap_fails_closed() {
    let (_, lyrics) =
        fuse_transcript_stage(&[transcript(TranscriptAuthorityV1::Generated)], None).unwrap();
    let (_, words) = fuse_alignment_stage(&lyrics, &[alignment()], 0, 1_000_000).unwrap();
    assert_eq!(words[0].confidence, None);
    assert_eq!(words[0].word_id, "word-0");
    let mut invalid = alignment();
    invalid.items[1].start = 300_000;
    assert!(fuse_alignment_stage(&lyrics, &[invalid], 0, 1_000_000).is_err());
}

#[test]
fn rmvpe_projection_keeps_voiced_f0_and_unvoiced_gaps() {
    let evidence = PitchEvidenceV03 {
        format: "uta.pitch-evidence".to_string(),
        format_version: "0.3.0".to_string(),
        timebase: 1_000_000,
        start: 2_000_000,
        hop: 10_000,
        frequency_hz: vec![Some(439.7), None, Some(440.1)],
        confidence: vec![Some(0.9), Some(0.1), Some(0.8)],
        model: BTreeMap::new(),
    };
    let points = project_rmvpe_f0(&evidence).unwrap();
    assert_eq!(points.len(), 2);
    assert_eq!(points[0].time, 2_000_000);
    assert_eq!(points[1].time, 2_020_000);
    assert_eq!(points[0].confidence, Some(0.9));
}

#[test]
fn f0_fallback_regions_respect_canonical_word_edges() {
    let words = vec![
        CanonicalWordBoundary {
            word_id: "word-0".to_string(),
            text: "sing".to_string(),
            range: TimeRange::new(100_000, 300_000).unwrap(),
            confidence: None,
            disagreement: None,
            source_experts: vec!["aligner".to_string()],
        },
        CanonicalWordBoundary {
            word_id: "word-1".to_string(),
            text: "now".to_string(),
            range: TimeRange::new(300_000, 500_000).unwrap(),
            confidence: None,
            disagreement: None,
            source_experts: vec!["aligner".to_string()],
        },
    ];
    assert_eq!(
        split_f0_range_at_word_edges(100_000, 500_000, &words),
        [(100_000, 300_000), (300_000, 500_000)]
    );
}

#[test]
fn caller_phrase_constraints_emit_one_confidence_weighted_soft_start() {
    let phrase = BoundaryConstraintV1 {
        token_id: Some("phrase-1".to_string()),
        level: BoundaryLevel::Phrase,
        start: 100_000,
        duration: 400_000,
        confidence: 0.6,
        authority: BoundaryAuthority::Soft,
        source: "user".to_string(),
    };
    let (alternatives, starts) = boundary_constraint_events(&[phrase.clone()], 0, 1_000_000)
        .expect("valid soft phrase constraint");
    assert_eq!(alternatives.len(), 1);
    assert_eq!(starts.len(), 1);
    assert_eq!(starts[0].kind, BoundaryConstraintKindV1::PhraseStart);
    assert_eq!(starts[0].time, phrase.start);
    assert_eq!(starts[0].source_local_strength, Some(0.6));

    let mut hard_phrase = phrase.clone();
    hard_phrase.authority = BoundaryAuthority::Hard;
    let (hard_alternatives, hard_starts) =
        boundary_constraint_events(&[hard_phrase], 0, 1_000_000).unwrap();
    assert!(hard_alternatives[0].hard);
    assert!(hard_starts.is_empty());

    let mut word = phrase;
    word.level = BoundaryLevel::Word;
    let (_, word_starts) = boundary_constraint_events(&[word], 0, 1_000_000).unwrap();
    assert!(word_starts.is_empty());
}

#[test]
fn f0_derived_lengths_work_with_game_and_acoustic_disabled() {
    let (transcript, _lyrics, alignment, words, pitch) = fused_inputs(false);
    let output = execute_singing_fusion_stage(
        &transcript,
        &alignment,
        &words,
        Some(&pitch),
        None,
        None,
        None,
        None,
        &[],
        &[],
        &[],
        0,
        1_000_000,
        "rmvpe",
    )
    .unwrap();
    assert_eq!(output.fusion.candidates.len(), 2);
    assert!(
        output
            .fusion
            .candidates
            .iter()
            .all(|candidate| candidate.acoustic.is_none())
    );
    assert!(
        output
            .provenance
            .iter()
            .any(|item| item.expert_id == "rmvpe.f0_segmentation")
    );
    assert!(output.fusion.candidates.iter().all(|candidate| {
        candidate.boundary_kind == BoundaryEvidenceKind::F0Derived
            && candidate.boundary_fractional_midi.is_none()
            && candidate.target_pitch_source == "rmvpe"
    }));
    assert!(!output.f0_curve.is_empty());
}

#[test]
fn f0_derived_lengths_reject_only_low_confidence_voiced_geometry() {
    let (transcript, _lyrics, alignment, words, mut pitch) = fused_inputs(false);
    for (frequency_hz, confidence) in pitch.frequency_hz.iter().zip(pitch.confidence.iter_mut()) {
        if frequency_hz.is_some() {
            *confidence = Some(0.01);
        }
    }
    let result = execute_singing_fusion_stage(
        &transcript,
        &alignment,
        &words,
        Some(&pitch),
        None,
        None,
        None,
        None,
        &[],
        &[],
        &[],
        0,
        1_000_000,
        "rmvpe",
    );
    let Err(error) = result else {
        panic!("low-confidence-only F0 unexpectedly produced note geometry");
    };
    assert!(error.message.contains("trustworthy voiced evidence"));
}

#[test]
fn full_typed_pipeline_is_deterministic_non_overlapping_and_uses_acoustic() {
    let (transcript, lyrics, alignment, words, pitch) = fused_inputs(false);
    let run = || {
        execute_singing_stages(
            &transcript,
            lyrics.clone(),
            &alignment,
            words.clone(),
            Some(&pitch),
            &game(),
            &acoustic(),
        )
        .unwrap()
    };
    let first = run();
    let second = run();
    assert_eq!(first.track, second.track);
    assert_eq!(first.track.notes.len(), 2);
    assert!(
        first.review_regions.is_empty(),
        "unknown confidence alone must not turn every clean note into a review region"
    );
    assert!(
        first
            .track
            .notes
            .windows(2)
            .all(|pair| pair[0].range.end <= pair[1].range.start)
    );
    let note = &first.track.notes[0];
    assert_eq!(note.midi_note, 69);
    assert_eq!(note.evidence.decision_trace.policy_version, 3);
    assert_eq!(
        note.evidence.decision_trace.selected_target_pitch_source,
        "rmvpe"
    );
    assert_eq!(
        note.evidence.decision_trace.pitch_selection_reason,
        crate::fusion::PitchSelectionReasonV1::GlobalPitchAlternative
    );
    assert_eq!(note.evidence.boundary_fractional_midi, Some(69.25));
    assert_eq!(note.confidence, None);
    assert_eq!(note.evidence.rmvpe_voiced_ratio, Some(1.0));
    assert!(note.evidence.rmvpe_pitch_mad_cents.unwrap().abs() < 0.001);
    assert_eq!(
        note.evidence
            .acoustic
            .as_ref()
            .and_then(|features| features.onset_supported),
        Some(true)
    );
    assert_eq!(note.center_pitch_hz, 440.0);
    assert!(note.center_offset_cents.abs() < 0.001);
    assert!(!note.f0_curve.is_empty());
}

#[test]
fn configured_acoustic_and_basic_pitch_both_survive_candidate_construction() {
    let (transcript, _lyrics, alignment, words, pitch) = fused_inputs(false);
    let basic_pitch = basic_pitch();
    let output = execute_singing_fusion_stage(
        &transcript,
        &alignment,
        &words,
        Some(&pitch),
        None,
        Some(&basic_pitch),
        Some(&game()),
        Some(&acoustic()),
        &[],
        &[],
        &[],
        0,
        1_000_000,
        "rmvpe",
    )
    .unwrap();
    assert!(
        output
            .fusion
            .candidates
            .iter()
            .all(|candidate| { candidate.acoustic.is_some() && candidate.basic_pitch.is_some() })
    );
}

#[test]
fn fcpe_primary_provenance_and_uncertainty_ignore_sparse_rmvpe_quality() {
    let (transcript, lyrics, alignment, words, mut rmvpe) = fused_inputs(false);
    for index in 11..40 {
        rmvpe.frequency_hz[index] = None;
    }
    let fcpe = pitch(false);
    let fusion = execute_singing_fusion_stage(
        &transcript,
        &alignment,
        &words,
        Some(&rmvpe),
        Some(&fcpe),
        None,
        Some(&game()),
        Some(&acoustic()),
        &[],
        &[],
        &[],
        0,
        1_000_000,
        "fcpe",
    )
    .unwrap();
    let output =
        execute_candidate_graph_stage(lyrics, words, fusion, FusionDecisionModeV1::Algorithm)
            .unwrap();
    assert!(
        output
            .track
            .notes
            .iter()
            .all(|note| { note.evidence.decision_trace.continuous_f0_source == "fcpe" })
    );
    assert!(output.review_regions.iter().all(|region| {
        !region
            .reasons
            .contains(&SingingReviewReason::LowPitchCoverage)
            && !region
                .reasons
                .contains(&SingingReviewReason::PitchInstability)
    }));
}

#[test]
fn sparse_rmvpe_coverage_is_reviewed_without_claiming_pitch_disagreement() {
    let (transcript, lyrics, alignment, words, mut pitch) = fused_inputs(false);
    for index in 11..40 {
        pitch.frequency_hz[index] = None;
    }
    let output = execute_singing_stages(
        &transcript,
        lyrics,
        &alignment,
        words,
        Some(&pitch),
        &game(),
        &acoustic(),
    )
    .unwrap();
    let note = &output.track.notes[0];
    assert!(note.uncertain);
    assert!((note.evidence.rmvpe_voiced_ratio.unwrap() - (1.0 / 30.0)).abs() < 1.0e-6);
    assert!(output.review_regions.iter().any(|region| {
        region
            .reasons
            .contains(&SingingReviewReason::LowPitchCoverage)
            && !region
                .reasons
                .contains(&SingingReviewReason::PitchDisagreement)
    }));
}

#[test]
fn measured_boundary_disagreement_creates_review_without_fake_probability() {
    let (transcript, lyrics, alignment, words, pitch) = fused_inputs(false);
    let mut acoustic = acoustic();
    for frame in &mut acoustic.frames {
        frame.spectral_flux = (frame.start > 0).then_some(0.01);
    }
    let output = execute_singing_stages(
        &transcript,
        lyrics,
        &alignment,
        words,
        Some(&pitch),
        &game(),
        &acoustic,
    )
    .unwrap();
    assert_eq!(
        output.track.notes[0]
            .evidence
            .acoustic
            .as_ref()
            .and_then(|features| features.onset_supported),
        Some(false)
    );
    assert!(output.review_regions.iter().any(|region| {
        region
            .reasons
            .contains(&SingingReviewReason::BoundaryDisagreement)
            && region.confidence.is_none()
    }));
}

#[test]
fn octave_disagreement_is_reviewed_without_quantizing_rmvpe_to_a_target() {
    let (transcript, lyrics, alignment, words, pitch) = fused_inputs(true);
    let output = execute_singing_stages(
        &transcript,
        lyrics,
        &alignment,
        words,
        Some(&pitch),
        &game(),
        &acoustic(),
    )
    .unwrap();
    assert_eq!(output.track.notes[0].midi_note, 69);
    assert_eq!(output.track.notes[0].alternatives[0].center_hz, 880.0);
    assert!(
        output
            .review_regions
            .iter()
            .any(|region| { region.reasons.contains(&SingingReviewReason::OctaveRisk) })
    );
}

#[test]
fn missing_game_and_non_finite_pitch_fail_closed() {
    let (transcript, lyrics, alignment, words, mut pitch) = fused_inputs(false);
    let mut missing = game();
    missing.notes.clear();
    assert!(
        execute_singing_stages(
            &transcript,
            lyrics.clone(),
            &alignment,
            words.clone(),
            Some(&pitch),
            &missing,
            &acoustic(),
        )
        .is_err()
    );
    pitch.frequency_hz[10] = Some(f64::NAN);
    assert!(
        execute_singing_stages(
            &transcript,
            lyrics.clone(),
            &alignment,
            words.clone(),
            Some(&pitch),
            &game(),
            &acoustic(),
        )
        .is_err()
    );
    let mut malformed_game = game();
    malformed_game.notes[0].midi = f32::NAN;
    assert!(
        execute_singing_stages(
            &transcript,
            lyrics,
            &alignment,
            words,
            None,
            &malformed_game,
            &acoustic(),
        )
        .is_err()
    );
}

#[test]
fn game_evidence_builds_primary_and_contextual_candidates() {
    let (transcript, _lyrics, alignment, words, pitch) = fused_inputs(false);
    let game = game();
    let output = execute_singing_fusion_stage(
        &transcript,
        &alignment,
        &words,
        Some(&pitch),
        None,
        None,
        Some(&game),
        None,
        &[],
        &[],
        &[],
        0,
        1_000_000,
        "rmvpe",
    )
    .unwrap();
    assert!(output.fusion.candidates.len() >= game.notes.len());
    assert!(output.fusion.candidates.iter().any(|candidate| {
        candidate.boundary_kind == BoundaryEvidenceKind::Game && candidate.boundary_source == "game"
    }));
    assert!(output.fusion.candidates.iter().all(|candidate| {
        candidate.boundary_source == "game" || candidate.boundary_kind != BoundaryEvidenceKind::Game
    }));
}

#[test]
fn aligned_word_boundaries_split_f0_region_candidates() {
    let (_, _, _, words, _) = fused_inputs(false);
    let curve = (0..100)
        .map(|index| F0Point {
            time: index * 10_000,
            hz: 440.0,
            confidence: Some(0.9),
        })
        .collect::<Vec<_>>();
    let regions =
        derive_f0_length_evidence(&curve, &words, 0, 1_000_000, "rmvpe", None, None).unwrap();
    for boundary in [100_000, 400_000, 500_000, 900_000] {
        assert!(
            regions
                .segments
                .iter()
                .any(|region| region.range.start == boundary || region.range.end == boundary),
            "aligned boundary {boundary} must participate in candidate generation"
        );
    }
}

#[test]
fn f0_derived_lengths_ignore_a_single_frame_octave_outlier_but_keep_a_sustained_leap() {
    let curve = |sustained: bool| {
        (0..40)
            .map(|index| F0Point {
                time: index * 10_000,
                hz: if (sustained && index >= 20) || (!sustained && index == 20) {
                    880.0
                } else {
                    440.0
                },
                confidence: Some(0.9),
            })
            .collect::<Vec<_>>()
    };
    let outlier =
        derive_f0_length_evidence(&curve(false), &[], 0, 400_000, "rmvpe", None, None).unwrap();
    assert_eq!(outlier.segments.len(), 1);
    assert_eq!(
        outlier.segments[0].range,
        TimeRange::new(0, 400_000).unwrap()
    );
    assert!(
        context_boundary_constraints(&[], &curve(false), "rmvpe", None, None)
            .iter()
            .all(|constraint| constraint.kind != BoundaryConstraintKindV1::PitchDiscontinuity),
        "one-frame octave noise must not leak back as contextual discontinuity evidence"
    );

    let leap =
        derive_f0_length_evidence(&curve(true), &[], 0, 400_000, "rmvpe", None, None).unwrap();
    assert_eq!(leap.segments.len(), 2);
    assert_eq!(leap.segments[0].range.end, 200_000);
    assert_eq!(leap.segments[1].range.start, 200_000);
    let discontinuities = context_boundary_constraints(&[], &curve(true), "rmvpe", None, None)
        .into_iter()
        .filter(|constraint| constraint.kind == BoundaryConstraintKindV1::PitchDiscontinuity)
        .collect::<Vec<_>>();
    assert_eq!(discontinuities.len(), 1);
    assert_eq!(discontinuities[0].time, 200_000);
}

#[test]
fn f0_derived_selected_notes_keep_typed_source_identity_and_uncertainty() {
    let (transcript, lyrics, alignment, words, pitch) = fused_inputs(false);
    let fusion = execute_singing_fusion_stage(
        &transcript,
        &alignment,
        &words,
        Some(&pitch),
        None,
        None,
        None,
        None,
        &[],
        &[],
        &[],
        0,
        1_000_000,
        "rmvpe",
    )
    .unwrap();
    let output =
        execute_candidate_graph_stage(lyrics, words, fusion, FusionDecisionModeV1::Algorithm)
            .unwrap();
    assert!(matches!(
        output.decision,
        CandidatePathDecisionV1::Algorithm { .. }
    ));
    assert!(!output.track.notes.is_empty());
    assert!(output.track.notes.iter().all(|note| note.uncertain));
    assert!(output.track.notes.iter().all(|note| {
        note.evidence.boundary_kind != BoundaryEvidenceKind::Game
            && note.evidence.boundary_source != "game"
    }));
}

#[cfg(unix)]
#[test]
fn algorithm_and_ai_selectors_receive_the_identical_candidate_pool() {
    let (transcript, lyrics, alignment, words, pitch) = fused_inputs(false);
    let build_fusion = || {
        execute_singing_fusion_stage(
            &transcript,
            &alignment,
            &words,
            Some(&pitch),
            None,
            None,
            Some(&game()),
            Some(&acoustic()),
            &[],
            &[],
            &[],
            0,
            1_000_000,
            "rmvpe",
        )
        .unwrap()
    };
    let algorithm = execute_candidate_graph_stage(
        lyrics.clone(),
        words.clone(),
        build_fusion(),
        FusionDecisionModeV1::Algorithm,
    )
    .unwrap();
    let (algorithm_digest, selected_ids) = match &algorithm.decision {
        CandidatePathDecisionV1::Algorithm {
            candidate_set_digest,
            selected_candidate_ids,
        } => (candidate_set_digest.clone(), selected_candidate_ids.clone()),
        _ => unreachable!(),
    };
    let selected = selected_ids
        .iter()
        .map(|id| {
            algorithm
                .fusion
                .candidates
                .iter()
                .find(|candidate| &candidate.id == id)
                .unwrap()
                .clone()
        })
        .collect::<Vec<_>>();
    let response = serde_json::json!({
        "contract": "uta.fusion_agent_response",
        "version": crate::contract::FUSION_AGENT_PROTOCOL_VERSION,
        "selected": selected,
    });
    let root = std::env::temp_dir().join(format!(
        "uta-selector-pool-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    std::fs::create_dir_all(&root).unwrap();
    let adapter = root.join("uta-fusion-agent-adapter");
    write_executable(
        &adapter,
        &format!("#!/bin/sh\ncat <<'UTA_RESPONSE'\n{response}\nUTA_RESPONSE\n"),
    );
    let cancellation = CancellationToken::default();
    let ai_fusion = build_fusion();
    let ai = execute_candidate_graph_stage(
        lyrics,
        words,
        ai_fusion,
        FusionDecisionModeV1::AiJudgment {
            executable: &adapter,
            timeout: std::time::Duration::from_secs(5),
            cancellation: &cancellation,
        },
    )
    .unwrap();
    let ai_digest = match &ai.decision {
        CandidatePathDecisionV1::AiJudgment {
            candidate_set_digest,
            ..
        } => candidate_set_digest,
        _ => unreachable!(),
    };
    assert_eq!(ai_digest, &algorithm_digest);
    assert_eq!(ai.fusion.candidates, algorithm.fusion.candidates);
    assert_eq!(ai.track, algorithm.track);
    std::fs::remove_dir_all(root).unwrap();
}

#[cfg(unix)]
#[test]
fn ai_adapter_failure_is_returned_without_algorithm_fallback() {
    let (transcript, lyrics, alignment, words, pitch) = fused_inputs(false);
    let fusion = execute_singing_fusion_stage(
        &transcript,
        &alignment,
        &words,
        Some(&pitch),
        None,
        None,
        None,
        None,
        &[],
        &[],
        &[],
        0,
        1_000_000,
        "rmvpe",
    )
    .unwrap();
    let root = std::env::temp_dir().join(format!(
        "uta-ai-no-fallback-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    std::fs::create_dir_all(&root).unwrap();
    let marker = root.join("provider-contacted");
    let adapter = root.join("uta-fusion-agent-adapter");
    write_executable(
        &adapter,
        &format!(
            "#!/bin/sh\nprintf contacted > '{}'\nprintf '%s\\n' '{{\"contract\":\"uta.fusion_agent_response\",\"version\":4,\"selected\":[]}}'\n",
            marker.display()
        ),
    );
    let cancellation = CancellationToken::default();
    let error = match execute_candidate_graph_stage(
        lyrics,
        words,
        fusion,
        FusionDecisionModeV1::AiJudgment {
            executable: &adapter,
            timeout: std::time::Duration::from_secs(5),
            cancellation: &cancellation,
        },
    ) {
        Ok(_) => panic!("an invalid adapter response must not fall back to Algorithm"),
        Err(error) => error,
    };
    assert_eq!(
        error.code,
        crate::contract::EngineErrorCode::OutputValidationFailed
    );
    assert!(
        marker.exists(),
        "the configured adapter path must be exercised"
    );
    std::fs::remove_dir_all(root).unwrap();
}
