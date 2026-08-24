# Source ledger

Retrieved: 2026-08-22. URLs were accessed for text/JSON/HTTP metadata only; no
model weight was downloaded. Hugging Face `x-linked-etag` values below are the
hosted large-file SHA-256 identities returned by the official file endpoint.

## RoFormer

| ID | Source | Publisher | Class | URL | Facts supported |
| --- | --- | --- | --- | --- | --- |
| R1 | `all_public_uvr_models` release | TRvlvr / UVR public catalog | primary catalog | <https://github.com/TRvlvr/model_repo/releases/tag/all_public_uvr_models> | Exact EP317, Karaoke, and Inst V2 filenames and asset sizes. GitHub reports no asset digest for these checkpoints. Repository has no detected license. |
| R2 | Application-data commit `3826b05b…` | TRvlvr | primary catalog/config | <https://github.com/TRvlvr/application_data/commit/3826b05b570dbd4fbedbc807758803b35348ba1b> | Exact config files and download mappings: EP317 is Viperx; Karaoke is aufr33 + viperx; Inst V2 is Unwa. Configs define target stems, 44.1 kHz stereo, STFT, chunk, and overlap. |
| R3 | BSRoformer.cpp commit `a7b9625…` | chenmozhijin | primary runtime | <https://github.com/chenmozhijin/BSRoformer.cpp/commit/a7b9625f0f4146cacf3c46080d1139833cd4d4c2> | MIT C++ GGML runtime/converter; BS- and MelBand-RoFormer support; GGUF conversion; 44.1 kHz input; STFT→network→mask→ISTFT→overlap-add pipeline. |
| R4 | BSRoformer.cpp v0.1.0 release | chenmozhijin | primary runtime release | <https://github.com/chenmozhijin/BSRoformer.cpp/releases/tag/v0.1.0> | Published runtime binary assets/digests. It does not publish Uta Studio's installed GGUFs or checkpoint licenses. |
| R5 | Mel-Band-Roformer-Inst revision `f86cd9e…` | pcunwa | primary checkpoint host | <https://huggingface.co/pcunwa/Mel-Band-Roformer-Inst/tree/f86cd9e99d63eb9499b00fca424bc4ed8a8aeaba> | Exact Inst V2 checkpoint/config presence. File endpoint publishes 1,574,477,088 bytes and SHA-256 `bd197666…`. No model card or license metadata. |
| R6 | MelBand denoise revision `4e39bc3…` | poiqazwsx; credits aufr33 | primary checkpoint host | <https://huggingface.co/poiqazwsx/melband-roformer-denoise/tree/4e39bc34a36dda8e73254cd8f5d44f15de2bd7b9> | Exact denoise filename; 913,097,300 bytes; SHA-256 `7c1c3919…`; no explicit license metadata. |
| R7 | MelBand dereverb revision `cef05ad…` | anvuew | primary checkpoint host/card | <https://huggingface.co/anvuew/dereverb_mel_band_roformer/tree/cef05ad2b5b3145ea5c149d3ad5d1f8439b34d06> | Exact dereverb filename; 913,107,578 bytes; SHA-256 `9262877b…`; card metadata says GPL-3.0 and describes vocal-dereverb limitations/training alignment bug. |
| R8 | BS-RoFormer paper | Lu et al. | primary paper | <https://arxiv.org/abs/2309.02612> | Architecture-level BS-RoFormer claims only; it does not establish the provenance/license of an arbitrary UVR checkpoint. |
| R9 | Mel-Band RoFormer paper | Wang et al. | primary paper | <https://arxiv.org/abs/2310.01809> | Architecture-level MelBand-RoFormer claims only; not binary provenance. |
| R10 | UVR vocal-split contract (`UVR.py`, `gui_data/constants.py`) | Anjok07 / Ultimate Vocal Remover | primary application semantics | <https://github.com/Anjok07/ultimatevocalremovergui/blob/master/gui_data/constants.py#L1058-L1063> | Defines Karaoke vocal splitting as removing lead vocals, labels split outputs lead/backing, and maps a Karaoke model with native `Vocals` primary to `lead_only`; Uta conservatively retains the complement as `vocal_residual` rather than claiming a pure backing/harmony stem. |

## Qwen

| ID | Source | Publisher | Class | URL | Facts supported |
| --- | --- | --- | --- | --- | --- |
| Q1 | Qwen3-ASR-1.7B revision `7278e1e…` | Qwen | primary model | <https://huggingface.co/Qwen/Qwen3-ASR-1.7B/commit/7278e1e70fe206f11671096ffdd38061171dd6e5> | Apache-2.0, architecture/config, 30 languages + 22 Chinese dialects, singing/BGM claim, language hints in official toolkit, source safetensors identity. |
| Q2 | Qwen3-ASR model card at pinned revision | Qwen | primary model card | <https://huggingface.co/Qwen/Qwen3-ASR-1.7B/blob/7278e1e70fe206f11671096ffdd38061171dd6e5/README.md> | Official capabilities, long-audio and official-toolkit behavior; ASR itself emits language/text and uses a separate Forced Aligner for timestamps. |
| Q3 | Qwen ASR preprocessor config | Qwen | primary config | <https://huggingface.co/Qwen/Qwen3-ASR-1.7B/blob/7278e1e70fe206f11671096ffdd38061171dd6e5/preprocessor_config.json> | 128 features, FFT 400, hop 160, 30-second/480,000-sample frontend contract. |
| Q4 | transcribe.cpp runtime commit `ea077b8…` | handy-computer | primary runtime | <https://github.com/handy-computer/transcribe.cpp/commit/ea077b87590bcfb090d7c38c03ab36cd1c7005d3> | MIT runtime identity. Pinned checkout docs say 16 kHz mono input, text/language output, no timestamp/forced-aligner head, and GGUF reproduction support. |
| Q5 | Qwen3-ASR-1.7B GGUF revision `92282af…` | handy-computer | primary converted artifact | <https://huggingface.co/handy-computer/Qwen3-ASR-1.7B-gguf/tree/92282af1610a2db19d66f2bef1e260f5deca782d> | Apache-2.0 inherited model license; Q4_K_M filename; 1,319,830,496 bytes; published SHA-256 `b7afe367…`; no timestamps; runtime does not accept explicit language hints. |
| Q6 | Qwen3 Forced Aligner HF revision `c07281d…` | Qwen | primary model | <https://huggingface.co/Qwen/Qwen3-ForcedAligner-0.6B-hf/commit/c07281df297b9905d24a508279258cccf987a064> | Apache-2.0; `Qwen3ASRForTokenClassification`; source filename `model.safetensors`, 1,835,545,960 bytes and hosted SHA-256 `00568245…`; 11 languages. |
| Q7 | Forced Aligner model card at pinned revision | Qwen | primary model card | <https://huggingface.co/Qwen/Qwen3-ForcedAligner-0.6B-hf/blob/c07281df297b9905d24a508279258cccf987a064/README.md> | Audio + transcript + language input; arbitrary-unit timestamps; up-to-five-minute speech; transcript normalization/tokenization is processor/language dependent. |
| Q8 | Forced Aligner processor config | Qwen | primary config | <https://huggingface.co/Qwen/Qwen3-ForcedAligner-0.6B-hf/blob/c07281df297b9905d24a508279258cccf987a064/processor_config.json> | 16 kHz, 128 features, FFT 400, hop 160, timestamp segment 80 ms. |
| Q9 | qwen3-asr.cpp runtime commit `6dcc586…` | predict-woo | primary runtime | <https://github.com/predict-woo/qwen3-asr.cpp/commit/6dcc586e5073fd6e85ee5728e75f0903d6c70c6c> | MIT runtime; F16 Forced Aligner conversion and word-timestamp contract; 16 kHz mono PCM WAV. README's model table is stale/incomplete relative to source/tests. |
| Q10 | Qwen3-ASR technical report | Qwen | primary paper | <https://arxiv.org/abs/2601.21337> | Architecture/benchmark context only; not GGUF binary provenance. |

## RMVPE and OpenVINO

| ID | Source | Publisher | Class | URL | Facts supported |
| --- | --- | --- | --- | --- | --- |
| M1 | RMVPE official implementation | Dream-High | primary project | <https://github.com/Dream-High/RMVPE/commit/a6db1cd7d26014aa739383367afd9bab57fc624c> | Official PyTorch implementation and Apache-2.0 code-repository license. It does not publish Uta Studio's exact ONNX. |
| M2 | RMVPE paper | Wei et al. | primary paper | <https://arxiv.org/abs/2306.15412> | Robust vocal F0 algorithm/architecture claims; not exact ONNX provenance. |
| M3 | Exact `rmvpe.onnx` host revision `e6d0c1a…` | lj1995 | primary artifact host | <https://huggingface.co/lj1995/VoiceConversionWebUI/tree/e6d0c1a17da07c33557852f9dfa2bd44cc75737d> | Hosted `rmvpe.onnx`, 361,688,443 bytes, published SHA-256 `5370e71a…`, repository metadata `license:mit`. |
| M4 | rmvpe-onnx source lineage | NewComer00 | secondary wrapper/provenance | <https://github.com/NewComer00/rmvpe-onnx/commit/a16200fd8b90aec04ba3e5691fcdd808f74259a8> | Documents exact lj1995 model hash, RVC frontend lineage, 16 kHz mono, 128 mel bins, 1,024 Hann FFT/window, 160 hop, 30–8,000 Hz frontend, 360 salience bins and local-average F0/confidence output. Installed package version observed locally is 0.2.3. |
| M5 | RVC RMVPE frontend commit | RVC Project | primary implementation lineage | <https://github.com/RVC-Project/Retrieval-based-Voice-Conversion/blob/7e03261/rvc/lib/rmvpe.py> | Reference frontend/decoder lineage cited by `rmvpe-onnx`; MIT project lineage. |
| M6 | OpenVINO runtime commit `8a17657…` | OpenVINO Toolkit | primary runtime | <https://github.com/openvinotoolkit/openvino/commit/8a17657b995fd3b4a52f8484acfcf2bb61214623> | Exact runtime source commit associated with tag 2026.3.0 and Apache-2.0 runtime licensing. Uta Studio's build/conversion recipe remains repository-owned. |

**License conflict note:** the official Dream-High code repository is
Apache-2.0, while the exact lj1995 ONNX host and the RVC/NewComer lineage state
MIT. These apply to different distributions. The intended package must retain
the exact ONNX host's notices and should not cite only the official-code
Apache license.

## Optional/future experts

| ID | Source | Publisher | Class | URL | Facts supported |
| --- | --- | --- | --- | --- | --- |
| O1 | FCPE official implementation | CNChTu | primary project | <https://github.com/CNChTu/FCPE/commit/6a149c1afb1c7e7821b71869dfb31ad50c95b516> | MIT; 16 kHz example, 160-sample hop, local-argmax, 80–880 Hz defaults, optional MIDI conversion. |
| O2 | Uta-selected FCPE ONNX revision `5800a2b…` | gzivdo | secondary/community conversion | <https://huggingface.co/gzivdo/fcpe-onnx/tree/5800a2b1944967f55bb0bfeb9718cb749f809310> | Explicitly unofficial export; MIT claim; exact ONNX 43,612,026 bytes, SHA-256 `b7e4f387…`; `[1,n_samples,1]` → Hz with 10 ms hop. |
| O3 | Basic Pitch official implementation | Spotify | primary project | <https://github.com/spotify/basic-pitch/commit/fa5997af0a8210982619003269994a1be25eddf3> | Apache-2.0; official ONNX is among shipped serializations; mono/downmix, 22,050 Hz, windowed polyphonic note/onset/contour to MIDI/note events; best on one instrument. |
| O4 | Uta-selected Basic Pitch ONNX mirror `327fd8c…` | AEmotionStudio | secondary mirror | <https://huggingface.co/AEmotionStudio/basic-pitch-onnx-models/tree/327fd8ccd2f0bb84cbe56b4a0e9d318398ddf763> | Mirror claims official Spotify ONNX; Apache-2.0; exact `nmp.onnx` 230,444 bytes and SHA-256 `2c3c1d1…`. |
| O5 | FireRedASR2S official commit `4e7d9aa…` | FireRedTeam | primary project | <https://github.com/FireRedTeam/FireRedASR2S/commit/4e7d9aaf4482a47cec1724807026b9b151926eb5> | Apache-2.0; AED family, Mandarin/dialects/English/code-switching and singing purpose; official fixture identity used by local record. |
| O6 | FireRed AED ONNX revision `13f9508…` | 42ailab / ManySpeech conversion | secondary converted artifact | <https://huggingface.co/42ailab/FireRedASR2-AED-ONNX/tree/13f950858934f7b6a0d3ce52bae65af0dc022258> | Apache-2.0 claim; split int8 encoder/decoder/CTC, CMVN/tokens; exact hosted source hashes. It is a redistributed conversion, not FireRedTeam's original binary. |
| O7 | STARS source commit `f0e43e9…` | gwx314 | primary project | <https://github.com/gwx314/STARS/commit/f0e43e96cfe953f71a6cf9efd8b908b2c9d7e167> | MIT code; transcription, alignment, technique and global-style purpose; requires phoneme/text metadata and pure vocals preferred. |
| O8 | STARS checkpoint revision `744a7ad…` | verstar | primary checkpoint host linked by project | <https://huggingface.co/verstar/STARS/tree/744a7ad02e1d788452293cd903ea6a933f7862c4> | Chinese checkpoint 601,773,408 bytes and hosted SHA-256 `9159dd37…`; repository has no explicit model-card license metadata. |
| O9 | ROSVOT source commit `3c8332b…` | RickyL-2000 | primary project | <https://github.com/RickyL-2000/ROSVOT/commit/3c8332bf43adae35f6e4d64971862f2f6139b310> | MIT code; singing waveform to MIDI notes, optional word-boundary conditioning, noisy/raw-song support; no Runtime Manager resource currently selected. |
| O10 | ROSVOT paper | Li et al. | primary paper | <https://arxiv.org/abs/2405.09940> | Architecture/purpose claims only. |

## Repository-owned evidence (not external sources)

Repository-owned current evidence is read directly from `native-inference/runtime-lock.json`, `runtime-manager/src/catalog.rs`, `analysis-engine/**`, worker source/manifests, focused tests, `tasks/remaining-models/STATE.md`, and `docs/KEY_CONCLUSIONS.md`. Machine-local inventory snapshots and validation journals are intentionally not retained as durable documentation.
