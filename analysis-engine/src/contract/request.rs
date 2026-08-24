use std::collections::{BTreeMap, BTreeSet};
use std::path::{Component, PathBuf};

use serde::{Deserialize, Serialize};
use uta_runtime_manager::RuntimePolicy;

use super::{EngineError, EngineErrorCode, EngineResult};

pub const ANALYZE_REQUEST_CONTRACT: &str = "uta.analysis-engine.request";
pub const ANALYZE_REQUEST_VERSION: u32 = 1;
pub const CANONICAL_TIMEBASE: u32 = 1_000_000;
pub const MAX_AUDIO_SOURCES: usize = 32;
pub const MAX_BOUNDARY_CONSTRAINTS: usize = 100_000;
pub const MAX_LYRIC_BYTES: usize = 4 * 1024 * 1024;

pub type CanonicalTime = u64;
pub type CanonicalDuration = u64;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AnalyzeRequestV1 {
    pub contract: String,
    pub version: u32,
    pub request_id: String,
    pub audio_sources: Vec<AudioSourceV1>,
    #[serde(default)]
    pub lyrics: LyricsV1,
    #[serde(default)]
    pub boundary_constraints: Vec<BoundaryConstraintV1>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub musical_context: Option<MusicalContextV1>,
    pub analysis: AnalysisSpecV1,
    pub requested_artifacts: RequestedArtifactsV1,
    #[serde(default)]
    pub execution_policy: ExecutionPolicyV1,
    #[serde(default)]
    pub extensions: BTreeMap<String, serde_json::Value>,
}

impl AnalyzeRequestV1 {
    pub fn validate(&self) -> EngineResult<()> {
        if self.contract != ANALYZE_REQUEST_CONTRACT || self.version != ANALYZE_REQUEST_VERSION {
            return Err(EngineError::new(
                EngineErrorCode::UnsupportedContractVersion,
                format!(
                    "expected {ANALYZE_REQUEST_CONTRACT}/{ANALYZE_REQUEST_VERSION}, got {}/{}",
                    self.contract, self.version
                ),
            ));
        }
        validate_identifier(&self.request_id, "request_id")?;
        if self.audio_sources.len() > MAX_AUDIO_SOURCES {
            return Err(invalid("audio source count exceeds the v1 limit"));
        }
        let primary_count = self
            .audio_sources
            .iter()
            .filter(|source| source.primary)
            .count();
        match primary_count {
            0 => {
                return Err(EngineError::new(
                    EngineErrorCode::MissingPrimarySource,
                    "exactly one primary audio source is required",
                ));
            }
            1 => {}
            _ => {
                return Err(EngineError::new(
                    EngineErrorCode::MultiplePrimarySources,
                    "exactly one primary audio source is required",
                ));
            }
        }
        let mut source_ids = BTreeSet::new();
        for source in &self.audio_sources {
            source.validate()?;
            if !source_ids.insert(&source.id) {
                return Err(invalid(format!("duplicate audio source id: {}", source.id)));
            }
        }
        let primary = self.primary_source()?;
        if matches!(
            primary.role,
            AudioRole::Instrumental | AudioRole::BackingVocal | AudioRole::HarmonyVocal
        ) {
            return Err(EngineError::new(
                EngineErrorCode::InvalidAudioRole,
                format!(
                    "audio role {:?} is reference-only and cannot be primary in v1",
                    primary.role
                ),
            ));
        }
        self.lyrics.validate()?;
        self.validate_constraints()?;
        self.analysis.validate()?;
        self.requested_artifacts.validate()?;
        if !self.requested_artifacts.requests_anything() {
            return Err(EngineError::new(
                EngineErrorCode::MissingRequiredInput,
                "requested_artifacts does not request any output",
            ));
        }
        if let Some(context) = &self.musical_context {
            context.validate()?;
        }
        if self.execution_policy.model_backend_overrides.len() > 128 {
            return Err(invalid("model backend override count exceeds the v1 limit"));
        }
        for (model_id, backend) in &self.execution_policy.model_backend_overrides {
            validate_identifier(model_id, "model backend override id")?;
            if independently_pinned_vulkan_model(model_id)
                && *backend != uta_runtime_manager::NativeBackend::Vulkan
            {
                return Err(invalid(format!(
                    "{model_id} keeps its independently pinned Vulkan backend"
                )));
            }
        }
        if self.analysis.enable_quantization {
            if !self.requested_artifacts.vocal_chart {
                return Err(EngineError::new(
                    EngineErrorCode::MissingRequiredInput,
                    "rhythm quantization requires a requested Candidate VocalChart output",
                )
                .with_capability("rhythm.quantize"));
            }
            let context = self.musical_context.as_ref().ok_or_else(|| {
                EngineError::new(
                    EngineErrorCode::MissingRequiredInput,
                    "rhythm quantization requires explicit musical_context BPM and grid",
                )
                .with_capability("rhythm.quantize")
            })?;
            if context.bpm.is_none() || context.quantization_grid.is_none() {
                return Err(EngineError::new(
                    EngineErrorCode::MissingRequiredInput,
                    "rhythm quantization requires explicit musical_context BPM and grid",
                )
                .with_capability("rhythm.quantize"));
            }
        }
        Ok(())
    }

    pub fn primary_source(&self) -> EngineResult<&AudioSourceV1> {
        let mut primary = self.audio_sources.iter().filter(|source| source.primary);
        let source = primary.next().ok_or_else(|| {
            EngineError::new(
                EngineErrorCode::MissingPrimarySource,
                "exactly one primary audio source is required",
            )
        })?;
        if primary.next().is_some() {
            return Err(EngineError::new(
                EngineErrorCode::MultiplePrimarySources,
                "exactly one primary audio source is required",
            ));
        }
        Ok(source)
    }

    fn validate_constraints(&self) -> EngineResult<()> {
        if self.boundary_constraints.len() > MAX_BOUNDARY_CONSTRAINTS {
            return Err(EngineError::new(
                EngineErrorCode::InvalidConstraints,
                "boundary constraint count exceeds the v1 limit",
            ));
        }
        let token_ids = self
            .lyrics
            .tokens
            .iter()
            .map(|token| token.id.as_str())
            .collect::<BTreeSet<_>>();
        let mut hard_ranges: BTreeMap<BoundaryLevel, Vec<(u64, u64)>> = BTreeMap::new();
        for constraint in &self.boundary_constraints {
            constraint.validate()?;
            if let Some(token_id) = constraint.token_id.as_deref()
                && !token_ids.contains(token_id)
            {
                return Err(EngineError::new(
                    EngineErrorCode::InvalidConstraints,
                    format!("boundary references unknown lyric token: {token_id}"),
                ));
            }
            if constraint.authority == BoundaryAuthority::Hard {
                hard_ranges
                    .entry(constraint.level)
                    .or_default()
                    .push((constraint.start, constraint.end()?));
            }
        }
        for ranges in hard_ranges.values_mut() {
            ranges.sort_unstable();
            if ranges.windows(2).any(|pair| pair[0].1 > pair[1].0) {
                return Err(EngineError::new(
                    EngineErrorCode::InvalidConstraints,
                    "overlapping hard boundaries at the same level are contradictory",
                ));
            }
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AudioSourceV1 {
    pub id: String,
    pub kind: AudioSourceKind,
    pub path: PathBuf,
    pub sha256: String,
    pub role: AudioRole,
    pub primary: bool,
    pub timeline: SourceTimelineV1,
}

impl AudioSourceV1 {
    pub fn validate(&self) -> EngineResult<()> {
        validate_identifier(&self.id, "audio source id")?;
        if self.kind != AudioSourceKind::LocalFile {
            return Err(invalid("v1 supports local_file audio sources only"));
        }
        if self.path.as_os_str().is_empty()
            || self
                .path
                .components()
                .any(|component| component == Component::ParentDir)
        {
            return Err(invalid(
                "audio source path is empty or contains path traversal",
            ));
        }
        self.timeline.validate()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AudioSourceKind {
    LocalFile,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AudioRole {
    OriginalMix,
    VocalStem,
    GuideVocals,
    LeadVocal,
    CleanLeadVocal,
    Instrumental,
    BackingVocal,
    HarmonyVocal,
}

impl AudioRole {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::OriginalMix => "original_mix",
            Self::VocalStem => "vocal_stem",
            Self::GuideVocals => "guide_vocals",
            Self::LeadVocal => "lead_vocal",
            Self::CleanLeadVocal => "clean_lead_vocal",
            Self::Instrumental => "instrumental",
            Self::BackingVocal => "backing_vocal",
            Self::HarmonyVocal => "harmony_vocal",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SourceTimelineV1 {
    pub timebase: u32,
    pub source_start: CanonicalTime,
}

impl SourceTimelineV1 {
    pub fn validate(&self) -> EngineResult<()> {
        if self.timebase != CANONICAL_TIMEBASE {
            return Err(EngineError::new(
                EngineErrorCode::TimelineInvalid,
                format!("v1 timeline timebase must be {CANONICAL_TIMEBASE}"),
            ));
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LyricsV1 {
    pub mode: LyricsMode,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub language: Option<String>,
    #[serde(default)]
    pub tokens: Vec<LyricTokenV1>,
}

impl Default for LyricsV1 {
    fn default() -> Self {
        Self {
            mode: LyricsMode::None,
            language: None,
            tokens: Vec::new(),
        }
    }
}

impl LyricsV1 {
    fn validate(&self) -> EngineResult<()> {
        if self.mode == LyricsMode::None && !self.tokens.is_empty() {
            return Err(invalid("lyrics mode none cannot contain tokens"));
        }
        if self.mode == LyricsMode::Canonical && self.tokens.is_empty() {
            return Err(invalid("canonical lyrics require at least one token"));
        }
        let mut ids = BTreeSet::new();
        let mut text_bytes = 0usize;
        for token in &self.tokens {
            validate_identifier(&token.id, "lyric token id")?;
            if token.text.trim().is_empty() {
                return Err(invalid("lyric token text must not be empty"));
            }
            if !ids.insert(&token.id) {
                return Err(invalid(format!("duplicate lyric token id: {}", token.id)));
            }
            text_bytes = text_bytes.saturating_add(token.text.len());
        }
        if text_bytes > MAX_LYRIC_BYTES {
            return Err(invalid("lyric text exceeds the v1 size limit"));
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LyricsMode {
    None,
    Reference,
    Canonical,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LyricTokenV1 {
    pub id: String,
    pub text: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reading: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub phonemes: Option<Vec<String>>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BoundaryConstraintV1 {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub token_id: Option<String>,
    pub level: BoundaryLevel,
    pub start: CanonicalTime,
    pub duration: CanonicalDuration,
    pub confidence: f32,
    #[serde(default)]
    pub authority: BoundaryAuthority,
    pub source: String,
}

impl BoundaryConstraintV1 {
    pub fn end(&self) -> EngineResult<CanonicalTime> {
        self.start.checked_add(self.duration).ok_or_else(|| {
            EngineError::new(
                EngineErrorCode::TimelineInvalid,
                "boundary end overflows canonical timeline",
            )
        })
    }

    fn validate(&self) -> EngineResult<()> {
        if self.duration == 0 || self.end().is_err() {
            return Err(EngineError::new(
                EngineErrorCode::InvalidConstraints,
                "boundary duration must be positive and must not overflow",
            ));
        }
        if !self.confidence.is_finite() || !(0.0..=1.0).contains(&self.confidence) {
            return Err(EngineError::new(
                EngineErrorCode::InvalidConstraints,
                "boundary confidence must be between zero and one",
            ));
        }
        if self.source.trim().is_empty() {
            return Err(EngineError::new(
                EngineErrorCode::InvalidConstraints,
                "boundary source must not be empty",
            ));
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BoundaryLevel {
    Phrase,
    Word,
    Syllable,
    Phoneme,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum BoundaryAuthority {
    #[default]
    Soft,
    Hard,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MusicalContextV1 {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub bpm: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub key: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub time_signature: Option<TimeSignatureV1>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub quantization_grid: Option<QuantizationGridV1>,
    #[serde(default)]
    pub authority: ContextAuthority,
}

impl MusicalContextV1 {
    fn validate(&self) -> EngineResult<()> {
        if self
            .bpm
            .is_some_and(|bpm| !bpm.is_finite() || bpm <= 0.0 || bpm > 1_000.0)
        {
            return Err(invalid("musical context BPM is invalid"));
        }
        if let Some(signature) = self.time_signature
            && (signature.beats == 0 || signature.unit == 0)
        {
            return Err(invalid("time signature values must be positive"));
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum QuantizationGridV1 {
    Eighth,
    Sixteenth,
    ThirtySecond,
}

impl QuantizationGridV1 {
    pub const fn steps_per_beat(self) -> u32 {
        match self {
            Self::Eighth => 2,
            Self::Sixteenth => 4,
            Self::ThirtySecond => 8,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TimeSignatureV1 {
    pub beats: u16,
    pub unit: u16,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum ContextAuthority {
    #[default]
    Hint,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AnalysisSpecV1 {
    pub profile: AnalysisProfile,
    pub track_target: TrackTarget,
    pub preserve_continuous_pitch: bool,
    pub enable_quantization: bool,
}

impl AnalysisSpecV1 {
    fn validate(&self) -> EngineResult<()> {
        Ok(())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AnalysisProfile {
    Fast,
    Balanced,
    Maximum,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TrackTarget {
    Lead,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RequestedArtifactsV1 {
    #[serde(default)]
    pub vocal_chart: bool,
    #[serde(default)]
    pub pitch_evidence: bool,
    #[serde(default)]
    pub singing_analysis: bool,
    #[serde(default)]
    pub transcript: bool,
    #[serde(default)]
    pub alignment: bool,
    #[serde(default)]
    pub stems: Vec<AudioRole>,
}

impl RequestedArtifactsV1 {
    pub fn requests_anything(&self) -> bool {
        self.vocal_chart
            || self.pitch_evidence
            || self.singing_analysis
            || self.transcript
            || self.alignment
            || !self.stems.is_empty()
    }

    fn validate(&self) -> EngineResult<()> {
        let mut stems = BTreeSet::new();
        for stem in &self.stems {
            if !matches!(
                stem,
                AudioRole::Instrumental
                    | AudioRole::GuideVocals
                    | AudioRole::LeadVocal
                    | AudioRole::BackingVocal
                    | AudioRole::HarmonyVocal
            ) {
                return Err(invalid(format!(
                    "audio role {stem:?} is not an exportable semantic stem"
                )));
            }
            if !stems.insert(*stem) {
                return Err(invalid("requested stem roles must be unique"));
            }
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ExecutionPolicyV1 {
    #[serde(default)]
    pub runtime_policy: RuntimePolicy,
    /// Explicit global model backend selection. `None` uses each model's
    /// pinned route unless an entry below overrides that model.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub requested_backend: Option<uta_runtime_manager::NativeBackend>,
    /// Per-model backend choices take precedence and remain fail-closed.
    /// Qwen and every RoFormer keep their independently pinned Vulkan runtime.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub model_backend_overrides: BTreeMap<String, uta_runtime_manager::NativeBackend>,
}

impl Default for ExecutionPolicyV1 {
    fn default() -> Self {
        Self {
            runtime_policy: RuntimePolicy::Experimental,
            requested_backend: None,
            model_backend_overrides: BTreeMap::new(),
        }
    }
}

impl ExecutionPolicyV1 {
    pub fn requested_backend_for(
        &self,
        model_id: &str,
    ) -> Option<uta_runtime_manager::NativeBackend> {
        if independently_pinned_vulkan_model(model_id) {
            return None;
        }
        self.model_backend_overrides
            .get(model_id)
            .copied()
            .or(self.requested_backend)
    }
}

fn independently_pinned_vulkan_model(model_id: &str) -> bool {
    matches!(
        model_id,
        "qwen3_asr_1_7b"
            | "qwen3_forced_aligner_0_6b"
            | "bs_roformer_vocals_ep317"
            | "melband_roformer_inst_v2"
            | "melband_roformer_harmony"
            | "melband_roformer_denoise_aufr33"
            | "melband_roformer_dereverb_anvuew"
    )
}

fn validate_identifier(value: &str, label: &str) -> EngineResult<()> {
    if value.is_empty()
        || value.len() > 128
        || !value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.'))
    {
        return Err(invalid(format!("{label} contains unsupported characters")));
    }
    Ok(())
}

fn invalid(message: impl Into<String>) -> EngineError {
    EngineError::new(EngineErrorCode::InvalidContract, message)
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    pub(crate) fn valid_request(role: AudioRole) -> AnalyzeRequestV1 {
        AnalyzeRequestV1 {
            contract: ANALYZE_REQUEST_CONTRACT.to_string(),
            version: ANALYZE_REQUEST_VERSION,
            request_id: "req-123".to_string(),
            audio_sources: vec![AudioSourceV1 {
                id: "main".to_string(),
                kind: AudioSourceKind::LocalFile,
                path: PathBuf::from("fixture.flac"),
                sha256: "a".repeat(64),
                role,
                primary: true,
                timeline: SourceTimelineV1 {
                    timebase: CANONICAL_TIMEBASE,
                    source_start: 0,
                },
            }],
            lyrics: LyricsV1::default(),
            boundary_constraints: Vec::new(),
            musical_context: None,
            analysis: AnalysisSpecV1 {
                profile: AnalysisProfile::Fast,
                track_target: TrackTarget::Lead,
                preserve_continuous_pitch: true,
                enable_quantization: false,
            },
            requested_artifacts: RequestedArtifactsV1 {
                vocal_chart: true,
                pitch_evidence: true,
                singing_analysis: true,
                transcript: true,
                alignment: true,
                stems: Vec::new(),
            },
            execution_policy: ExecutionPolicyV1::default(),
            extensions: BTreeMap::new(),
        }
    }

    #[test]
    fn valid_request_round_trips() {
        let request = valid_request(AudioRole::OriginalMix);
        request.validate().unwrap();
        let encoded = serde_json::to_vec(&request).unwrap();
        let decoded: AnalyzeRequestV1 = serde_json::from_slice(&encoded).unwrap();
        assert_eq!(decoded, request);
    }

    #[test]
    fn model_backend_override_precedes_global_and_pinned_vulkan_models_ignore_both() {
        let mut policy = ExecutionPolicyV1 {
            requested_backend: Some(uta_runtime_manager::NativeBackend::OpenVino),
            ..ExecutionPolicyV1::default()
        };
        policy.model_backend_overrides.insert(
            "fcpe".to_string(),
            uta_runtime_manager::NativeBackend::CpuReference,
        );
        assert_eq!(
            policy.requested_backend_for("fcpe"),
            Some(uta_runtime_manager::NativeBackend::CpuReference)
        );
        assert_eq!(
            policy.requested_backend_for("rmvpe"),
            Some(uta_runtime_manager::NativeBackend::OpenVino)
        );
        for model_id in [
            "qwen3_asr_1_7b",
            "bs_roformer_vocals_ep317",
            "melband_roformer_denoise_aufr33",
            "melband_roformer_dereverb_anvuew",
        ] {
            assert_eq!(policy.requested_backend_for(model_id), None);
        }
    }

    #[test]
    fn roformer_openvino_override_is_rejected() {
        let mut request = valid_request(AudioRole::OriginalMix);
        request.execution_policy.model_backend_overrides.insert(
            "melband_roformer_denoise_aufr33".to_string(),
            uta_runtime_manager::NativeBackend::OpenVino,
        );
        assert_eq!(
            request.validate().unwrap_err().code,
            EngineErrorCode::InvalidContract
        );
    }

    #[test]
    fn quantization_requires_explicit_bpm_and_grid() {
        let mut request = valid_request(AudioRole::LeadVocal);
        request.analysis.enable_quantization = true;
        let error = request.validate().unwrap_err();
        assert_eq!(error.code, EngineErrorCode::MissingRequiredInput);
        assert_eq!(error.capability.as_deref(), Some("rhythm.quantize"));

        request.musical_context = Some(MusicalContextV1 {
            bpm: Some(120.0),
            key: None,
            time_signature: Some(TimeSignatureV1 { beats: 4, unit: 4 }),
            quantization_grid: Some(QuantizationGridV1::Sixteenth),
            authority: ContextAuthority::Hint,
        });
        request.validate().unwrap();

        request.requested_artifacts.vocal_chart = false;
        let error = request.validate().unwrap_err();
        assert_eq!(error.code, EngineErrorCode::MissingRequiredInput);
        assert_eq!(error.capability.as_deref(), Some("rhythm.quantize"));
    }

    #[test]
    fn rejects_unsupported_contract_version() {
        let mut request = valid_request(AudioRole::LeadVocal);
        request.version = 2;
        assert_eq!(
            request.validate().unwrap_err().code,
            EngineErrorCode::UnsupportedContractVersion
        );
    }

    #[test]
    fn rejects_zero_or_multiple_primary_sources() {
        let mut request = valid_request(AudioRole::LeadVocal);
        request.audio_sources[0].primary = false;
        assert_eq!(
            request.validate().unwrap_err().code,
            EngineErrorCode::MissingPrimarySource
        );
        request.audio_sources[0].primary = true;
        request.audio_sources.push(request.audio_sources[0].clone());
        request.audio_sources[1].id = "other".to_string();
        assert_eq!(
            request.validate().unwrap_err().code,
            EngineErrorCode::MultiplePrimarySources
        );
    }

    #[test]
    fn rejects_instrumental_primary_float_time_and_traversal() {
        let request = valid_request(AudioRole::Instrumental);
        assert_eq!(
            request.validate().unwrap_err().code,
            EngineErrorCode::InvalidAudioRole
        );

        let json = serde_json::to_value(valid_request(AudioRole::LeadVocal)).unwrap();
        let mut json = json;
        json["audio_sources"][0]["timeline"]["source_start"] = serde_json::json!(1.5);
        assert!(serde_json::from_value::<AnalyzeRequestV1>(json).is_err());

        let mut request = valid_request(AudioRole::LeadVocal);
        request.audio_sources[0].path = PathBuf::from("../escape.flac");
        assert_eq!(
            request.validate().unwrap_err().code,
            EngineErrorCode::InvalidContract
        );
    }

    #[test]
    fn rejects_timeline_overflow_and_unknown_constraint_tokens() {
        let mut request = valid_request(AudioRole::LeadVocal);
        request.boundary_constraints.push(BoundaryConstraintV1 {
            token_id: Some("missing".to_string()),
            level: BoundaryLevel::Word,
            start: u64::MAX,
            duration: 1,
            confidence: 0.8,
            authority: BoundaryAuthority::Soft,
            source: "fixture".to_string(),
        });
        assert_eq!(
            request.validate().unwrap_err().code,
            EngineErrorCode::InvalidConstraints
        );
    }

    #[test]
    fn reference_only_roles_cannot_be_primary() {
        for role in [
            AudioRole::Instrumental,
            AudioRole::BackingVocal,
            AudioRole::HarmonyVocal,
        ] {
            assert_eq!(
                valid_request(role).validate().unwrap_err().code,
                EngineErrorCode::InvalidAudioRole
            );
        }
    }

    #[test]
    fn only_semantic_export_stems_are_accepted() {
        let mut request = valid_request(AudioRole::LeadVocal);
        request.requested_artifacts.stems = vec![AudioRole::VocalStem];
        assert_eq!(
            request.validate().unwrap_err().code,
            EngineErrorCode::InvalidContract
        );
        request.requested_artifacts.stems = vec![AudioRole::GuideVocals];
        request.validate().unwrap();
    }
}
