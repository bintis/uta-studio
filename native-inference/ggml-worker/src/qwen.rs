use std::io::Write;
use std::path::Path;
use std::sync::Arc;

use serde::{Deserialize, Serialize};
use uta_ggml_runtime::qwen::aligner::{Alignment, AlignmentWindowTrace, AudioScope};
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
    #[serde(default)]
    audio_range: Option<AudioRange>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct AudioRange {
    start: u64,
    end: u64,
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
    #[serde(default, skip_serializing_if = "Option::is_none")]
    timing_issue: Option<String>,
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct Diagnostics {
    raw_classes: Vec<u32>,
    raw_timestamp_ms: Vec<u64>,
    corrected_timestamp_ms: Vec<u64>,
    windows: Vec<AlignmentWindowTrace>,
    prompt_tokens: usize,
    encoder_seconds: f64,
    decoder_seconds: f64,
}

pub fn infer(
    loaded: Option<crate::prepared::Weights>,
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
    let mut qwen = crate::prepared::qwen(loaded, runtime, device, model_path)?;
    qwen.retain_audio_intermediates(
        config
            .get("turbo_acceleration")
            .and_then(serde_json::Value::as_bool)
            == Some(true)
            && device.kind != uta_ggml_runtime::DeviceKind::Cpu,
    );
    let texts = request
        .words
        .iter()
        .map(|word| word.text.clone())
        .collect::<Vec<_>>();
    let scopes = alignment_scopes(&request)?;
    let aligned = qwen.align_wav(wav, &texts, &scopes, &mut progress)?;
    let (retained, reused) = qwen.audio_residency_bytes();
    if retained > 0 {
        crate::audio_cache::diagnostic(&format!(
            "Qwen device-resident audio bytes: retained={retained}, reused={reused}"
        ));
    }
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

fn alignment_scopes(request: &Request) -> Result<Vec<Option<AudioScope>>, String> {
    request
        .words
        .iter()
        .map(|word| {
            word.audio_range
                .as_ref()
                .map(|range| {
                    if range.start < request.source_start_micros || range.end <= range.start {
                        return Err(
                            "Qwen alignment anchor is outside the source timeline".to_string()
                        );
                    }
                    let to_sample = |time: u64| {
                        usize::try_from(
                            (time - request.source_start_micros) as u128
                                * uta_ggml_runtime::qwen::frontend::SAMPLE_RATE as u128
                                / 1_000_000,
                        )
                        .map_err(|_| "Qwen alignment sample offset overflows".to_string())
                    };
                    Ok(AudioScope {
                        start_sample: to_sample(range.start)?,
                        end_sample: to_sample(range.end)?,
                    })
                })
                .transpose()
        })
        .collect()
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
            timing_issue: aligned.timing_issue.clone(),
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
            windows: aligned.windows,
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
                || item.duration == 0
                || item
                    .timing_issue
                    .as_ref()
                    .is_some_and(|issue| issue.is_empty())
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
                audio_range: Some(AudioRange {
                    start: 500_000,
                    end: 1_500_000,
                }),
            }],
            language: Some("en".to_string()),
            source_start_micros: 500_000,
            model_content_digest: "model-provenance".to_string(),
        };
        let scopes = alignment_scopes(&request).unwrap();
        assert_eq!(
            scopes[0],
            Some(AudioScope {
                start_sample: 0,
                end_sample: 16_000
            })
        );
        let aligned = Alignment {
            words: vec![uta_ggml_runtime::qwen::aligner::AlignedWord {
                text: "All".to_string(),
                start_seconds: 0.08,
                end_seconds: 0.24,
                timing_issue: None,
            }],
            raw_classes: vec![1, 3],
            raw_timestamp_ms: vec![80, 240],
            corrected_timestamp_ms: vec![80, 240],
            windows: Vec::new(),
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
