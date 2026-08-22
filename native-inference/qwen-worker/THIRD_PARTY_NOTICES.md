# Uta Studio Qwen workers — third-party notices

The Uta Studio NDJSON worker source is GPL-3.0-only. It supervises separately
built native components and user-confirmed model installations:

- `handy-computer/transcribe.cpp`, MIT license, for Qwen3-ASR-1.7B;
- `predict-woo/qwen3-asr.cpp`, MIT license, for Qwen3 Forced Aligner;
- GGML, MIT license, with the runtime-lock-pinned Vulkan revision;
- Qwen3-ASR-1.7B and Qwen3 Forced Aligner model artifacts under their upstream
  model license terms.

The Forced Aligner build carries Uta Studio's fail-closed patch requiring a GPU
backend. Full source/runtime/model identities and hashes are recorded in
`native-inference/runtime-lock.json` and installed manifests. Models are not
bundled and are installed only after explicit user confirmation.
