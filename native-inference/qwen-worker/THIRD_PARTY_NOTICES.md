# Uta Studio Qwen workers — third-party notices

The Uta Studio NDJSON worker source is GPL-3.0-only. It supervises separately
built native components and user-confirmed model installations:

- `handy-computer/transcribe.cpp` at
  `ea077b87590bcfb090d7c38c03ab36cd1c7005d3`, MIT license, for Qwen3-ASR-1.7B;
- `predict-woo/qwen3-asr.cpp` at
  `6dcc586e5073fd6e85ee5728e75f0903d6c70c6c`, MIT license, for Qwen3 Forced
  Aligner;
- GGML at `8c63e70982c95ceb862e3a1073a2c1beef75d60a`, MIT license;
- canonical `Qwen/Qwen3-ASR-1.7B` source weights at
  `7278e1e70fe206f11671096ffdd38061171dd6e5`, Apache-2.0;
- converted `handy-computer/Qwen3-ASR-1.7B-gguf` artifact repository at
  `92282af1610a2db19d66f2bef1e260f5deca782d`, derived from the canonical Qwen
  model and distributed under the recorded upstream model terms;
- canonical `Qwen/Qwen3-ForcedAligner-0.6B-hf` source weights at
  `c07281df297b9905d24a508279258cccf987a064`, Apache-2.0;
- the local F16 GGUF derived from that canonical Forced Aligner source, accepted
  only by exact SHA-256 through Runtime Manager LocalImport.

The Forced Aligner build carries Uta Studio's fail-closed patch requiring a GPU
backend. Its conversion contract uses the MIT-licensed predict-woo converter at
the runtime commit plus Uta Studio's vendored flat-HF/classifier adaptation;
both patches and the exact conversion command are hashed or recorded in
`native-inference/runtime-lock.json`. Full canonical-source, converted-artifact,
runtime, and model identities and hashes are recorded in that lock and in
deterministic Runtime Manager install receipts. Models are not bundled and are
installed only after explicit user confirmation.
