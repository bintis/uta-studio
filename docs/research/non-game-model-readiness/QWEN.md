# Qwen model and runtime research

Qwen ASR and Qwen Forced Aligner are separate model/runtime contracts. A pass
for one is not evidence for the other. Source IDs refer to `SOURCE_LEDGER.md`.
No Qwen worker or Vulkan runtime was executed for this research.

## `qwen3_asr_1_7b` — baseline transcription

### Exact identity

| Field | Collected result |
| --- | --- |
| Canonical source model | `Qwen/Qwen3-ASR-1.7B`, revision `7278e1e70fe206f11671096ffdd38061171dd6e5` [Q1][Q2]. |
| Model license | Apache-2.0. Commercial use and redistribution are permitted subject to Apache notice/license requirements; no model-specific non-commercial term was found [Q1]. |
| Uta-selected converted artifact | `handy-computer/Qwen3-ASR-1.7B-gguf`, current researched revision `92282af1610a2db19d66f2bef1e260f5deca782d`, file `Qwen3-ASR-1.7B-Q4_K_M.gguf` [Q5]. |
| GGUF size/hash | 1,319,830,496 bytes; hosting metadata SHA-256 `b7afe3674f653fa84f712ed2440353c6e7cf7f93697fef76b05a26538b24844e` [Q5]. This exactly matches the runtime lock and local bytes. |
| Runtime | `handy-computer/transcribe.cpp` commit `ea077b87590bcfb090d7c38c03ab36cd1c7005d3`, MIT [Q4]. Runtime lock pins GGML `8c63e70982c95ceb862e3a1073a2c1beef75d60a`. |
| Quantization/format | GGUF Q4_K_M. The host repo states it is a quantized conversion of the pinned Qwen source checkpoint and reports English LibriSpeech WER for this quant; that does not prove singing quality [Q5]. |

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
- Official Qwen tooling supports a language argument and context/hotword
  concepts; the pinned transcribe.cpp family documentation says explicit
  language hints are **not supported** and only auto detection is accepted
  [Q2][Q4][Q5]. Uta's current worker nevertheless forwards config `language`
  as `-l` and writes the requested value into evidence rather than parsing the
  detected language. This is a repository/runtime contract conflict requiring
  static correction and later validation.
- The worker passes `--n-ctx 0` and `--timestamps none`, captures bounded output,
  and emits only text evidence. Exact generation-limit behavior must remain
  pinned: the historical record describes an earlier 256-token full-song
  truncation, while the researched runtime docs describe a newer bounded input
  contract. Current binary/runtime behavior needs exact acceptance evidence,
  not assumptions from another revision.

### Existing evidence and reuse boundary

Prior bounded validation, now summarized in `docs/KEY_CONCLUSIONS.md`, established exact model/runtime/GGML identities, useful short-run Vulkan behavior, and insufficient evidence for full-song singing Production quality. The raw validation journal is intentionally not retained.

A later `/tmp/uta-qwen-smoke.C8TlEu` bounded result also used the exact locked
hash but emitted poor/repetitive Japanese text. It is candidate protocol/runtime
evidence only.

The current catalog truthfully keeps ASR at `BenchmarkCandidate`. Existing
Vulkan evidence can be reused only for the exact short recipes recorded; it
cannot justify current Production singing quality.

### Repository discrepancy audit

| Field | Classification | Detail |
| --- | --- | --- |
| source model repository/revision | MATCH in runtime lock | Exact Qwen revision matches [Q1]. Catalog itself exposes only the GGUF repository. |
| GGUF repository/filename/hash/size | MATCH | Exact [Q5]. |
| GGUF repository revision | MISSING IN REPO | Catalog uses no revision; runtime lock also omits the GGUF repo commit. Pin `92282af…` only after confirming that is the intended generation. |
| license | MATCH | Apache-2.0. |
| source format | MATCH | Catalog says GGUF for installed resource; lock separately records source model. |
| acquisition | PARTIAL | ManagedDownload fields exist, but audited URL/revision/atomic receipt details must be confirmed; license notices are informational only in the install implementation. |
| runtime repository/commit | MATCH | Exact [Q4]. |
| runtime GGML commit | MATCH to repository evidence | `8c63e709…`. |
| language API | CONFLICT | Worker can pass `-l`; selected runtime contract says no explicit hints [Q4][Q5]. |
| output language evidence | CONFLICT | Worker records requested language rather than detected runtime language. |
| validation state/evidence | MATCH | BenchmarkCandidate and short validation record; not Production. |

### Required 20-question status

| # | Status | Answer |
| ---: | --- | --- |
| 1–2 | KNOWN | Exact canonical source and converted GGUF repositories known. |
| 3–4 | KNOWN | Source revision and filenames known; GGUF repo revision missing from lock/catalog but collected here. |
| 5–6 | KNOWN | Apache-2.0; redistribution/commercial use permitted with compliance. |
| 7 | KNOWN | GGUF host publishes exact SHA-256. |
| 8 | KNOWN | Source safetensors; installed Q4_K_M GGUF. |
| 9–10 | KNOWN | transcribe.cpp and exact commit/GGML lock known. |
| 11–12 | KNOWN | 16 kHz mono; 128-bin/400-FFT/160-hop audio frontend into audio encoder/LM. |
| 13 | KNOWN | Text + detected language; no timestamps. |
| 14 | KNOWN | Native frontend and model prompt contract are documented. |
| 15 | CONFLICT | Language-prefix parsing/evidence ownership and explicit hint behavior conflict in current worker. |
| 16 | KNOWN | GGUF conversion/reproduction is documented by converted-artifact/runtime source [Q4][Q5]. |
| 17 | CONFLICT | Artifact acquisition can be deterministic after pinning GGUF revision, but current worker language/output contract and Production recipe are not complete. |
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
| Uta GGUF | `qwen3-forced-aligner-predict-woo-f16.gguf`, F16, 1,842,216,416 bytes; local manifest SHA-256 `c70553d4e363b752db9110bba0a1ef5fb87355cd80e14703c457fbe7f39a936b`. No official upstream GGUF repository/file/hash was found. |
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

The local exact GGUF was produced from the pinned HF model and is hash-recorded. Prior converter work established that the tested runtime converter expected an older `thinker.*` layout and required local adaptation for the current flat HF layout and classifier `score.weight`. Reproducibility therefore depends on the source-controlled converter contract/patch and expected output identity in current source, not on a deleted validation journal.

Therefore the exact current recipe is **not reproducible from recorded metadata
alone** on a clean host. A later source-controlled converter patch/command and
expected output hash are mandatory. A clean installation can import the exact
local GGUF today, but cannot independently regenerate it from only the lock.

### Existing evidence and Production claim

The repository records CPU-reference and Vulkan alignment runs, including a
short 12.8-second singing comparison, full-song executions with bad
Whisper-derived text, and a machine-clean worker smoke. The short exact worker
result is real implementation evidence. Full-song quality was explicitly not
accepted because bad transcript input collapsed many intervals.

Runtime Manager currently labels this exact model/runtime `ProductionPinned`.
The collected material supports a pinned candidate implementation, but does not
support a complete Production singing-quality claim without:

- correct full lyrics and language for a complete-song golden;
- deterministic tokenizer/normalization acceptance;
- repeat/cancellation/restart/package evidence;
- a reproducible converter/import receipt;
- safety evidence for the selected Vulkan runtime.

Under the current no-Vulkan policy, it should remain blocked rather than gain
new evidence from this task.

### Repository discrepancy audit

| Field | Classification | Detail |
| --- | --- | --- |
| canonical source repository/revision | MATCH | Exact [Q6]. |
| source filename | CONFLICT | Catalog points to HF source repo but names a local GGUF that does not exist there; source file is `model.safetensors`. |
| source/converted hashes | CONFLICT | Catalog `source.sha256` is local GGUF hash, not source-model hash. Lock does not record source safetensors hash. |
| GGUF repository | MISSING IN REPO / MISSING UPSTREAM | No published GGUF repository selected; clean acquisition is LocalImport. |
| license | MATCH | Apache-2.0. |
| runtime/commits/patch | MATCH | Exact runtime and GGML identities recorded. |
| converter patch | MISSING IN REPO LOCK | Critical flat-layout/classifier mapping is not vendored/hashed. |
| input normalization/tokenization | MISSING IN REPO CONTRACT | Worker forwards text/language, but accepted normalization profile is not versioned. |
| validation state | CONFLICT WITH EVIDENCE SCOPE | ProductionPinned exceeds the short/quality-limited evidence recorded. |

### Required 20-question status

| # | Status | Answer |
| ---: | --- | --- |
| 1–4 | KNOWN | Exact canonical source, revision, source filename and local GGUF filename known. |
| 5–6 | KNOWN | Apache-2.0 and redistribution/commercial permission with compliance. |
| 7 | CONFLICT | Source safetensors and local GGUF hashes are known; no upstream-published hash/source exists for the exact GGUF. |
| 8 | KNOWN | Source safetensors; local F16 GGUF. |
| 9–10 | KNOWN | predict-woo runtime and exact commits/patch identities known. |
| 11–13 | KNOWN | 16 kHz mono; audio+text/language token-classification input; ordered unit timestamps at 80 ms classes. |
| 14 | KNOWN | Audio frontend and language-aware tokenization are documented. |
| 15 | CONFLICT | Runtime/Uta zero-duration merging is known, but a versioned caller normalization contract is missing. |
| 16 | CONFLICT | Upstream converter exists, but exact current HF-layout adaptation is local/unpinned. |
| 17 | MISSING | Clean deterministic regeneration/acquisition cannot be authored from lock metadata alone. Exact LocalImport can be authored if the external GGUF is supplied. |
| 18–19 | KNOWN | Exact model/runtime evidence exists. |
| 20 | KNOWN | Yes for a defensible Production claim: complete-lyrics singing quality, limits, cancellation/repeat and Vulkan safety. Current runtime requires separately authorized Vulkan. |

## What can proceed without Vulkan

The following later work needs no inference:

- pin the ASR GGUF repository revision and complete managed-download receipts;
- separate source safetensors identity from converted-GGUF identity for the
  aligner;
- vendor/hash the exact aligner converter adaptation;
- version language/text normalization and evidence language ownership;
- create deterministic local-import/acquisition plans and license notices;
- statically reconcile worker CLI arguments with pinned runtime docs.

Neither Qwen resource can be newly justified as Production from research alone.
Both current execution paths are Vulkan-only and need separately authorized
future Vulkan evidence for unresolved execution/quality gates.
