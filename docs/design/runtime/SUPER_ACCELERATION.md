# Super acceleration

## Authorized scope and current status

User clarification, 2026-09-10: assign **different complete model tasks** to the AMD integrated
GPU and Intel B580 using dependencies, loading order and predicted device completion times.
**Do not split one model invocation's chunks across GPUs.** Retain model hot loading, reuse of
identical decoded audio, shared separation results and genuinely consumed device intermediates
when they reduce whole-pipeline elapsed time. Keeping both GPUs busy is not the objective.

**The earlier chunk-splitting interpretation is withdrawn. Corrected complete-model task-level
execution is implemented and covered by CPU/protocol tests. GPU validation remains paused; there
is no production qualification or measured end-to-end speed claim for this scheduler.**

Reuse the existing stable `turbo_acceleration` setting, default off. Settings > Models & runtime
owns it. While enabled, global/per-model runtime and device controls plus Processing Studio model
choices are visibly disabled; saved manual choices are preserved and become active again when the
mode is turned off. Super requests omit those manual route fields so the Engine owns placement.
Save errors are visible and do not leave the UI claiming an unsaved change. Each new exact request
snapshots the value; changing the preference does not mutate already queued/running requests.

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
- Preserve saved explicit device choices without applying them to a Super request; they become
  active again when Super mode is disabled. Unknown timing or memory information is not a reason
  to infer on CPU. Do not run unrequested calibration inference merely to fill a cost table, or
  substitute theoretical GPU FLOPS for measured complete-task times.
- Prepare the selected upcoming model on its assigned device, retain its actual typed weights,
  and consume them in that same precision-isolated worker. Model hot loading is not just warming
  the filesystem cache. Residency must include upcoming allocations as well as active consumers.
- Keep one producer for each identical decode/resample representation or semantic separation
  output, with multiple downstream readers. Concurrent consumers need coordinated publication
  rather than racing the current disk-cache index or independently decoding the same source.
  Useful tensors remain until their last dependent consumer; no arbitrary raw-stem uploads.
- The Engine now owns an AMD lightweight queue that can overlap the Intel speech queue after audio
  preparation. Once transcript alignment and RMVPE are both ready, independently assigned STARS
  and ROSVOT tasks may also overlap. Foreground/quiescence ownership is keyed by exact backend and
  device lane, so one lane stays serial through worker shutdown while another physical GPU may
  progress. Joined tasks inherit request events, shared audio and acceleration ownership; failure
  cancels siblings, preserves the originating error and joins children before output rollback.

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
- `844c016` removes the earlier RoFormer secondary-backend/chunk-splitting API and worker path,
  keeping ordinary single-device overlap-add unchanged and consuming prepared weights as before.
  Settings copy now explicitly says complete-model GPU scheduling is in development. Historical
  dual-chunk tests and timings are not acceptance for complete-model task scheduling.
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

## Global optimization audit (2026-09-10)

This is a source audit and read-only reanalysis of existing observations, **not a new execution
or measured speedup**. Evidence: `test-artifacts/super-acceleration/global-audit.json`, operation
`20260910T095859-63caa23dcca2`. Keep the corrected whole-model scheduling objective.

### Highest-impact orchestration opportunities

1. **Account for waiting before optimizing arithmetic.** The recorded ordinary 12-second input
   took 150.865198 s on `a9741bc`; native node-start to worker-spawn intervals sum to **105.521 s**
   across twelve models. Eleven intervals are 7.693–10.000 s. These include the existing global
   GGML lease/quiescence and host scheduling, not model compute. They are not 105.521 s of proven
   removable cost. `execution/client.rs::acquire_ggml_lease` and its `Done` handler currently
   serialize and quit each foreground worker. Review task/device ownership and useful work that
   can overlap permitted waits; do not simply delete the lock, synchronization or exit handling.
2. **Execute the actual ready branches.** `planner/plan.rs` exposes independent ASR, RMVPE, FCPE,
   Basic Pitch, GAME and Acoustic DSP inputs after preparation, while `engine.rs` runs them
   sequentially and completes Acoustic DSP before starting ASR. GAME's hard boundary inputs come
   from the exact request, not generated alignment. JBM555 additionally needs its original mix.
   STARS/ROSVOT need shared RMVPE and timed alignment; FireRed's actual applicability may depend
   on Qwen's detected language. Preserve these data/control dependencies and deterministic fusion
   order. CPU DSP and whole GPU tasks are separate schedulable work, not intra-model chunk splits.
   Both acceleration and lifecycle-event contexts are thread-local today; spawning threads without
   explicit shared request ownership would lose scheduling, cache and event context.
3. **Make hot loading start during useful work.** Qwen worker wrappers report `progress(1, 1)`
   only after complete transcription/alignment, whereas `acceleration::start_next` waits for a
   positive completed work unit. In the historical complete Super trace, the first positive unit
   preceded Qwen node completion by only **35 / 29 ms**. Report real completed windows, or a
   truthful allocation/phase event with observed memory, rather than fabricated work units. Keep
   headroom for later graph phases. Cache hits and successful preparation are not proof of
   inference/load overlap. The coordinator also has one global pending slot and a linear cursor;
   future preloads should follow the actually selected device queues and known branch decisions,
   rather than blindly preparing another entry or loading every model that fits.
4. **Learn the right task costs.** Raw lifecycle duration includes the above queue wait. Qwen's
   encoder/decoder timers are host elapsed intervals and omit mel construction and model loading;
   they are not kernel timestamps or complete-task latency. Account separately for queueing,
   initialization/weights, frontend, compute/transfers, output validation/publication and teardown.
   Estimates must use actual device/model/precision/input shape and cold/resident state. The
   decision minimizes final pipeline completion, not equal task counts or theoretical TFLOPS.

### Confirmed reuse gaps, suitable for CPU-first changes

5. **Unify Engine and worker audio reuse.** The worker cache covers `audio::decode_wav` only.
   `audio/decode.rs` independently probes, fully decodes and builds `DecodedAudio` metrics/profile
   on every call; `audio/acoustic.rs` independently decodes 16 kHz mono again. In
   `engine/workflow_execution.rs`, dual separation validates both FLACs, then `engine.rs` decodes
   their published paths again. Cleanup validation discards the profile, then cleanup comparison
   and final quality evaluation decode the same cleaned output again. Carry the validated facts /
   signal profile through publication and share the exact PCM representation with DSP/workers.
   A cache key must include source/producer identity, stream selection, range, rate, channels and
   effective conversion semantics—not just a semantic role. Engine explicitly maps the first
   audio stream; worker FFmpeg currently uses automatic selection, so those views must not be
   assumed identical for multi-stream sources. Keep full validation; reuse its result, not skip it.
6. **One audio producer, many readers.** `audio_cache.rs` currently uses separate lookup/decode/
   store calls and read-modify-rename of one JSON index. Concurrent task misses would duplicate
   decoding and can lose each other's index entries. Give the analysis an explicit shared owner
   and in-flight producer per exact representation before enabling concurrency. Preserve failure
   wakeup, cancellation/reaping, immutable reader inputs and source-media ownership. A rename of
   a validated generated artifact should carry its existing descriptor, not lose reuse merely
   because its path changed.
7. **Reuse constant frontend preparation.** `stft.rs` rebuilds Hann windows and FFT plans per
   transform; Qwen constructs its Slaney filters/window/FFT in each transcription/alignment
   window; FCPE constructs its frontend plan per fixed audio window. Reuse immutable plans and
   worker-local scratch while retaining exact padding, transform and reduction order. Current
   scratch is already reused *within* a transform, so do not claim per-frame scratch allocation
   as a new finding. Cross-model mel reuse additionally requires identical windows, centering,
   filters and normalization: Qwen ASR/aligner share a frontend implementation but their long-song
   window boundaries differ, and dynamic-range normalization depends on each window. Their model
   encoder outputs are not interchangeable.

### Larger native-runtime candidates requiring numerical/device validation

8. **Extend useful residency inside RMVPE.** In `rmvpe.rs::run_window`, CNN output is downloaded,
   GRU chunks upload slices and prior hidden states, chunk outputs/final states are downloaded,
   forward/backward arrays are combined on the host, and the output head uploads the combination.
   This is real device/host/device traffic, not a hypothetical raw-stem cache. Keep those values
   on the same model's device until their final consumer while preserving the existing GRU chunk
   boundaries, direction/state resets, compute completion and error propagation. No giant fused
   submission or changed arithmetic is implied. Benefit remains unmeasured.
9. **Reuse graph construction and allocation where shapes permit.** FCPE rebuilds `GraphRun` for
   every fixed-size window. RMVPE rebuilds its CNN/GRU/head runs; Qwen ASR creates and allocates a
   `DecoderRun` on every decode call despite already retaining KV buffers. Start with stable
   shapes/allocators; Qwen's growing history and dynamic views need explicit lifetime handling.
   RoFormer already reuses its graph when frame count matches—do not redo that completed work.
   Verify sequential different inputs and state reset, not only repeated identical inputs.
10. **Reduce Qwen result transfer without changing decisions.** Each ASR step downloads the entire
    vocabulary and then performs host `argmax`. A device-side finite-value check and deterministic
    first-maximum reduction could return only the token, keeping full logits for explicitly
    requested diagnostics. Existing rejection of any non-finite logit, first-index tie behavior,
    token/EOS budgets and synchronization must remain. This is a candidate, not validated parity.
11. **Retain by future benefit, not indefinitely.** The current PCM cache lasts to analysis exit,
    and a prepared worker is consumed then shut down. A future scheduler can retain an exact
    model's weights/compatible fixed graphs for repeated tasks and release audio/intermediates
    after their final consumers. Account for host RAM, GPU graph/weight residency and transfer
    costs; precision-isolated workers must never inherit another model's process-wide policy.
    Do not equate file size plus today's free-memory observation with a guarantee of later fit.

### Output and batch boundaries

- Prefer multiple consumers of one published separation result, not repeated separation or
  repeated encoding. Native temporary F32 estimates can differ from the decoded integer FLAC
  consumed by today's pipeline (rounding/clipping). Bypassing that boundary with raw tensors is
  **not** automatically equivalent. First reuse the decoded published representation; any deeper
  handoff change needs explicit sample/quality comparison, matching MIME and atomic publication.
- Already present and worth retaining: one Leap invocation produces vocals plus residual;
  STARS note/technique share one invocation; STARS/ROSVOT consume shared RMVPE evidence; the
  historical Super trace contains four actual PCM hits and two consumed Qwen-residency reports.
  These are not four new optimizations to implement again.
- Studio's `analyzer/run.rs` processes songs serially, and `analyzer/engine_run.rs` captures and
  atomically publishes artifacts after Engine completion. The 150.865198 s CLI observation does
  not measure that Studio publication tail. Include it in eventual user-visible completion
  measurements. Cross-song hot-model reuse is a later batch optimization, not permission to
  reorder the user's queue, mutate exact requests or schedule model internals from Studio.

**Recommended implementation order:** shared Engine PCM/facts/profile ownership and concurrent
single-producer tests; truthful early preparation progress; explicit whole-task/device scheduling
and cost accounting; then measured frontend/graph/resident-transfer improvements. No model,
precision, context/overlap, fusion threshold, safety measure or GPU launch changed in this audit.

## Implemented after the global audit (2026-09-10)

The user authorized implementing the audited opportunities. These independent changes are now
connected to Super mode; they do **not** establish whole-model GPU scheduling or measured speedup:

- `6a5ce26`: shared Engine/worker `uta-audio-reuse` PCM ownership, cross-process single producers,
  immutable completed publication, failure/cancellation wakeups and child reaping. Engine facts,
  metrics and signal profiles survive artifact renames within the request. First-stream and
  automatic stream selection remain distinct unless a single audio stream is established.
- `f6275f2`: task-owned STFT/ISTFT, FCPE and Qwen FFT/window/filter/scratch reuse. Differing sequential
  CPU inputs remain bit-identical to ordinary preparation; scratch does not leak between requests.
- `5c39192`: Qwen reports real completed windows, enabling earlier preloading on multi-window work.
  No synthetic work units or changed windows/overlap. Single-window execution still has no such
  early completed window; allocation-phase preparation remains future work.
- `2fb6d01`: owned Acoustic DSP overlap, inherited audio/event context, parent-linked cancellation,
  failure-cause preservation and join-before-cleanup. This is CPU/model overlap, not dual-GPU work.
- `9ce129e`: acceleration snapshots share one request-owned context across joined tasks. Pending
  preparation is reaped before the last owner removes PCM storage; nested disabled scopes suppress
  inheritance. Sixteen supervisor/context tests, four task-owner tests and two prediction tests
  passed at `20260910T121202-985dcc11e7d1`. The preload cursor and foreground lease remain serial.
- `4d8db66`, fixture correction `e1d5eac`: FCPE reuses its fixed-shape graph within the model/backend
  lifetime, overwriting every input before synchronous compute/readback.
- `c04f079`: optional RMVPE CNN → chunked GRU → head resident handoffs on the existing assigned
  backend. Exact-size GRU graphs are reused within a window; directions, chunk order and +0 hidden
  resets are unchanged. Allocation completes before compute, so unavailable optional allocation
  may use ordinary execution on that same backend. Compute failures are not retried. The CNN arena
  is released after compact capture; recurrent graphs are released after their final consumer.
- `6df2a5e`: Qwen incremental-token context/allocator reuse with a freshly rebuilt growing-history
  graph. The larger prefill arena is not pinned; cached graph views drop before session KV storage.
  Full logits and the existing greedy decision rule remain in use.

Executed CPU evidence includes shared-cache concurrency/failure/cancellation tests, ordinary/Super
frontend comparisons, worker/progress tests and ownership/supervisor tests. Native CPU primitive
checks at `20260910T113607-45fdcc9232e3` passed the FCPE input-refresh and resident-copy/lifetime
fixtures; `20260910T115838-e7354a6bc379` passed Qwen/worker tests and a changing-shape native CPU
arena-reset fixture. These tiny primitives alone are **not** complete-model numerical qualification.

Subsequent weighted CPU fixtures (`dd0074b`, operation `20260910T123226-4407a8977033`, observations
`test-artifacts/super-acceleration/weighted-cpu-observation/`) compared **470,520 finite values
bit-for-bit** against fresh ordinary execution using read-only installed GGUF files: RMVPE 253,440
values over 256/192/256-frame inputs, including a real short GRU tail and different successive mel
inputs; FCPE 217,080 values over three different windows with the same cached graph. Tests assert
actual residency and graph reuse, not successful fallback. Vulkan adapters were enumerated by the
loader, but both tests explicitly created the **CPU** backend. These are bounded weighted-window
numerical checks, not GPU, full-song/perceptual, performance or host-stability qualification.
Native pointer metadata borrows were also scoped to their copy in `39984f0`. Targeted CLI tests and
changed Rust formatting passed at `20260910T123921-8f13ed9beb0b`.

Preserved failed records: `20260910T111830-19e0273fea8e` stopped at Cargo lock resolution during
concurrent LibTorch manifest edits; `20260910T113503-63b8228d7ca3` stopped at a test-only unbound
`ggml_scale` call, corrected with the already bound add operation. Neither reached native tests.
An earlier supervisor fixture encountered `ETXTBSY`; subsequent explicitly recorded serial checks
passed. No automatic retry or GPU inference is implied by these follow-up operations.

Implemented complete-model scheduling uses three dependency phases (audio preparation,
independent evidence and conditioned evidence), measured complete-task seed costs, predicted lane
availability and dependency-ready time. It resolves each planned model against the resulting total
schedule, preferring LibTorch XPU/B580 for heavy work and GGML Vulkan/AMD integrated GPU for ready
light work; automatic route availability may select the other GPU backend, never CPU. Diagnostics
record requested mode and predicted placements separately from `dual_device_work_measured`, which
remains false without device telemetry. The current preload coordinator still has one shared
pending slot; queue-aware multi-pending residency, persisted/revised runtime observations and
observed phase/cost accounting remain. Reduced Qwen readback must preserve exact
first-maximum/non-finite semantics. Source review
found that pinned Vulkan `argmax.comp` resolves equal maxima by reduction lane, not always original
index (equal maxima at indices 1 and one subgroup-width can select the latter). Direct substitution
for host argmax is therefore invalid. Do not change token selection merely to reduce readback.
Cross-song reuse and Studio publication timing remain separate batch/end-to-end boundaries.

Current CPU/protocol verification: combined operation `20260911T155605-0e383e3b32f1` passed
282 Analysis Engine unit tests, four packaged-boundary tests, 438 app-core tests (one ignored) and
242 desktop tests. Coverage includes per-lane serialization, cross-lane admission, dependency/cost
placement, inherited task context, cancellation/cleanup, exact-request projection, disabled manual
controls and save-before-visible-state behavior. These checks did not launch either GPU. Targeted
clippy operation `20260911T155349-b726a85e2197` stopped on two pre-existing warnings in
`app-core/src/backend_cli/process.rs`; no scheduler warning preceded that blocker.

The concurrently authorized independent LibTorch work is preserved. Shared host DSP entry points
must not call GGML model graphs; scheduler routing must respect the selected backend and precision.
Vulkan Super/full-pipeline experiments remain paused, saved pre-correction binaries remain
unsuitable, and no production promotion or corrected end-to-end performance claim is made. A later
explicit request resumed separate bounded native LibTorch XPU tests, recorded in
[LibTorch execution](LIBTORCH_EXECUTION.md); these do not qualify Super scheduling.

## Correction verification

Operation `20260910T095055-5df40fd72c78` on `844c016`: **53 CPU / isolated protocol tests passed**
(four RoFormer, 34 worker, thirteen supervisor and two preload-prediction tests). These cover
single-owner chunk processing, immediate failure propagation, preparation reuse, exact-format
cache behavior and cancellation/cleanup. No GPU inference was run, no release binary was rebuilt,
and no numerical, throughput or host-stability qualification of the corrected design is claimed.
Product identity scan passed (`20260910T095215-585b34c730c4`); changed source files remain under
2,000 lines. This is historical correction evidence; the later complete-model scheduler retains
those ownership and cleanup properties.

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
