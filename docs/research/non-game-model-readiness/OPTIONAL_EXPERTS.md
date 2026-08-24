# Optional and future non-GAME experts

These resources are not Fast baseline blockers. `analysis-engine/src/planner/plan.rs`
marks FireRed, FCPE, and Basic Pitch optional for non-Fast candidate work;
denoise/dereverb are optional only in Maximum. GAME is excluded from this pack,
and Basic Pitch is never its replacement.

## RoFormer cleanup

Full identities are in `ROFORMER.md`.

| Resource | Planner role | Research disposition |
| --- | --- | --- |
| `melband_roformer_denoise_aufr33` | Maximum-only optional `audio.denoise` when lead analysis is needed | Exact source revision/file/size/hash and local FP16 GGUF exist [R6]. Checkpoint license is missing. Vulkan evidence is graph/schedule-specific candidate evidence only. |
| `melband_roformer_dereverb_anvuew` | Maximum-only optional `audio.dereverb` | Exact source revision/file/size/hash and local FP16 GGUF exist [R7]. Card says GPL-3.0 and documents model-specific artifacts/limitations. Packaging scope and current backend acceptance remain unresolved. |

Neither capability is currently marked implemented in the capability registry,
so resource presence must not be confused with an executable Engine node.

## FireRed ASR2 AED — optional transcript challenger

| Field | Result |
| --- | --- |
| Canonical project | FireRedTeam/FireRedASR2S, Apache-2.0 [O5]. |
| Uta-selected source artifact | Community/secondary int8 ONNX conversion `42ailab/FireRedASR2-AED-ONNX@13f950858934f7b6a0d3ce52bae65af0dc022258`, itself attributed to ManySpeech and FireRedTeam [O6]. |
| Files | `encoder.int8.onnx`, `decoder.int8.onnx`, `ctc.int8.onnx`, `cmvn.ark`, `tokens.txt`. Hosting metadata publishes per-file SHA-256; Uta's local manifest records encoder `0fe4038f…`, decoder `aeef2267…`, and CTC `8881d31c…`. |
| Purpose/languages | AED speech recognition focused on Mandarin, 20+ Chinese dialects/accents, English and code switching; official project includes singing ASR claims [O5]. |
| Input/frontend | Uta worker decodes 16 kHz mono and implements Kaldi-compatible 80-bin fbank plus binary CMVN. |
| Output | Encoder representation + autoregressive AED token text; the split CTC graph can supply timing evidence, but current Uta smoke proves only token loop/text on one fixed fixture. |
| OpenVINO state | Local fixed 230-feature-frame encoder, 58-frame CTC, and decoder cache buckets 0..10 exist and emitted `你好世界` on OpenVINO GPU. Variable-length buckets, confidence/timestamp parity, cancellation and full-song/singing behavior are missing. |
| Catalog gap | Runtime Manager source/license fields are empty and acquisition is `Unavailable`, despite exact local source/IR manifests. It should remain `BenchmarkCandidate`, not a baseline blocker. |

The selected ONNX is not an official FireRedTeam model binary. Its conversion
lineage and per-file hashes must remain explicit in any future recipe [O6].

## FCPE — optional secondary pitch expert

| Field | Result |
| --- | --- |
| Canonical project | `CNChTu/FCPE`, MIT [O1]. |
| Selected artifact | Explicitly unofficial community ONNX export `gzivdo/fcpe-onnx@5800a2b1944967f55bb0bfeb9718cb749f809310` [O2]. |
| Source file | `fcpe.onnx`, 43,612,026 bytes, published SHA-256 `b7e4f3871b10641869b7ac5a2d56ed94deb37552c0336d77e17ad6e66760adf0` [O2]. |
| Input/output | Raw 16 kHz mono float32 `[1,n_samples,1]` → Hz `[1,n_frames,1]`, 160-sample/10 ms hop; 0 means unvoiced, and the export card warns quiet frames may produce NaN [O2]. Official examples use local argmax, threshold 0.006 and default 80–880 Hz bounds [O1]. |
| Local OpenVINO state | Fixed `[1,32000,1]` IR and manifest exist; two-second 440 Hz smoke produced 201 frames/439.63 Hz mean. |
| Missing | Runtime Manager source/license/acquisition metadata, audited variable-length/bucket conversion, NaN/unvoiced contract, real-singing disagreement value, cancellation/repeat/package evidence. |

FCPE remains an optional secondary disagreement expert. It must not replace
RMVPE automatically.

## Basic Pitch — optional onset/note challenger

| Field | Result |
| --- | --- |
| Canonical project | Spotify Basic Pitch, Apache-2.0 [O3]. |
| Selected source | AEmotionStudio mirror of Spotify's ONNX, revision `327fd8ccd2f0bb84cbe56b4a0e9d318398ddf763`, file `nmp.onnx`, 230,444 bytes, published SHA-256 `2c3c1d144bfa61ad236e92e169c13535c880469a12a047d4e73451f2c059a0ec` [O4]. |
| Purpose | Instrument-agnostic polyphonic audio-to-MIDI/note transcription with pitch bends; official project says it works best on one instrument at a time [O3]. |
| Input | Any source rate may be decoded/downmixed, then 22,050 Hz mono model audio. Uta's fixed source/IR window is `[1,43844,1]`. |
| Outputs | Raw onset, note, and contour activations; official postprocessing produces note events and MIDI [O3]. Uta currently emits activation evidence only. |
| Local state | Source ONNX and fixed OpenVINO IR are present; one 172-frame finite-activation smoke exists. |
| Missing | First-party artifact receipt (the selected repo is a mirror), Runtime Manager provenance/acquisition, exact window stitching and official postprocessing parity, singing/onset quality, cancellation/repeat/package evidence. |

Basic Pitch is only `notes.secondary`. It is **never** evidence for or a
substitute for the required primary note/boundary capability excluded from this
research.

## STARS — experimental/future

| Field | Result |
| --- | --- |
| Canonical project | `gwx314/STARS`, commit `f0e43e96cfe953f71a6cf9efd8b908b2c9d7e167`; source code MIT [O7]. |
| Model purpose | Unified singing transcription, phoneme/word alignment, phoneme-level technique prediction, and global style classification [O7]. |
| Checkpoint | Project-linked `verstar/STARS@744a7ad02e1d788452293cd903ea6a933f7862c4`; Chinese checkpoint `stars_chinese/model_ckpt_steps_200000.ckpt`, 601,773,408 bytes, published SHA-256 `9159dd37516918448b0815ed86e1e3976d39c3044117da78db0ef65d1941db3c` [O8]. The checkpoint repo has no model-card license metadata; code MIT must not automatically be asserted for weights. |
| Input/contract | Prefers isolated vocals and requires text/phoneme arrays and mappings; timing may be supplied or predicted [O7]. This is not a drop-in audio-only technique classifier. |
| Current repository role | Runtime Manager publishes `model:stars` as a native `BenchmarkCandidate` with `notes.stars` and `technique.analyze`. The Engine and Studio retain technique/style as non-authoritative read-only evidence. |
| Current feasibility | Non-monolithic Stage A–E conversion, native Viterbi/phoneme aggregation, shared 24 kHz frontend, exact annotation-RMVPE adapter and versioned native Chinese G2P are integrated without product-time Python. CPU and bounded Intel GPU parity pass for P1. |
| Remaining decision/gates | Broad labeled P1 quality and exact checkpoint license identity remain before Production promotion; integration is complete. |

## ROSVOT and capability placeholders

ROSVOT has a canonical official project, `RickyL-2000/ROSVOT`, MIT code [O9].
It converts singing waveform to MIDI note events, can use or predict word
boundaries, and was designed for noisy/separated or accompanied singing [O9][O10].
The published checkpoint bundle also includes RWBD and RMVPE dependencies and
was trained primarily on Mandarin M4Singer material.

Current Uta! Studio state:

- `notes.rosvot` is an unimplemented optional secondary-expert capability and requires TimedTranscript;
- Runtime Manager has **no** `model:rosvot` resource until real execution exists;
- pinned source `3c8332bf…`, immutable checkpoint/config hashes, safe audit, frame/pitch conversion, shared exact annotation RMVPE, native host algorithms, and a strict typed provenance contract now exist;
- automatic RWBD is excluded from P0, so historical RWBD GPU corruption is not a P0 prerequisite;
- production buckets/long-input semantics, real singing goldens, selected-backend parity, Worker/Runtime/Engine execution, LocalImport generation, and checkpoint license remain.

ROSVOT is selected as an optional secondary singing-note challenger and must not replace GAME.

`technique.analyze` now has an exact STARS P1 owner and is scheduled only when
that exact model is selected. Its nine raw/source-local uncalibrated phoneme
scores and separately scoped global styles remain review evidence, not an
implicit authority over GAME note segmentation or every future technique
capability.

## Optional-resource metadata discrepancy summary

| Resource | Local artifact | Catalog source/license | Acquisition | Validation truth |
| --- | --- | --- | --- | --- |
| FireRed | present | missing | `Unavailable` | fixed OpenVINO smoke only |
| FCPE | present | missing | `Unavailable` | fixed OpenVINO smoke only |
| Basic Pitch | present | missing | `Unavailable` | fixed OpenVINO smoke only |
| STARS | checkpoint/source present | missing | `Unavailable` | conversion blocked |
| Denoise | GGUF present | source pin present; license unresolved | `Unavailable` | Vulkan candidate only |
| Dereverb | GGUF present | source pin present; GPL card needs packaging review | `Unavailable` | graph-specific Vulkan candidate only |

These are catalog-recognition/provenance/implementation gaps, not blanket local
artifact-acquisition gaps.

## Finalization reconciliation — no new model execution

The discrepancy table above is the research-time snapshot. The later optional
experts finalization pass reconciled the current repository without running a
model, compiling a GPU graph, converting an artifact, or creating a GPU context:

| Resource | Runtime Manager closure | Engine boundary | Current policy |
| --- | --- | --- | --- |
| `firered_asr2_aed` | Exact FireRedTeam canonical identity is separate from the selected `42ailab`/ManySpeech split int8 ONNX conversion. The three source graph hashes, fixed-window IR manifest, OpenVINO runtime identity, Apache attribution, and explicit verified `LocalImport` are represented. | Non-Fast plans request it only when transcript evidence is actually needed. It is an optional `speech.transcribe.challenger`; no transcript-consumption/fusion node is claimed. Canonical caller lyrics do not request it. | `BenchmarkCandidate`; Production unusable. |
| `fcpe` | Exact CNChTu canonical identity is separate from the unofficial `gzivdo` ONNX export. Source ONNX, fixed 32,000-sample IR manifest/files, MIT attribution, OpenVINO runtime, and explicit verified `LocalImport` are represented. | Optional `pitch.secondary`; RMVPE remains required primary continuous F0. No Engine consumption node is claimed. | `BenchmarkCandidate`; Production unusable. |
| `basic_pitch` | Exact Spotify canonical identity is separate from the AEmotionStudio ONNX mirror. Source ONNX, fixed 43,844-sample IR manifest/files, Apache attribution, OpenVINO runtime, and explicit verified `LocalImport` are represented. | Optional `notes.basic_pitch`; GAME remains the required primary note expert. Activation evidence cannot satisfy `notes.game`, and no Engine consumption node is claimed. | `BenchmarkCandidate`; Production unusable. |

The historical fixed-window manifests do not identify a reproducible conversion
recipe. Runtime Manager therefore records that field as absent rather than
reusing a source, manifest, XML/BIN, runtime, or catalog recipe hash. Import
verifies the exact manifest and every manifest-declared regular file, stages an
immutable generation, and atomically publishes `current.json` without modifying
the source directory or older generations.

Existing operator success and recorded bounded smoke evidence remain the only
inference evidence. This reconciliation added static metadata/import/planner and
worker-contract checks only; **new inference evidence: none**. Full-song or
variable-window behavior, quality/parity, repeat/cancellation, packaged launch,
and real Engine challenger consumption remain separate model-specific or
algorithmic promotion gates.
