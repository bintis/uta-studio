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
    ANALYZE_REQUEST_CONTRACT, ANALYZE_REQUEST_VERSION, AnalysisCliClient, AnalysisPlanWire,
    AnalysisProfileWire, AnalysisSpecWire, AnalyzeRequestWire, AudioRoleWire, AudioSourceKindWire,
    AudioSourceWire, CANONICAL_TIMEBASE, ContextAuthorityWire, DeviceClassWire,
    ExecutionPolicyWire, LyricTokenWire, LyricsModeWire, LyricsWire, MusicalContextWire,
    NativeBackendWire, QuantizationGridWire, RequestedArtifactsWire, RuntimePolicyWire,
    RuntimeResourceStatusWire, SourceTimelineWire, TimeSignatureWire, TrackTargetWire,
};
use crate::config::AppConfig;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ResolvedAnalysisSource {
    pub library_file_hash: String,
    pub path: PathBuf,
    pub sha256: String,
    pub role: AudioRoleWire,
}

pub fn resolve_true_source(file_hash: &str) -> Result<ResolvedAnalysisSource, String> {
    let song = crate::library_db::load_song_by_hash(file_hash)
        .map_err(|error| format!("could not load song {file_hash}: {error}"))?
        .ok_or_else(|| format!("song not found: {file_hash}"))?;
    if song.origin != crate::song::SongOrigin::LocalFile {
        return Err("Engine requires a local TrueSource".to_string());
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
        role: AudioRoleWire::OriginalMix,
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
    requested: &RequestedArtifactsWire,
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
    #[serde(default)]
    pub model_settings: uta_model_settings::ModelSettings,
    #[serde(default)]
    pub turbo_acceleration: bool,
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
    #[serde(default)]
    pub model_settings: uta_model_settings::ModelSettings,
    #[serde(default)]
    pub turbo_acceleration: bool,
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
            turbo_acceleration: config.turbo_acceleration.unwrap_or(false),
            file_hash: file_hash.to_string(),
            request_id: automatic_request_id(),
            lyrics: StudioLyricsContext::default(),
            target_override,
            requested_outputs: None,
            compute_backend: config.compute_backend.clone(),
            model_settings: config.model_settings.clone(),
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
            turbo_acceleration: config.turbo_acceleration.unwrap_or(false),
            file_hash: file_hash.to_string(),
            request_id: automatic_request_id(),
            lyrics: StudioLyricsContext::default(),
            target_override,
            requested_outputs: None,
            compute_backend: config.compute_backend.clone(),
            model_settings: config.model_settings.clone(),
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
    let mut request = compile_analyze_request(
        AnalysisRequestIntent {
            request_id: draft.request_id,
            turbo_acceleration: draft.turbo_acceleration,
            source: source.clone(),
            lyrics,
            target_override: draft.target_override,
            requested_outputs: Some(requested_outputs),
            compute_backend: draft.compute_backend,
            model_settings: draft.model_settings,
            model_backend_overrides: draft.model_backend_overrides,
            default_device_class: draft.default_device_class,
            model_device_overrides: draft.model_device_overrides,
        },
        &effective,
    )?;
    attach_song_execution_context(&mut request, &draft.file_hash, &effective)?;
    preview_analyze_request(request, source, effective)
}

fn attach_song_execution_context(
    request: &mut AnalyzeRequestWire,
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
        request.musical_context = Some(MusicalContextWire {
            bpm,
            key,
            time_signature: quantization_enabled.then_some(TimeSignatureWire { beats: 4, unit: 4 }),
            quantization_grid: quantization_enabled.then_some(QuantizationGridWire::Sixteenth),
            authority: ContextAuthorityWire::Hint,
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

fn studio_tokens_from_timed_lrc(
    timed_lrc: &str,
    source_duration_secs: f64,
) -> Result<Vec<StudioLyricToken>, String> {
    let mut parsed = crate::lrc::parse_lrc(timed_lrc)?;
    parsed.extend_inferred_final_end(source_duration_secs);
    Ok(parsed
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
        .collect())
}

fn resolve_analysis_language(configured: Option<&str>, stored: Option<&str>) -> Option<String> {
    match configured.map(str::trim).filter(|code| !code.is_empty()) {
        Some(code) if code.eq_ignore_ascii_case("auto") => None,
        Some(code) => Some(code.to_string()),
        None => stored
            .map(str::trim)
            .filter(|code| !code.is_empty() && !code.eq_ignore_ascii_case("auto"))
            .map(str::to_string),
    }
}

fn lyrics_context_for_song(
    file_hash: &str,
    requested_outputs: AnalysisOutputSelection,
) -> Result<StudioLyricsContext, String> {
    let song = crate::library_db::load_song_by_hash(file_hash)
        .map_err(|error| format!("could not load lyrics context for {file_hash}: {error}"))?
        .ok_or_else(|| format!("song not found: {file_hash}"))?;
    let language_hint = resolve_analysis_language(
        AppConfig::load().language_override(file_hash),
        song.language.as_deref(),
    );
    if let Some(lyrics) = crate::lyrics::load_lyrics_file(file_hash) {
        if let Some(timed_lrc) = lyrics.timed_lrc {
            let tokens = studio_tokens_from_timed_lrc(&timed_lrc, song.duration_secs)?;
            if !tokens.is_empty() {
                return Ok(StudioLyricsContext {
                    mode: StudioLyricsMode::Canonical,
                    language_hint: language_hint.clone(),
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
                        language_hint: language_hint.clone(),
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
                language_hint: language_hint.clone(),
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
            return Err("Timed lyrics cannot be represented as exact Engine alignment input. Choose an independent target or edit supplied plain lyrics first.".to_string());
        }
        return Ok(StudioLyricsContext {
            mode: StudioLyricsMode::Canonical,
            language_hint: language_hint.clone(),
            tokens,
        });
    }
    if song.transcript_source == Some(crate::song::TranscriptSource::Usdx)
        && (requested_outputs.candidate_chart || requested_outputs.alignment)
    {
        return Err("Timed lyrics cannot be represented as exact Engine alignment input. Choose an independent target or edit supplied plain lyrics first.".to_string());
    }
    if song.transcript_source == Some(crate::song::TranscriptSource::Lyrics)
        && (requested_outputs.candidate_chart || requested_outputs.alignment)
    {
        return Err("Known lyrics were selected, but their canonical text is unavailable. Restore the lyrics before rebuilding the preview.".to_string());
    }
    Ok(StudioLyricsContext {
        mode: StudioLyricsMode::None,
        language_hint,
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
    primary_role: AudioRoleWire,
    primary_path: &Path,
) -> Vec<AudioSourceWire> {
    decision
        .cached_sources
        .iter()
        .filter(|cached| cached.role != primary_role || cached.path != primary_path)
        .enumerate()
        .map(|(index, cached)| AudioSourceWire {
            id: format!("cached_step1_{index}"),
            kind: AudioSourceKindWire::LocalFile,
            path: cached.path.clone(),
            // This remains identity/provenance metadata. Engine input
            // validation uses the actual file and does not hash-verify it.
            sha256: cached.identity.clone(),
            role: cached.role,
            primary: false,
            timeline: SourceTimelineWire {
                timebase: CANONICAL_TIMEBASE,
                source_start: 0,
            },
        })
        .collect()
}

fn ensure_original_mix_source(
    library_source: &ResolvedAnalysisSource,
    sources: &mut Vec<AudioSourceWire>,
) {
    if library_source.role != AudioRoleWire::OriginalMix {
        return;
    }
    if sources
        .iter()
        .any(|source| source.role == AudioRoleWire::OriginalMix)
    {
        return;
    }
    sources.push(AudioSourceWire {
        id: "original_mix".to_string(),
        kind: AudioSourceKindWire::LocalFile,
        path: library_source.path.clone(),
        sha256: library_source.sha256.clone(),
        role: AudioRoleWire::OriginalMix,
        primary: false,
        timeline: SourceTimelineWire {
            timebase: CANONICAL_TIMEBASE,
            source_start: 0,
        },
    });
}

pub fn compile_analyze_request(
    intent: AnalysisRequestIntent,
    effective: &EffectiveAnalysisExperience,
) -> Result<AnalyzeRequestWire, String> {
    uta_model_settings::validate(&intent.model_settings)?;
    if !intent.source.path.is_absolute() {
        return Err("analysis source path must be absolute".to_string());
    }
    if !valid_identifier(&intent.request_id) {
        return Err("analysis request_id contains unsupported characters".to_string());
    }
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
    if outputs.candidate_chart && lyrics.mode == LyricsModeWire::Canonical {
        requested_artifacts.transcript = false;
    }
    // The Step 1 audio chain's "skip if unchanged" cache only ever applies
    // when the source hasn't already been given an explicit, non-default
    // role by the caller -- an explicit role is a deliberate decision this
    // function must not second-guess.
    let mut source_path = intent.source.path.clone();
    let mut source_role = intent.source.role;
    let mut satisfied_capabilities = Vec::new();
    let mut reused_step_one_sources = Vec::new();
    let mut extensions = BTreeMap::new();
    if source_role == AudioRoleWire::OriginalMix
        && let Ok(stored_workflow) =
            crate::workflow::load_song_workflow(&intent.source.library_file_hash)
    {
        let decision = crate::chain_cache::plan_chain_cache(
            &intent.source.library_file_hash,
            &stored_workflow.definition,
            &intent.model_settings,
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
    let mut audio_sources = Vec::with_capacity(reused_step_one_sources.len() + 2);
    audio_sources.push(AudioSourceWire {
        id: "true_source".to_string(),
        kind: AudioSourceKindWire::LocalFile,
        path: source_path,
        sha256: intent.source.sha256.clone(),
        role: source_role,
        primary: true,
        timeline: SourceTimelineWire {
            timebase: CANONICAL_TIMEBASE,
            source_start: 0,
        },
    });
    audio_sources.extend(reused_step_one_sources);
    ensure_original_mix_source(&intent.source, &mut audio_sources);
    Ok(AnalyzeRequestWire {
        contract: ANALYZE_REQUEST_CONTRACT.to_string(),
        version: ANALYZE_REQUEST_VERSION,
        request_id: intent.request_id,
        audio_sources,
        lyrics,
        boundary_constraints: Vec::new(),
        musical_context: None,
        analysis: AnalysisSpecWire {
            profile: match effective.quality_profile.value {
                crate::analysis_experience::AnalysisQualityProfile::Fast => {
                    AnalysisProfileWire::Fast
                }
                crate::analysis_experience::AnalysisQualityProfile::Balanced => {
                    AnalysisProfileWire::Balanced
                }
                crate::analysis_experience::AnalysisQualityProfile::Maximum => {
                    AnalysisProfileWire::Maximum
                }
            },
            track_target: TrackTargetWire::Lead,
            preserve_continuous_pitch: effective.preserve_continuous_pitch.value,
            // Song musical context is attached immediately before Preview so
            // enabled quantization can never travel without explicit BPM/grid.
            enable_quantization: false,
        },
        requested_artifacts,
        execution_policy: ExecutionPolicyWire {
            model_settings: intent.model_settings,
            turbo_acceleration: intent.turbo_acceleration,
            runtime_policy: RuntimePolicyWire::Production,
            // Super mode owns placement. Manual choices remain persisted in
            // AppConfig and become active again when the mode is disabled,
            // but they are deliberately absent from this exact request.
            requested_backend: if intent.turbo_acceleration {
                None
            } else {
                match intent.compute_backend.as_deref() {
                    None | Some("auto") => None,
                    Some(configured) => {
                        Some(NativeBackendWire::parse_setting(configured).ok_or_else(|| {
                            format!("unsupported analysis compute backend: {configured}")
                        })?)
                    }
                }
            },
            model_backend_overrides: if intent.turbo_acceleration {
                BTreeMap::new()
            } else {
                intent
                    .model_backend_overrides
                    .into_iter()
                    .map(|(model_id, backend)| {
                        if !valid_identifier(&model_id) {
                            return Err(format!("invalid model backend override id: {model_id}"));
                        }
                        let backend =
                            NativeBackendWire::parse_setting(&backend).ok_or_else(|| {
                                format!("unsupported backend {backend} for model {model_id}")
                            })?;
                        Ok((model_id, backend))
                    })
                    .collect::<Result<_, String>>()?
            },
            requested_device: if intent.turbo_acceleration {
                None
            } else {
                match intent.default_device_class.as_deref() {
                    None => None,
                    Some("cpu") => Some(DeviceClassWire::Cpu),
                    Some("gpu") => Some(DeviceClassWire::Gpu),
                    Some("integrated_gpu") => Some(DeviceClassWire::IntegratedGpu),
                    Some(other) => {
                        return Err(format!("unsupported analysis device class: {other}"));
                    }
                }
            },
            model_device_overrides: if intent.turbo_acceleration {
                BTreeMap::new()
            } else {
                intent
                    .model_device_overrides
                    .into_iter()
                    .map(|(model_id, device)| {
                        if !valid_identifier(&model_id) {
                            return Err(format!("invalid model device override id: {model_id}"));
                        }
                        let device = match device.as_str() {
                            "cpu" => DeviceClassWire::Cpu,
                            "gpu" => DeviceClassWire::Gpu,
                            "integrated_gpu" => DeviceClassWire::IntegratedGpu,
                            other => {
                                return Err(format!(
                                    "unsupported device class {other} for model {model_id}"
                                ));
                            }
                        };
                        Ok((model_id, device))
                    })
                    .collect::<Result<_, String>>()?
            },
        },
        satisfied_capabilities,
        extensions,
    })
}

fn compile_lyrics(lyrics: StudioLyricsContext) -> Result<LyricsWire, String> {
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
    Ok(LyricsWire {
        mode: match lyrics.mode {
            StudioLyricsMode::None => LyricsModeWire::None,
            StudioLyricsMode::Reference => LyricsModeWire::Reference,
            StudioLyricsMode::Canonical => LyricsModeWire::Canonical,
        },
        language: lyrics.language_hint,
        tokens: lyrics
            .tokens
            .into_iter()
            .map(|token| LyricTokenWire {
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

fn requested_artifacts(outputs: AnalysisOutputSelection) -> RequestedArtifactsWire {
    let mut requested = RequestedArtifactsWire {
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
            .then_some([AudioRoleWire::Instrumental, AudioRoleWire::GuideVocals])
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
    pub engine_plan: AnalysisPlanWire,
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

pub fn preview_analyze_request(
    request: AnalyzeRequestWire,
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

fn plan_resource_blockers(plan: &AnalysisPlanWire) -> Vec<String> {
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

fn resource_ready(status: &RuntimeResourceStatusWire) -> bool {
    status.usable
}

fn runtime_status_reason(status: &RuntimeResourceStatusWire) -> String {
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
    let request: AnalyzeRequestWire = serde_json::from_str(&preview.request_json)
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
    request: &AnalyzeRequestWire,
    plan: &AnalysisPlanWire,
) -> Result<(), String> {
    let request_workflow = request
        .extensions
        .get(crate::workflow::WORKFLOW_EXECUTION_EXTENSION_KEY)
        .map(|value| {
            serde_json::from_value::<crate::workflow::WorkflowExecutionWire>(value.clone())
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
                crate::workflow::WorkflowFusionModeWire::Algorithm => {
                    crate::backend_cli::FusionModeWire::Algorithm
                }
                crate::workflow::WorkflowFusionModeWire::AiJudgment => {
                    crate::backend_cli::FusionModeWire::AiJudgment
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

fn studio_lyrics_from_wire(lyrics: &LyricsWire) -> StudioLyricsContext {
    StudioLyricsContext {
        mode: match lyrics.mode {
            LyricsModeWire::None => StudioLyricsMode::None,
            LyricsModeWire::Reference => StudioLyricsMode::Reference,
            LyricsModeWire::Canonical => StudioLyricsMode::Canonical,
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
#[path = "analysis_engine_adapter/turbo_tests.rs"]
mod turbo_tests;

#[cfg(test)]
mod tests;
