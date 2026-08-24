# RoFormer provenance and runtime research

Source IDs refer to `SOURCE_LEDGER.md`. Machine-local inventory snapshots are intentionally not retained; current artifact/runtime identity must be read from Runtime Manager manifests/catalog and current source. No model was run or converted for this research document.

## Shared runtime and conversion identity

The historical in-repository RoFormer path is based on BSRoformer.cpp commit
`a7b9625f0f4146cacf3c46080d1139833cd4d4c2` and GGML commit
`8c63e70982c95ceb862e3a1073a2c1beef75d60a`. BSRoformer.cpp is MIT and supports
BS-RoFormer and MelBand-RoFormer GGUF [R3], but runtime licensing does **not**
license checkpoint weights.

Current accepted integrations for EP317, Karaoke lead isolation, Inst V2,
Denoise and Dereverb use model-specific manifest-pinned OpenVINO IR routes.
Their converters, device placement and Runtime Manager identities are current
source authority; old GGUF/Vulkan artifacts are historical inputs, not an
automatic fallback. Every accepted route remains `BenchmarkCandidate` unless
the catalog explicitly says otherwise.

Common source configs use 44,100 Hz stereo, FFT/window 2,048, hop 441, periodic
Hann/reflect-centered STFT behavior, mask application, ISTFT and chunk
overlap-add [R2][R3]. Exact chunk size, overlap, gathered-band layout and model
topology remain model-specific and must not be generalized.

Historical Vulkan runs and rejected OpenVINO schedules include whole-host
lock/power-loss failures. Safety evidence is graph- and schedule-specific; a
short success never authorizes a monolithic graph or repeated compile lifecycle
for long audio.

## P0 — `bs_roformer_vocals_ep317`

### Identity and contract

| Field | Result |
| --- | --- |
| Canonical catalog identity | UVR catalog “BS-Roformer-Viperx-1297”; exact checkpoint `model_bs_roformer_ep_317_sdr_12.9755.ckpt` [R1][R2]. “EP317” is the filename epoch marker; the catalog display value includes the reported SDR. |
| Author/owner | Checkpoint catalog credits Viperx; hosted by TRvlvr's UVR public catalog [R1][R2]. A separate author-owned immutable model repository was not found. |
| Release/revision | Release tag `all_public_uvr_models`; exact immutable checkpoint commit/revision is **MISSING** because the release asset is mutable independently of the Git tag [R1]. |
| Source size/hash | GitHub asset size 639,331,213 bytes. GitHub publishes no digest. Uta! Studio records local/source SHA-256 `5b84f37e8d444c8cb30c79d77f613a41c05868ff9c9ac6c7049c00aefae115aa`; that is repository evidence, not a hash published by source [R1]. |
| License | **MISSING.** `TRvlvr/model_repo` has no detected license and the release body supplies none. Redistribution/commercial use therefore cannot be approved from authoritative material. Runtime MIT does not cure this. |
| Architecture | BS-RoFormer, dim 512, depth 12, one target stem, stereo, 159.8M-class parameter graph in local conversion evidence [R2]. |
| Input/preprocessing | 44.1 kHz stereo; chunk 352,800 samples (~8 s); FFT/window 2,048; hop 441; reflect-centered STFT; target config's low-level silence threshold `0.001` [R2][R3]. |
| Overlap | Config `num_overlap=4`, batch 1 [R2]. Changing overlap is a quality-affecting parameter, not a transparent optimization [R3]. |
| Output semantics | Target instrument `Vocals`; one predicted vocal stem. Uta maps it to `GuideVocals` for `audio.extract_vocals`. Instrumental/residual semantics are not part of the current one-file native worker output contract. |
| Local converted artifact | `model-fp16.gguf`, 320,092,800 bytes; installed manifest SHA-256 `8dc288b3…`, backend `vulkan_candidate`. |
| Evidence | Historical bounded Vulkan successes did not establish Production/general stability. The durable current interpretation is summarized in `docs/KEY_CONCLUSIONS.md`; current source/tests decide present readiness. |

### Repository discrepancy audit

| Repository field | Classification | Detail |
| --- | --- | --- |
| source URL | MATCH | Points to TRvlvr release catalog. |
| revision | REPO PLACEHOLDER | `all_public_uvr_models` identifies a release, not immutable asset bytes. |
| filename | MATCH | Exact. |
| source SHA-256 | MISSING UPSTREAM | Repository has a value; GitHub source publishes no digest. |
| estimated size | CONFLICT | Catalog uses 639,000,000; source metadata says 639,331,213. |
| license | REPO PLACEHOLDER | `review_recorded_user_download` is workflow state, not legal terms. |
| source format | MATCH | PyTorch `.ckpt`. |
| acquisition | CONFLICT WITH LOCAL STATE | `Unavailable` truthfully means no audited recipe, but the host already has source-derived GGUF. |
| runtime/backend | MATCH | GGML/Vulkan candidate implementation. No OpenVINO parity. |
| runtime commit/digest | MISSING IN REPO CATALOG | Historical record supplies source commits; model entry has no runtime recipe digest. |
| validation evidence | MISSING IN ENTRY | Relevant validation docs exist, but model backend has no evidence ID and remains candidate. |

### P0 questions

| # | Status | Answer |
| ---: | --- | --- |
| 1–2 | KNOWN | Exact UVR catalog identity/release host is known [R1][R2]. |
| 3 | CONFLICT | Release tag known; immutable model-asset revision missing. |
| 4 | KNOWN | Exact checkpoint filename known. |
| 5–6 | MISSING | Checkpoint license and redistribution permission not published. |
| 7 | MISSING | No upstream-published checkpoint hash. |
| 8 | KNOWN | PyTorch checkpoint + YAML source; local FP16 GGUF. |
| 9–10 | KNOWN | BSRoformer.cpp/GGML identities known from repository evidence, but absent from catalog recipe fields. |
| 11–15 | KNOWN | 44.1 kHz stereo/config/STFT/chunk/overlap and vocal-target semantics are known; product residual handling is not used. |
| 16 | KNOWN | Upstream converter route exists [R3]; Uta's deterministic packaging wrapper does not. |
| 17 | MISSING | Legal clearance, immutable source pin, converter dependencies, output digest, atomic install and runtime recipe are incomplete. |
| 18 | KNOWN | Validation records and `/tmp` logs located. |
| 19 | KNOWN | Existing manifests/logs identify this exact GGUF/model graph, while old XPU evidence is a different obsolete backend. |
| 20 | KNOWN | Yes. Current Production justification requires new graph-specific safety/quality evidence; under current worker that would require a separately authorized Vulkan lane, or a newly implemented/validated OpenVINO route. |

## P0 — `melband_roformer_harmony`

### Identity and lead/residual semantic contract

| Field | Result |
| --- | --- |
| Canonical source identity | UVR “Karaoke MelBand Roformer (aufr33 & viperx)”; checkpoint `mel_band_roformer_karaoke_aufr33_viperx_sdr_10.1956.ckpt` [R1][R2]. There is no authoritative upstream model named `melband_roformer_harmony`. |
| Release/revision | `all_public_uvr_models`; immutable asset revision missing. |
| Source size/hash | 913,096,801 bytes; source publishes no digest. Uta records SHA-256 `1de20d459332fe8869aeb01327a31df0032262706e1365114e852dc271779813`. |
| License | **MISSING**; no source license/redistribution grant found. |
| Architecture/input | MelBand-RoFormer, dim 384, depth 6, 60 bands, one target stem; 44.1 kHz stereo; chunk 352,800; FFT/window 2,048; hop 441; overlap 4; batch 1 [R2]. |
| Source target naming | Config names instruments `Vocals` and `Instrumental` and target `Vocals` [R2]. UVR's upstream vocal-split contract explicitly says Karaoke models remove lead vocals, maps a Karaoke `Vocals` primary to `lead_only`, and labels its complement `backing_only` in that workflow [R10]. |
| Product mapping | Uta maps the neural target to `LeadVocal`. The exact deterministic complement is `vocal_residual = all_vocals - lead_vocal`; framework authority intentionally does not relabel that residual as a pure Backing/Harmony stem. |
| Prior separator dependency | Planner feeds EP317's all-vocals output into this stage for original mixes. This matches UVR's documented vocal-split workflow, which auto-processes a generated vocal stem with a Karaoke model [R10]. Direct original-mix input is rejected by the Worker semantic contract. |
| Backing/harmony output | The Worker emits typed `lead_vocal` and `vocal_residual`; Engine validates both but only publishes the requested lead in v1. Pure backing/harmony export remains the separate future `audio.lead_partition` capability. |
| Accepted artifact | OpenVINO schema-2 split generation `ed73768dd348d6357423adf4486007f65f67dc2b3c46dd816d21b13e8dea0590`: 21 ordered islands and 42 hash-pinned XML/BIN files. The source checkpoint/config and historical FP16 GGUF remain separate identities. |

The former output-interpretation blocker is resolved by the UVR vocal-split
contract rather than by filename convention alone. The neural output is lead;
the complement is a deterministic vocal residual and must not be presented as
a pure backing or harmony stem without a separate partition capability.

### Accepted OpenVINO topology and safety schedule

The exact dim-384/depth-6 model is split into CPU BandSplit, six time and six
frequency Transformer GPU islands, and eight bounded CPU Mask groups. Time
attention keeps all 801 frames while microbatching only ten independent bands;
frequency attention keeps all 60 bands while microbatching only 64 independent
frames. Product split parity against the source graph has relative L2
`2.182252899307499e-6`.

Long audio uses stage-major rolling residency. Native DSP first prepares every
chunk's gathered tensor on the host. Each GPU island is then compiled once,
run over every chunk, and released before the next island is compiled. CPU Mask
inference and overlap-add reconstruction follow. This preserves exact attention
context while reducing a seven-chunk task from 84 GPU compilations to 12.

The monolithic GPU graph is rejected: a 12-second run coincided with a hard
restart and had no clean shutdown, OOM, panic or GPU-reset record. The first
split schedule, which recompiled all 12 GPU islands for each audio chunk, also
coincided with a hard restart near chunk five of seven. Neither path may be
retried or selected by the product. The accepted stage-major schedule completed
two byte-identical 12-second runs, with stereo/timeline preservation, no overlap
seam anomaly and complement max error `1.1920928955078125e-7`. The real Engine
published the identical lead FLAC, removed the residual task directory, and
passed supervised cancellation followed by restart in the same Engine process.

### Repository discrepancy audit

| Field | Classification | Detail |
| --- | --- | --- |
| source URL/tag/filename | MATCH | Correct UVR asset identity. |
| SHA-256 | MISSING UPSTREAM | Repository/local hash only. |
| license | REPO PLACEHOLDER | No legal terms established. |
| display/purpose/output semantics | REFINED | UVR defines Karaoke vocal splitting as lead removal. Uta exposes the primary as lead and conservatively names the complement `vocal_residual`. |
| input dependency | MATCH | UVR documents applying Karaoke/BV split models to generated vocal stems; Uta uses EP317 all-vocals output. |
| acquisition | REFINED | Runtime Manager LocalImport accepts only the schema-2 split manifest and verifies every ordered island path, size and hash; the source release still lacks immutable asset identity. |
| runtime/evidence | MATCH | `openvino_2026_3` resolves generation `ed73768d…a0590` under Benchmark policy; Worker and Engine evidence match that exact generation. |

### P0 questions

| # | Status | Answer |
| ---: | --- | --- |
| 1–2 | KNOWN | Exact Karaoke checkpoint is known; UVR's vocal-split contract confirms lead-removal semantics while Uta keeps “harmony” only as the resource alias. |
| 3 | CONFLICT | Release tag known, immutable asset revision missing. |
| 4 | KNOWN | Exact source and local GGUF filenames known. |
| 5–7 | MISSING | Checkpoint license, redistribution permission, and published hash missing. |
| 8–12 | KNOWN | Checkpoint/config/GGUF, runtime, 44.1 kHz stereo and feature contract known. |
| 13 | KNOWN | One `Vocals` target is the lead output under UVR Karaoke semantics; the deterministic complement is Uta's non-overclaimed `vocal_residual`. |
| 14 | KNOWN | STFT/chunk/overlap known. |
| 15 | KNOWN | `vocal_residual = all_vocals - lead_vocal`; pure Backing/Harmony export remains out of scope for `audio.lead_isolate`. |
| 16 | KNOWN | Source-controlled split converter and strict Runtime Manager LocalImport wrapper are implemented. |
| 17 | PARTIAL | Conversion/install/runtime identity is closed; checkpoint license and immutable upstream asset revision remain missing. |
| 18–19 | KNOWN | Exact split generation, parity, long-input Worker and real Engine evidence match. |
| 20 | KNOWN | Integration safety, semantics and cancellation are accepted on the exact OpenVINO schedule. Broader quality/latency, packaging, Benchmark promotion and checkpoint license identity remain Production gates. |

## P0 requested-output resource — `melband_roformer_inst_v2`

This model is required only when an instrumental stem is explicitly requested.
`analysis-engine/src/planner/plan.rs` does not include it in a normal Full
Candidate request without that requested output.

| Field | Result |
| --- | --- |
| Canonical identity | “MelBand Roformer Kim / Inst V2 by Unwa”; `pcunwa/Mel-Band-Roformer-Inst`, immutable HF revision `f86cd9e99d63eb9499b00fca424bc4ed8a8aeaba` [R2][R5]. |
| Filename/size/hash | `melband_roformer_inst_v2.ckpt`; 1,574,477,088 bytes; hosting metadata SHA-256 `bd19766620f7d6f58fdf7aaada7e89907fe41bc64490ce3faa9a6dab15d6e1f2` [R5]. |
| License | **MISSING.** Repository has no model card/license metadata or license file. Redistribution/commercial compatibility is unproven. |
| Architecture/input | MelBand-RoFormer, dim 384, depth 12, 60 bands, one target; 44.1 kHz stereo; chunk 485,100 (~11 s); FFT/window 2,048; hop 441; overlap 2; batch 1 [R2]. |
| Output | Target instrument `Instrumental`; source config lists `Instrumental` then `Vocals`. Current one-stem worker publishes the instrumental target. |
| Local artifact | FP16 GGUF 787,918,656 bytes; manifest SHA-256 `e2b39b97…`. |
| Evidence | Short candidate matrix and one historical full-track pass; not Production quality/stability promotion. |

### Repository discrepancy audit

| Field | Classification | Detail |
| --- | --- | --- |
| repository/filename/source hash/size | MATCH | Hash and exact size match host metadata. |
| revision | REPO PLACEHOLDER | Catalog says `main`; exact source revision is `f86cd9e…`. |
| license | REPO PLACEHOLDER | No upstream grant found. |
| acquisition/local state | CONFLICT WITH LOCAL STATE | Local GGUF exists; audited recipe absent. |
| runtime/evidence | MISSING IN ENTRY | Candidate backend exists but no runtime digest/evidence ID. |

### P0 questions

| # | Status | Answer |
| ---: | --- | --- |
| 1–4 | KNOWN | Exact owner/repo/revision/filename known. |
| 5–6 | MISSING | Checkpoint license and redistribution permission absent. |
| 7–16 | KNOWN | Published hash, source format, runtime identity, 44.1 kHz stereo, feature/pre/post and instrumental semantics, and converter route known. |
| 17 | MISSING | License plus production converter/install/runtime pin and output digest are incomplete. |
| 18–19 | KNOWN | Exact local artifact/evidence located. |
| 20 | KNOWN | Yes for Production; current runtime needs separately authorized graph-specific Vulkan safety/quality validation, or a validated OpenVINO replacement. |

## P1 optional cleanup models

| Resource | Exact source identity | License/hash | Semantics and state |
| --- | --- | --- | --- |
| `melband_roformer_denoise_aufr33` | `poiqazwsx/melband-roformer-denoise@4e39bc34…`, `denoise_mel_band_roformer_aufr33_sdr_27.9959.ckpt`, 913,097,300 bytes [R6] | License **MISSING**; published SHA-256 `7c1c3919…` | Optional Maximum-profile cleanup. Local FP16 GGUF/hash and short/full historical evidence exist, but unsafe async Vulkan and graph-specific conditions prevent Production generalization. |
| `melband_roformer_dereverb_anvuew` | `anvuew/dereverb_mel_band_roformer@cef05ad…`, `dereverb_mel_band_roformer_anvuew_sdr_19.1729.ckpt`, 913,107,578 bytes [R7] | Card says GPL-3.0; published SHA-256 `9262877b…`. Weight-vs-code scope still needs packaging review. | Vocal dereverb. Source warns this checkpoint was trained with an alignment bug and may also remove separation residue/string content and some non-center harmony. Historical exact safe-looking Vulkan schedule is graph-specific and not current authorization. |

Both are optional only for the Maximum profile when lead analysis is needed.
They must not block Fast/Balanced baseline routes.

## Deterministic RoFormer handoff facts

EP317, Karaoke lead isolation and Inst V2 now have model-specific source/config
identity, manifest-pinned split OpenVINO generations, strict Runtime Manager
validation, exact-context parity, typed Worker protocols, long-input evidence,
cancellation/restart and real Engine routes. Denoise and Dereverb have their own
accepted model-specific OpenVINO contracts. No route may substitute another
model's gathered-band layout or output semantics.

These integrations remain non-Production where the catalog says
`BenchmarkCandidate`. Remaining Production work is model-specific but includes
broader representative quality/latency, packaged-runtime acceptance and exact
checkpoint license identity where still unresolved. Historical GGUF availability
or a short accelerator success does not satisfy those gates.
