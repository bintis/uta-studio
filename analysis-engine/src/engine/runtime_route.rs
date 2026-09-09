use super::{AnalysisEngine, EnginePlan, optional_execution_supported};
use crate::artifact::{TranscriptArtifactV1, TranscriptAuthorityV1, TranscriptTokenV1};
use crate::contract::{
    AnalyzeRequestV1, EngineError, EngineErrorCode, EngineResult, LyricTokenV1,
    ResolvedResourceProvenanceV1,
};
use crate::fusion::CanonicalLyrics;

pub(super) fn cancelled(request: &AnalyzeRequestV1) -> EngineError {
    EngineError::new(EngineErrorCode::Cancelled, "analysis request was cancelled")
        .for_request(&request.request_id)
}

/// Joins caller lyric tokens with an explicit newline between every token.
/// Unlike the compact text used for reference comparison, the canonical
/// artifact preserves caller-authored line boundaries for later local stages.
fn caller_transcript_text(tokens: &[LyricTokenV1]) -> String {
    tokens
        .iter()
        .map(|token| token.text.as_str())
        .collect::<Vec<_>>()
        .join("\n")
}

pub(super) fn caller_transcript(request: &AnalyzeRequestV1) -> EngineResult<TranscriptArtifactV1> {
    let text = caller_transcript_text(&request.lyrics.tokens);
    if text.is_empty() {
        return Err(EngineError::new(
            EngineErrorCode::MissingRequiredInput,
            "canonical lyrics contain no text",
        ));
    }
    let artifact = TranscriptArtifactV1 {
        contract: "uta.analysis-engine.transcript".to_string(),
        version: 1,
        authority: TranscriptAuthorityV1::CallerCanonical,
        language: request.lyrics.language.clone(),
        text,
        tokens: request
            .lyrics
            .tokens
            .iter()
            .map(|token| TranscriptTokenV1 {
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
        let texts = if character_units {
            transcript
                .text
                .chars()
                .filter(|character| !character.is_whitespace())
                .map(|character| character.to_string())
                .collect::<Vec<_>>()
        } else {
            transcript
                .text
                .split_whitespace()
                .map(str::to_string)
                .collect::<Vec<_>>()
        };
        texts
            .into_iter()
            .enumerate()
            .map(|(index, text)| (format!("aligned-word-{index}"), text))
            .collect()
    };
    if units.is_empty() || units.iter().any(|(_, text)| text.trim().is_empty()) {
        return Err(EngineError::new(
            EngineErrorCode::MissingRequiredInput,
            "Qwen forced alignment requires non-empty canonical transcript units",
        )
        .with_capability("speech.align"));
    }
    Ok(units
        .into_iter()
        .map(|(id, text)| serde_json::json!({"id": id, "text": text}))
        .collect())
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

pub(super) fn request_lyrics_text(request: &AnalyzeRequestV1) -> String {
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

pub(super) fn fingerprint_request(request: &AnalyzeRequestV1) -> EngineResult<serde_json::Value> {
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

pub(super) struct RoformerRoute {
    backend: &'static str,
    device_class: Option<&'static str>,
}

pub(super) fn resolve_roformer_route(
    model: &uta_runtime_manager::ResolvedModel,
    request: &AnalyzeRequestV1,
) -> EngineResult<RoformerRoute> {
    if model.backend != uta_runtime_manager::NativeBackend::Ggml
        || model.runtime_id != "ggml_vulkan"
    {
        return Err(EngineError::new(
            EngineErrorCode::RuntimeResolutionFailed,
            format!(
                "model {} did not resolve to the GGML runtime",
                model.model_id
            ),
        ));
    }
    let (backend, device_class) = match request
        .execution_policy
        .requested_device_for(&model.model_id)
    {
        Some(uta_runtime_manager::NativeDeviceClass::Gpu) => ("ggml_vulkan", Some("gpu")),
        Some(uta_runtime_manager::NativeDeviceClass::IntegratedGpu) => {
            ("ggml_vulkan", Some("integrated_gpu"))
        }
        Some(uta_runtime_manager::NativeDeviceClass::Cpu) => ("ggml_cpu", Some("cpu")),
        None => ("ggml_vulkan", None),
    };
    Ok(RoformerRoute {
        backend,
        device_class,
    })
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
    request: &AnalyzeRequestV1,
    semantic_output: &str,
) -> EngineResult<(&'static str, serde_json::Value)> {
    let route = resolve_roformer_route(model, request)?;
    let (component, mut config) =
        roformer_dispatch_config(&route, &model.model_path, semantic_output)?;
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
    request: &AnalyzeRequestV1,
) -> EngineResult<(&'static str, serde_json::Value)> {
    model_dispatch(model, request, "pitch")
}

pub(super) fn execution_device(_backend: uta_runtime_manager::NativeBackend) -> &'static str {
    "ggml"
}

pub(super) fn resource_provenance(
    resource: &uta_runtime_manager::ResolvedModel,
) -> ResolvedResourceProvenanceV1 {
    ResolvedResourceProvenanceV1 {
        resource: format!("model:{}", resource.model_id),
        generation: resource.generation.clone(),
        content_digest: resource.model_content_digest.clone(),
        runtime: resource.runtime_id.clone(),
        runtime_generation: resource.runtime_generation.clone(),
        runtime_recipe_digest: resource.runtime_recipe_digest.clone(),
        backend: match resource.backend {
            uta_runtime_manager::NativeBackend::Ggml => "ggml",
        }
        .to_string(),
        device: execution_device(resource.backend).to_string(),
    }
}

impl AnalysisEngine {
    pub(super) fn resolve_execution_resources(
        &self,
        request: &AnalyzeRequestV1,
        plan: &EnginePlan,
    ) -> EngineResult<(Vec<uta_runtime_manager::ResolvedModel>, Vec<String>)> {
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
                    match self.runtime_manager.resolve_model_with_backend(
                        &resource.id,
                        request.execution_policy.runtime_policy,
                        request.execution_policy.requested_backend_for(&resource.id),
                    ) {
                        Ok(model) => resolved.push(model),
                        Err(error) if !requirement.required => degraded.push(format!(
                            "optional capability {} skipped: {}",
                            requirement.reason, error
                        )),
                        Err(error) => return Err(EngineError::from(error)),
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

    fn token(id: &str, text: &str) -> LyricTokenV1 {
        LyricTokenV1 {
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
    fn firered_is_limited_to_its_supported_language_families() {
        assert!(firered_language_applicable(Some("zh-CN"), Some("zh")));
        assert!(firered_language_applicable(Some("en"), None));
        assert!(firered_language_applicable(None, Some("und")));
        assert!(!firered_language_applicable(Some("ja"), Some("ja-JP")));
        assert!(firered_language_applicable(Some("ja"), Some("yue")));
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
        assert_eq!(qwen_alignment_words(&caller).unwrap()[0]["id"], "line-1");

        let mut generated = caller;
        generated.text = "风吹沙".to_string();
        generated.language = Some("zh".to_string());
        generated.tokens.clear();
        let units = qwen_alignment_words(&generated).unwrap();
        assert_eq!(units.len(), 3);
        assert_eq!(units[2]["text"], "沙");
    }
}
