# Uta! Studio — Model-owned quality controls

## Product behavior

**Settings → Analysis → Per-model quality & sensitivity** lists all eighteen current catalog
resources. Select a model, edit a number with minus / editable value / plus, and use **Apply**
or Enter for typed values. Changes are saved before the visible configuration changes; failures
are shown in the Settings notice. **Reset model** removes only that model's overrides.

The user's explicit requirement is real **Overlap** control, not merely Fast / Balanced / Maximum
presets. All six RoFormer/PolarFormer separation and cleanup resources expose independent overlap
factors. Stride is `chunk_samples / overlap`: 2 → 50%, 4 → 75%, 8 → 87.5%. More overlapping contexts
and blending can improve seams/separation at increased cost; it is not a monotonic quality promise.
Chunk length and precision are not changed. No override means the GGUF's actual default, not an
assumed universal value (for example, some MelBand artifacts default to four). The UI says **Default**
until explicitly overridden. Factor one uses no fade, because no neighboring chunk can fill a faded
seam. Both native routes share ordered overlap-add, preserve exact output length, and stay on one device.

Quality parameters survive Super acceleration: automatic scheduling owns backend/device placement,
not user-selected overlap or sensitivity. Every queued request snapshots `execution_policy.model_settings`.
Settings changes do not modify running requests, source media, installed assets, or existing charts;
existing chart data changes only after re-analysis. Exact request fingerprints and Step 1 stem-cache
recipes include model settings. Live stem publication uses the running request's snapshot, not current
settings, so a changed overlap cannot silently reuse the earlier stem. Unrelated pitch controls do not
invalidate separation caches.

## Upstream search and native support

Research fetched upstream source/model documentation read-only on 2026-09-11. Saved responses and
failed URL attempts are under `test-artifacts/model-quality-settings/upstream/`; operation records start
at `20260911T183201-3df49d3de8f8`. These sources were inspected, not executed or installed. Default branches
are research snapshots, not new artifact pins or model identities. Current native defaults are preserved
where they differ from upstream examples.

| Model resources | Supported control exposed | Upstream evidence and native ownership |
| --- | --- | --- |
| Leap XE90 vocals and instrumental; BS-PolarFormer Public; Harmony, Denoise, Dereverb | Overlap factor; model-file default unless overridden | [Music Source Separation Training demix](https://github.com/ZFTurbo/Music-Source-Separation-Training/blob/main/utils/model_utils.py) reads `num_overlap`, chunk size and batch size. Both native RoFormer host paths already own corresponding chunk/overlap geometry. |
| RMVPE | Voiced salience threshold (current default 0.03) | [Official local-average decoder](https://github.com/Dream-High/RMVPE/blob/main/src/utils.py) has `thred`; [inference wrapper](https://github.com/Dream-High/RMVPE/blob/main/src/inference.py) separately owns batch size. Studio retains raw Hz/confidence and publishes the selected voiced threshold for Engine and STARS/ROSVOT conditioning. |
| FCPE | Voiced threshold (0.006) | [Official README](https://github.com/CNChTu/FCPE#readme) documents `threshold=0.006`; shared native centroid decoding consumes it on both routes. |
| GAME small / medium / large | D3PM sampling steps (8); boundary threshold (0.2); voiced-note threshold (0.2) | [Release schema](https://github.com/openvpi/GAME/blob/v1.0.3/lib/config/schema.py) exposes sample steps, boundary and presence thresholds. Existing native `GameInferParams` carries them; worker evidence reports actual values. Model size is still selected in Workflow, not changed by these controls. |
| JBM555 | Onset threshold (0.32); offset threshold (0.70) | [Published inference configuration](https://github.com/york135/CECTC_baseline_APSIPA25/blob/main/configs/inference_jbm.yaml). Shared native dual-input decoder consumes both; evidence uses stable decoder identity plus actual thresholds. |
| STARS | Note-boundary threshold (0.8) | [Chinese checkpoint config](https://github.com/gwx314/STARS/blob/main/configs/stars_chinese.yaml), `note_bd_threshold`. Shared transcript-conditioned pipeline passes this to rhythm regulation, not to technique confidence. |
| ROSVOT | Note-boundary threshold (current native default 0.85) | [Official config](https://github.com/RickyL-2000/ROSVOT/blob/main/configs/rosvot.yaml) exposes `note_bd_threshold` (example 0.8). Current native 0.85 remains the default; word guidance and gap regulation are unchanged. |
| Qwen3-ASR 1.7B | Maximum new tokens per window (256) | [Official usage](https://github.com/QwenLM/Qwen3-ASR#quick-inference) documents `max_new_tokens` separately from `max_inference_batch_size`. Both native routes already accept a decoder budget. Increasing it may help dense text but increases unfinished-window work. |
| FireRedASR AED | Maximum new tokens per native window (58) | [Official inference command](https://github.com/FireRedTeam/FireRedASR2S/blob/main/examples_infer/asr/inference_asr_aed.sh) exposes decoding length, batch and beam. Studio's current native decoder is greedy with a 58-step capacity and fixed ~2.3-second audio window; controls expose only that implemented capacity, not the upstream 300-token/beam example. |
| Basic Pitch | Explicit support explanation; no inactive MIDI controls | [Official predict](https://github.com/spotify/basic-pitch/blob/main/basic_pitch/inference.py) offers onset/frame thresholds in its MIDI note decoder. Studio consumes raw note/onset/contour activations instead; these thresholds would not control its current native output. Fixed 30-frame overlap remains unchanged. |
| Qwen3 Forced Aligner 0.6B | Explicit support explanation; no generation controls | [Official aligner](https://github.com/QwenLM/Qwen3-ASR/blob/main/qwen_asr/inference/qwen3_forced_aligner.py) performs timestamp classification and supports upstream batching. No beam, temperature or generation-token quality setting exists in the native classifier; trained timestamp resolution remains fixed. |

**Batch is not a quality multiplier.** Several upstream implementations support batched inference,
but the packaged native host/model routes currently execute individual windows. No fake batch slider,
serial loop labeled as batching, network fallback, or upstream script runtime was added. The per-model
copy distinguishes upstream support from implemented native controls.

The UI clamps edited values and explains sensitivity versus computational budget. Higher thresholds
usually mean *fewer* detections, not higher quality. There is no cross-model generic "quality strength".

## Code ownership and verification boundary

- `model-settings/`: backend-neutral numeric representation and per-model descriptors; no model/runtime imports.
- `app-core/src/analysis_engine_adapter.rs`, `chain_cache.rs`: settings snapshot and cache recipe propagation.
- `analysis-engine/src/engine/runtime_route.rs`, `workflow_execution.rs`, `worker_tasks.rs`: actual native dispatch, including every audio preparation branch.
- `native-inference/ggml-worker/src/`: setting application and typed evidence publication.
- `native-inference/ggml-runtime/src/` and `libtorch-runtime/src/`: native host overlap/decoder consumption.
- `desktop/src/studio/settings/tuning.rs`: model selection, numeric controls, per-model reset and transactional save errors.
- Existing `load_config` / `save_config` APIs represent persistent settings. New `ui.settings.select_model_tuning`
  is read/navigation; `set_model_parameter`, `adjust_model_parameter`, `apply_model_parameter` and
  `reset_model_parameters` are mutations in the local UI registry and NDJSON dispatcher.

Focused verification and isolated Wayland smoke results are recorded in the task index. CPU tests include
factor 1/2/4/8/16 short/tail/multi-window identity reconstruction, changed work counts, non-default decoder
results/evidence, Super preservation, save failure and isolated cache invalidation. These are not real-song
quality measurements, GPU inference acceptance, listening qualification, Windows verification, or release
packaging. No model readiness promotion follows from adding settings.
