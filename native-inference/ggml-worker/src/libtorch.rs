//! Native LibTorch XPU execution route of the shared worker.
//!
//! A task whose config names `backend: "libtorch_xpu"` runs its model through
//! the installed native LibTorch runtime instead of the GGML shared libraries.
//! Rust keeps ownership of decoding, conditioning, typed evidence and
//! publication; only the learned graph executes inside the native library.
//! The route is explicit: a missing runtime or device is an error, never a
//! fallback to GGML or to the CPU.
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use serde::Deserialize;
use serde_json::Value;
use sha2::{Digest, Sha256};
use uta_libtorch_runtime as native;
use uta_libtorch_runtime::{Backend, Library, Model, Precision};

pub const BACKEND: &str = "libtorch_xpu";
const RUNTIME_DIRECTORY: &str = "libtorch-xpu";

/// True when the task explicitly selects the LibTorch XPU backend.
pub fn selected(config: &Value) -> bool {
    config.get("backend").and_then(Value::as_str) == Some(BACKEND)
}

#[derive(Debug, Deserialize)]
struct Manifest {
    backend: String,
    native_library: String,
    #[serde(default)]
    environment: BTreeMap<String, String>,
}

/// The installed native runtime directory, validated before any device work.
#[derive(Debug)]
pub struct Runtime {
    root: PathBuf,
    library: PathBuf,
    environment: BTreeMap<String, String>,
    manifest_content_digest: String,
}

fn runtime_root() -> Result<PathBuf, String> {
    if let Some(configured) = std::env::var_os("UTA_STUDIO_LIBTORCH_RUNTIME_DIR") {
        return Ok(PathBuf::from(configured));
    }
    let data = std::env::var_os("XDG_DATA_HOME")
        .map(PathBuf::from)
        .or_else(|| {
            std::env::var_os("HOME")
                .map(PathBuf::from)
                .map(|home| home.join(".local/share"))
        })
        .ok_or_else(|| "LibTorch runtime location is unavailable".to_string())?;
    Ok(data
        .join("uta-studio")
        .join("runtime")
        .join(RUNTIME_DIRECTORY))
}

impl Runtime {
    /// Reads and validates the installed runtime manifest. This neither loads
    /// the native library nor touches a device.
    pub fn locate(model_id: &str) -> Result<Self, String> {
        crate::runtime::validate_runtime(model_id)
            .map_err(|_| format!("model {model_id} has no native executor in this worker"))?;
        let root = runtime_root()?;
        let canonical_root = root.canonicalize().map_err(|error| {
            format!(
                "LibTorch XPU runtime directory is unavailable: {}: {error}",
                root.display()
            )
        })?;
        let bytes = std::fs::read(canonical_root.join("runtime-manifest.json"))
            .map_err(|error| format!("LibTorch XPU runtime manifest is unavailable: {error}"))?;
        let manifest: Manifest = serde_json::from_slice(&bytes)
            .map_err(|error| format!("LibTorch XPU runtime manifest is invalid: {error}"))?;
        if manifest.backend != BACKEND {
            return Err(format!(
                "installed native runtime is {}, not {BACKEND}; no backend fallback",
                manifest.backend
            ));
        }
        let relative = Path::new(&manifest.native_library);
        if relative.as_os_str().is_empty()
            || relative
                .components()
                .any(|component| !matches!(component, std::path::Component::Normal(_)))
        {
            return Err("LibTorch XPU runtime manifest names an unsafe library path".to_string());
        }
        let library = canonical_root
            .join(relative)
            .canonicalize()
            .map_err(|error| format!("LibTorch XPU native library is unavailable: {error}"))?;
        if !library.starts_with(&canonical_root) || !library.is_file() {
            return Err("LibTorch XPU native library escapes its runtime or is not a file".into());
        }
        Ok(Self {
            root: canonical_root,
            library,
            environment: manifest.environment,
            manifest_content_digest: hex::encode(Sha256::digest(&bytes)),
        })
    }

    pub fn manifest_content_digest(&self) -> &str {
        &self.manifest_content_digest
    }

    /// Applies the installed runtime's declared process environment (device
    /// selector, driver locations, kernel cache, strict oneDNN math) before
    /// the native library initializes. Values are read by the native
    /// libraries at initialization, so they must be set before loading.
    fn apply_environment(&self) {
        for (name, value) in &self.environment {
            // The dynamic loader reads the library search path only at
            // process start; the Analysis Engine applies that entry when it
            // spawns this worker, so it is not re-applied here.
            if name == "LD_LIBRARY_PATH" {
                continue;
            }
            // SAFETY: the worker is single-threaded at this point; no other
            // thread reads the environment concurrently.
            unsafe {
                std::env::set_var(name, value);
            }
        }
    }

    /// Opens one model session on the explicitly selected XPU device. The
    /// precision policy follows the recorded full-song qualification: mixed
    /// attention for the six separators, strict for every other model.
    pub fn open(&self, model_id: &str, model_path: &Path, config: &Value) -> Result<Model, String> {
        let device = device_index(config)?;
        self.apply_environment();
        let library = Library::load(&self.library)?;
        let info = library.build_info();
        if info.compiled_backend != BACKEND {
            return Err(format!(
                "installed native library at {} was built for {}, not {BACKEND}",
                self.root.display(),
                info.compiled_backend
            ));
        }
        library.open(
            model_id,
            model_path,
            Backend::LibtorchXpu,
            device,
            precision(model_id),
        )
    }
}

fn device_index(config: &Value) -> Result<u16, String> {
    match config.get("device_class").and_then(Value::as_str) {
        None | Some("gpu") => {}
        Some(other) => {
            return Err(format!(
                "LibTorch XPU executes only on the discrete Intel GPU; device class {other} has no native route"
            ));
        }
    }
    match config.get("xpu_device") {
        None => Ok(0),
        Some(value) => value
            .as_u64()
            .and_then(|index| u16::try_from(index).ok())
            .ok_or_else(|| "xpu_device must be a small non-negative integer".to_string()),
    }
}

/// RoFormer's `layout_preserving_roformer_attention` casts Q/K/V to FP16 before
/// SDPA with no overflow guard. `bs_polarformer_public_instrumental` OOM'd and
/// `melband_roformer_harmony` emitted nonfinite masks under this path during the
/// original 12-second qualification pass; `bs_roformer_leap_xe90_vocals` then
/// emitted nonfinite masks twice in production on a real full song. All six
/// models share the same unguarded cast, so none of them get MixedAttention
/// until it accumulates in FP32 instead of FP16.
fn precision(_model_id: &str) -> Precision {
    Precision::Strict
}

/// Executes one task's model through the native runtime, writing the same raw
/// engine output the GGML route produces so publication stays shared.
#[allow(clippy::too_many_arguments)]
pub fn execute(
    runtime: &Runtime,
    model_id: &str,
    model_path: &Path,
    input: &Path,
    secondary_input: Option<&Path>,
    secondary_source: Option<&Path>,
    config: &Value,
    engine_output: &Path,
    report: &mut dyn FnMut(u64, u64),
) -> Result<(), String> {
    let model = runtime.open(model_id, model_path, config)?;
    let digest = runtime.manifest_content_digest();
    match model_id {
        "rmvpe" => {
            let frames = native::rmvpe::Rmvpe::from_model(model).process_wav(input, report)?;
            crate::engine::write_raw_rmvpe_evidence(frames, engine_output)
        }
        "fcpe" => {
            let frames = native::fcpe::Fcpe::from_model(model)?.process_wav_with_threshold(
                input,
                uta_model_settings::number(config, "voiced_threshold", 0.006) as f32,
                report,
            )?;
            crate::engine::write_raw_fcpe_evidence(frames, engine_output)
        }
        "basic_pitch" => {
            let frames =
                native::basic_pitch::BasicPitch::from_model(model).process_wav(input, report)?;
            crate::engine::write_raw_basic_pitch_evidence(frames, engine_output)
        }
        "game_1_0_3_small" | "game_1_0_3_medium" | "game_1_0_3_large" => crate::game::infer_with(
            model_path,
            input,
            digest,
            BACKEND,
            config,
            engine_output,
            report,
            |input, params, report| {
                let game = native::game::Game::from_model(model)?;
                let variant = game.config().variant.to_string();
                let output = game.process_wav_with_progress(input, &game_params(params), report)?;
                Ok((variant, game_output(output)))
            },
        ),
        "jbm555_cectc_80" => {
            let vocal = secondary_input
                .ok_or_else(|| "JBM555 requires the prepared vocal input".to_string())?;
            crate::jbm555::infer_with(
                input,
                vocal,
                "shared_libtorch_xpu",
                BACKEND,
                config,
                engine_output,
                report,
                |mix, vocal, report| {
                    let (notes, samples) = native::jbm555::Jbm555::from_model(model)
                        .process_wavs_with_thresholds(
                            mix,
                            vocal,
                            uta_model_settings::number(config, "onset_threshold", 0.32) as f32,
                            uta_model_settings::number(config, "offset_threshold", 0.70) as f32,
                            report,
                        )?;
                    Ok((notes.into_iter().map(jbm_note).collect(), samples))
                },
            )
        }
        "stars" => crate::stars::infer_with(
            input,
            secondary_source.ok_or_else(|| "STARS requires shared RMVPE evidence".to_string())?,
            digest,
            BACKEND,
            config,
            engine_output,
            |completed, total| report(completed, total),
            |wav, raw_f0, words, source_start_micros, include_technique, g2p| {
                let shared = native::stars::prepare_wav_inputs(wav, raw_f0)?;
                let words = words
                    .iter()
                    .map(|word| native::stars::TranscriptWord {
                        id: word.id.clone(),
                        text: word.text.clone(),
                        start_micros: word.start_micros,
                        duration_micros: word.duration_micros,
                    })
                    .collect::<Vec<_>>();
                let result = native::stars::Stars::from_model(model)
                    .infer_transcript_with_threshold(
                        &shared,
                        &words,
                        source_start_micros,
                        include_technique,
                        uta_model_settings::number(config, "boundary_threshold", 0.8) as f32,
                        |texts| {
                            let phones = g2p.phonemize_words(texts)?;
                            Ok(native::stars::PhonemeInput {
                                phone_ids: phones.phone_ids,
                                phone_to_word: phones.phone_to_word,
                            })
                        },
                        |_, _| {},
                    )?;
                Ok(stars_result(result))
            },
        ),
        "rosvot" => crate::rosvot::infer_with(
            input,
            secondary_source.ok_or_else(|| "ROSVOT requires shared RMVPE evidence".to_string())?,
            digest,
            BACKEND,
            config,
            engine_output,
            |completed, total| report(completed, total),
            |wav, raw_f0, words, source_start_micros| {
                let shared = native::rosvot::prepare_wav_inputs(wav, raw_f0)?;
                let words = words
                    .iter()
                    .map(|word| native::rosvot::TranscriptWord {
                        id: word.id.clone(),
                        text: word.text.clone(),
                        start_micros: word.start_micros,
                        duration_micros: word.duration_micros,
                    })
                    .collect::<Vec<_>>();
                let result = native::rosvot::Rosvot::from_model(model)
                    .infer_transcript_with_threshold(
                        &shared,
                        &words,
                        source_start_micros,
                        uta_model_settings::number(config, "boundary_threshold", 0.85) as f32,
                        |_, _| {},
                    )?;
                Ok(rosvot_result(result))
            },
        ),
        "qwen3_forced_aligner_0_6b" => crate::qwen::infer_with(
            input,
            digest,
            BACKEND,
            config,
            engine_output,
            |completed, total| report(completed, total),
            |wav, texts, scopes, progress| {
                let scopes = scopes
                    .iter()
                    .map(|scope| {
                        scope
                            .as_ref()
                            .map(|scope| native::qwen::aligner::AudioScope {
                                start_sample: scope.start_sample,
                                end_sample: scope.end_sample,
                            })
                    })
                    .collect::<Vec<_>>();
                let aligned = native::qwen::Qwen::from_model(model)?
                    .align_wav(wav, texts, &scopes, progress)?;
                Ok(alignment(aligned))
            },
        ),
        "qwen3_asr_1_7b" => crate::qwen_asr::infer_with(
            input,
            digest,
            BACKEND,
            config,
            engine_output,
            |completed, total| report(completed, total),
            |wav, forced_language, progress| {
                let transcription = native::qwen::Qwen::from_model(model)?.transcribe_wav(
                    wav,
                    uta_model_settings::number(
                        config,
                        "max_new_tokens",
                        native::qwen::asr::DEFAULT_MAX_NEW_TOKENS as f64,
                    ) as usize,
                    forced_language,
                    progress,
                )?;
                Ok(qwen_transcription(transcription))
            },
        ),
        "firered_asr2_aed" => crate::firered::infer_with(
            model_path,
            input,
            digest,
            BACKEND,
            config,
            engine_output,
            |completed, total| report(completed, total),
            |wav, cmvn, tokens, progress| {
                let transcription = native::firered::FireRed::from_model(model)
                    .transcribe_wav_with_budget(
                        wav,
                        cmvn,
                        tokens,
                        uta_model_settings::number(
                            config,
                            "max_new_tokens",
                            native::firered::MAX_GENERATED_TOKENS as f64,
                        ) as usize,
                        progress,
                    )?;
                Ok(firered_transcription(transcription))
            },
        ),
        _ => {
            // Every separator: complete direct stem; the worker publishes the
            // residual from the decoded input exactly as on the GGML route.
            let mut roformer = native::roformer::Roformer::from_model(model)?;
            if config["model_settings"]["overlap"].is_number() {
                roformer.set_overlap(uta_model_settings::number(config, "overlap", 2.0) as usize)?;
            }
            roformer.process_wav(input, engine_output, &mut |completed, total| {
                report(completed, total)
            })
        }
    }
}

// The native crate compiles the shared host pipelines against its own model
// type, so their result structs are distinct nominal types with identical
// fields. These conversions keep the worker's evidence builders single-sourced
// on the GGML-runtime definitions.

fn game_params(params: &uta_ggml_runtime::game::GameInferParams) -> native::game::GameInferParams {
    native::game::GameInferParams {
        language: params.language,
        d3pm_steps: params.d3pm_steps,
        boundary_threshold: params.boundary_threshold,
        boundary_radius: params.boundary_radius,
        note_threshold: params.note_threshold,
        seed: params.seed,
        known_boundaries: params.known_boundaries.clone(),
    }
}

fn game_output(output: native::game::GameInferOutput) -> uta_ggml_runtime::game::GameInferOutput {
    uta_ggml_runtime::game::GameInferOutput {
        notes: output
            .notes
            .into_iter()
            .map(|note| uta_ggml_runtime::game::GameNote {
                offset_micros: note.offset_micros,
                duration_micros: note.duration_micros,
                pitch_midi: note.pitch_midi,
                voiced: note.voiced,
            })
            .collect(),
        boundaries: output.boundaries,
        num_frames: output.num_frames,
    }
}

fn jbm_note(note: native::jbm555::Note) -> uta_ggml_runtime::jbm555::Note {
    uta_ggml_runtime::jbm555::Note {
        range: uta_ggml_runtime::jbm555::NoteRange {
            start: note.range.start,
            end: note.range.end,
        },
        midi: note.midi,
        onset_score: note.onset_score,
        offset_score: note.offset_score,
        pitch_score: note.pitch_score,
    }
}

fn stars_result(result: native::stars::StarsResult) -> uta_ggml_runtime::stars::StarsResult {
    uta_ggml_runtime::stars::StarsResult {
        valid_frames: result.valid_frames,
        note_boundary_logits: result.note_boundary_logits,
        regulated_note_boundaries: result.regulated_note_boundaries,
        notes: result
            .notes
            .into_iter()
            .map(|note| uta_ggml_runtime::stars::RawNote {
                start_frame: note.start_frame,
                end_frame: note.end_frame,
                pitch_logits: note.pitch_logits,
                midi: note.midi,
            })
            .collect(),
        techniques: result.techniques.map(|techniques| {
            techniques
                .into_iter()
                .map(|technique| uta_ggml_runtime::stars::RawTechnique {
                    start_frame: technique.start_frame,
                    end_frame: technique.end_frame,
                    phoneme_id: technique.phoneme_id,
                    raw_logits: technique.raw_logits,
                    source_local_scores: technique.source_local_scores,
                })
                .collect()
        }),
        styles: result.styles.map(|styles| {
            styles
                .into_iter()
                .map(|style| uta_ggml_runtime::stars::GlobalStyle {
                    start_frame: style.start_frame,
                    end_frame: style.end_frame,
                    logits: style.logits,
                })
                .collect()
        }),
    }
}

fn rosvot_result(result: native::rosvot::RosvotResult) -> uta_ggml_runtime::rosvot::RosvotResult {
    uta_ggml_runtime::rosvot::RosvotResult {
        valid_frames: result.valid_frames,
        note_boundary_logits: result.note_boundary_logits,
        regulated_note_boundaries: result.regulated_note_boundaries,
        notes: result
            .notes
            .into_iter()
            .map(|note| uta_ggml_runtime::rosvot::RawNote {
                start_frame: note.start_frame,
                end_frame: note.end_frame,
                pitch_logits: note.pitch_logits,
                midi: note.midi,
            })
            .collect(),
    }
}

fn alignment(
    aligned: native::qwen::aligner::Alignment,
) -> uta_ggml_runtime::qwen::aligner::Alignment {
    uta_ggml_runtime::qwen::aligner::Alignment {
        words: aligned
            .words
            .into_iter()
            .map(|word| uta_ggml_runtime::qwen::aligner::AlignedWord {
                text: word.text,
                start_seconds: word.start_seconds,
                end_seconds: word.end_seconds,
                timing_issue: word.timing_issue,
            })
            .collect(),
        raw_classes: aligned.raw_classes,
        raw_timestamp_ms: aligned.raw_timestamp_ms,
        corrected_timestamp_ms: aligned.corrected_timestamp_ms,
        windows: aligned
            .windows
            .into_iter()
            .map(
                |window| uta_ggml_runtime::qwen::aligner::AlignmentWindowTrace {
                    start_micros: window.start_micros,
                    end_micros: window.end_micros,
                    first_word: window.first_word,
                    word_count: window.word_count,
                    anchored: window.anchored,
                    raw_timestamp_ms: window.raw_timestamp_ms,
                    corrected_timestamp_ms: window.corrected_timestamp_ms,
                    timing_issues: window.timing_issues,
                },
            )
            .collect(),
        prompt_tokens: aligned.prompt_tokens,
        encoder_seconds: aligned.encoder_seconds,
        decoder_seconds: aligned.decoder_seconds,
    }
}

fn qwen_transcription(
    transcription: native::qwen::asr::Transcription,
) -> uta_ggml_runtime::qwen::asr::Transcription {
    uta_ggml_runtime::qwen::asr::Transcription {
        text: transcription.text,
        segments: transcription
            .segments
            .into_iter()
            .map(|segment| uta_ggml_runtime::qwen::asr::TranscriptSegment {
                start_sample: segment.start_sample,
                end_sample: segment.end_sample,
                text_start: segment.text_start,
                text_end: segment.text_end,
            })
            .collect(),
        language_name: transcription.language_name,
        raw_text: transcription.raw_text,
        generated_tokens: transcription.generated_tokens,
        finished: transcription.finished,
        unfinished_windows: transcription.unfinished_windows,
        prompt_tokens: transcription.prompt_tokens,
        encoder_seconds: transcription.encoder_seconds,
        decoder_seconds: transcription.decoder_seconds,
    }
}

fn firered_transcription(
    transcription: native::firered::Transcription,
) -> uta_ggml_runtime::firered::Transcription {
    uta_ggml_runtime::firered::Transcription {
        text: transcription.text,
        token_ids: transcription.token_ids,
        windows: transcription
            .windows
            .into_iter()
            .map(|window| uta_ggml_runtime::firered::TranscriptWindow {
                index: window.index,
                start_sample: window.start_sample,
                end_sample: window.end_sample,
                text: window.text,
                token_ids: window.token_ids,
            })
            .collect(),
        unfinished_windows: transcription.unfinished_windows,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_the_explicit_backend_selects_the_native_route() {
        assert!(selected(&serde_json::json!({"backend": "libtorch_xpu"})));
        assert!(!selected(&serde_json::json!({"backend": "ggml_vulkan"})));
        assert!(!selected(&serde_json::json!({})));
    }

    #[test]
    fn the_discrete_gpu_is_the_only_device_class_and_defaults_to_device_zero() {
        assert_eq!(device_index(&serde_json::json!({})).unwrap(), 0);
        assert_eq!(
            device_index(&serde_json::json!({"device_class": "gpu", "xpu_device": 1})).unwrap(),
            1
        );
        for class in ["cpu", "integrated_gpu"] {
            assert!(device_index(&serde_json::json!({"device_class": class})).is_err());
        }
    }

    #[test]
    fn mixed_attention_is_disabled_pending_an_fp32_accumulating_fix() {
        assert_eq!(
            precision("melband_roformer_denoise_aufr33"),
            Precision::Strict
        );
        assert_eq!(precision("bs_roformer_leap_xe90_vocals"), Precision::Strict);
        assert_eq!(precision("melband_roformer_harmony"), Precision::Strict);
        assert_eq!(precision("rmvpe"), Precision::Strict);
        assert_eq!(precision("qwen3_asr_1_7b"), Precision::Strict);
    }

    #[test]
    fn a_missing_runtime_directory_is_an_error_not_a_fallback() {
        let root = std::env::temp_dir().join(format!(
            "uta-libtorch-missing-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        // SAFETY: test-local override; no other test in this binary reads it
        // concurrently for a conflicting value.
        unsafe {
            std::env::set_var("UTA_STUDIO_LIBTORCH_RUNTIME_DIR", &root);
        }
        let error = Runtime::locate("rmvpe").unwrap_err();
        assert!(error.contains("LibTorch XPU runtime directory"), "{error}");
        std::fs::create_dir_all(root.join("lib")).unwrap();
        std::fs::write(
            root.join("runtime-manifest.json"),
            br#"{"backend":"libtorch_rocm","native_library":"lib/libuta_libtorch.so"}"#,
        )
        .unwrap();
        std::fs::write(root.join("lib/libuta_libtorch.so"), b"elf").unwrap();
        let error = Runtime::locate("rmvpe").unwrap_err();
        assert!(error.contains("no backend fallback"), "{error}");
        std::fs::write(
            root.join("runtime-manifest.json"),
            br#"{"backend":"libtorch_xpu","native_library":"lib/libuta_libtorch.so","environment":{"ONEAPI_DEVICE_SELECTOR":"level_zero:gpu"}}"#,
        )
        .unwrap();
        let runtime = Runtime::locate("rmvpe").unwrap();
        assert_eq!(runtime.manifest_content_digest().len(), 64);
        assert_eq!(
            runtime
                .environment
                .get("ONEAPI_DEVICE_SELECTOR")
                .map(String::as_str),
            Some("level_zero:gpu")
        );
        unsafe {
            std::env::remove_var("UTA_STUDIO_LIBTORCH_RUNTIME_DIR");
        }
        let _ = std::fs::remove_dir_all(root);
    }
}
