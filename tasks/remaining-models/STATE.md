# Remaining Models + Final Feature Closure — State

**Updated:** 2026-08-23
**Owner:** active `docs/agent-tasks/TASK_ALL_MODEL_REPAIRS.md` agent

This file stores current effective state only. Long-form execution/completion logs are not retained. Durable cross-cutting conclusions live in `docs/KEY_CONCLUSIONS.md`.

Current source and focused tests override stale historical conclusions.

## Model closure

| Resource | State | integration_ready | production_ready | Current conclusion |
| --- | --- | --- | --- | --- |
| `rmvpe` | READY | yes | yes | OpenVINO `ProductionPinned`; primary continuous-F0 path. |
| `game` | READY | yes | yes | Technical Production path accepted; CC-BY-NC-SA checkpoint conditions remain recorded and user-visible but are non-blocking under the current open-license policy. |
| `qwen3_asr_1_7b` | READY | yes | no | Live pinned Vulkan worker emits truthful runtime-detected language in strict schema 2 and the Engine typed/Fusion path consumes it. Production retains representative full-input singing quality/stability gates. |
| `qwen3_forced_aligner_0_6b` | READY | yes | no | Strict schema-2 Engine consumption and deterministic bounded long-input windowing are verified. The 305.813375 s fixture preserves complete lyrics, reaches 268.96 s, and repeats byte-identically; broad labeled Production quality remains open. |
| `melband_roformer_denoise_aufr33` | READY | yes | no | Exact native stereo DSP, OpenVINO worker output/done, semantic dry FLAC, repeat/cancel/restart, LocalImport/verify/Benchmark resolve, and Engine consumption are verified. Benchmark status blocks Production; an explicit open checkpoint license is sufficient for the license gate. |
| `melband_roformer_dereverb_anvuew` | READY | yes | no | The Denoise-class dynamic `[1,T,7916]` neural-island topology reuses the exact native stereo MelBand DSP with pinned Dereverb config/checkpoint and explicit `noreverb` semantics. Current Worker output/done, 1.5 s byte-identical repeat/restart, GPU-chunk cancellation cleanup, 12 s stereo/timeline preservation, historical native regression (corr 0.999986; relative L2 0.00538), LocalImport/verify/Benchmark resolve and Engine dereverb route pass. Benchmark validation, broader dereverb quality/packaging and exact checkpoint license identity block Production; restrictions on an explicit open license are non-blocking under current policy. |
| `melband_roformer_harmony` | READY | yes | no | UVR Karaoke semantics map the predicted `Vocals` target to lead; the required input is EP317 all-vocals and the exact complement is the non-overclaimed `vocal_residual = all_vocals - lead_vocal`. The accepted schema-2 generation `ed73768d…a0590` is a manifest-pinned 21-island OpenVINO topology: CPU BandSplit and eight bounded Mask groups surround six full-context time/frequency GPU pairs. Time attention keeps all 801 frames and microbatches only independent bands; frequency attention keeps all 60 bands and microbatches only independent frames. For long audio, host DSP prepares all chunks and each GPU island is compiled once across every chunk before release. Exact ORT/CPU/product parity, real 12 s stereo/timeline/complement/seam validation, byte-identical repeat, dual typed Worker output, LocalImport/verify/Benchmark resolve, Engine lead-only publication/residual cleanup and supervisor cancel/restart pass. Monolithic GPU execution and per-chunk recompilation of all GPU islands are rejected after correlated hard restarts. Benchmark status, unresolved checkpoint license identity, broader quality/latency and packaging block Production. |
| `melband_roformer_inst_v2` | READY | yes | no | Exact T=1101 semantics are preserved by a bounded 33-island manifest-pinned CPU/GPU OpenVINO topology. Exact ORT/CPU/product parity, 12.8 s three-window Instrumental semantics/seams, deterministic restart, cancellation cleanup, LocalImport/verify/Benchmark resolve and the real Engine worker route are verified. Broader quality/latency block Production; explicit open checkpoint licensing is sufficient for the license gate. |
| `bs_roformer_vocals_ep317` | READY | yes | no | Exact T=801 BS topology is preserved by a manifest-pinned 34-island CPU/GPU OpenVINO route: full time/frequency attention context, rolling GPU residency, exact parity, real 12 s GuideVocals quality near the historical EP317 path, deterministic Worker/Engine output, cancellation cleanup, LocalImport/verify/Benchmark resolve and Engine consumption pass. Benchmark validation, broader quality/latency and packaging remain Production gates; explicit open checkpoint licensing is sufficient for the license gate. |
| `firered_asr2_aed` | READY | yes | no | Fixed 230-frame Encoder/CTC and 0–10 Decoder cache-bucket IR topology is preserved behind deterministic full-coverage schema-3 windowing with rolling bucket residency. Official `你好世界`, three-window long input, byte-identical restart, inference-stage cancellation cleanup, terminal protocol, LocalImport/Benchmark resolve and Engine alternative-only challenger consumption pass. The representative real-singing fixture decodes only `<sil>` and now fails closed, so broad singing usefulness/quality and Benchmark validation block Production. |
| `fcpe` | READY | yes | no | The exact 32,000-sample/201-frame FCPE IR runs as deterministic full-length 2 s windows with endpoint ownership and tail clipping. Schema 3 preserves only continuous F0/null values and no longer fabricates voiced or confidence claims. Real 6 s singing yields 601 frames and agrees within 50 cents on 91.25% of RMVPE-voiced frames (median 3.72 cents); terminal protocol, byte-identical restart, GPU-window cancellation cleanup, LocalImport/Benchmark resolve and Engine secondary-only consumption pass. Benchmark status, unofficial ONNX provenance and broader quality/voicing calibration block Production. |
| `basic_pitch` | READY | yes | no | The exact `[1,43844,1] -> 172×(88 note, 88 onset, 264 contour)` IR now follows the reference 30-frame overlap path: 3,840-sample edge padding, 36,164-sample window hop, 15+15 frame trim, 142 owned frames and a strict 256-sample timeline. Real 6 s singing emits 516 finite non-constant activation frames; terminal protocol, byte-identical restart, GPU-window cancellation cleanup, LocalImport/Benchmark resolve, Engine helper consumption and onset-only fusion tests pass. It remains non-substituting source-local activation evidence, not GAME notes or calibrated confidence. Benchmark status, mirror provenance, broader labeled quality and full note-event postprocessing block Production. |
| `stars` | READY | yes | no | Chinese P0 remains strictly `notes.stars`; `technique.analyze` stays disabled. The accepted `c279da93…83ea` LocalImport generation contains fixed T=256/N=32 Stage A/B/C graphs plus the exact shared frontend and embedded hash-pinned Chinese G2P asset. Native segmentation preserves the complete conditioned timeline, and selected OpenVINO GPU parity passes with worst relative L2 `2.34e-6` and 1.60 GB peak RSS. A real 6 s singing Engine-helper run emits 1,125 frames, three notes and two MIDI claims; independent Worker restarts are byte-identical (`34769ff4…3774`), active cancellation publishes no partial artifact, and restart succeeds. Runtime verify/Benchmark resolve and strict Engine correlated-provenance consumption pass without replacing GAME. Benchmark validation, unresolved checkpoint license identity, broader labeled note/alignment accuracy, latency and release packaging block Production. |
| `rosvot` | READY | yes | no | P0 requires TimedTranscript and excludes automatic RWBD. The accepted `a84ef89f…e9e5` LocalImport generation contains only fixed T=256/N=32 frame/pitch graphs plus the same immutable shared frontend/annotation-RMVPE generation; native projection, regulation, aggregation and seam handling preserve full conditioned timelines. Selected OpenVINO GPU parity passes with worst relative L2 `9.74e-7` and 1.10 GB peak RSS. A real 6 s singing Engine-helper run emits 1,125 frames, six notes and one MIDI claim; independent Worker restarts are byte-identical (`360dfa4f…46d0`), active cancellation publishes no partial artifact, and restart succeeds. Runtime verify/Benchmark resolve and strict Engine TimedTranscript-correlated challenger consumption pass without replacing GAME. Benchmark validation, unresolved checkpoint license identity, broader labeled note/boundary accuracy, latency and release packaging block Production. |

The repository's license gate is permissive: an explicit open-source/open-model license is sufficient for technical Production. Attribution, NC/SA, commercial-use, or redistribution conditions remain recorded and user-visible but are non-blocking; only missing, ambiguous, or non-open license identity may block on licensing grounds.

## Pending execution tasks

| # | Task | State | Gate |
| ---: | --- | --- | --- |
| 14 | `14_GLOBAL_BUBBLE_SMOKE.md` | RUNNING | all model resources are now integration-ready; aggregate bubbles are in progress |
| 15 | `../final-features/15_COMPILED_WORKFLOW_EXECUTOR.md` | PENDING | Phase B after model closure |
| 16 | `../final-features/16_CONDITIONAL_EXPERT_SCHEDULER.md` | PENDING | requires 15=READY |
| 17 | `../final-features/17_LEAD_BACKING_HARMONY_PARTITION.md` | PENDING | requires Harmony effective `integration_ready=yes` |
| 18 | `../final-features/18_STARS_TECHNIQUE_STYLE_P1.md` | PENDING | requires STARS effective `integration_ready=yes` |
| 19 | `../final-features/19_ENGINE_RHYTHM_QUANTIZATION.md` | PENDING | Phase B after model closure |
| 20A | `../final-features/20A_STUDIO_BACKEND_UI_PARITY_CLOSURE.md` | PENDING | requires 15–19 READY |
| 20 | `../final-features/20_PRODUCT_E2E_FEATURE_BUBBLE.md` | PENDING | requires 15–19 + 20A READY |
| 21 | `../final-features/21_FINAL_DESIGN_PARITY_AUDIT.md` | PENDING | final design/UI/process-boundary audit |

## State rules

Allowed states:

```text
PENDING
RUNNING
READY
BLOCKED
FAILED_SAFE
SKIPPED_ALREADY_CLOSED
SKIPPED_PRECONDITION
NEEDS_REVIEW
```

`RUNNING` is progress metadata only; it is not a concurrency gate.

A repair that reaches `READY` updates the resource's effective current row directly. Do not recreate historical failure logs; retain only the current conclusion and any blocker that still matters.

Cards 15–21 and 20A must obey `tasks/final-features/PROCESS_BOUNDARY_RULES.md` and `tasks/final-features/STUDIO_BACKEND_UI_PARITY.md`.

For accelerator authorization, follow `docs/agent-tasks/MODEL_GPU_WORK_POLICY.md`: non-Qwen Vulkan/Level Zero calls require explicit user permission; Qwen is exempt; other calls have no repository GPU restriction.
