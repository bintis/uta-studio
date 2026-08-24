use std::fmt;

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct CapabilityId(String);

impl CapabilityId {
    pub fn new(value: impl Into<String>) -> Result<Self, String> {
        let value = value.into();
        if value.is_empty()
            || !value.bytes().all(|byte| {
                byte.is_ascii_lowercase()
                    || byte.is_ascii_digit()
                    || matches!(byte, b'.' | b'_' | b'-')
            })
        {
            return Err(format!("invalid capability id: {value}"));
        }
        Ok(Self(value))
    }

    pub fn from_static(value: &'static str) -> Self {
        Self::new(value).expect("built-in capability ids are valid")
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for CapabilityId {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.0)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CapabilityDescriptor {
    pub id: CapabilityId,
    pub input_semantic_types: Vec<String>,
    pub output_semantic_types: Vec<String>,
    pub baseline_required: bool,
    pub implementation_exists: bool,
    pub runtime_policy_satisfied: bool,
}

pub fn capability_registry() -> Vec<CapabilityDescriptor> {
    let definitions = [
        ("audio.decode", true, true),
        ("audio.extract_vocals", true, true),
        ("audio.extract_instrumental", false, true),
        ("audio.lead_isolate", true, true),
        ("audio.lead_partition", false, false),
        ("audio.denoise", false, true),
        ("audio.dereverb", false, true),
        ("speech.transcribe", true, true),
        ("speech.transcribe.challenger", false, true),
        ("speech.align", true, true),
        ("pitch.track", true, true),
        ("pitch.secondary", false, true),
        ("notes.game", true, true),
        ("notes.basic_pitch", false, true),
        ("notes.rosvot", false, true),
        ("notes.stars", false, true),
        ("technique.analyze", true, false),
        ("analysis.acoustic_dsp", true, true),
        ("fusion.transcript", true, true),
        ("fusion.alignment", true, true),
        ("fusion.singing", true, true),
        ("fusion.candidate_graph", true, true),
        ("finalize.vocal_chart", true, true),
        ("rhythm.quantize", true, true),
    ];
    definitions
        .into_iter()
        .map(|(id, required, implemented)| CapabilityDescriptor {
            id: CapabilityId::from_static(id),
            input_semantic_types: capability_inputs(id),
            output_semantic_types: capability_outputs(id),
            baseline_required: required,
            implementation_exists: implemented,
            runtime_policy_satisfied: implemented,
        })
        .collect()
}

fn capability_inputs(id: &str) -> Vec<String> {
    let values: &[&str] = if id == "audio.decode" {
        &["local_file"]
    } else if id.starts_with("audio.") {
        &["decoded_audio"]
    } else if matches!(id, "notes.rosvot" | "notes.stars") {
        &["analysis_ready_lead", "timed_transcript"]
    } else if id.starts_with("speech.")
        || id.starts_with("pitch.")
        || id.starts_with("notes.")
        || id.starts_with("technique.")
        || id == "analysis.acoustic_dsp"
    {
        &["analysis_ready_lead"]
    } else if id == "fusion.singing" {
        &["canonical_evidence"]
    } else if id == "fusion.candidate_graph" {
        &["singing_candidates"]
    } else if id.starts_with("fusion.") {
        &["canonical_evidence"]
    } else if id == "finalize.vocal_chart" {
        &["canonical_singing_track"]
    } else if id == "rhythm.quantize" {
        &["candidate_vocal_chart"]
    } else {
        &[]
    };
    values.iter().map(|value| (*value).to_string()).collect()
}

fn capability_outputs(id: &str) -> Vec<String> {
    let values: &[&str] = match id {
        "audio.decode" => &["decoded_audio", "decoded_audio_facts"],
        "audio.extract_vocals" => &["guide_vocals"],
        "audio.extract_instrumental" => &["instrumental"],
        "audio.lead_isolate" => &["lead_vocal", "vocal_residual"],
        "audio.lead_partition" => &["partitioned_lead_vocals"],
        "audio.denoise" | "audio.dereverb" => &["clean_lead_vocal"],
        "speech.transcribe" | "speech.transcribe.challenger" => &["transcript_evidence"],
        "speech.align" => &["alignment_evidence"],
        "pitch.track" | "pitch.secondary" => &["pitch_evidence"],
        "notes.game" | "notes.basic_pitch" | "notes.rosvot" | "notes.stars" => {
            &["note_candidate_evidence"]
        }
        "technique.analyze" => &["technique_evidence"],
        "analysis.acoustic_dsp" => &["acoustic_evidence"],
        "fusion.transcript" => &["transcript"],
        "fusion.alignment" => &["alignment"],
        "fusion.singing" => &["singing_candidates"],
        "fusion.candidate_graph" => &["canonical_singing_track"],
        "finalize.vocal_chart" => &["candidate_vocal_chart"],
        "rhythm.quantize" => &["quantized_candidate_vocal_chart"],
        _ => &[],
    };
    values.iter().map(|value| (*value).to_string()).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn advanced_note_experts_require_timed_transcript_without_enabling_technique() {
        let capabilities = capability_registry();
        for id in ["notes.rosvot", "notes.stars"] {
            let capability = capabilities
                .iter()
                .find(|capability| capability.id.as_str() == id)
                .unwrap();
            assert_eq!(
                capability.input_semantic_types,
                ["analysis_ready_lead", "timed_transcript"]
            );
            assert_eq!(
                capability.output_semantic_types,
                ["note_candidate_evidence"]
            );
        }
        assert!(
            !capabilities
                .iter()
                .find(|capability| capability.id.as_str() == "technique.analyze")
                .unwrap()
                .implementation_exists
        );
    }

    #[test]
    fn denoise_is_implemented_with_only_clean_lead_output_semantics() {
        let denoise = capability_registry()
            .into_iter()
            .find(|capability| capability.id.as_str() == "audio.denoise")
            .unwrap();
        assert!(denoise.implementation_exists);
        assert_eq!(denoise.output_semantic_types, vec!["clean_lead_vocal"]);
        let dereverb = capability_registry()
            .into_iter()
            .find(|capability| capability.id.as_str() == "audio.dereverb")
            .unwrap();
        assert!(dereverb.implementation_exists);
    }

    #[test]
    fn implemented_optional_executors_are_not_reported_as_missing() {
        let capabilities = capability_registry();
        for id in [
            "audio.dereverb",
            "speech.transcribe.challenger",
            "pitch.secondary",
            "notes.basic_pitch",
        ] {
            let capability = capabilities
                .iter()
                .find(|capability| capability.id.as_str() == id)
                .unwrap();
            assert!(!capability.baseline_required, "{id} must remain optional");
            assert!(
                capability.implementation_exists,
                "{id} has an Engine executor and must not be reported as unimplemented"
            );
        }
    }
}
