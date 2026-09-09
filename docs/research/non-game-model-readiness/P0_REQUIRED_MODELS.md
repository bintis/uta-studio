# P0 required-resource matrix

> **Historical research matrix (2026-09-08).** The current product model set is defined in `tasks/remaining-models/STATE.md`. Qwen and other unported experts below are no longer Catalog resources or executable routes; historical readiness observations do not override current `MissingCapability` behavior.

“Baseline required” follows current planner code, not catalog bundle names.
Local presence is reconciled separately from clean-install acquisition.

| Resource | Capability | Baseline required? | Source identity | License | Published hash | Acquisition info | Runtime info | Existing evidence | Remaining gap |
| --- | --- | --- | --- | --- | --- | --- | --- | --- | --- |
| `bs_roformer_vocals_ep317` | `audio.extract_vocals` | **Normal Full Candidate from original mix**; omitted for supplied lead/vocal sources | UVR BS-Roformer-Viperx-1297; `model_bs_roformer_ep_317_sdr_12.9755.ckpt`; release `all_public_uvr_models` [R1][R2] | **MISSING** for checkpoint | **MISSING upstream**; Uta records source `5b84f37e…`; local GGUF `8dc288b3…` | Local source-derived GGUF exists; immutable release-asset pin and audited conversion/install receipt missing | BSRoformer.cpp/GGML FP16 GGUF, Vulkan candidate; no OpenVINO parity | Exact bounded/full historical records, but host failures and candidate-only verdict | Legal clearance, immutable source pin, converter/runtime recipe, quality/repeat/cancel/safety |
| `melband_roformer_harmony` | `audio.lead_isolate` | **Normal Full Candidate unless primary source is already Lead/CleanLead** | UVR Karaoke MelBand RoFormer aufr33+viperx; `mel_band_roformer_karaoke_aufr33_viperx_sdr_10.1956.ckpt` [R1][R2] | **MISSING** | **MISSING upstream**; Uta source `1de20d45…`; local GGUF `d463c06a…` | Source checkpoint/config and GGUF exist; audited recipe absent | Same Vulkan candidate runtime | Exact short candidate and historical graph evidence | Above gaps plus authoritative LeadVocal/VocalResidual semantics and validity after prior vocal extraction; it does not implement future `audio.lead_partition` |
| `qwen3_asr_1_7b` | `speech.transcribe` | **Required when lyrics are generated**; canonical caller lyrics can skip ASR | Official Qwen BF16 shards at `7278e1e…`; locally converted `Qwen3-ASR-1.7B-F16.gguf` [Q1][Q4] | Apache-2.0 | Source shards `a4cd1f1a…`, `6e0b9d9e…`; local F16 GGUF `b20587d2…` | Exact LocalImport artifact and vendored conversion recipe | transcribe.cpp `ea077b8…`, GGML `8c63e709…`, Vulkan | Historical smoke used replaced Q4 bytes; F16 graph structure verified | Fresh F16 real-inference/performance evidence; full-song generation/quality/limits/cancel/repeat/safety |
| `qwen3_forced_aligner_0_6b` | `speech.align` | **Normal Candidate alignment**; also canonical-lyrics alignment | Official Qwen BF16 `model.safetensors` at `c07281d…`; locally converted F16 GGUF [Q6][Q9] | Apache-2.0 | Source `00568245…`; local F16 GGUF `c70553d4…` | Exact LocalImport bytes and vendored current-layout conversion recipe | predict-woo `6dcc586…`; pinned GGML override/patch; Vulkan | Regenerated F16 is byte-identical to accepted artifact | Broad labeled full-song quality and package acceptance |
| `rmvpe` | `pitch.track` | **Normal Candidate chart and any requested pitch evidence** | Exact lj1995 `rmvpe.onnx` at HF revision `e6d0c1a…`; algorithm source Dream-High [M1][M3] | Exact ONNX lineage MIT; official code Apache-2.0 | **KNOWN:** `5370e71a…` | Source ONNX, static and current bucketed IR all exist; explicit source acquisition → conversion → LocalImport is documentable | OpenVINO 2026.3.0 commit `8a17657…`; GPU-only bucketed IR, no production CPU fallback | Exact 2 s/12 s OpenVINO smokes and manifests | Catalog provenance/license field repair; real singing golden, V/UV, full-song repeat, cancel/coordinator/package acceptance |
| `melband_roformer_inst_v2` | `audio.extract_instrumental` | **Only when instrumental output is explicitly requested**; not normal Full Candidate | `pcunwa/Mel-Band-Roformer-Inst@f86cd9e…`, exact V2 checkpoint [R5] | **MISSING** | **KNOWN:** source `bd197666…`; local GGUF `e2b39b97…` | Source metadata and local GGUF exist; audited converter/install/runtime receipt missing | BSRoformer.cpp/GGML Vulkan candidate | Short and one historical full-track candidate pass | License, exact catalog revision, recipe and Production quality/repeat/cancel/safety |

## Route distinctions

- Required for a typical generated-lyrics Full Candidate from an original mix:
  EP317, Karaoke/“harmony”, Qwen ASR, Qwen Forced Aligner, and RMVPE.
- Required only for requested output: Inst V2.
- Canonical lyrics remove the ASR requirement but not alignment when alignment or
  Candidate chart output is requested.
- A supplied clean lead can remove both RoFormer preparation requirements.
- FireRed, FCPE, and Basic Pitch are optional challengers for non-Fast profiles.
- Denoise/dereverb are Maximum-only optional cleanup.
- GAME excluded from this research pack by task scope.

No row is newly Production-ready. `LOCAL_ARTIFACT_PRESENT` does not imply that
Runtime Manager has a deterministic clean-install recipe or accepted execution
evidence.
