use crate::artifact::{
    AcousticEvidenceV1, AdvancedNoteEvidenceV1, AlignmentArtifactV1, AlignmentItemV1,
    BasicPitchEvidenceV3, GameEvidenceV1, PitchEvidenceV03, TranscriptArtifactV1,
    TranscriptAuthorityV1, TranscriptTokenV1,
};
use crate::contract::{
    BoundaryAuthority, BoundaryLevel, CANONICAL_TIMEBASE, EngineError, EngineErrorCode,
    EngineResult,
};
use crate::fusion::{
    CanonicalLyrics, CanonicalSingingTrack, CanonicalWordBoundary, EvidenceProvenance, ExpertTask,
    F0Point, HarmonyMetadata, LyricsAuthority, PitchGrid, SingingFusionEvidence,
    SingingReviewRegion, TranscriptHypothesis, TranscriptTokenEvidence, WordBoundaryEvidence,
    build_canonical_singing_track, build_review_regions, decode_candidate_graph,
    fuse_singing_evidence, fuse_transcripts, fuse_word_boundaries,
};

fn output_error(message: impl Into<String>) -> EngineError {
    EngineError::new(EngineErrorCode::OutputValidationFailed, message)
}

fn normalized(text: &str) -> String {
    text.split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .to_lowercase()
}

fn compact_normalized(text: &str) -> String {
    text.chars()
        .filter(|character| !character.is_whitespace())
        .flat_map(char::to_lowercase)
        .collect()
}

fn transcript_tokens(artifact: &TranscriptArtifactV1) -> Vec<TranscriptTokenEvidence> {
    artifact
        .tokens
        .iter()
        .map(|token| TranscriptTokenEvidence {
            id: Some(token.id.clone()),
            text: token.text.clone(),
            range: None,
            confidence: token.confidence,
        })
        .collect()
}

pub fn fuse_transcript_stage(
    evidence: &[TranscriptArtifactV1],
    reference_lyrics: Option<&str>,
) -> EngineResult<(TranscriptArtifactV1, CanonicalLyrics)> {
    if evidence.is_empty() {
        return Err(output_error(
            "fusion.transcript requires transcript evidence",
        ));
    }
    for artifact in evidence {
        artifact.validate()?;
    }
    let caller = evidence
        .iter()
        .filter(|artifact| artifact.authority == TranscriptAuthorityV1::CallerCanonical)
        .collect::<Vec<_>>();
    let (mut artifact, mut canonical) = if !caller.is_empty() {
        if caller.len() != 1 || evidence.len() != 1 {
            return Err(output_error(
                "caller-canonical lyrics cannot be ranked against generated text",
            ));
        }
        let artifact = caller[0].clone();
        let canonical = CanonicalLyrics {
            text: artifact.text.clone(),
            language: artifact.language.clone(),
            authority: LyricsAuthority::CallerCanonical,
            tokens: transcript_tokens(&artifact),
            confidence: None,
            source_experts: artifact.source_experts.clone(),
            alternatives: artifact.alternatives.clone(),
        };
        (artifact, canonical)
    } else {
        let hypotheses = evidence
            .iter()
            .map(|artifact| TranscriptHypothesis {
                expert_id: artifact.source_experts.join("+"),
                language: artifact.language.clone(),
                text: artifact.text.clone(),
                tokens: transcript_tokens(artifact),
                confidence: artifact.confidence,
                correlation_group: None,
                dependencies: Vec::new(),
            })
            .collect::<Vec<_>>();
        let canonical = fuse_transcripts(&hypotheses).map_err(output_error)?;
        let representative = evidence
            .iter()
            .find(|artifact| {
                artifact.source_experts.iter().any(|source| {
                    canonical
                        .source_experts
                        .iter()
                        .any(|winner| winner.contains(source))
                }) && normalized(&artifact.text) == normalized(&canonical.text)
            })
            .ok_or_else(|| output_error("transcript fusion lost representative provenance"))?;
        let artifact = TranscriptArtifactV1 {
            contract: representative.contract.clone(),
            version: representative.version,
            authority: TranscriptAuthorityV1::Generated,
            language: canonical.language.clone(),
            text: canonical.text.clone(),
            tokens: canonical
                .tokens
                .iter()
                .enumerate()
                .map(|(index, token)| TranscriptTokenV1 {
                    id: token
                        .id
                        .clone()
                        .unwrap_or_else(|| format!("transcript-{index}")),
                    text: token.text.clone(),
                    confidence: token.confidence,
                })
                .collect(),
            confidence: canonical.confidence,
            source_experts: canonical.source_experts.clone(),
            alternatives: canonical.alternatives.clone(),
            model_sha256: representative.model_sha256.clone(),
            runtime_manifest_sha256: representative.runtime_manifest_sha256.clone(),
            backend: representative.backend.clone(),
        };
        (artifact, canonical)
    };

    if let Some(reference) = reference_lyrics
        .map(str::trim)
        .filter(|text| !text.is_empty())
        && normalized(reference) != normalized(&artifact.text)
        && !artifact
            .alternatives
            .iter()
            .any(|item| normalized(item) == normalized(reference))
    {
        artifact.alternatives.push(reference.to_string());
        canonical.alternatives.push(reference.to_string());
    }
    artifact.validate()?;
    Ok((artifact, canonical))
}

pub fn fuse_alignment_stage(
    transcript: &CanonicalLyrics,
    evidence: &[AlignmentArtifactV1],
    source_start: u64,
    source_duration: u64,
) -> EngineResult<(AlignmentArtifactV1, Vec<CanonicalWordBoundary>)> {
    if evidence.is_empty() {
        return Err(output_error("fusion.alignment requires alignment evidence"));
    }
    let source_end = source_start
        .checked_add(source_duration)
        .ok_or_else(|| output_error("alignment source timeline overflows"))?;
    let mut projected = Vec::new();
    for artifact in evidence {
        if artifact.contract != "uta.analysis-engine.alignment"
            || artifact.version != 1
            || normalized(&artifact.transcript) != normalized(&transcript.text)
            || artifact.source_expert.trim().is_empty()
        {
            return Err(output_error(
                "alignment evidence does not correspond to the canonical transcript",
            ));
        }
        for item in &artifact.items {
            let end = item
                .start
                .checked_add(item.duration)
                .ok_or_else(|| output_error("alignment boundary overflows"))?;
            if item.level != BoundaryLevel::Word
                || item.authority != BoundaryAuthority::Soft
                || item.start < source_start
                || end > source_end
            {
                return Err(output_error(
                    "alignment boundary is outside the source timeline",
                ));
            }
            projected.push(WordBoundaryEvidence {
                word_id: item.id.clone(),
                text: item.text.clone(),
                range: crate::fusion::TimeRange {
                    start: item.start,
                    end,
                },
                confidence: item.confidence,
                expert_id: artifact.source_expert.clone(),
                correlation_group: None,
                dependencies: Vec::new(),
            });
        }
    }
    let words = fuse_word_boundaries(&projected).map_err(output_error)?;
    let aligned_text = words
        .iter()
        .map(|word| word.text.as_str())
        .collect::<String>();
    if compact_normalized(&aligned_text) != compact_normalized(&transcript.text) {
        return Err(output_error(
            "canonical alignment words do not correspond to the canonical transcript",
        ));
    }
    let representative = &evidence[0];
    let artifact = AlignmentArtifactV1 {
        contract: representative.contract.clone(),
        version: representative.version,
        transcript: transcript.text.clone(),
        language: transcript.language.clone(),
        items: words
            .iter()
            .map(|word| AlignmentItemV1 {
                id: word.word_id.clone(),
                text: word.text.clone(),
                level: BoundaryLevel::Word,
                start: word.range.start,
                duration: word.range.end - word.range.start,
                confidence: word.confidence,
                authority: BoundaryAuthority::Soft,
            })
            .collect(),
        source_expert: words
            .iter()
            .flat_map(|word| word.source_experts.iter().cloned())
            .collect::<std::collections::BTreeSet<_>>()
            .into_iter()
            .collect::<Vec<_>>()
            .join("+"),
        model_sha256: representative.model_sha256.clone(),
        runtime_manifest_sha256: representative.runtime_manifest_sha256.clone(),
        backend: representative.backend.clone(),
    };
    Ok((artifact, words))
}

pub fn project_pitch_f0(evidence: &PitchEvidenceV03) -> EngineResult<Vec<F0Point>> {
    if evidence.format != "uta.pitch-evidence"
        || evidence.format_version != "0.3.0"
        || evidence.timebase != u64::from(CANONICAL_TIMEBASE)
        || evidence.hop == 0
        || evidence.frequency_hz.len() != evidence.confidence.len()
    {
        return Err(output_error("pitch evidence contract or shape is invalid"));
    }
    let mut points = Vec::new();
    for (index, (frequency, confidence)) in evidence
        .frequency_hz
        .iter()
        .zip(&evidence.confidence)
        .enumerate()
    {
        if confidence.is_some_and(|value| !value.is_finite() || !(0.0..=1.0).contains(&value)) {
            return Err(output_error("pitch evidence confidence is invalid"));
        }
        let time = (index as u64)
            .checked_mul(evidence.hop)
            .and_then(|offset| evidence.start.checked_add(offset))
            .ok_or_else(|| output_error("pitch evidence timeline overflows"))?;
        if let Some(frequency) = frequency {
            if !frequency.is_finite() || *frequency <= 0.0 || *frequency > f64::from(f32::MAX) {
                return Err(output_error("pitch evidence frequency is invalid"));
            }
            points.push(F0Point {
                time,
                hz: *frequency as f32,
                confidence: confidence.map(|value| value as f32),
            });
        }
    }
    if points.windows(2).any(|pair| pair[0].time >= pair[1].time) {
        return Err(output_error("projected F0 is not strictly ordered"));
    }
    Ok(points)
}

pub fn project_rmvpe_f0(evidence: &PitchEvidenceV03) -> EngineResult<Vec<F0Point>> {
    project_pitch_f0(evidence)
}

#[allow(clippy::too_many_arguments)]
fn provenance(
    transcript: &TranscriptArtifactV1,
    alignment: &AlignmentArtifactV1,
    pitch: Option<&PitchEvidenceV03>,
    fcpe: Option<&PitchEvidenceV03>,
    basic_pitch: Option<&BasicPitchEvidenceV3>,
    game: &GameEvidenceV1,
    acoustic: &AcousticEvidenceV1,
    advanced_notes: &[AdvancedNoteEvidenceV1],
) -> Vec<EvidenceProvenance> {
    let mut result = vec![
        EvidenceProvenance {
            expert_id: transcript.source_experts.join("+"),
            task: ExpertTask::Transcript,
            model_hash: transcript.model_sha256.clone(),
            runtime_identity: transcript.runtime_manifest_sha256.clone(),
            calibration_version: None,
            correlation_group: None,
            depends_on: Vec::new(),
        },
        EvidenceProvenance {
            expert_id: alignment.source_expert.clone(),
            task: ExpertTask::WordBoundary,
            model_hash: Some(alignment.model_sha256.clone()),
            runtime_identity: Some(alignment.runtime_manifest_sha256.clone()),
            calibration_version: None,
            correlation_group: None,
            depends_on: vec![transcript.source_experts.join("+")],
        },
        EvidenceProvenance {
            expert_id: game.model_id.clone(),
            task: ExpertTask::NoteBoundary,
            model_hash: Some(game.model_manifest_sha256.clone()),
            runtime_identity: Some(game.runtime_manifest_sha256.clone()),
            calibration_version: None,
            correlation_group: None,
            depends_on: Vec::new(),
        },
        EvidenceProvenance {
            expert_id: acoustic.algorithm.clone(),
            task: ExpertTask::Acoustic,
            model_hash: None,
            runtime_identity: Some(acoustic.decoded_audio_sha256.clone()),
            calibration_version: None,
            correlation_group: None,
            depends_on: Vec::new(),
        },
    ];
    for (expert_id, evidence) in [("rmvpe", pitch), ("fcpe", fcpe)] {
        if let Some(evidence) = evidence {
            result.push(EvidenceProvenance {
                expert_id: expert_id.to_string(),
                task: ExpertTask::ContinuousPitch,
                model_hash: evidence
                    .model
                    .get("manifest_sha256")
                    .and_then(serde_json::Value::as_str)
                    .map(str::to_string),
                runtime_identity: evidence
                    .model
                    .get("runtime_manifest_sha256")
                    .and_then(serde_json::Value::as_str)
                    .map(str::to_string),
                calibration_version: None,
                correlation_group: Some("continuous-pitch-neural".to_string()),
                depends_on: Vec::new(),
            });
        }
    }
    if let Some(evidence) = basic_pitch {
        result.push(EvidenceProvenance {
            expert_id: "basic_pitch".to_string(),
            task: ExpertTask::Onset,
            model_hash: Some(evidence.model_manifest_sha256.clone()),
            runtime_identity: Some(evidence.runtime_manifest_sha256.clone()),
            calibration_version: None,
            correlation_group: None,
            depends_on: Vec::new(),
        });
    }
    for evidence in advanced_notes {
        result.push(evidence.provenance());
        if evidence.techniques.is_some() {
            result.push(evidence.technique_provenance());
        }
    }
    result
}

pub struct SingingFusionStageOutput {
    pub fusion: SingingFusionEvidence,
    f0_curve: Vec<F0Point>,
    provenance: Vec<EvidenceProvenance>,
}

pub struct SingingStagesOutput {
    pub fusion: SingingFusionEvidence,
    pub track: CanonicalSingingTrack,
    pub review_regions: Vec<SingingReviewRegion>,
}

#[allow(clippy::too_many_arguments)]
pub fn execute_singing_fusion_stage(
    transcript_artifact: &TranscriptArtifactV1,
    alignment_artifact: &AlignmentArtifactV1,
    words: &[CanonicalWordBoundary],
    pitch_evidence: Option<&PitchEvidenceV03>,
    fcpe_evidence: Option<&PitchEvidenceV03>,
    basic_pitch_evidence: Option<&BasicPitchEvidenceV3>,
    game: &GameEvidenceV1,
    acoustic: &AcousticEvidenceV1,
    advanced_notes: &[AdvancedNoteEvidenceV1],
    source_start: u64,
    source_duration: u64,
) -> EngineResult<SingingFusionStageOutput> {
    let rmvpe_grid = pitch_evidence
        .map(|evidence| PitchGrid::new(evidence.start, evidence.hop, evidence.frequency_hz.len()))
        .transpose()
        .map_err(output_error)?;
    let fcpe_grid = fcpe_evidence
        .map(|evidence| PitchGrid::new(evidence.start, evidence.hop, evidence.frequency_hz.len()))
        .transpose()
        .map_err(output_error)?;
    let f0_curve = pitch_evidence
        .map(project_rmvpe_f0)
        .transpose()?
        .unwrap_or_default();
    let fcpe_curve = fcpe_evidence
        .map(project_pitch_f0)
        .transpose()?
        .unwrap_or_default();
    // These conditioned experts are challengers only. Validate their complete
    // canonical timeline before retaining correlated provenance, but never
    // substitute their boundaries or pitch for the required GAME baseline.
    for evidence in advanced_notes {
        evidence.canonical_notes(source_start, source_duration)?;
    }
    let fusion = fuse_singing_evidence(
        words,
        game,
        &f0_curve,
        rmvpe_grid,
        &fcpe_curve,
        fcpe_grid,
        acoustic,
        basic_pitch_evidence,
    )
    .map_err(output_error)?;
    Ok(SingingFusionStageOutput {
        fusion,
        f0_curve,
        provenance: provenance(
            transcript_artifact,
            alignment_artifact,
            pitch_evidence,
            fcpe_evidence,
            basic_pitch_evidence,
            game,
            acoustic,
            advanced_notes,
        ),
    })
}

pub fn execute_candidate_graph_stage(
    transcript: CanonicalLyrics,
    words: Vec<CanonicalWordBoundary>,
    singing: SingingFusionStageOutput,
) -> EngineResult<SingingStagesOutput> {
    let decoded = decode_candidate_graph(&singing.fusion.candidates).map_err(output_error)?;
    if decoded.is_empty() {
        return Err(output_error("candidate graph selected no GAME note states"));
    }
    let track = build_canonical_singing_track(
        transcript,
        words,
        decoded,
        singing.f0_curve,
        HarmonyMetadata::default(),
        singing.provenance,
    )
    .map_err(output_error)?;
    let review_regions = build_review_regions(&track);
    Ok(SingingStagesOutput {
        fusion: singing.fusion,
        track,
        review_regions,
    })
}

#[allow(clippy::too_many_arguments)]
pub fn build_baseline_review_regions(
    transcript_artifact: &TranscriptArtifactV1,
    transcript: CanonicalLyrics,
    alignment_artifact: &AlignmentArtifactV1,
    words: Vec<CanonicalWordBoundary>,
    pitch_evidence: Option<&PitchEvidenceV03>,
    game: &GameEvidenceV1,
    acoustic: &AcousticEvidenceV1,
    source_start: u64,
    source_duration: u64,
) -> EngineResult<Vec<SingingReviewRegion>> {
    let fusion = execute_singing_fusion_stage(
        transcript_artifact,
        alignment_artifact,
        &words,
        pitch_evidence,
        None,
        None,
        game,
        acoustic,
        &[],
        source_start,
        source_duration,
    )?;
    Ok(execute_candidate_graph_stage(transcript, words, fusion)?.review_regions)
}

#[allow(clippy::too_many_arguments)]
pub fn execute_singing_stages(
    transcript_artifact: &TranscriptArtifactV1,
    transcript: CanonicalLyrics,
    alignment_artifact: &AlignmentArtifactV1,
    words: Vec<CanonicalWordBoundary>,
    pitch_evidence: Option<&PitchEvidenceV03>,
    game: &GameEvidenceV1,
    acoustic: &AcousticEvidenceV1,
) -> EngineResult<SingingStagesOutput> {
    let fusion = execute_singing_fusion_stage(
        transcript_artifact,
        alignment_artifact,
        &words,
        pitch_evidence,
        None,
        None,
        game,
        acoustic,
        &[],
        0,
        acoustic
            .frames
            .last()
            .map_or(1, |frame| frame.start.saturating_add(acoustic.hop)),
    )?;
    execute_candidate_graph_stage(transcript, words, fusion)
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use super::*;
    use crate::artifact::{
        ACOUSTIC_EVIDENCE_CONTRACT, ACOUSTIC_EVIDENCE_VERSION, AcousticEvidenceFrameV1,
        GameNoteEvidenceV1,
    };
    use crate::fingerprint::ACOUSTIC_DSP_VERSION;
    use crate::fusion::{LyricsAuthority, SingingReviewReason, TimeRange};

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
        let (alignment, words) =
            fuse_alignment_stage(&lyrics, &[alignment()], 0, 1_000_000).unwrap();
        (
            transcript,
            lyrics,
            alignment,
            words,
            pitch(octave_disagreement),
        )
    }

    #[test]
    fn caller_authority_is_distinct_from_unknown_model_confidence() {
        let (artifact, canonical) =
            fuse_transcript_stage(&[transcript(TranscriptAuthorityV1::CallerCanonical)], None)
                .unwrap();
        assert_eq!(artifact.confidence, None);
        assert_eq!(canonical.confidence, None);
        assert_eq!(canonical.authority, LyricsAuthority::CallerCanonical);
        assert!(artifact.model_sha256.is_none());
        assert_eq!(canonical.tokens[0].id.as_deref(), Some("caller-1"));
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
        assert_eq!(note.evidence.game_fractional_midi, 69.25);
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
        assert_eq!(note.center_pitch_hz, 440.0 * 2.0_f32.powf(0.25 / 12.0));
        assert!((note.center_offset_cents - 25.0).abs() < 0.001);
        assert!(!note.f0_curve.is_empty());
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
}
