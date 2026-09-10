# Super acceleration

## Authorized scope and current status

User clarification, 2026-09-10: assign **different complete model tasks** to the AMD integrated
GPU and Intel B580 using dependencies, loading order and predicted device completion times.
**Do not split one model invocation's chunks across GPUs.** Retain model hot loading, reuse of
identical decoded audio, shared separation results and genuinely consumed device intermediates
when they reduce whole-pipeline elapsed time. Keeping both GPUs busy is not the objective.

**The earlier chunk-splitting interpretation is withdrawn. Corrected task-level execution is
not implemented yet; GPU validation is paused after a host restart. No production qualification
or general speed claim.**

Reuse the existing stable `turbo_acceleration` setting, default off. Settings > Models & runtime
owns it. Save errors must be visible and must not leave the UI claiming an unsaved change.
Each new exact analysis request snapshots the value; changing the preference must not mutate
already queued/running requests. Existing load/save configuration APIs represent the setting.

## Execution ownership

Studio sends execution intent through the packaged Analysis Engine protocol only. Analysis
Engine owns dependency ordering, cancellation and the lifetime of this analysis's resources.
The native GGML worker/runtime owns model weights, device-local tensors and GPU submission.
No Studio/backend crate imports, model scripts, network services or CPU inference fallback.

The normal execution mode remains available. The super mode does not change models, precision,
chunk context/overlap, fusion rules, or quality thresholds. A complete model may produce different
numerics on a different GPU; compare outputs without changing its precision policy.
The current worker makes the F32 matmul choice process-wide before initializing GGML. Weight
residency/session reuse must preserve that per-model choice rather than silently reusing the
first model's precision for every subsequent model.

## Work and residency

- Assign each complete model invocation to one GPU. Its original chunks, serial frontend /
  inference / reconstruction, synchronization and overlap-add remain on that device. Different
  ready model tasks may overlap across GPUs only through the Engine's task scheduler.
- Preload the next planned model, after current graph allocations are visible. If a conditional
  branch skips that model, release its worker rather than executing or reusing the wrong weights.
  Use an actual free/budget observation, not installed VRAM size. An unavailable observation or
  insufficient headroom skips speculative preloading, not the requested analysis. Allocation
  observations are advisory and cannot reserve memory against other applications.
- Retain reusable decoded/resampled representations and genuinely consumed GPU intermediates
  within one analysis. Compact GPU encoder output is released at its final decoder consumer;
  shared PCM files live until this analysis exits, including cancellation/failure. Keep
  artifact publication separate from the hot path; FLAC publication must still be correct and
  atomic. Do not upload unused raw media simply to claim a GPU cache.
- Avoid duplicate FFmpeg decode/resample passes for the same authorized source and representation.
  Retained resources must be bounded; no user-library/model mutation, destructive global cache
  operation, stale cross-run reuse, hash verification or compatibility/migration path.
- Expose real device work, cache reuse and preload outcomes through local analysis diagnostics/
  lifecycle events. Distinguish a requested mode from measured dual-device work and cache hits.

## Corrected task-level scheduling design

- Schedule from actual Engine artifact dependencies, not a round-robin device list or the visual
  order of Studio cards. The existing plan already exposes independent ASR, pitch and note
  branches after audio preparation. Alignment still waits for its transcript, conditioned note
  experts wait for their inputs, and fusion waits for the required evidence.
- Compare predicted finishes using dependency readiness, device queued work, required loading /
  uploads, actual model/device execution observations, and any data-transfer cost. Account for
  downstream critical-path work and the total finish time across devices. Waiting briefly for
  B580 can beat immediately sending a long critical task to the integrated GPU. Estimates are
  revisable observations, not fixed model/device assignments or frozen acceptance thresholds.
- Preserve explicit device choices. Unknown timing or memory information is not a reason to
  block required analysis or infer on CPU. Do not run unrequested calibration inference merely
  to fill a cost table, or substitute theoretical GPU FLOPS for measured complete-task times.
- Prepare the selected upcoming model on its assigned device, retain its actual typed weights,
  and consume them in that same precision-isolated worker. Model hot loading is not just warming
  the filesystem cache. Residency must include upcoming allocations as well as active consumers.
- Keep one producer for each identical decode/resample representation or semantic separation
  output, with multiple downstream readers. Concurrent consumers need coordinated publication
  rather than racing the current disk-cache index or independently decoding the same source.
  Useful tensors remain until their last dependent consumer; no arbitrary raw-stem uploads.
- The current linear Engine orchestration, thread-local run ownership and global GGML foreground
  lease do not yet implement this scheduler. Replace orchestration with explicitly owned tasks,
  device queues and deterministic result assembly; do not merely remove the global lock and
  claim safe parallel execution. Carry cancellation/reaping, precision isolation, existing
  synchronization and teardown semantics through that work.

## Safety review carried forward

Read [restart handoff](../../ROFORMER_B580_REBOOT_HANDOFF_2026-09-07.md),
[submission/serial semantics](../../ROFORMER_B580_XE90_FULL_2026-09-07.md),
[upload incident](../../ROFORMER_B580_UPLOAD_BLACKOUT_2026-09-07.md) and
[operation recording](../../ROFORMER_OPERATION_RECORDING.md). Their surviving conclusions are:

- A successful process, a tiny copy test, sufficient VRAM or an unchanged boot during a test
  does not establish later host stability. Creation, transfer and teardown also involve drivers.
- Preserve synchronous completion, buffer ownership through final use, cancellation/error
  propagation and child reaping. Historical batch/serial/no-async semantics are not equivalent
  to a power limit, per-submission fence, or proof against reboot.
- Do not identify the last logged operation or simultaneous compilation as the cause. Do not
  revive deleted backend implementations or apply past single-run exceptions as product defaults.
- Do not add power caps, fixed delays, hashes, frozen baselines, load-threshold gates or retries
  as a substitute for diagnosis. Existing measures are not removed merely to simplify scheduling.
- This correction proceeds through source changes and CPU-only verification. GPU experiments
  remain paused; restarting a failed workload solely to obtain more logs is not the next step.

## Reusable implementation and historical observations

- The Engine owns one optional upcoming worker per analysis. Preparation starts after the
  current model's first completed work unit, uses `VK_EXT_memory_budget` observations, and
  retains actual typed weights for the following `Run`. Unknown/insufficient budgets skip
  preparation. Foreground serialization/quiescence remains unchanged. Workers retain their
  own model-specific process-wide precision choice. The schedule avoids a second instrumental
  preparation when the existing combined separator produces both artifacts, and uses requested
  language applicability to predict conditional preloads. This does not change which models
  actually execute: a later detected language can still require an on-demand FireRed run.
- The earlier RoFormer secondary-backend/chunk-splitting path is rejected by the clarified
  requirement. Remove that path, keeping ordinary single-device overlap-add unchanged. Its
  historical dual-chunk tests and timings are not acceptance for complete-model task scheduling.
- Exact source identity, sample rate and channel count key a per-analysis PCM cache. Workers
  reuse immutable hard-linked/copy inputs; another format is decoded from the original source,
  never from an already resampled representation. Source files remain untouched.
- Qwen retains compact encoder embeddings rather than the whole encoder graph arena. The
  decoder copies them into its input on the same device, then releases them after prefill or
  classification. The pinned Vulkan backend implements this as a synchronous same-device
  buffer copy; no host audio-embedding round trip is needed. Actual retained/reused byte counts,
  PCM hits and preparation outcomes have task-correlated diagnostics.
  Raw stems are not uploaded merely to claim residency: their current FFT consumers run on the
  host. Existing separation artifacts already serve multiple downstream models without rerunning
  their producer. Arbitrary cross-process GPU-tensor sharing and independent DAG-node inference
  overlap are not implemented.
- Focused verification: settings/request propagation and persistence-error tests passed;
  eight RoFormer scheduling tests, two preload-prediction tests, two budget tests, 34 worker
  tests, 23 Qwen tests and 13
  supervisor/cleanup tests passed. One explicit native CPU reference test verifies retained
  tensor lifetime and decoder-offset copying. GPU model-reference tests were not implied by
  those unit-test results.

**Historical measurements below describe the rejected chunk-splitting implementation, not the
corrected complete-model scheduler.** One paired 60-second RoFormer run on commit `a8e082f` completed with the same source, model,
precision, runtime and observer settings: ordinary **32.148910 s**, super **28.888136 s**
(**10.1427% less wall time**). B580 processed eight chunks and AMD 780M one; sixteen sampled
intervals show compute activity attributed to that worker on both devices. Both output FLACs
fully decoded, had 5,292,000 finite samples, and the vocal relative RMS difference was
`6.54594e-5` (instrumental `6.16167e-5`). This is neither full-pipeline throughput evidence nor
perceptual/bitwise parity qualification. Details: `test-artifacts/super-acceleration/chunk-comparison.json`.

The first instrumented 12-second whole-pipeline pair on `a8e082f` completed in **143.260202 /
143.958165 s** (ordinary/super): no demonstrated total gain. Both ran twelve models successfully.
The super trace proves actual prepared-weight consumption, exact-format PCM hits, and Qwen
same-device retention/injection (ASR **1,384,448 bytes**, aligner **745,472 bytes**). The first
separator took 9.266 / 18.036 s: its two chunks gave the cold integrated GPU no useful
continuation with which to amortize its slower work. Three unnecessary preloads were discarded.
The ordinary preflight also saw unrelated compilation, so that pair is not an isolated speed
comparison. These observations motivated `8069ee4` and `5c0e0a5`, not changes to model math.
`a9741bc` corrects the new scheduling fixture to use overlapping chunks without changing the
existing crossfade. Eight scheduling, two prediction and thirteen supervisor tests then passed;
release CLI/worker build: `20260910T092805-8cb46841f749`.

The refined ordinary run on `a9741bc` exited successfully in **150.865198 s** at
**18:32:58 +09:00**. Contrary to an initial conversational statement, durable records establish
that refined super **did launch at 18:35:09 +09:00**. It entered the first Leap separation task;
the last saved work progress is **0/2 chunks completed**, with no completion record. The new
boot began **18:36:16 +09:00** (`fce41cf7-2fa3-4747-8238-16043dabf181`, following
`eacf1481-aa6d-4d60-b768-1b4d41acb175`). This timing does not establish the cause or exact failure
instant. The 18:33 preflight saw about 99.94% aggregate CPU use and a `rustc` process at 1527.5%
of one core; its owning command/session is not recorded by that sample. Previous-boot kernel
logs remain unreadable due to permissions. GPU experiments are paused, incomplete outputs are
preserved, and no automatic retry is authorized by these records. Evidence:
`test-artifacts/super-acceleration/refined/pipeline-super-observation/`, launch operation
`20260910T093505-99361ad5df8d`, and read-only review `20260910T093840-953484ccee48`.

## Verification and handoff

Use isolated CPU scheduling/cache fixtures for dependency readiness, unequal whole-task costs,
critical-path completion, loading/residency reuse, single-producer/multiple-consumer results,
insufficient or unknown memory, unused preloads, cancellation and failure cleanup. Test setting persistence,
exact-request propagation and surfaced UI errors. Keep source files below 2,000 lines.

Record and commit each independent implementation before executing it. Before GPU runs, inspect
recorded host load and GPU snapshots. Use both Vulkan ICDs for dual-GPU measurements; the previous
full-song debug command exposed only the Intel ICD and is not a dual-device fixture. Start with
bounded same-input ordinary/super runs, observe real work on both DRM devices, compare outputs,
report wall time/cache hits/peak residency, then exercise the full pipeline if relevant checks
justify it. No automatic retries or hardware isolation claims. Do not compare performance against
the 2.27 GB full-debug logging run. Normal non-debug observations must be matched.

Task progress and accepted measurements belong in
[`STATE.md`](../../../tasks/remaining-models/STATE.md). Engineering and component boundaries remain
those in [`engineering-constraints.md`](../../engineering-constraints.md) and
[`design/README.md`](../README.md). The older
[`performance direction`](../../PERFORMANCE_DIRECTION_2026-09-09.md) motivates this work but its
pre-attention-optimization device-rate estimates are not current measurements.
