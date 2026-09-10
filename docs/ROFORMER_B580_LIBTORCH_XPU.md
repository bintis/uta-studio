# Isolated native LibTorch XPU comparison on B580

Measured on 2026-09-10. The original comparison below uses synthetic XE90
projection and attention shapes, not a run of the XE90 model or an audio-quality
qualification. The final section separately records the subsequent authorized
native RoFormer model optimization. Application libraries, configured models, settings, launchers and
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

The original evidence supports implementing a further isolated native XPU model/block
comparison. It does not establish whole-model speedup, end-to-end audio parity,
AOT compilation benefits, long-track behavior, Windows support, or release
readiness. Do not multiply the single-operator ratios into an unmeasured model
speedup or silently migrate the application based on this result.

## Native RoFormer optimization — in progress (2026-09-10 UTC)

The user authorized optimizing LibTorch RoFormer on B580 toward **60 seconds for
the existing 354.88-second song**, informed by GGML's layout/attention work.
The historical native `mixed_attention` full-song operation
`20260910T190148-7ab9294967f8` completed 38 chunks in **112.060496322 seconds**,
excluding runtime/weight load and destruction. It is not a matched new control.
New evidence: `test-artifacts/libtorch-roformer-speed/`. No installed library,
model, audio source, precision policy, full attention context, chunk, overlap,
serial chunk execution, cancellation or completion synchronization is changed.
No Vulkan Super or hardware-counter stress group is resumed.

Separate committed changes:

- `a1924e7`: monotonic timestamps on the existing opt-in synchronization trace.
  With tracing off there is no timestamp or added synchronization overhead.
  Consecutive complete timestamps include intervening dispatch/copy/host work;
  they are **not GPU-only kernel timings**.
- `9898de9`: XPU ordinary RoPE uses complex views and one native complex multiply,
  with cached FP32 complex phases. PolarFormer keeps its distinct softplus and
  phase geometry; CPU/ROCm retain their existing arithmetic.
- `fa51ee0`: XPU RoFormer SDPA retains head-interleaved FP16 operands rather than
  forcing BHLD copies. FP32 projection/norm/residual and the existing FP16 SDPA
  input/output rounding remain unchanged; math-SDPA fallback stays disabled.
- `b7cc13f`: XPU RoFormer uses native fused FP32 RMS normalization with the same
  explicit `1e-12` epsilon. Other model families/backends are unchanged.

Executed verification so far:

- CPU primitive checks and the ABI fixture pass; the focused Rust library suite
  passes **54 tests** (`20260910T192101-a75e7436412b`). Builds retain pre-existing
  dead-code warnings from shared Rust DSP/diagnostic modules.
- XPU rotation compares every element on both full XE90 axes, 79,349,760 values
  each, plus packed/strided, singleton and tail shapes. Maximum error against the
  decomposed FP32 formula is `1.19209e-7`; against the complete FP64 arithmetic
  oracle it is below `8.94e-8`. Input values are preserved. Synchronized ABBA
  samples measure roughly **14–15 ms to 2.2 ms per rotation**, not whole attention
  or whole-model time (`20260910T191337-d3e4b3773350`).
- The layout check compares all 79,349,760 attention values on each full axis
  **exactly** with the packed native path, and also checks a complete small FP64
  attention oracle (`20260910T191702-1a5cf21cd13d`). This is layout equivalence,
  not exact FP32 attention; the small rounded-input oracle's maximum difference
  is `0.000399998`, NMSE `4.18043e-8` from FP16 SDPA output rounding/arithmetic.
- The 12-second real-audio control, rotation-only and rotation+layout traces each
  complete the model's two default chunks. All 1,058,400 audio samples are finite
  and compared. Rotation+layout versus control has maximum absolute difference
  `4.97698783875e-6`, SNR **112.6107 dB**, not bitwise equality. Diagnostic rotation
  intervals fall from about 31/30 ms to 6/5 ms (time/frequency); this synchronized
  trace is not normal-throughput acceptance. `bounded-comparison.json` retains
  both independent changes' results.
- Fused normalization's CPU and XPU checks pass, including 8/16/256/384/516-wide
  packed and strided rows at zero, tiny, ordinary and high amplitudes. Complete
  full-shape FP64 comparisons cover 39,674,880 XE90 and 18,455,040 mel-band values;
  maximum absolute errors are `2.6747e-7` and `2.6563e-7`, respectively
  (`20260910T192400-87a86ab25936`). The final two-chunk XE90 model check completes
  (`20260910T192429-7f790442817d`): all 1,058,400 waveform values are finite,
  same 12.000-second/44.1-kHz/stereo shape, max difference **8.35955143e-6** and
  SNR **111.2070 dB** against the control. `normalized-comparison.json` contains
  the complete scan. These runs observed other GPU clients and are numerical
  evidence only, not evidence that fused normalization improves model speed.
- The first new control invocation used a nonexistent top-level model path and
  failed before model loading/inference (`20260910T190918-03d97127c817`). Its
  record remains. The explicitly corrected generation path completed under
  `20260910T190950-d415828a2e64`; no failed GPU inference was automatically retried.

At 19:20–19:21 UTC, pre-run observations found another `uta-ggml-worker` using
B580 (total GPU busy 96–98%). After CPU checks and command preparation, the
19:23:42 snapshot reported only 7% total busy. The subsequent bounded native
checks completed, but their continuous samples captured new GGML work:
`normalized-check` observed clients 83557/84777, and `normalized-profile`
observed 84777/85057 with CCS activity up to about 49%/30%. At 19:24:51 total
GPU busy was again 98%. Do not use these normalized timings as speed evidence.
No other client was terminated, no idle threshold/retry loop was added, and no
further GPU benchmark was launched after recognizing the continued contention.

`observed-competition-summary.json` parses the actual `interval_summary`
records (`20260910T192712-31b0c55091ca`). The preceding
`competition-summary.json` used a nonexistent field and parsed no intervals;
it is retained as **unusable**, not evidence of an idle host. Earlier control,
rotation and layout traces sampled no other GGML compute client, but desktop
activity/partial visibility still preclude an exclusivity claim.

Source-size/whitespace and product-identity checks pass
(`20260910T192341-aaf61c7faf06`). Native observers report only Intel OpenCL/Level
Zero and xe PCI `0000:07:00.0`, unchanged boot IDs through each observation and
no observer read errors. These statements do not establish later host stability.
Unrelated working-tree changes remain untouched.

### Resumed full-song result and retired TF32 investigation

After the recorded cancellation and subsequent explicit user resumption, a fresh
normal-throughput full-song pair completed with tracing off:

| Variant | Inference, 38 chunks | Observed process wall |
| --- | ---: | ---: |
| Original control | 114.231050341 s | 116.239005466 s |
| Rotary/layout/normalization candidate | 72.537182110 s | 74.533642362 s |

This is **36.50% less inference time / 1.575× speedup**, still above 60 seconds.
`current-fullsong-comparison.json` scans all **31,300,416 finite samples**:
max absolute difference **0.0006777942**, RMSE **1.2645061e-5**, SNR **83.26265 dB**.
It is not bit-identical and is not a listening or production-parity qualification.
Both observers sampled no other CCS compute clients, report zero read errors and
unchanged boot IDs; mean total CPU busy was about 8%. Partial observation cannot
prove exclusivity or later host stability. Raw cases: `fullsong-control-current/`
and `fullsong-normalized-current/` in the speed evidence root.

A separately compiled, single-model diagnostic temporarily enabled oneDNN's
process-global TF32 allowance only around QKV and FFN input/output projections.
Everything else retained its preceding precision. A matched-source control
(`projection-control-build`) and diagnostic (`reduced-build`) measured
**73.695609837 / 73.182877546 s** inference and **75.739714376 / 75.251221610 s**
observed process wall. The nominal **0.696%** difference is not an established
speedup: mean CPU busy differed (**9.34% / 19.78%**) and the candidate observer
had one read error. Neither sampled other CCS clients; boot IDs were unchanged.
All waveform samples were compared: max difference **2.384185791e-7**, RMSE
**1.140929391e-8**, SNR **144.155894 dB**; one-second largest-error windows are
retained in `tf32-fullsong-comparison.json`. No blind listening was performed.

The two-chunk verbose diagnostic reports 192 TF32-attributed and 668 strict
matmuls, with F32 operands and `jit:gemm:any`. This proves the attribute request,
**not actual reduced-precision XMX execution**. Wheel-matched ATen `Matmul.cpp`
reads `allowTF32OneDNN`; oneDNN revision
`80afa71049cd69a3df32adcccb623b12cd7baa22`, `jit/gen_kernel.cpp`, retains the base
F32 candidate and adds TF32 matches rather than forcing their selection. These
observations do not establish TF32 hardware acceleration or general TF32 audio
accuracy on B580. Raw diagnostics and upstream sources remain in the evidence
root. The old benchmark omitted the native `roformer_projection_math` field
from serialized Rust metadata; `4e5daf9` forwards it, with a passing unit test.
`native-build-math.json` separately records post-run, read-only native metadata
inspection, not reconstructed launch metadata.

The user then explicitly **retired TF32**, prioritizing speed without reducing
precision and avoiding recurrence of GPU power loss. `8483b41` removes the
experimental switch, projection scope and tests; IEEE projection metadata stays.
No installed runtime was replaced. Historical evidence remains, but TF32 is not
a current candidate or a production mode. Current library tests passed **55**
before this removal and again afterward (`20260910T202405-573055382379`), along
with the CPU ABI check. Later C++ changes also pass their focused CPU checks.

The family series completed XE90 vocals and instrumental control/candidate
executions on 12-second audio; complete waveform analysis is pending. The
**original PolarFormer control** failed inside native SDPA requesting **8.23 GiB**
(`20260910T200940-6ea639e1d138`); the series stopped with no automatic retry.
Its candidate and three mel-band pairs have not run. `family-results.json`
records exact scope. Do not claim family-wide acceptance.

### Further precision-preserving fusions — bounded results

The finer trace (`5fa0499`) executed in `conversion-profile/`, operation
`20260910T202432-c127b7bd6e79`. Median synchronized intervals include about
5.12/4.81 ms for time/frequency rotary, 1.65/1.66 ms query conversion,
1.57/1.57 ms key conversion, 2.08/2.07 ms value conversion, 12.43/1.88 ms SDPA,
and 1.44/1.35 ms attention-output conversion. These are diagnostic host-complete
intervals, not GPU-only or normal-throughput timings. First-use compilation
intervals are retained separately in `conversion-profile-summary.json`.

Further independent source changes:

- `e2674d9`: complex-FP32 rotary multiplication writes directly to the half
  storage already required by mixed attention. Input and cached phases stay
  FP32. Strict attention, PolarFormer and other backends retain their preceding
  rotary path. Every output on both 79,349,760-element axes equals the previous
  FP32 rotation then half conversion **exactly**. Isolated synchronized calls
  are about **3.6–3.8 ms → 1.54 ms**; two control samples reach about 7.55 ms.
  Evidence: `writeback-small-check/`, `writeback-full-axis-check/`, operation
  `20260910T203144-74d6ec03443c`. This is not whole-song speed.
- `d8fa55c`: defer half SDPA output promotion into the existing FP32 gate
  multiply. Complete small strided/singleton checks and gated native attention
  match explicit FP32 promotion exactly (`gating-small-check/`).
- `6382318` / `45db8e1`: use registered native `mkldnn::_linear_pointwise` for
  FP32 FFN input projection plus **erf GELU**, not tanh approximation. The
  wheel-matched `Linear.cpp` and `FusionUtils.cpp` confirm the native post-op;
  `_addmm_activation` itself still executes separate GELU on this XPU wheel.
  Verbose XPU checks/model execution confirm `attr-post-ops:eltwise_gelu_erf`
  with F32 operands. The first build rejected an implicit empty `c10::List`;
  the explicit-construction correction is separately committed and recorded.
- `dafc2a7`: native `_linear_pointwise.binary` combines attention-output and
  FFN-output projection with the FP32 residual add. Bias retains its original
  order. Inputs remain read-only; explicit matrix views avoid rank mismatch in
  the binary post-op. All small XPU residual results match ordinary FP32
  projection + addition exactly, including bias/no-bias and strided cases.
  CPU/ROCm model execution is unchanged. Evidence: `residual-small-check/` and
  `residual-profile/`; native verbose records the binary-add post-op.

All new helpers have CPU checks; final CPU ABI execution passes
(`20260910T205424-9b536ed69fad`). Projection oracle details matter: the initial
unit-bounded absolute tolerance rejected CPU oneDNN GELU at max `2.54537e-6`,
NMSE `7.8212e-14`; the ordinary CPU path measured `1.4151e-6` / `3.30314e-14`.
Both new GELU paths now use the same magnitude-scaled `2e-6 * max(1, peak)` bound
and `1e-12` NMSE bound. Small XPU fused versus ordinary GELU differences are at
most `4.76837e-7`. A cancellation-heavy 1536-wide residual fixture rejected even
ordinary FP32 under the generic NMSE threshold (`2.90822e-12`). Its two paths
therefore use the standard per-element FP32 forward-error bound
`gamma(2*K+2) * (abs(X)*abs(W)^T + abs(bias) + abs(residual))`, with unit roundoff
from the float type; max error/NMSE and bound fractions are still reported.
These changes affect only the new projection oracle tests, not runtime
precision or existing rotary/normalization checks. Failed CPU records remain.
Neither FP32 fusion nor a passed roundoff bound means bit-identical arithmetic.

Complete two-chunk real-audio comparisons (1,058,400 finite samples each):

| Change versus predecessor | Max absolute sample difference | SNR |
| --- | ---: | ---: |
| Rotary writeback | 2.384185791e-7 | 143.23614 dB |
| Attention gating | 2.384185791e-7 | 143.22494 dB |
| Erf GELU post-op | 7.688999176e-6 | 111.75044 dB |
| Residual post-op | 2.384185791e-7 | 143.22851 dB |
| All four versus `conversion-profile` | 7.688999176e-6 | 111.75176 dB |

See `precision-fusion-comparisons.json` and `residual-comparison.json`. These are
complete waveform comparisons, not listening qualification. GELU/residual model
diagnostics also enable oneDNN verbose logging, so their total times must not be
compared with ordinary throughput. No installed assets are changed.

**Next:** fresh full-song `profile-build` control versus `residual-build`, with
tracing and oneDNN verbose off, warm runs zero, original source/model/defaults.
New whole-song speed is not yet measured; the prior accepted result remains
72.54 seconds. Preserve precision, full context, cancellation and synchronization.
Runs remain bounded and serial; no clock/power changes, counter stress, automatic
retry or removal of safety. Kernel journal reading was denied by permissions
(`20260910T205622-b1bff200266f`); boot IDs do not prove absence of GPU resets.
The prior power-loss cause is unresolved. No 60-second, perceptual qualification
or production-readiness claim.

### Whole-song ablation and precision-first selection

The fresh control completed in **74.567869923 s** inference / 76.898180361 s
process. All four fusions completed in **71.421349470 s** / 73.651351894 s:
only **4.22%** less inference time. Full waveform max difference is
**0.00114057213**, SNR **79.33864 dB**; observed VRAM peaks are about
4.51 / 3.03 GiB. Neither run sampled other CCS clients or compiler processes;
CPU means were 10.07% / 13.12%, with zero observer errors and unchanged boot.
See `precision-fullsong-comparison.json` and `fusion-stage-and-cpu-review.json`.

A subsequent snapshot found compiler work at **78.68% CPU busy**, so the next
ablation was deferred while existing source/evidence was reviewed. No process
was killed, no idle threshold/loop was installed, and no automatic retry ran.
After that review and a fresh quiet snapshot, the conversion-only variant
(`gating-build`, without GELU/residual post-ops) completed in **64.525976343 s** /
**66.647711604 s**. All **31,300,416** samples are finite and compared with the
fresh control: max difference **3.576278687e-7**, RMSE **1.138084602e-8**, SNR
**144.17758 dB**. Its CPU mean was 11.86%, no compiler/other CCS clients were
sampled, observer read errors were zero and boot IDs unchanged. VRAM peak was
about 3.62 GiB. This is the selected precision-first result, not bitwise or
listening qualification and still above 60 seconds.

The GELU-only post-op variant completed in **62.112326177 s** / 64.351504229 s,
but full waveform differences remain **79.33864 dB** / **0.00114057213**. Separate
CPU compilation was observed (24.45% mean total busy), so this is not an exclusive
host timing. Even though its arithmetic stayed FP32/erf, the substantially larger
full-song numerical difference is not adopted for about two seconds of apparent
gain under the user's precision-first direction. `a87814e` removes both post-op
implementations, their model routing and dedicated tests. Historical failed and
successful evidence remains; the experimental test bounds are not active code.
`fusion-ablation-comparison.json` retains the complete comparisons and CPU scope.

An additional arithmetic-free value-copy candidate (`00d0cc1`, routed by
`2b45ab2`) reinterprets representable FP32 coordinates as complex pairs only for
native copy to paired half storage; **no complex multiply or other arithmetic**
is performed. Nonrepresentable shape/stride/offset/dtype cases use the same
native scalar half conversion on the same device, not a backend fallback or
new restriction. Complete half-storage-bit comparisons pass for both full
79,349,760-element axes and packed/strided/shifted/odd-width fixtures, including
negative zero, half-way rounding and subnormals. Local synchronized samples are
about **1.85 → 1.50 ms**, with some slower scalar tail samples. Evidence:
`paired-small-check/`, `paired-full-axis-check/`, operation
`20260910T213828-d63726fa9660`. At that stage, candidate `paired-build` passed
CPU checks, ABI and two-chunk execution, while full-song/family review remained
pending. That review and the follow-up normalization-axis investigation are
complete below; neither candidate is retained in model routing.

### Final selection and verification scope

The paired-copy full-song trial (`selected-fullsong-comparison.json`) measured
**71.935902365 → 65.162248519 s** inference, with process wall
74.240350370 / 67.564503399 s. Both sampled separate compiler work (mean CPU
21.14% / 21.29%); no other CCS clients were sampled, but the candidate observer
had one read error. Complete waveform SNR is **126.81178 dB**, max difference
**2.65240669e-6**. This confirms execution and numerical scope, not an incremental
whole-model advantage over the cleaner **64.52598 s / 144.17758 dB** variant.
`71c0ad0` therefore restores scalar value conversion in the model; paired copy
is **diagnostic-only**, not retained merely for a microbenchmark gain.

The normalization-axis probe (`fe16542`, `norm-axis-full-check/`, operation
`20260910T214740-d181dcaba43b`) matches FP32 storage bits exactly but measures
essentially equal packed/direct times: about 1.92–2.03 ms on the time axis and
1.68 ms on the frequency axis, with one slower packed sample. Outputs are
contiguous. There is no material demonstrated gain, so model axis copies stay.
No claim about an uninspected internal copy implementation is needed.

**Retained active path:** ordinary rotary FP32-to-half writeback plus FP32
promotion inside gating, on top of the earlier accepted rotary/layout/RMS norm.
Its active operations match the measured `gating-build`; current source is
`71c0ad0`, freshly built into **`selected-build`**. CPU primitive checks, CPU ABI
fixture and **55 Rust library tests** pass in
`20260910T221000-8f26c62c9a55`. TF32 and GELU/residual post-op implementations are
removed. Paired copy and axis-layout probes do not change model execution.
The selected full-song evidence remains **64.525976343 s** inference /
**66.647711604 s** process, **not under 60 seconds**.

Final active-source regression executed four complete 12-second model paths;
every retained guide waveform sample was compared against the original control,
including earlier rotary/layout/normalization changes:

| Model | Max absolute difference | SNR |
| --- | ---: | ---: |
| XE90 vocals | 8.34465027e-6 | 111.20783 dB |
| XE90 instrumental | 5.07570803e-5 | 67.62442 dB |
| Mel-band denoise | 8.47503543e-7 | 136.84058 dB |
| Mel-band dereverb | 8.76300037e-5 | 84.53295 dB |

`final-active-comparison.json` records all 1,058,400 finite samples per case;
`selected-family-comparison.json` preserves the preceding paired-candidate trial.
Additional XE90 comparisons against the **pre-conversion normalized** controls
isolate this round: vocals **143.20499 dB**, max difference **2.38418579e-7**;
instrumental **140.00019 dB**, max difference **2.23517418e-8**.
These different reference scopes must not be confused with the conversion-only
full-song 144.18 dB result; none is a listening or family-wide parity claim.
Original Harmony control produced **nonfinite masks**
(`20260910T215111-9b8cb57be73d`), so that control was not retried and no Harmony
candidate was run. The original PolarFormer SDPA OOM also remains unresolved.
No context shortening, alternate precision or backend fallback was used to make
either failing model appear to pass.

At final source review, a separate compiler drove a **68.10% CPU** snapshot;
current-build GPU smoke was deferred during independent source/docs checks.
After that work, a fresh observation found **7.23% CPU** with active desktop
graphics (49% aggregate GPU). Four serial **numerical-only** smokes then completed
in `20260910T222555-40b409a2bc8d`; no throughput conclusions are drawn. Final
comparison operation: `20260910T222743-2cf28c28b116`. All report IEEE projections;
observer read errors are 0/1/2/0 in table order, with unchanged boot IDs. No more
GPU execution is planned for this handoff. Kernel log access remains unavailable;
no GPU-reset absence or post-exit host-stability guarantee is made. Installed
assets are untouched, and this is not production or release acceptance.
