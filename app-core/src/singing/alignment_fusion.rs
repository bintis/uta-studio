use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use super::{FusedEstimate, TimeRange, WeightedEstimate, fuse_scalar};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct WordBoundaryEvidence {
    pub word_id: String,
    pub text: String,
    pub range: TimeRange,
    pub confidence: f32,
    pub expert_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub correlation_group: Option<String>,
    #[serde(default)]
    pub dependencies: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CanonicalWordBoundary {
    pub word_id: String,
    pub text: String,
    pub range: TimeRange,
    pub confidence: f32,
    pub disagreement_seconds: f32,
    pub source_experts: Vec<String>,
}

fn boundary_estimates(
    evidence: &[&WordBoundaryEvidence],
    value: impl Fn(&WordBoundaryEvidence) -> f32,
) -> Vec<WeightedEstimate> {
    evidence
        .iter()
        .map(|item| WeightedEstimate {
            expert_id: item.expert_id.clone(),
            value: value(item),
            calibrated_confidence: item.confidence,
            base_weight: 1.0,
            correlation_group: item.correlation_group.clone(),
            dependencies: item.dependencies.clone(),
        })
        .collect()
}

pub fn fuse_word_boundaries(
    evidence: &[WordBoundaryEvidence],
) -> Result<Vec<CanonicalWordBoundary>, String> {
    let mut groups: BTreeMap<&str, Vec<&WordBoundaryEvidence>> = BTreeMap::new();
    for item in evidence {
        if item.word_id.is_empty()
            || item.text.is_empty()
            || !item.confidence.is_finite()
            || item.range.start < 0.0
            || item.range.end <= item.range.start
        {
            return Err("word-boundary evidence is invalid".to_string());
        }
        groups.entry(&item.word_id).or_default().push(item);
    }
    let mut words = Vec::with_capacity(groups.len());
    for (word_id, items) in groups {
        let start: FusedEstimate =
            fuse_scalar(&boundary_estimates(&items, |item| item.range.start as f32))?;
        let end: FusedEstimate =
            fuse_scalar(&boundary_estimates(&items, |item| item.range.end as f32))?;
        if end.value <= start.value {
            return Err(format!("fused boundary for {word_id} is empty"));
        }
        let representative = items
            .iter()
            .copied()
            .max_by(|left, right| left.confidence.total_cmp(&right.confidence))
            .expect("a boundary group is non-empty");
        words.push(CanonicalWordBoundary {
            word_id: word_id.to_string(),
            text: representative.text.clone(),
            range: TimeRange {
                start: start.value as f64,
                end: end.value as f64,
            },
            confidence: ((start.confidence + end.confidence) * 0.5).clamp(0.0, 1.0),
            disagreement_seconds: start.disagreement.max(end.disagreement),
            source_experts: start.contributors,
        });
    }
    words.sort_by(|left, right| left.range.start.total_cmp(&right.range.start));
    if words
        .windows(2)
        .any(|pair| pair[0].range.start > pair[1].range.start)
    {
        return Err("fused words are not ordered".to_string());
    }
    Ok(words)
}
