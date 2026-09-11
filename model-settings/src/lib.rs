//! Backend-neutral model-owned analysis settings. No runtime or model is loaded here.
use std::collections::BTreeMap;

pub type ModelSettings = BTreeMap<String, BTreeMap<String, serde_json::Value>>;

#[derive(Debug, Clone, Copy)]
pub struct Parameter {
    pub key: &'static str,
    pub label: &'static str,
    pub description: &'static str,
    pub default: f64,
    pub minimum: f64,
    pub maximum: f64,
    pub step: f64,
    pub integer: bool,
}

impl Parameter {
    pub fn value(self, value: f64) -> Result<serde_json::Value, String> {
        if !value.is_finite() {
            return Err(format!("{} must be a finite number", self.label));
        }
        let value = value.clamp(self.minimum, self.maximum);
        Ok(if self.integer {
            serde_json::json!(value.round() as u64)
        } else {
            // Decimal controls must not accumulate binary step noise in saved JSON.
            serde_json::json!((value * 1_000_000.0).round() / 1_000_000.0)
        })
    }
}

#[derive(Debug, Clone, Copy)]
pub struct Model {
    pub id: &'static str,
    pub label: &'static str,
    pub parameters: &'static [Parameter],
    pub note: &'static str,
}

const OVERLAP: &[Parameter] = &[Parameter {
    key: "overlap", label: "Overlap factor", default: 2.0,
    minimum: 1.0, maximum: 16.0, step: 1.0, integer: true,
    description: "Chunk stride = chunk length / factor (2 = 50%, 4 = 75%). More overlap increases work and may reduce seam artifacts; it does not guarantee better separation. Reset uses the model file's own default.",
}];
const GAME: &[Parameter] = &[
    Parameter { key: "sampling_steps", label: "Sampling steps", description: "D3PM boundary refinement steps. More steps cost more inference time; accuracy is not monotonic.", default: 8.0, minimum: 1.0, maximum: 128.0, step: 1.0, integer: true },
    Parameter { key: "boundary_threshold", label: "Boundary threshold", description: "Lower values propose more note boundaries; higher values can merge notes. This is sensitivity, not a quality score.", default: 0.2, minimum: 0.0, maximum: 1.0, step: 0.01, integer: false },
    Parameter { key: "note_threshold", label: "Voiced-note threshold", description: "Controls voiced/unvoiced note decisions independently of boundary sensitivity.", default: 0.2, minimum: 0.0, maximum: 1.0, step: 0.01, integer: false },
];
const RMVPE: &[Parameter] = &[Parameter { key: "voiced_threshold", label: "Voiced threshold", description: "Minimum salience for voiced F0. Lower values retain faint vocals but may admit noise; raw frequency and confidence remain available.", default: 0.03, minimum: 0.0, maximum: 1.0, step: 0.005, integer: false }];
const FCPE: &[Parameter] = &[Parameter { key: "voiced_threshold", label: "Voiced threshold", description: "Minimum local pitch salience. Lower values retain faint pitch but can increase false voiced frames.", default: 0.006, minimum: 0.0, maximum: 1.0, step: 0.001, integer: false }];
const JBM: &[Parameter] = &[
    Parameter { key: "onset_threshold", label: "Onset threshold", description: "Minimum onset score to begin a note; lower values detect more attacks.", default: 0.32, minimum: 0.0, maximum: 1.0, step: 0.01, integer: false },
    Parameter { key: "offset_threshold", label: "Offset threshold", description: "Minimum offset score to end a note; lower values can shorten sustained notes.", default: 0.7, minimum: 0.0, maximum: 1.0, step: 0.01, integer: false },
];
const STARS: &[Parameter] = &[Parameter { key: "boundary_threshold", label: "Note boundary threshold", description: "STARS rhythm decoder sensitivity. Lower values propose more boundaries; transcript and pitch conditioning are unchanged.", default: 0.8, minimum: 0.0, maximum: 1.0, step: 0.01, integer: false }];
const ROSVOT: &[Parameter] = &[Parameter { key: "boundary_threshold", label: "Note boundary threshold", description: "ROSVOT boundary decoder sensitivity. Lower values propose more boundaries; word-boundary guidance is retained.", default: 0.85, minimum: 0.0, maximum: 1.0, step: 0.01, integer: false }];
const QWEN: &[Parameter] = &[Parameter { key: "max_new_tokens", label: "Maximum new tokens / window", description: "Decoder budget per audio window. Increase for dense lyrics; higher values cost more on unfinished windows and do not guarantee more accurate text.", default: 256.0, minimum: 1.0, maximum: 4096.0, step: 64.0, integer: true }];
const FIRERED: &[Parameter] = &[Parameter { key: "max_new_tokens", label: "Maximum new tokens / window", description: "Greedy decoder budget for the native 2.3-second window. The native decoder supports up to 58 steps; upstream beam search is not implemented here.", default: 58.0, minimum: 1.0, maximum: 58.0, step: 1.0, integer: true }];
const SERIAL: &str = "Upstream supports batching. Both packaged native routes currently execute one window at a time; batch size is not an active setting. Chunk length and model precision remain model-owned.";

pub const MODELS: &[Model] = &[
    Model { id: "bs_roformer_leap_xe90_vocals", label: "BS-RoFormer Leap XE90 · Vocals", parameters: OVERLAP, note: SERIAL },
    Model { id: "bs_roformer_leap_xe90_instrumental", label: "BS-RoFormer Leap XE90 · Instrumental", parameters: OVERLAP, note: SERIAL },
    Model { id: "bs_polarformer_public_instrumental", label: "BS-PolarFormer Public", parameters: OVERLAP, note: SERIAL },
    Model { id: "melband_roformer_harmony", label: "MelBand-RoFormer · Harmony", parameters: OVERLAP, note: SERIAL },
    Model { id: "melband_roformer_denoise_aufr33", label: "MelBand-RoFormer · Denoise", parameters: OVERLAP, note: SERIAL },
    Model { id: "melband_roformer_dereverb_anvuew", label: "MelBand-RoFormer · Dereverb", parameters: OVERLAP, note: SERIAL },
    Model { id: "rmvpe", label: "RMVPE", parameters: RMVPE, note: "Continuous pitch expert. Sensitivity does not change the 10 ms evidence grid." },
    Model { id: "fcpe", label: "FCPE", parameters: FCPE, note: "Independent pitch expert, not an RMVPE fallback. Native batching is not exposed." },
    Model { id: "basic_pitch", label: "Basic Pitch", parameters: &[], note: "Upstream exposes MIDI onset/frame thresholds, but Studio consumes raw note/onset/contour activations, not that MIDI decoder. Its fixed 30-frame overlap remains unchanged; no inactive decoder or batch controls are shown." },
    Model { id: "game_1_0_3_small", label: "GAME · Small", parameters: GAME, note: SERIAL },
    Model { id: "game_1_0_3_medium", label: "GAME · Medium", parameters: GAME, note: SERIAL },
    Model { id: "game_1_0_3_large", label: "GAME · Large", parameters: GAME, note: SERIAL },
    Model { id: "jbm555_cectc_80", label: "JBM555", parameters: JBM, note: "Mix + prepared vocal dual-input expert. Onset and offset thresholds are independent." },
    Model { id: "stars", label: "STARS", parameters: STARS, note: SERIAL },
    Model { id: "rosvot", label: "ROSVOT", parameters: ROSVOT, note: SERIAL },
    Model { id: "firered_asr2_aed", label: "FireRed ASR · AED", parameters: FIRERED, note: "Upstream offers batch/beam decoding; the native route is serial greedy decoding. These settings never substitute FireRed for the primary transcript." },
    Model { id: "qwen3_asr_1_7b", label: "Qwen3-ASR · 1.7B", parameters: QWEN, note: SERIAL },
    Model { id: "qwen3_forced_aligner_0_6b", label: "Qwen3 Forced Aligner · 0.6B", parameters: &[], note: "Timestamp classification, not text generation: no beam, temperature or generation-token quality knob. Upstream supports batching, but native alignment is serial. Timestamp resolution is fixed by the trained model." },
];

pub fn parameter(model: &str, key: &str) -> Option<Parameter> {
    MODELS.iter().find(|item| item.id == model)?.parameters.iter().find(|item| item.key == key).copied()
}

/// Normalize only the edited control. Other model choices are never rewritten.
pub fn set(settings: &mut ModelSettings, model: &str, key: &str, value: f64) -> Result<(), String> {
    let parameter = parameter(model, key).ok_or_else(|| format!("No native parameter {model}.{key}"))?;
    settings.entry(model.to_string()).or_default().insert(key.to_string(), parameter.value(value)?);
    Ok(())
}

pub fn validate(settings: &ModelSettings) -> Result<(), String> {
    for (model, values) in settings {
        for (key, value) in values {
            let parameter = parameter(model, key).ok_or_else(|| format!("No native parameter {model}.{key}"))?;
            let number = value.as_f64().ok_or_else(|| format!("{model}.{key} must be numeric"))?;
            if !number.is_finite() || number < parameter.minimum || number > parameter.maximum || (parameter.integer && number.fract() != 0.0) {
                return Err(format!("{model}.{key} is outside its supported control range"));
            }
        }
    }
    Ok(())
}

pub fn number(config: &serde_json::Value, key: &str, default: f64) -> f64 {
    config.get("model_settings").and_then(|settings| settings.get(key)).and_then(serde_json::Value::as_f64).unwrap_or(default)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn controls_are_owned_numeric_and_clamped() {
        let mut settings = ModelSettings::new();
        set(&mut settings, "rmvpe", "voiced_threshold", -1.0).unwrap();
        set(&mut settings, "fcpe", "voiced_threshold", 0.007).unwrap();
        set(&mut settings, "game_1_0_3_small", "sampling_steps", 12.4).unwrap();
        assert_eq!(settings["rmvpe"]["voiced_threshold"], 0.0);
        assert_eq!(settings["fcpe"]["voiced_threshold"], 0.007);
        assert_eq!(settings["game_1_0_3_small"]["sampling_steps"], 12);
        validate(&settings).unwrap();
        assert!(set(&mut settings, "basic_pitch", "batch_size", 4.0).is_err());
        assert!(set(&mut settings, "rmvpe", "voiced_threshold", f64::NAN).is_err());
    }
    #[test]
    fn each_model_has_its_own_explanation_and_valid_defaults() {
        let mut settings = ModelSettings::new();
        let mut ids = std::collections::BTreeSet::new();
        for model in MODELS {
            assert!(ids.insert(model.id));
            assert!(!model.note.is_empty());
            for control in model.parameters {
                set(&mut settings, model.id, control.key, control.default).unwrap();
            }
        }
        validate(&settings).unwrap();
        let roundtrip: ModelSettings = serde_json::from_str(&serde_json::to_string(&settings).unwrap()).unwrap();
        assert_eq!(settings, roundtrip);
    }
}
