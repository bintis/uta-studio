# Native model runtime smoke matrix

Updated: 2026-08-22

Task completion means every model exposed by the current Uta Studio model catalog
or Models & runtime page has a locked native runtime and a real-input smoke.
Models not represented by the current product catalog (future VocalParse and
TechniqueStudent proposals) are outside this matrix until they gain a model ID,
artifact contract, and explicit installation target.

Generic production experts consume OpenVINO IR. The two Qwen models retain
locked GGML/Vulkan recipes. RoFormer currently has only a conservative Vulkan
candidate lane because OpenVINO parity has not been established and its
sustained Vulkan lane has historical machine-level failures; its passing short
smokes do not make it production. CPU and script runtimes are not production
fallbacks.

| Family / model ID | Target runtime | Local source state | Smoke state |
| --- | --- | --- | --- |
| `bs_roformer_vocals_ep317` | locked GGML/Vulkan candidate | versioned local runtime and FP16 GGUF installed | **pass**: fresh process, conservative 12 s real mix, complete WAV |
| `melband_roformer_inst_v2` | locked GGML/Vulkan candidate | versioned local runtime and FP16 GGUF installed | **pass**: fresh process, conservative 12 s real mix, complete WAV |
| `melband_roformer_harmony` | locked GGML/Vulkan candidate | karaoke checkpoint mapped explicitly to catalog harmony role | **pass**: fresh process, conservative 12 s real mix, complete WAV |
| `melband_roformer_denoise_aufr33` | locked GGML/Vulkan candidate | versioned local runtime and FP16 GGUF installed | **pass**: fresh process, conservative 12 s real vocal, complete WAV |
| `melband_roformer_dereverb_anvuew` | locked GGML/Vulkan candidate | versioned local runtime and FP16 GGUF installed | **pass**: fresh process, conservative 12 s real vocal, complete WAV |
| `firered_asr2_aed` | OpenVINO GPU IR | Apache-2.0 split source graphs and 230-frame/decoder smoke IR installed/hash-verified | **pass**: official 2.32 s Chinese fixture, full AED token loop → `你好世界` |
| `qwen3_asr_1_7b` | pinned transcribe.cpp Vulkan | installed, hash-verified native runtime and GGUF | **pass**: NDJSON Worker, 7.224 s Japanese speech, golden text exact |
| `qwen3_forced_aligner_0_6b` | pinned predict-woo Vulkan | installed, hash-verified GPU-required runtime and GGUF | **pass**: NDJSON Worker, 12.8 s singing, 23 ordered boundaries |
| `rmvpe` | OpenVINO GPU bucketed IR v11 | installed and hash-verified | **pass**: 2 s and 12 s 440 Hz |
| `fcpe` | OpenVINO GPU IR | MIT source ONNX and fixed smoke IR installed/hash-verified | **pass**: 2 s 440 Hz, 201 frames, 439.63 Hz mean |
| `game` | OpenVINO GPU IR | **unresolved identity**: final design supplies no repository, paper, checkpoint, license, or hash; public indexes found no corresponding model | **blocked — model replacement/identity decision required** |
| `stars` | OpenVINO GPU IR | official MIT source `f0e43e96` and Chinese checkpoint `9159dd…` installed; 1,354 tensors match strictly | **blocked — official forward contains CPU Viterbi/data-dependent control flow and is not directly ONNX-exportable** |
| `basic_pitch` | OpenVINO GPU IR | Apache-2.0 source ONNX and fixed official-window IR installed/hash-verified | **pass**: 172 frames, finite onset/note/contour activations |

A short smoke is not a ProductionPinned quality or stability promotion. The
registry continues to fail closed until each separate parity/full-song gate is
accepted.
