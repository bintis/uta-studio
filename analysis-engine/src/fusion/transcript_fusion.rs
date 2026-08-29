use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use super::{TimeRange, WeightedEstimate, correlation_aware_score};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TranscriptTokenEvidence {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub id: Option<String>,
    pub text: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub range: Option<TimeRange>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub confidence: Option<f32>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LyricsAuthority {
    CallerCanonical,
    Generated,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TranscriptHypothesis {
    pub expert_id: String,
    /// Stable provider preference used only after exact-consensus count and
    /// comparable calibrated confidence. It is not a model score.
    #[serde(default)]
    pub preference_rank: u32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub language: Option<String>,
    pub text: String,
    #[serde(default)]
    pub tokens: Vec<TranscriptTokenEvidence>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub confidence: Option<f32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub correlation_group: Option<String>,
    #[serde(default)]
    pub dependencies: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CanonicalLyrics {
    pub text: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub language: Option<String>,
    pub authority: LyricsAuthority,
    pub tokens: Vec<TranscriptTokenEvidence>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub confidence: Option<f32>,
    pub source_experts: Vec<String>,
    #[serde(default)]
    pub alternatives: Vec<String>,
}

fn normalize_text(text: &str) -> String {
    text.split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .to_lowercase()
}

fn hypothesis_text(hypothesis: &TranscriptHypothesis) -> String {
    hypothesis.text.clone()
}

fn calibrated_group_score(members: &[&TranscriptHypothesis]) -> Result<Option<f32>, String> {
    let Some(estimates) = members
        .iter()
        .map(|hypothesis| {
            hypothesis.confidence.map(|confidence| WeightedEstimate {
                expert_id: hypothesis.expert_id.clone(),
                value: 1.0,
                calibrated_confidence: confidence,
                base_weight: 1.0,
                correlation_group: hypothesis.correlation_group.clone(),
                dependencies: hypothesis.dependencies.clone(),
            })
        })
        .collect::<Option<Vec<_>>>()
    else {
        return Ok(None);
    };
    correlation_aware_score(&estimates).map(Some)
}

/// Correlation-aware transcript consensus.
///
/// A larger exact-text consensus is preferred using the observed expert count.
/// Calibrated confidence is used only when every member of the compared group
/// supplies it. Ties without comparable calibrated confidence use normalized
/// lexical order and preserve every conflicting text as an alternative; this
/// deterministic tie-break is not represented as probability.
pub fn fuse_transcripts(hypotheses: &[TranscriptHypothesis]) -> Result<CanonicalLyrics, String> {
    if hypotheses.is_empty() {
        return Err("transcript fusion requires at least one hypothesis".to_string());
    }
    let mut groups: BTreeMap<String, Vec<&TranscriptHypothesis>> = BTreeMap::new();
    for hypothesis in hypotheses {
        if hypothesis.expert_id.trim().is_empty() || hypothesis.text.trim().is_empty() {
            return Err("transcript hypothesis is empty or has no expert identity".to_string());
        }
        if hypothesis
            .confidence
            .is_some_and(|value| !value.is_finite() || !(0.0..=1.0).contains(&value))
            || hypothesis.tokens.iter().any(|token| {
                token.text.trim().is_empty()
                    || token
                        .confidence
                        .is_some_and(|value| !value.is_finite() || !(0.0..=1.0).contains(&value))
            })
        {
            return Err("transcript hypothesis has invalid text or confidence".to_string());
        }
        let text = hypothesis_text(hypothesis);
        groups
            .entry(normalize_text(&text))
            .or_default()
            .push(hypothesis);
    }

    let mut ranked = groups
        .into_iter()
        .map(|(normalized, mut members)| {
            members.sort_by(|left, right| left.expert_id.cmp(&right.expert_id));
            let score = calibrated_group_score(&members)?;
            let preference_rank = members
                .iter()
                .map(|hypothesis| hypothesis.preference_rank)
                .min()
                .unwrap_or(u32::MAX);
            Ok((normalized, members, score, preference_rank))
        })
        .collect::<Result<Vec<_>, String>>()?;
    ranked.sort_by(|left, right| {
        right
            .1
            .len()
            .cmp(&left.1.len())
            .then_with(|| match (left.2, right.2) {
                (Some(left_score), Some(right_score)) => right_score.total_cmp(&left_score),
                _ => std::cmp::Ordering::Equal,
            })
            .then_with(|| left.3.cmp(&right.3))
            .then_with(|| left.0.cmp(&right.0))
    });
    let (_, winners, confidence, _) = ranked
        .first()
        .ok_or_else(|| "transcript fusion produced no candidates".to_string())?;
    let representative = winners
        .iter()
        .copied()
        .min_by(|left, right| match (left.confidence, right.confidence) {
            (Some(left_score), Some(right_score)) => right_score
                .total_cmp(&left_score)
                .then_with(|| left.expert_id.cmp(&right.expert_id)),
            _ => left.expert_id.cmp(&right.expert_id),
        })
        .expect("a ranked transcript group is non-empty");
    let alternatives = ranked
        .iter()
        .skip(1)
        .map(|(_, members, _, _)| hypothesis_text(members[0]))
        .collect();
    Ok(CanonicalLyrics {
        text: hypothesis_text(representative),
        language: representative.language.clone(),
        authority: LyricsAuthority::Generated,
        tokens: representative.tokens.clone(),
        confidence: *confidence,
        source_experts: winners
            .iter()
            .map(|hypothesis| hypothesis.expert_id.clone())
            .collect(),
        alternatives,
    })
}
