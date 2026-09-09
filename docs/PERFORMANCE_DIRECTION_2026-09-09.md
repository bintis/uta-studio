# Where the analysis time actually goes

**Measured:** 2026-09-09, Intel Arc B580, patched pinned GGML runtime.

This records what a full analysis spends its time on, so optimisation effort goes where the time
is rather than where it feels like it should be.

> **Correction.** The first version of this document, committed as `04c4a4f`, concluded that Leap
> was host-bound and that flash attention was a minor cost. Both claims came from a bad sum of the
> `GGML_VK_PERF_LOGGER` output that dropped the `FLASH_ATTN_EXT` lines, whose labels carry tensor
> shapes. Direct stage timing inside the chunk path refuted it. The numbers below are the corrected
> ones; the direction they imply is close to the opposite of the first version's.

## Method

Short clips are enough. Every model here is chunked, so cost is `fixed + chunks x per_chunk`, and
three short runs pin both terms. Leap separation, the most expensive model in the graph:

| Input | Chunks | Wall |
| --- | ---: | ---: |
| 6 s | 1 | 13.02 s |
| 12 s | 2 | 24.36 s |
| 24 s | 5 | 57.83 s |

Fitting gives `fixed 1.82 s + per chunk 11.20 s`. The fit predicts the 38-chunk full song at
**427.5 s** against a measured **428.6 s**, so the model is sound and a full song never has to be
run to evaluate a change.

Two instruments then split a chunk. `UTA_STUDIO_STAGE_PROFILE=1` times the host stages of the
RoFormer chunk path (`native-inference/ggml-runtime/src/stage_profile.rs`), and
`GGML_VK_PERF_LOGGER=1` times every Vulkan dispatch.

## Finding 1: Leap is GPU-bound, and the host frontend is free

Stage profile over a two-chunk run, 22.65 s wall:

| Stage | Time | Share |
| --- | ---: | ---: |
| `ggml_backend_graph_compute` | 21.922 s | 96.8% |
| reconstruct (mask + iSTFT) | 0.111 s | 0.5% |
| STFT (both channels) | 0.045 s | 0.2% |
| input preparation | 0.047 s | 0.2% |
| graph reservation | 0.036 s | 0.2% |
| upload + download | 0.069 s | 0.3% |
| deinterleave | 0.004 s | 0.0% |
| unattributed | 0.416 s | 1.8% |

The Vulkan dispatches in that same run total **22.088 s**, or 11.044 s per chunk against a fitted
11.20 s per chunk. **The GPU is busy for about 98% of a separation pass.** The whole Rust frontend
— deinterleave, two STFTs, input packing, mask application, four iSTFTs — costs 0.15 s per chunk.

The process uses 11.0 s of user CPU across 23.8 s of wall, all of it GGML's command-buffer
recording, and all of it already hidden under GPU execution.

## Finding 2: flash attention is two thirds of both separators

Per chunk, Leap:

| Operation | Dispatches | Time | Share |
| --- | ---: | ---: | ---: |
| `FLASH_ATTN_EXT` | 32 | 7.021 s | **63.6%** |
| `MUL_MAT` | 430 | 2.363 s | 21.4% |
| `RMS_NORM_MUL` | 155 | 0.420 s | 3.8% |
| `CONT` | 404 | 0.418 s | 3.8% |
| `ADD` | 430 | 0.319 s | 2.9% |
| everything else | 610 | 0.503 s | 4.5% |

PolarFormer is the same shape of problem: 66.9% `FLASH_ATTN_EXT`, 15.1% `MUL_MAT`.

Leap's attention splits into two shapes, and one of them is almost the whole cost:

| Attention | Per call | Calls | Per chunk | Rate |
| --- | ---: | ---: | ---: | ---: |
| time axis, `dst(64,8,1722,90)` | 398.7 ms | 16 | 6.380 s | 1,371 GFLOP/s |
| frequency axis, `dst(64,8,90,1722)` | 38.9 ms | 16 | 0.622 s | 735 GFLOP/s |

**One kernel shape is 58% of the entire full-song separation.** It is quadratic in the 1,722-frame
chunk length, which is why it dominates: attention cost per second of audio scales linearly with
chunk length, while everything else stays flat.

PolarFormer's equivalent shape, with F16 K/V, runs at 1,542 GFLOP/s against Leap's 1,371 — 12%
apart, on tensors of a different head size. So F16 K/V is worth testing but is not obviously the
whole answer.

## Finding 3: the B580 cannot run GGML's cooperative-matrix attention, and would not want to

The suspicion that Leap misses Intel's matrix engine is **correct**, and the reason is a shape
mismatch. Querying `vkGetPhysicalDeviceCooperativeMatrixPropertiesKHR` on this machine:

| Device | Cooperative matrix shapes |
| --- | --- |
| Intel Arc B580 | `M=8 N=16 K=16` only (f16 x f16 into f16 or f32, plus int8) |
| AMD Radeon 780M | `M=16 N=16 K=16` (f16 x f16 into f16 or f32, plus int8) |

GGML's coopmat1 flash attention requires exactly 16x16x16:

```c
bool shape_ok = (f32acc && device->coopmat_support_16x16x16_f32acc) ||
                (!f32acc && device->coopmat_support_16x16x16_f16acc);
if (!shape_ok || !shmem_ok) { path = FA_SCALAR; }
```

Both flags are set only for `MSize == 16`, so on the B580 flash attention **always** falls back to
the scalar shader. Four control runs confirm it: `GGML_VK_DISABLE_COOPMAT` changed the dominant
attention dispatch by 0.1% and left the output within 2.4e-7, and `GGML_VK_DISABLE_COOPMAT2` left
it bit-identical. Nothing cooperative is running.

> **Correction, same day.** The paragraph that followed here concluded the missing path was not
> worth having, on the strength of the 780M being slower with coopmat. That inference does not
> survive: AMD's RDNA3 WMMA is not a separate matrix engine — it issues through the same vector
> ALUs, so coopmat there is expected to be a wash and the measurement below only confirms that.
> Intel's XMX *is* a separate systolic array, and on this very B580 GGML's cooperative-matrix
> **matmul** — which accepts the `M=8` shape, because it records whatever shape the device reports —
> is **8.6x faster** than the scalar path on a real shape from Leap's graph:
>
> | `MUL_MAT f32 m=1024 n=1722 k=256` | Time | Rate |
> | --- | ---: | ---: |
> | coopmat (XMX) | 173.6 us | 5,203 GFLOP/s |
> | `GGML_VK_DISABLE_COOPMAT=1` | 1,493.7 us | 605 GFLOP/s |
>
> So the matrix engine is reachable on this device and worth roughly 8x where GGML already uses it.
> Flash attention is the one place it does not, purely because of the `16x16x16` gate. The gate is a
> shader-authoring convenience, not a hardware limit: `MatBc` and `MatBr` are already tile constants
> in `flash_attn_cm1.comp`, and `M` is `MatBc`, so an `M=8` variant is a build of the same shader
> with a different constant rather than a rewrite. **This is worth doing**, and it is now the top
> item in the direction below.

The 780M does expose 16x16x16 and does take the coopmat attention path, and it is slower there —
which is what a shared-ALU WMMA implementation predicts, and says nothing about XMX:

| Device | Path | Dominant attention dispatch | Rate |
| --- | --- | ---: | ---: |
| B580 | scalar | 398.7 ms | 1,371 GFLOP/s |
| 780M | coopmat1 | 467.9 ms | 1,168 GFLOP/s |
| 780M | scalar (`GGML_VK_DISABLE_COOPMAT=1`) | 419.8 ms | 1,302 GFLOP/s |

On the 780M, GGML's cooperative-matrix attention loses to its own scalar shader by 11%. On RDNA3
that is the expected result and does not transfer to Intel.

## Finding 4: the two knobs worth testing are both answered, and both are no

| Knob | Speed | Numerics | Verdict |
| --- | --- | --- | --- |
| F16 attention accumulation | no change (408.1 ms vs 398.7 ms) | 3.6e-3 peak, 3.8e-3 relative RMS | Rejected. Costs precision, buys nothing. |
| F16 attention K/V | no change | bit-identical | Not applicable. Leap loads through the public schema, which already casts K and V to F16. |

## Finding 5: the 780M is only 2.2x slower than the B580

Leap, same 12-second clip, same build:

| Device | Kernel time per chunk |
| --- | ---: |
| Intel Arc B580 | 11.04 s |
| AMD Radeon 780M | 23.93 s |

Run together on independent chunks, combined throughput is `1/11.04 + 1/23.93 = 0.132` chunks per
second, or 7.55 s per chunk against 11.04 s on the B580 alone — a **1.46x ceiling**, before any
contention between the discrete card and the integrated one sharing system memory. On a full song
that is 428.6 s to roughly 294 s.

## Finding 6: the matmul comparison does not test Intel's matrix engine

The earlier version claimed the XMX hypothesis was retired because Leap's F32 matmul matched
PolarFormer's F16 one. That comparison was invalid: the local F32 matmul patch routes F32 x F32 to
the scalar shaders for **both** models, so both were measured on the same path. They agree because
they are the same code, not because F16 buys nothing.

What is true is that this path is not the problem. Leap's large batched matmuls run at 5.1-5.6
TFLOP/s, roughly a fifth of the B580's F32 peak, and `MUL_MAT` is only 21% of the time. Whether
coopmat would beat it is untested — and it is exactly the precision the patch exists to avoid.

## Finding 7: the matrix engine is faster *and* more accurate, and it is now reachable

`native-inference/ggml-worker/patches/0002-vulkan-flash-attention-on-8x16x16-coopmat.patch` gives
GGML's coopmat attention shader an `M=8` build and selects it where that is the only shape the
device reports. `MatBc` is the M of the multiply, so the tile constant carries most of the change;
the PV stage additionally had to stop using that same constant as its contraction step, which only
works when M is 16. The rewrite is bit-identical to upstream on the 780M, where M is still 16.

Leap on the B580, five chunks:

| | Wall | Per chunk | Dominant attention dispatch |
| --- | ---: | ---: | ---: |
| scalar attention | 56.88 s | 11.38 s | 398.7 ms |
| XMX attention | 31.35 s | 6.27 s | 114.5 ms |

**1.81x**, and the attention operation itself is **3.55x**. It is 1.81 rather than 3.55 because
attention was 63.5% of a chunk: even a free attention would cap the chunk at 2.74x.

**Accuracy settles the question in the patch's favour.** Against the CPU reference lane on the one
chunk where the two paths disagree most:

| Path | Relative RMS from the CPU reference |
| --- | ---: |
| XMX coopmat attention | **2.256e-4** |
| scalar attention (today's default on the B580) | 1.292e-3 |

The matrix path is **5.7x closer to the reference** than the scalar path it replaces. This is not a
speed-for-precision trade, and the patch does not belong behind the turbo toggle: it is simply
better on both axes.

## Finding 8: the remaining matmul cost is a weight-precision decision

After the attention patch, `MUL_MAT` is the largest item in a Leap chunk at 39.7%, and it did not
move at all (2.342 s to 2.379 s). The reason is in the GGUF:

| Model | Weights |
| --- | --- |
| Leap | F32, all 983 tensors |
| PolarFormer | F32, all 678 tensors (despite the file being named `model-fp16.gguf`) |
| MelBand denoise / dereverb / harmony | F16 for the ~300 tensors that hold the weight bytes |

Leap and PolarFormer carry F32 weights, so their matmuls are F32 x F32 — exactly the path patch
0001 pins to the scalar shaders for precision. The three MelBand separators already carry F16
weights, so their matmuls already reach the matrix engine.

That makes the size of the prize directly measurable, without converting anything. Denoise, whose
weights are already F16, on the same audio with cooperative matrices on and off:

| Operation | coopmat | scalar | Gain |
| --- | ---: | ---: | ---: |
| `MUL_MAT` | 0.472 s | 2.364 s | **5.01x** |
| `FLASH_ATTN_EXT` | 0.412 s | 1.299 s | 3.16x |
| all kernels | 1.685 s | 4.660 s | **2.77x** |
| wall | 2.469 s | 5.669 s | 2.30x |

So converting Leap and PolarFormer to F16 weights would take their `MUL_MAT` from 2.379 s to
roughly 0.48 s a chunk, or 5.99 s to about 4.09 s — a further **1.46x**, and **2.68x** against
where this document started. It is a real decision rather than a free win: patch 0001 exists
because F16-rounded operands moved FireRed's encoder from 4.234e-7 to 2.388e-3. But the precedent
is already in the product, since three of the five separators ship F16 weights today.

## Finding 9: a bigger key block does not help, it hurts

Giving the M=8 attention build the same 64-key block the M=16 build gets — two
`M` tiles per subgroup instead of one — was the obvious way to put more matrix
work between two softmax passes, which is what Xe2's co-issue of matrix and
extended-math operations rewards.

Measured on the GPU timeline, the dominant attention dispatch goes **114.5 ms
to 171.9 ms**, 50% slower. Everything that scales with the key block — the
score tile, the probability tile, the staging buffer — doubles in shared
memory, and on this device the occupancy that costs is worth more than the
extra matrix work in flight. Accuracy improves slightly, to 9.598e-4 against
the CPU reference from 1.322e-3, because the online softmax rescales half as
often, but not enough to matter.

Reverted. The 32-key block stands.

**A note on method.** Wall time could not have carried this conclusion. Three
runs of the identical 32-key configuration measured 5.111 s, 6.602 s and
5.830 s — a 25% spread, on a machine that had been running GPU and 16-thread
CPU work for hours. Per-dispatch timings from `GGML_VK_PERF_LOGGER` are read
from GPU timestamps rather than host wall clock and are stable to well under a
percent across those same runs, so comparisons in this document rest on them.
Any wall-time difference smaller than about 30% here means nothing.

## Direction, ranked by measured value

| Change | Full-song saving on Leap | Notes |
| --- | ---: | --- |
| **Cooperative-matrix flash attention on Intel** | ~190 s (428.6 to ~237) | **Done**, patch 0002. 1.81x on Leap, 1.62x on PolarFormer, and more accurate than the path it replaces. |
| **F16 weights for Leap and PolarFormer** | a further ~1.46x | Measured on Denoise, which already ships F16: `MUL_MAT` 5.01x, whole model 2.30x. Needs a precision decision, because patch 0001 exists to keep F32 operands out of F16 shaders. |
| **Split chunks across both GPUs** | ~135 s (428.6 to ~294) | Chunks are independent, the work is GPU-bound, and the 780M is only 2.17x behind. Multiplies with the row above. Mixes two devices' numerics inside one output, so it belongs behind the turbo toggle. |
| Chunk length and overlap | linear in both | `overlap = 2` processes every sample twice, and attention cost scales linearly with chunk length. Both are upstream inference defaults and both trade output quality, so neither is ours to change unilaterally. Recorded as the size of the prize, not as a plan. |
| Reuse one worker session per analysis | ~1.8 s per model execution | 17 executions in the full sweep, so about 31 s. Protocol already allows several tasks per process. |
| Cache decoded input inside a session | 1-2 s per repeated decode | The song was decoded 17 times in one sweep and Leap's vocal four times. |
| Prefetch the next model's weights | 1-5 s | Concentrated in the two multi-GB speech models. Must never be why a running task fails. |
| CPU reference lane thread count | 1.7x on that lane | Done: `UTA_STUDIO_GGML_CPU_THREADS`, measured 9.98 s to 5.88 s on a twelve-second RMVPE run. |

**Retired by these measurements**: F16 attention accumulation, and casting attention K/V to F16.
The first costs precision and buys no time; the second is already what the public schema does.

**Also retired**: caching the STFT plan and window, parallelising STFT frames and
channels, and overlapping the host frontend with GPU execution. The entire host frontend is 1.3% of
a chunk and already runs while the GPU is busy. Perfecting it would save under six seconds on a
full song.
