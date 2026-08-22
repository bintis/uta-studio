# Qwen runtime and repository validation record

## Native Worker update (2026-08-22)

Uta Studio now exposes separate `uta-qwen-asr-worker` and
`uta-qwen-align-worker` stdio NDJSON components. Both verify installed engine,
GGML, model, source revision, component recipe, and binary-manifest identities
before inference. Child output is captured away from protocol stdout and child
processes receive a Linux parent-death signal. The Aligner source carries a
pinned fail-closed patch: `QWEN_REQUIRE_GPU=1` rejects an unavailable GPU rather
than retaining upstream's CPU-only scheduler.

Fresh Worker smokes on Intel Arc B580 passed:

- ASR: 7.224-second Japanese PCM, `Vulkan0`, exact golden text
  `うちの中学は弁当制で、持っていけない場合は50円の学校販売のパンを買う。`;
- Forced Aligner: 12.8-second Japanese singing fixture, `Vulkan`, 23 ordered
  character boundaries from 0.56 to 12.16 seconds.

Both workers emitted machine-clean ready/progress/output/done frames, exited on
`quit`, removed compatibility WAVs, and left no engine process. These remain
short runtime smokes, not full-song singing-quality acceptance.

Date: 2026-08-22

Status: `Qwen3-ForcedAligner-0.6B` has completed real-audio CPU and Intel Arc
Vulkan inference through the `predict-woo/qwen3-asr.cpp` model graph.
`Qwen3-ASR-1.7B` has completed real-audio Intel Arc Vulkan transcription through
`handy-computer/transcribe.cpp`. The same local ASR weights were then repacked
without requantization into `predict-woo`'s GGUF naming contract, allowing one
`predict-woo` process to load ASR 1.7B and Forced Aligner 0.6B and complete its
combined transcribe-and-align mode. The unified runtime works functionally, but
the real-song ASR/alignment quality is not accepted. Neither runtime is
integrated into the Uta Studio analysis DAG yet.

This record distinguishes the two Qwen models. They have different heads,
contracts, repositories, and intended product roles; a successful ASR run does
not prove Forced Aligner support, and vice versa.

## Runtime assignment summary

| Model | Product role | Runtime repository | Runtime revision | Backend tested | Current decision |
| --- | --- | --- | --- | --- | --- |
| Qwen3-ForcedAligner-0.6B | Given audio and lyrics, produce character/word boundaries | [`predict-woo/qwen3-asr.cpp`](https://github.com/predict-woo/qwen3-asr.cpp) | `6dcc586e5073fd6e85ee5728e75f0903d6c70c6c` | GGML CPU and GGML Vulkan | Production candidate: Vulkan primary, CPU fallback |
| Qwen3-ASR-1.7B | Generate transcript text from audio | [`handy-computer/transcribe.cpp`](https://github.com/handy-computer/transcribe.cpp) | `ea077b87590bcfb090d7c38c03ab36cd1c7005d3` (`0.2.1`) | GGML Vulkan | Original local GGUF runtime; singing quality and generation-cap behavior are not accepted |
| Qwen3-ASR-1.7B + Forced Aligner 0.6B | Transcribe and immediately align in one process | [`predict-woo/qwen3-asr.cpp`](https://github.com/predict-woo/qwen3-asr.cpp) | `6dcc586e5073fd6e85ee5728e75f0903d6c70c6c` | GGML Vulkan | Unified candidate runs both local models; full-song quality failed |

`transcribe.cpp` does not currently implement the Forced Aligner's 5,000-class
timestamp head. Its own family note explicitly tracks
`qwen3-forced-aligner-0.6b` outside the current Qwen3-ASR port. The Forced
Aligner GGUF therefore must not be routed to `transcribe.cpp` merely because
both models share a Qwen3 audio encoder and language-model body.

## Qwen3-ForcedAligner-0.6B

### Source and artifact identity

| Item | Value |
| --- | --- |
| Runtime | `predict-woo/qwen3-asr.cpp` |
| Runtime revision | `6dcc586e5073fd6e85ee5728e75f0903d6c70c6c` |
| Runtime license | MIT |
| Runtime's pinned GGML revision | `9be313313c8ecb9488911bd64550190e3ed80f38` (`0.17.0`) |
| Source model | [`Qwen/Qwen3-ForcedAligner-0.6B-hf`](https://huggingface.co/Qwen/Qwen3-ForcedAligner-0.6B-hf) |
| Source model revision | `c07281df297b9905d24a508279258cccf987a064` |
| Model license | Apache-2.0 |
| Model contract | `Qwen3ASRForTokenClassification`; 24 audio layers, 28 text layers, 5,000 timestamp classes |
| Test GGUF | F16, 708 tensors, 1,842,216,416 bytes |
| Test GGUF SHA-256 | `c70553d4e363b752db9110bba0a1ef5fb87355cd80e14703c457fbe7f39a936b` |

The Hugging Face checkpoint uses the current flat Transformers layout, while
the runtime converter at the tested revision expects the older `thinker.*`
layout. The temporary conversion adaptation made these contract changes:

- recognize `Qwen3ASRForTokenClassification` and its flat configuration;
- map `model.audio_tower.*`, `model.multi_modal_projector.*`, and
  `model.language_model.*`;
- map the classifier `score.weight` to the runtime tensor `output.weight`;
- read vocabulary, merges, and added tokens from `tokenizer.json`;
- preserve all 708 tensors, with no skipped weight.

The `score.weight` mapping is mandatory. Falling back to the tied ASR language-
model head would silently change a timestamp classifier into a transcript
generator and is not a valid conversion.

### CPU result

The CPU binary used the runtime's pinned GGML revision `9be3133` and eight host
threads.

| Input | Audio duration | Mel | Audio encoder | Text decoder | Runtime total | Process wall | Result |
| --- | ---: | ---: | ---: | ---: | ---: | ---: | --- |
| Real Japanese singing excerpt | 12.800 s | 33 ms | 653 ms | 721 ms | 1.407 s | not separately recorded | 23 characters; monotonic boundaries |
| Full EP317 vocal stem plus Whisper-derived text | 305.813 s | 793 ms | 37.693 s | 150.589 s | 189.077 s | 190.259 s | 387 characters; runtime completed |

For the 12.8-second excerpt, grouping the character output back into the 17
reference units gave a 207 ms mean boundary difference and 1.04 s maximum
boundary difference against the PyTorch reference. Twenty-three of 34 grouped
start/end boundaries were exact.

The full-song result is performance evidence only. Its Whisper-derived text
contains omissions, recognition errors, and fabricated
`ご視聴ありがとうございました` text. The alignment consequently collapsed many
characters to zero duration and ended at 153.04 seconds instead of covering the
305.813-second song. It is not a quality acceptance result.

### Vulkan result

The Forced Aligner graph and CLI remained from `predict-woo/qwen3-asr.cpp`, but
the successful Vulkan binary linked the compatible GGML `0.20.2` Vulkan build
at revision `8c63e70982c95ceb862e3a1073a2c1beef75d60a`. The host's available
CMake and `glslc` store binaries were truncated during the experiment, so the
runtime's pinned `9be3133` Vulkan backend could not be built. This compatibility
build proves the graph/backend path, but production must select, build, and pin
one reproducible GGML revision rather than silently mixing revisions.

The run selected the discrete GPU with:

```text
QWEN_USE_VRAM=1
GGML_VK_VISIBLE_DEVICES=0
Intel(R) Arc(tm) B580 Graphics (BMG G21)
```

| Input | Audio duration | Mel | Audio encoder | Text decoder | Runtime total | Process wall | Result |
| --- | ---: | ---: | ---: | ---: | ---: | ---: | --- |
| Real Japanese singing excerpt | 12.800 s | 33 ms | 194 ms | 191 ms | 418 ms | 1.727 s | 23 characters; CPU and Vulkan boundaries identical 46/46 |
| Full EP317 vocal stem plus Whisper-derived text | 305.813 s | 806 ms | 1.164 s | 2.899 s | 4.872 s | 6.188 s | Runtime completed; 387 characters |
| Original full-song mix plus the same text | 305.813 s | 976 ms | 1.180 s | 3.241 s | 5.400 s | 6.921 s | Runtime completed; 387 characters |

The short Vulkan result exactly matched every CPU start/end boundary. On the
full vocal-stem run, 715 of 774 CPU/Vulkan boundaries were exact, mean absolute
boundary difference was 76.9 ms, and the maximum difference was 3.76 s. Both
full-song runs inherited the invalid transcript problem. The original-mix run
was worse: 355 of 387 units had zero duration and the last boundary was 144.08
seconds. These are not quality passes.

No boot change, GPU hang, reset, fault, OOM, or panic was observed during the
short or full Vulkan runs. This is real whole-song runtime evidence, not a
long-run reliability campaign.

### Other Forced Aligner backends

| Backend | Result | Production status |
| --- | --- | --- |
| Direct OpenVINO fixed-input FP32 IR | Arc B580 forward completed in 86 ms; GPU/PyTorch logits max difference `2.6226044e-05` | Capability proof only; transcript, tokens, and masks were fixed during conversion |
| Direct OpenVINO general four-input graph | GPU compile failed on a dynamic `NonZero` shape with `to_shape was called on a dynamic shape` | Unsupported |
| GGML OpenVINO, host-pointer weights | Segfaulted during model load | Unsupported |
| GGML OpenVINO with `QWEN_USE_VRAM=1` | Model loaded; compute failed because an OpenVINO topology contained duplicate `result: (permuted)` primitive IDs | Unsupported |
| GGML SYCL/XPU | Matching `icpx`/`dpcpp` compiler was unavailable; no compiler or package was installed for the test | Not tested |

### Forced Aligner production gate

The current candidate assignment is GGML Vulkan primary with GGML CPU fallback.
Before integration it still requires:

1. a reproducible, single GGML revision and normal build;
2. the adapted converter or an audited upstream equivalent under source control;
3. a correct complete-lyrics whole-song quality test;
4. bounded process/API inputs, cancellation, and error mapping;
5. packaging and explicit installation through **Settings > Models & runtime**;
6. integration into the local process command boundary and analysis DAG.

## Qwen3-ASR-1.7B

### Source and artifact identity

| Item | Value |
| --- | --- |
| Runtime | [`handy-computer/transcribe.cpp`](https://github.com/handy-computer/transcribe.cpp) |
| Runtime revision | `ea077b87590bcfb090d7c38c03ab36cd1c7005d3` (`0.2.1`) |
| Runtime license | MIT |
| GGML revision | `8c63e70982c95ceb862e3a1073a2c1beef75d60a` (`0.20.2`) |
| Original model | [`Qwen/Qwen3-ASR-1.7B`](https://huggingface.co/Qwen/Qwen3-ASR-1.7B) |
| Original model revision | `7278e1e70fe206f11671096ffdd38061171dd6e5` |
| Model license | Apache-2.0 |
| Test GGUF repository | [`handy-computer/Qwen3-ASR-1.7B-gguf`](https://huggingface.co/handy-computer/Qwen3-ASR-1.7B-gguf) |
| Test GGUF | `Qwen3-ASR-1.7B-Q4_K_M.gguf`, 1,319,830,496 bytes |
| Test GGUF SHA-256 | `b7afe3674f653fa84f712ed2440353c6e7cf7f93697fef76b05a26538b24844e` |
| Model contract | Transcript generation with automatic language detection; no timestamps or forced alignment |

### Vulkan result

The runtime enumerated the Arc B580 as `Vulkan0`, device index 0, with 11.93
GiB total memory. Tests explicitly selected `--backend vulkan --device 0` and
used an 8,192-token context, reported as approximately 896 MiB maximum KV
storage.

| Input | Duration | Runtime inference | Process wall | Result |
| --- | ---: | ---: | ---: | --- |
| Repository Japanese speech sample | 7.224 s | 5.667 s | 7.800 s | Correct Japanese text; detected `ja` |
| Full EP317 vocal stem | 305.813 s | 6.732 s | 8.292 s | Runtime completed after local generation-budget override; detected `zh`; singing transcript poor |

At the tested revision, Qwen3-ASR hard-codes a 256-token per-run generation
budget. The unmodified full-song run reached that budget, returned the explicit
truncation status, and produced an incomplete transcript. A temporary test-only
change raised the budget to 1,024 tokens; that run reached EOS and produced the
whole runtime result above. The source clone was restored afterward. This is an
upstream contract issue, not an accepted Uta Studio patch.

The full-song performance result is not a quality pass. The model misclassified
the Japanese singing as Chinese and produced substantially incorrect lyrics.
No GPU hang, reset, fault, or boot change occurred.

### ASR production decision

`transcribe.cpp` is a credible native Vulkan ASR runtime and has a documented,
validated Qwen3-ASR implementation. The tested 1.7B model is nevertheless not
part of the current production model matrix because:

- the product already targets Whisper for transcription;
- Japanese singing quality failed this real-song test;
- the default 256-token generation budget truncates this whole-song workload;
- it provides no timestamps and cannot load the Forced Aligner head;
- no Uta Studio command API, package, or DAG integration exists yet.

It remains an evaluated alternative, not a replacement for the separate
Forced Aligner runtime by itself.

## Unified `predict-woo` ASR + Forced Aligner test

### Upstream support evidence

The upstream README's supported-model table is stale and lists only ASR 0.6B,
but the implementation has supported ASR 1.7B since merged pull request
[`#3`](https://github.com/predict-woo/qwen3-asr.cpp/pull/3), merge commit
`c4939cd33a59ac6ea5bfc0c6f7f6280d38d55dc6`. The current repository contains:

- `tests/qwen3asr_test.cpp`, which loads ASR 0.6B, ASR 1.7B, and Forced Aligner
  0.6B in one executable;
- CLI `--transcribe-align` mode, which loads ASR and aligner models together,
  transcribes 30-second chunks with a 28-second stride, then aligns each
  transcript;
- a successful pull-request Actions run covering Linux x64 Vulkan, Linux ARM64
  Vulkan, Windows Vulkan, CPU, and macOS:
  <https://github.com/Jaffe2718/qwen3-asr.cpp/actions/runs/23350485999>.

This makes `predict-woo/qwen3-asr.cpp` the only evaluated native GGML/Vulkan
candidate that can own both exact product roles in one process.

### Local model compatibility repack

No additional model was downloaded for this test. The local ASR artifact was
the already-tested `handy-computer/transcribe.cpp` GGUF:

```text
Qwen3-ASR-1.7B-Q4_K_M.gguf
SHA-256: b7afe3674f653fa84f712ed2440353c6e7cf7f93697fef76b05a26538b24844e
```

Its contract uses `general.architecture=qwen3_asr`, `stt.qwen3_asr.*` metadata,
and `enc.*`/`dec.*` tensor names. `predict-woo` expects
`general.architecture=qwen3-asr`, `qwen3-asr.*` metadata, and
`audio.encoder.*`/`blk.*` tensor names. Passing the original file directly made
`predict-woo` fall back to 0.6B dimensions and segfault during CPU inference;
this is an explicit compatibility failure, not a valid model probe.

A temporary offline repack translated metadata and tensor names only. It copied
all 707 model tensors with their original Q4_K_M/Q6_K/Q8_0/F32 quantized bytes,
omitted only the two `transcribe.cpp`-specific frontend constants because
`predict-woo` computes its own mel frontend, and performed no dequantization or
requantization. The compatibility artifact was:

```text
Qwen3-ASR-1.7B-Q4_K_M-predict-woo-compat.gguf
SHA-256: 16bd17fe0adcf9ce09b09494d41c0e81fbf24398445587fe16ea728b19011c17
```

The repack is development evidence, not a distributable model. Production
should implement an audited runtime compatibility layer or source-controlled
conversion contract rather than silently rewriting installed user artifacts.

### Real inference results

The CPU and Vulkan ASR probes used the same 7.224-second Japanese speech sample.
Both produced the correct sentence. CPU process wall time was 1.669 seconds;
Vulkan process wall time was 2.092 seconds including model loading. The Vulkan
output was:

```text
うちの中学は弁当制で、持っていけない場合は、五十円の学校販売のパンを買う。
```

The combined Vulkan process then loaded that local ASR artifact and the local
F16 Forced Aligner together with `QWEN_USE_VRAM=1` and
`GGML_VK_VISIBLE_DEVICES=0`:

| Input | ASR | Alignment | Runtime total | Process wall | Output |
| --- | ---: | ---: | ---: | ---: | --- |
| 7.224 s Japanese speech | 308 ms | 79 ms | 387 ms | 2.692 s | 37 monotonic Japanese character boundaries |
| 305.813 s original song mix, `--language japanese` | 33.268 s | 4.480 s | 37.748 s | 42.859 s | 1,361 units; runtime completed, quality failed |

The CLI accepted `--language japanese`, but upstream ASR prompt construction
currently ends with `(void)language` and does not encode the requested language
into the assistant prefix. The option therefore selected Japanese splitting for
the aligner but did not constrain ASR language generation. The logged English
ASR detection is consistent with this implementation defect.

The whole-song run proves that both local models fit and execute consecutively
through one Arc B580 Vulkan process. It does not prove useful analysis quality:
1,323 of 1,361 units contained ASCII English, 1,339 units had zero duration,
five of eleven chunks returned empty transcripts, and the last output boundary
was 224.0 seconds. The model repeatedly generated English phrases while its
language option was being ignored by ASR. The combined pipeline then correctly tried
to align that invalid ASR text, so its timestamps were unusable.

The test boot did not change and no GPU hang, reset, fault, OOM, or panic was
recorded. The kernel did record the earlier CPU segfault from deliberately
passing the incompatible original GGUF directly; that event preceded the
successful repacked CPU/Vulkan and combined tests.

### Unified runtime decision

One runtime can therefore execute both models, but it is not production-ready:

- **Runtime consolidation:** pass — one `predict-woo` process loads both models.
- **Local-weight reuse:** pass with a temporary metadata/name repack; no model
  download or requantization occurred.
- **Japanese speech smoke:** pass.
- **Whole-song execution and GPU stability:** pass for this run.
- **Whole-song transcription/alignment quality:** fail.
- **Installed-model contract and packaging:** not implemented.

Do not remove `transcribe.cpp` or declare the unified runtime selected solely on
this result. First determine whether the poor song output is caused by
`predict-woo` generation/prompt handling, the local Q4_K_M compatibility
contract, or the ASR model's singing behavior, using reference transcripts and
an accuracy GGUF.

## Final repository rule

Do not refer to a generic “Qwen runtime” in implementation or manifests. Record
both the model role and runtime lineage explicitly:

```text
forced alignment:
  predict-woo/qwen3-asr.cpp + Qwen/Qwen3-ForcedAligner-0.6B-hf

original local ASR artifact/runtime:
  handy-computer/transcribe.cpp + handy-computer/Qwen3-ASR-1.7B-gguf

unified development candidate:
  predict-woo/qwen3-asr.cpp + local ASR compatibility contract + local Forced Aligner
```

A future package manifest must additionally pin the exact GGML revision,
artifact SHA-256, backend flags, and model license. Runtime or model acquisition
must occur only after explicit user confirmation in **Settings > Models &
runtime**; diagnostics and application startup must not download either model.
