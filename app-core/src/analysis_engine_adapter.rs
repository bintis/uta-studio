//! Studio intent compilation and exact Analysis CLI preview/queue boundary.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use ts_rs::TS;

use crate::analysis_experience::{
    AnalysisDefaultTarget, AnalysisOutputSelection, EffectiveAnalysisExperience,
};
use crate::backend_cli::{
    ANALYZE_REQUEST_CONTRACT, ANALYZE_REQUEST_VERSION, AnalysisCliClient, AnalysisPlanWireV1,
    AnalysisProfileWireV1, AnalysisSpecWireV1, AnalyzeRequestWireV1, AudioRoleWireV1,
    AudioSourceKindWireV1, AudioSourceWireV1, CANONICAL_TIMEBASE, ContextAuthorityWireV1,
    DeviceClassWireV1, ExecutionPolicyWireV1, LyricTokenWireV1, LyricsModeWireV1, LyricsWireV1,
    MusicalContextWireV1, NativeBackendWireV1, QuantizationGridWireV1, RequestedArtifactsWireV1,
    RuntimePolicyWireV1, RuntimeResourceStatusWireV1, SourceTimelineWireV1, TimeSignatureWireV1,
    TrackTargetWireV1,
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
    Ok(ResolvedAnalysisSource {
        library_file_hash: library_file_hash.to_string(),
        path,
        sha256: library_file_hash.to_string(),
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
    /// This token's known real-audio time range, in `CANONICAL_TIMEBASE`
    /// units (microseconds), when one exists -- e.g. a Timed LRC line's own
    /// stamped span. `None` for untimed known lyrics. Lets forced alignment
    /// search near where this token actually is instead of a position
    /// blindly inferred from its index among all tokens.
    #[serde(default)]
    pub start: Option<u64>,
    #[serde(default)]
    pub end: Option<u64>,
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
    project_lyrics_context_for_request(
        context,
        &requested_artifacts(AnalysisOutputSelection::from_target(target)),
    )
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
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub requested_outputs: Option<AnalysisOutputSelection>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub compute_backend: Option<String>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub model_backend_overrides: BTreeMap<String, String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub default_device_class: Option<String>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub model_device_overrides: BTreeMap<String, String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct EngineRunDraft {
    pub file_hash: String,
    pub request_id: String,
    #[serde(default)]
    pub lyrics: StudioLyricsContext,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub target_override: Option<AnalysisDefaultTarget>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub requested_outputs: Option<AnalysisOutputSelection>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub compute_backend: Option<String>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub model_backend_overrides: BTreeMap<String, String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub default_device_class: Option<String>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub model_device_overrides: BTreeMap<String, String>,
    #[serde(default)]
    pub run_override: crate::analysis_experience::AnalysisExperienceOverride,
}

static AUTOMATIC_REQUEST_SEQUENCE: AtomicU64 = AtomicU64::new(1);

fn automatic_request_id() -> String {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|duration| duration.as_nanos())
        .unwrap_or(0);
    let sequence = AUTOMATIC_REQUEST_SEQUENCE.fetch_add(1, Ordering::Relaxed);
    format!("studio-auto-{}-{now}-{sequence}", std::process::id())
}

/// Build, validate, plan and queue one exact Engine request without exposing
/// the retired loose analyzer protocol. Automatic/bulk callers still receive
/// request-specific blockers and never silently downgrade to legacy execution.
pub fn preview_and_queue_engine_run(
    file_hash: &str,
    target_override: Option<AnalysisDefaultTarget>,
) -> Result<QueuedEngineRun, String> {
    let config = crate::config::AppConfig::load();
    let preview = preview_engine_run(
        EngineRunDraft {
            file_hash: file_hash.to_string(),
            request_id: automatic_request_id(),
            lyrics: StudioLyricsContext::default(),
            target_override,
            requested_outputs: None,
            compute_backend: config.compute_backend.clone(),
            model_backend_overrides: config.model_backend_overrides.clone(),
            default_device_class: config.default_device_class.clone(),
            model_device_overrides: config.model_device_overrides.clone(),
            run_override: Default::default(),
        },
        &config.analysis_experience,
    )?;
    if !preview.ready {
        return Err(format!(
            "exact Engine preview is blocked: {}",
            preview.blockers.join("; ")
        ));
    }
    queue_exact_preview(&preview)
}

pub fn preview_and_stage_engine_run(
    file_hash: &str,
    target_override: Option<AnalysisDefaultTarget>,
) -> Result<QueuedEngineRun, String> {
    let config = crate::config::AppConfig::load();
    let preview = preview_engine_run(
        EngineRunDraft {
            file_hash: file_hash.to_string(),
            request_id: automatic_request_id(),
            lyrics: StudioLyricsContext::default(),
            target_override,
            requested_outputs: None,
            compute_backend: config.compute_backend.clone(),
            model_backend_overrides: config.model_backend_overrides.clone(),
            default_device_class: config.default_device_class.clone(),
            model_device_overrides: config.model_device_overrides.clone(),
            run_override: Default::default(),
        },
        &config.analysis_experience,
    )?;
    if !preview.ready {
        return Err(format!(
            "exact Engine preview is blocked: {}",
            preview.blockers.join("; ")
        ));
    }
    stage_exact_preview(&preview)
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
    let requested_outputs = draft
        .requested_outputs
        .unwrap_or_else(|| AnalysisOutputSelection::from_target(target));
    let lyrics = if draft.lyrics == StudioLyricsContext::default() {
        lyrics_context_for_song(&draft.file_hash, requested_outputs)?
    } else {
        draft.lyrics
    };
    let mut request = compile_analyze_request_v1(
        AnalysisRequestIntent {
            request_id: draft.request_id,
            source: source.clone(),
            lyrics,
            target_override: draft.target_override,
            requested_outputs: Some(requested_outputs),
            compute_backend: draft.compute_backend,
            model_backend_overrides: draft.model_backend_overrides,
            default_device_class: draft.default_device_class,
            model_device_overrides: draft.model_device_overrides,
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
    let quantization_enabled =
        effective.enable_quantization.value && request.requested_artifacts.vocal_chart;
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
    requested_outputs: AnalysisOutputSelection,
) -> Result<StudioLyricsContext, String> {
    let song = crate::library_db::load_song_by_hash(file_hash)
        .map_err(|error| format!("could not load lyrics context for {file_hash}: {error}"))?
        .ok_or_else(|| format!("song not found: {file_hash}"))?;
    if let Some(lyrics) = crate::lyrics::load_lyrics_file(file_hash) {
        if let Some(timed_lrc) = lyrics.timed_lrc {
            let tokens = crate::lrc::parse_lrc(&timed_lrc)?
                .segments
                .into_iter()
                .enumerate()
                .map(|(index, segment)| StudioLyricToken {
                    id: format!("lrc-{index}"),
                    text: segment.text,
                    reading: None,
                    phonemes: None,
                    start: Some((segment.start * f64::from(CANONICAL_TIMEBASE)).round() as u64),
                    end: Some((segment.end * f64::from(CANONICAL_TIMEBASE)).round() as u64),
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
        let tokens = lyrics
            .lines
            .into_iter()
            .map(|line| line.trim().to_string())
            .filter(|line| !line.is_empty())
            .collect::<Vec<_>>();
        if !tokens.is_empty() {
            // Applying or importing Timed LRC creates a transcript with real
            // per-line ranges. A later plain-lyrics sidecar can contain the
            // exact same line text (real repro: Asphodelos), and previously
            // masked that timed transcript merely because the sidecar was
            // checked first. Reuse the existing ranges only when every line
            // still matches exactly; a genuine plain-text edit must continue
            // to override stale LRC text and use blind alignment.
            if song.transcript_source == Some(crate::song::TranscriptSource::Lrc) {
                let lrc_segments = crate::lyrics::lrc_transcript_line_segments(
                    &crate::cache::CacheDir::new(),
                    file_hash,
                );
                if let Some(tokens) = matching_lrc_tokens(&tokens, &lrc_segments) {
                    return Ok(StudioLyricsContext {
                        mode: StudioLyricsMode::Canonical,
                        language_hint: song.language,
                        tokens,
                    });
                }
            }
            let tokens = tokens
                .into_iter()
                .enumerate()
                .map(|(index, text)| StudioLyricToken {
                    id: format!("known-{index}"),
                    text,
                    reading: None,
                    phonemes: None,
                    start: None,
                    end: None,
                })
                .collect();
            return Ok(StudioLyricsContext {
                mode: StudioLyricsMode::Canonical,
                language_hint: song.language,
                tokens,
            });
        }
    }
    if song.transcript_source == Some(crate::song::TranscriptSource::Lrc)
        && (requested_outputs.candidate_chart || requested_outputs.alignment)
    {
        // Timed LRC's line text is caller-canonical lyrics, same as plain
        // known lyrics above -- it just came from a different editor mode.
        // Route it through the same skip-ASR, feed-forced-alignment path
        // instead of refusing to align a song whose lyrics are already known.
        let tokens =
            crate::lyrics::lrc_transcript_line_segments(&crate::cache::CacheDir::new(), file_hash)
                .into_iter()
                .enumerate()
                .map(|(index, (start, end, text))| StudioLyricToken {
                    id: format!("lrc-{index}"),
                    text,
                    reading: None,
                    phonemes: None,
                    start: Some((start * f64::from(CANONICAL_TIMEBASE)).round() as u64),
                    end: Some((end * f64::from(CANONICAL_TIMEBASE)).round() as u64),
                })
                .collect::<Vec<_>>();
        if tokens.is_empty() {
            return Err("Timed lyrics cannot be represented as exact Engine v1 alignment input. Choose an independent target or edit supplied plain lyrics first.".to_string());
        }
        return Ok(StudioLyricsContext {
            mode: StudioLyricsMode::Canonical,
            language_hint: song.language,
            tokens,
        });
    }
    if song.transcript_source == Some(crate::song::TranscriptSource::Usdx)
        && (requested_outputs.candidate_chart || requested_outputs.alignment)
    {
        return Err("Timed lyrics cannot be represented as exact Engine v1 alignment input. Choose an independent target or edit supplied plain lyrics first.".to_string());
    }
    if song.transcript_source == Some(crate::song::TranscriptSource::Lyrics)
        && (requested_outputs.candidate_chart || requested_outputs.alignment)
    {
        return Err("Known lyrics were selected, but their canonical text is unavailable. Restore the lyrics before rebuilding the preview.".to_string());
    }
    Ok(StudioLyricsContext {
        mode: StudioLyricsMode::None,
        language_hint: song.language,
        tokens: Vec::new(),
    })
}

fn matching_lrc_tokens(
    plain_lines: &[String],
    lrc_segments: &[(f64, f64, String)],
) -> Option<Vec<StudioLyricToken>> {
    if plain_lines.len() != lrc_segments.len()
        || !plain_lines
            .iter()
            .zip(lrc_segments)
            .all(|(plain, (_, _, timed))| plain == timed)
    {
        return None;
    }
    Some(
        lrc_segments
            .iter()
            .enumerate()
            .map(|(index, (start, end, text))| StudioLyricToken {
                id: format!("lrc-{index}"),
                text: text.clone(),
                reading: None,
                phonemes: None,
                start: Some((start * f64::from(CANONICAL_TIMEBASE)).round() as u64),
                end: Some((end * f64::from(CANONICAL_TIMEBASE)).round() as u64),
            })
            .collect(),
    )
}

fn cached_step_one_audio_sources(
    decision: &crate::chain_cache::ChainCacheDecision,
    primary_role: AudioRoleWireV1,
    primary_path: &Path,
) -> Vec<AudioSourceWireV1> {
    decision
        .cached_sources
        .iter()
        .filter(|cached| cached.role != primary_role || cached.path != primary_path)
        .enumerate()
        .map(|(index, cached)| AudioSourceWireV1 {
            id: format!("cached_step1_{index}"),
            kind: AudioSourceKindWireV1::LocalFile,
            path: cached.path.clone(),
            // This remains identity/provenance metadata. Engine input
            // validation uses the actual file and does not hash-verify it.
            sha256: cached.identity.clone(),
            role: cached.role,
            primary: false,
            timeline: SourceTimelineWireV1 {
                timebase: CANONICAL_TIMEBASE,
                source_start: 0,
            },
        })
        .collect()
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
    let diagnostic_policy = intent.compute_backend.as_deref() == Some("diagnostic_cpu")
        || intent
            .model_backend_overrides
            .values()
            .any(|backend| backend == "diagnostic_cpu");
    let target = intent
        .target_override
        .unwrap_or(effective.default_target.value);
    let outputs = intent
        .requested_outputs
        .unwrap_or_else(|| AnalysisOutputSelection::from_target(target));
    if outputs.is_empty() {
        return Err("select at least one analysis output".to_string());
    }
    let lyrics = compile_lyrics(intent.lyrics)?;
    let mut requested_artifacts = requested_artifacts(outputs);
    if outputs.candidate_chart && !effective.preserve_continuous_pitch.value {
        requested_artifacts.pitch_evidence = false;
    }
    if outputs.candidate_chart && lyrics.mode == LyricsModeWireV1::Canonical {
        requested_artifacts.transcript = false;
    }
    // The Step 1 audio chain's "skip if unchanged" cache only ever applies
    // when the source hasn't already been given an explicit, non-default
    // role by the caller -- an explicit role is a deliberate decision this
    // function must not second-guess.
    let mut source_path = intent.source.path;
    let mut source_role = intent.source.role;
    let mut satisfied_capabilities = Vec::new();
    let mut reused_step_one_sources = Vec::new();
    let mut extensions = BTreeMap::new();
    if source_role == AudioRoleWireV1::OriginalMix
        && let Ok(stored_workflow) =
            crate::workflow::load_song_workflow(&intent.source.library_file_hash)
    {
        let decision = crate::chain_cache::plan_chain_cache(
            &intent.source.library_file_hash,
            &stored_workflow.definition,
        );
        if let Some(cached_path) = decision.source_path.clone() {
            source_path = cached_path;
            source_role = decision.role;
        }
        reused_step_one_sources =
            cached_step_one_audio_sources(&decision, source_role, &source_path);
        satisfied_capabilities = decision.satisfied_capabilities;
        if let Ok(fingerprints) = serde_json::to_value(&decision.fingerprints) {
            extensions.insert(
                crate::chain_cache::CHAIN_FINGERPRINTS_EXTENSION_KEY.to_string(),
                fingerprints,
            );
        }
        for role in crate::chain_cache::stems_to_request_for_caching(&stored_workflow.definition) {
            if !requested_artifacts.stems.contains(&role) {
                requested_artifacts.stems.push(role);
            }
        }
    }
    Ok(AnalyzeRequestWireV1 {
        contract: ANALYZE_REQUEST_CONTRACT.to_string(),
        version: ANALYZE_REQUEST_VERSION,
        request_id: intent.request_id,
        audio_sources: std::iter::once(AudioSourceWireV1 {
            id: "true_source".to_string(),
            kind: AudioSourceKindWireV1::LocalFile,
            path: source_path,
            sha256: intent.source.sha256,
            role: source_role,
            primary: true,
            timeline: SourceTimelineWireV1 {
                timebase: CANONICAL_TIMEBASE,
                source_start: 0,
            },
        })
        .chain(reused_step_one_sources)
        .collect(),
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
            // Song musical context is attached immediately before Preview so
            // enabled quantization can never travel without explicit BPM/grid.
            enable_quantization: false,
        },
        requested_artifacts,
        execution_policy: ExecutionPolicyWireV1 {
            runtime_policy: if diagnostic_policy {
                RuntimePolicyWireV1::Experimental
            } else {
                RuntimePolicyWireV1::Production
            },
            requested_backend: match intent.compute_backend.as_deref() {
                None | Some("auto" | "openvino") => None,
                Some("vulkan") => Some(NativeBackendWireV1::Vulkan),
                Some("diagnostic_cpu") => Some(NativeBackendWireV1::CpuReference),
                Some(other) => {
                    return Err(format!("unsupported analysis compute backend: {other}"));
                }
            },
            model_backend_overrides: intent
                .model_backend_overrides
                .into_iter()
                .map(|(model_id, backend)| {
                    if !valid_identifier(&model_id) {
                        return Err(format!("invalid model backend override id: {model_id}"));
                    }
                    let backend = match backend.as_str() {
                        "openvino" => NativeBackendWireV1::OpenVino,
                        "vulkan" => NativeBackendWireV1::Vulkan,
                        "native_dsp" => NativeBackendWireV1::NativeDsp,
                        "diagnostic_cpu" => NativeBackendWireV1::CpuReference,
                        other => {
                            return Err(format!(
                                "unsupported backend {other} for model {model_id}"
                            ));
                        }
                    };
                    Ok((model_id, backend))
                })
                .collect::<Result<_, String>>()?,
            requested_device: match intent.default_device_class.as_deref() {
                None => None,
                Some("cpu") => Some(DeviceClassWireV1::Cpu),
                Some("gpu") => Some(DeviceClassWireV1::Gpu),
                Some("integrated_gpu") => Some(DeviceClassWireV1::IntegratedGpu),
                Some(other) => {
                    return Err(format!("unsupported analysis device class: {other}"));
                }
            },
            model_device_overrides: intent
                .model_device_overrides
                .into_iter()
                .map(|(model_id, device)| {
                    if !valid_identifier(&model_id) {
                        return Err(format!("invalid model device override id: {model_id}"));
                    }
                    let device = match device.as_str() {
                        "cpu" => DeviceClassWireV1::Cpu,
                        "gpu" => DeviceClassWireV1::Gpu,
                        "integrated_gpu" => DeviceClassWireV1::IntegratedGpu,
                        other => {
                            return Err(format!(
                                "unsupported device class {other} for model {model_id}"
                            ));
                        }
                    };
                    Ok((model_id, device))
                })
                .collect::<Result<_, String>>()?,
        },
        satisfied_capabilities,
        extensions,
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
                start: token.start,
                end: token.end,
            })
            .collect(),
    })
}

fn requested_artifacts(outputs: AnalysisOutputSelection) -> RequestedArtifactsWireV1 {
    let mut requested = RequestedArtifactsWireV1 {
        vocal_chart: outputs.candidate_chart,
        pitch_evidence: outputs.pitch_evidence,
        singing_analysis: outputs.candidate_chart,
        transcript: outputs.transcript,
        alignment: outputs.alignment,
        // `instrumental` requests the authoring audio pair, not just the
        // accompaniment track: `Song::refresh_authoring_state`/`get_audio_paths`
        // read a matching `vocals` compatibility file unconditionally, and
        // GuideVocals is already computed as an internal byproduct of
        // separation regardless (needed for pitch/alignment) -- confirmed
        // against a real song where requesting only Instrumental left the
        // editor's vocals slot pointing at a compatibility path that was
        // never published, so its stem never actually loaded. Publishing it
        // alongside Instrumental costs nothing extra to compute.
        stems: outputs
            .instrumental
            .then_some([AudioRoleWireV1::Instrumental, AudioRoleWireV1::GuideVocals])
            .into_iter()
            .flatten()
            .collect(),
    };
    // Candidate compilation needs all singing evidence. These are Engine
    // dependencies, not hidden run-sheet selections.
    if outputs.candidate_chart {
        requested.pitch_evidence = true;
        requested.transcript = true;
        requested.alignment = true;
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
    let capabilities = client
        .capabilities(request.execution_policy.runtime_policy)
        .map_err(|error| error.to_string())?;
    let plan = client
        .plan(&request_value, &request.request_id)
        .map_err(|error| error.to_string())?;
    if plan.requirements != requirements {
        return Err(
            "Analysis CLI returned inconsistent requirements and plan snapshots".to_string(),
        );
    }
    validate_workflow_plan_identity(&request, &plan)?;
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
    blockers.extend(plan_resource_blockers(&plan));
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

fn plan_resource_blockers(plan: &AnalysisPlanWireV1) -> Vec<String> {
    let mut blockers = Vec::new();
    for resource in &plan.resolved_resources {
        if !resource.requirement.required {
            continue;
        }
        match resource.status.as_ref() {
            Some(status) if resource_ready(status) => {}
            Some(status) => blockers.push(format!(
                "{} is not runnable under the requested policy ({})",
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
    blockers
}

fn resource_ready(status: &RuntimeResourceStatusWireV1) -> bool {
    status.usable
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

fn queued_engine_run(preview: &EngineRunPreview) -> QueuedEngineRun {
    QueuedEngineRun {
        file_hash: preview.source.library_file_hash.clone(),
        request_id: preview.request_id.clone(),
        request_digest: preview.request_digest.clone(),
        status: "queued".to_string(),
    }
}

/// Persist and enqueue the exact request snapshot confirmed by Plan Preview.
pub fn queue_exact_preview(preview: &EngineRunPreview) -> Result<QueuedEngineRun, String> {
    let current_source = resolve_true_source(&preview.source.library_file_hash)?;
    let intent = exact_queue_intent(preview, &current_source)?;
    if crate::library_db::analysis_queue_status(&preview.source.library_file_hash)
        .map_err(|error| error.to_string())?
        .as_deref()
        == Some("staged")
    {
        // Queue-page editing keeps the user's position, replaces the frozen
        // exact request, then starts that edited item.
        crate::analyzer::replace_staged_engine_intent(&intent)?;
        crate::analyzer::resume_engine_intent(&intent.file_hash);
    } else {
        crate::analyzer::enqueue_engine_intent(&intent)?;
    }
    Ok(queued_engine_run(preview))
}

/// Persist an exact request in the visible processing queue without starting
/// the analysis worker. The user starts it explicitly from the queue.
pub fn stage_exact_preview(preview: &EngineRunPreview) -> Result<QueuedEngineRun, String> {
    let current_source = resolve_true_source(&preview.source.library_file_hash)?;
    let intent = exact_queue_intent(preview, &current_source)?;
    crate::analyzer::stage_engine_intent(&intent)?;
    Ok(queued_engine_run(preview))
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
    validate_workflow_plan_identity(&request, &preview.engine_plan)?;
    request
        .audio_sources
        .iter()
        .find(|source| source.primary)
        .ok_or_else(|| "analysis preview request has no primary source".to_string())?;
    if current_source.library_file_hash != preview.source.library_file_hash
        || current_source.path != preview.source.path
        || current_source.role != preview.source.role
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

pub(crate) fn validate_workflow_plan_identity(
    request: &AnalyzeRequestWireV1,
    plan: &AnalysisPlanWireV1,
) -> Result<(), String> {
    let request_workflow = request
        .extensions
        .get(crate::workflow::WORKFLOW_EXECUTION_EXTENSION_KEY)
        .map(|value| {
            serde_json::from_value::<crate::workflow::WorkflowExecutionWireV1>(value.clone())
                .map_err(|error| format!("workflow request snapshot is malformed: {error}"))
        })
        .transpose()?;
    match (request_workflow.as_ref(), plan.workflow_execution.as_ref()) {
        (None, None) => Ok(()),
        (Some(request_workflow), Some(planned)) => {
            let identity = &planned.identity;
            if identity.contract != request_workflow.contract
                || identity.version != request_workflow.version
                || identity.workflow_schema_version != request_workflow.workflow_schema_version
                || identity.workflow_id != request_workflow.workflow_id
                || identity.workflow_revision != request_workflow.workflow_revision
            {
                return Err(
                    "Analysis CLI workflow identity does not match the exact request snapshot"
                        .to_string(),
                );
            }
            let requested_fusion_mode = match request_workflow.fusion_mode {
                crate::workflow::WorkflowFusionModeWireV1::Algorithm => {
                    crate::backend_cli::FusionModeWireV1::Algorithm
                }
                crate::workflow::WorkflowFusionModeWireV1::AiJudgment => {
                    crate::backend_cli::FusionModeWireV1::AiJudgment
                }
            };
            if planned.fusion_mode != requested_fusion_mode {
                return Err(
                    "Analysis CLI workflow decision mode does not match the exact request snapshot"
                        .to_string(),
                );
            }
            if planned.nodes.len() != request_workflow.nodes.len()
                || planned.terminal_outputs != request_workflow.terminal_outputs
            {
                return Err(
                    "Analysis CLI workflow plan does not represent the exact compiled snapshot"
                        .to_string(),
                );
            }
            let mut planned_bindings = planned
                .nodes
                .iter()
                .flat_map(|node| node.input_bindings.iter().cloned())
                .collect::<Vec<_>>();
            planned_bindings.sort_by(|left, right| {
                (
                    &left.from_node,
                    &left.from_port,
                    &left.to_node,
                    &left.to_port,
                )
                    .cmp(&(
                        &right.from_node,
                        &right.from_port,
                        &right.to_node,
                        &right.to_port,
                    ))
            });
            let mut request_bindings = request_workflow.bindings.clone();
            request_bindings.sort_by(|left, right| {
                (
                    &left.from_node,
                    &left.from_port,
                    &left.to_node,
                    &left.to_port,
                )
                    .cmp(&(
                        &right.from_node,
                        &right.from_port,
                        &right.to_node,
                        &right.to_port,
                    ))
            });
            if planned_bindings != request_bindings {
                return Err(
                    "Analysis CLI workflow plan changed compiled artifact bindings".to_string(),
                );
            }
            for requested in &request_workflow.nodes {
                let node = planned
                    .nodes
                    .iter()
                    .find(|node| node.instance_id == requested.instance_id)
                    .ok_or_else(|| {
                        format!(
                            "Analysis CLI workflow plan omitted instance {}",
                            requested.instance_id
                        )
                    })?;
                if node.execution_policy != requested.execution_policy
                    || node.priority != requested.priority
                {
                    return Err(format!(
                        "Analysis CLI workflow plan changed instance {}",
                        requested.instance_id
                    ));
                }
            }
            Ok(())
        }
        (Some(_), None) => {
            Err("Analysis CLI omitted the requested compiled workflow execution plan".to_string())
        }
        (None, Some(_)) => Err(
            "Analysis CLI returned a compiled workflow for a request that omitted one".to_string(),
        ),
    }
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
                start: token.start,
                end: token.end,
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

    #[test]
    fn identical_plain_and_lrc_lines_recover_the_existing_time_anchors() {
        let lines = vec!["一行目".to_string(), "二行目".to_string()];
        let segments = vec![
            (40.67, 47.16, "一行目".to_string()),
            (47.16, 52.76, "二行目".to_string()),
        ];
        let tokens = matching_lrc_tokens(&lines, &segments).unwrap();
        assert_eq!(tokens.len(), 2);
        assert_eq!(tokens[0].id, "lrc-0");
        assert_eq!(tokens[0].start, Some(40_670_000));
        assert_eq!(tokens[0].end, Some(47_160_000));
        assert_eq!(tokens[1].text, "二行目");
    }

    #[test]
    fn edited_plain_lines_do_not_reuse_stale_lrc_time_anchors() {
        let lines = vec!["一行目".to_string(), "編集した二行目".to_string()];
        let segments = vec![
            (40.67, 47.16, "一行目".to_string()),
            (47.16, 52.76, "二行目".to_string()),
        ];
        assert!(matching_lrc_tokens(&lines, &segments).is_none());
    }

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
    fn automatic_queue_request_ids_are_unique_and_studio_owned() {
        let first = automatic_request_id();
        let second = automatic_request_id();
        assert_ne!(first, second);
        assert!(first.starts_with("studio-auto-"));
        assert!(second.starts_with("studio-auto-"));
    }

    #[test]
    fn deep_step_one_cache_hit_keeps_every_earlier_semantic_source() {
        let decision = crate::chain_cache::ChainCacheDecision {
            role: AudioRoleWireV1::CleanLeadVocal,
            source_path: Some(PathBuf::from("/cache/clean.flac")),
            cached_sources: vec![
                crate::chain_cache::CachedChainSource {
                    role: AudioRoleWireV1::GuideVocals,
                    path: PathBuf::from("/cache/guide.flac"),
                    identity: "guide".to_string(),
                },
                crate::chain_cache::CachedChainSource {
                    role: AudioRoleWireV1::Instrumental,
                    path: PathBuf::from("/cache/instrumental.flac"),
                    identity: "instrumental".to_string(),
                },
                crate::chain_cache::CachedChainSource {
                    role: AudioRoleWireV1::LeadVocal,
                    path: PathBuf::from("/cache/lead.flac"),
                    identity: "lead".to_string(),
                },
                crate::chain_cache::CachedChainSource {
                    role: AudioRoleWireV1::CleanLeadVocal,
                    path: PathBuf::from("/cache/clean.flac"),
                    identity: "clean".to_string(),
                },
            ],
            ..Default::default()
        };

        let sources = cached_step_one_audio_sources(
            &decision,
            AudioRoleWireV1::CleanLeadVocal,
            Path::new("/cache/clean.flac"),
        );

        assert_eq!(
            sources.iter().map(|source| source.role).collect::<Vec<_>>(),
            vec![
                AudioRoleWireV1::GuideVocals,
                AudioRoleWireV1::Instrumental,
                AudioRoleWireV1::LeadVocal,
            ]
        );
        assert!(sources.iter().all(|source| !source.primary));
    }

    #[test]
    fn true_source_resolution_reuses_library_identity_without_hash_verification() {
        let path = source_fixture("identity", b"lossless source fixture bytes");
        let library_hash = crate::song::compute_file_hash(&path).unwrap();
        let before = std::fs::read(&path).unwrap();
        let source = resolve_true_source_path(&library_hash, &path).unwrap();
        assert_eq!(source.library_file_hash.len(), 32);
        assert_eq!(source.library_file_hash, source.sha256);
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
                                start: None,
                                end: None,
                            }],
                        }
                    } else {
                        StudioLyricsContext::default()
                    },
                    target_override: Some(target),
                    requested_outputs: None,
                    compute_backend: None,
                    model_backend_overrides: BTreeMap::new(),
                    default_device_class: None,
                    model_device_overrides: BTreeMap::new(),
                },
                &effective(target),
            )
            .unwrap();
            assert_eq!(
                request.execution_policy.runtime_policy,
                RuntimePolicyWireV1::Production
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
                    [AudioRoleWireV1::Instrumental, AudioRoleWireV1::GuideVocals]
                ),
                AnalysisDefaultTarget::FullCandidate => unreachable!(),
            }
        }
    }

    #[test]
    fn request_compiler_preserves_independent_multi_output_run_sheet() {
        let outputs = AnalysisOutputSelection {
            candidate_chart: false,
            pitch_evidence: false,
            transcript: true,
            alignment: false,
            instrumental: true,
        };
        let request = compile_analyze_request_v1(
            AnalysisRequestIntent {
                request_id: "multi-output".to_string(),
                source: ResolvedAnalysisSource {
                    library_file_hash: "library".to_string(),
                    path: std::env::temp_dir().join("source.flac"),
                    sha256: "a".repeat(64),
                    role: AudioRoleWireV1::OriginalMix,
                },
                lyrics: StudioLyricsContext::default(),
                target_override: None,
                requested_outputs: Some(outputs),
                compute_backend: None,
                model_backend_overrides: BTreeMap::new(),
                default_device_class: None,
                model_device_overrides: BTreeMap::new(),
            },
            &effective(AnalysisDefaultTarget::FullCandidate),
        )
        .unwrap();
        assert!(request.requested_artifacts.transcript);
        assert_eq!(
            request.requested_artifacts.stems,
            [AudioRoleWireV1::Instrumental, AudioRoleWireV1::GuideVocals]
        );
        assert!(!request.requested_artifacts.vocal_chart);
        assert!(!request.requested_artifacts.pitch_evidence);
        assert!(!request.requested_artifacts.alignment);
        assert!(!request.requested_artifacts.singing_analysis);
    }

    #[test]
    fn request_compiler_rejects_an_empty_run_sheet() {
        let error = compile_analyze_request_v1(
            AnalysisRequestIntent {
                request_id: "empty-output-sheet".to_string(),
                source: ResolvedAnalysisSource {
                    library_file_hash: "library".to_string(),
                    path: std::env::temp_dir().join("source.flac"),
                    sha256: "a".repeat(64),
                    role: AudioRoleWireV1::OriginalMix,
                },
                lyrics: StudioLyricsContext::default(),
                target_override: None,
                requested_outputs: Some(AnalysisOutputSelection {
                    candidate_chart: false,
                    pitch_evidence: false,
                    transcript: false,
                    alignment: false,
                    instrumental: false,
                }),
                compute_backend: None,
                model_backend_overrides: BTreeMap::new(),
                default_device_class: None,
                model_device_overrides: BTreeMap::new(),
            },
            &effective(AnalysisDefaultTarget::FullCandidate),
        )
        .unwrap_err();
        assert_eq!(error, "select at least one analysis output");
    }

    #[test]
    fn request_compiler_preserves_explicit_cpu_and_vulkan_selection() {
        for (configured, expected) in [
            ("diagnostic_cpu", NativeBackendWireV1::CpuReference),
            ("vulkan", NativeBackendWireV1::Vulkan),
        ] {
            let request = compile_analyze_request_v1(
                AnalysisRequestIntent {
                    request_id: format!("backend-{configured}"),
                    source: ResolvedAnalysisSource {
                        library_file_hash: "library".to_string(),
                        path: std::env::temp_dir().join("source.flac"),
                        sha256: "a".repeat(64),
                        role: AudioRoleWireV1::OriginalMix,
                    },
                    lyrics: StudioLyricsContext::default(),
                    target_override: Some(AnalysisDefaultTarget::PitchEvidence),
                    requested_outputs: None,
                    compute_backend: Some(configured.to_string()),
                    model_backend_overrides: BTreeMap::new(),
                    default_device_class: None,
                    model_device_overrides: BTreeMap::new(),
                },
                &effective(AnalysisDefaultTarget::PitchEvidence),
            )
            .unwrap();
            assert_eq!(request.execution_policy.requested_backend, Some(expected));
            assert_eq!(
                request.execution_policy.runtime_policy,
                if configured == "diagnostic_cpu" {
                    RuntimePolicyWireV1::Experimental
                } else {
                    RuntimePolicyWireV1::Production
                }
            );
        }
    }

    #[test]
    fn request_compiler_preserves_per_model_backend_choices() {
        let request = compile_analyze_request_v1(
            AnalysisRequestIntent {
                request_id: "model-backends".to_string(),
                source: ResolvedAnalysisSource {
                    library_file_hash: "library".to_string(),
                    path: std::env::temp_dir().join("source.flac"),
                    sha256: "a".repeat(64),
                    role: AudioRoleWireV1::OriginalMix,
                },
                lyrics: StudioLyricsContext::default(),
                target_override: Some(AnalysisDefaultTarget::Instrumental),
                requested_outputs: None,
                compute_backend: None,
                model_backend_overrides: BTreeMap::from([
                    (
                        "bs_roformer_leap_xe90_vocals".to_string(),
                        "vulkan".to_string(),
                    ),
                    ("rmvpe".to_string(), "diagnostic_cpu".to_string()),
                ]),
                default_device_class: None,
                model_device_overrides: BTreeMap::new(),
            },
            &effective(AnalysisDefaultTarget::Instrumental),
        )
        .unwrap();
        assert_eq!(request.execution_policy.requested_backend, None);
        assert_eq!(
            request.execution_policy.runtime_policy,
            RuntimePolicyWireV1::Experimental
        );
        assert_eq!(
            request
                .execution_policy
                .model_backend_overrides
                .get("bs_roformer_leap_xe90_vocals"),
            Some(&NativeBackendWireV1::Vulkan)
        );
        assert_eq!(
            request
                .execution_policy
                .model_backend_overrides
                .get("rmvpe"),
            Some(&NativeBackendWireV1::CpuReference)
        );
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
                        start: None,
                        end: None,
                    }],
                },
                target_override: Some(AnalysisDefaultTarget::FullCandidate),
                requested_outputs: None,
                compute_backend: None,
                model_backend_overrides: BTreeMap::new(),
                default_device_class: None,
                model_device_overrides: BTreeMap::new(),
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
    fn disabling_continuous_pitch_omits_only_the_published_pitch_artifact() {
        let mut settings = effective(AnalysisDefaultTarget::FullCandidate);
        settings.preserve_continuous_pitch.value = false;
        let request = compile_analyze_request_v1(
            AnalysisRequestIntent {
                request_id: "candidate-without-pitch-artifact".to_string(),
                source: ResolvedAnalysisSource {
                    library_file_hash: "library".to_string(),
                    path: std::env::temp_dir().join("source.flac"),
                    sha256: "a".repeat(64),
                    role: AudioRoleWireV1::OriginalMix,
                },
                lyrics: StudioLyricsContext::default(),
                target_override: Some(AnalysisDefaultTarget::FullCandidate),
                requested_outputs: None,
                compute_backend: None,
                model_backend_overrides: BTreeMap::new(),
                default_device_class: None,
                model_device_overrides: BTreeMap::new(),
            },
            &settings,
        )
        .unwrap();
        assert!(request.requested_artifacts.vocal_chart);
        assert!(request.requested_artifacts.singing_analysis);
        assert!(!request.requested_artifacts.pitch_evidence);
        assert!(!request.analysis.preserve_continuous_pitch);
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
                start: None,
                end: None,
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
                requested_outputs: None,
                compute_backend: None,
                model_backend_overrides: BTreeMap::new(),
                default_device_class: None,
                model_device_overrides: BTreeMap::new(),
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
    fn exact_plan_rejects_a_fusion_mode_mismatch() {
        let (preview, source) = exact_preview_fixture();
        let mut request: AnalyzeRequestWireV1 =
            serde_json::from_str(&preview.request_json).unwrap();
        let request_workflow = crate::workflow::WorkflowExecutionWireV1 {
            contract: "uta.workflow-execution".to_string(),
            version: 1,
            workflow_schema_version: crate::workflow::WORKFLOW_SCHEMA_VERSION,
            workflow_id: "workflow:test".to_string(),
            workflow_revision: 7,
            quality_mode: "balanced".to_string(),
            definition_digest: "digest".to_string(),
            nodes: Vec::new(),
            bindings: Vec::new(),
            terminal_outputs: Vec::new(),
            fusion_policy: None,
            fusion_mode: crate::workflow::WorkflowFusionModeWireV1::AiJudgment,
        };
        request.extensions.insert(
            crate::workflow::WORKFLOW_EXECUTION_EXTENSION_KEY.to_string(),
            serde_json::to_value(&request_workflow).unwrap(),
        );
        let mut plan = preview.engine_plan;
        plan.workflow_execution = Some(crate::backend_cli::WorkflowExecutionPlanWireV1 {
            identity: crate::backend_cli::WorkflowPlanIdentityWireV1 {
                contract: request_workflow.contract,
                version: request_workflow.version,
                workflow_schema_version: request_workflow.workflow_schema_version,
                workflow_id: request_workflow.workflow_id,
                workflow_revision: request_workflow.workflow_revision,
                definition_digest: request_workflow.definition_digest,
            },
            nodes: Vec::new(),
            terminal_outputs: Vec::new(),
            fusion_policy: None,
            fusion_mode: crate::backend_cli::FusionModeWireV1::Algorithm,
        });
        assert_eq!(
            validate_workflow_plan_identity(&request, &plan).unwrap_err(),
            "Analysis CLI workflow decision mode does not match the exact request snapshot"
        );
        std::fs::remove_file(source.path).unwrap();
    }

    #[test]
    fn exact_preview_blocks_missing_and_unusable_fusion_adapters() {
        let (mut preview, source) = exact_preview_fixture();
        preview.engine_plan.resolved_resources = vec![
            serde_json::from_value(serde_json::json!({
                "requirement": {
                    "resource": "tool:fusion_agent_adapter",
                    "required": true,
                    "reason": "fusion.candidate_graph / ai_judgment"
                },
                "status": null,
                "resolution_error": "resource_missing: adapter is not configured"
            }))
            .unwrap(),
        ];
        assert_eq!(
            plan_resource_blockers(&preview.engine_plan),
            [
                "tool:fusion_agent_adapter could not be resolved (resource_missing: adapter is not configured)"
            ]
        );

        preview.engine_plan.resolved_resources[0] = serde_json::from_value(serde_json::json!({
            "requirement": {
                "resource": "tool:fusion_agent_adapter",
                "required": true,
                "reason": "fusion.candidate_graph / ai_judgment"
            },
            "status": {
                "resource": "tool:fusion_agent_adapter",
                "install_state": "absent",
                "origin": "missing",
                "integrity_verified": false,
                "runnable": false,
                "validation_state": "production_pinned",
                "dependencies_ready": true,
                "executable_ready": false,
                "usable": false,
                "reasons": ["executable_missing"]
            }
        }))
        .unwrap();
        assert_eq!(
            plan_resource_blockers(&preview.engine_plan),
            [
                "tool:fusion_agent_adapter is not runnable under the requested policy (executablemissing)"
            ]
        );
        std::fs::remove_file(source.path).unwrap();
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
    fn exact_preview_accepts_a_cached_chain_input_for_an_unchanged_true_source() {
        let (mut preview, source) = exact_preview_fixture();
        let cached_path = source_fixture("cached-guide", b"cached guide vocal bytes");
        let mut request: AnalyzeRequestWireV1 =
            serde_json::from_str(&preview.request_json).unwrap();
        request.audio_sources[0].path = cached_path.clone();
        request.audio_sources[0].role = AudioRoleWireV1::GuideVocals;
        preview.engine_plan.source_route.input_role = AudioRoleWireV1::GuideVocals;
        preview.request_json = serde_json::to_string(&request).unwrap();
        preview.request_digest = digest_json(&preview.request_json);

        let intent = exact_queue_intent(&preview, &source).unwrap();
        assert_eq!(intent.source_path, source.path);
        assert_eq!(
            serde_json::from_str::<AnalyzeRequestWireV1>(&intent.request_json)
                .unwrap()
                .audio_sources[0]
                .path,
            cached_path
        );

        std::fs::remove_file(cached_path).unwrap();
        std::fs::remove_file(source.path).unwrap();
    }

    #[test]
    fn exact_preview_still_rejects_a_changed_library_true_source() {
        let (preview, source) = exact_preview_fixture();
        let replacement_path = source_fixture("replacement", b"replacement source bytes");
        let mut replacement = source.clone();
        replacement.path = replacement_path.clone();

        assert!(
            exact_queue_intent(&preview, &replacement)
                .unwrap_err()
                .contains("source_identity_changed")
        );

        std::fs::remove_file(replacement_path).unwrap();
        std::fs::remove_file(source.path).unwrap();
    }

    #[test]
    fn invalidated_preview_is_rejected_but_digest_metadata_is_not_verified() {
        let (mut preview, source) = exact_preview_fixture();
        preview.invalidated = true;
        assert!(
            exact_queue_intent(&preview, &source)
                .unwrap_err()
                .contains("invalidated")
        );
        preview.invalidated = false;
        preview.request_digest = "opaque-digest-metadata".to_string();
        assert!(exact_queue_intent(&preview, &source).is_ok());
        std::fs::remove_file(source.path).unwrap();
    }
}
