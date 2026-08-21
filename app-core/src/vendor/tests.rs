mod ready_marker_contract_tests {
    use super::{ready_marker_is_compatible_for, ready_marker_is_usable_for};

    #[test]
    fn v4_intel_marker_still_lets_analysis_run() {
        assert!(ready_marker_is_usable_for("runtime-v4:intel", "intel"));
        assert!(!ready_marker_is_compatible_for("runtime-v4:intel", "intel"));
    }

    #[test]
    fn v5_marker_is_current() {
        assert!(ready_marker_is_compatible_for("runtime-v5:intel", "intel"));
        assert!(ready_marker_is_usable_for("runtime-v5:intel", "intel"));
    }

    #[test]
    fn other_backend_marker_is_not_usable() {
        assert!(!ready_marker_is_usable_for("runtime-v4:cpu", "intel"));
        assert!(!ready_marker_is_usable_for("runtime-v5:cuda", "intel"));
    }
}

mod torch_runtime_probe_tests {
    use super::TORCH_RUNTIME_PROBE;

    #[test]
    fn intel_probe_executes_xpu_without_querying_device_properties() {
        assert!(TORCH_RUNTIME_PROBE.contains("device = \"xpu\""));
        assert!(TORCH_RUNTIME_PROBE.contains("x = torch.arange"));
        assert!(TORCH_RUNTIME_PROBE.contains("y = x @ x"));
        assert!(TORCH_RUNTIME_PROBE.contains("torch.xpu.synchronize()"));
        assert!(!TORCH_RUNTIME_PROBE.contains("torch.xpu.get_device_name"));
        assert!(!TORCH_RUNTIME_PROBE.contains("torch.xpu.get_device_properties"));
    }
}

mod node_model_availability_tests {
    use super::{ModelAvailabilityChecks, node_model_availability_from_checks};
    use crate::analysis_graph::AnalysisNodeId;

    #[test]
    fn separator_and_pitch_map_directly_to_their_own_node() {
        let map = node_model_availability_from_checks(ModelAvailabilityChecks {
            separator_ready: false,
            pitch_ready: true,
            asr_engine: "whisper",
            backend: "cpu",
            primary_asr_model_ready: true,
            language_detector_ready: true,
            align_backend: "whisperx",
            align_model_ready: true,
        });
        assert!(!map[&AnalysisNodeId::new("stems.separate")]);
        assert!(map[&AnalysisNodeId::new("pitch.extract")]);
    }

    #[test]
    fn plain_cpu_whisper_does_not_need_the_language_detector() {
        // Whisper detects language with the same model it transcribes
        // with -- unlike parakeet/intel, which need a separate tiny model
        // first. A missing (false) detector must not block this path.
        let map = node_model_availability_from_checks(ModelAvailabilityChecks {
            separator_ready: true,
            pitch_ready: true,
            asr_engine: "whisper",
            backend: "cpu",
            primary_asr_model_ready: true,
            language_detector_ready: false,
            align_backend: "whisperx",
            align_model_ready: true,
        });
        assert!(map[&AnalysisNodeId::new("lyrics.transcribe")]);
    }

    #[test]
    fn parakeet_requires_both_its_own_model_and_the_language_detector() {
        let map = node_model_availability_from_checks(ModelAvailabilityChecks {
            separator_ready: true,
            pitch_ready: true,
            asr_engine: "parakeet",
            backend: "cuda",
            primary_asr_model_ready: true,
            language_detector_ready: false,
            align_backend: "whisperx",
            align_model_ready: true,
        });
        assert!(!map[&AnalysisNodeId::new("lyrics.transcribe")]);

        let map = node_model_availability_from_checks(ModelAvailabilityChecks {
            separator_ready: true,
            pitch_ready: true,
            asr_engine: "parakeet",
            backend: "cuda",
            primary_asr_model_ready: true,
            language_detector_ready: true,
            align_backend: "whisperx",
            align_model_ready: true,
        });
        assert!(map[&AnalysisNodeId::new("lyrics.transcribe")]);
    }

    #[test]
    fn intel_backend_requires_the_language_detector_regardless_of_asr_engine() {
        let map = node_model_availability_from_checks(ModelAvailabilityChecks {
            separator_ready: true,
            pitch_ready: true,
            asr_engine: "whisper",
            backend: "intel",
            primary_asr_model_ready: true,
            language_detector_ready: false,
            align_backend: "whisperx",
            align_model_ready: true,
        });
        assert!(!map[&AnalysisNodeId::new("lyrics.transcribe")]);
    }

    #[test]
    fn missing_primary_asr_model_blocks_transcription_even_with_a_ready_detector() {
        let map = node_model_availability_from_checks(ModelAvailabilityChecks {
            separator_ready: true,
            pitch_ready: true,
            asr_engine: "parakeet",
            backend: "cuda",
            primary_asr_model_ready: false,
            language_detector_ready: true,
            align_backend: "whisperx",
            align_model_ready: true,
        });
        assert!(!map[&AnalysisNodeId::new("lyrics.transcribe")]);
    }

    #[test]
    fn whisperx_and_ctc_alignment_are_never_blocked_by_a_missing_fixed_model() {
        // Neither backend has one fixed, trackable model -- they resolve a
        // per-language wav2vec2 model on demand, so `align_model_ready` must
        // simply be ignored for them, not used as a gate.
        for backend in ["whisperx", "ctc"] {
            let map = node_model_availability_from_checks(ModelAvailabilityChecks {
                separator_ready: true,
                pitch_ready: true,
                asr_engine: "whisper",
                backend: "cpu",
                primary_asr_model_ready: true,
                language_detector_ready: true,
                align_backend: backend,
                align_model_ready: false,
            });
            assert!(map[&AnalysisNodeId::new("lyrics.align")]);
        }
    }

    #[test]
    fn qwen_and_mms_karaoke_alignment_are_blocked_when_their_model_is_missing() {
        for backend in ["qwen", "mms_karaoke"] {
            let map = node_model_availability_from_checks(ModelAvailabilityChecks {
                separator_ready: true,
                pitch_ready: true,
                asr_engine: "whisper",
                backend: "cpu",
                primary_asr_model_ready: true,
                language_detector_ready: true,
                align_backend: backend,
                align_model_ready: false,
            });
            assert!(!map[&AnalysisNodeId::new("lyrics.align")]);

            let map = node_model_availability_from_checks(ModelAvailabilityChecks {
                separator_ready: true,
                pitch_ready: true,
                asr_engine: "whisper",
                backend: "cpu",
                primary_asr_model_ready: true,
                language_detector_ready: true,
                align_backend: backend,
                align_model_ready: true,
            });
            assert!(map[&AnalysisNodeId::new("lyrics.align")]);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{ComputeBackend, inference_runtime_reinstall_args, onnx_runtime_package};

    #[test]
    fn inference_runtime_reinstall_uses_a_bare_distribution_name() {
        for backend in [
            ComputeBackend::Cpu,
            ComputeBackend::Cuda,
            ComputeBackend::Intel,
        ] {
            let (name, requirement) = onnx_runtime_package(backend);
            let args = inference_runtime_reinstall_args(backend, "/test/python");
            let reinstall_index = args
                .iter()
                .position(|arg| *arg == "--reinstall-package")
                .expect("runtime install must request an explicit reinstall");

            assert_eq!(args[reinstall_index + 1], name);
            assert!(
                !name
                    .chars()
                    .any(|ch| matches!(ch, '<' | '>' | '=' | '!' | '~'))
            );
            assert_eq!(args[reinstall_index + 2], requirement);
            assert!(requirement.starts_with(name));
        }
    }

    #[test]
    fn inference_runtime_packages_match_each_compute_backend() {
        assert_eq!(
            onnx_runtime_package(ComputeBackend::Cpu),
            ("onnxruntime", "onnxruntime>=1.17")
        );
        assert_eq!(
            onnx_runtime_package(ComputeBackend::Cuda),
            ("onnxruntime-gpu", "onnxruntime-gpu>=1.17")
        );
        assert_eq!(
            onnx_runtime_package(ComputeBackend::Intel),
            ("onnxruntime-openvino", "onnxruntime-openvino>=1.17")
        );
    }
}
