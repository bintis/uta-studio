# Qwen model and runtime research

> **Historical research (2026-09-08).** Qwen is not a current Catalog resource or executable capability. Its incomplete Rust/C++ workers and runtime recipes were removed; transcription and forced-alignment requests now return `MissingCapability`. The provenance and measurements below describe deleted candidate implementations only.

Qwen ASR and Qwen Forced Aligner are separate model/runtime contracts. A pass
for one is not evidence for the other. Source IDs refer to `SOURCE_LEDGER.md`.
No Qwen worker or Vulkan runtime was executed for this research.

## `qwen3_asr_1_7b` — baseline transcription

### Exact identity

| Field | Collected result |
| --- | --- |
| Canonical source model | `Qwen/Qwen3-ASR-1.7B`, revision `7278e1e70fe206f11671096ffdd38061171dd6e5` [Q1][Q2]. |
| Model license | Apache-2.0. Commercial use and redistribution are permitted subject to Apache notice/license requirements; no model-specific non-commercial term was found [Q1]. |
| Official source weights | Two BF16 Safetensors shards, 4,698,521,512 bytes total; SHA-256 `a4cd1f1a…` and `6e0b9d9e…`. |
| Uta-selected converted artifact | Local conversion from that exact official revision: `Qwen3-ASR-1.7B-F16.gguf`. No third-party converted model artifact is used. |
| GGUF size/hash | 4,083,087,904 bytes; SHA-256 `b20587d247ae3d3b2e82f111944b7f3cb98a2a272068bf64580c191bf3b8272b`. |
| Runtime | `handy-computer/transcribe.cpp` commit `ea077b87590bcfb090d7c38c03ab36cd1c7005d3`, MIT [Q4]. Runtime lock pins GGML `8c63e70982c95ceb862e3a1073a2c1beef75d60a`. |
| Quantization/format | GGUF F16 for all matrix/embedding/convolution weights; F32 is retained for normalization, bias, and frontend tensors. The conversion patch and exact recipe are vendored. |

### Input, frontend, and output

- Official and transcribe.cpp contracts use 16 kHz mono audio [Q3][Q4][Q5].
  Uta's worker supplies a path to the runtime; compatibility audio creation and
  cleanup are worker responsibilities.
- Source preprocessing is 128-feature log-mel, FFT 400, hop 160 samples
  (10 ms), 30-second/480,000-sample feature windows [Q3]. The selected native
  runtime owns an equivalent frontend; raw model input is not arbitrary PCM
  tensor shape.
- The source architecture is a 24-layer bidirectional audio encoder feeding
  audio tokens into a 28-layer Qwen3 causal LM (hidden 2,048/intermediate
  6,144) [Q1][Q5].
- Source Qwen supports 30 languages plus 22 named Chinese dialects and claims
  speech, singing voice, and songs-with-BGM inputs [Q2]. The selected GGUF card
  records the 30 language codes and auto detection [Q5].
- Output is detected language + transcript text. The selected runtime has no
  timestamps, translation, streaming, VAD, or Forced Aligner head [Q4][Q5].
  Uta correctly routes timing to the separate aligner.
- Official Qwen tooling supports language and context concepts, while the
  pinned transcribe.cpp contract accepts automatic detection only [Q2][Q4].
  Uta! Studio rejects explicit hints before launch and records only the
  runtime-detected language in schema-2 evidence.
- The worker passes `--n-ctx 0` and `--timestamps none`, captures bounded output,
  and emits only text evidence. Exact generation-limit behavior must remain
  pinned: the historical record describes an earlier 256-token full-song
  truncation, while the researched runtime docs describe a newer bounded input
  contract. Current binary/runtime behavior needs exact acceptance evidence,
  not assumptions from another revision.

### Existing evidence and reuse boundary

Prior bounded validation, now summarized in `docs/KEY_CONCLUSIONS.md`, established exact model/runtime/GGML identities, useful short-run Vulkan behavior, and insufficient evidence for full-song singing Production quality. The raw validation journal is intentionally not retained.

Historical bounded runs used the replaced Q4_K_M bytes and included
poor/repetitive Japanese text. They remain scoped protocol/runtime evidence and
must not be presented as an inference test of the new F16 artifact. The current
catalog retains the owner's `ProductionPinned` route policy; broad labeled
singing quality remains an explicit evidence limitation.

### Repository discrepancy audit

| Field | Classification | Detail |
| --- | --- | --- |
| source model repository/revision | MATCH | Catalog and runtime lock expose the exact official Qwen revision [Q1]. |
| official source artifacts | MATCH | Both BF16 shards and the index are separately recorded. |
| converted GGUF identity | MATCH | Local F16 filename, size, format, and conversion provenance are recorded. |
| license | MATCH | Apache-2.0. |
| source format | MATCH | Official BF16 Safetensors are distinct from the locally converted F16 GGUF. |
| acquisition | MATCH | The converted GGUF uses explicit LocalImport; no nonexistent official GGUF URL is advertised. |
| runtime repository/commit | MATCH | Exact [Q4]. |
| runtime GGML commit | MATCH to repository evidence | `8c63e709…`. |
| language API | MATCH | Explicit hints are rejected before launch for this runtime contract. |
| output language evidence | MATCH | Schema-2 evidence records runtime-detected language and rejects conflicts. |
| validation state/evidence | BOUNDED | Route policy is `ProductionPinned`; historical real-inference evidence is scoped to the replaced Q4 bytes, while F16 graph compatibility is structurally verified. |

### Required 20-question status

| # | Status | Answer |
| ---: | --- | --- |
| 1–2 | KNOWN | Exact canonical source and local F16 conversion identity are known. |
| 3–4 | KNOWN | Official source revision, shard filenames, and local GGUF filename are recorded. |
| 5–6 | KNOWN | Apache-2.0; redistribution/commercial use permitted with compliance. |
| 7 | KNOWN | Official shard and locally generated GGUF identities are recorded. |
| 8 | KNOWN | Official BF16 Safetensors; installed F16 GGUF. |
| 9–10 | KNOWN | transcribe.cpp and exact commit/GGML lock known. |
| 11–12 | KNOWN | 16 kHz mono; 128-bin/400-FFT/160-hop audio frontend into audio encoder/LM. |
| 13 | KNOWN | Text + detected language; no timestamps. |
| 14 | KNOWN | Native frontend and model prompt contract are documented. |
| 15 | CONFLICT | Language-prefix parsing/evidence ownership and explicit hint behavior conflict in current worker. |
| 16 | KNOWN | GGUF conversion is pinned by source revision, converter revision, vendored patch, environment, command, and output identity. |
| 17 | KNOWN | Runtime Manager uses explicit LocalImport for the generated F16 GGUF. |
| 18–19 | KNOWN | Exact-hash Uta evidence exists and matches runtime/model identities for bounded runs. |
| 20 | KNOWN | Yes. Current Production justification requires accepted real-singing/full-track quality, limits, cancellation/restart, and safety evidence. Under current architecture this is Vulkan and requires separate user authorization. |

## `qwen3_forced_aligner_0_6b` — baseline forced alignment

### Exact identity

| Field | Collected result |
| --- | --- |
| Canonical source | `Qwen/Qwen3-ForcedAligner-0.6B-hf`, revision `c07281df297b9905d24a508279258cccf987a064` [Q6]. |
| Source filename/hash | `model.safetensors`, 1,835,545,960 bytes, hosting SHA-256 `00568245ceca5af1991d28562a75fe1ddc9bfeb041c27fda66947ea05c47fb86` [Q6]. |
| License | Apache-2.0; redistribution/commercial use permitted with compliance [Q6]. |
| Source architecture | `Qwen3ASRForTokenClassification`: 24-layer 1,024-d audio encoder, 28-layer text body, classifier with 5,000 timestamp classes [Q6][Q9]. |
| Uta GGUF | `Qwen3-ForcedAligner-0.6B-F16.gguf`, F16, 1,842,216,416 bytes; local manifest SHA-256 `c70553d4e363b752db9110bba0a1ef5fb87355cd80e14703c457fbe7f39a936b`. No official upstream GGUF repository/file/hash was found. |
| Runtime | `predict-woo/qwen3-asr.cpp` commit `6dcc586e5073fd6e85ee5728e75f0903d6c70c6c`, MIT [Q9]. Runtime lock pins CPU-reference GGML `9be3133…`, Vulkan override `8c63e709…`, and a hashed GPU-required integration patch. |

### Input, text, and timestamp contract

- Input is 16 kHz mono PCM audio normalized to float samples by the runtime,
  plus a non-empty transcript and optional supported language [Q7][Q8][Q9].
- Frontend: 128 mel features, FFT 400, hop 160, 30-second feature windows;
  timestamp segment is 80 ms [Q8].
- Official model scope is timestamp prediction for arbitrary text units within
  up to five minutes of **speech**, in Chinese, English, Cantonese, French,
  German, Italian, Japanese, Korean, Portuguese, Russian, and Spanish [Q7].
  Singing is not an official source-card input claim; Uta's singing evidence is
  therefore required product-specific validation.
- Runtime tokenization is language-sensitive. Chinese/Japanese are split into
  CJK units; Korean can use a bundled dictionary; other text follows runtime
  tokenizer/word logic [Q9]. Caller text, language code normalization,
  punctuation, Unicode normalization, and unsupported-language behavior must be
  versioned as part of the Uta input contract.
- The classifier produces 80 ms timestamp classes; runtime applies monotonic
  correction and emits ordered `{word,start,end}` entries [Q9].
- Uta's worker additionally merges zero-duration Unicode pieces into adjacent
  measured units without inventing timestamps, rejects invalid/overlapping
  timing, and outputs alignment evidence. That postprocessing is repository
  behavior, not an upstream model guarantee.

### Conversion reproducibility

The local exact GGUF was regenerated from the pinned official HF model. The
vendored converter patch adapts the current flat `model.*` layout and
`score.weight` timestamp classifier to the pinned runtime's tensor names. The
shared recipe records source and converter revisions, converter environment,
command, source artifacts, and output identity. Regeneration produced the same
1,842,216,416-byte GGUF and SHA-256 as the existing accepted F16 artifact.

### Existing evidence and Production claim

The repository records CPU-reference and Vulkan alignment runs, including a
short 12.8-second singing comparison, full-song executions with bad
Whisper-derived text, and a machine-clean worker smoke. The short exact worker
result is real implementation evidence. Full-song quality was explicitly not
accepted because bad transcript input collapsed many intervals.

Runtime Manager currently labels this exact model/runtime `ProductionPinned`.
The current accepted implementation includes deterministic windowed long-input
alignment, versioned tokenizer/text/language profiles, repeat evidence, and a
reproducible converter/import receipt. Broad labeled singing-quality coverage
remains a documented advisory limitation rather than an alternate fallback.

### Repository discrepancy audit

| Field | Classification | Detail |
| --- | --- | --- |
| canonical source repository/revision | MATCH | Exact [Q6]. |
| source filename | MATCH | Catalog records official `model.safetensors` separately from the converted GGUF. |
| source/converted identities | MATCH | Runtime lock and catalog preserve distinct source and generated artifact identities. |
| GGUF repository | NOT APPLICABLE | No official converted repository is claimed; acquisition is LocalImport. |
| license | MATCH | Apache-2.0. |
| runtime/commits/patch | MATCH | Exact runtime and GGML identities recorded. |
| converter patch | MATCH | The flat-layout/classifier F16 patch and shared conversion recipe are vendored and recorded. |
| input normalization/tokenization | MATCH | Text, language, and 80 ms alignment profiles are versioned and fail closed. |
| validation state | BOUNDED | `ProductionPinned` is the owner's route policy; broad quality limits remain visible. |

### Required 20-question status

| # | Status | Answer |
| ---: | --- | --- |
| 1–4 | KNOWN | Exact canonical source, revision, source filename and local GGUF filename known. |
| 5–6 | KNOWN | Apache-2.0 and redistribution/commercial permission with compliance. |
| 7 | KNOWN | Official source and locally regenerated GGUF identities are recorded separately. |
| 8 | KNOWN | Source safetensors; local F16 GGUF. |
| 9–10 | KNOWN | predict-woo runtime and exact commits/patch identities known. |
| 11–13 | KNOWN | 16 kHz mono; audio+text/language token-classification input; ordered unit timestamps at 80 ms classes. |
| 14 | KNOWN | Audio frontend and language-aware tokenization are documented. |
| 15 | KNOWN | Zero-duration merging and caller normalization are versioned. |
| 16 | KNOWN | The exact current-HF-layout converter patch and environment are pinned. |
| 17 | KNOWN | Deterministic regeneration metadata and explicit LocalImport are available. |
| 18–19 | KNOWN | Exact model/runtime evidence exists. |
| 20 | KNOWN | Yes for a defensible Production claim: complete-lyrics singing quality, limits, cancellation/repeat and Vulkan safety. Current runtime requires separately authorized Vulkan. |

## Current migration boundary

Both resources now derive their F16 GGUFs from pinned official BF16 weights with
vendored conversion patches and explicit LocalImport. The remaining migration
work is the real Rust/WGPU model graph (audio frontend/encoder, Qwen decoder,
KV cache, tokenizer, and aligner classifier), followed by model-specific Vulkan
parity, performance, and stability runs. Historical C++/GGML evidence must not
be relabeled as Rust/WGPU evidence.
