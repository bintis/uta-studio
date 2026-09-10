# B580 XE90 attention: fragment reuse and shared workgroups

## Accepted result and scope

Backend commit `8aef559` adds `0008-vulkan-attention-fragment-workgroups.patch`
to the runtime recipe. It is measured against the already optimized seven-patch
query-owned kernel, not the older scalar or six-patch implementation. Boundary
regression commit: `5e49a6d`. Measurement date: 2026-09-10.

The final rebuilt default measures 12.667 effective TFLOPS on the full XE90 time
attention shape. In the real production Rust Roformer graph, time attention
falls from 63.249 to 43.158 ms and frequency attention from 6.087 to 5.084 ms.
The two complete six-second output WAVs are byte-identical to their controls.
This is targeted kernel/model evidence, not whole-song or release qualification.

The current working tree also declares the subsequent floating-storage patch
`0009-vulkan-attention-floating-storage.patch`, documented in
[the floating-storage investigation](ROFORMER_B580_FLOATING_STORAGE.md).
The numbers here isolate the freshly rebuilt eight-patch snapshot. They must
not be relabeled as a new test of the later combined recipe or its other models.

The selection boundary remains Intel Xe2, M8 cooperative matrices, HSK=HSV=64,
F16 K/V, F32 accumulators, native SIMD32 and more than 32 query rows. Eight
subgroups each own eight queries; the workgroup covers 64 queries. A device
without the required workgroup or shared-memory capacity retains four groups.
`UTA_STUDIO_GGML_FA_SHARED_GROUPS=4` explicitly compares the smaller group count.
It is still the new shader, not an exact seven-patch control.

`UTA_STUDIO_GGML_FA_QUERY_OWNED=0` selects the earlier six-patch algorithm;
it must not be mislabeled as the direct control for this change. Explicit
query-row or KV-staging diagnostics retain their existing routing. Small-query
and GQA paths keep the original capacity and implementation.

No model weights, attention context, audio chunking, overlap, F16 probability
rounding, F32 running state or per-output accumulation order changed. F32 matmul
promotion was not enabled. No installed model, runtime or source media was replaced.

## What changed

1. **Share K/V across independent query owners.** The per-subgroup query and
   output fragments remain the same size. Doubling the subgroup count reuses
   each coalesced K/V load across twice as many queries without doubling one
   subgroup's live accumulators. For 1722 queries, the number of query workgroups
   per head/batch falls from 54 to 27 with the same total padded query count.
   This halves source-level K/V loading for that shape. It is not a measurement
   of DRAM bytes: caches and the driver's lowering also affect physical traffic.
2. **Reuse score scratch for output.** The F32 scratch changes from 128 to 64
   vec4 elements per subgroup. Output is stored and consumed one 8x16 fragment
   at a time instead of reserving a complete 8x64 output tile. Matrix load/store
   layouts remain portable; subgroup barriers protect each reuse. The four
   workgroup barriers per key block remain. At four groups the allocation falls
   from 14,368 to 10,272 bytes. The final eight-group allocation is 20,512 bytes:
   total shared memory is not halved when the workgroup size also doubles.
3. **Reuse rounded probability fragments.** Each of the two probability matrix
   operands is loaded once and used across all four output-depth contractions.
   The source-level probability matrix loads per key block fall from eight to
   two. The block's PV product is still accumulated separately and then added
   to the persistent output, preserving its existing arithmetic ordering.

The final Mesa compiler reports a SIMD32 kernel with 2,479 instructions,
128 GRF registers, 20 spills and 112 fills, 240 sends and 20,512 shared bytes.
These are static compiler diagnostics, not measured hardware stall fractions.
The earlier seven-patch diagnostic reported 39 spills and 142 fills. There are
still spills; this work does not claim register pressure has been eliminated.

## Final default measurements

Evidence root: `test-artifacts/xe90-fragment-handoff-study/`.
`verification.json` retains individual GPU samples, numerical fixtures, model
results, complete-output comparisons and completion records. The shorter
`verification-summary.json` contains the final matched comparisons.

The device is an Intel Arc B580, Mesa 26.2.2, xe driver, PCI `0000:07:00.0`.
All final verification used boot `ad1bdfc3-25a9-4f60-818f-d480ac877e37`.
Final pre-run observation recorded about 10.75% aggregate CPU activity and an
idle B580. This does not establish exclusive use or fixed clocks.

The isolated tests use the full production shapes and interleaved query layout:

| Axis | Q/K/V shape in GGML order | QK plus PV FLOPs per call |
| --- | --- | ---: |
| Time | 64 x 1722 x 8 x 90 | 546,561,146,880 |
| Frequency | 64 x 90 x 8 x 1722 | 28,565,913,600 |

Q is F32; K/V are F16; output and accumulators are F32. Each shape has eight
warmups and eight measured calls. TFLOPS is the listed matrix FLOP count divided
by the whole fused attention GPU timestamp duration, including softmax and
synchronization. Initialization, compilation, upload and reference computation
are outside these GPU measurements. Every measured sample is retained.

| Final isolated comparison | Seven-patch control | Rebuilt default |
| --- | ---: | ---: |
| Time attention, mean +/- sample SD | 63.030 +/- 0.280 ms | 43.148 +/- 0.330 ms |
| Time effective throughput | 8.671 TFLOPS | 12.667 TFLOPS |
| Frequency attention, mean +/- sample SD | 6.069 +/- 0.014 ms | 5.094 +/- 0.011 ms |
| Frequency effective throughput | 4.707 TFLOPS | 5.607 TFLOPS |

The final cases are `final-shapes-control` and `final-shapes-default`.
The default run unsets all seven tuning overrides, including the shared-group
variable. Its pipeline log confirms `Br=64 Bc=32 subgroups=8 lanes=32`.

### Real XE90 graph

Both versions use the same installed F32 GGUF and read-only six-second,
44.1-kHz stereo WAV. Each configuration runs the production Rust graph twice.
The timer includes WAV frontend and synthesis, but excludes model loading,
worker IPC and codec/publication. The control and final runs use the same newly
built Rust test executable and differ only in the selected shared libraries.

| Final real-model comparison | Control | Rebuilt default |
| --- | ---: | ---: |
| First processing pass | 7.289586 s | 6.917024 s |
| Second processing pass | 7.233809 s | 6.883738 s |
| Mean time-attention kernel | 63.249200 ms | 43.158200 ms |
| Mean frequency-attention kernel | 6.086630 ms | 5.084150 ms |
| Time-attention effective throughput | 8.641 TFLOPS | 12.664 TFLOPS |
| All 32 attention kernels per pass | 1.109373 s | 0.771878 s |

Time-attention latency decreases by 31.765%, and throughput increases by
46.552%. Frequency-attention latency decreases by 16.470%. The observed second
processing pass is 4.839% shorter. Do not substitute the attention percentage
for the model percentage or extrapolate either into a whole-song guarantee.

Each output contains 529,200 F32 samples and is 2,116,868 bytes. Complete file
comparisons are byte-identical for both final cross-version passes, the earlier
scoped experiment, and the earlier control versus the rebuilt default. Repeats
within each model run have zero maximum absolute difference. This qualifies
this fixture, not all possible audio or perceptual quality.

## Rejected and intermediate experiments

The experiments remain outside the runtime recipe. No diagnostic-only lane-map
failure path is included in the accepted default.

| Experiment | Observation and disposition |
| --- | --- |
| Skip identity online rescaling | Numerical tests pass, but initial apparent gains did not establish a repeatable advantage. Not selected. |
| Portable row-scale register shuffle | Numerical tests pass; additional mapping/liveness did not beat the control. Not selected. |
| Full-tile aligned K/V loading | Numerical tests pass; no clear gain. Not selected. |
| Native-fragment softmax plus fallback in one shader | Correct on the tested cases, but roughly 106 ms versus about 64 ms; compiler reports 108 spills / 475 fills. Not selected. |
| Isolated fragment-only and compact-scratch variants | Confirmed actual lane-map branch execution with diagnostic checks, but did not establish an improvement. Not selected. |
| Q held in shared memory instead of persistent fragments | About 50.25 ms versus roughly 44-45.5 ms with cached Q in the same comparison. Not selected. |
| Sixteen shared subgroups | 392.156 ms on time attention versus about 43.9 ms with eight groups. Not selected. |
| Probability-fragment reuse at eight groups | 43.038 and 43.022 ms versus 43.537 ms for the preceding eight-group kernel. Retained, then verified in the complete rebuilt default and model pair. |

The native-fragment prototype avoids hard-coding unverified lane ownership by
loading coordinate matrices and checking the actual component mapping. The
standalone diagnostic deliberately fails finite-output tests on mismatch; it
is not a production fallback. The accepted shader does not use that prototype
or assume a component-to-lane distribution.

Some exploratory runs overlapped other GPU work; one frequency run was a large
outlier. They remain in `all_shape_experiments` and their case directories, not
silently discarded. Later matched controls and final default runs are reported
separately. A host reboot occurred between cohorts. The prior boot journal was
unavailable, so its cause is unknown; no attribution to a particular task is
made. Successful process completions and stable final boot IDs are not a claim
of long-term host stability.

## Verification and reproduction

The eight-patch recipe was built from a fresh pinned GGML checkout through a
snapshot of the existing `build-ggml-runtime.sh`, not by installing ad-hoc
libraries. Build operation: `20260910T043039-60fa8e346dd1` (exit 0).
Final isolated runtime: `test-artifacts/xe90-fragment-handoff-study/runtime-integrated/lib`.

The final B580 matrix passes 37 fixture cases: seven legacy tail/stride/head
cases, thirteen query-owned tail/mask/stride cases, seven grouped-head/broadcast
cases, six query-workgroup boundary cases and four online-softmax stress cases,
including 4097 keys. The large-shape tests check every output for finiteness and
compare sampled outputs with the f64 reference; they do not compute a full
79-million-element f64 reference. The new boundary test covers 63/64/65 and
127/128/129 queries with padding and causal masking.

`bash dev.sh -c cargo test --locked -p uta-ggml-runtime -p uta-ggml-worker -j2`
passes 110 runtime tests and 30 worker tests. The ordinary suite ignores 31
explicit opt-in tests; the relevant B580 tests above were invoked separately.
Ordinary test operation: `20260910T043448-de303b022881`.
Final numerical operation: `20260910T043554-2d8c503c55f4`.
Final shape/model operation: `20260910T043750-2adc13f18bb6`.

For a rebuild, use a fresh checkout of GGML commit
`8c63e70982c95ceb862e3a1073a2c1beef75d60a`, unique experimental build/output
locations, the repository operation recorder and `bash dev.sh`. Do not point
the build script at an installed runtime: it replaces its explicit destination.
For an exact control, build the preserved seven-patch snapshot separately.
Setting the new group override to four is useful for ablation but is not the
old shader. Application inference benefits only after selecting a newly built
runtime through the normal explicit lifecycle; changing source is not an
installation action.

No same-shape/same-precision XPU comparison, 20-TFLOPS attention result, whole-song
performance result, other-vendor regression or full release/Nix acceptance is
claimed. The remaining optimization work is still matrix/softmax scheduling,
register pressure and data movement, not context reduction or global precision
promotion.

## Primary technical references

- Khronos, `GLSL_KHR_cooperative_matrix`: component distribution is implementation-dependent.
  https://github.com/KhronosGroup/GLSL/blob/main/extensions/khr/GLSL_KHR_cooperative_matrix.txt
- Intel, *Registers and Performance*: register liveness, spills and subgroup-width tradeoffs.
  https://www.intel.com/content/www/us/en/docs/oneapi/optimization-guide-gpu/2025-0/registers-and-performance.html
- The installed Mesa source used for the rejected layout experiment was read at
  `src/intel/compiler/brw/brw_nir_lower_cooperative_matrix.c` in the source
  corresponding to the installed 26.2.2 derivation. Its lowering is evidence for
  that implementation, not a portable Vulkan lane-layout contract.

### Handoff formatting check

The modified attention fixture passes direct `rustfmt --edition 2024 --check`
(record `20260910T045209-c89c9334a6e4`). A later package-wide formatting check
(`20260910T045145-7e6626ac6ad2`) reports formatting differences in the concurrently
changed `native-inference/ggml-runtime/src/game/infer.rs`, outside this patch's
scope; those changes were not reverted or reformatted by this investigation.
The successful 110/30 ordinary test results above remain scoped to their
recorded source state, not an assertion that every subsequent workspace edit
was tested.
