use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use super::{FusedEstimate, TimeRange, WeightedEstimate, fuse_scalar};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct WordBoundaryEvidence {
    pub word_id: String,
    pub text: String,
    pub range: TimeRange,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub confidence: Option<f32>,
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
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub confidence: Option<f32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub disagreement: Option<u64>,
    pub source_experts: Vec<String>,
}

fn boundary_estimates(
    evidence: &[&WordBoundaryEvidence],
    origin: u64,
    value: impl Fn(&WordBoundaryEvidence) -> u64,
) -> Option<Vec<WeightedEstimate>> {
    evidence
        .iter()
        .map(|item| {
            item.confidence.map(|confidence| WeightedEstimate {
                expert_id: item.expert_id.clone(),
                value: (value(item) - origin) as f32,
                calibrated_confidence: confidence,
                base_weight: 1.0,
                correlation_group: item.correlation_group.clone(),
                dependencies: item.dependencies.clone(),
            })
        })
        .collect()
}

fn restore_time(origin: u64, estimate: &FusedEstimate) -> Result<u64, String> {
    if !estimate.value.is_finite() || estimate.value < 0.0 {
        return Err("fused canonical boundary is invalid".to_string());
    }
    origin
        .checked_add(estimate.value.round() as u64)
        .ok_or_else(|| "fused canonical boundary overflows".to_string())
}

/// Fuses word boundaries without manufacturing weights for experts that do not
/// expose calibrated confidence. A single expert is canonicalized directly.
/// Multiple unknown-confidence experts may pass only when their ranges agree
/// exactly; conflicting unrankable ranges fail closed.
pub fn fuse_word_boundaries(
    evidence: &[WordBoundaryEvidence],
) -> Result<Vec<CanonicalWordBoundary>, String> {
    if evidence.is_empty() {
        return Err("alignment fusion requires at least one boundary".to_string());
    }
    let mut groups: BTreeMap<&str, Vec<&WordBoundaryEvidence>> = BTreeMap::new();
    for item in evidence {
        if item.word_id.is_empty()
            || item.text.trim().is_empty()
            || item.expert_id.trim().is_empty()
            || item.range.end <= item.range.start
            || item
                .confidence
                .is_some_and(|value| !value.is_finite() || !(0.0..=1.0).contains(&value))
        {
            return Err("word-boundary evidence is invalid".to_string());
        }
        groups.entry(&item.word_id).or_default().push(item);
    }
    let mut words = Vec::with_capacity(groups.len());
    for (word_id, mut items) in groups {
        items.sort_by(|left, right| left.expert_id.cmp(&right.expert_id));
        if items.iter().any(|item| item.text != items[0].text) {
            return Err(format!("alignment experts disagree on text for {word_id}"));
        }
        let source_experts = items
            .iter()
            .map(|item| item.expert_id.clone())
            .collect::<Vec<_>>();
        let (range, confidence, disagreement) = if items.len() == 1 {
            (items[0].range, items[0].confidence, None)
        } else if items.iter().any(|item| item.confidence.is_none()) {
            if items.iter().any(|item| item.range != items[0].range) {
                return Err(format!(
                    "alignment experts for {word_id} conflict without comparable confidence"
                ));
            }
            (items[0].range, None, Some(0))
        } else {
            let origin = items
                .iter()
                .map(|item| item.range.start)
                .min()
                .expect("a boundary group is non-empty");
            let start = fuse_scalar(
                &boundary_estimates(&items, origin, |item| item.range.start)
                    .expect("all confidence values were checked"),
            )?;
            let end = fuse_scalar(
                &boundary_estimates(&items, origin, |item| item.range.end)
                    .expect("all confidence values were checked"),
            )?;
            let start_time = restore_time(origin, &start)?;
            let end_time = restore_time(origin, &end)?;
            if end_time <= start_time {
                return Err(format!("fused boundary for {word_id} is empty"));
            }
            (
                TimeRange {
                    start: start_time,
                    end: end_time,
                },
                Some(((start.confidence + end.confidence) * 0.5).clamp(0.0, 1.0)),
                Some(start.disagreement.max(end.disagreement).round().max(0.0) as u64),
            )
        };
        words.push(CanonicalWordBoundary {
            word_id: word_id.to_string(),
            text: items[0].text.clone(),
            range,
            confidence,
            disagreement,
            source_experts,
        });
    }
    words.sort_by(|left, right| {
        left.range
            .start
            .cmp(&right.range.start)
            .then_with(|| left.word_id.cmp(&right.word_id))
    });
    if words
        .windows(2)
        .any(|pair| pair[0].range.end > pair[1].range.start)
    {
        return Err("fused word boundaries overlap".to_string());
    }
    Ok(words)
}
