use crate::artifact::{TranscriptArtifactV1, TranscriptAuthorityV1, TranscriptTokenV1};
use crate::contract::{
    AnalyzeRequestV1, EngineError, EngineErrorCode, EngineResult, LyricTokenV1,
    ResolvedResourceProvenanceV1,
};

pub(super) fn cancelled(request: &AnalyzeRequestV1) -> EngineError {
    EngineError::new(EngineErrorCode::Cancelled, "analysis request was cancelled")
        .for_request(&request.request_id)
}

/// Joins caller lyric tokens with an explicit newline between every token
/// (unlike `request_lyrics_text`, which drops all separators between CJK
/// tokens for compact display/reference-comparison purposes). Downstream
/// long-form alignment windowing (`alignment_text_units`) only recognizes
/// line/word units via whitespace; joining CJK lyrics with no separator at
/// all collapses an entire song into a single character run, forcing a much
/// more fragile per-character split instead of the per-line split every
/// other language already gets from its own inter-word spaces. Preserving
/// line boundaries here does not change what text exists, only how the
/// caller's own line tokens are concatenated for the aligner to consume.
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

pub(super) fn roformer_backend(
    model: &uta_runtime_manager::ResolvedModel,
) -> EngineResult<&'static str> {
    match model.backend {
        uta_runtime_manager::NativeBackend::OpenVino => Ok("openvino_gpu"),
        uta_runtime_manager::NativeBackend::CpuReference => Ok("openvino_cpu"),
        uta_runtime_manager::NativeBackend::Vulkan => Ok("ggml_vulkan"),
        _ => Err(EngineError::new(
            EngineErrorCode::RuntimeResolutionFailed,
            format!(
                "model {} resolved to a backend unsupported by the RoFormer route",
                model.model_id
            ),
        )),
    }
}

pub(super) fn roformer_component(backend: &str) -> &'static str {
    match backend {
        "ggml_vulkan" => "uta-ggml-worker",
        _ => "uta-openvino-worker",
    }
}

pub(super) fn openvino_backend(
    model: &uta_runtime_manager::ResolvedModel,
) -> EngineResult<&'static str> {
    match model.backend {
        uta_runtime_manager::NativeBackend::OpenVino => Ok("openvino_gpu"),
        uta_runtime_manager::NativeBackend::CpuReference => Ok("openvino_cpu"),
        _ => Err(EngineError::new(
            EngineErrorCode::RuntimeResolutionFailed,
            format!(
                "model {} resolved to a backend that the OpenVINO worker cannot execute",
                model.model_id
            ),
        )),
    }
}

pub(super) fn execution_device(backend: uta_runtime_manager::NativeBackend) -> &'static str {
    match backend {
        uta_runtime_manager::NativeBackend::OpenVino
        | uta_runtime_manager::NativeBackend::Vulkan => "device:0",
        uta_runtime_manager::NativeBackend::NativeDsp => "native",
        uta_runtime_manager::NativeBackend::CpuReference => "diagnostic_cpu",
    }
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
            uta_runtime_manager::NativeBackend::OpenVino => "openvino",
            uta_runtime_manager::NativeBackend::Vulkan => "vulkan",
            uta_runtime_manager::NativeBackend::NativeDsp => "native_dsp",
            uta_runtime_manager::NativeBackend::CpuReference => "cpu_reference",
        }
        .to_string(),
        device: execution_device(resource.backend).to_string(),
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
        }
    }

    #[test]
    fn caller_transcript_text_preserves_line_boundaries_for_cjk_lyrics() {
        let tokens = vec![
            token("line-1", "风吹沙蝶恋花千古佳话"),
            token("line-2", "似水中月情迷着镜中花"),
        ];
        let text = caller_transcript_text(&tokens);
        // A newline between lines, not the empty separator that previously
        // collapsed every line into one character run and forced the
        // long-form aligner into fragile per-character windowing instead of
        // the much more robust per-line windowing every other language
        // already gets from its own inter-word spaces.
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
}
