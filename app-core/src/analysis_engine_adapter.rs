//! Studio intent compilation and exact Analysis CLI preview/queue boundary.

use std::collections::{BTreeMap, BTreeSet};
use std::io::Read;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use ts_rs::TS;

use crate::analysis_experience::{AnalysisDefaultTarget, EffectiveAnalysisExperience};
use crate::backend_cli::{
    ANALYZE_REQUEST_CONTRACT, ANALYZE_REQUEST_VERSION, AnalysisCliClient, AnalysisPlanWireV1,
    AnalysisProfileWireV1, AnalysisSpecWireV1, AnalyzeRequestWireV1, AudioRoleWireV1,
    AudioSourceKindWireV1, AudioSourceWireV1, CANONICAL_TIMEBASE, ContextAuthorityWireV1,
    ExecutionPolicyWireV1, LyricTokenWireV1, LyricsModeWireV1, LyricsWireV1, MusicalContextWireV1,
    QuantizationGridWireV1, RequestedArtifactsWireV1, RuntimePolicyWireV1,
    RuntimeResourceStatusWireV1, SourceTimelineWireV1, TimeSignatureWireV1, TrackTargetWireV1,
};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ResolvedAnalysisSource {
    pub library_file_hash: String,
    pub path: PathBuf,
    pub sha256: String,
    pub role: AudioRoleWireV1,
}

pub fn resolve_true_source(file_hash: &str) -> Result<ResolvedAnalysisSource, String> {
    let song = crate::library_db::load_song_by_hash(file_hash)
        .map_err(|error| format!("could not load song {file_hash}: {error}"))?
        .ok_or_else(|| format!("song not found: {file_hash}"))?;
    if song.origin != crate::song::SongOrigin::LocalFile {
        return Err("Engine v1 requires a local TrueSource".to_string());
    }
    resolve_true_source_path(file_hash, &song.path)
}

fn resolve_true_source_path(
    library_file_hash: &str,
    source_path: &Path,
) -> Result<ResolvedAnalysisSource, String> {
    let path = source_path.canonicalize().map_err(|error| {
        format!(
            "could not resolve TrueSource {}: {error}",
            source_path.display()
        )
    })?;
    let metadata = path
        .metadata()
        .map_err(|error| format!("could not inspect TrueSource {}: {error}", path.display()))?;
    if !metadata.is_file() {
        return Err(format!(
            "TrueSource is not a regular file: {}",
            path.display()
        ));
    }
    if metadata.len() == 0 {
        return Err(format!("TrueSource is empty: {}", path.display()));
    }
    let current_library_hash = crate::song::compute_file_hash(&path)
        .map_err(|error| format!("could not verify Studio source identity: {error}"))?;
    if current_library_hash != library_file_hash {
        return Err(format!(
            "source_identity_changed: expected Studio identity {library_file_hash}, got {current_library_hash}"
        ));
    }
    let mut file = std::fs::File::open(&path)
        .map_err(|error| format!("could not read TrueSource {}: {error}", path.display()))?;
    let mut hasher = Sha256::new();
    let mut buffer = [0_u8; 64 * 1024];
    loop {
        let read = file
            .read(&mut buffer)
            .map_err(|error| format!("could not hash TrueSource {}: {error}", path.display()))?;
        if read == 0 {
            break;
        }
        hasher.update(&buffer[..read]);
    }
    Ok(ResolvedAnalysisSource {
        library_file_hash: library_file_hash.to_string(),
        path,
        sha256: format!("{:x}", hasher.finalize()),
        role: AudioRoleWireV1::OriginalMix,
    })
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
#[ts(export)]
pub enum StudioLyricsMode {
    #[default]
    None,
    Reference,
    Canonical,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct StudioLyricToken {
    pub id: String,
    pub text: String,
    #[serde(default)]
    pub reading: Option<String>,
    #[serde(default)]
    pub phonemes: Option<Vec<String>>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct StudioLyricsContext {
    pub mode: StudioLyricsMode,
    #[serde(default)]
    pub language_hint: Option<String>,
    #[serde(default)]
    pub tokens: Vec<StudioLyricToken>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct StudioLyricsContextProjection {
    pub mode: StudioLyricsMode,
    pub text_supplied: bool,
    pub tokens_supplied: bool,
    pub language_hint: Option<String>,
    pub transcript_requested: bool,
    pub alignment_requested: bool,
}

pub fn project_lyrics_context(
    context: &StudioLyricsContext,
    target: AnalysisDefaultTarget,
) -> StudioLyricsContextProjection {
    project_lyrics_context_for_request(context, &requested_artifacts(target))
}

fn project_lyrics_context_for_request(
    context: &StudioLyricsContext,
    requested: &RequestedArtifactsWireV1,
) -> StudioLyricsContextProjection {
    StudioLyricsContextProjection {
        mode: context.mode,
        text_supplied: context
            .tokens
            .iter()
            .any(|token| !token.text.trim().is_empty()),
        tokens_supplied: !context.tokens.is_empty(),
        language_hint: context.language_hint.clone(),
        transcript_requested: requested.transcript,
        alignment_requested: requested.alignment,
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AnalysisRequestIntent {
    pub request_id: String,
    pub source: ResolvedAnalysisSource,
    #[serde(default)]
    pub lyrics: StudioLyricsContext,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub target_override: Option<AnalysisDefaultTarget>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct EngineRunDraft {
    pub file_hash: String,
    pub request_id: String,
    #[serde(default)]
    pub lyrics: StudioLyricsContext,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub target_override: Option<AnalysisDefaultTarget>,
    #[serde(default)]
    pub run_override: crate::analysis_experience::AnalysisExperienceOverride,
}

pub fn preview_engine_run(
    draft: EngineRunDraft,
    global: &crate::analysis_experience::AnalysisExperienceSettings,
) -> Result<EngineRunPreview, String> {
    let song_profile = crate::analysis_profile::get_song_analysis_profile(&draft.file_hash);
    let effective = crate::analysis_experience::resolve_analysis_experience(
        global,
        song_profile
            .as_ref()
            .map(|profile| &profile.analysis_experience),
        Some(&draft.run_override),
    );
    let source = resolve_true_source(&draft.file_hash)?;
    let target = draft
        .target_override
        .unwrap_or(effective.default_target.value);
    let lyrics = if draft.lyrics == StudioLyricsContext::default() {
        lyrics_context_for_song(&draft.file_hash, target)?
    } else {
        draft.lyrics
    };
    let mut request = compile_analyze_request_v1(
        AnalysisRequestIntent {
            request_id: draft.request_id,
            source: source.clone(),
            lyrics,
            target_override: draft.target_override,
        },
        &effective,
    )?;
    attach_song_execution_context(&mut request, &draft.file_hash, &effective)?;
    preview_analyze_request_v1(request, source, effective)
}

fn attach_song_execution_context(
    request: &mut AnalyzeRequestWireV1,
    file_hash: &str,
    effective: &EffectiveAnalysisExperience,
) -> Result<(), String> {
    let song = crate::library_db::load_song_by_hash(file_hash)
        .map_err(|error| format!("could not load song execution context: {error}"))?
        .ok_or_else(|| format!("song not found: {file_hash}"))?;
    let bpm = song
        .bpm
        .filter(|value| value.is_finite() && *value > 0.0 && *value <= 1_000.0);
    let key = song
        .override_key
        .filter(|value| !value.trim().is_empty())
        .or_else(|| song.key.filter(|value| !value.trim().is_empty()));
    let candidate_output =
        request.requested_artifacts.vocal_chart || request.requested_artifacts.singing_analysis;
    let quantization_enabled = effective.enable_quantization.value && candidate_output;
    if quantization_enabled && bpm.is_none() {
        return Err(
            "Rhythm quantization is enabled, but this song has no explicit BPM. Set song BPM or disable quantization before previewing the exact plan."
                .to_string(),
        );
    }
    request.analysis.enable_quantization = quantization_enabled;
    if bpm.is_some() || key.is_some() || quantization_enabled {
        request.musical_context = Some(MusicalContextWireV1 {
            bpm,
            key,
            time_signature: quantization_enabled
                .then_some(TimeSignatureWireV1 { beats: 4, unit: 4 }),
            quantization_grid: quantization_enabled.then_some(QuantizationGridWireV1::Sixteenth),
            authority: ContextAuthorityWireV1::Hint,
        });
    }

    let stored = crate::workflow::load_song_workflow(file_hash)?;
    let snapshot = crate::workflow::compile_workflow(&stored.definition)
        .map_err(|error| format!("could not compile Processing Studio workflow: {error}"))?;
    request.extensions.insert(
        crate::workflow::WORKFLOW_EXECUTION_EXTENSION_KEY.to_string(),
        crate::workflow::workflow_execution_extension(&snapshot)?,
    );
    Ok(())
}

fn lyrics_context_for_song(
    file_hash: &str,
    target: AnalysisDefaultTarget,
) -> Result<StudioLyricsContext, String> {
    let song = crate::library_db::load_song_by_hash(file_hash)
        .map_err(|error| format!("could not load lyrics context for {file_hash}: {error}"))?
        .ok_or_else(|| format!("song not found: {file_hash}"))?;
    if let Some(lyrics) = crate::lyrics::load_lyrics_file(file_hash) {
        let tokens = lyrics
            .lines
            .into_iter()
            .map(|line| line.trim().to_string())
            .filter(|line| !line.is_empty())
            .enumerate()
            .map(|(index, text)| StudioLyricToken {
                id: format!("known-{index}"),
                text,
                reading: None,
                phonemes: None,
            })
            .collect::<Vec<_>>();
        if !tokens.is_empty() {
            return Ok(StudioLyricsContext {
                mode: StudioLyricsMode::Canonical,
                language_hint: song.language,
                tokens,
            });
        }
    }
    if matches!(
        song.transcript_source,
        Some(crate::song::TranscriptSource::Lrc | crate::song::TranscriptSource::Usdx)
    ) && matches!(
        target,
        AnalysisDefaultTarget::FullCandidate | AnalysisDefaultTarget::Alignment
    ) {
        return Err("Timed lyrics cannot be represented as exact Engine v1 alignment input. Choose an independent target or edit supplied plain lyrics first.".to_string());
    }
    if song.transcript_source == Some(crate::song::TranscriptSource::Lyrics)
        && matches!(
            target,
            AnalysisDefaultTarget::FullCandidate | AnalysisDefaultTarget::Alignment
        )
    {
        return Err("Known lyrics were selected, but their canonical text is unavailable. Restore the lyrics before rebuilding the preview.".to_string());
    }
    Ok(StudioLyricsContext {
        mode: StudioLyricsMode::None,
        language_hint: song.language,
        tokens: Vec::new(),
    })
}

pub fn compile_analyze_request_v1(
    intent: AnalysisRequestIntent,
    effective: &EffectiveAnalysisExperience,
) -> Result<AnalyzeRequestWireV1, String> {
    if !intent.source.path.is_absolute() {
        return Err("analysis source path must be absolute".to_string());
    }
    if !valid_identifier(&intent.request_id) {
        return Err("analysis request_id contains unsupported characters".to_string());
    }
    if intent.source.sha256.len() != 64
        || !intent
            .source
            .sha256
            .bytes()
            .all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase())
    {
        return Err("analysis source SHA-256 is invalid".to_string());
    }
    let target = intent
        .target_override
        .unwrap_or(effective.default_target.value);
    let lyrics = compile_lyrics(intent.lyrics)?;
    let mut requested_artifacts = requested_artifacts(target);
    if target == AnalysisDefaultTarget::FullCandidate && lyrics.mode == LyricsModeWireV1::Canonical
    {
        requested_artifacts.transcript = false;
    }
    Ok(AnalyzeRequestWireV1 {
        contract: ANALYZE_REQUEST_CONTRACT.to_string(),
        version: ANALYZE_REQUEST_VERSION,
        request_id: intent.request_id,
        audio_sources: vec![AudioSourceWireV1 {
            id: "true_source".to_string(),
            kind: AudioSourceKindWireV1::LocalFile,
            path: intent.source.path,
            sha256: intent.source.sha256,
            role: intent.source.role,
            primary: true,
            timeline: SourceTimelineWireV1 {
                timebase: CANONICAL_TIMEBASE,
                source_start: 0,
            },
        }],
        lyrics,
        boundary_constraints: Vec::new(),
        musical_context: None,
        analysis: AnalysisSpecWireV1 {
            profile: match effective.quality_profile.value {
                crate::analysis_experience::AnalysisQualityProfile::Fast => {
                    AnalysisProfileWireV1::Fast
                }
                crate::analysis_experience::AnalysisQualityProfile::Balanced => {
                    AnalysisProfileWireV1::Balanced
                }
                crate::analysis_experience::AnalysisQualityProfile::Maximum => {
                    AnalysisProfileWireV1::Maximum
                }
            },
            track_target: TrackTargetWireV1::Lead,
            preserve_continuous_pitch: effective.preserve_continuous_pitch.value,
            // Engine card 19 is not implemented. Keep the v1 wire field for
            // compatibility, but never advertise a no-op quantization request.
            enable_quantization: false,
        },
        requested_artifacts,
        execution_policy: ExecutionPolicyWireV1 {
            runtime_policy: RuntimePolicyWireV1::Experimental,
        },
        extensions: BTreeMap::new(),
    })
}

fn compile_lyrics(lyrics: StudioLyricsContext) -> Result<LyricsWireV1, String> {
    if lyrics.mode == StudioLyricsMode::None && !lyrics.tokens.is_empty() {
        return Err("lyrics mode none cannot contain tokens".to_string());
    }
    if lyrics.mode == StudioLyricsMode::Canonical && lyrics.tokens.is_empty() {
        return Err("canonical lyrics require at least one token".to_string());
    }
    let mut ids = BTreeSet::new();
    for token in &lyrics.tokens {
        if !valid_identifier(&token.id) || token.text.trim().is_empty() || !ids.insert(&token.id) {
            return Err("lyrics contain an invalid or duplicate token".to_string());
        }
    }
    Ok(LyricsWireV1 {
        mode: match lyrics.mode {
            StudioLyricsMode::None => LyricsModeWireV1::None,
            StudioLyricsMode::Reference => LyricsModeWireV1::Reference,
            StudioLyricsMode::Canonical => LyricsModeWireV1::Canonical,
        },
        language: lyrics.language_hint,
        tokens: lyrics
            .tokens
            .into_iter()
            .map(|token| LyricTokenWireV1 {
                id: token.id,
                text: token.text,
                reading: token.reading,
                phonemes: token.phonemes,
            })
            .collect(),
    })
}

fn requested_artifacts(target: AnalysisDefaultTarget) -> RequestedArtifactsWireV1 {
    let mut requested = RequestedArtifactsWireV1 {
        vocal_chart: false,
        pitch_evidence: false,
        singing_analysis: false,
        transcript: false,
        alignment: false,
        stems: Vec::new(),
    };
    match target {
        AnalysisDefaultTarget::FullCandidate => {
            requested.vocal_chart = true;
            requested.pitch_evidence = true;
            requested.singing_analysis = true;
            requested.transcript = true;
            requested.alignment = true;
        }
        AnalysisDefaultTarget::Transcript => requested.transcript = true,
        AnalysisDefaultTarget::Alignment => requested.alignment = true,
        AnalysisDefaultTarget::PitchEvidence => requested.pitch_evidence = true,
        AnalysisDefaultTarget::Instrumental => requested.stems.push(AudioRoleWireV1::Instrumental),
    }
    requested
}

fn valid_identifier(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 128
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.'))
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct EngineRunPreview {
    pub request_id: String,
    pub request_json: String,
    pub request_digest: String,
    pub engine_plan: AnalysisPlanWireV1,
    pub effective_settings: EffectiveAnalysisExperience,
    pub lyrics_context: StudioLyricsContextProjection,
    pub source: ResolvedAnalysisSource,
    pub ready: bool,
    pub blockers: Vec<String>,
    pub created_at_ms: i64,
    pub invalidated: bool,
}

impl EngineRunPreview {
    pub fn invalidate(&mut self) {
        self.invalidated = true;
        self.ready = false;
    }
}

pub fn preview_analyze_request_v1(
    request: AnalyzeRequestWireV1,
    source: ResolvedAnalysisSource,
    effective_settings: EffectiveAnalysisExperience,
) -> Result<EngineRunPreview, String> {
    let request_json = serde_json::to_string(&request).map_err(|error| error.to_string())?;
    let request_value = serde_json::from_str(&request_json).map_err(|error| error.to_string())?;
    let request_digest = digest_json(&request_json);
    let lyrics_context = project_lyrics_context_for_request(
        &studio_lyrics_from_wire(&request.lyrics),
        &request.requested_artifacts,
    );
    let mut client = AnalysisCliClient::connect().map_err(|error| error.to_string())?;
    client
        .validate(&request_value, &request.request_id)
        .map_err(|error| error.to_string())?;
    let requirements = client
        .requirements(&request_value, &request.request_id)
        .map_err(|error| error.to_string())?;
    let capabilities = client.capabilities().map_err(|error| error.to_string())?;
    let plan = client
        .plan(&request_value, &request.request_id)
        .map_err(|error| error.to_string())?;
    if plan.requirements != requirements {
        return Err(
            "Analysis CLI returned inconsistent requirements and plan snapshots".to_string(),
        );
    }
    let capabilities = capabilities
        .into_iter()
        .map(|item| (item.id.0.clone(), item))
        .collect::<BTreeMap<_, _>>();
    let mut blockers = Vec::new();
    for capability in &plan.required_capabilities {
        match capabilities.get(capability.as_str()) {
            Some(item) if item.implementation_exists => {}
            Some(_) => blockers.push(format!("{} is not implemented", capability)),
            None => blockers.push(format!(
                "{} was omitted from Engine capabilities",
                capability
            )),
        }
    }
    for resource in &plan.resolved_resources {
        if !resource.requirement.required {
            continue;
        }
        match resource.status.as_ref() {
            Some(status) if testing_resource_ready(status) => {}
            Some(status) => blockers.push(format!(
                "{} is not runnable for local testing ({})",
                resource.requirement.resource,
                runtime_status_reason(status)
            )),
            None => blockers.push(format!(
                "{} could not be resolved ({})",
                resource.requirement.resource,
                resource
                    .resolution_error
                    .as_deref()
                    .unwrap_or("no status returned")
            )),
        }
    }
    blockers.sort();
    blockers.dedup();
    let created_at_ms = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|duration| duration.as_millis() as i64)
        .unwrap_or(0);
    Ok(EngineRunPreview {
        request_id: request.request_id,
        request_json,
        request_digest,
        engine_plan: plan,
        effective_settings,
        lyrics_context,
        source,
        ready: blockers.is_empty(),
        blockers,
        created_at_ms,
        invalidated: false,
    })
}

fn testing_resource_ready(status: &RuntimeResourceStatusWireV1) -> bool {
    status.usable || (status.runnable && status.executable_ready)
}

fn runtime_status_reason(status: &RuntimeResourceStatusWireV1) -> String {
    if status.reasons.is_empty() {
        format!("state {:?}", status.install_state).to_lowercase()
    } else {
        status
            .reasons
            .iter()
            .map(|reason| format!("{reason:?}").to_lowercase())
            .collect::<Vec<_>>()
            .join(", ")
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct QueuedEngineRun {
    pub file_hash: String,
    pub request_id: String,
    pub request_digest: String,
    pub status: String,
}

/// Persist and enqueue the exact request snapshot confirmed by Plan Preview.
pub fn queue_exact_preview(preview: &EngineRunPreview) -> Result<QueuedEngineRun, String> {
    if preview.invalidated {
        return Err("analysis preview was invalidated; rebuild it before queueing".to_string());
    }
    if !preview.ready {
        return Err("analysis preview is blocked and cannot be queued".to_string());
    }
    if preview.request_json.trim().is_empty()
        || digest_json(&preview.request_json) != preview.request_digest
    {
        return Err("analysis preview request digest does not match its JSON snapshot".to_string());
    }
    let current_source = resolve_true_source(&preview.source.library_file_hash)?;
    let intent = exact_queue_intent(preview, &current_source)?;
    crate::analyzer::enqueue_engine_intent(&intent)?;
    Ok(QueuedEngineRun {
        file_hash: preview.source.library_file_hash.clone(),
        request_id: preview.request_id.clone(),
        request_digest: preview.request_digest.clone(),
        status: "queued".to_string(),
    })
}

fn exact_queue_intent(
    preview: &EngineRunPreview,
    current_source: &ResolvedAnalysisSource,
) -> Result<crate::library_db::EngineQueueIntent, String> {
    if preview.invalidated {
        return Err("analysis preview was invalidated; rebuild it before queueing".to_string());
    }
    if !preview.ready {
        return Err("analysis preview is blocked and cannot be queued".to_string());
    }
    if preview.request_json.trim().is_empty() {
        return Err("analysis preview has no request snapshot".to_string());
    }
    if digest_json(&preview.request_json) != preview.request_digest {
        return Err("analysis preview request digest does not match its JSON snapshot".to_string());
    }
    let request: AnalyzeRequestWireV1 = serde_json::from_str(&preview.request_json)
        .map_err(|error| format!("analysis preview request JSON is malformed: {error}"))?;
    if request.request_id != preview.request_id
        || preview.engine_plan.request_id != preview.request_id
    {
        return Err("analysis preview contains inconsistent request IDs".to_string());
    }
    if request.contract != ANALYZE_REQUEST_CONTRACT || request.version != ANALYZE_REQUEST_VERSION {
        return Err("analysis preview uses an unsupported request contract".to_string());
    }
    let request_source = request
        .audio_sources
        .iter()
        .find(|source| source.primary)
        .ok_or_else(|| "analysis preview request has no primary source".to_string())?;
    if current_source != &preview.source
        || request_source.path != current_source.path
        || request_source.sha256 != current_source.sha256
        || request_source.role != current_source.role
    {
        return Err(
            "source_identity_changed: the previewed TrueSource no longer matches the library"
                .to_string(),
        );
    }
    Ok(crate::library_db::EngineQueueIntent {
        file_hash: preview.source.library_file_hash.clone(),
        request_id: preview.request_id.clone(),
        request_json: preview.request_json.clone(),
        request_digest: preview.request_digest.clone(),
        plan_json: serde_json::to_string(&preview.engine_plan)
            .map_err(|error| error.to_string())?,
        source_path: preview.source.path.clone(),
        source_sha256: preview.source.sha256.clone(),
        queued_at_ms: now_ms(),
    })
}

pub(crate) fn digest_json(json: &str) -> String {
    format!("{:x}", Sha256::digest(json.as_bytes()))
}
fn now_ms() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|duration| duration.as_millis() as i64)
        .unwrap_or(0)
}

fn studio_lyrics_from_wire(lyrics: &LyricsWireV1) -> StudioLyricsContext {
    StudioLyricsContext {
        mode: match lyrics.mode {
            LyricsModeWireV1::None => StudioLyricsMode::None,
            LyricsModeWireV1::Reference => StudioLyricsMode::Reference,
            LyricsModeWireV1::Canonical => StudioLyricsMode::Canonical,
        },
        language_hint: lyrics.language.clone(),
        tokens: lyrics
            .tokens
            .iter()
            .map(|token| StudioLyricToken {
                id: token.id.clone(),
                text: token.text.clone(),
                reading: token.reading.clone(),
                phonemes: token.phonemes.clone(),
            })
            .collect(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::analysis_experience::{
        AnalysisExperienceSettings, AnalysisQualityProfile, resolve_analysis_experience,
    };

    fn effective(target: AnalysisDefaultTarget) -> EffectiveAnalysisExperience {
        resolve_analysis_experience(
            &AnalysisExperienceSettings {
                quality_profile: AnalysisQualityProfile::Balanced,
                default_target: target,
                ..Default::default()
            },
            None,
            None,
        )
    }

    fn source_fixture(label: &str, bytes: &[u8]) -> PathBuf {
        let unique = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let path = std::env::temp_dir().join(format!(
            "uta-studio-true-source-{label}-{}-{unique}.flac",
            std::process::id()
        ));
        std::fs::write(&path, bytes).unwrap();
        path
    }

    #[test]
    fn true_source_resolution_keeps_library_and_engine_identities_distinct() {
        let path = source_fixture("identity", b"lossless source fixture bytes");
        let library_hash = crate::song::compute_file_hash(&path).unwrap();
        let before = std::fs::read(&path).unwrap();
        let source = resolve_true_source_path(&library_hash, &path).unwrap();
        assert_eq!(source.library_file_hash.len(), 32);
        assert_eq!(source.sha256.len(), 64);
        assert_ne!(source.library_file_hash, source.sha256);
        assert_eq!(std::fs::read(&path).unwrap(), before);
        std::fs::remove_file(path).unwrap();
    }

    #[test]
    fn request_compiler_maps_product_targets_without_backend_types() {
        let path = std::env::temp_dir().join("source.flac");
        for target in [
            AnalysisDefaultTarget::Transcript,
            AnalysisDefaultTarget::Alignment,
            AnalysisDefaultTarget::PitchEvidence,
            AnalysisDefaultTarget::Instrumental,
        ] {
            let request = compile_analyze_request_v1(
                AnalysisRequestIntent {
                    request_id: format!("test-{}", target.as_str()),
                    source: ResolvedAnalysisSource {
                        library_file_hash: "library".to_string(),
                        path: path.clone(),
                        sha256: "a".repeat(64),
                        role: AudioRoleWireV1::OriginalMix,
                    },
                    lyrics: if target == AnalysisDefaultTarget::Alignment {
                        StudioLyricsContext {
                            mode: StudioLyricsMode::Canonical,
                            language_hint: Some("ja".to_string()),
                            tokens: vec![StudioLyricToken {
                                id: "token-1".to_string(),
                                text: "歌".to_string(),
                                reading: None,
                                phonemes: None,
                            }],
                        }
                    } else {
                        StudioLyricsContext::default()
                    },
                    target_override: Some(target),
                },
                &effective(target),
            )
            .unwrap();
            assert_eq!(
                request.execution_policy.runtime_policy,
                RuntimePolicyWireV1::Experimental
            );
            assert!(!request.analysis.enable_quantization);
            match target {
                AnalysisDefaultTarget::Transcript => {
                    assert!(request.requested_artifacts.transcript)
                }
                AnalysisDefaultTarget::Alignment => {
                    assert!(request.requested_artifacts.alignment);
                    assert!(!request.requested_artifacts.transcript);
                }
                AnalysisDefaultTarget::PitchEvidence => {
                    assert!(request.requested_artifacts.pitch_evidence)
                }
                AnalysisDefaultTarget::Instrumental => assert_eq!(
                    request.requested_artifacts.stems,
                    [AudioRoleWireV1::Instrumental]
                ),
                AnalysisDefaultTarget::FullCandidate => unreachable!(),
            }
        }
    }

    #[test]
    fn canonical_full_candidate_does_not_request_redundant_asr() {
        let request = compile_analyze_request_v1(
            AnalysisRequestIntent {
                request_id: "known-candidate".to_string(),
                source: ResolvedAnalysisSource {
                    library_file_hash: "library".to_string(),
                    path: std::env::temp_dir().join("source.flac"),
                    sha256: "a".repeat(64),
                    role: AudioRoleWireV1::OriginalMix,
                },
                lyrics: StudioLyricsContext {
                    mode: StudioLyricsMode::Canonical,
                    language_hint: Some("ja".to_string()),
                    tokens: vec![StudioLyricToken {
                        id: "known-0".to_string(),
                        text: "歌".to_string(),
                        reading: None,
                        phonemes: None,
                    }],
                },
                target_override: Some(AnalysisDefaultTarget::FullCandidate),
            },
            &effective(AnalysisDefaultTarget::FullCandidate),
        )
        .unwrap();
        assert!(request.requested_artifacts.vocal_chart);
        assert!(request.requested_artifacts.alignment);
        assert!(!request.requested_artifacts.transcript);
        let projection = project_lyrics_context_for_request(
            &studio_lyrics_from_wire(&request.lyrics),
            &request.requested_artifacts,
        );
        assert!(projection.alignment_requested);
        assert!(!projection.transcript_requested);
    }

    #[test]
    fn lyrics_projection_is_studio_owned_and_truthful() {
        let context = StudioLyricsContext {
            mode: StudioLyricsMode::Reference,
            language_hint: Some("en".to_string()),
            tokens: vec![StudioLyricToken {
                id: "one".to_string(),
                text: "sing".to_string(),
                reading: None,
                phonemes: None,
            }],
        };
        let projection = project_lyrics_context(&context, AnalysisDefaultTarget::Alignment);
        assert!(
            projection.text_supplied
                && projection.tokens_supplied
                && projection.alignment_requested
        );
        assert!(!projection.transcript_requested);
    }

    fn exact_preview_fixture() -> (EngineRunPreview, ResolvedAnalysisSource) {
        let path = source_fixture("queue", b"exact source bytes");
        let library_hash = crate::song::compute_file_hash(&path).unwrap();
        let source = resolve_true_source_path(&library_hash, &path).unwrap();
        let effective = effective(AnalysisDefaultTarget::Transcript);
        let request = compile_analyze_request_v1(
            AnalysisRequestIntent {
                request_id: "exact-preview-1".to_string(),
                source: source.clone(),
                lyrics: StudioLyricsContext::default(),
                target_override: Some(AnalysisDefaultTarget::Transcript),
            },
            &effective,
        )
        .unwrap();
        let request_json = serde_json::to_string(&request).unwrap();
        let plan: AnalysisPlanWireV1 = serde_json::from_value(serde_json::json!({
            "schema":"uta.analysis-engine.plan", "schema_version":1,
            "request_id":"exact-preview-1",
            "source_route":{"primary_source_id":"true_source","input_role":"original_mix","preparation":[]},
            "requested_outputs":["transcript"], "required_capabilities":[], "optional_capabilities":[],
            "requirements":{"schema":"uta.runtime.requirements","schema_version":1,"resources":[]},
            "resolved_resources":[], "execution_nodes":[], "quality_gates":[],
            "fallback_policy":[],
            "artifact_declarations":[{"semantic_type":"transcript","required":true,"media_type":"application/vnd.uta.transcript+json;version=1"}]
        })).unwrap();
        (
            EngineRunPreview {
                request_id: request.request_id,
                request_digest: digest_json(&request_json),
                request_json,
                engine_plan: plan,
                effective_settings: effective,
                lyrics_context: StudioLyricsContextProjection {
                    mode: StudioLyricsMode::None,
                    text_supplied: false,
                    tokens_supplied: false,
                    language_hint: None,
                    transcript_requested: true,
                    alignment_requested: false,
                },
                source: source.clone(),
                ready: true,
                blockers: Vec::new(),
                created_at_ms: now_ms(),
                invalidated: false,
            },
            source,
        )
    }

    #[test]
    fn exact_preview_snapshot_is_persisted_without_recompilation() {
        let (preview, source) = exact_preview_fixture();
        let intent = exact_queue_intent(&preview, &source).unwrap();
        assert_eq!(intent.request_id, preview.request_id);
        assert_eq!(
            intent.request_json.as_bytes(),
            preview.request_json.as_bytes()
        );
        assert_eq!(intent.request_digest, preview.request_digest);
        std::fs::remove_file(source.path).unwrap();
    }

    #[test]
    fn invalidated_and_digest_mismatched_previews_refuse_queue_intent() {
        let (mut preview, source) = exact_preview_fixture();
        preview.invalidated = true;
        assert!(
            exact_queue_intent(&preview, &source)
                .unwrap_err()
                .contains("invalidated")
        );
        preview.invalidated = false;
        preview.request_digest = "0".repeat(64);
        assert!(
            exact_queue_intent(&preview, &source)
                .unwrap_err()
                .contains("digest")
        );
        std::fs::remove_file(source.path).unwrap();
    }
}
