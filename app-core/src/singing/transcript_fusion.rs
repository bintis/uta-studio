use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use super::{TimeRange, WeightedEstimate, correlation_aware_score};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TranscriptTokenEvidence {
    pub text: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub range: Option<TimeRange>,
    pub confidence: f32,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TranscriptHypothesis {
    pub expert_id: String,
    pub language: String,
    pub tokens: Vec<TranscriptTokenEvidence>,
    pub confidence: f32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub correlation_group: Option<String>,
    #[serde(default)]
    pub dependencies: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CanonicalLyrics {
    pub text: String,
    pub language: String,
    pub tokens: Vec<TranscriptTokenEvidence>,
    pub confidence: f32,
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

pub fn fuse_transcripts(hypotheses: &[TranscriptHypothesis]) -> Result<CanonicalLyrics, String> {
    if hypotheses.is_empty() {
        return Err("transcript fusion requires at least one hypothesis".to_string());
    }
    let mut groups: BTreeMap<String, Vec<&TranscriptHypothesis>> = BTreeMap::new();
    for hypothesis in hypotheses {
        if hypothesis.tokens.is_empty() || !hypothesis.confidence.is_finite() {
            return Err("transcript hypothesis is empty or has invalid confidence".to_string());
        }
        let text = hypothesis
            .tokens
            .iter()
            .map(|token| token.text.as_str())
            .collect::<Vec<_>>()
            .join(" ");
        groups
            .entry(normalize_text(&text))
            .or_default()
            .push(hypothesis);
    }

    let mut ranked = groups
        .into_iter()
        .map(|(normalized, members)| {
            let estimates = members
                .iter()
                .map(|hypothesis| WeightedEstimate {
                    expert_id: hypothesis.expert_id.clone(),
                    value: 1.0,
                    calibrated_confidence: hypothesis.confidence,
                    base_weight: 1.0,
                    correlation_group: hypothesis.correlation_group.clone(),
                    dependencies: hypothesis.dependencies.clone(),
                })
                .collect::<Vec<_>>();
            correlation_aware_score(&estimates).map(|score| (normalized, members, score))
        })
        .collect::<Result<Vec<_>, _>>()?;
    ranked.sort_by(|left, right| {
        right
            .2
            .total_cmp(&left.2)
            .then_with(|| left.0.cmp(&right.0))
    });
    let (_, winners, score) = ranked
        .first()
        .ok_or_else(|| "transcript fusion produced no candidates".to_string())?;
    let representative = winners
        .iter()
        .copied()
        .max_by(|left, right| left.confidence.total_cmp(&right.confidence))
        .expect("a ranked transcript group is non-empty");
    let alternatives = ranked
        .iter()
        .skip(1)
        .map(|(_, members, _)| {
            members[0]
                .tokens
                .iter()
                .map(|token| token.text.as_str())
                .collect::<Vec<_>>()
                .join(" ")
        })
        .collect();
    Ok(CanonicalLyrics {
        text: representative
            .tokens
            .iter()
            .map(|token| token.text.as_str())
            .collect::<Vec<_>>()
            .join(" "),
        language: representative.language.clone(),
        tokens: representative.tokens.clone(),
        confidence: *score,
        source_experts: winners
            .iter()
            .map(|hypothesis| hypothesis.expert_id.clone())
            .collect(),
        alternatives,
    })
}
