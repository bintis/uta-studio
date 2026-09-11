use super::{AnalysisEngine, EnginePlan, optional_execution_supported};
use crate::artifact::{TranscriptArtifact, TranscriptAuthority, TranscriptToken};
use crate::contract::{
    AnalyzeRequest, EngineError, EngineErrorCode, EngineResult, LyricToken,
    ResolvedResourceProvenance,
};
use crate::fusion::CanonicalLyrics;

pub(super) fn cancelled(request: &AnalyzeRequest) -> EngineError {
    EngineError::new(EngineErrorCode::Cancelled, "analysis request was cancelled")
        .for_request(&request.request_id)
}

/// Joins caller lyric tokens with an explicit newline between every token.
/// Unlike the compact text used for reference comparison, the canonical
/// artifact preserves caller-authored line boundaries for later local stages.
fn caller_transcript_text(tokens: &[LyricToken]) -> String {
    tokens
        .iter()
        .map(|token| token.text.as_str())
        .collect::<Vec<_>>()
        .join("\n")
}

pub(super) fn caller_transcript(request: &AnalyzeRequest) -> EngineResult<TranscriptArtifact> {
    let text = caller_transcript_text(&request.lyrics.tokens);
    if text.is_empty() {
        return Err(EngineError::new(
            EngineErrorCode::MissingRequiredInput,
            "canonical lyrics contain no text",
        ));
    }
    let artifact = TranscriptArtifact {
        contract: "uta.analysis-engine.transcript".to_string(),
        version: 1,
        authority: TranscriptAuthority::CallerCanonical,
        language: request.lyrics.language.clone(),
        text,
        audio_segments: Vec::new(),
        tokens: request
            .lyrics
            .tokens
            .iter()
            .map(|token| TranscriptToken {
                id: token.id.clone(),
                text: token.text.clone(),
                confidence: None,
            })
            .collect(),
        confidence: None,
        source_experts: vec!["caller.canonical_lyrics".to_string()],
        alternatives: Vec::new(),
        model_sha256: None,
        runtime_manifest_sha256: None,
        backend: "caller".to_string(),
    };
    artifact.validate()?;
    Ok(artifact)
}

pub(super) fn qwen_alignment_words(
    transcript: &CanonicalLyrics,
    segments: &[crate::artifact::TranscriptAudioSegment],
) -> EngineResult<Vec<serde_json::Value>> {
    let units = if !transcript.tokens.is_empty() {
        transcript
            .tokens
            .iter()
            .enumerate()
            .map(|(index, token)| {
                (
                    token
                        .id
                        .clone()
                        .unwrap_or_else(|| format!("aligned-word-{index}")),
                    token.text.clone(),
                    token.range,
                )
            })
            .collect::<Vec<_>>()
    } else {
        let language = transcript
            .language
            .as_deref()
            .unwrap_or("und")
            .split(['-', '_'])
            .next()
            .unwrap_or("und")
            .to_ascii_lowercase();
        let character_units = matches!(language.as_str(), "zh" | "yue" | "ja" | "ko");
        alignment_text_units(&transcript.text, character_units)
            .into_iter()
            .enumerate()
            .map(|(index, text)| (format!("aligned-word-{index}"), text, None))
            .collect()
    };
    if units.is_empty() || units.iter().any(|(_, text, _)| text.trim().is_empty()) {
        return Err(EngineError::new(
            EngineErrorCode::MissingRequiredInput,
            "Qwen forced alignment requires non-empty canonical transcript units",
        )
        .with_capability("speech.align"));
    }
    let mut character_offset = 0;
    Ok(units
        .into_iter()
        .map(|(id, text, caller_range)| {
            let characters = text
                .chars()
                .filter(|character| !character.is_whitespace())
                .collect::<Vec<_>>();
            let lexical_offset = characters
                .iter()
                .position(|character| character.is_alphanumeric())
                .unwrap_or(0);
            let position = character_offset + lexical_offset;
            let anchor = segments
                .iter()
                .find(|segment| (segment.text_start..segment.text_end).contains(&position));
            character_offset += characters.len();
            let range = caller_range.or_else(|| {
                anchor.map(|segment| crate::fusion::TimeRange {
                    start: segment.start,
                    end: segment.start + segment.duration,
                })
            });
            serde_json::json!({"id": id, "text": text, "audio_range": range})
        })
        .collect())
}

fn alignment_text_units(text: &str, character_units: bool) -> Vec<String> {
    let raw = if character_units {
        text.chars()
            .filter(|character| !character.is_whitespace())
            .map(|character| character.to_string())
            .collect::<Vec<_>>()
    } else {
        text.split_whitespace().map(str::to_string).collect()
    };
    let mut units: Vec<String> = Vec::new();
    let mut prefix = String::new();
    for unit in raw {
        // Punctuation is text, not a separate sung onset. Attach it to the
        // adjacent lexical unit so it cannot manufacture a zero-time word.
        let modifier = character_units
            && unit.chars().all(|character| {
                matches!(
                    character,
                    'ゃ' | 'ゅ'
                        | 'ょ'
                        | 'ぁ'
                        | 'ぃ'
                        | 'ぅ'
                        | 'ぇ'
                        | 'ぉ'
                        | 'ャ'
                        | 'ュ'
                        | 'ョ'
                        | 'ァ'
                        | 'ィ'
                        | 'ゥ'
                        | 'ェ'
                        | 'ォ'
                )
            });
        if (modifier || !unit.chars().any(char::is_alphanumeric)) && !units.is_empty() {
            units.last_mut().expect("nonempty units").push_str(&unit);
        } else if !unit.chars().any(char::is_alphanumeric) {
            prefix.push_str(&unit);
        } else {
            prefix.push_str(&unit);
            units.push(std::mem::take(&mut prefix));
        }
    }
    if !prefix.is_empty() {
        units.push(prefix);
    }
    units
}

pub(super) fn firered_language_applicable(
    request_language: Option<&str>,
    detected_language: Option<&str>,
) -> bool {
    let mut observed = request_language
        .into_iter()
        .chain(detected_language)
        .map(|language| {
            language
                .split(['-', '_'])
                .next()
                .unwrap_or(language)
                .to_ascii_lowercase()
        })
        .filter(|language| !language.is_empty() && language != "und")
        .peekable();
    if observed.peek().is_none() {
        return true;
    }
    observed.any(|language| matches!(language.as_str(), "zh" | "yue" | "en"))
}

/// STARS ships a Chinese G2P lexicon only. Japanese (and other) lyrics must not
/// fail the whole analysis for a missing Chinese reading.
pub(super) fn stars_g2p_language_applicable(request_language: Option<&str>) -> bool {
    let language = request_language
        .unwrap_or_default()
        .split(['-', '_'])
        .next()
        .unwrap_or_default()
        .to_ascii_lowercase();
    language.is_empty() || language == "und" || matches!(language.as_str(), "zh" | "yue")
}

pub(super) fn request_lyrics_text(request: &AnalyzeRequest) -> String {
    let separator = match request.lyrics.language.as_deref() {
        Some(language)
            if language.starts_with("zh")
                || language.starts_with("ja")
                || language.starts_with("ko") =>
        {
            ""
        }
        _ => " ",
    };
    request
        .lyrics
        .tokens
        .iter()
        .map(|token| token.text.as_str())
        .collect::<Vec<_>>()
        .join(separator)
}

pub(super) fn fingerprint_request(request: &AnalyzeRequest) -> EngineResult<serde_json::Value> {
    let mut value = serde_json::to_value(request).map_err(|error| {
        EngineError::new(
            EngineErrorCode::InternalError,
            format!("could not serialize request fingerprint identity: {error}"),
        )
    })?;
    if let Some(sources) = value
        .get_mut("audio_sources")
        .and_then(serde_json::Value::as_array_mut)
    {
        for source in sources {
            if let Some(source) = source.as_object_mut() {
                source.remove("path");
            }
        }
    }
    Ok(value)
}

#[derive(Debug)]
pub(super) struct RoformerRoute {
    backend: &'static str,
    device_class: Option<&'static str>,
}

/// Maps a resolved backend and the caller's device-class preference to the
/// worker backend and device class. The resolved backend decides which native
/// runtime the shared worker loads; the device class only narrows the
/// physical device inside that runtime and never authorizes a fallback.
fn worker_route(
    model_id: &str,
    backend: uta_runtime_manager::NativeBackend,
    requested_device: Option<uta_runtime_manager::NativeDeviceClass>,
) -> EngineResult<RoformerRoute> {
    use uta_runtime_manager::{NativeBackend, NativeDeviceClass};
    let (backend, device_class) = match (backend, requested_device) {
        (NativeBackend::Ggml, Some(NativeDeviceClass::Gpu)) => ("ggml_vulkan", Some("gpu")),
        (NativeBackend::Ggml, Some(NativeDeviceClass::IntegratedGpu)) => {
            ("ggml_vulkan", Some("integrated_gpu"))
        }
        (NativeBackend::Ggml, Some(NativeDeviceClass::Cpu)) => ("ggml_cpu", Some("cpu")),
        (NativeBackend::Ggml, None) => ("ggml_vulkan", None),
        (NativeBackend::LibtorchXpu, None | Some(NativeDeviceClass::Gpu)) => {
            ("libtorch_xpu", Some("gpu"))
        }
        (NativeBackend::LibtorchXpu, Some(other)) => {
            return Err(EngineError::new(
                EngineErrorCode::RuntimeResolutionFailed,
                format!(
                    "model {model_id} selected the LibTorch XPU runtime, which executes only on the discrete Intel GPU; device class {other:?} has no LibTorch route and CPU is not a fallback"
                ),
            ));
        }
    };
    Ok(RoformerRoute {
        backend,
        device_class,
    })
}

pub(super) fn resolve_roformer_route(
    model: &uta_runtime_manager::ResolvedModel,
    request: &AnalyzeRequest,
) -> EngineResult<RoformerRoute> {
    if model.runtime_id != model.backend.runtime_id() {
        return Err(EngineError::new(
            EngineErrorCode::RuntimeResolutionFailed,
            format!(
                "model {} resolved backend {:?} but runtime {}",
                model.model_id, model.backend, model.runtime_id
            ),
        ));
    }
    let requested_device = if request.execution_policy.turbo_acceleration {
        Some(match model.backend {
            uta_runtime_manager::NativeBackend::Ggml => {
                uta_runtime_manager::NativeDeviceClass::IntegratedGpu
            }
            uta_runtime_manager::NativeBackend::LibtorchXpu => {
                uta_runtime_manager::NativeDeviceClass::Gpu
            }
        })
    } else {
        request
            .execution_policy
            .requested_device_for(&model.model_id)
    };
    worker_route(&model.model_id, model.backend, requested_device)
}

pub(super) fn roformer_dispatch_config(
    route: &RoformerRoute,
    model_path: &std::path::Path,
    semantic_output: &str,
) -> EngineResult<(&'static str, serde_json::Value)> {
    let mut config = serde_json::json!({
        "model_path": model_path,
        "backend": route.backend,
        "semantic_output": semantic_output,
    });
    if let Some(device_class) = route.device_class {
        config["device_class"] = serde_json::Value::from(device_class);
    }
    Ok(("uta-ggml-worker", config))
}

pub(super) fn model_dispatch(
    model: &uta_runtime_manager::ResolvedModel,
    request: &AnalyzeRequest,
    semantic_output: &str,
) -> EngineResult<(&'static str, serde_json::Value)> {
    let route = resolve_roformer_route(model, request)?;
    let (component, mut config) =
        roformer_dispatch_config(&route, &model.model_path, semantic_output)?;
    config["turbo_acceleration"] =
        serde_json::Value::Bool(request.execution_policy.turbo_acceleration);
    config["model_artifacts"] = serde_json::to_value(&model.model_artifacts).map_err(|error| {
        EngineError::new(
            EngineErrorCode::InternalError,
            format!("could not serialize resolved model artifacts: {error}"),
        )
    })?;
    Ok((component, config))
}

pub(super) fn pitch_dispatch(
    model: &uta_runtime_manager::ResolvedModel,
    request: &AnalyzeRequest,
) -> EngineResult<(&'static str, serde_json::Value)> {
    model_dispatch(model, request, "pitch")
}

pub(super) fn execution_device(backend: uta_runtime_manager::NativeBackend) -> &'static str {
    match backend {
        uta_runtime_manager::NativeBackend::Ggml => "ggml",
        uta_runtime_manager::NativeBackend::LibtorchXpu => "xpu",
    }
}

pub(super) fn backend_name(backend: uta_runtime_manager::NativeBackend) -> &'static str {
    match backend {
        uta_runtime_manager::NativeBackend::Ggml => "ggml",
        uta_runtime_manager::NativeBackend::LibtorchXpu => "libtorch_xpu",
    }
}

pub(super) fn resource_provenance(
    resource: &uta_runtime_manager::ResolvedModel,
) -> ResolvedResourceProvenance {
    ResolvedResourceProvenance {
        resource: format!("model:{}", resource.model_id),
        generation: resource.generation.clone(),
        content_digest: resource.model_content_digest.clone(),
        runtime: resource.runtime_id.clone(),
        runtime_generation: resource.runtime_generation.clone(),
        runtime_recipe_digest: resource.runtime_recipe_digest.clone(),
        backend: backend_name(resource.backend).to_string(),
        device: execution_device(resource.backend).to_string(),
    }
}

impl AnalysisEngine {
    pub(super) fn resolve_execution_resources(
        &self,
        request: &AnalyzeRequest,
        plan: &EnginePlan,
    ) -> EngineResult<(Vec<uta_runtime_manager::ResolvedModel>, Vec<String>)> {
        let automatic_schedule = request.execution_policy.turbo_acceleration.then(|| {
            crate::device_scheduler::schedule_models(
                plan.requirements.resources.iter().filter_map(|requirement| {
                    requirement.resource.strip_prefix("model:")
                }),
            )
        });
        let mut resolved = Vec::new();
        let mut degraded = Vec::new();
        for requirement in plan.requirements.resources.iter().filter(|requirement| {
            requirement.required || optional_execution_supported(&requirement.reason)
        }) {
            let resource: uta_runtime_manager::ResourceRef =
                requirement.resource.parse().map_err(|error| {
                    EngineError::new(
                        EngineErrorCode::RuntimeResolutionFailed,
                        format!("invalid planned resource: {error}"),
                    )
                })?;
            match resource.kind {
                uta_runtime_manager::ResourceKind::Model => {
                    let resolution = if request.execution_policy.turbo_acceleration {
                        let placement = automatic_schedule
                            .as_ref()
                            .and_then(|schedule| {
                                schedule.iter().find(|item| item.model_id == resource.id)
                            })
                            .cloned()
                            .unwrap_or_else(|| {
                                crate::device_scheduler::placement_for(&resource.id)
                            });
                        let mut errors = Vec::new();
                        let mut selected = None;
                        for backend in placement.candidate_backends() {
                            match self.runtime_manager.resolve_model_with_backend(
                                &resource.id,
                                request.execution_policy.runtime_policy,
                                Some(backend),
                            ) {
                                Ok(model) => {
                                    selected = Some(model);
                                    break;
                                }
                                Err(error) => errors.push(error.to_string()),
                            }
                        }
                        selected.ok_or_else(|| {
                            EngineError::new(
                                EngineErrorCode::RuntimeResolutionFailed,
                                format!(
                                    "automatic GPU scheduling found no usable native route for {}: {}",
                                    resource.id,
                                    errors.join("; ")
                                ),
                            )
                            .with_resource(resource.clone())
                        })
                    } else {
                        self.runtime_manager
                            .resolve_model_with_backend(
                                &resource.id,
                                request.execution_policy.runtime_policy,
                                request.execution_policy.requested_backend_for(&resource.id),
                            )
                            .map_err(EngineError::from)
                    };
                    match resolution {
                        Ok(model) => resolved.push(model),
                        Err(error) if !requirement.required => degraded.push(format!(
                            "optional capability {} skipped: {}",
                            requirement.reason, error
                        )),
                        Err(error) => return Err(error),
                    }
                }
                uta_runtime_manager::ResourceKind::Tool => {
                    let status = self
                        .runtime_manager
                        .status(&resource, request.execution_policy.runtime_policy)
                        .map_err(EngineError::from)?;
                    if !status.usable {
                        return Err(EngineError::new(
                            EngineErrorCode::WorkerUnavailable,
                            format!("required execution tool is unavailable: {resource}"),
                        )
                        .with_resource(&resource));
                    }
                }
                uta_runtime_manager::ResourceKind::Runtime
                | uta_runtime_manager::ResourceKind::Bundle => {
                    return Err(EngineError::new(
                        EngineErrorCode::RuntimeResolutionFailed,
                        format!("unsupported direct execution requirement: {resource}"),
                    ));
                }
            }
        }
        Ok((resolved, degraded))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn token(id: &str, text: &str) -> LyricToken {
        LyricToken {
            id: id.to_string(),
            text: text.to_string(),
            reading: None,
            phonemes: None,
            start: None,
            end: None,
        }
    }

    #[test]
    fn caller_transcript_text_preserves_line_boundaries_for_cjk_lyrics() {
        let tokens = vec![
            token("line-1", "风吹沙蝶恋花千古佳话"),
            token("line-2", "似水中月情迷着镜中花"),
        ];
        let text = caller_transcript_text(&tokens);
        // Canonical text retains the caller's line structure.
        assert_eq!(text, "风吹沙蝶恋花千古佳话\n似水中月情迷着镜中花");
        assert_eq!(text.lines().count(), 2);
    }

    #[test]
    fn caller_transcript_text_preserves_every_line_including_repeats() {
        let tokens = vec![
            token("a", "风吹沙蝶恋花千古佳话"),
            token("b", "似水中月情迷着镜中花"),
            token("c", "风吹沙蝶恋花千古佳话"),
        ];
        let text = caller_transcript_text(&tokens);
        let lines: Vec<&str> = text.lines().collect();
        assert_eq!(lines.len(), 3);
        assert_eq!(lines[0], lines[2]);
    }

    #[test]
    fn caller_transcript_text_handles_a_single_token_without_trailing_newline() {
        let text = caller_transcript_text(&[token("only", "唱")]);
        assert_eq!(text, "唱");
    }

    #[test]
    fn caller_transcript_text_is_empty_for_no_tokens() {
        assert_eq!(caller_transcript_text(&[]), "");
    }

    #[test]
    fn worker_routes_follow_the_resolved_backend_without_fallback() {
        use uta_runtime_manager::{NativeBackend, NativeDeviceClass};
        let ggml = worker_route("rmvpe", NativeBackend::Ggml, None).unwrap();
        assert_eq!((ggml.backend, ggml.device_class), ("ggml_vulkan", None));
        let cpu = worker_route("rmvpe", NativeBackend::Ggml, Some(NativeDeviceClass::Cpu)).unwrap();
        assert_eq!((cpu.backend, cpu.device_class), ("ggml_cpu", Some("cpu")));
        let libtorch = worker_route("rmvpe", NativeBackend::LibtorchXpu, None).unwrap();
        assert_eq!(
            (libtorch.backend, libtorch.device_class),
            ("libtorch_xpu", Some("gpu"))
        );
        let explicit = worker_route(
            "rmvpe",
            NativeBackend::LibtorchXpu,
            Some(NativeDeviceClass::Gpu),
        )
        .unwrap();
        assert_eq!(explicit.backend, "libtorch_xpu");
        for device in [NativeDeviceClass::Cpu, NativeDeviceClass::IntegratedGpu] {
            let error =
                worker_route("rmvpe", NativeBackend::LibtorchXpu, Some(device)).unwrap_err();
            assert_eq!(error.code, EngineErrorCode::RuntimeResolutionFailed);
        }
        let (component, config) = roformer_dispatch_config(
            &libtorch,
            std::path::Path::new("/models/rmvpe.gguf"),
            "pitch",
        )
        .unwrap();
        assert_eq!(component, "uta-ggml-worker");
        assert_eq!(config["backend"], "libtorch_xpu");
        assert_eq!(config["device_class"], "gpu");
        assert_eq!(backend_name(NativeBackend::LibtorchXpu), "libtorch_xpu");
        assert_eq!(execution_device(NativeBackend::LibtorchXpu), "xpu");
    }

    #[test]
    fn firered_is_limited_to_its_supported_language_families() {
        assert!(firered_language_applicable(Some("zh-CN"), Some("zh")));
        assert!(firered_language_applicable(Some("en"), None));
        assert!(firered_language_applicable(None, Some("und")));
        assert!(!firered_language_applicable(Some("ja"), Some("ja-JP")));
        assert!(firered_language_applicable(Some("ja"), Some("yue")));
    }

    #[test]
    fn stars_g2p_is_limited_to_chinese_families() {
        assert!(stars_g2p_language_applicable(Some("zh-CN")));
        assert!(stars_g2p_language_applicable(Some("yue")));
        assert!(stars_g2p_language_applicable(None));
        assert!(stars_g2p_language_applicable(Some("und")));
        assert!(!stars_g2p_language_applicable(Some("ja")));
        assert!(!stars_g2p_language_applicable(Some("ja-JP")));
        assert!(!stars_g2p_language_applicable(Some("en")));
    }

    #[test]
    fn qwen_alignment_units_preserve_caller_ids_and_segment_generated_cjk() {
        let caller = CanonicalLyrics {
            text: "sing now".to_string(),
            language: Some("en".to_string()),
            authority: crate::fusion::LyricsAuthority::CallerCanonical,
            tokens: vec![crate::fusion::TranscriptTokenEvidence {
                id: Some("line-1".to_string()),
                text: "sing now".to_string(),
                range: None,
                confidence: None,
            }],
            confidence: None,
            source_experts: vec!["caller".to_string()],
            alternatives: Vec::new(),
        };
        assert_eq!(
            qwen_alignment_words(&caller, &[]).unwrap()[0]["id"],
            "line-1"
        );

        let mut generated = caller;
        generated.text = "风吹沙".to_string();
        generated.language = Some("zh".to_string());
        generated.tokens.clear();
        let units = qwen_alignment_words(&generated, &[]).unwrap();
        assert_eq!(units.len(), 3);
        assert_eq!(units[2]["text"], "沙");
    }

    #[test]
    fn alignment_audio_scopes_keep_intro_gaps_and_punctuation_out_of_note_boundaries() {
        let transcript = CanonicalLyrics {
            text: "「きゃ！」君。".to_string(),
            language: Some("ja".to_string()),
            authority: crate::fusion::LyricsAuthority::Generated,
            tokens: Vec::new(),
            confidence: None,
            source_experts: vec!["qwen3_asr_1_7b".to_string()],
            alternatives: Vec::new(),
        };
        let segments = [
            crate::artifact::TranscriptAudioSegment {
                start: 20_000_000,
                duration: 8_000_000,
                text_start: 0,
                text_end: 5,
            },
            crate::artifact::TranscriptAudioSegment {
                start: 60_000_000,
                duration: 8_000_000,
                text_start: 5,
                text_end: 7,
            },
        ];
        let units = qwen_alignment_words(&transcript, &segments).unwrap();
        assert_eq!(units.len(), 2);
        assert_eq!(units[0]["text"], "「きゃ！」");
        assert_eq!(units[1]["text"], "君。");
        assert_eq!(units[0]["audio_range"]["start"], 20_000_000);
        assert_eq!(units[1]["audio_range"]["start"], 60_000_000);
        assert!(qwen_alignment_words(&transcript, &[]).unwrap()[0]["audio_range"].is_null());
    }
}
