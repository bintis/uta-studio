use std::io::Write;
use std::path::Path;
use std::sync::Arc;

use serde::{Deserialize, Serialize};
use uta_ggml_runtime::qwen::Qwen;
use uta_ggml_runtime::qwen::aligner::Alignment;
use uta_ggml_runtime::{DeviceDescriptor, GgmlRuntime};

const MODEL_ID: &str = "qwen3_forced_aligner_0_6b";

#[derive(Debug, Deserialize)]
struct Request {
    words: Vec<WordConfig>,
    #[serde(default)]
    language: Option<String>,
    #[serde(default)]
    source_start_micros: u64,
    model_content_digest: String,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct WordConfig {
    id: String,
    text: String,
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct Evidence {
    contract: String,
    version: u32,
    transcript: String,
    language: Option<String>,
    items: Vec<Item>,
    source_expert: String,
    model_sha256: String,
    runtime_manifest_sha256: String,
    backend: String,
    diagnostics: Diagnostics,
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct Item {
    id: String,
    text: String,
    level: String,
    start: u64,
    duration: u64,
    confidence: Option<f32>,
    authority: String,
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct Diagnostics {
    raw_classes: Vec<u32>,
    raw_timestamp_ms: Vec<u64>,
    corrected_timestamp_ms: Vec<u64>,
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
        .map_err(|error| format!("Qwen forced-alignment request is invalid: {error}"))?;
    validate_request(&request)?;
    let qwen = Qwen::load(runtime, device, model_path)?;
    let texts = request
        .words
        .iter()
        .map(|word| word.text.clone())
        .collect::<Vec<_>>();
    let aligned = qwen.align_wav(wav, &texts)?;
    progress(1, 1);
    let evidence = evidence(request, aligned, runtime_manifest_digest, backend)?;
    write_evidence(destination, &evidence)
}

pub fn publish(source: &Path, destination: &Path) -> Result<(), String> {
    if destination.exists() {
        return Err("Qwen alignment evidence destination already exists".to_string());
    }
    let bytes = std::fs::read(source)
        .map_err(|error| format!("could not read raw Qwen alignment evidence: {error}"))?;
    let evidence: Evidence = serde_json::from_slice(&bytes)
        .map_err(|error| format!("raw Qwen alignment evidence is invalid: {error}"))?;
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
        .map_err(|error| format!("could not create Qwen publication temporary: {error}"))?;
    if let Err(error) = encode_evidence(&mut file, &evidence) {
        drop(file);
        let _ = std::fs::remove_file(&temporary);
        return Err(error);
    }
    drop(file);
    let publish = std::fs::hard_link(&temporary, destination)
        .map_err(|error| format!("could not publish Qwen evidence without overwrite: {error}"));
    let cleanup = std::fs::remove_file(&temporary)
        .map_err(|error| format!("could not remove Qwen temporary evidence: {error}"));
    publish.and(cleanup)
}

fn validate_request(request: &Request) -> Result<(), String> {
    if request.words.is_empty() {
        return Err("Qwen forced alignment requires non-empty caller word units".to_string());
    }
    if request.model_content_digest.trim().is_empty() {
        return Err("Qwen forced alignment requires model provenance".to_string());
    }
    if request
        .words
        .iter()
        .any(|word| word.id.trim().is_empty() || word.text.trim().is_empty())
    {
        return Err("Qwen forced-alignment word ids and text must be non-empty".to_string());
    }
    Ok(())
}

fn evidence(
    request: Request,
    aligned: Alignment,
    runtime_manifest_digest: &str,
    backend: &str,
) -> Result<Evidence, String> {
    if request.words.len() != aligned.words.len() {
        return Err("Qwen forced aligner changed the caller word count".to_string());
    }
    let mut items = Vec::with_capacity(request.words.len());
    for (word, aligned) in request.words.iter().zip(&aligned.words) {
        let start = seconds_to_micros(aligned.start_seconds)?
            .checked_add(request.source_start_micros)
            .ok_or("Qwen aligned start overflows the canonical timeline")?;
        let end = seconds_to_micros(aligned.end_seconds)?
            .checked_add(request.source_start_micros)
            .ok_or("Qwen aligned end overflows the canonical timeline")?;
        items.push(Item {
            id: word.id.clone(),
            text: word.text.clone(),
            level: "word".to_string(),
            start,
            duration: end.saturating_sub(start),
            confidence: None,
            authority: "soft".to_string(),
        });
    }
    Ok(Evidence {
        contract: "uta.analysis-engine.alignment".to_string(),
        version: 1,
        transcript: request
            .words
            .iter()
            .map(|word| word.text.as_str())
            .collect::<Vec<_>>()
            .join(" "),
        language: request.language,
        items,
        source_expert: MODEL_ID.to_string(),
        model_sha256: request.model_content_digest,
        runtime_manifest_sha256: runtime_manifest_digest.to_string(),
        backend: backend.to_string(),
        diagnostics: Diagnostics {
            raw_classes: aligned.raw_classes,
            raw_timestamp_ms: aligned.raw_timestamp_ms,
            corrected_timestamp_ms: aligned.corrected_timestamp_ms,
            prompt_tokens: aligned.prompt_tokens,
            encoder_seconds: aligned.encoder_seconds,
            decoder_seconds: aligned.decoder_seconds,
        },
    })
}

fn seconds_to_micros(seconds: f64) -> Result<u64, String> {
    let value = seconds * 1_000_000.0;
    if !value.is_finite() || value < 0.0 || value > u64::MAX as f64 {
        return Err("Qwen timestamp is outside the canonical timeline".to_string());
    }
    Ok(value.round() as u64)
}

fn validate_evidence(evidence: &Evidence) -> Result<(), String> {
    if evidence.contract != "uta.analysis-engine.alignment"
        || evidence.version != 1
        || evidence.source_expert != MODEL_ID
        || evidence.items.is_empty()
        || evidence.items.iter().any(|item| {
            item.id.is_empty()
                || item.text.is_empty()
                || item.level != "word"
                || item.authority != "soft"
                || item.start.checked_add(item.duration).is_none()
        })
        || evidence.diagnostics.raw_classes.len() != evidence.items.len() * 2
        || evidence.diagnostics.raw_timestamp_ms.len() != evidence.items.len() * 2
        || evidence.diagnostics.corrected_timestamp_ms.len() != evidence.items.len() * 2
    {
        return Err("raw Qwen alignment evidence is structurally invalid".to_string());
    }
    Ok(())
}

fn write_evidence(destination: &Path, evidence: &Evidence) -> Result<(), String> {
    if destination.exists() {
        return Err("raw Qwen alignment target already exists".to_string());
    }
    let mut file = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(destination)
        .map_err(|error| format!("could not create raw Qwen alignment evidence: {error}"))?;
    encode_evidence(&mut file, evidence)
}

fn encode_evidence(writer: &mut impl Write, evidence: &Evidence) -> Result<(), String> {
    serde_json::to_writer(writer, evidence)
        .map_err(|error| format!("could not encode Qwen alignment evidence: {error}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn evidence_preserves_caller_units_and_offsets() {
        let request = Request {
            words: vec![WordConfig {
                id: "word-1".to_string(),
                text: "All".to_string(),
            }],
            language: Some("en".to_string()),
            source_start_micros: 500_000,
            model_content_digest: "model-provenance".to_string(),
        };
        let aligned = Alignment {
            words: vec![uta_ggml_runtime::qwen::aligner::AlignedWord {
                text: "All".to_string(),
                start_seconds: 0.08,
                end_seconds: 0.24,
            }],
            raw_classes: vec![1, 3],
            raw_timestamp_ms: vec![80, 240],
            corrected_timestamp_ms: vec![80, 240],
            prompt_tokens: 4,
            encoder_seconds: 1.0,
            decoder_seconds: 2.0,
        };
        let value = evidence(request, aligned, "runtime-provenance", "ggml_cpu").unwrap();
        assert_eq!(value.items[0].start, 580_000);
        assert_eq!(value.items[0].duration, 160_000);
        assert_eq!(value.items[0].authority, "soft");
        validate_evidence(&value).unwrap();
    }
}
