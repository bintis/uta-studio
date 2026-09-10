# Fused attention across F16 and F32 K/V storage

## Accepted scope

The floating-storage extension applies after the compact fragment-workgroup
backend in `0008-vulkan-attention-fragment-workgroups.patch`. It retains that
backend's four/eight-subgroup selection, reduced score scratch, cached
probability operands and blockwise F32 output accumulation. It does not replace
installed shared libraries, weights, model generations or user media.

Eligibility is tensor- and device-based, not a model-name whitelist: Intel Xe2,
native SIMD32, cooperative M8, HSK/HSV 64, F32 accumulation, more than 32 query
rows, and independently F16 or F32 K/V storage. Existing explicit query-row and
staging diagnostics remain on their previous path. Small-query/GQA capacity,
non-H64 heads, quantized/BF16 K/V and other device paths are unchanged.

Non-public-schema RoFormer graphs retain F32 K/V views. Those did not enter the
previous F16-only query-owned kernel even when their head size was 64. The new
aliased F32 views load the original tensor directly into the shared operand
tile. Conversion to F16 matches the existing cooperative shader's F32 decoder;
there is no extra graph cast or global conversion tensor. K/V storage types are
independent. Byte offsets and the host's F32 vec4-block row strides are converted
back to scalar element addresses separately for K and V. Pipeline diagnostics
now report the selected K/V types.

Full key context, existing F16 operand/probability rounding, F32 accumulators,
masks, tail guards and production model chunking/reconstruction are preserved.
The score/probability matrix is never materialized in global memory.

## Follow-up non-XE verification

`ROFORMER_B580_MULTIMODEL_VERIFICATION.md` records final same-library pairs for
Harmony, Denoise and Dereverb, including exact repeat checks, full-waveform
differences and sampled external GPU load. The three final time-attention
latencies are approximately 8.2 ms versus 20.7 ms controls, with 4.76%-5.01%
complete-processing reductions. Dereverb's 48.74 dB cross-kernel SNR remains
an explicit quality-acceptance limitation. These later pairs supersede the
independent study's externally contended exploratory timings, not its evidence.

## Evidence and baseline identity

All paths below are relative to the repository unless otherwise stated.
The isolated study is `test-artifacts/attention-query-residency-study/`.
The combined experiment was committed as `a42f068`; it rebases floating storage
on the declared eight-patch backend, whose fragment-workgroup change is
`8aef559`. The combined build record is
`test-artifacts/operations/20260910T043552-53f969d83f40/`: exit 0, 163.921 s.
A replayed connector call stopped at the existing build-start marker; it did
not run a second compiler. Both operation records are retained.

The comparison baseline is the previously accepted seven-patch library set at
`test-artifacts/xe90-subgroup-handoff-study/runtime-final/lib`.
The combined library set is `runtime-combined/lib` under this study. The same
recorded Rust test executable, `attention-tests-storage`, and six-second stereo
44.1 kHz input were used. Model processing executes the production Rust graph,
frontend, overlap/add and synthesis; worker codec/publication is outside its
scope. Each model process performs two complete passes.

Immediately before the combined model series, recorded CPU load was 0.958%
across all CPUs and the B580 snapshot reported 0% GPU utilization. These are
snapshots, not isolation guarantees. Every model run also retains process-tree
and host samples; unrelated desktop or concurrent work may still affect timing.
No unrelated process was stopped, and no samples were silently removed.

## Real-model results

The following are the second complete pass. Attention time is the weighted
mean GPU time per attention operation, not total model wall time. The FLOP
convention is `4 * queries * keys * head_dimension * heads * batches`, covering
QK and PV. XE90 has 16 time and 16 frequency calls in this pass; Harmony and
Denoise each have 18 of each.

| Model | Time attention, baseline -> combined | Effective TFLOPS, baseline -> combined | Frequency attention, baseline -> combined | Complete process seconds, baseline -> combined |
| --- | --- | --- | --- | --- |
| XE90 vocals | 63.1003 -> 43.0719 ms | 8.6618 -> 12.6895 | 6.0552 -> 5.1316 ms | 7.2046 -> 6.8763 |
| MelBand Harmony | 20.8147 -> 8.2211 ms | 3.7877 -> 9.5899 | 2.2374 -> 1.2271 ms | 5.0712 -> 4.6978 |
| MelBand Denoise aufr33 | 20.6635 -> 8.1390 ms | 3.8154 -> 9.6867 | 2.2177 -> 1.2426 ms | 4.9244 -> 4.6926 |

XE90's gain over the seven-patch baseline comes from the fragment-workgroup
change already present in patch eight. The storage extension enables that
query-owned architecture for the two non-XE models; their prior F32 K/V tensors
did not select it. This does not establish acceleration for every installed
model, Qwen, other head dimensions, or another GPU vendor.

All 529,200 interleaved samples in both passes were compared, not just sampled
frames. Every output is finite and each version's two passes are repeatable.

| Model | Maximum absolute difference | RMSE | Relative L2 | SNR | Cross-version sample identity |
| --- | --- | --- | --- | --- | --- |
| XE90 vocals | 0 | 0 | 0 | exact | Bit-identical |
| Harmony | 0.0009318292 | 0.0000489236 | 0.0004444035 | 67.0444 dB | Not bit-identical |
| Denoise | 0.0005028993 | 0.0000708909 | 0.0001901264 | 74.4192 dB | Not bit-identical |

The non-XE differences arise when moving from the earlier cooperative
organization to query-owned attention. They are reported, not disguised as
lossless equivalence. These signal comparisons are not perceptual quality
certification. Model weights and graph semantics were not altered.

Machine-readable details are in `model-summary.json`. Cases are
`harmony-default`, `harmony-combined`, `denoise-default`, `denoise-combined`,
`xe-default` and `xe-combined`, with complete stdout/stderr, launch/completion,
sampling records and diagnostic audio. The summary operation is
`test-artifacts/operations/20260910T044026-2709786ff7b7/`.

## Numerical coverage

The combined library passed all 61 explicitly invoked attention fixtures:

- 30 query-owned cases: tails, masks, poisoned padding, GQA/key-head broadcast,
  sharp online-softmax rescaling and workgroup boundaries.
- 24 floating/mixed-storage cases: F32/F32, F32/F16 and F16/F32; both sides of
  the query-owned threshold; sharp scores; H128 fallback; genuinely F32 values
  rather than only exactly representable half inputs.
- 7 original short-query/H72 fallback cases.

Small numerical cases compare full key context against the existing F64
reference, with unchanged `NMSE < 5e-4` tolerance. GPU invocation records are
`combined-query`, `combined-floating` and `combined-fallback` in the study.
The added power-of-two score-gain fixtures were committed as `46d0023`.
The independent floating-storage fixtures are provided by `786aab4`.

## Experiments not promoted

The same study retains negative and marginal results to avoid repeating them.
Shared Q residency (`701cb6b`) increased the 1722-query time from paired
72.992/74.792 ms controls to 112.418 ms; it is rejected. Direct PV accumulation
(`12a717b`) passed numerical tests but changed addition order and did not show
stable throughput benefit; it is not selected.

The zero-stride row-scale probe (`c7923bf`) reduced scalar scale publication from
128 floats to eight and removed one hot-loop subgroup barrier. Its four-group
measurements were 63.259/63.639 ms versus 63.924/64.203 ms controls; the small
margin does not establish a substantial speedup. A slow frequency-control
sample remains in `shape-summary.json`. Crucially, the probe used the unused
upper half of the older F32 scratch at offset 64. Patch eight removes that half,
so this probe is **not combined with the accepted compact scratch**. Reusing it
without a new storage plan would be out of bounds.

The four-group floating-storage probe (`626aac5`) independently improved
Harmony time attention to 12.8487 ms and Denoise to 12.7700 ms before integration.
Its full-output differences exactly match the combined version's differences.
It is superseded by the compact-workgroup combination, not a separate runtime
recipe patch.

## Remaining acceptance

The real-model result is approximately 12.69 TFLOPS, not 20 TFLOPS. The current
XE90 shape would need about 27.3281 ms per time-attention operation to reach
20 TFLOPS under the same FLOP convention. This pass does not establish zero
register spills or eliminate cooperative fragment-rescale/handoff costs.
Continue profiling those costs in the fused implementation rather than
materializing a full probability matrix in separate GEMMs.

Whole-song testing of this combination, Windows execution, AMD/NVIDIA
regression and formal Nix release acceptance have not been performed. Installed
runtime/model replacement and remote pushes have not been performed. Process
exit zero and unchanged boot IDs do not establish post-exit host stability.

## Final declaration and ordinary regression

The accepted storage extension is committed as `ba0d5ab` and declared as
`native-inference/ggml-worker/patches/0009-vulkan-attention-floating-storage.patch`.
`test-artifacts/operations/20260910T044328-a2298832e3a5/` verifies every recipe
patch digest, applies all nine patches in order to a fresh pinned checkout, and
compares every changed source byte against the source used to build the measured
combined library. All match; identities are in `declared-source-identity.json`.

`test-artifacts/operations/20260910T044329-4059fd920d61/` records ordinary
`cargo test -p uta-ggml-runtime -p uta-ggml-worker`: 110 runtime tests and 30 worker
tests passed, with 31 opt-in runtime tests ignored by that ordinary invocation.
The 61 GPU fixtures above were executed separately, not counted as ordinary
passes. `test-artifacts/operations/20260910T044331-bed31ba25804/` records successful
Rust formatting, whitespace and canonical product-identity checks.

## Follow-up: compact row-scale publication

Patch `0010-vulkan-attention-compact-row-scales.patch` separates eight F32 row
scales from score scratch and removes one hot-loop subgroup barrier. It keeps
this storage extension and tensor/device eligibility unchanged. Paired shape
measurements show a small kernel improvement; XE90, Harmony and Denoise retain
byte-identical complete outputs against this nine-patch backend, but no
consistent whole-model speedup is established. The padded-handoff experiment
is rejected. See [the complete measurements and limits](ROFORMER_B580_COMPACT_RESCALE.md).
