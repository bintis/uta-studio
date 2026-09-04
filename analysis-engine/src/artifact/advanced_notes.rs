use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::contract::{CANONICAL_TIMEBASE, EngineError, EngineErrorCode, EngineResult};
use crate::fusion::{EvidenceProvenance, ExpertTask, TimeRange};

use super::timed_notes::{
    TIMED_NOTE_EVIDENCE_CONTRACT, TIMED_NOTE_EVIDENCE_VERSION, TimedNoteExpertEvidenceV1,
    TimedNoteHypothesisV1,
};

const MAX_EVIDENCE_BYTES: u64 = 256 * 1024 * 1024;
const MAX_FRAMES: usize = 4 * 60 * 60 * 188;
const ROSVOT_COMMIT: &str = "3c8332bf43adae35f6e4d64971862f2f6139b310";
#[cfg(test)]
const ROSVOT_CHECKPOINT: &str = "7501fb5f913d971c2f51bcb3063b930027b03206581820a4d2bfdc394c9c3fcb";
#[cfg(test)]
const ROSVOT_CONFIG: &str = "2ad2cb756623418c471b7dc2f56175cce88b69a70b4a2c354fa1a78525aa54e2";
const STARS_COMMIT: &str = "f0e43e96cfe953f71a6cf9efd8b908b2c9d7e167";
#[cfg(test)]
const STARS_CHECKPOINT: &str = "9159dd37516918448b0815ed86e1e3976d39c3044117da78db0ef65d1941db3c";
#[cfg(test)]
const STARS_CONFIG: &str = "01e8a495ba2e47b47b21fccda8db2605c85ec76cdaae258768d10a459e4e7e91";
#[cfg(test)]
const ANNOTATION_RMVPE: &str = "19dc1809cf4cdb0a18db93441816bc327e14e5644b72eeaae5220560c6736fe2";
const FRONTEND_PROFILE: &str = "shared-singing-frontend-24k-v1";
const G2P_PROFILE: &str = "stars-chinese-g2p-pypinyin-0.55.0-v1";
#[cfg(test)]
const G2P_ASSET_SHA256: &str = "433fcd2a7379cb9554a7a0dfe254746c3c7ee70bfd5de4fa18c1462757b888a5";
const TECHNIQUE_TAXONOMY: [&str; 9] = [
    "bubble",
    "breathe",
    "pharyngeal",
    "vibrato",
    "glissando",
    "mixed",
    "falsetto",
    "weak",
    "strong",
];
const STYLE_HEADS: [&str; 7] = [
    "technique_group",
    "language",
    "gender",
    "emotion",
    "method",
    "pace",
    "range",
];

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DependencyKind {
    SharedFrontend,
    AnnotationRmvpe,
    TimedTranscript,
    ChineseG2p,
}

impl DependencyKind {
    fn as_str(&self) -> &'static str {
        match self {
            Self::SharedFrontend => "shared_frontend",
            Self::AnnotationRmvpe => "annotation_rmvpe",
            Self::TimedTranscript => "timed_transcript",
            Self::ChineseG2p => "chinese_g2p",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DependencyIdentity {
    pub kind: DependencyKind,
    pub generation: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AdvancedRawNoteV1 {
    pub start_frame: usize,
    pub end_frame: usize,
    pub pitch_logits: Vec<f32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub midi: Option<u8>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AdvancedNoteEvidenceV1 {
    pub schema_version: u32,
    pub model_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub capability: Option<String>,
    #[serde(default)]
    pub capabilities: Vec<String>,
    pub upstream_commit: String,
    pub checkpoint_sha256: String,
    pub config_sha256: String,
    pub model_generation: String,
    pub runtime_manifest_sha256: String,
    pub backend: String,
    pub shared_frontend_profile: String,
    pub shared_frontend_generation: String,
    pub annotation_rmvpe_sha256: String,
    pub word_boundary_source: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub g2p_profile: Option<String>,
    pub frame_step_num: u32,
    pub frame_step_den: u32,
    pub valid_frames: usize,
    pub note_boundary_logits: Vec<f32>,
    pub regulated_note_boundaries: Vec<usize>,
    pub notes: Vec<AdvancedRawNoteV1>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub technique_taxonomy: Option<Vec<String>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub technique_calibration: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub techniques: Option<Vec<AdvancedRawTechniqueV1>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub style_scope: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub styles: Option<Vec<AdvancedRawGlobalStyleV1>>,
    pub dependencies: Vec<DependencyIdentity>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AdvancedRawTechniqueV1 {
    pub start_frame: usize,
    pub end_frame: usize,
    pub phoneme_id: i64,
    pub raw_logits: Vec<f32>,
    pub source_local_scores: Vec<f32>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AdvancedRawStyleHeadV1 {
    pub taxonomy: Vec<String>,
    pub raw_logits: Vec<f32>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AdvancedRawGlobalStyleV1 {
    pub start_frame: usize,
    pub end_frame: usize,
    pub heads: BTreeMap<String, AdvancedRawStyleHeadV1>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TechniqueIntervalV1 {
    pub range: TimeRange,
    pub phoneme_id: i64,
    pub raw_logits: Vec<f32>,
    pub source_local_scores: Vec<f32>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct GlobalStyleIntervalV1 {
    pub range: TimeRange,
    pub heads: BTreeMap<String, AdvancedRawStyleHeadV1>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TechniqueEvidenceV1 {
    pub contract: String,
    pub version: u32,
    pub model_id: String,
    pub taxonomy: Vec<String>,
    pub calibration: String,
    pub intervals: Vec<TechniqueIntervalV1>,
    pub style_scope: String,
    pub styles: Vec<GlobalStyleIntervalV1>,
    pub provenance: EvidenceProvenance,
}

impl AdvancedNoteEvidenceV1 {
    pub fn provenance(&self) -> EvidenceProvenance {
        let dependencies = self
            .dependencies
            .iter()
            .map(|dependency| format!("{}:{}", dependency.kind.as_str(), dependency.generation))
            .collect::<Vec<_>>();
        EvidenceProvenance {
            expert_id: self.model_id.clone(),
            task: ExpertTask::NoteBoundary,
            model_hash: Some(self.checkpoint_sha256.clone()),
            runtime_identity: Some(self.runtime_manifest_sha256.clone()),
            calibration_version: None,
            correlation_group: Some(format!("conditioned:{}", dependencies.join("|"))),
            depends_on: dependencies,
        }
    }

    pub fn canonical_notes(
        &self,
        source_start: u64,
        source_duration: u64,
    ) -> EngineResult<Vec<(TimeRange, Option<u8>)>> {
        let source_end = source_start
            .checked_add(source_duration)
            .ok_or_else(|| invalid("advanced note source timeline overflows"))?;
        let frame_tolerance = u64::from(self.frame_step_num)
            .checked_mul(u64::from(CANONICAL_TIMEBASE))
            .and_then(|value| value.checked_add(u64::from(self.frame_step_den) - 1))
            .map(|value| value / u64::from(self.frame_step_den))
            .ok_or_else(|| invalid("advanced note frame tolerance overflows"))?;
        self.notes
            .iter()
            .map(|note| {
                let convert = |frame: usize| -> EngineResult<u64> {
                    let units = (frame as u128)
                        .checked_mul(u128::from(self.frame_step_num))
                        .and_then(|value| value.checked_mul(u128::from(CANONICAL_TIMEBASE)))
                        .map(|value| value / u128::from(self.frame_step_den))
                        .ok_or_else(|| invalid("advanced note frame conversion overflows"))?;
                    let local = u64::try_from(units)
                        .map_err(|_| invalid("advanced note frame conversion overflows"))?;
                    source_start
                        .checked_add(local)
                        .ok_or_else(|| invalid("advanced note timeline overflows"))
                };
                let start = convert(note.start_frame)?;
                let raw_end = convert(note.end_frame)?;
                if start >= source_end || raw_end > source_end.saturating_add(frame_tolerance) {
                    return Err(invalid("advanced note exceeds the source timeline"));
                }
                let range = TimeRange::new(start, raw_end.min(source_end)).map_err(invalid)?;
                Ok((range, note.midi))
            })
            .collect()
    }

    pub fn timed_note_evidence(
        &self,
        source_start: u64,
        source_duration: u64,
    ) -> EngineResult<TimedNoteExpertEvidenceV1> {
        let notes = self
            .canonical_notes(source_start, source_duration)?
            .into_iter()
            .map(|(range, midi)| TimedNoteHypothesisV1 {
                source_id: self.model_id.clone(),
                range,
                midi,
                source_local_boundary_score: None,
                source_local_pitch_score: None,
                calibrated_boundary_confidence: None,
                calibrated_pitch_confidence: None,
            })
            .collect();
        let evidence = TimedNoteExpertEvidenceV1 {
            contract: TIMED_NOTE_EVIDENCE_CONTRACT.to_string(),
            version: TIMED_NOTE_EVIDENCE_VERSION,
            expert_id: self.model_id.clone(),
            model_generation: self.model_generation.clone(),
            backend: self.backend.clone(),
            notes,
            provenance: self.provenance(),
        };
        evidence.validate(source_start, source_duration)?;
        Ok(evidence)
    }

    pub fn technique_artifact(
        &self,
        source_start: u64,
        source_duration: u64,
    ) -> EngineResult<Option<TechniqueEvidenceV1>> {
        let (Some(taxonomy), Some(calibration), Some(techniques), Some(style_scope), Some(styles)) = (
            self.technique_taxonomy.as_ref(),
            self.technique_calibration.as_ref(),
            self.techniques.as_ref(),
            self.style_scope.as_ref(),
            self.styles.as_ref(),
        ) else {
            return Ok(None);
        };
        let intervals = techniques
            .iter()
            .map(|technique| {
                Ok(TechniqueIntervalV1 {
                    range: self.canonical_range(
                        technique.start_frame,
                        technique.end_frame,
                        source_start,
                        source_duration,
                    )?,
                    phoneme_id: technique.phoneme_id,
                    raw_logits: technique.raw_logits.clone(),
                    source_local_scores: technique.source_local_scores.clone(),
                })
            })
            .collect::<EngineResult<Vec<_>>>()?;
        let styles = styles
            .iter()
            .map(|style| {
                Ok(GlobalStyleIntervalV1 {
                    range: self.canonical_range(
                        style.start_frame,
                        style.end_frame,
                        source_start,
                        source_duration,
                    )?,
                    heads: style.heads.clone(),
                })
            })
            .collect::<EngineResult<Vec<_>>>()?;
        Ok(Some(TechniqueEvidenceV1 {
            contract: "uta.analysis-engine.technique-evidence".to_string(),
            version: 1,
            model_id: self.model_id.clone(),
            taxonomy: taxonomy.clone(),
            calibration: calibration.clone(),
            intervals,
            style_scope: style_scope.clone(),
            styles,
            provenance: self.technique_provenance(),
        }))
    }

    pub fn technique_provenance(&self) -> EvidenceProvenance {
        let mut provenance = self.provenance();
        provenance.task = ExpertTask::Technique;
        provenance.calibration_version = Some("source_local_sigmoid_uncalibrated".to_string());
        provenance
    }

    fn canonical_range(
        &self,
        start_frame: usize,
        end_frame: usize,
        source_start: u64,
        source_duration: u64,
    ) -> EngineResult<TimeRange> {
        let source_end = source_start
            .checked_add(source_duration)
            .ok_or_else(|| invalid("advanced note source timeline overflows"))?;
        let convert = |frame: usize| -> EngineResult<u64> {
            let units = (frame as u128)
                .checked_mul(u128::from(self.frame_step_num))
                .and_then(|value| value.checked_mul(u128::from(CANONICAL_TIMEBASE)))
                .map(|value| value / u128::from(self.frame_step_den))
                .ok_or_else(|| invalid("advanced note frame conversion overflows"))?;
            let local = u64::try_from(units)
                .map_err(|_| invalid("advanced note frame conversion overflows"))?;
            source_start
                .checked_add(local)
                .ok_or_else(|| invalid("advanced note timeline overflows"))
        };
        let start = convert(start_frame)?;
        let end = convert(end_frame)?.min(source_end);
        TimeRange::new(start, end).map_err(invalid)
    }
}

pub fn parse_advanced_note_evidence(
    path: &Path,
    expected_model: &str,
) -> EngineResult<AdvancedNoteEvidenceV1> {
    let metadata = std::fs::metadata(path)
        .map_err(|error| invalid(format!("advanced note evidence is unavailable: {error}")))?;
    if !metadata.is_file() || metadata.len() == 0 || metadata.len() > MAX_EVIDENCE_BYTES {
        return Err(invalid("advanced note evidence size is invalid"));
    }
    let evidence: AdvancedNoteEvidenceV1 = serde_json::from_slice(
        &std::fs::read(path)
            .map_err(|error| invalid(format!("could not read advanced note evidence: {error}")))?,
    )
    .map_err(|error| invalid(format!("advanced note evidence JSON is invalid: {error}")))?;
    let (capability, commit, requires_g2p) = match expected_model {
        "rosvot" => ("notes.rosvot", ROSVOT_COMMIT, false),
        "stars" => ("notes.stars", STARS_COMMIT, true),
        _ => {
            return Err(invalid(
                "advanced note parser does not accept baseline substitution",
            ));
        }
    };
    let capability_identity_valid = if expected_model == "stars" {
        let capabilities_valid = match evidence.capabilities.as_slice() {
            [notes] => notes == capability,
            [notes, technique] => notes == capability && technique == "technique.analyze",
            _ => false,
        };
        evidence.schema_version == 2 && evidence.capability.is_none() && capabilities_valid
    } else {
        evidence.schema_version == 1
            && evidence.capability.as_deref() == Some(capability)
            && evidence.capabilities.is_empty()
    };
    if !capability_identity_valid
        || evidence.model_id != expected_model
        || evidence.upstream_commit != commit
        || !matches!(
            evidence.backend.as_str(),
            "openvino_gpu" | "openvino_cpu" | "openvino_gpu_cpu_staged" | "ggml_native"
        )
        || evidence.shared_frontend_profile != FRONTEND_PROFILE
        || evidence.word_boundary_source != "timed_transcript"
        || evidence.frame_step_num != 128
        || evidence.frame_step_den != 24_000
        || evidence.valid_frames == 0
        || evidence.valid_frames > MAX_FRAMES
        || evidence.note_boundary_logits.len() != evidence.valid_frames
        || evidence
            .note_boundary_logits
            .iter()
            .any(|value| !value.is_finite())
        || evidence.notes.is_empty()
    {
        return Err(invalid(
            "advanced note evidence identity or frame contract is invalid",
        ));
    }
    if requires_g2p != (evidence.g2p_profile.as_deref() == Some(G2P_PROFILE)) {
        return Err(invalid("advanced note G2P identity is invalid"));
    }
    validate_technique_contract(&evidence)?;
    validate_dependencies(
        &evidence.dependencies,
        requires_g2p,
        &evidence.shared_frontend_generation,
    )?;
    let mut previous_boundary = None;
    for boundary in &evidence.regulated_note_boundaries {
        if *boundary == 0
            || *boundary >= evidence.valid_frames
            || previous_boundary.is_some_and(|previous| *boundary <= previous)
        {
            return Err(invalid("advanced note boundaries are invalid"));
        }
        previous_boundary = Some(*boundary);
    }
    let mut previous_end = 0;
    for note in &evidence.notes {
        if note.start_frame < previous_end
            || note.end_frame <= note.start_frame
            || note.end_frame > evidence.valid_frames
            || note.pitch_logits.len() != 89
            || note.pitch_logits.iter().any(|value| !value.is_finite())
            || note.midi.is_some_and(|value| !(30..=85).contains(&value))
        {
            return Err(invalid("advanced note event contract is invalid"));
        }
        previous_end = note.end_frame;
    }
    Ok(evidence)
}

fn validate_technique_contract(evidence: &AdvancedNoteEvidenceV1) -> EngineResult<()> {
    let enabled = evidence
        .capabilities
        .iter()
        .any(|capability| capability == "technique.analyze");
    let fields_present = evidence.technique_taxonomy.is_some()
        && evidence.technique_calibration.is_some()
        && evidence.techniques.is_some()
        && evidence.style_scope.is_some()
        && evidence.styles.is_some();
    if !enabled {
        if fields_present
            || evidence.technique_taxonomy.is_some()
            || evidence.technique_calibration.is_some()
            || evidence.techniques.is_some()
            || evidence.style_scope.is_some()
            || evidence.styles.is_some()
        {
            return Err(invalid("disabled STARS technique output must be absent"));
        }
        return Ok(());
    }
    if !fields_present
        || evidence
            .technique_taxonomy
            .as_ref()
            .is_none_or(|taxonomy| !taxonomy.iter().map(String::as_str).eq(TECHNIQUE_TAXONOMY))
        || evidence.technique_calibration.as_deref() != Some("source_local_sigmoid_uncalibrated")
        || evidence.style_scope.as_deref() != Some("segment_global")
    {
        return Err(invalid(
            "STARS technique identity or calibration is invalid",
        ));
    }
    let techniques = evidence.techniques.as_ref().unwrap();
    if techniques.is_empty()
        || techniques
            .windows(2)
            .any(|pair| pair[0].end_frame > pair[1].start_frame)
        || techniques.iter().any(|technique| {
            technique.end_frame <= technique.start_frame
                || technique.end_frame > evidence.valid_frames
                || technique.phoneme_id < 0
                || technique.raw_logits.len() != TECHNIQUE_TAXONOMY.len()
                || technique.source_local_scores.len() != TECHNIQUE_TAXONOMY.len()
                || technique.raw_logits.iter().any(|value| !value.is_finite())
                || technique
                    .source_local_scores
                    .iter()
                    .any(|value| !value.is_finite() || !(0.0..=1.0).contains(value))
                || technique
                    .raw_logits
                    .iter()
                    .zip(&technique.source_local_scores)
                    .any(|(logit, score)| {
                        let projected = 1.0 / (1.0 + (-logit).exp());
                        (projected - score).abs() > 1e-5
                    })
        })
    {
        return Err(invalid("STARS phoneme technique evidence is invalid"));
    }
    let styles = evidence.styles.as_ref().unwrap();
    if styles.is_empty()
        || styles
            .windows(2)
            .any(|pair| pair[0].end_frame > pair[1].start_frame)
        || styles.iter().any(|style| {
            style.end_frame <= style.start_frame
                || style.end_frame > evidence.valid_frames
                || style.heads.len() != STYLE_HEADS.len()
                || STYLE_HEADS.iter().any(|name| {
                    style.heads.get(*name).is_none_or(|head| {
                        let expected = style_taxonomy(name);
                        head.taxonomy
                            .iter()
                            .map(String::as_str)
                            .ne(expected.iter().copied())
                            || head.raw_logits.len() != expected.len()
                            || head.raw_logits.iter().any(|value| !value.is_finite())
                    })
                })
        })
    {
        return Err(invalid("STARS global style evidence is invalid"));
    }
    Ok(())
}

fn style_taxonomy(name: &str) -> &'static [&'static str] {
    match name {
        "technique_group" => &[
            "control",
            "mixed",
            "falsetto",
            "pharyngeal",
            "glissando",
            "vibrato",
            "breathy",
            "weak",
            "strong",
            "bubble",
        ],
        "language" => &[
            "Chinese", "English", "Italian", "French", "Japanese", "Spanish", "German", "Korean",
            "Russian",
        ],
        "gender" => &["female", "male"],
        "emotion" => &["neutral", "happy", "sad", "angry"],
        "method" => &["pop", "bel_canto"],
        "pace" => &["slow", "moderate", "fast"],
        "range" => &["low", "medium", "high"],
        _ => &[],
    }
}

fn validate_dependencies(
    dependencies: &[DependencyIdentity],
    requires_g2p: bool,
    shared_frontend_generation: &str,
) -> EngineResult<()> {
    let kinds = dependencies
        .iter()
        .map(|dependency| {
            let valid = is_identity(&dependency.generation);
            if !valid {
                return Err(invalid("advanced note dependency generation is invalid"));
            }
            Ok(dependency.kind.clone())
        })
        .collect::<EngineResult<BTreeSet<_>>>()?;
    let mut expected = BTreeSet::from([
        DependencyKind::SharedFrontend,
        DependencyKind::AnnotationRmvpe,
        DependencyKind::TimedTranscript,
    ]);
    if requires_g2p {
        expected.insert(DependencyKind::ChineseG2p);
    }
    if kinds != expected || dependencies.len() != expected.len() {
        return Err(invalid(
            "advanced note evidence omits or duplicates correlation dependencies",
        ));
    }
    if dependencies.iter().find_map(|dependency| {
        (dependency.kind == DependencyKind::SharedFrontend)
            .then_some(dependency.generation.as_str())
    }) != Some(shared_frontend_generation)
        || dependencies.iter().find_map(|dependency| {
            (dependency.kind == DependencyKind::AnnotationRmvpe)
                .then_some(dependency.generation.as_str())
        }) != Some(shared_frontend_generation)
    {
        return Err(invalid(
            "annotation RMVPE must come from the declared shared frontend generation",
        ));
    }
    Ok(())
}

fn is_identity(value: &str) -> bool {
    !value.trim().is_empty()
        && value.len() <= 128
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.' | b':'))
}

fn invalid(message: impl Into<String>) -> EngineError {
    EngineError::new(EngineErrorCode::OutputValidationFailed, message)
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicU64, Ordering};

    use super::*;

    static FIXTURE_SEQUENCE: AtomicU64 = AtomicU64::new(0);

    fn fixture(model: &str) -> serde_json::Value {
        let (capability, commit, checkpoint, config, g2p) = if model == "rosvot" {
            (
                "notes.rosvot",
                ROSVOT_COMMIT,
                ROSVOT_CHECKPOINT,
                ROSVOT_CONFIG,
                None,
            )
        } else {
            (
                "notes.stars",
                STARS_COMMIT,
                STARS_CHECKPOINT,
                STARS_CONFIG,
                Some(G2P_PROFILE),
            )
        };
        let frontend_generation = "c".repeat(64);
        let mut dependencies = vec![
            serde_json::json!({"kind":"shared_frontend","generation":frontend_generation}),
            serde_json::json!({"kind":"annotation_rmvpe","generation":frontend_generation}),
            serde_json::json!({"kind":"timed_transcript","generation":"transcript-generation"}),
        ];
        if model == "stars" {
            dependencies
                .push(serde_json::json!({"kind":"chinese_g2p","generation":G2P_ASSET_SHA256}));
        }
        serde_json::json!({
            "schema_version":if model == "stars" { 2 } else { 1 },"model_id":model,
            "capability":if model == "stars" { None::<String> } else { Some(capability.to_string()) },
            "capabilities":if model == "stars" { vec![capability] } else { Vec::<&str>::new() },
            "upstream_commit":commit,"checkpoint_sha256":checkpoint,"config_sha256":config,
            "model_generation":"b".repeat(64),"runtime_manifest_sha256":"a".repeat(64),
            "backend":"openvino_gpu","shared_frontend_profile":FRONTEND_PROFILE,
            "shared_frontend_generation":"c".repeat(64),
            "annotation_rmvpe_sha256":ANNOTATION_RMVPE,
            "word_boundary_source":"timed_transcript","g2p_profile":g2p,
            "frame_step_num":128,"frame_step_den":24000,"valid_frames":16,
            "note_boundary_logits":vec![0.0;16],"regulated_note_boundaries":[8],
            "notes":[
                {"start_frame":0,"end_frame":8,"pitch_logits":vec![0.0;89],"midi":60},
                {"start_frame":8,"end_frame":16,"pitch_logits":vec![0.0;89],"midi":62}
            ],
            "dependencies":dependencies
        })
    }

    fn write(value: &serde_json::Value) -> std::path::PathBuf {
        let path = std::env::temp_dir().join(format!(
            "uta-advanced-notes-{}-{}.json",
            std::process::id(),
            FIXTURE_SEQUENCE.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::write(&path, serde_json::to_vec(value).unwrap()).unwrap();
        path
    }

    #[test]
    fn both_experts_require_correlation_aware_dependencies() {
        for model in ["rosvot", "stars"] {
            let path = write(&fixture(model));
            let evidence = parse_advanced_note_evidence(&path, model).unwrap();
            let provenance = evidence.provenance();
            assert!(provenance.depends_on.len() >= 2);
            assert!(provenance.correlation_group.is_some());
            std::fs::remove_file(path).unwrap();
        }
    }

    #[test]
    fn final_frame_overhang_is_clamped_to_one_hop_and_larger_escape_fails() {
        let path = write(&fixture("rosvot"));
        let evidence = parse_advanced_note_evidence(&path, "rosvot").unwrap();
        let notes = evidence.canonical_notes(1_000_000, 82_000).unwrap();
        assert_eq!(notes.last().unwrap().0.end, 1_082_000);
        assert!(evidence.canonical_notes(1_000_000, 79_000).is_err());
        std::fs::remove_file(path).unwrap();
    }

    #[test]
    fn staged_stars_backend_is_explicitly_accepted() {
        let mut value = fixture("stars");
        value["backend"] = serde_json::json!("openvino_gpu_cpu_staged");
        let path = write(&value);
        assert!(parse_advanced_note_evidence(&path, "stars").is_ok());
        std::fs::remove_file(path).unwrap();
    }

    #[test]
    fn stars_technique_preserves_raw_taxonomy_scope_and_correlation() {
        let mut value = fixture("stars");
        value["capabilities"] = serde_json::json!(["notes.stars", "technique.analyze"]);
        value["technique_taxonomy"] = serde_json::json!(TECHNIQUE_TAXONOMY);
        value["technique_calibration"] = serde_json::json!("source_local_sigmoid_uncalibrated");
        value["techniques"] = serde_json::json!([{
            "start_frame":0,"end_frame":8,"phoneme_id":1,
            "raw_logits":vec![0.0;9],"source_local_scores":vec![0.5;9]
        }]);
        value["style_scope"] = serde_json::json!("segment_global");
        let heads = STYLE_HEADS
            .iter()
            .map(|name| {
                (
                    (*name).to_string(),
                    serde_json::json!({
                        "taxonomy":style_taxonomy(name),
                        "raw_logits":vec![0.0;style_taxonomy(name).len()]
                    }),
                )
            })
            .collect::<serde_json::Map<_, _>>();
        value["styles"] = serde_json::json!([{
            "start_frame":0,"end_frame":16,"heads":heads
        }]);
        let path = write(&value);
        let evidence = parse_advanced_note_evidence(&path, "stars").unwrap();
        let artifact = evidence.technique_artifact(0, 82_000).unwrap().unwrap();
        assert_eq!(artifact.taxonomy, TECHNIQUE_TAXONOMY);
        assert_eq!(artifact.provenance.task, ExpertTask::Technique);
        assert!(artifact.provenance.correlation_group.is_some());
        assert_eq!(artifact.intervals.len(), 1);
        std::fs::remove_file(path).unwrap();
    }

    #[test]
    fn dependency_correlation_and_baseline_substitution_fail_closed_without_hash_rejection() {
        let mut value = fixture("rosvot");
        value["dependencies"] = serde_json::json!([]);
        let path = write(&value);
        assert!(parse_advanced_note_evidence(&path, "rosvot").is_err());
        assert!(parse_advanced_note_evidence(&path, "game").is_err());
        std::fs::remove_file(path).unwrap();

        let mut value = fixture("stars");
        value["dependencies"][1]["generation"] = serde_json::json!("e".repeat(64));
        let path = write(&value);
        assert!(parse_advanced_note_evidence(&path, "stars").is_err());
        std::fs::remove_file(path).unwrap();

        let mut value = fixture("stars");
        value["dependencies"][3]["generation"] = serde_json::json!("f".repeat(64));
        let path = write(&value);
        assert!(parse_advanced_note_evidence(&path, "stars").is_ok());
        std::fs::remove_file(path).unwrap();
    }
}
