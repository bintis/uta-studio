use std::collections::{BTreeMap, HashSet};

use crate::artifact::{
    AcousticEvidence, AdvancedNoteEvidence, AlignmentArtifact, AlignmentItem, BasicPitchEvidence,
    GameEvidence, PitchEvidence, TechniqueEvidence, TimedNoteExpertEvidence, TranscriptArtifact,
    TranscriptAuthority, TranscriptToken,
};
use crate::contract::{
    BoundaryAuthority, BoundaryConstraint, BoundaryLevel, CANONICAL_TIMEBASE, EngineError,
    EngineErrorCode, EngineResult, Lyrics, LyricsMode,
};
use crate::execution::CancellationToken;
use crate::fusion::{
    BoundaryAlternative, BoundaryConstraintEvidence, BoundaryConstraintKind, BoundaryEvidenceKind,
    BoundaryEvidenceSet, BoundarySegmentEvidence, CanonicalLyrics, CanonicalSingingTrack,
    CanonicalWordBoundary, EvidenceProvenance, ExpertTask, F0Point, HardBoundarySet,
    HarmonyMetadata, LyricsAuthority, PitchGrid, SingingFusionEvidence, SingingReviewReason,
    SingingReviewRegion, TranscriptHypothesis, TranscriptTokenEvidence, WordBoundaryEvidence,
    attach_boundary_constraints, build_canonical_singing_track, build_review_regions,
    decode_candidate_graph_with_boundaries, fuse_singing_evidence_with_challengers,
    fuse_transcripts, fuse_word_boundaries, persistent_f0_shifts, trustworthy_f0_point,
    validate_candidate_path_with_boundaries, validate_candidate_pool,
};

/// How Stage 4 (Expert Fusion) decides the final non-overlapping candidate
/// path. `Algorithm` is the default, production-pinned HSMM decoder.
/// `AiJudgment` is an explicit, non-default opt-in that hands the same
/// candidate pool to the Runtime Manager-resolved Fusion Agent Adapter; see
/// `crate::execution::agent_client`. Neither mode bypasses
/// `validate_canonical_singing_track`.
pub enum FusionDecisionMode<'a> {
    Algorithm,
    AiJudgment {
        /// Runtime Manager-resolved, manifest-verified adapter executable.
        executable: &'a std::path::Path,
        timeout: std::time::Duration,
        cancellation: &'a CancellationToken,
    },
}

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

fn sequence_units(text: &str) -> Vec<String> {
    let words = text
        .split_whitespace()
        .map(|word| {
            word.chars()
                .filter(|character| character.is_alphanumeric())
                .flat_map(char::to_lowercase)
                .collect::<String>()
        })
        .filter(|word| !word.is_empty())
        .collect::<Vec<_>>();
    if words.len() > 1 {
        words
    } else {
        text.chars()
            .filter(|character| character.is_alphanumeric())
            .flat_map(char::to_lowercase)
            .map(|character| character.to_string())
            .collect()
    }
}

fn sequence_edit_distance(left: &[String], right: &[String]) -> usize {
    let mut previous = (0..=right.len()).collect::<Vec<_>>();
    let mut current = vec![0; right.len() + 1];
    for (left_index, left_unit) in left.iter().enumerate() {
        current[0] = left_index + 1;
        for (right_index, right_unit) in right.iter().enumerate() {
            current[right_index + 1] = if left_unit == right_unit {
                previous[right_index]
            } else {
                (previous[right_index] + 1)
                    .min(previous[right_index + 1] + 1)
                    .min(current[right_index] + 1)
            };
        }
        std::mem::swap(&mut previous, &mut current);
    }
    previous[right.len()]
}

fn sequence_similarity(left: &str, right: &str) -> f32 {
    let left = sequence_units(left);
    let right = sequence_units(right);
    let maximum = left.len().max(right.len());
    if maximum == 0 {
        return 1.0;
    }
    1.0 - sequence_edit_distance(&left, &right) as f32 / maximum as f32
}

pub fn build_transcript_disagreement_regions(
    transcript: &TranscriptArtifact,
    reference_lyrics: Option<&str>,
    reference_language: Option<&str>,
    source_range: crate::fusion::TimeRange,
) -> Vec<SingingReviewRegion> {
    if source_range.end <= source_range.start {
        return Vec::new();
    }
    let mut reasons = Vec::new();
    if transcript
        .confidence
        .is_some_and(|confidence| confidence < 0.65)
        || transcript
            .tokens
            .iter()
            .any(|token| token.confidence.is_some_and(|confidence| confidence < 0.65))
    {
        reasons.push(SingingReviewReason::TranscriptLowConfidence);
    }
    if let Some(reference) = reference_lyrics
        .map(str::trim)
        .filter(|reference| !reference.is_empty())
    {
        let transcript_units = sequence_units(&transcript.text).len();
        let reference_units = sequence_units(reference).len();
        if normalized(reference) != normalized(&transcript.text) {
            reasons.push(SingingReviewReason::TranscriptReferenceMismatch);
        }
        if transcript_units > 0
            && reference_units > 0
            && (transcript_units * 2 < reference_units
                || reference_units.saturating_mul(2) < transcript_units)
        {
            reasons.push(SingingReviewReason::TranscriptCoverageMismatch);
        }
    }
    if let (Some(detected), Some(reference)) = (
        transcript.language.as_deref(),
        reference_language
            .map(str::trim)
            .filter(|value| !value.is_empty()),
    ) && detected.split(['-', '_']).next().is_some_and(|detected| {
        !detected.eq_ignore_ascii_case(reference.split(['-', '_']).next().unwrap_or(reference))
    }) {
        reasons.push(SingingReviewReason::TranscriptLanguageMismatch);
    }
    reasons.sort();
    reasons.dedup();
    if reasons.is_empty() {
        return Vec::new();
    }
    let mut evidence_experts = transcript.source_experts.clone();
    if reference_lyrics.is_some() {
        evidence_experts.push("caller.reference".to_string());
    }
    evidence_experts.sort();
    evidence_experts.dedup();
    vec![SingingReviewRegion {
        id: format!(
            "transcript-review-{}-{}",
            source_range.start, source_range.end
        ),
        range: source_range,
        confidence: transcript.confidence,
        reasons,
        evidence_experts,
        reviewed: false,
    }]
}

fn transcript_tokens(artifact: &TranscriptArtifact) -> Vec<TranscriptTokenEvidence> {
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

/// Restores the caller's authoritative Timed-LRC line ranges after the
/// transcript artifact has crossed the model-facing transcript protocol,
/// whose tokens intentionally contain text but no timing fields.
pub fn attach_caller_lyric_ranges(transcript: &mut CanonicalLyrics, lyrics: &Lyrics) {
    if transcript.authority != LyricsAuthority::CallerCanonical
        || lyrics.mode != LyricsMode::Canonical
    {
        return;
    }
    for token in &mut transcript.tokens {
        let Some(id) = token.id.as_deref() else {
            continue;
        };
        let Some(caller) = lyrics
            .tokens
            .iter()
            .find(|caller| caller.id == id && caller.text == token.text)
        else {
            continue;
        };
        if let (Some(start), Some(end)) = (caller.start, caller.end)
            && let Ok(range) = crate::fusion::TimeRange::new(start, end)
        {
            token.range = Some(range);
        }
    }
}

pub fn fuse_transcript_stage(
    evidence: &[TranscriptArtifact],
    reference_lyrics: Option<&str>,
) -> EngineResult<(TranscriptArtifact, CanonicalLyrics)> {
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
        .filter(|artifact| artifact.authority == TranscriptAuthority::CallerCanonical)
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
            .enumerate()
            .map(|(preference_rank, artifact)| TranscriptHypothesis {
                expert_id: artifact.source_experts.join("+"),
                preference_rank: preference_rank as u32,
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
        let artifact = TranscriptArtifact {
            contract: representative.contract.clone(),
            version: representative.version,
            authority: TranscriptAuthority::Generated,
            language: canonical.language.clone(),
            text: canonical.text.clone(),
            audio_segments: representative.audio_segments.clone(),
            tokens: canonical
                .tokens
                .iter()
                .enumerate()
                .map(|(index, token)| TranscriptToken {
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
    {
        let generated = artifact.text.clone();
        if sequence_similarity(&generated, reference) >= 0.5 {
            artifact.text = reference.to_string();
            canonical.text = reference.to_string();
            // The current ASR contracts do not supply stable token timing for
            // sequence reconciliation. Do not leave tokens claiming identity
            // that belonged to the pre-reconciled text.
            artifact.tokens.clear();
            // These scopes index the generated text, not a replacement lyric.
            artifact.audio_segments.clear();
            canonical.tokens.clear();
            if !artifact
                .source_experts
                .iter()
                .any(|source| source == "caller.reference")
            {
                artifact.source_experts.push("caller.reference".to_string());
            }
            if !canonical
                .source_experts
                .iter()
                .any(|source| source == "caller.reference")
            {
                canonical
                    .source_experts
                    .push("caller.reference".to_string());
            }
            if !artifact
                .alternatives
                .iter()
                .any(|item| normalized(item) == normalized(&generated))
            {
                artifact.alternatives.push(generated.clone());
                canonical.alternatives.push(generated);
            }
        } else if !artifact
            .alternatives
            .iter()
            .any(|item| normalized(item) == normalized(reference))
        {
            artifact.alternatives.push(reference.to_string());
            canonical.alternatives.push(reference.to_string());
        }
    }
    if canonical.authority == LyricsAuthority::Generated {
        // ASR audio scopes are search windows, not lyric sentences. Derive
        // textual lines from the selected text, including reference corrections,
        // without inventing line timing or calibrated confidence.
        artifact.tokens = crate::lyric_lines::generated_lyric_lines(&artifact.text);
        canonical.tokens = transcript_tokens(&artifact);
    }
    artifact.validate()?;
    Ok((artifact, canonical))
}

/// Maps every alignment item to the lyric line it was cut from.
///
/// Canonical transcripts carry one token per authored or generated line. Alignment
/// units are those lines cut into lexical units in order, and the stage has
/// already verified that the units' concatenated text is the transcript text,
/// so character offsets in the whitespace-free transcript identify the line
/// of every unit -- including unresolved ones -- without relying on unit ids.
/// Transcripts without line tokens yield no mapping.
fn lyric_line_by_alignment_item(
    transcript: &CanonicalLyrics,
    items: &[AlignmentItem],
) -> BTreeMap<String, String> {
    if transcript.tokens.is_empty() {
        return BTreeMap::new();
    }
    let mut line_ends = Vec::with_capacity(transcript.tokens.len());
    let mut offset = 0usize;
    for token in &transcript.tokens {
        let Some(id) = token.id.as_deref() else {
            return BTreeMap::new();
        };
        offset += compact_normalized(&token.text).chars().count();
        line_ends.push((offset, id));
    }
    if offset != compact_normalized(&transcript.text).chars().count() {
        return BTreeMap::new();
    }
    let mut lines = BTreeMap::new();
    let mut cursor = 0usize;
    for item in items {
        let length = compact_normalized(&item.text).chars().count();
        let line = line_ends
            .iter()
            .find(|(end, _)| cursor < *end)
            .or_else(|| line_ends.last())
            .map(|(_, id)| (*id).to_string());
        if let Some(line) = line {
            lines.insert(item.id.clone(), line);
        }
        cursor += length;
    }
    lines
}

pub fn fuse_alignment_stage(
    transcript: &CanonicalLyrics,
    evidence: &[AlignmentArtifact],
    source_start: u64,
    source_duration: u64,
) -> EngineResult<(AlignmentArtifact, Vec<CanonicalWordBoundary>)> {
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
            || compact_normalized(&artifact.transcript) != compact_normalized(&transcript.text)
            || artifact.source_expert.trim().is_empty()
        {
            return Err(output_error(
                "alignment evidence does not correspond to the canonical transcript",
            ));
        }
        let all_text = artifact
            .items
            .iter()
            .map(|item| item.text.as_str())
            .collect::<String>();
        if compact_normalized(&all_text) != compact_normalized(&transcript.text) {
            return Err(output_error(
                "alignment units lost canonical transcript text",
            ));
        }
        let line_by_item = lyric_line_by_alignment_item(transcript, &artifact.items);
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
            // An unresolved item's range is an audition/search scope. Using
            // it as a word boundary would cut or suppress unrelated notes.
            if item.timing_issue.is_some() {
                continue;
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
                line_id: line_by_item.get(item.id.as_str()).cloned(),
            });
        }
    }
    let words = if projected.is_empty() {
        Vec::new()
    } else {
        fuse_word_boundaries(&projected).map_err(output_error)?
    };
    let representative = &evidence[0];
    let by_id = words
        .iter()
        .map(|word| (word.word_id.as_str(), word))
        .collect::<std::collections::BTreeMap<_, _>>();
    let items = representative
        .items
        .iter()
        .map(|item| {
            if let Some(word) = by_id.get(item.id.as_str()) {
                Ok(AlignmentItem {
                    id: word.word_id.clone(),
                    text: word.text.clone(),
                    level: BoundaryLevel::Word,
                    start: word.range.start,
                    duration: word.range.end - word.range.start,
                    confidence: word.confidence,
                    authority: BoundaryAuthority::Soft,
                    timing_issue: None,
                })
            } else if item.timing_issue.is_some() {
                Ok(item.clone())
            } else {
                Err(output_error(
                    "alignment fusion lost a measured word identity",
                ))
            }
        })
        .collect::<EngineResult<Vec<_>>>()?;
    if items
        .iter()
        .filter(|item| item.timing_issue.is_none())
        .count()
        != words.len()
    {
        return Err(output_error(
            "alignment fusion introduced unrelated word identities",
        ));
    }
    let artifact = AlignmentArtifact {
        contract: representative.contract.clone(),
        version: representative.version,
        transcript: transcript.text.clone(),
        language: transcript.language.clone(),
        items,
        source_expert: words
            .iter()
            .flat_map(|word| word.source_experts.iter().cloned())
            .chain(std::iter::once(representative.source_expert.clone()))
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

pub fn project_pitch_f0(evidence: &PitchEvidence) -> EngineResult<Vec<F0Point>> {
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

pub fn project_rmvpe_f0(evidence: &PitchEvidence) -> EngineResult<Vec<F0Point>> {
    project_pitch_f0(evidence)
}

fn pitch_identity(
    owner: &str,
    rmvpe: Option<&PitchEvidence>,
    fcpe: Option<&PitchEvidence>,
) -> (Option<String>, Option<String>) {
    let evidence = if owner == "fcpe" {
        fcpe.or(rmvpe)
    } else {
        rmvpe.or(fcpe)
    };
    let manifest = evidence
        .and_then(|evidence| evidence.model.get("manifest_sha256"))
        .and_then(serde_json::Value::as_str)
        .map(str::to_string);
    let runtime = evidence
        .and_then(|evidence| evidence.model.get("runtime_manifest_sha256"))
        .and_then(serde_json::Value::as_str)
        .map(str::to_string);
    (manifest, runtime)
}

fn split_f0_range_at_word_edges(
    start: u64,
    end: u64,
    words: &[CanonicalWordBoundary],
) -> Vec<(u64, u64)> {
    let mut cuts = vec![start, end];
    for word in words {
        for edge in [word.range.start, word.range.end] {
            if edge > start && edge < end {
                cuts.push(edge);
            }
        }
    }
    cuts.sort_unstable();
    cuts.dedup();
    cuts.windows(2)
        .filter_map(|pair| (pair[1] > pair[0]).then_some((pair[0], pair[1])))
        .collect()
}

fn derive_f0_length_evidence(
    curve: &[F0Point],
    words: &[CanonicalWordBoundary],
    source_start: u64,
    source_duration: u64,
    owner: &str,
    rmvpe: Option<&PitchEvidence>,
    fcpe: Option<&PitchEvidence>,
) -> EngineResult<BoundaryEvidenceSet> {
    let trustworthy_curve = curve
        .iter()
        .filter(|point| trustworthy_f0_point(point))
        .cloned()
        .collect::<Vec<_>>();
    let curve = trustworthy_curve.as_slice();
    if curve.is_empty() {
        return Err(output_error(
            "At least one enabled F0 expert must produce trustworthy voiced evidence before note lengths can be derived",
        ));
    }
    let source_end = source_start
        .checked_add(source_duration)
        .ok_or_else(|| output_error("F0-derived length timeline overflows"))?;
    let mut hops = curve
        .windows(2)
        .filter_map(|pair| pair[1].time.checked_sub(pair[0].time))
        .filter(|hop| *hop > 0)
        .collect::<Vec<_>>();
    hops.sort_unstable();
    let hop = hops.get(hops.len() / 2).copied().unwrap_or(10_000);
    let max_gap = hop.saturating_mul(3).max(60_000);

    let pitch_split_times = persistent_f0_shifts(curve)
        .into_iter()
        .map(|(time, _)| time)
        .collect::<HashSet<_>>();
    let mut groups = Vec::<Vec<&F0Point>>::new();
    let mut current = Vec::<&F0Point>::new();
    for (index, point) in curve.iter().enumerate() {
        let gap_split = index > 0 && point.time.saturating_sub(curve[index - 1].time) > max_gap;
        let pitch_split = pitch_split_times.contains(&point.time);
        if (gap_split || pitch_split) && !current.is_empty() {
            groups.push(std::mem::take(&mut current));
        }
        current.push(point);
    }
    if !current.is_empty() {
        groups.push(current);
    }

    let segments = groups
        .into_iter()
        .filter_map(|group| {
            let start = group[0].time.max(source_start);
            let end = group
                .last()
                .map(|point| point.time.saturating_add(hop).min(source_end))
                .unwrap_or(start);
            (end > start).then_some((start, end))
        })
        .flat_map(|(start, end)| split_f0_range_at_word_edges(start, end, words))
        .map(|(start, end)| {
            Ok(BoundarySegmentEvidence {
                range: crate::fusion::TimeRange::new(start, end).map_err(output_error)?,
                fractional_midi: None,
                boundary_decision_parameter: None,
                presence_decision_parameter: None,
            })
        })
        .collect::<EngineResult<Vec<_>>>()?;
    if segments.is_empty() {
        return Err(output_error(
            "Enabled F0 evidence did not contain a usable voiced duration",
        ));
    }
    let (model_hash, runtime_identity) = pitch_identity(owner, rmvpe, fcpe);
    Ok(BoundaryEvidenceSet {
        source_expert: format!("{owner}.f0_segmentation"),
        kind: BoundaryEvidenceKind::F0Derived,
        model_hash,
        runtime_identity,
        segments,
    })
}

#[allow(clippy::too_many_arguments)]
fn provenance(
    transcript: &TranscriptArtifact,
    alignment: &AlignmentArtifact,
    pitch: Option<&PitchEvidence>,
    fcpe: Option<&PitchEvidence>,
    basic_pitch: Option<&BasicPitchEvidence>,
    boundary: &BoundaryEvidenceSet,
    acoustic: Option<&AcousticEvidence>,
    advanced_notes: &[AdvancedNoteEvidence],
    timed_notes: &[TimedNoteExpertEvidence],
    techniques: &[TechniqueEvidence],
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
            expert_id: boundary.source_expert.clone(),
            task: ExpertTask::NoteBoundary,
            model_hash: boundary.model_hash.clone(),
            runtime_identity: boundary.runtime_identity.clone(),
            calibration_version: None,
            correlation_group: (boundary.kind == BoundaryEvidenceKind::F0Derived)
                .then(|| "continuous-pitch-neural".to_string()),
            depends_on: if boundary.kind == BoundaryEvidenceKind::F0Derived {
                vec![
                    boundary
                        .source_expert
                        .strip_suffix(".f0_segmentation")
                        .unwrap_or(&boundary.source_expert)
                        .to_string(),
                ]
            } else {
                Vec::new()
            },
        },
    ];
    if let Some(acoustic) = acoustic {
        result.push(EvidenceProvenance {
            expert_id: acoustic.algorithm.clone(),
            task: ExpertTask::Acoustic,
            model_hash: None,
            runtime_identity: Some(acoustic.decoded_audio_sha256.clone()),
            calibration_version: None,
            correlation_group: None,
            depends_on: Vec::new(),
        });
    }
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
            model_hash: None,
            runtime_identity: Some(evidence.runtime_manifest_sha256.clone()),
            calibration_version: None,
            correlation_group: None,
            depends_on: Vec::new(),
        });
    }
    for evidence in advanced_notes {
        result.push(evidence.provenance());
    }
    result.extend(
        timed_notes
            .iter()
            .map(|evidence| evidence.provenance.clone()),
    );
    for evidence in techniques {
        result.push(evidence.provenance.clone());
    }
    result
}

fn boundary_constraint_events(
    constraints: &[BoundaryConstraint],
    source_start: u64,
    source_duration: u64,
) -> EngineResult<(Vec<BoundaryAlternative>, Vec<BoundaryConstraintEvidence>)> {
    let source_end = source_start
        .checked_add(source_duration)
        .ok_or_else(|| output_error("constraint source timeline overflows"))?;
    let mut result = Vec::with_capacity(constraints.len());
    let mut phrase_starts = Vec::new();
    for constraint in constraints {
        let end = constraint.end()?;
        if constraint.start < source_start || end > source_end {
            return Err(output_error(format!(
                "boundary constraint from {} is outside the analyzed source timeline",
                constraint.source
            )));
        }
        let level = match constraint.level {
            BoundaryLevel::Phrase => "phrase",
            BoundaryLevel::Word => "word",
            BoundaryLevel::Syllable => "syllable",
            BoundaryLevel::Phoneme => "phoneme",
        };
        let token = constraint.token_id.as_deref().unwrap_or("timeline");
        let source_expert = format!("constraint.{}.{}.{}", constraint.source, level, token);
        result.push(BoundaryAlternative {
            source_expert: source_expert.clone(),
            range: crate::fusion::TimeRange::new(constraint.start, end).map_err(output_error)?,
            kind: if constraint.level == BoundaryLevel::Phrase {
                BoundaryEvidenceKind::PhraseConstraint
            } else {
                BoundaryEvidenceKind::Constraint
            },
            fractional_midi: None,
            source_local_score: Some(constraint.confidence),
            source_local_pitch_score: None,
            calibrated_boundary_confidence: None,
            calibrated_pitch_confidence: None,
            hard: constraint.authority == BoundaryAuthority::Hard,
        });
        if constraint.level == BoundaryLevel::Phrase
            && constraint.authority == BoundaryAuthority::Soft
        {
            phrase_starts.push(BoundaryConstraintEvidence {
                source_expert,
                kind: BoundaryConstraintKind::PhraseStart,
                time: constraint.start,
                source_local_strength: Some(constraint.confidence),
                calibrated_confidence: None,
                calibration_version: None,
                correlation_group: Some(format!("constraint.{}", constraint.source)),
                depends_on: Vec::new(),
            });
        }
    }
    Ok((result, phrase_starts))
}

fn context_boundary_constraints(
    words: &[CanonicalWordBoundary],
    f0_curve: &[F0Point],
    pitch_owner: &str,
    basic_pitch: Option<&BasicPitchEvidence>,
    acoustic: Option<&AcousticEvidence>,
) -> Vec<BoundaryConstraintEvidence> {
    let mut constraints = Vec::new();

    for word in words {
        constraints.push(BoundaryConstraintEvidence {
            source_expert: "forced_alignment".to_string(),
            kind: BoundaryConstraintKind::WordStart,
            time: word.range.start,
            source_local_strength: None,
            calibrated_confidence: None,
            calibration_version: None,
            correlation_group: None,
            depends_on: Vec::new(),
        });
        constraints.push(BoundaryConstraintEvidence {
            source_expert: "forced_alignment".to_string(),
            kind: BoundaryConstraintKind::WordEnd,
            time: word.range.end,
            source_local_strength: None,
            calibrated_confidence: None,
            calibration_version: None,
            correlation_group: None,
            depends_on: Vec::new(),
        });
    }

    let trustworthy_f0 = f0_curve
        .iter()
        .filter(|point| trustworthy_f0_point(point))
        .cloned()
        .collect::<Vec<_>>();
    if trustworthy_f0.len() >= 2 {
        let mut hops = trustworthy_f0
            .windows(2)
            .filter_map(|pair| pair[1].time.checked_sub(pair[0].time))
            .filter(|hop| *hop > 0)
            .collect::<Vec<_>>();
        hops.sort_unstable();
        let median_hop = hops.get(hops.len() / 2).copied().unwrap_or(10_000);
        let voicing_gap = median_hop.saturating_mul(3).max(60_000);
        for pair in trustworthy_f0.windows(2) {
            let gap = pair[1].time.saturating_sub(pair[0].time);
            if gap > voicing_gap {
                constraints.push(BoundaryConstraintEvidence {
                    source_expert: format!("{pitch_owner}_voicing_transition"),
                    kind: BoundaryConstraintKind::VoicingTransition,
                    time: pair[1].time,
                    source_local_strength: Some(
                        ((gap as f32 / voicing_gap as f32) - 1.0).clamp(0.0, 1.0),
                    ),
                    calibrated_confidence: None,
                    calibration_version: Some("f0-transition-source-local".to_string()),
                    correlation_group: Some("continuous-pitch-neural".to_string()),
                    depends_on: vec![pitch_owner.to_string()],
                });
            }
        }
        constraints.extend(persistent_f0_shifts(&trustworthy_f0).into_iter().map(
            |(time, strength)| BoundaryConstraintEvidence {
                source_expert: format!("{pitch_owner}_pitch_discontinuity"),
                kind: BoundaryConstraintKind::PitchDiscontinuity,
                time,
                source_local_strength: Some(strength),
                calibrated_confidence: None,
                calibration_version: Some("f0-pitch-discontinuity-source-local".to_string()),
                correlation_group: Some("continuous-pitch-neural".to_string()),
                depends_on: vec![pitch_owner.to_string()],
            },
        ));
    }

    if let Some(basic_pitch) = basic_pitch {
        constraints.extend(
            basic_pitch
                .frames
                .iter()
                .filter(|frame| frame.onset_activation.is_finite() && frame.onset_activation >= 0.5)
                .map(|frame| BoundaryConstraintEvidence {
                    source_expert: "basic_pitch".to_string(),
                    kind: BoundaryConstraintKind::BasicPitchOnset,
                    time: frame.time,
                    source_local_strength: Some(frame.onset_activation),
                    calibrated_confidence: None,
                    calibration_version: Some("basic-pitch-onset-source-local".to_string()),
                    correlation_group: None,
                    depends_on: Vec::new(),
                }),
        );
    }

    if let Some(acoustic) = acoustic {
        let mut fluxes = acoustic
            .frames
            .iter()
            .filter_map(|frame| frame.spectral_flux)
            .filter(|flux| flux.is_finite() && *flux > 0.0)
            .collect::<Vec<_>>();
        if !fluxes.is_empty() {
            fluxes.sort_by(f32::total_cmp);
            let threshold_index = ((fluxes.len() - 1) * 9) / 10;
            let threshold = fluxes[threshold_index].max(0.05);
            constraints.extend(acoustic.frames.iter().filter_map(|frame| {
                frame.spectral_flux.and_then(|flux| {
                    (flux.is_finite() && flux >= threshold).then(|| BoundaryConstraintEvidence {
                        source_expert: acoustic.algorithm.clone(),
                        kind: BoundaryConstraintKind::AcousticArticulation,
                        time: frame.start,
                        source_local_strength: Some((flux / (threshold * 2.0)).clamp(0.0, 1.0)),
                        calibrated_confidence: None,
                        calibration_version: Some("acoustic-articulation-source-local".to_string()),
                        correlation_group: None,
                        depends_on: Vec::new(),
                    })
                })
            }));
        }
    }

    constraints.sort_by(|left, right| {
        (left.time, &left.source_expert, left.kind as u8).cmp(&(
            right.time,
            &right.source_expert,
            right.kind as u8,
        ))
    });
    constraints.dedup_by(|left, right| {
        left.time == right.time
            && left.source_expert == right.source_expert
            && left.kind == right.kind
    });
    constraints
}

pub struct SingingFusionStageOutput {
    pub fusion: SingingFusionEvidence,
    f0_curve: Vec<F0Point>,
    continuous_f0_source: String,
    provenance: Vec<EvidenceProvenance>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CandidatePathDecision {
    Algorithm {
        candidate_set_digest: String,
        selected_candidate_ids: Vec<String>,
    },
    AiJudgment {
        candidate_set_digest: String,
        selected_candidate_ids: Vec<String>,
        response_digest: String,
    },
}

pub struct SingingStagesOutput {
    pub fusion: SingingFusionEvidence,
    pub track: CanonicalSingingTrack,
    pub review_regions: Vec<SingingReviewRegion>,
    pub decision: CandidatePathDecision,
}

#[allow(clippy::too_many_arguments)]
pub fn execute_singing_fusion_stage_with_timed_notes(
    transcript_artifact: &TranscriptArtifact,
    alignment_artifact: &AlignmentArtifact,
    words: &[CanonicalWordBoundary],
    pitch_evidence: Option<&PitchEvidence>,
    fcpe_evidence: Option<&PitchEvidence>,
    basic_pitch_evidence: Option<&BasicPitchEvidence>,
    game: Option<&GameEvidence>,
    acoustic: Option<&AcousticEvidence>,
    advanced_notes: &[AdvancedNoteEvidence],
    timed_notes: &[TimedNoteExpertEvidence],
    technique_evidence: &[TechniqueEvidence],
    boundary_constraints: &[BoundaryConstraint],
    source_start: u64,
    source_duration: u64,
    pitch_owner: &str,
) -> EngineResult<SingingFusionStageOutput> {
    let source_end = source_start
        .checked_add(source_duration)
        .ok_or_else(|| output_error("hard-boundary source timeline overflows"))?;
    let hard_boundaries =
        HardBoundarySet::from_constraints(boundary_constraints, source_start, source_end)
            .map_err(output_error)?;
    let rmvpe_grid = pitch_evidence
        .map(|evidence| PitchGrid::new(evidence.start, evidence.hop, evidence.frequency_hz.len()))
        .transpose()
        .map_err(output_error)?;
    let fcpe_grid = fcpe_evidence
        .map(|evidence| PitchGrid::new(evidence.start, evidence.hop, evidence.frequency_hz.len()))
        .transpose()
        .map_err(output_error)?;
    let rmvpe_curve = pitch_evidence
        .map(project_rmvpe_f0)
        .transpose()?
        .unwrap_or_default();
    let fcpe_curve = fcpe_evidence
        .map(project_pitch_f0)
        .transpose()?
        .unwrap_or_default();
    let f0_curve = match pitch_owner {
        "fcpe" if !fcpe_curve.is_empty() => fcpe_curve.clone(),
        "fcpe" => {
            return Err(output_error(
                "FCPE is selected as the continuous F0 owner but produced no voiced evidence",
            ));
        }
        "rmvpe" if !rmvpe_curve.is_empty() => rmvpe_curve.clone(),
        "rmvpe" => {
            return Err(output_error(
                "RMVPE is selected as the continuous F0 owner but produced no voiced evidence",
            ));
        }
        other => {
            return Err(output_error(format!(
                "unsupported continuous F0 owner: {other}"
            )));
        }
    };

    let mut boundary_challengers = Vec::new();
    for evidence in advanced_notes {
        boundary_challengers.extend(
            evidence
                .timed_note_evidence(source_start, source_duration)?
                .boundary_alternatives(source_start, source_duration)?,
        );
    }
    for evidence in timed_notes {
        boundary_challengers.extend(evidence.boundary_alternatives(source_start, source_duration)?);
    }
    let (constraint_challengers, phrase_start_constraints) =
        boundary_constraint_events(boundary_constraints, source_start, source_duration)?;
    boundary_challengers.extend(constraint_challengers);

    let game_boundary = game
        .map(BoundaryEvidenceSet::from_game)
        .transpose()
        .map_err(output_error)?;
    let derived_boundary = game_boundary.is_none().then(|| {
        derive_f0_length_evidence(
            &f0_curve,
            words,
            source_start,
            source_duration,
            pitch_owner,
            pitch_evidence,
            fcpe_evidence,
        )
    });
    let derived_boundary = derived_boundary.transpose()?;
    let boundary_evidence = game_boundary
        .as_ref()
        .or(derived_boundary.as_ref())
        .expect("F0 boundary evidence is constructed when GAME is absent");

    // Stage 3 participation is authoritative. Every available evidence source
    // contributes onset and non-onset context to the same candidate pool.
    let acoustic_for_fusion = acoustic;
    let acoustic_onset_enabled = true;
    let basic_pitch_for_fusion = basic_pitch_evidence;
    let mut fusion = fuse_singing_evidence_with_challengers(
        words,
        boundary_evidence,
        pitch_owner,
        &rmvpe_curve,
        rmvpe_grid,
        &fcpe_curve,
        fcpe_grid,
        acoustic_for_fusion,
        acoustic_onset_enabled,
        basic_pitch_for_fusion,
        &boundary_challengers,
        technique_evidence,
    )
    .map_err(output_error)?;
    fusion.hard_boundaries = hard_boundaries;
    let mut context_constraints = context_boundary_constraints(
        words,
        &f0_curve,
        pitch_owner,
        basic_pitch_for_fusion,
        acoustic_for_fusion,
    );
    context_constraints.extend(phrase_start_constraints);
    attach_boundary_constraints(&mut fusion.candidates, &context_constraints)
        .map_err(output_error)?;
    Ok(SingingFusionStageOutput {
        fusion,
        f0_curve,
        continuous_f0_source: pitch_owner.to_string(),
        provenance: provenance(
            transcript_artifact,
            alignment_artifact,
            pitch_evidence,
            fcpe_evidence,
            basic_pitch_for_fusion,
            boundary_evidence,
            acoustic_for_fusion,
            advanced_notes,
            timed_notes,
            technique_evidence,
        ),
    })
}

#[allow(clippy::too_many_arguments)]
pub fn execute_singing_fusion_stage(
    transcript_artifact: &TranscriptArtifact,
    alignment_artifact: &AlignmentArtifact,
    words: &[CanonicalWordBoundary],
    pitch_evidence: Option<&PitchEvidence>,
    fcpe_evidence: Option<&PitchEvidence>,
    basic_pitch_evidence: Option<&BasicPitchEvidence>,
    game: Option<&GameEvidence>,
    acoustic: Option<&AcousticEvidence>,
    advanced_notes: &[AdvancedNoteEvidence],
    technique_evidence: &[TechniqueEvidence],
    boundary_constraints: &[BoundaryConstraint],
    source_start: u64,
    source_duration: u64,
    pitch_owner: &str,
) -> EngineResult<SingingFusionStageOutput> {
    execute_singing_fusion_stage_with_timed_notes(
        transcript_artifact,
        alignment_artifact,
        words,
        pitch_evidence,
        fcpe_evidence,
        basic_pitch_evidence,
        game,
        acoustic,
        advanced_notes,
        &[],
        technique_evidence,
        boundary_constraints,
        source_start,
        source_duration,
        pitch_owner,
    )
}

#[derive(Debug)]
struct TimedLyricWordGroup {
    range: crate::fusion::TimeRange,
    word_indices: Vec<usize>,
}

fn timed_lyric_word_groups(
    transcript: &CanonicalLyrics,
    words: &[CanonicalWordBoundary],
) -> Vec<TimedLyricWordGroup> {
    let mut groups = transcript
        .tokens
        .iter()
        .filter_map(|token| token.range)
        .map(|range| TimedLyricWordGroup {
            range,
            word_indices: Vec::new(),
        })
        .collect::<Vec<_>>();
    // Only measured words survive alignment fusion. Counting their text as
    // though unresolved words never existed shifts every later line (especially
    // repeated lyrics). Measured words belong to the caller audio scope in
    // which they actually occur; neither text length nor repetition sets time.
    for (word_index, word) in words.iter().enumerate() {
        let owner = groups
            .iter()
            .enumerate()
            .filter_map(|(token_index, group)| {
                let overlap = range_overlap(word.range, group.range);
                (overlap > 0).then_some((overlap, std::cmp::Reverse(token_index), token_index))
            })
            .max_by_key(|(overlap, token_index, _)| (*overlap, *token_index));
        if let Some((_, _, token_index)) = owner {
            groups[token_index].word_indices.push(word_index);
        }
    }
    groups.retain(|group| !group.word_indices.is_empty());
    groups
}

fn range_overlap(left: crate::fusion::TimeRange, right: crate::fusion::TimeRange) -> u64 {
    left.end
        .min(right.end)
        .saturating_sub(left.start.max(right.start))
}

fn range_distance(left: crate::fusion::TimeRange, right: crate::fusion::TimeRange) -> u64 {
    if left.end <= right.start {
        right.start - left.end
    } else if right.end <= left.start {
        left.start.saturating_sub(right.end)
    } else {
        0
    }
}

fn timed_lyric_word_owner_from_groups<'a>(
    note: crate::fusion::TimeRange,
    groups: &[TimedLyricWordGroup],
    words: &'a [CanonicalWordBoundary],
) -> Option<&'a str> {
    let group = groups
        .iter()
        .enumerate()
        .filter_map(|(index, group)| {
            let overlap = range_overlap(note, group.range);
            (overlap > 0).then_some((overlap, std::cmp::Reverse(index), group))
        })
        .max_by_key(|(overlap, index, _)| (*overlap, *index))?
        .2;

    group
        .word_indices
        .iter()
        .map(|&index| {
            let word = &words[index];
            (
                range_overlap(note, word.range),
                std::cmp::Reverse(range_distance(note, word.range)),
                std::cmp::Reverse(index),
                word,
            )
        })
        .max_by_key(|(overlap, distance, index, _)| (*overlap, *distance, *index))
        .map(|(_, _, _, word)| word.word_id.as_str())
}

#[cfg(test)]
fn timed_lyric_word_owner<'a>(
    note: crate::fusion::TimeRange,
    transcript: &CanonicalLyrics,
    words: &'a [CanonicalWordBoundary],
) -> Option<&'a str> {
    timed_lyric_word_owner_from_groups(note, &timed_lyric_word_groups(transcript, words), words)
}

/// The selector must see the original, immutable evidence pool. Once it has
/// chosen a path, project only its otherwise-unowned notes onto the caller's
/// timed lyric lines. This keeps model selection unchanged while preventing a
/// collapsed forced-alignment word span from silently discarding real notes
/// elsewhere in the same authoritative lyric line during chart finalization.
fn attach_timed_lyric_owners(
    selected: &mut [crate::fusion::SegmentCandidate],
    transcript: &CanonicalLyrics,
    words: &[CanonicalWordBoundary],
) {
    if transcript.authority != LyricsAuthority::CallerCanonical
        || !transcript.tokens.iter().any(|token| token.range.is_some())
    {
        return;
    }
    let groups = timed_lyric_word_groups(transcript, words);
    for candidate in selected
        .iter_mut()
        .filter(|candidate| candidate.word_id.is_none())
    {
        candidate.word_id =
            timed_lyric_word_owner_from_groups(candidate.range, &groups, words).map(str::to_string);
    }
}

pub fn execute_candidate_graph_stage(
    transcript: CanonicalLyrics,
    words: Vec<CanonicalWordBoundary>,
    singing: SingingFusionStageOutput,
    mode: FusionDecisionMode<'_>,
) -> EngineResult<SingingStagesOutput> {
    validate_candidate_pool(&singing.fusion.candidates).map_err(output_error)?;
    // Candidate construction is complete before selector dispatch. Both modes
    // are therefore bound to this exact, selector-independent pool identity.
    singing
        .fusion
        .hard_boundaries
        .validate()
        .map_err(output_error)?;
    let candidate_set_digest = crate::execution::candidate_set_digest(&singing.fusion)?;
    let (mut decoded, decision) = match mode {
        FusionDecisionMode::Algorithm => {
            let decoded = decode_candidate_graph_with_boundaries(
                &singing.fusion.candidates,
                &singing.fusion.hard_boundaries,
            )
            .map_err(output_error)?;
            let decision = CandidatePathDecision::Algorithm {
                candidate_set_digest: candidate_set_digest.clone(),
                selected_candidate_ids: decoded
                    .iter()
                    .map(|candidate| candidate.id.clone())
                    .collect(),
            };
            (decoded, decision)
        }
        FusionDecisionMode::AiJudgment {
            executable,
            timeout,
            cancellation,
        } => {
            let agent = crate::execution::run_fusion_agent_for_pool_with_lyrics(
                executable,
                &singing.fusion,
                &transcript,
                &words,
                timeout,
                cancellation,
            )?;
            let decision = CandidatePathDecision::AiJudgment {
                candidate_set_digest: candidate_set_digest.clone(),
                selected_candidate_ids: agent
                    .selected
                    .iter()
                    .map(|candidate| candidate.id.clone())
                    .collect(),
                response_digest: agent.response_digest,
            };
            (agent.selected, decision)
        }
    };
    if decoded.is_empty() {
        return Err(output_error("candidate graph selected no note states"));
    }
    validate_candidate_path_with_boundaries(
        &singing.fusion.candidates,
        &decoded,
        &singing.fusion.hard_boundaries,
    )
    .map_err(output_error)?;
    attach_timed_lyric_owners(&mut decoded, &transcript, &words);
    let track = build_canonical_singing_track(
        transcript,
        words,
        decoded,
        singing.f0_curve,
        &singing.continuous_f0_source,
        HarmonyMetadata::default(),
        singing.provenance,
    )
    .map_err(output_error)?;
    let review_regions = build_review_regions(&track);
    Ok(SingingStagesOutput {
        fusion: singing.fusion,
        track,
        review_regions,
        decision,
    })
}

#[allow(clippy::too_many_arguments)]
pub fn build_baseline_review_regions(
    transcript_artifact: &TranscriptArtifact,
    transcript: CanonicalLyrics,
    alignment_artifact: &AlignmentArtifact,
    words: Vec<CanonicalWordBoundary>,
    pitch_evidence: Option<&PitchEvidence>,
    fcpe_evidence: Option<&PitchEvidence>,
    game: &GameEvidence,
    acoustic: &AcousticEvidence,
    source_start: u64,
    source_duration: u64,
    pitch_owner: &str,
) -> EngineResult<Vec<SingingReviewRegion>> {
    let fusion = execute_singing_fusion_stage(
        transcript_artifact,
        alignment_artifact,
        &words,
        pitch_evidence,
        fcpe_evidence,
        None,
        Some(game),
        Some(acoustic),
        &[],
        &[],
        &[],
        source_start,
        source_duration,
        pitch_owner,
    )?;
    Ok(
        execute_candidate_graph_stage(transcript, words, fusion, FusionDecisionMode::Algorithm)?
            .review_regions,
    )
}

#[allow(clippy::too_many_arguments)]
pub fn execute_singing_stages(
    transcript_artifact: &TranscriptArtifact,
    transcript: CanonicalLyrics,
    alignment_artifact: &AlignmentArtifact,
    words: Vec<CanonicalWordBoundary>,
    pitch_evidence: Option<&PitchEvidence>,
    game: &GameEvidence,
    acoustic: &AcousticEvidence,
) -> EngineResult<SingingStagesOutput> {
    let fusion = execute_singing_fusion_stage(
        transcript_artifact,
        alignment_artifact,
        &words,
        pitch_evidence,
        None,
        None,
        Some(game),
        Some(acoustic),
        &[],
        &[],
        &[],
        0,
        acoustic
            .frames
            .last()
            .map_or(1, |frame| frame.start.saturating_add(acoustic.hop)),
        "rmvpe",
    )?;
    execute_candidate_graph_stage(transcript, words, fusion, FusionDecisionMode::Algorithm)
}

#[cfg(test)]
#[path = "candidate_pipeline_tests.rs"]
mod tests;
