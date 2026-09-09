use std::io::Write;
use std::path::Path;
use std::sync::Arc;

use serde::{Deserialize, Serialize};
use uta_ggml_runtime::qwen::Qwen;
use uta_ggml_runtime::qwen::asr::{DEFAULT_MAX_NEW_TOKENS, Transcription};
use uta_ggml_runtime::{DeviceDescriptor, GgmlRuntime};

const MODEL_ID: &str = "qwen3_asr_1_7b";

#[derive(Debug, Deserialize)]
struct Request {
    model_content_digest: String,
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct Evidence {
    contract: String,
    version: u32,
    authority: String,
    language: Option<String>,
    text: String,
    tokens: Vec<TranscriptToken>,
    confidence: Option<f32>,
    source_experts: Vec<String>,
    alternatives: Vec<String>,
    model_sha256: String,
    runtime_manifest_sha256: String,
    backend: String,
    diagnostics: Diagnostics,
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct TranscriptToken {
    id: String,
    text: String,
    confidence: Option<f32>,
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct Diagnostics {
    language_name: Option<String>,
    generated_tokens: Vec<u32>,
    finished: bool,
    /// Windows the decoder could not finish and that contributed no text.
    /// Non-zero is expected on real songs, where instrumental passages give
    /// the decoder nothing to transcribe.
    #[serde(default)]
    unfinished_windows: usize,
    prompt_tokens: usize,
    encoder_seconds: f64,
    decoder_seconds: f64,
}

pub fn infer(
    runtime: Arc<GgmlRuntime>,
    device: &DeviceDescriptor,
    model_path: &Path,
    wav: &Path,
    runtime_manifest_digest: &str,
    backend: &str,
    config: &serde_json::Value,
    destination: &Path,
    mut progress: impl FnMut(u64, u64),
) -> Result<(), String> {
    let request: Request = serde_json::from_value(config.clone())
        .map_err(|error| format!("Qwen ASR request is invalid: {error}"))?;
    if request.model_content_digest.trim().is_empty() {
        return Err("Qwen ASR requires model provenance".to_string());
    }
    let qwen = Qwen::load(runtime, device, model_path)?;
    let transcription = qwen.transcribe_wav(wav, DEFAULT_MAX_NEW_TOKENS)?;
    progress(1, 1);
    let evidence = evidence(request, transcription, runtime_manifest_digest, backend)?;
    write_evidence(destination, &evidence)
}

pub fn publish(source: &Path, destination: &Path) -> Result<(), String> {
    if destination.exists() {
        return Err("Qwen transcript evidence destination already exists".to_string());
    }
    let bytes = std::fs::read(source)
        .map_err(|error| format!("could not read raw Qwen transcript evidence: {error}"))?;
    let evidence: Evidence = serde_json::from_slice(&bytes)
        .map_err(|error| format!("raw Qwen transcript evidence is invalid: {error}"))?;
    validate_evidence(&evidence)?;
    let temporary = destination.with_extension(format!(
        "json.{}.{}.tmp",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos()
    ));
    let mut file = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&temporary)
        .map_err(|error| {
            format!("could not create Qwen transcript publication temporary: {error}")
        })?;
    if let Err(error) = encode_evidence(&mut file, &evidence) {
        drop(file);
        let _ = std::fs::remove_file(&temporary);
        return Err(error);
    }
    drop(file);
    let publish = std::fs::hard_link(&temporary, destination)
        .map_err(|error| format!("could not publish Qwen transcript without overwrite: {error}"));
    let cleanup = std::fs::remove_file(&temporary)
        .map_err(|error| format!("could not remove Qwen transcript temporary: {error}"));
    publish.and(cleanup)
}

fn evidence(
    request: Request,
    transcription: Transcription,
    runtime_manifest_digest: &str,
    backend: &str,
) -> Result<Evidence, String> {
    let language = transcription
        .language_name
        .as_deref()
        .map(language_name_to_bcp47);
    let evidence = Evidence {
        contract: "uta.analysis-engine.transcript".to_string(),
        version: 1,
        authority: "generated".to_string(),
        language,
        text: transcription.text,
        tokens: Vec::new(),
        confidence: None,
        source_experts: vec![MODEL_ID.to_string()],
        alternatives: Vec::new(),
        model_sha256: request.model_content_digest,
        runtime_manifest_sha256: runtime_manifest_digest.to_string(),
        backend: backend.to_string(),
        diagnostics: Diagnostics {
            language_name: transcription.language_name,
            generated_tokens: transcription.generated_tokens,
            finished: transcription.finished,
            unfinished_windows: transcription.unfinished_windows,
            prompt_tokens: transcription.prompt_tokens,
            encoder_seconds: transcription.encoder_seconds,
            decoder_seconds: transcription.decoder_seconds,
        },
    };
    validate_evidence(&evidence)?;
    Ok(evidence)
}

fn validate_evidence(evidence: &Evidence) -> Result<(), String> {
    if evidence.contract != "uta.analysis-engine.transcript"
        || evidence.version != 1
        || evidence.authority != "generated"
        || evidence.text.trim().is_empty()
        || evidence.source_experts != [MODEL_ID]
        || evidence.model_sha256.trim().is_empty()
        || evidence.runtime_manifest_sha256.trim().is_empty()
        || !matches!(evidence.backend.as_str(), "ggml_cpu" | "ggml_vulkan")
        || evidence.language
            != evidence
                .diagnostics
                .language_name
                .as_deref()
                .map(language_name_to_bcp47)
        || evidence.diagnostics.generated_tokens.is_empty()
        || !evidence.diagnostics.finished
        || evidence.diagnostics.prompt_tokens == 0
        || !evidence.diagnostics.encoder_seconds.is_finite()
        || evidence.diagnostics.encoder_seconds < 0.0
        || !evidence.diagnostics.decoder_seconds.is_finite()
        || evidence.diagnostics.decoder_seconds < 0.0
    {
        return Err("raw Qwen transcript evidence is structurally invalid".to_string());
    }
    Ok(())
}

fn language_name_to_bcp47(language: &str) -> String {
    let normalized = language.trim().to_ascii_lowercase();
    let code = match normalized.as_str() {
        "english" => "en",
        "chinese" | "mandarin" => "zh",
        "cantonese" => "yue",
        "japanese" => "ja",
        "korean" => "ko",
        "german" => "de",
        "french" => "fr",
        "spanish" => "es",
        "portuguese" => "pt",
        "russian" => "ru",
        "italian" => "it",
        "dutch" => "nl",
        "polish" => "pl",
        "turkish" => "tr",
        "arabic" => "ar",
        "persian" => "fa",
        "vietnamese" => "vi",
        "thai" => "th",
        "indonesian" => "id",
        "malay" => "ms",
        "hindi" => "hi",
        "bengali" => "bn",
        "urdu" => "ur",
        "hebrew" => "he",
        "greek" => "el",
        "czech" => "cs",
        "swedish" => "sv",
        "danish" => "da",
        "finnish" => "fi",
        "norwegian" => "no",
        "hungarian" => "hu",
        value
            if (2..=3).contains(&value.len())
                && value.bytes().all(|byte| byte.is_ascii_lowercase()) =>
        {
            value
        }
        _ => "und",
    };
    code.to_string()
}

fn write_evidence(destination: &Path, evidence: &Evidence) -> Result<(), String> {
    if destination.exists() {
        return Err("raw Qwen transcript target already exists".to_string());
    }
    let mut file = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(destination)
        .map_err(|error| format!("could not create raw Qwen transcript evidence: {error}"))?;
    encode_evidence(&mut file, evidence)
}

fn encode_evidence(writer: &mut impl Write, evidence: &Evidence) -> Result<(), String> {
    serde_json::to_writer(writer, evidence)
        .map_err(|error| format!("could not encode Qwen transcript evidence: {error}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn transcription() -> Transcription {
        Transcription {
            text: "All he just is for us.".to_string(),
            language_name: Some("English".to_string()),
            raw_text: "language English<asr_text>All he just is for us.".to_string(),
            generated_tokens: vec![11_528, 6_364, 151_645],
            finished: true,
            unfinished_windows: 0,
            prompt_tokens: 171,
            encoder_seconds: 0.4,
            decoder_seconds: 3.7,
        }
    }

    #[test]
    fn evidence_preserves_native_transcript_and_provenance() {
        let value = evidence(
            Request {
                model_content_digest: "model-provenance".to_string(),
            },
            transcription(),
            "runtime-provenance",
            "ggml_vulkan",
        )
        .unwrap();
        assert_eq!(value.contract, "uta.analysis-engine.transcript");
        assert_eq!(value.text, "All he just is for us.");
        assert_eq!(value.language.as_deref(), Some("en"));
        assert_eq!(value.source_experts, [MODEL_ID]);
        validate_evidence(&value).unwrap();
    }

    #[test]
    fn language_names_are_normalized_to_bcp47_without_guessing_unknown_names() {
        assert_eq!(language_name_to_bcp47("Chinese"), "zh");
        assert_eq!(language_name_to_bcp47("Cantonese"), "yue");
        assert_eq!(language_name_to_bcp47("xx"), "xx");
        assert_eq!(language_name_to_bcp47("unlisted language"), "und");
    }

    #[test]
    fn publication_is_atomic_and_does_not_overwrite() {
        let root = std::env::temp_dir().join(format!(
            "uta-qwen-asr-evidence-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_nanos()
        ));
        std::fs::create_dir(&root).unwrap();
        let raw = root.join("raw.json");
        let published = root.join("transcript.json");
        let value = evidence(
            Request {
                model_content_digest: "model-provenance".to_string(),
            },
            transcription(),
            "runtime-provenance",
            "ggml_cpu",
        )
        .unwrap();
        write_evidence(&raw, &value).unwrap();
        publish(&raw, &published).unwrap();
        assert!(publish(&raw, &published).is_err());
        assert!(published.is_file());
        assert_eq!(
            std::fs::read(&raw).unwrap(),
            std::fs::read(&published).unwrap()
        );
        std::fs::remove_dir_all(root).unwrap();
    }
}
