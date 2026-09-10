# Isolated native LibTorch XPU comparison on B580

Measured on 2026-09-10. This is a synthetic operator comparison using current XE90
projection and attention shapes, not a run of the XE90 model or an audio-quality
qualification. Application libraries, configured models, settings, launchers and
production backend declarations were not modified by this experiment.

## Executed implementation

Sources: `tools/libtorch-xpu-probe/`. Evidence root:
`test-artifacts/libtorch-xpu-isolated/`.

Two standalone C++20 executables call ATen/LibTorch XPU and GGML Vulkan. They share
the deterministic tensor generator, physical input layout, full-contraction FP64
reference and result schema. Python is used only for acquisition, recording,
observation and summaries; the operator executables do not initialize Python or
load a model. No torch.compile, AOTInductor or model-conversion route is used.

The official `torch 2.13.0+xpu` Linux wheel supplies C++ headers and native
libraries in the private `native/torch` directory. Its oneDNN reports v3.12.0,
commit `80afa71049cd69a3df32adcccb623b12cd7baa22`. Native SYCL/MKL dependencies
are installed only into the private venv. The executed Intel Level Zero driver
is `26.31.39395.13`, reporting `1.17.39395`; the GGML run loads Mesa 26.2.2.
Both select the Intel Arc B580 at PCI `0000:07:00.0`.

The GGML comparator uses a private copy of the installed library set, not an
experimental replacement. Its source ABI headers come from the pinned GGML
checkout used by the existing project. The entire experiment is outside normal
application execution.

## Primary timing convention

Each full-size invocation has eight warmups followed by eight measured calls.
The primary interval is synchronized host compute: dispatch, operator execution,
completion synchronization and any internal conversion/reordering. Input
creation, initial upload, validation and final download are excluded and input
preparation is reported separately. All measured samples are retained.

LibTorch also records XPU stream profiling events. GGML emits its own GPU
operator logger. Do not substitute a GPU-only time for the synchronized time in
one side of a comparison. Diagnostic verbose runs use one warmup/one sample and
are not the performance acceptance set.

Final comparisons below are from operation `20260910T075756-399ad82923cc`.
Their host samples did not observe another compute client above the existing
observer threshold. Sampling, desktop activity, clock changes and CPU scheduling
still preclude an exclusive-device or fixed-clock claim.

## Strict FP32 GEMM results

The LibTorch policy requests IEEE FP32 and disables oneDNN TF32. Input and output
are FP32. The strict GEMM numerical requirement is NMSE below `1e-10`, rather
than accepting a mixed-precision error under the looser attention threshold.

`M` below is output channels, `N` rows per batch, and `K` contraction length.
Weights are shared across batches. Times are mean synchronized milliseconds.

| Shape | GGML Vulkan | LibTorch XPU | Speed ratio |
| --- | ---: | ---: | ---: |
| QKV time: M1536 N1722 K256 batch90 | 24.0435 | 10.2435 | 2.347x |
| FFN time: M1024 N1722 K256 batch90 | 15.8184 | 6.8411 | 2.312x |
| Down time: M256 N1722 K1024 batch90 | 16.9608 | 6.8543 | 2.474x |
| QKV frequency: M1536 N90 K256 batch1722; GGML explicit FP32 packing | 34.6764 | 10.8049 | 3.209x |
| FFN frequency: M1024 N90 K256 batch1722; GGML explicit FP32 packing | 23.2844 | 7.5054 | 3.102x |

The explicit packing is inside the GGML timed graph, not hidden in preparation.
LibTorch receives the original transposed view; any internal handling is timed.
The five LibTorch strict FP32 cases have sampled full-contraction reference NMSE
between `4.6907e-14` and `3.7593e-13`. Packed GGML frequency cases return to
approximately `7e-14`. These improvements do not rely on silently allowing half
precision in the strict FP32 comparison.

Verbose evidence in `diagnostic-torch-qkv-time-f32/stdout.txt` confirms oneDNN
`matmul,jit:gemm:any`, with F32 inputs and output. The contiguous time batch folds
to `154980x256:256x1536`; the frequency variant retains a batched, strided
`1722x90x256:1x256x1536` operation. This is an operator/runtime comparison, not a
claim that both systems selected the identical GEMM microkernel or partition.

## FP16 attention results

Full context is retained: time attention is B90 H8 N1722 D64, and frequency
attention is B1722 H8 N90 D64. The FLOP convention counts both QK and PV:
`4 * B * H * N * N * D`. Time attention is 546,561,146,880 FLOPs per call.

| Shape | GGML synchronized time | LibTorch synchronized time | GGML / LibTorch effective TFLOPS |
| --- | ---: | ---: | ---: |
| Time attention | 51.1056 ms | 14.9838 ms | 10.6947 / 36.4769 |
| Frequency attention | 5.6076 ms | 3.5867 ms | 5.0941 / 7.9645 |

The first uncontended-by-sampling series measured LibTorch time attention at
14.3194 ms / 38.1694 TFLOPS. The final return run measured 15.9376 ms /
34.2938 TFLOPS. These are complete per-run means, not selected individual best
samples. Thus greater than 20 TFLOPS is observed in this native XPU synthetic
attention experiment, but not yet in the production XE90 model or GGML backend.

The final time-attention XPU profiling interval is 14.4805 ms / 37.7446 TFLOPS;
the first series event interval is 13.8424 ms / 39.4845 TFLOPS. These secondary
numbers must not be mixed with the primary synchronized comparison above.

`diagnostic-torch-attention-time-f16/stdout.txt` confirms execution of
`sdpa,ocl:micro:reusable` with the exact full shape and a fused
QK/scaling/softmax/PV graph. Math-SDPA fallback is disabled in the probe. No full
quadratic score/probability tensor is deliberately materialized by the probe.

## Precision distinctions and BF16

GGML attention keeps F32 Q storage, existing internal half operand/probability
rounding and F32 output accumulation. LibTorch receives the same half-rounded
input values in FP16 storage, returns a FP16 tensor, and converts it to FP32
inside the timed interval. Output storage after the wrapper is F32, but these
are not identical arithmetic paths or bitwise-equivalent outputs.

Every successful case checks all output elements for finite values and compares
128 deterministic positions to an FP64 reference over the complete contraction
or complete key sequence. A complete attention output contains 79,349,760 values.
This is a full finite-value scan plus sampled error testing, not a full-output
error scan. Both original-F32-input and rounded-input reference errors are saved.

For LibTorch time attention, original-input reference NMSE is `1.1626e-7` in FP16
and `8.1508e-6` in BF16. BF16 measures 14.6128 ms / 37.4030 TFLOPS in the final
run but has roughly seventy times the sampled NMSE of FP16 on this fixture.
The frequency FP16/BF16 results are 3.5867/4.1599 ms and approximately
`1.0505e-7`/`7.0626e-6` NMSE. This does not favor BF16 as the first model candidate.

Ordinary FP16 GEMM is also faster in the first series: QKV time is 10.9518 ms
GGML versus 6.3133 ms LibTorch; the down projection is 6.1031 versus 2.3988 ms.
The same output-rounding distinction applies. No audio or end-to-end model
accuracy is inferred from these synthetic comparisons.

## Exposed GGML noncontiguous FP32 behavior

Two original-view GGML F32 frequency GEMMs fail the strict numerical criterion:
QKV NMSE `5.3142e-8` and FFN NMSE `7.4340e-8`, rather than roughly `1e-13`.
Their results match the reference error from half-rounded input data. The
source in the pinned `ggml-vulkan.cpp` explicitly reformats noncontiguous
operands to half (`y_non_contig`, `f16_type`, `to_fp16_vk_1`, around lines
9356-9428 in the inspected checkout). The existing patch that preserves the
all-F32 pipeline does not cover this separate conversion/dispatch route.

Adding only an explicit F32 `ggml_cont` to the diagnostic graph restores the
strict numerical result; its time is included in the table. The failing cases
remain in the records. The installed backend was not patched during this task.
This finding concerns the tested noncontiguous invocation; it is not evidence
that every model F32 GEMM takes that path.

Flattening shared-weight GGML batches alone is not enough to close the gap.
The separate layout series records flat QKV time F32 at 23.4014 ms and flat
frequency F32 at 24.6497 ms. Forced LibTorch BMM gives 10.0265/10.7602 ms.
These diagnostic observations indicate a remaining implementation difference,
not solely an advantage from folding the time batch. They are separate runs,
not substitutes for the paired final table.

## Failures and resource competition retained

There are 80 observed native process runs: 76 exit successfully, four fail.
Two failures are the exposed GGML noncontiguous F32 cases. The other two are
initial LibTorch SDPA failures caused by missing `libOpenCL.so.1` in the private
search path. Exposing the already-installed OpenCL loader/vendor files to the
process fixes the smoke and full-size SDPA runs. Neither the system driver nor
the application environment was changed.

The intermediate confirmation series overlaps unrelated processes named
`benchmark`. Its time-attention means balloon to 78.5257 ms for LibTorch and
158.7221 ms for GGML. Those records and competing-client identities are retained;
they are not used to claim speedup, and no unrelated process was stopped.

Initial dependency acquisition also retains unsuccessful/replayed attempts;
only duplicate download processes launched by this experiment were stopped.
The subsequent explicit ranged downloader obtains official release wheels and
installs them privately. No unavailable check is relabeled as success.

## Reproduction and handoff

Machine-readable evidence: `measurements.json` contains all observed cases;
`acceptance-summary.json` selects final pairs without deleting earlier results.
Each case directory contains stdout/stderr, launch/completion metadata and host
samples. The selected source/runtime directories and commands are recorded in
`test-artifacts/operations/` before execution.

Important operation records:

- `20260910T074237-498f32b0a6ff`: private reference copy and native CMake build.
- `20260910T075003-c6b01f9c0247`: first full-size series, with two precision failures.
- `20260910T075425-185d68e7632c`: confirmation, BF16 and layout diagnostics, including contention.
- `20260910T075756-399ad82923cc`: final paired recheck and return measurements.
- `20260910T080024-f802004f970c`: full-shape oneDNN implementation diagnostics.
- `20260910T075831-d729e38a5f92`: shell/Python/whitespace/product-identity checks.

The source README contains build and run instructions. Keep the compiled
executables and dependencies in `test-artifacts/libtorch-xpu-isolated`; nothing
has been installed as the application's backend.

The evidence supports implementing a further isolated native XPU model/block
comparison. It does not establish whole-model speedup, end-to-end audio parity,
AOT compilation benefits, long-track behavior, Windows support, or release
readiness. Do not multiply the single-operator ratios into an unmeasured model
speedup or silently migrate the application based on this result.
