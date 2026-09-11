//! Pure, run-scoped projection used by DAG Inspect and local automation.
//! No current artifact lookup, source decode, inference or reference-score estimation.
use crate::EngineRunHistoryProjection;
use crate::backend_cli::{AnalysisDiagnosticsWire, SeparationQualityEvidenceWire};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SeparationInspection {
    pub request_id: String,
    pub node_id: String,
    pub evidence: Option<SeparationQualityEvidenceWire>,
    pub explanation: String,
}

#[derive(Deserialize)]
struct ResultProjection {
    request_id: String,
    diagnostics: AnalysisDiagnosticsWire,
}

pub fn inspect_separation_quality(
    engine: &EngineRunHistoryProjection,
    node_id: &str,
) -> Result<SeparationInspection, String> {
    let evidence = if let Some(json) = engine.result_json.as_deref() {
        let result: ResultProjection = serde_json::from_str(json).map_err(|error| {
            format!("Could not read this run's separation measurements: {error}")
        })?;
        if result.request_id != engine.request_id {
            return Err("Separation measurements belong to a different analysis request.".into());
        }
        result
            .diagnostics
            .separation_quality
            .into_iter()
            .find(|evidence| evidence.node_id == node_id)
    } else {
        None
    };
    let explanation = if evidence.is_some() {
        "Measured from this run's decoded separation outputs, before lead isolation or cleanup. These signal statistics are not perceptual quality scores."
    } else {
        "No separation measurements were recorded for this node in the selected run. Pending, reused and unmeasured outputs are not assigned scores; current artifacts are not substituted."
    };
    Ok(SeparationInspection {
        request_id: engine.request_id.clone(),
        node_id: node_id.into(),
        evidence,
        explanation: explanation.into(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn run() -> EngineRunHistoryProjection {
        EngineRunHistoryProjection {
            request_id: "request:selected".into(),
            request_json: "{}".into(),
            request_digest: "fixture".into(),
            plan_json: "{}".into(),
            result_json: Some(
                serde_json::json!({
                    "request_id": "request:selected",
                    "diagnostics": {"separation_quality": [{
                        "node_id": "separate", "model_id": "separator",
                        "measurement": "decoded_separation_output",
                        "reference_status": "unavailable_no_ground_truth_stems",
                        "stems": [{
                            "role": "guide_vocals", "artifact_path": "stems/guide_vocals.flac",
                            "sample_rate": 44100, "channels": 2, "frame_count": 44100,
                            "duration_seconds": 1.0, "sample_count": 88200,
                            "finite_samples": true, "peak_amplitude": 0.9, "rms_amplitude": 0.2,
                            "near_full_scale_ratio": 0.0, "silent_sample_ratio": 0.1
                        }]
                    }]}
                })
                .to_string(),
            ),
            fingerprint: None,
            source_sha256: "fixture".into(),
        }
    }

    #[test]
    fn separation_inspection_is_exact_node_and_run_scoped() {
        let engine = run();
        let inspection = inspect_separation_quality(&engine, "separate").unwrap();
        let evidence = inspection.evidence.unwrap();
        assert_eq!(inspection.request_id, engine.request_id);
        assert_eq!(evidence.model_id, "separator");
        assert_eq!(evidence.stems[0].rms_amplitude, 0.2);
        assert_eq!(
            evidence.reference_status,
            "unavailable_no_ground_truth_stems"
        );
        assert!(
            inspect_separation_quality(&engine, "cleanup")
                .unwrap()
                .evidence
                .is_none()
        );
    }

    #[test]
    fn separation_inspection_does_not_invent_pending_or_reused_measurements() {
        let mut engine = run();
        engine.result_json = None;
        assert!(
            inspect_separation_quality(&engine, "separate")
                .unwrap()
                .evidence
                .is_none()
        );
        engine.result_json = Some(
            serde_json::json!({
                "request_id": engine.request_id, "diagnostics": {"separation_quality": []}
            })
            .to_string(),
        );
        assert!(
            inspect_separation_quality(&engine, "separate")
                .unwrap()
                .evidence
                .is_none()
        );
    }

    #[test]
    fn separation_inspection_surfaces_malformed_or_cross_request_results() {
        let mut engine = run();
        engine.request_id = "different".into();
        assert!(
            inspect_separation_quality(&engine, "separate")
                .unwrap_err()
                .contains("different analysis request")
        );
        engine.result_json = Some("{broken".into());
        assert!(
            inspect_separation_quality(&engine, "separate")
                .unwrap_err()
                .contains("Could not read")
        );
    }

    #[test]
    fn separation_inspection_is_discoverable_as_read_only() {
        let capability = crate::api_capabilities()
            .iter()
            .find(|entry| entry.command == "inspect_separation_quality")
            .unwrap();
        assert_eq!(capability.access, "read");
    }
}
