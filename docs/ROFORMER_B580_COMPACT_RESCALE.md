# B580 fused attention: independent compact row scales

## Scope and implementation

This follow-up starts from the nine-patch backend documented in
[the floating-storage study](ROFORMER_B580_FLOATING_STORAGE.md), not the earlier
8.12-TFLOPS seven-patch result. The isolated evidence directory is
`test-artifacts/attention-compact-rescale-study/`.

The accepted change is declared as
`native-inference/ggml-worker/patches/0010-vulkan-attention-compact-row-scales.patch`.
Its identical experimental patch was committed as `2a39642`. Each subgroup
publishes eight F32 row scales in a dedicated shared array, then loads an
8-by-16 cooperative broadcast matrix using a zero-stride column-major load.
This replaces the previous 128 scalar scale writes. Separating scales from
live score storage removes one subgroup barrier in each key-block iteration;
probabilities and scales are published together by the remaining barrier.
Final normalization uses the same independent storage.

No matrix-component-to-lane mapping is assumed. The scale values, F16 operand
and probability rounding, F32 online state and output accumulators, blockwise
addition order, full key context, masks, tails and tensor addressing are
unchanged. No global probability matrix or conversion tensor is introduced.

Shared-memory accounting changes from 2,560 to 2,592 bytes per subgroup, plus
the existing 32-byte common table. Both workgroup selection and support checks
include those additional bytes. Eight subgroups use 20,768 bytes; four use
10,400 bytes. This does not reuse the out-of-bounds upper score-scratch area
from the older row-scale experiment.

Eligibility remains tensor/device-based: the existing Intel Xe2 native-SIMD32
M8 H64 F32-accumulator query-owned path, with independently F16 or F32 K/V.
There is no XE model-name whitelist. Other GPU vendors, head dimensions,
small-query/GQA paths and unsupported storage types are not newly selected.

## Reproducible build and numerical evidence

The control libraries are
`test-artifacts/attention-query-residency-study/runtime-combined/lib`.
The candidate libraries are `runtime/lib` under this study. They were built
from the pinned upstream checkout plus nine declared patches and the compact
row-scale patch. Build operation
`test-artifacts/operations/20260910T045214-b2da9caddff1/` completed with exit zero
in 116.201 seconds, using `bash dev.sh` and two build jobs.
`source-compact/` preserves that source separately from subsequent experiments.
The test executable is the existing
`test-artifacts/attention-query-residency-study/attention-tests-storage`.

All 61 explicit numerical fixtures pass with default eight-subgroup selection:
30 query-owned tail/mask/stride/GQA/rescale/boundary cases, 24 floating or mixed
K/V cases, and seven original short-query/H72 fallback cases. Their records are
`query`, `floating` and `fallback`. The same 61 fixtures pass with
`UTA_STUDIO_GGML_FA_SHARED_GROUPS=4`, recorded as `compact-four-query`,
`compact-four-floating` and `compact-four-fallback`. These are 122 executed
fixture instances, not 122 distinct tests. Existing error tolerances are not
relaxed. The padded experiment's separate 61 passing fixtures are not counted
as acceptance of that experiment's performance.

## Paired isolated kernel measurements

Each shape uses eight warmups and eight measured GPU calls. Time attention is
H64 with 1,722 queries/keys, eight heads and 90 batches; frequency attention
exchanges the 1,722 and 90 dimensions. FLOP accounting covers QK and PV only:
546,561,146,880 and 28,565,913,600 FLOPs respectively. GPU timestamps cover the
complete fused attention kernel. All samples, including warmups and outliers,
are retained in `shape-summary.json` and the original case logs.

| Run order | Time attention mean +/- sample SD | Effective TFLOPS | Frequency attention mean +/- sample SD |
| --- | ---: | ---: | ---: |
| Nine-patch control, first | 43.9159 +/- 1.8249 ms | 12.4456 | 5.1004 +/- 0.0044 ms |
| Compact scales, first | 42.6878 +/- 0.2842 ms | 12.8037 | 5.0688 +/- 0.1348 ms |
| Compact scales, return | 42.4743 +/- 0.2096 ms | 12.8680 | 5.0027 +/- 0.0054 ms |
| Nine-patch control, return | 43.0103 +/- 0.2957 ms | 12.7077 | 5.0982 +/- 0.0081 ms |

The first control is noisier. The approximately 1.2 percent reduction against
the return control is a small kernel improvement, not evidence for a major
throughput breakthrough or a whole-model percentage. The pre-series snapshot
showed about 0.90 percent aggregate CPU utilization and zero B580 utilization.
Every observed invocation retains host samples; these snapshots and partial
DRM observations do not establish isolation, fixed clocks or exclusive use.

## Compiler evidence is not hardware-counter evidence

Separate `compiler-control` and `compiler-compact` invocations set
`INTEL_DEBUG=cs` and disable the Mesa shader cache. Their timings are not used
above. For the tested F16-K/V SIMD32 shader, the compiler reports:

| Diagnostic | Nine-patch control | Compact scales |
| --- | ---: | ---: |
| Instructions | 2,479 | 2,475 |
| Spills / fills | 20 / 112 | 18 / 122 |
| Sends | 240 | 230 |
| GRF registers | 128 | 128 |

Spills decrease but fills increase. This is neither zero-spill execution nor
a measurement of hardware stall percentages. Static compiler counts alone
do not establish the cause of a timing change.

## Real models and complete audio comparison

Each configuration executes two complete passes of the same six-second,
44.1-kHz stereo fixture through the production Rust RoFormer graph, frontend
and synthesis. The process timer excludes model loading, worker IPC and final
codec/publication. XE90 runs control then candidate; Harmony runs candidate
then control; Denoise runs control then candidate. Cases are `xe-control`,
`xe-compact`, `harmony-control`, `harmony-compact`, `denoise-control` and
`denoise-compact`. `model-summary.json` retains the numerical timings below.

| Model | Second-pass time attention, control -> compact | Frequency attention, control -> compact | Complete process seconds, control -> compact |
| --- | --- | --- | --- |
| XE90 | 43.4468 -> 43.1572 ms | 5.12176 -> 5.03903 ms | 6.910269 -> 6.926076 |
| MelBand Harmony | 8.68573 -> 8.62001 ms | 1.24443 -> 1.24331 ms | 4.709962 -> 4.756379 |
| MelBand Denoise aufr33 | 8.91824 -> 8.44163 ms | 1.27145 -> 1.27039 ms | 4.748766 -> 4.728994 |

The candidate's real XE90 attention is 12.6644 TFLOPS in this pair. The isolated
12.8680-TFLOPS result must not be substituted for a real-model measurement.
The model pairs show small attention improvements, but no consistent
whole-model speedup: XE90 and Harmony second-pass process time increases by
about 0.23 and 0.99 percent, while Denoise decreases by about 0.42 percent.
There are only two passes per configuration, without fixed clocks.

For each model, both complete candidate WAVs are byte-identical to their
corresponding control WAVs, and the candidate repeats byte-identically.
Each output contains 529,200 finite F32 samples. Complete comparisons are
recorded by `test-artifacts/operations/20260910T051039-31ced3d5542e/`; they are
not sampled waveform comparisons. This preserves the nine-patch result; it
does not retroactively make that earlier backend bit-identical to older
implementations or establish perceptual quality on other inputs.

## Rejected padded-handoff experiment

`attention-padded-handoff.patch`, committed as `b701cec` and corrected as
`d4ce583`, adds row padding to scores and probabilities after compact scales.
All 61 numerical fixtures pass, but the layout is not promoted. The time-shape
ABBA series is 42.6283, 53.7714, 53.8461 and 42.7646 ms for compact control,
padded candidate, padded return and compact-control return. Corresponding
frequency means are 5.1241, 5.7783, 5.7483 and 5.5276 ms. Raw observations and
all samples are retained in `handoff-timings.json` and the four case directories.
The pre-series B580 snapshot showed 11 percent utilization; frequency timing
is visibly less stable. These results reject this layout, not prove a specific
bank-conflict or stall-counter explanation.

The rejected library remains in `runtime-padded/lib`; the accepted library
remains in `runtime/lib`. The initial malformed patch failed to apply before
execution. A connector replay of the later commit/apply/build command stopped
at an already-completed commit; the original build completed successfully in
`test-artifacts/operations/20260910T050225-289ff8b83b13/`. Failed and duplicate
operation records are retained. No GPU test was automatically replayed.

## Remaining scope

Twenty TFLOPS is not reached. The current XE90 time shape would require about
27.3281 ms under the same FLOP convention. Remaining work concerns live
cooperative-fragment state, spill/fill traffic and the score/probability
handoff within fused attention, not materializing a global probability matrix.

Installed runtimes, model weights and source media were not replaced or
modified. No remote push, whole-song validation of this change, Windows
execution, AMD/NVIDIA regression or formal Nix release acceptance was performed.
Unchanged boot IDs and process exit zero do not establish post-exit host
stability. Integration readiness is not production readiness.

## Declared-source and ordinary regression closure

Backend commit `0321ca9` declares the compact-only change as patch ten.
`test-artifacts/operations/20260910T051509-c209f43d3767/` applies all ten recipe
patches to a fresh checkout of the pinned GGML commit. Every affected source
file, including the newly added query-owned shader, matches `source-compact`
byte-for-byte. The result is retained in `declared-source.json`; this checks
the measured source against the recipe without substituting the rejected
padded library.

`test-artifacts/operations/20260910T051531-fade6e5c510a/` runs the focused
`cargo test --locked -p uta-ggml-runtime -p uta-ggml-worker -j2` command through
`bash dev.sh`: 110 runtime tests and 30 worker tests pass. The ordinary command
ignores 31 explicit runtime tests. The separately executed GPU fixture groups
above are not represented as having run automatically in that ordinary suite.
