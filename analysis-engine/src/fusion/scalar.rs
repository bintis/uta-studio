use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct WeightedEstimate {
    pub expert_id: String,
    pub value: f32,
    pub calibrated_confidence: f32,
    pub base_weight: f32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub correlation_group: Option<String>,
    #[serde(default)]
    pub dependencies: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct FusedEstimate {
    pub value: f32,
    pub confidence: f32,
    pub disagreement: f32,
    pub contributors: Vec<String>,
}

fn effective_weights(estimates: &[WeightedEstimate]) -> Result<Vec<f32>, String> {
    let mut weights = Vec::with_capacity(estimates.len());
    for estimate in estimates {
        if !estimate.value.is_finite()
            || !estimate.calibrated_confidence.is_finite()
            || !estimate.base_weight.is_finite()
            || estimate.base_weight < 0.0
        {
            return Err("fusion estimate contains invalid values".to_string());
        }
        let dependency_discount = 0.8_f32.powi(estimate.dependencies.len() as i32);
        weights.push(
            estimate.base_weight
                * estimate.calibrated_confidence.clamp(0.0, 1.0)
                * dependency_discount,
        );
    }

    // A correlated family may contribute at most its strongest member's raw
    // weight. This prevents two projections of the same upstream evidence
    // from masquerading as two independent votes.
    let mut groups: BTreeMap<&str, Vec<usize>> = BTreeMap::new();
    for (index, estimate) in estimates.iter().enumerate() {
        if let Some(group) = estimate.correlation_group.as_deref() {
            groups.entry(group).or_default().push(index);
        }
    }
    for indices in groups.values() {
        let sum = indices.iter().map(|index| weights[*index]).sum::<f32>();
        let cap = indices
            .iter()
            .map(|index| weights[*index])
            .fold(0.0_f32, f32::max);
        if sum > cap && sum > 0.0 {
            let scale = cap / sum;
            for index in indices {
                weights[*index] *= scale;
            }
        }
    }
    Ok(weights)
}

pub fn fuse_scalar(estimates: &[WeightedEstimate]) -> Result<FusedEstimate, String> {
    if estimates.is_empty() {
        return Err("fusion requires at least one available estimate".to_string());
    }
    let weights = effective_weights(estimates)?;
    let total = weights.iter().sum::<f32>();
    if total <= f32::EPSILON {
        return Err("fusion estimates have no usable confidence".to_string());
    }
    let value = estimates
        .iter()
        .zip(&weights)
        .map(|(estimate, weight)| estimate.value * weight)
        .sum::<f32>()
        / total;
    let variance = estimates
        .iter()
        .zip(&weights)
        .map(|(estimate, weight)| weight * (estimate.value - value).powi(2))
        .sum::<f32>()
        / total;
    let confidence = estimates
        .iter()
        .zip(&weights)
        .map(|(estimate, weight)| estimate.calibrated_confidence.clamp(0.0, 1.0) * weight)
        .sum::<f32>()
        / total;
    Ok(FusedEstimate {
        value,
        confidence,
        disagreement: variance.sqrt(),
        contributors: estimates
            .iter()
            .filter(|estimate| estimate.calibrated_confidence > 0.0)
            .map(|estimate| estimate.expert_id.clone())
            .collect(),
    })
}

pub fn correlation_aware_score(estimates: &[WeightedEstimate]) -> Result<f32, String> {
    if estimates.is_empty() {
        return Ok(0.0);
    }
    let weights = effective_weights(estimates)?;
    Ok(weights.iter().sum::<f32>().clamp(0.0, 1.0))
}
