# B580 XE90 attention: measured defaults and remaining gap

Current default and measurements: [query-owned H64 attention](ROFORMER_B580_QUERY_OWNED.md).
Patch 0007 supersedes the six-patch long-query default below with four query-owning SIMD32
subgroups. The final same-library model pair measures 5.29 to 8.12 TFLOPS; cross-version
output is numerically close, not byte-identical. The sections below preserve prior-step evidence.

## Previous six-patch scope and accepted change

Measured on 2026-09-10 JST (operation records use 2026-09-09 UTC), Intel Arc B580,
Mesa 26.2.2, pinned GGML `8c63e70982c95ceb862e3a1073a2c1beef75d60a`.
This is targeted kernel/model validation, not whole-song or release qualification.
No 20-TFLOPS attention result or same-shape XPU comparison is claimed.

The preceding six-patch recipe includes the separately validated local score/value reuse
patch from `a9e6b5480c7f27065375197597f371e3f77bf668`. Its measurements and exact
output comparison are recorded under **Local score and value reuse** below.
The subgroup comparison in the first sections remains a separate prior step.

Commit `a34c69200557c69105031c477e94cb58bd735f2b` keeps the measured sixteen-query
setting and stops forcing a sixteen-lane subgroup. The sixteen-query default was
already an in-progress workspace correction; the subgroup correction was measured
against a matched library with the same query tile. Both belong together in the
runtime recipe. No installed model or runtime was replaced.

That six-patch Intel Xe2/M8/F16-KV/H64/F32-accumulator baseline uses:

- 16 query rows, 32 key columns and four subgroups;
- the native device subgroup (32 lanes on the measured B580);
- unchanged full attention context, masks and F32 accumulation.

`UTA_STUDIO_GGML_FA_SMALL_SUBGROUP` explicitly requests the diagnostic sixteen-lane
subgroup when supported. The existing `UTA_STUDIO_GGML_FA_SG32` override takes
precedence and retains the native subgroup. `UTA_STUDIO_GGML_FA_QUERY_ROWS` keeps
16/32/64 query-tile experiments available. `UTA_STUDIO_GGML_FA_STAGE_KV` remains an
explicit staging experiment. No new device fallback or precision downgrade was added.

## What caused the actionable loss

The existing M8 cooperative-matrix patch already enables B580 matrix-engine
attention; this investigation is about the remaining cost after that patch.
The subsequent subgroup patch assumed that fewer inactive output lanes would
make SIMD16 faster. The matched measurements contradict that assumption.

Larger query tiles are also not an automatic improvement. Recorded compiler
outputs for sixteen-query variants reserve 15,216 bytes of shared memory, versus
24,752 bytes for thirty-two-query variants. Some larger variants also spill
registers. This is compiler evidence, not a measured hardware occupancy counter.
See `test-artifacts/xe90-attention-breakdown-20260910/compiler-stats-query-16/`
and `compiler-stats-query-32/`. Preserve the smaller measured default rather than
assuming more arithmetic per workgroup must be faster.

## Matched attention measurements

Evidence root: `test-artifacts/xe90-attention-review/`.
`validated-summary.json` contains numerical summaries and sampled host conditions;
each case preserves its full command, stdout/stderr, host samples and completion.

Both shapes use head dimension 64, eight heads, interleaved F32 Q, F16 K/V and F32
accumulators. Time attention has 1,722 queries/keys and 90 independent batches;
frequency attention has 90 queries/keys and 1,722 independent batches. The workload
is not shortened or approximated. FLOPs count QK and PV, with two FLOPs per FMA.
The time-shape numerator is 546,561,146,880 FLOPs; frequency is 28,565,913,600.

Every shape performs eight warmups followed by eight measured calls. The table
uses GPU timestamps, not initialization, compilation, upload or reference-check time.
All three same-source libraries were built with the same toolchain/options.

| Configuration | Time attention, mean +/- SD | Effective TFLOPS | Frequency attention, mean +/- SD |
| --- | ---: | ---: | ---: |
| 16 query rows, forced subgroup 16 | 145.630 +/- 1.207 ms | 3.753 | 9.885 +/- 0.428 ms |
| 16 query rows, native subgroup 32 | 110.230 +/- 0.933 ms | 4.958 | 8.858 +/- 0.165 ms |
| Native subgroup plus redundant-barrier experiment | 108.830 +/- 0.936 ms | 5.022 | 8.756 +/- 0.107 ms |
| Native subgroup plus shared-scratch experiment | 110.758 +/- 1.024 ms | 4.935 | 8.835 +/- 0.114 ms |

The subgroup comparison reduces time-attention latency by about 24.3%, increasing
effective throughput by about 32.1%. It does not multiply whole-model performance
by that amount. Frequency attention improves by about 10.4% in this pair.

After rebuilding the accepted patches, `final-default-shapes` explicitly removed
the tuning environment variables and measured 109.735 +/- 1.675 ms (4.981 TFLOPS)
and 9.028 +/- 0.669 ms respectively. The frequency outlier is retained in the mean.
This confirms the actual compiled default, not just a fast environment override.

## Real XE90 graph and audio comparison

The same installed F32 GGUF was read without modification:
`bs_roformer_leap_xe90_vocals/generations/87e37631565339a3da4562a2b5a28908d6bd19622ce025241e2fb454c3b53e60/bs_leap_xe_voc-F32.gguf`.
The existing six-second stereo input is recorded in both case commands.
`attention_model_tests::b580_roformer_model_comparison` executes the production
Rust RoFormer graph, WAV frontend and synthesis twice after loading the model.
Worker IPC, FFmpeg publication and output encoding are outside this timer.
F32 matmul promotion was not enabled for this comparison.

| Configuration | First processing pass | Second processing pass | Within-configuration repeat maximum difference |
| --- | ---: | ---: | ---: |
| Forced subgroup 16, query rows 16 | 6.880 s | 6.449 s | 0 |
| Native subgroup 32, query rows 16 | 6.626 s | 6.024 s | 0 |

The observed second-pass reduction is 6.6%, not a guaranteed whole-song speedup.
This is one clip and two passes per configuration. Host sampling did not observe
another CCS compute client during these cases, but CPU load varied substantially,
including a short near-100% aggregate CPU sample in the native-subgroup case.
The more tightly controlled GPU-timestamp kernel comparison is the stronger
performance evidence. No exclusive-device or fixed-clock guarantee is implied.

All 529,200 output samples (264,600 stereo frames, exactly 6 seconds at 44.1 kHz)
were compared, not just an audio thumbnail:

- maximum absolute difference: 0.0003051832318305969;
- RMSE: 0.00003226768875107856;
- relative L2: 0.00020538566462359473;
- SNR relative to the subgroup-16 output: 73.74859744575193 dB.

Outputs are finite and repeat exactly within each configuration, but are not
bit-identical across subgroup configurations. This is numerical regression evidence,
not a perceptual-quality or full-song parity certification.

## Why the remaining gap cannot be assigned to one switch

The primitive controls use actual attention matrix geometry rather than a large
square GEMM. The long case microbatches eight independent bands to bound memory;
it still keeps all 1,722 keys. These are standalone GGML primitives, not a replacement
production graph, XPU measurements or direct full-batch attention speedups.
Sixteen warmups and eight timed host compute calls were used.

| Primitive | Sequence | Independent batches | Mean host compute | Effective matmul TFLOPS |
| --- | ---: | ---: | ---: | ---: |
| QK | 1722 | 8 | 5.580 ms | 4.353 |
| Softmax | 1722 | 8 | 8.890 ms | Not a matmul metric |
| PV | 1722 | 8 | 4.230 ms | 5.743 |
| QK | 90 | 1722 | 5.993 ms | 2.383 |
| Softmax | 90 | 1722 | 7.290 ms | Not a matmul metric |
| PV | 90 | 1722 | 4.411 ms | 3.238 |

QK has reduction depth 64; frequency PV has reduction depth 90. Their reuse and
memory traffic differ from a high-throughput large GEMM. Materializing probabilities
also makes standalone softmax expensive. These measurements show that GGML's
unfused matrix primitives at this geometry are not delivering 20 TFLOPS either;
they do not establish a hardware ceiling or rule out a faster same-shape XPU kernel.

The cooperative shader still crosses shared memory between matrix fragments,
row-wise softmax and the value product, with repeated workgroup synchronization
and online output rescaling. A major further gain needs better shape-specific work
partitioning, operand reuse and matrix-to-softmax handoff, not merely one fewer
barrier. A same-shape, same-precision, same-timing-boundary XPU control remains needed
before attributing the entire 5-versus-20 gap to GGML implementation efficiency.
FlashAttention-2 (arXiv:2307.08691) provides relevant general work-partitioning
rationale; its NVIDIA performance numbers are not evidence about this B580.

## Experiments not enabled in the runtime recipe

`experiments/attention-single-value-tile-barrier.patch` removes a duplicate value-stage
barrier only when the single HSV tile performs no intervening shared-memory staging.
It retains tail, decoded/misaligned and multi-HSV-tile synchronization. The measured
roughly 1.3% gain is too small in this sample to justify promoting it as a substantial
optimization without wider shape and repeated model evidence.

`experiments/attention-score-output-sharing.patch` aliases disjoint F32 score/output
scratch lifetimes across existing barriers. It reduces declared scratch allocation
but did not improve measured time attention. It is not enabled. Its first compile
attempt failed because GLSL specialization-sized arrays cannot use `max()` as their
extent here; the committed experiment uses a conditional specialization expression.
Neither experiment changes model weights or attention arithmetic.

## Measurement integrity and verification

The initial `control-shaped` case overlapped another GPU job: sampled external CCS
activity reached about 92%, and time attention averaged 411.031 ms. It is retained
as interference evidence and excluded from the optimization comparison. Subsequent
compared shape cases sampled no external CCS activity; DRM visibility remains partial.
No existing job was stopped to obtain a favorable result.

The accepted shape cases check all 79,349,760 outputs per shape for finiteness and
compare 768 sampled output values to an f64 reference using complete key context.
Time-shape NMSE is 6.7433e-10 and maximum reference error is 5.6840e-5; frequency
NMSE is 7.3864e-9 and maximum error is 1.8827e-4. Seven additional fixtures cover
31/32/33-key boundaries, padded strides, head dimension 72, a causal mask and
1,722 keys. The rebuilt default and both isolated shader experiments pass them.

Focused `cargo test --locked -p uta-ggml-runtime -p uta-ggml-worker` passes:
108 runtime tests and 30 worker tests, zero failures; 25 opt-in tests are ignored
by the ordinary suite and are not claimed as automatically executed. Relevant B580
opt-in tests were invoked separately and retain operation records.

All observed case completions retain the same boot ID. This establishes no reboot
during the recorded runs, not future host stability. AMD/NVIDIA validation, full-song
performance, perceptual evaluation and packaged/Nix release acceptance are not part
of this measurement. Raw diagnostic libraries remain under
`test-artifacts/xe90-attention-review/runtime-final/lib`; user installations are unchanged.

## Local score and value reuse

Commit `a9e6b5480c7f27065375197597f371e3f77bf668` adds
`native-inference/ggml-worker/patches/0006-vulkan-m8-attention-local-tile-reuse.patch`
to `runtime-recipe.json`. This is an upstream-backend shader change, not a new
model implementation. The installed models, installed runtime and input media
were not changed.

The specialization applies only to M8 cooperative attention with F16 K/V, F32
accumulation, HSK=HSV=64, Br=16, Bc=32 and four 32-lane subgroups. Other shape/type
combinations use the existing shader branch. It makes two local changes:

- Keep a packed vector of four query scores between maximum reduction and
  exponentiation, instead of separate source-level scalar and vector reads from
  shared memory. Rescaling temporaries are scoped to those four query rows.
- Load each V cooperative-matrix operand once for both M8 query-row products,
  rather than repeating that load inside each query-row tile. Each accumulator
  still visits key chunks in the original order.

Full attention context, F16 probability rounding, F32 running maxima, normalizers
and output accumulators are preserved. No workgroup barrier is removed. These
changes reduce redundant data movement without enlarging the query tile or
lowering precision. Actual hardware stall percentages and SLM transaction counts
were not measured; the source change and controlled latency comparisons establish
the improvement, not an inferred hardware-counter attribution.

### Isolated kernel measurements

Evidence root: `test-artifacts/xe90-pv-reuse-study/`. `measurements.json` preserves
the individual GPU samples and model results, and each run directory retains its
command, stdout/stderr, host observations and completion record. Each shape has
eight warmups and eight measured calls, using the same geometry and FLOP counts
specified above. Every measured call, including outliers, is retained.

| Configuration | Time attention, mean +/- SD | Effective TFLOPS | Frequency attention, mean +/- SD |
| --- | ---: | ---: | ---: |
| Fresh native-SIMD32/query-16 control | 111.069 +/- 2.123 ms | 4.921 | 8.796 +/- 0.010 ms |
| V-operand reuse only | 107.130 +/- 1.872 ms | 5.102 | 8.887 +/- 0.471 ms |
| V reuse plus packed softmax prototype | 97.986 +/- 1.684 ms | 5.578 | 7.946 +/- 0.017 ms |
| Return to unchanged control | 109.869 +/- 1.594 ms | 4.975 | 8.944 +/- 0.414 ms |
| Rebuilt six-patch default, tuning overrides unset | 95.898 +/- 0.689 ms | 5.699 | 7.948 +/- 0.016 ms |

The final row uses `runtime-default/lib` and explicitly removes the subgroup,
query-row, KV-staging and F32-matmul-promotion tuning variables. This tests the
compiled default rather than an environment-only fast path. The final source
patch and recipe snapshot are preserved alongside the libraries. The earlier
unscoped prototypes remain diagnostic experiments, not additional recipe patches.

The packed-prototype pre-run snapshot overlapped heavy CPU compilation; its GPU
timestamps are retained, but its host time is not treated as a clean comparison.
Final-default pre-run CPU load was about 1.24%, with B580 idle before the run.
Neither observation establishes exclusive use or fixed clocks.

### Real XE90 graph and complete output comparison

The same installed F32 GGUF and six-second stereo fixture described above ran
through the production Rust Roformer graph twice per configuration. Both model
pre-run snapshots had low CPU load (about 1.1% and 1.3%) and no measured B580 load.
F32 matmul promotion was not enabled.

| Measurement | Native-SIMD32/query-16 control | Scoped local reuse |
| --- | ---: | ---: |
| First processing pass | 5.844483 s | 5.698333 s |
| Second processing pass | 5.784859 s | 5.577105 s |
| Mean time-attention kernel across both passes | 108.626500 ms | 96.237150 ms |
| Mean frequency-attention kernel across both passes | 8.784460 ms | 7.923345 ms |
| Time-attention effective throughput | 5.032 TFLOPS | 5.679 TFLOPS |
| All 32 attention kernels per processing pass | 1.878575 s | 1.666568 s |

Time-attention latency decreases by 11.41%, with a 12.87% throughput increase;
frequency-attention latency decreases by 9.80%. The observed second processing
pass is 3.59% shorter. Those are separate measurements: the attention improvement
must not be presented as a whole-model or whole-song percentage. The model timer
includes WAV frontend/synthesis but excludes loading, worker IPC and FFmpeg
publication/encoding. There are only two processing passes per configuration.

`audio-comparison.json` compares all four complete WAV files. Both cross-version
pass comparisons and both within-version repeats are byte-identical, including
all 529,200 F32 samples per output. Each file is 2,116,868 bytes. This is stronger
than a sampled numerical comparison for this fixture, but is not a broad
perceptual-quality or full-song parity claim.

A separate final-recipe model run (`model-default`) then removed all five tuning
overrides and completed two passes in 5.622703 s and 5.573595 s. Both complete
outputs are also byte-identical to the corresponding control outputs, recorded
as `default_audio_comparison` in `measurements.json`. This confirms the rebuilt
runtime default rather than relying only on the earlier scoped experiment;
these later timings do not replace the matched pair above.

### Final verification and reproducibility limits

The final-default seven-case tail/mask/stride numerical test passes. Maximum NMSE
across those cases is 1.7164e-8. Both large synthetic shapes retain the reference
errors reported above, and every output is checked for finiteness.

The post-integration focused command
`bash dev.sh -c cargo test --locked -p uta-ggml-runtime -p uta-ggml-worker -j2`
passes 108 runtime tests and 30 worker tests, with 25 opt-in tests ignored by that
ordinary suite. Relevant B580 tests were invoked separately. The test operation
record is `test-artifacts/operations/20260909T191005-00a842230127/`.

All recorded test completions have exit code zero and unchanged boot IDs. Two
long-build calls produced duplicate execution records during connector response
failures; the original default build completed successfully, while its duplicate
stopped before compilation because the source was already patched. Both records
are retained rather than treating the failed duplicate as a failed GPU test.

The compiled diagnostic runtime is
`test-artifacts/xe90-pv-reuse-study/runtime-default/lib`. No installation or
whole-workspace/Nix release pass was performed. This work does not establish a
20-TFLOPS attention result, a same-shape XPU comparison, full-song performance,
AMD/NVIDIA regression coverage, or long-term host stability. The remaining gap
still calls for shape-matched XPU controls and a more substantial redesign of
matrix/softmax work partitioning, not indiscriminately larger tiles.
