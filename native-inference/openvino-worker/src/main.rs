mod advanced_notes;
mod audio;
mod basic_pitch;
mod fcpe;
mod firered;
mod game;
mod jbm555;
mod kaldi_fbank;
mod mel;
mod melband_roformer_denoise;
mod melband_roformer_harmony;
mod melband_roformer_harmony_split;
mod melband_roformer_inst_v2;
mod polarformer;
mod protocol;
mod rmvpe;
mod rosvot_host;
mod runtime;
mod singing_frontend;
mod stars_g2p;
mod stars_viterbi;

use std::io::BufRead;
use std::path::Path;

use protocol::{COMPONENT_RECIPE, PROTOCOL_VERSION, WorkerCommand, WorkerFrame, emit};

fn run_task(
    task_id: &str,
    model_id: &str,
    input_artifacts: &[std::path::PathBuf],
    output_dir: &Path,
    config: &serde_json::Value,
) -> Result<(), String> {
    if roformer_is_ggml_only(model_id) {
        return Err(format!(
            "{model_id} is pinned to the GGML/Vulkan Worker and cannot run through OpenVINO"
        ));
    }
    if !output_dir.is_dir() {
        return Err("authorized task output directory is unavailable".to_string());
    }
    let source = input_artifacts
        .first()
        .ok_or_else(|| "native OpenVINO task has no input audio artifact".to_string())?;
    emit(WorkerFrame::Progress {
        task_id,
        fraction: 0.01,
        message: "Decoding source audio to the model sample rate",
        work_units_completed: None,
        work_units_total: None,
    })?;
    if matches!(model_id, "stars" | "rosvot") {
        let audio_24k = audio::decode_mono(source, output_dir, 24_000)?;
        let audio_16k = audio::decode_mono(source, output_dir, 16_000)?;
        let output = advanced_notes::infer(
            model_id,
            &audio_24k,
            &audio_16k,
            output_dir,
            config,
            |fraction, message, work_units| {
                let _ = emit(WorkerFrame::Progress {
                    task_id,
                    fraction: 0.02 + fraction * 0.97,
                    message,
                    work_units_completed: work_units.map(|(completed, _)| completed),
                    work_units_total: work_units.map(|(_, total)| total),
                });
            },
        )?;
        emit(WorkerFrame::Output {
            task_id,
            artifact: "advanced_note_evidence",
            path: &output,
            media_type: if model_id == "stars" {
                "application/vnd.uta.advanced-note-evidence+json;version=2"
            } else {
                "application/vnd.uta.advanced-note-evidence+json;version=1"
            },
        })?;
        return emit(WorkerFrame::Done {
            task_id,
            status: "ok",
        });
    }
    if model_id == "jbm555_cectc_80" {
        if input_artifacts.len() != 2 {
            return Err(
                "JBM555 requires exactly two artifacts: original mix and prepared vocal"
                    .to_string(),
            );
        }
        let mix = audio::decode_mono(&input_artifacts[0], output_dir, 44_100)?;
        let vocal = audio::decode_mono(&input_artifacts[1], output_dir, 44_100)?;
        let output = jbm555::infer(&mix, &vocal, output_dir, config, |fraction, message| {
            let _ = emit(WorkerFrame::Progress {
                task_id,
                fraction: 0.02 + fraction * 0.97,
                message,
                work_units_completed: None,
                work_units_total: None,
            });
        })?;
        emit(WorkerFrame::Output {
            task_id,
            artifact: "jbm555_note_evidence",
            path: &output,
            media_type: "application/vnd.uta.jbm555-note-evidence+json;version=1",
        })?;
        return emit(WorkerFrame::Done {
            task_id,
            status: "ok",
        });
    }
    let audio = if matches!(
        model_id,
        "melband_roformer_denoise_aufr33"
            | "melband_roformer_dereverb_anvuew"
            | "melband_roformer_harmony"
            | "melband_roformer_inst_v2"
            | "bs_polarformer_public_instrumental"
    ) {
        audio::decode_stereo(source, output_dir)?
    } else {
        let sample_rate = match model_id {
            "basic_pitch" => 22_050,
            "game" => 44_100,
            _ => 16_000,
        };
        audio::decode_mono(source, output_dir, sample_rate)?
    };
    if model_id == "melband_roformer_harmony" {
        let (lead, residual) =
            melband_roformer_harmony::infer(&audio, output_dir, config, |fraction, message| {
                let _ = emit(WorkerFrame::Progress {
                    task_id,
                    fraction: 0.02 + fraction * 0.97,
                    message,
                    work_units_completed: None,
                    work_units_total: None,
                });
            })?;
        for (artifact, path) in [("lead_vocal", lead), ("vocal_residual", residual)] {
            emit(WorkerFrame::Output {
                task_id,
                artifact,
                path: &path,
                media_type: "audio/flac",
            })?;
        }
        return emit(WorkerFrame::Done {
            task_id,
            status: "ok",
        });
    }
    let output = match model_id {
        "rmvpe" => rmvpe::infer(
            &audio,
            output_dir,
            config,
            |fraction, message, work_units| {
                let _ = emit(WorkerFrame::Progress {
                    task_id,
                    fraction: 0.02 + fraction * 0.97,
                    message,
                    work_units_completed: work_units.map(|(completed, _)| completed),
                    work_units_total: work_units.map(|(_, total)| total),
                });
            },
        )?,
        "fcpe" => fcpe::infer(
            &audio,
            output_dir,
            config,
            |fraction, message, (completed, total)| {
                let _ = emit(WorkerFrame::Progress {
                    task_id,
                    fraction: 0.02 + fraction * 0.97,
                    message,
                    work_units_completed: Some(completed),
                    work_units_total: Some(total),
                });
            },
        )?,
        "basic_pitch" => basic_pitch::infer(
            &audio,
            output_dir,
            config,
            |fraction, message, (completed, total)| {
                let _ = emit(WorkerFrame::Progress {
                    task_id,
                    fraction: 0.02 + fraction * 0.97,
                    message,
                    work_units_completed: Some(completed),
                    work_units_total: Some(total),
                });
            },
        )?,
        "firered_asr2_aed" => firered::infer(&audio, output_dir, config)?,
        "game" => game::infer(
            &audio,
            output_dir,
            config,
            |fraction, message, work_units| {
                let _ = emit(WorkerFrame::Progress {
                    task_id,
                    fraction: 0.02 + fraction * 0.97,
                    message,
                    work_units_completed: work_units.map(|(completed, _)| completed),
                    work_units_total: work_units.map(|(_, total)| total),
                });
            },
        )?,
        "melband_roformer_denoise_aufr33" | "melband_roformer_dereverb_anvuew" => {
            melband_roformer_denoise::infer(
                model_id,
                &audio,
                output_dir,
                config,
                |fraction, message| {
                    let _ = emit(WorkerFrame::Progress {
                        task_id,
                        fraction: 0.02 + fraction * 0.97,
                        message,
                        work_units_completed: None,
                        work_units_total: None,
                    });
                },
            )?
        }
        "melband_roformer_inst_v2" => {
            melband_roformer_inst_v2::infer(&audio, output_dir, config, |fraction, message| {
                let _ = emit(WorkerFrame::Progress {
                    task_id,
                    fraction: 0.02 + fraction * 0.97,
                    message,
                    work_units_completed: None,
                    work_units_total: None,
                });
            })?
        }
        "bs_polarformer_public_instrumental" => {
            polarformer::infer(&audio, output_dir, config, |fraction, message| {
                let _ = emit(WorkerFrame::Progress {
                    task_id,
                    fraction: 0.02 + fraction * 0.97,
                    message,
                    work_units_completed: None,
                    work_units_total: None,
                });
            })?
        }
        _ => {
            return Err(format!(
                "model {model_id} is not implemented by this OpenVINO worker"
            ));
        }
    };
    emit(WorkerFrame::Output {
        task_id,
        artifact: match model_id {
            "basic_pitch" => "basic_pitch_evidence",
            "firered_asr2_aed" => "transcript_evidence",
            "game" => "note_candidate_evidence",
            "melband_roformer_denoise_aufr33" => "clean_lead_vocal",
            "melband_roformer_dereverb_anvuew" => "dereverbed_vocal",
            "melband_roformer_inst_v2" => "instrumental",
            "bs_polarformer_public_instrumental" => "instrumental",
            _ => "pitch_evidence",
        },
        path: &output,
        media_type: if matches!(
            model_id,
            "melband_roformer_denoise_aufr33"
                | "melband_roformer_dereverb_anvuew"
                | "melband_roformer_harmony"
                | "melband_roformer_inst_v2"
                | "bs_polarformer_public_instrumental"
        ) {
            "audio/flac"
        } else {
            "application/json"
        },
    })?;
    emit(WorkerFrame::Done {
        task_id,
        status: "ok",
    })
}

fn roformer_is_ggml_only(model_id: &str) -> bool {
    matches!(
        model_id,
        "melband_roformer_inst_v2"
            | "melband_roformer_harmony"
            | "melband_roformer_denoise_aufr33"
            | "melband_roformer_dereverb_anvuew"
    )
}

fn main() {
    runtime::configure_process_environment();
    if !std::env::args().any(|argument| argument == "--stdio-json") {
        eprintln!("uta-openvino-worker requires --stdio-json");
        std::process::exit(2);
    }
    if let Err(error) = protocol::isolate_native_stdout() {
        eprintln!("{error}");
        std::process::exit(3);
    }
    if emit(WorkerFrame::Ready {
        protocol: PROTOCOL_VERSION,
        component: "uta-openvino-worker",
        runtime_recipe_digest: COMPONENT_RECIPE,
    })
    .is_err()
    {
        std::process::exit(3);
    }

    for line in std::io::stdin().lock().lines() {
        let line = match line {
            Ok(line) if !line.trim().is_empty() => line,
            Ok(_) => continue,
            Err(error) => {
                eprintln!("OpenVINO worker stdin failed: {error}");
                break;
            }
        };
        let command = match serde_json::from_str::<WorkerCommand>(&line) {
            Ok(command) => command,
            Err(error) => {
                let message = error.to_string();
                let _ = emit(WorkerFrame::Error {
                    task_id: None,
                    code: "invalid_command",
                    message: &message,
                    retryable: false,
                });
                continue;
            }
        };
        if protocol::command_protocol(&command) != PROTOCOL_VERSION {
            let _ = emit(WorkerFrame::Error {
                task_id: None,
                code: "unsupported_protocol",
                message: "unsupported native worker protocol",
                retryable: false,
            });
            continue;
        }
        match command {
            WorkerCommand::Quit { .. } => break,
            WorkerCommand::Cancel { task_id, .. } => {
                let _ = emit(WorkerFrame::Error {
                    task_id: Some(&task_id),
                    code: "cancelled",
                    message: "task cancelled before execution",
                    retryable: false,
                });
            }
            WorkerCommand::Run {
                task_id,
                node_id,
                model_id,
                input_artifacts,
                output_dir,
                config,
                ..
            } => {
                eprintln!("[uta-openvino-worker] node={node_id} model={model_id}");
                if let Err(message) =
                    run_task(&task_id, &model_id, &input_artifacts, &output_dir, &config)
                {
                    let _ = emit(WorkerFrame::Error {
                        task_id: Some(&task_id),
                        code: "native_inference_failed",
                        message: &message,
                        retryable: false,
                    });
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_roformer_is_rejected_by_the_openvino_worker() {
        for model_id in [
            "melband_roformer_inst_v2",
            "melband_roformer_harmony",
            "melband_roformer_denoise_aufr33",
            "melband_roformer_dereverb_anvuew",
        ] {
            assert!(roformer_is_ggml_only(model_id));
        }
        assert!(!roformer_is_ggml_only("rmvpe"));
    }
}
