# B580 H64 attention: query-owned subgroups

The subsequent eight-subgroup/compact-scratch optimization is documented in
[Fragment reuse and shared workgroups](ROFORMER_B580_FRAGMENT_HANDOFF.md).
The measurements below remain the separate seven-patch investigation.

## Accepted implementation and scope

Measured on 2026-09-10, Intel Arc B580, Mesa 26.2.2, pinned upstream GGML
`8c63e70982c95ceb862e3a1073a2c1beef75d60a`. Implementation commit: `e0d1b92`.
This is bounded kernel and six-second model validation, not a release or full-song qualification.

The runtime recipe now includes
`native-inference/ggml-worker/patches/0007-vulkan-query-owned-attention.patch`.
It follows the six patches documented in [the preceding investigation](ROFORMER_B580_ATTENTION.md).
This is a specialized fused QK/softmax/PV backend kernel, not a replacement model,
standalone model executable, or a precision-reduced GEMM route.

The new default is limited to Intel Xe2, M8 cooperative matrices, F16 K/V,
HSK=HSV=64, F32 accumulators, native SIMD32, and more than 32 query rows.
Four subgroups own eight query rows each, so one workgroup covers 32 queries.
Other devices, types, head sizes and small-query paths retain their previous implementation.
`UTA_STUDIO_GGML_FA_QUERY_OWNED=0` selects the prior kernel for a controlled comparison.
Explicit query-row or KV-staging diagnostics also retain the prior implementation.
The exploratory `OWNED_GROUPS` and `OWNED_WIDTH` controls are not exposed by the accepted patch.

No installed model, runtime, or source media was changed. The final runtime was built into
`test-artifacts/xe90-subgroup-handoff-study/runtime-final` through the existing build script.

## What changed

The prior shader partitions QK by key rows, softmax by query rows, and PV by output columns.
That requires workgroup-wide exchanges between different owners and sends each PV result
through shared memory before updating the scalar/vector output accumulator.

The new kernel keeps ownership of each eight-query group across all three stages:

- Q is scaled and rounded to F16 exactly as before, then four cooperative A fragments are cached.
- Four neighboring SIMD32 lanes share each softmax row. Local packed reads plus two XOR shuffles
  perform row maximum and final sum reductions without keeping eight row states in every lane.
- Four F32 cooperative output fragments remain live through the complete key loop. Online
  rescaling uses a row-broadcast cooperative matrix; PV products are added directly to these fragments.
  PV is no longer stored to and reloaded from a separate shared output buffer after every key block.
- Coalesced K and V tiles are shared by the four query-owning subgroups. The query scratch is reused
  for K/V after the invariant query fragments have been loaded. Workgroup barriers protect these
  shared operands; subgroup barriers protect per-subgroup score/probability/rescaling storage.

The shader does not assume the implementation-dependent mapping of cooperative-matrix components
onto individual lanes. It uses cooperative loads/stores and explicit shared layouts at that boundary.
Score/probability handoff still uses shared memory; this is not a register-only softmax implementation.

Complete key context, masks, tail checks, F16 probability rounding, F32 online state and F32 output
accumulators are preserved. Reduction order changes, so cross-version bit identity is not promised.
The dispatch also preserves the original 16-row M8 GQA capacity: the new 32-query tile must not enlarge
an unrelated small-query grouped-head dispatch. Seven new grouped-head/broadcast fixtures cover this.

## Kernel measurements and experimental progression

Evidence root: `test-artifacts/xe90-subgroup-handoff-study/`.
`verification.json` contains selected raw GPU samples, numerical results, model results, host-load
summaries and completion status. Individual case directories retain the complete command and logs.
Compiler diagnostic runs are not substituted for normal timing runs.

Both real shapes use H64, eight heads and interleaved F32 queries with F16 K/V.
Time attention has 1,722 queries/keys and 90 independent batches; frequency attention has
90 queries/keys and 1,722 batches. There are eight warmups and eight measured calls per shape.
The effective FLOP count is QK plus PV, with two operations per FMA:
546,561,146,880 FLOPs for time attention and 28,565,913,600 for frequency attention.
GPU time covers the complete fused kernel, including non-matmul operations.

| Experiment | Time attention mean | Frequency attention mean | Disposition |
| --- | ---: | ---: | --- |
| Six-patch control | 96.788 ms | 7.943 ms | Prior default |
| Initial four-subgroup query-owned prototype | 147.232 ms | 13.031 ms | Rejected; excess live state/spills |
| Four-lane row ownership | 108.240 ms | 8.929 ms | Intermediate |
| Shared coalesced K/V, SIMD32 | 63.725 ms / 8.577 TFLOPS | 6.104 ms | Accepted algorithm |
| Shared coalesced K/V, SIMD16 | 76.492 ms | 7.471 ms | Not selected |
| Simple transposed V staging | 93.956 ms | 7.550 ms | Not selected |

The direct V transpose reduced compiler-reported spills from 39:142 to 26:107 and reduced sends,
but made global loading less favorable and did not improve measured latency. A four-lane shuffle
transpose also passed the numerical fixtures without beating the selected implementation.
These are negative experiments, not additional default patches.

The selected SIMD32 shader still reports 39 spills and 142 fills, 128 GRF registers, and
14,368 bytes of shared memory. These are compiler diagnostics, not hardware stall counters.
The first prototype reported 94 spills and 331 fills. Fewer instructions or spills alone did not
predict the best latency; operand sharing and global-access patterns also mattered.

## Rebuilt default versus opt-out in the same library

A fresh pinned checkout was built with all seven recipe patches using
`native-inference/ggml-worker/build-ggml-runtime.sh`, not merely by changing environment variables
against an old binary. Build operation: `20260910T023559-93c71ec1a59a`, exit code zero.
The subsequent duplicate build invocation stopped at the already-existing clone directory;
it did not overwrite or invalidate the successful build. Both operation records are retained.

All tuning overrides were unset for default cases. Control cases changed only
`UTA_STUDIO_GGML_FA_QUERY_OWNED=0`, using the same library and test executable.

| Run order | Time attention mean | Frequency attention mean | Maximum sampled external CCS |
| --- | ---: | ---: | ---: |
| New default, first | 63.858 ms / 8.559 TFLOPS | 6.382 ms | 31.90% |
| Prior kernel, first | 98.175 ms | 8.766 ms | 5.64% |
| Prior kernel, return | 98.121 ms | 8.386 ms | 7.73% |
| New default, return | 130.425 ms | 13.082 ms | 93.25% |

The final case overlapped heavy external compute and is retained as interference evidence.
It must not be silently dropped from the run record, or averaged into an alleged isolated kernel result.
Even the other cases do not establish exclusive device use or fixed clocks. The first default case
confirms the actual compiled route and its approximately 8.56-TFLOPS result, not a guaranteed latency.
No other process was stopped to obtain a favorable comparison.

## Final same-library real-model pair

The same installed XE90 F32 GGUF and the existing six-second, 44.1-kHz stereo fixture ran twice
per configuration through `attention_model_tests::b580_roformer_model_comparison`.
The final default ran first, followed by the same library with the opt-out set to zero.

| Measurement | Prior kernel | New default |
| --- | ---: | ---: |
| First processing pass | 8.155767 s | 7.583704 s |
| Second processing pass | 8.119126 s | 7.531146 s |
| Mean time-attention kernel across both passes | 103.246000 ms | 67.346300 ms |
| Time-attention effective throughput | 5.293776 TFLOPS | 8.115682 TFLOPS |
| Mean frequency-attention kernel across both passes | 8.228310 ms | 6.391140 ms |
| All 32 attention kernels per processing pass | 1.783589 s | 1.179799 s |

Time-attention latency is 34.77% lower and effective throughput 53.31% higher in this pair.
Frequency-attention latency is 22.33% lower. The observed second processing pass is 7.24% shorter.
The model timer includes WAV frontend, the production Rust graph and synthesis, but excludes
model loading, worker IPC and FFmpeg publication/encoding. Two passes on one clip are not a
whole-song performance guarantee. Peak sampled aggregate CPU load was 11.01% versus 10.71%,
and neither final model case sampled external CCS compute; DRM visibility remains partial.
Earlier model cases had very different CPU load, including 100% aggregate CPU in the control,
and are not used in place of this final pair.

All 529,200 F32 samples in each complete output were compared. Final default output is byte-identical
to its tested shared-K/V prototype, and final opt-out output is byte-identical to the earlier control.
Each configuration repeats byte-identically. Across old/new configurations:

- maximum absolute sample difference: 0.00038939714431762695;
- RMSE: 0.00003921901589727532;
- relative L2: 0.00024963249840005746;
- SNR relative to the prior output: 72.05397753387332 dB.

All samples are finite. These are full-output numerical results, not perceptual certification.

## Verification and remaining work

The rebuilt default passes 27 explicit fixture cases: thirteen query-owned tail/mask/stride cases,
seven GQA/key-head-broadcast cases, and seven original small-query/H72 fallback cases.
Small fixtures compare every query and every head against complete-context f64 references.
The maximum NMSE over all 27 cases is 2.6598165239776097e-7. Large fixtures check all
79,349,760 outputs per shape for finiteness and sample 768 complete-context reference values.
The ordinary focused suite passes 108 runtime tests and 30 worker tests; 27 opt-in tests are ignored
by the ordinary suite and are not represented as automatically executed. Relevant B580 tests were
invoked separately. All selected completed runs have exit code zero and unchanged boot IDs.

This does not establish 20 TFLOPS, a same-shape XPU comparison, whole-song quality/latency,
Windows or other-vendor coverage, packaged/Nix release acceptance, or long-term host stability.
The next performance targets are the remaining spills, cooperative-fragment rescaling and
score-to-probability handoff. Further changes must be measured against the selected shared-K/V
kernel, not the slower initial prototype or an unrelated large square GEMM.
