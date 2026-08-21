# RoFormer Runtime Vulkan phase-one record

Date: 2026-08-21

Status: the original upstream runtime builds with GGML/Vulkan. BS-RoFormer Vocals EP317
and MelBand-RoFormer Inst V2 each returned successfully from one default-path
12-second Intel Arc B580 inference, but the machine hard-locked several minutes
after the second call and required a reboot. The default Intel cooperative-
matrix path therefore fails the current stability requirement. After reboot,
all five required models completed consecutive real 12-second smokes with
`GGML_VK_DISABLE_COOPMAT=1` and 10-second inter-model checks. That is direct
short-smoke evidence, not a long-duration stability verdict. A subsequent
354.88-second original-song EP317 run hard-locked the machine during its first
minute even with cooperative matrices disabled. The conservative Intel Vulkan
path therefore also fails sustained-load stability and is not supported.

After that failure, the runtime boundary was replaced by Uta Studio's in-repo
helper calling the current GGML 0.20.2 Vulkan API directly. With all prior
feature-disable overrides cleared, a crash-recoverable serialized-submission
run completed the same 354.88-second song in 610.07 seconds. A second run with
asynchronous Vulkan restored completed in 355.33 seconds. An asynchronous
Asphodelos run subsequently demonstrated that batch one is stable for this
workload while batch two can hard-reset the B580 host. The Denoise graph later
hard-reset on its second asynchronous batch-one compute. A narrowly scoped
two-chunk rerun completed both computes only after disabling asynchronous
Vulkan, serializing every submission wait, and running CPU preprocess, GPU
compute, and CPU postprocess in strict order. This isolates a viable diagnostic
schedule. An explicitly authorized full-track rerun with that exact schedule
then completed all 39 graphs in 83.37 seconds without a kernel GPU error. It is
still a diagnostic path rather than evidence that asynchronous Denoise is safe.
A lighter-weight `--vulkan-no-async` mode (no per-submission fence waits or
diagnostic logging) then passed a full-track overlap-four Denoise run at 2.22x
that diagnostic run's speed with a byte-identical output. The same flags were
not safe in general: chaining that output into a same-length Dereverb run
under the identical `--vulkan-no-async` schedule cut host power at chunk
107/159, requiring a manual reboot. Per-graph validation is required before any
`--vulkan-no-async` or `--vulkan-fast` run; passing evidence for one graph does
not transfer to another.

This is development evidence, not an installed-model contract. Converted models
and build intermediates remain under `/tmp`; persistent diagnostic logs and
generated smoke WAV files are under the ignored
`target/roformer-diagnostics` directory. Installed checkpoints, source music,
configuration, model directories, and library data were read only.

## Implementation lineage and direct runtime

| Item | Observed value |
| --- | --- |
| Runtime | Uta Studio `native-inference/roformer` direct GGML helper |
| Graph/GGUF reference | `yasoukyoku/BSRoformer.cpp` revision `a7b9625f0f4146cacf3c46080d1139833cd4d4c2` (MIT) |
| GGML revision | `8c63e70982c95ceb862e3a1073a2c1beef75d60a` (`0.20.2`) |
| Build | Release C++17 helper; direct GGML and Vulkan linkage |
| Enabled GPU backend | `GGML_VULKAN=ON` |
| Disabled GPU backends | CUDA, Metal, and SYCL |

The resulting `uta-roformer-runtime` links `libggml-vulkan.so` and
`libvulkan.so.1`. Its dynamic
dependency scan found no SYCL, Level Zero, or oneAPI library. `--help` completed
and exposed the expected model, input WAV, output WAV, chunk-size, and overlap
arguments plus batch size, Vulkan device selection, durable logging, complete
feature restoration, serialized Vulkan diagnostics, and asynchronous fast mode.

The no-model tests `test_audio`, `test_chunking_logic`, and
`test_cancel_callback` passed. Model-dependent tests were deferred until an
audited GGUF existed.

## EP317 conversion

The conversion used BSRoformer.cpp's upstream `scripts/convert_to_gguf.py`
directly. The conversion environment reused Uta Studio's existing managed
PyTorch environment and the already-present llama.cpp `gguf-py` source tree;
it installed or downloaded no package. Static dependency inspection after the
incident showed that this XPU-enabled Torch build directly links
`libtorch_xpu`, `libsycl`, and the Unified Runtime loader even when the
checkpoint is loaded with `map_location="cpu"`. The conversion logs contain no
XPU operation, but this environment is not accepted as a clean CPU-only
conversion environment for future isolation tests.

| Item | Observed value |
| --- | --- |
| Source model | Installed `bs_roformer_vocals_ep317/model.ckpt` |
| Architecture | `bs_roformer` |
| Source parameters | 159,758,796 |
| Requested output type | `--dtype fp16` |
| Output size | 305.26 MiB |
| GGUF file type | `1` (`F16`) |
| GGUF tensor count | 702 |
| Tensor types | 306 F16 weights, 393 F32 norm/bias or other sensitive tensors, 3 I32 buffers |

No Q8 or Q4 conversion was performed. The mixed F16/F32 layout is the upstream
converter's precision-preserving policy: matrix weights use F16 while norm and
bias tensors remain F32.

The other four exact installed checkpoints were converted with the same FP16
command and inspected before inference:

| Model | Parameters | GGUF bytes | Tensor types |
| --- | ---: | ---: | --- |
| MelBand-RoFormer Inst V2 | 393,520,260 | 787,918,656 | 420 F16, 528 F32, 3 I32 |
| MelBand-RoFormer Karaoke | 228,203,172 | 457,008,736 | 300 F16, 384 F32, 3 I32 |
| MelBand-RoFormer Denoise | 228,203,172 | 457,008,736 | 300 F16, 384 F32, 3 I32 |
| MelBand-RoFormer Dereverb | 228,203,172 | 457,008,736 | 300 F16, 384 F32, 3 I32 |

## Intel Arc smoke

A 12-second, 44.1 kHz stereo PCM fixture was created under `/tmp` by mixing a
matching pair of authorized cached lossless vocal and instrumental stems. The
two source stems were not modified. The smoke selected Vulkan device zero
through `GGML_VK_VISIBLE_DEVICES=0` and ran the upstream CLI with the converted
EP317 model.

Observed device and runtime output:

```text
Intel(R) Arc(tm) B580 Graphics (BMG G21)
Intel open-source Mesa driver
fp16: 1; bf16: 1; warp size: 32
backend: Vulkan0
loaded tensors: 702
chunk_size: 352800
num_overlap: 4
graph nodes: 2104
compute buffer: 594.883 MiB
```

The six processing chunks completed in 12.3558 seconds; total process wall time
was 12.706 seconds. The single returned vocal stem was written as an actual
PCM-float WAV and passed a full FFmpeg decode.

| Output check | Result |
| --- | --- |
| Duration | 12.000 seconds |
| Sample rate | 44,100 Hz |
| Channels | 2 |
| Codec/sample format | PCM 32-bit float / `flt` |
| Mean volume | -19.2 dB |
| Max volume | -4.9 dB |
| Complete decode | Pass |

An earlier sandboxed attempt exposed only llvmpipe. It was interrupted as soon
as the device log identified a CPU Vulkan device and is not counted as model or
GPU evidence. The passing run used direct `/dev/dri` access and explicitly
identified the Arc B580 before inference.

### Inst V2 call and subsequent hard lock

Inst V2 loaded 951 tensors and built a 2,470-node graph with a 743.38 MiB
compute buffer. Its five processing chunks returned in 14.988 seconds and the
CLI wrote the complete 12-second output at 15:56:32. The log explicitly named
the same Arc B580 Vulkan device.

The later timeline is:

| Time | Direct evidence |
| --- | --- |
| 15:56:32 | Inst V2 output file completed |
| 15:57:26 | Karaoke FP16 GGUF conversion completed; no GPU inference |
| 15:58:32 | Denoise FP16 GGUF conversion completed; no GPU inference |
| 15:59:35 | Dereverb FP16 GGUF conversion completed; no GPU inference |
| 16:00:06 | Last persistent journal entry on the old boot |
| 16:02:39 | New boot began after the hard lock and manual recovery |

There is no normal shutdown record, OOM report, kernel panic, persistent `xe`
reset message, or pstore crash record. The absence of a GPU error does not
clear the GPU: the machine stopped writing logs abruptly, matching the earlier
hard-lock class where the final driver event may not reach persistent storage.

The passing device log reported `matrix cores: KHR_coopmat`. GGML revision
`8c63e709` contains an Intel Xe2/Xe3-specific cooperative-matrix warptile path.
Its runtime guard is `GGML_VK_DISABLE_COOPMAT`; the similarly named
`GGML_VK_DISABLE_COOPMAT2` controls the NVIDIA
`VK_NV_cooperative_matrix2` extension and does not disable the B580 path. Both
successful calls used the default KHR cooperative-matrix path.

This evidence supports a delayed Intel `xe`/Mesa/GGML Vulkan stability failure,
but it does not yet distinguish a cooperative-matrix shader/driver defect from
another Vulkan cleanup defect. The XPU-linked conversion environment is also a
confounder and must be removed from any future reproduction.

### Post-reboot conservative Vulkan matrix

The host was rebooted before this matrix. Each invocation used a fresh CLI
process with both `GGML_VK_VISIBLE_DEVICES=0` and
`GGML_VK_DISABLE_COOPMAT=1`. Every device line reported the Arc B580 and
`matrix cores: none`; no Torch, SYCL, Unified Runtime, or Level Zero process was
started during inference. Per user direction, the inter-model observation was
10 seconds.

| Model | Input | Inference time | Mean / max volume | Result |
| --- | --- | ---: | --- | --- |
| EP317 | real lossless mix | 23.648 s | -19.2 / -4.9 dB | Pass |
| Inst V2 | real lossless mix | 23.465 s | -26.0 / -6.1 dB | Pass |
| Karaoke | real lossless mix | 9.599 s | -19.3 / -5.2 dB | Pass |
| Denoise | real lossless vocal | 9.623 s | -19.2 / -5.1 dB | Pass |
| Dereverb | real lossless vocal | 8.023 s | -19.6 / -6.4 dB | Pass |

All five outputs are 12.000-second, 44.1 kHz, stereo PCM-float WAV files. They
passed complete FFmpeg decode and were non-silent. EP317 and Inst V2 outputs
were not byte-for-byte identical to their earlier cooperative-matrix outputs;
this is consistent with a different floating-point accumulation path, so the
conservative setting still requires model-quality comparison rather than only
container validation.

No current-boot `xe`/GPU hang, reset, fault, timeout, OOM, or panic entry
appeared during the five-model matrix or its 10-second checks. Because the
earlier hard lock was delayed by minutes, this result does not yet prove the
absence of a delayed driver failure.

### Full-song sustained-load failure

The next test passed the authorized source WAV unchanged to EP317:

| Source property | Value |
| --- | --- |
| Duration | 354.880 seconds |
| Codec | PCM signed 16-bit WAV |
| Sample rate / channels | 44.1 kHz / stereo |
| Loaded interleaved samples | 31,300,416 |

The process again used a fresh CLI, `GGML_VK_VISIBLE_DEVICES=0`, and
`GGML_VK_DISABLE_COOPMAT=1`. The device line reported the Arc B580 and
`matrix cores: none`. It loaded the FP16 EP317 model and began processing, then
the display blacked out during the first minute and the machine required a hard
reboot. No output stem was published.

The old boot's last persistent journal entry was at 16:21:10 and the new boot
began at 16:24:16. As with the earlier failure, there is no normal shutdown,
OOM, persistent `xe` reset/fault, kernel panic, or pstore record. This test did
not load Torch, SYCL, Unified Runtime, or Level Zero, so those runtimes are not
required to trigger the sustained-load failure.

The observed low VRAM use and slow projection are explained by the upstream
chunking implementation rather than by full-song model loading. With
`chunk_size=352800` and `num_overlap=4`, the step is 88,200 samples. Reflection
padding expands this song to 16,179,408 stereo frames, producing 184 sequential
model chunks. The runtime uploads the approximately 305 MiB model once and
reuses one 594.883 MiB compute graph; each chunk performs a synchronous GGML
graph compute followed by a device-to-host result copy. Disabling cooperative
matrices also changed the 12-second EP317 time from 12.356 to 23.648 seconds.
Using the measured conservative-path chunk rate, a successful full run would
take roughly 12 minutes while still using only about one chunk's working set in
VRAM.

### Direct GGML crash-recoverable rerun

The next implementation no longer invoked the upstream CLI. Uta Studio's
in-repo `native-inference/roformer` helper directly linked GGML 0.20.2 and its
Vulkan backend. It explicitly initialized Vulkan device 0 and rejected an
implicit CPU fallback. A sandbox-only preflight demonstrated that failure path;
the real run used authorized `/dev/dri` access and reported:

```text
Vulkan0: Intel(R) Arc(tm) B580 Graphics (BMG G21)
device_id: 0000:07:00.0
fp16: 1; bf16: 1; int dot: 1; matrix cores: KHR_coopmat
```

The diagnostic invocation cleared every inherited `GGML_VK_DISABLE_*`
override. It enabled debug markers, memory logging, serialized submissions, and
an opt-in GGML patch that logs each submitted graph-node range immediately
before and after its fence wait. The helper opened the log with `O_DSYNC`,
redirected stdout and stderr to it, and explicitly called `fdatasync` after each
Uta Studio stage event.

| Observation | Result |
| --- | ---: |
| Source duration | 354.880 s |
| Model precision | GGUF F16 policy; no quantization change |
| Chunk size / overlap | 352,800 / 4 |
| Chunks completed | 184 / 184 |
| Submission begin/end records | 55,568 / 55,568 |
| Model / compute Vulkan allocations | 305.22 / 594.88 MiB |
| Processing / total process time | 609.004 / 610.068 s |
| Final Vulkan allocation counter | 0 B |
| Kernel GPU hang/reset/fault records during run | none |

The generated vocal stem is a 354.880-second, 44.1 kHz stereo PCM Float32 WAV
of 125,201,708 bytes. FFmpeg decoded the entire file. Its overall RMS was
-14.536 dB, peak was +1.055 dB, and it contained zero NaN, Inf, or denormal
samples. The positive float peak is preserved by the Float32 container and was
not clipped to integer PCM during this smoke.

The serialized run survived for more than ten minutes and completed cleanly,
so the former hard lock did not reproduce under that execution schedule. It is
not a performance result: 302 fence waits per chunk and synchronous log writes
deliberately reduce concurrency. A short asynchronous `--vulkan-fast` run kept
the same durable per-stage log and reduced processing from 7.307 to 3.957
seconds. Its WAV was byte-identical to the serialized short-run WAV.

The same-quality asynchronous full-song run then completed all 184 chunks in
354.761 seconds; total process time was 355.332 seconds, approximately 1.00x
real time and 1.72x faster than serialized diagnostics. The output passed a
complete FFmpeg decode and was byte-identical to the serialized full-song WAV.
No GPU hang, reset, fault, or timeout appeared in the run window, and visible
device memory remained bounded rather than growing with chunk count.

### Asphodelos batch comparison and hard reset

The next source was the authorized `03. Rena — Asphodelos.flac`. FFmpeg decoded
it on CPU to a 305.813333-second, 44.1 kHz stereo Float32 WAV before GPU work;
the source FLAC was not modified. The CLI accepted `--batch-size` and
`--vulkan-device` as runtime inputs, logged their effective values before model
initialization, and retained batch one as the default.

A 12-second real-audio preflight processed two chunks in one batch-two graph.
It completed in 3.893 seconds versus 3.957 seconds for batch one, and its WAV
was byte-identical to the batch-one output. This established tensor-layout
correctness for that fixture but only improved short-run time by about 1.6%.

The full asynchronous batch-two run did not complete:

| Observation | Batch two result |
| --- | ---: |
| Total chunks | 159 |
| Last accumulated progress | 96 chunks / 60.412819% |
| Last completed compute | chunks 95-96, success |
| Final durable event | chunks 97-98 `batch.compute.begin` |
| Compute buffer | 1,857,807,360 bytes |
| Reported free / total device memory | 8,866,000,896 / 12,809,404,416 bytes |
| Output WAV | not published |

Device memory stayed bounded, so the failure was not a gradual VRAM leak or
OOM. The durable log ended inside a GPU compute without a GGML/Vulkan error
return. The next boot recovered the root filesystem journal and reported that
another filesystem had not been properly unmounted. There was no orderly
shutdown, pstore record, prior-boot `xe` reset, AMD-Vi page fault, kernel panic,
or OOM record; the kernel did not get a chance to persist a cause before the
hard reset.

The batch-two graph used the KHR cooperative-matrix path because
`--vulkan-fast` cleared `GGML_VK_DISABLE_COOPMAT`. This closely matches
[Intel IGCIT #1330](https://github.com/IGCIT/Intel-GPU-Community-Issue-Tracker-IGCIT/issues/1330):
Arc GGML/Vulkan AI workloads with larger prompts or models caused display
blackouts, GPU failures, and in some reports a required hard reset despite free
VRAM; multiple reporters used `GGML_VK_DISABLE_COOPMAT=1` as a workaround. That
is strong evidence for the cooperative-matrix/driver path but not proof of the
lowest-level fault on this Linux host. AMD IOMMU was active in translated mode,
yet the absence of any `AMD-Vi IO_PAGE_FAULT` makes the IOMMU-conflict hypothesis
weaker than the directly matching GGML/Vulkan failure class.

After reboot and the required 10-second observation interval, the same decoded
Asphodelos WAV ran with explicit `--batch-size 1 --vulkan-device 0
--vulkan-fast`:

| Observation | Batch one result |
| --- | ---: |
| Chunks completed | 159 / 159 |
| Compute buffer | 623,780,352 bytes |
| Processing / total process time | 307.168657 / 308.497 seconds |
| Output duration | 305.813333 seconds |
| Codec / format | PCM Float32 / 44.1 kHz stereo |
| Overall RMS / peak | -14.952303 / +0.577593 dB |
| NaN / Inf / denormal samples | 0 / 0 / 0 |
| Kernel GPU errors in run window | none |

The entire output decoded successfully. Batch one is therefore the retained
default and the only accepted batch size for the current Arc B580 phase-one
matrix.

A final explicit workaround test set `GGML_VK_DISABLE_COOPMAT=1` after enabling
fast mode; the log confirmed `matrix cores: none`. Its two-chunk preflight
completed in 7.115 seconds, compared with 3.893 seconds with cooperative
matrices enabled. The same Asphodelos batch-two full run then completed only 30
of 159 chunks (18.249706%). Its final durable boundary was chunks 31-32
`batch.compute.begin`; the host hard-reset again and no output WAV was
published. The prior boot ended without an orderly shutdown and a new boot
began at 18:01:20.

This second reproduction disproves cooperative-matrix disablement as a
sufficient batch-two mitigation. The hard-reset trigger is associated with the
larger batch graph or its sustained Vulkan scheduling, not solely with KHR
cooperative-matrix kernels. The CLI and public runtime API now reject every
batch size other than one before model/GPU initialization. This safety boundary
is based on two directly observed full-machine hard resets; ordinary type,
version, or unit-test checks cannot prevent a driver/firmware hang caused by a
valid but unsafe GPU workload.

### Denoise chained-input failure at batch one

After the successful EP317 batch-one run, its verified 305.813333-second vocal
WAV was passed unchanged to the FP16 MelBand-RoFormer Denoise model. The direct
helper used explicit Vulkan device zero, asynchronous fast mode, and the now
mandatory batch size one. The model and first chunk did execute:

| Observation | Result |
| --- | ---: |
| Model tensors | 687 |
| Graph nodes | 1,834 |
| Compute buffer | 567,095,184 bytes |
| First chunk compute | success / 1,186.71 ms |
| First chunk download and ISTFT | success |
| Last durable boundary | second chunk `batch.compute.begin` |
| Free / total device memory at boundary | 9,841,639,424 / 12,809,404,416 bytes |
| Output WAV | not published |

The log stopped 3.23 seconds after process start and the host hard-reset. The
next boot recovered the root journal and found an improperly unmounted
filesystem; no OOM or persistent GPU fault was recorded. This is not model-load
failure, batch-two pressure, or VRAM exhaustion. It demonstrates that batch one
is necessary for EP317 but is not sufficient to qualify the MelBand Denoise
graph on this Intel Vulkan stack. Denoise is therefore failed for the current
asynchronous direct-runtime path and must not be retried unattended or treated
as a supported cleanup stage.

### Denoise serialized two-chunk isolation

The authorized rerun deliberately did not use the full track. CPU FFmpeg
created a 16.000-second, 44.1 kHz stereo Float32 fixture from the verified
Asphodelos EP317 output. With `chunk_size=352800` and overlap one, that fixture
exercised exactly two consecutive model chunks. The invocation kept batch size
one and all Vulkan math features enabled, but set `GGML_VK_DISABLE_ASYNC=1`,
enabled the durable serialized-submission path, and selected the runtime's
strict CPU-preprocess -> GPU-compute -> CPU-postprocess schedule.

| Observation | Result |
| --- | ---: |
| First graph | 1,834 nodes / 283 serialized submissions / 2,655.03 ms |
| Second graph | 1,834 nodes / 285 serialized submissions / 3,037.97 ms |
| Processing / total process time | 5.943853 / 7.511916 s |
| Output | 16.000 s PCM Float32, 44.1 kHz stereo |
| NaN / Inf / denormal samples | 0 / 0 / 0 |
| Complete FFmpeg decode | pass |

Both graph calls and their device-to-host downloads returned success, including
the second call that previously hard-reset the machine. Device memory remained
bounded and all tracked Vulkan allocations returned to zero on process exit.
This result is consistent with a Vulkan submission/synchronization or sustained
concurrency defect rather than invalid Denoise weights or an inherently invalid
second graph. Because three variables changed together--GGML asynchronous
submission, per-submission fence serialization, and the host pipeline--the run
does not prove which one is individually necessary. No full-track Denoise run
was implied by this result; the following full-track run received separate,
explicit authorization.

### Denoise serialized full-track pass

The full 305.813333-second Asphodelos EP317 Float32 output was then passed
unchanged to the same Denoise model. The run retained every setting from the
passing isolation: FP16 GGUF, batch one, chunk size 352,800, overlap one, all
Vulkan math features enabled, asynchronous Vulkan disabled, durable serialized
submission waits, and the strict host pipeline. After process exit, the host
was observed for ten seconds before checking the kernel log.

| Observation | Result |
| --- | ---: |
| Graphs completed | 39 / 39 |
| Submission wait begin / end | 11,113 / 11,113 |
| Mean / min / max graph compute | 2,064.208 / 1,855.060 / 3,028.540 ms |
| Processing / total process time | 83.371197 / 84.538716 s |
| Throughput | 3.67x real time |
| Output | 305.813333 s PCM Float32, 44.1 kHz stereo |
| Overall RMS / peak | -14.953224 / +0.574947 dB |
| NaN / Inf / denormal samples | 0 / 0 / 0 |
| Complete FFmpeg decode | pass |
| Kernel GPU errors during run and observation | none |

Device memory remained bounded and the runtime's allocation counter returned
to zero during cleanup. This full-song result shows that the conservative
submission schedule can sustain the Denoise graph on this host, despite the
same graph hard-resetting on its second asynchronous call. It does not identify
which of the three scheduling changes is individually necessary, and it does
not qualify the faster asynchronous path.

Overlap one was intentionally retained to avoid changing the passing isolation
while increasing duration. It removes cross-chunk overlap and minimizes the
number of graph calls. The user auditioned that output and confirmed audible
chunk seams.

### Denoise serialized overlap-four pass

The same full input and execution schedule were run again with overlap four.
This increased graph calls from 39 to 159 and restored overlap-add blending at
each chunk boundary. The process completed normally and was followed by the
same ten-second observation and kernel-log check.

| Observation | Result |
| --- | ---: |
| Graphs completed | 159 / 159 |
| Submission wait begin / end | 45,313 / 45,313 |
| Processing / total process time | 349.310215 / 349.959874 s |
| Speed relative to overlap one | 4.19x slower |
| Output | 305.813333 s PCM Float32, 44.1 kHz stereo |
| Overall RMS / peak | -14.953093 / +0.575290 dB |
| NaN / Inf / denormal samples | 0 / 0 / 0 |
| Complete FFmpeg decode | pass |
| Kernel GPU errors during run and observation | none |

Vulkan allocations again returned to zero. This run is the quality-oriented
audition candidate for checking whether overlap-add removes the confirmed
eight-second seams. Its timing also makes the optimization target explicit:
retain batch one, strict host-stage ordering, and `GGML_VK_DISABLE_ASYNC=1`, but
test without per-submission fence serialization, submit logging, memory
logging, or debug markers. Per-chunk begin/end events remain sufficient to
identify the last entered graph after a hard reset.

### Denoise `--vulkan-no-async` full-track pass

The stated optimization target was run next: the same full 305.813333-second
Asphodelos EP317 vocal output, batch one, chunk size 352,800, overlap four, and
`GGML_VK_DISABLE_ASYNC=1` via `--vulkan-no-async --serial-pipeline`, but with
per-submission fence serialization, submit logging, memory logging, and debug
markers all cleared. This isolates diagnostic overhead from the synchronous-
submission cost measured in the overlap-four diagnostic run above. After
process exit, the host was observed for the usual delayed kernel-log check.

| Observation | Result |
| --- | ---: |
| Graphs completed | 159 / 159 |
| Mean / min / max graph compute | 835.291 / 822.595 / 923.660 ms |
| Processing / total process time | 156.994 / 157.504 s |
| Speed vs. fully-serialized overlap-four diagnostic run | 2.22x faster |
| Throughput | 1.95x real time |
| Output | 305.813333 s PCM Float32, 44.1 kHz stereo |
| Byte comparison to serialized overlap-four output | identical |
| NaN / Inf / denormal samples | 0 / 0 / 0 |
| Complete FFmpeg decode | pass |
| Kernel GPU errors during run and observation | none |

The output was byte-for-byte identical to the fully-serialized overlap-four
WAV, so this scheduling change altered only timing, not the produced samples.
Removing per-submission fence waits and durable diagnostic logging accounts for
nearly all of the earlier 4.19x overlap-four slowdown: mean graph-compute time
dropped from 2,064.208 ms (fully serialized) to 835.291 ms per chunk. This is a
pass for the `--vulkan-no-async` full-track Denoise path at overlap four. It
does not qualify the fully asynchronous (`--vulkan-fast`) path, which still
hard-resets this host on Denoise's second batch-one graph compute; disabling
GGML asynchronous submission remains necessary evidence, not just the removed
logging overhead. It also does not qualify `--vulkan-no-async` for any other
graph: the very next test, below, hard-failed the host on the MelBand-RoFormer
Dereverb graph under the identical flags.

### Dereverb `--vulkan-no-async` full-track failure and host power loss

The Denoise `--vulkan-no-async` output above was chained unchanged into the
FP16 MelBand-RoFormer Dereverb model, the first time this direct GGML/Vulkan
helper had run that graph at all. A 16.068-second fixture cut from the start of
the Denoise output first passed cleanly: 100% complete in 15.562 seconds, valid
FFmpeg decode, no kernel GPU errors, host stable afterward. The full
305.813333-second Denoise output was then passed unchanged, keeping every
setting from the passing Denoise run: batch one, chunk size 352,800, overlap
four, `--vulkan-no-async --serial-pipeline`, vulkan-device 0.

| Observation | Result |
| --- | ---: |
| Graphs completed | 106 / 159 |
| Last completed compute | chunk 106, success, 66.705826% |
| Final durable event | chunk 107 `batch.compute.begin`, no matching end |
| Free / total device memory at last event | 9,785,016,320 / 12,809,404,416 bytes |
| Output WAV | not published |

The process stopped producing log entries at 2026-08-21T09:56:00.526Z (18:56:00
JST). The user reported a full black screen; unlike the earlier documented
Intel hard locks, this incident cut power rather than leaving a frozen but
powered display, and the user performed a manual power-cycle reboot. The
previous boot's own journal (not just the kernel ring buffer) has no shutdown
record: an unrelated background process (`hiraya-finder`) was still logging
normally every ten seconds through 18:55:29 JST, then the journal for that boot
ends with no panic, OOM, machine-check, or `xe` fault entry, and the next boot
started at 18:57:07 JST. Device memory was not exhausted (9.79 GiB free of
12.81 GiB) before the failure, so this is not a VRAM-pressure explanation.

This directly falsifies treating `--vulkan-no-async --serial-pipeline` as a
generally safe optimization mode: it passed a full-track, overlap-four,
159-chunk Denoise run cleanly, then failed a same-shape, same-length,
same-flags Dereverb run at chunk 107 of 159. The passing Denoise result must
not be extrapolated to other graphs, and no further `--vulkan-no-async` or
`--vulkan-fast` run should be attempted on Dereverb, or assumed safe on any
untested graph, without new explicit authorization and a narrower isolation
plan (for example, bisecting toward the smallest chunk range that reproduces
the Dereverb failure, the way the Denoise two-chunk isolation was done for the
fully-serialized diagnostic path).

### Dereverb bisection and a second, earlier power loss

Two follow-up experiments tested whether the failure was tied to the specific
audio content near chunk 107 or to sustained GPU load carried over from the
preceding Denoise run.

A 35.0-second fixture cut directly from the failing region (the source seconds
that chunk 107 covers) was run through Dereverb with the identical
`--vulkan-no-async --serial-pipeline` flags in a fresh process. It completed
normally in 24.832 seconds, decoded completely, and left the host stable. That
audio content is therefore not independently fatal to the graph in a
low-submission-count run, which argues against a purely content-specific
trigger.

The full 305.813333-second Denoise output was then re-run through Dereverb a
second time, unchanged except that this invocation was deliberately started
fresh, with roughly 90 idle seconds and no preceding GPU work in the same boot
or process history:

| Observation | Result |
| --- | ---: |
| Graphs completed | 49 / 159 |
| Last completed compute | chunk 49, success, 30.835712% |
| Final durable event | chunk 50 `batch.compute.begin`, no matching end |
| Free / total device memory at last event | 9,795,502,080 / 12,809,404,416 bytes |
| Output WAV | not published |

This run failed earlier than the chained attempt (chunk 50 of 159 vs. chunk
107 of 159) despite having no preceding GPU load, which falsifies the
sustained-load/cumulative-submission-count hypothesis as a necessary
condition. The user again reported a full black screen and power loss requiring
a manual reboot. The new boot's kernel log additionally recorded, 27 seconds
after boot, a userspace segfault unrelated to this helper's own process:
`surface-DP-7[1561]: segfault ... in libEGL_mesa.so.0.0.0 ... likely on CPU 5`,
naming a desktop-compositor output thread crashing inside the same Mesa EGL
stack this runtime links against. That segfault occurred during normal desktop
session startup after the reboot, not during a roformer invocation, but it is
independent evidence that this host's Mesa/Intel Vulkan/EGL stack is broadly
unstable right now, not only under this helper's specific workload.

Combined with the differing, non-repeating failure chunk indices (107, then
50) for the same input, model, and flags, this points to a stochastic
driver/firmware-level fault in Dereverb's sustained Vulkan submission on this
host, not a deterministic function of chunk content, chunk index, or prior
session load. `--vulkan-no-async` must be treated as unsafe for Dereverb on
this host without cooperative matrices, until a materially different
mitigation exists. No further hardware run against Dereverb should be
attempted without new explicit authorization, and two power-loss incidents in
one session is reason to pause broader native Vulkan hardware testing
generally pending user direction.

### Dereverb `--vulkan-no-async` full-track pass with cooperative matrices disabled

The next authorized test added `GGML_VK_DISABLE_COOPMAT=1` on top of
`--vulkan-no-async --serial-pipeline`, unchanged batch one and overlap four. A
16-second smoke first confirmed the device line reported `matrix cores: none`
on both the Arc B580 and the AMD iGPU and completed in 15.6 seconds with no
kernel error. The full 305.813333-second Denoise output was then run through
Dereverb again.

| Observation | Result |
| --- | ---: |
| Graphs completed | 159 / 159 |
| Mean / min / max graph compute | 1,359.24 / 1,341.68 / 1,554.99 ms |
| Processing / total process time | 240.560 / 241.631 s |
| Output | 305.813333 s PCM Float32, 44.1 kHz stereo |
| RMS / peak (overall) | -16.118439 / -0.785135 dB |
| NaN / Inf / denormal samples | 0 / 0 / 0 |
| Complete FFmpeg decode | pass |
| Kernel GPU errors during run | none |
| Host state after run | same boot throughout; no reboot |

This is the first clean full-track Dereverb pass on this direct GGML/Vulkan
helper. Disabling cooperative matrices costs roughly 1.6x the per-chunk compute
time compared with the earlier COOPMAT-enabled no-async Denoise run (1,359 ms
vs. 835 ms mean), consistent with the earlier EP317 COOPMAT-disabled slowdown
recorded in this document. One clean pass after two stochastic, differently-
located failures is supporting evidence, not proof of reliability: the same
non-deterministic failure signature seen at chunks 107 and 50 could in
principle still occur on a later run.

### Dereverb full-track failure with default async submission and COOPMAT disabled

The next authorized test isolated which of the two changes — disabling async
submission (`--vulkan-no-async --serial-pipeline`) or disabling cooperative
matrices (`GGML_VK_DISABLE_COOPMAT=1`) — was doing the work. A 16-second smoke
with `GGML_VK_DISABLE_COOPMAT=1` and neither `--vulkan-no-async` nor
`--serial-pipeline` (default async submission, three-stage overlap, `matrix
cores: none` confirmed in the device log) completed cleanly in the same
pattern as every prior short smoke. The full 305.813333-second Denoise output
was then run through Dereverb under that same configuration.

| Observation | Result |
| --- | ---: |
| Graphs completed | 70 / 159 |
| Last durable progress | chunk 70, success, 44.051014% |
| In-flight work at failure | chunk 71 compute begun; chunk 75 preprocessing already begun (three-stage overlap keeps multiple chunks in flight) |
| Output WAV | not published |
| Host result | black screen, power loss, manual reboot |

This resolves part of the causal question: `GGML_VK_DISABLE_COOPMAT=1` alone is
not sufficient, and `--vulkan-no-async --serial-pipeline` alone was already
shown insufficient (the chunk-107 and chunk-50 failures above both used it with
COOPMAT enabled).

### Dereverb full-track failure with `--vulkan-no-async` but no `--serial-pipeline`

A fourth configuration tested whether `GGML_VK_DISABLE_ASYNC=1` alone, without
the CLI's own `--serial-pipeline` single-chunk-in-flight ordering, was
sufficient together with COOPMAT disabled. A 16-second smoke passed. The full
305.813333-second Denoise output was then run through Dereverb with
`--vulkan-no-async` (no `--serial-pipeline`) and `GGML_VK_DISABLE_COOPMAT=1`.

| Observation | Result |
| --- | ---: |
| Graphs completed | 46 / 159 |
| Last durable progress | chunk 46, success, 28.947809% |
| In-flight work at failure | chunk 47 compute begun; chunk 51 preprocessing already begun concurrently |
| Output WAV | not published |
| Host result | black screen, power loss, manual reboot |

This is the fourth power-loss incident in this session and the fourth
distinct partial configuration to fail, each at a different chunk (107, 50,
70, 46 — no repeating failure point, reinforcing that this is a stochastic
driver-level fault tied to concurrent/overlapping GPU submission rather than
specific content, chunk index, or session history). Setting
`GGML_VK_DISABLE_ASYNC=1` changes how each individual graph compute call waits
internally, but by itself does not stop the CLI's default three-stage overlap
from keeping multiple chunks' preprocess/upload/compute/download/postprocess
work concurrently in flight across threads; that concurrency is evidently the
remaining unsafe factor. Only `--serial-pipeline`, which forces exactly one
chunk through CPU-preprocess -> GPU-compute -> CPU-postprocess at a time
before starting the next, removes it.

The only configuration that has passed a full-track Dereverb run on this host
combines all three conditions at once: batch one, overlap four,
`--vulkan-no-async --serial-pipeline` (both together, not either alone), and
`GGML_VK_DISABLE_COOPMAT=1`. All three are necessary; none is sufficient
individually or in any tested pairing. Do not test further partial
combinations against Dereverb on this host — four crashes across four
partial configurations already establish the pattern, and repeated hard
power loss carries its own risk (filesystem dirty-bit recovery has already
been observed after an earlier incident in the companion XPU test record).
Treat the fully serialized, COOPMAT-disabled configuration as the only
accepted way to run Dereverb through this helper, and still require explicit
authorization for each attempt.

A second full-track run of that exact configuration (batch one, overlap four,
`--vulkan-no-async --serial-pipeline`, `GGML_VK_DISABLE_COOPMAT=1`) completed
159/159 chunks in 241.877 seconds, decoded completely, 0 NaN/Inf/denormal, and
produced a byte-identical WAV to the first passing run. Host stayed on the
same boot throughout with no kernel error.

### Dereverb reliability stress test: 11/11 consecutive passes

Nine further consecutive full-track runs of the identical configuration
(batch one, overlap four, `--vulkan-no-async --serial-pipeline`,
`GGML_VK_DISABLE_COOPMAT=1`) were then run back-to-back in one script, each
starting immediately after the previous run's process exited, with no
cooldown interval.

| Observation | Result |
| --- | ---: |
| Runs in this batch | 9 (numbered 3-11) |
| Exit code | 0 for all 9 |
| Byte comparison to the first passing run | identical for all 9 |
| Complete FFmpeg decode | pass for all checked |
| Host reboots during the ~36-minute batch | 0 (single boot throughout) |
| Kernel GPU errors during the batch | none |

Combined with the two earlier passes, this configuration has now completed
**11/11** full-track Dereverb runs with byte-identical output and no host
failure, back-to-back with zero cooldown between runs. This is materially
different evidence from the single earlier pass: the four prior failures
(under partial configurations lacking one of the three required conditions)
struck at inconsistent, non-repeating chunk indices with no apparent pattern,
so a single pass could plausibly have been luck. Eleven consecutive passes
with no failure make that explanation implausible. Treat `--vulkan-no-async
--serial-pipeline` with `GGML_VK_DISABLE_COOPMAT=1` (batch one, overlap four)
as a reliably validated configuration for running MelBand-RoFormer Dereverb
full-track through this helper on this host. Still require explicit
authorization before further hardware runs on other graphs or other flag
combinations; this validation is specific to Dereverb with this exact flag
set and does not transfer to Denoise, EP317, Inst V2, Karaoke, or any
asynchronous/partial-serialization variant.

## Karaoke

MelBand-RoFormer Karaoke (`melband_roformer_karaoke_aufr33_viperx`) shares the
same graph topology as Denoise and Dereverb (1,834 nodes, 567,095,184-byte
compute buffer, identical GGUF tensor layout), so the Dereverb-validated
configuration was tried directly rather than repeating the full bisection.

A first 16-second smoke, run with fully default settings (async submission
enabled, no `--serial-pipeline`, COOPMAT enabled) to establish a baseline,
crashed the host with a black screen and power loss inside a 16-second window
— worse than Dereverb, whose short smokes always completed regardless of flag
combination and only its full-track runs failed under unsafe configurations.
Karaoke should therefore be treated as at least as fragile as Dereverb under
default async scheduling, not more forgiving.

A second 16-second smoke using `--vulkan-no-async --serial-pipeline` with
`GGML_VK_DISABLE_COOPMAT=1` (batch one, overlap four) completed cleanly. The
full 305.813333-second Dereverb output (denoise -> dereverb -> karaoke chain)
was then run through Karaoke under that same configuration.

| Observation | Result |
| --- | ---: |
| Graphs completed | 159 / 159 |
| Processing / total process time | 248.351 / 250.127 s |
| Output | 305.813333 s PCM Float32, 44.1 kHz stereo |
| RMS / peak (overall) | -16.187941 / -0.726097 dB |
| NaN / Inf / denormal samples | 0 / 0 / 0 |
| Complete FFmpeg decode | pass |
| Kernel GPU errors during run | none |
| Host state after run | same boot throughout; no reboot |

This is a first clean full-track pass for Karaoke under the same
`--vulkan-no-async --serial-pipeline` + `GGML_VK_DISABLE_COOPMAT=1`
configuration validated 11/11 for Dereverb. Unlike Dereverb, Karaoke has not
yet been repeated multiple times, so treat this as a single pass, not
established reliability — the same stochastic failure class documented for
Dereverb could in principle still occur on a later Karaoke run.

MelBand-RoFormer Inst V2 remains completely untested on this native
GGML/Vulkan helper (no smoke, no full-track, under any configuration).

## Karaoke

MelBand-RoFormer Karaoke (aufr33 + viperx) shares Dereverb's graph topology
exactly (1834 nodes, 567,095,184-byte compute buffer, 457,008,736-byte FP16
GGUF), so it had never previously been run through this direct GGML/Vulkan
helper at all — only through the original, since-abandoned upstream CLI in the
very first Intel Arc smoke phase.

A 16-second smoke under default settings (async submission enabled, no
`--serial-pipeline`, COOPMAT enabled) crashed the host with a black screen and
power loss, worse than any Dereverb result: every Dereverb short smoke passed
regardless of configuration, and only full-track runs failed. Karaoke failing
on a 16-second default-config smoke suggests it may be more fragile than
Dereverb under unsafe scheduling, though this is one data point.

The validated safe configuration (batch one, overlap four, `--vulkan-no-async
--serial-pipeline`, `GGML_VK_DISABLE_COOPMAT=1`) was then run full-track,
chained from the same Dereverb output used elsewhere in this document.

| Observation | Result |
| --- | ---: |
| Graphs completed | 159 / 159 |
| Processing / total process time | 248.351 / 250.127 s |
| Output | 305.813333 s PCM Float32, 44.1 kHz stereo |
| RMS / peak (overall) | -16.187941 / -0.726097 dB |
| NaN / Inf / denormal samples | 0 / 0 / 0 |
| Complete FFmpeg decode | pass |
| Kernel GPU errors during run | none |
| Host state after run | same boot throughout; no reboot |

This is one clean pass, not yet the eleven-pass reliability bar established
for Dereverb. A separate follow-up attempt intended to test the CPU backend
(`UTA_STUDIO_ROFORMER_FORCE_CPU=1`) instead ran with the variable unset —
confirmed from its own logged `environment name=UTA_STUDIO_ROFORMER_FORCE_CPU
value=<unset>` line and ~1-second-per-chunk compute timing consistent with
Vulkan, not CPU — so it silently fell through to the default unsafe Vulkan
config (async, no serial-pipeline, COOPMAT enabled) and crashed the host. That
crash is not new evidence about the CPU backend; it is a repeat of the
already-known-unsafe default GPU config, consistent with every other test of
that configuration in this document. The CPU backend
(`ggml_backend_init_by_type(GGML_BACKEND_DEVICE_TYPE_CPU, ...)` in
`native-inference/roformer/src/graph.cpp:107-119`) remains completely
untested — no successful or failed run has yet actually exercised it. Confirm
`Using backend: CPU` (not `Vulkan0`) in a run's own log/stdout before treating
any future CPU-mode result as real CPU evidence.

## Inst V2

MelBand-RoFormer Inst V2 has a different graph than Denoise/Dereverb/Karaoke:
depth 12 vs. 6, 2,470 nodes vs. 1,834, a 779,490,384-byte compute buffer vs.
567,095,184, and model-native defaults of `chunk_size=485100, num_overlap=2`
rather than the others' `352800`/model-native overlap. It had never been run
through this direct GGML/Vulkan helper before (only through the abandoned
upstream CLI in the original Intel Arc smoke phase). Input was the full
305.813333-second Asphodelos mix (`asphodelos-full-f32.wav`), not a chained
stem, since Inst V2 separates instrumental directly from a source mix.

A 16-second smoke with the same starting configuration used for
Dereverb/Karaoke (`--vulkan-no-async --serial-pipeline`,
`GGML_VK_DISABLE_COOPMAT=1`, batch one, model-native overlap) passed cleanly.
The full track was then run under the same configuration and completed
without incident on the first attempt.

| Observation | Result |
| --- | ---: |
| Graphs completed | 58 / 58 |
| Processing / total process time | 247.527 / 248.767 s |
| Output | 305.813333 s PCM Float32, 44.1 kHz stereo |
| RMS / peak (overall) | -10.837522 / +2.118606 dB |
| NaN / Inf / denormal samples | 0 / 0 / 0 |
| Complete FFmpeg decode | pass |
| Kernel GPU errors during run | none |
| Host state after run | same boot throughout; no reboot |

One clean pass on the first attempt, with no prior short-smoke crash (unlike
Karaoke's 16-second default-config failure). Not yet repeated to the
eleven-pass reliability bar established for Dereverb.

## OpenVINO GPU path (exploratory, separate from the GGML/Vulkan work above)

This is a parallel investigation into whether Intel's own OpenVINO stack is a
viable alternative to the direct GGML/Vulkan helper for this Arc B580 host.
It is exploratory Python tooling under a scratchpad directory, not part of
`native-inference/roformer`; nothing here has been integrated into the
production pipeline.

### Existing ONNX/OpenVINO coverage before doing any new conversion work

A web search before converting anything found:

- `bs_roformer_vocals_ep317` (EP317): a third party already published an ONNX
  export of the exact same checkpoint
  (`model_bs_roformer_ep_317_sdr_12.9755.ckpt`, viperx) at
  [xycld/BS-RoFormer-ONNX](https://huggingface.co/xycld/BS-RoFormer-ONNX),
  MIT-licensed, STFT/ISTFT kept outside the ONNX graph (same split this
  document's native runtime already uses). Not yet used.
- A different MelBand-RoFormer vocals checkpoint (KimberleyJSN's, not one of
  this catalog's 5 models) has an official Intel-published OpenVINO IR,
  [Intel/vocals_mel_band_roformer_kimberleyJSN_openvino](https://huggingface.co/Intel/vocals_mel_band_roformer_kimberleyJSN_openvino),
  split into three IRs (`mel_band_pre`/`mel_band_fwd`/`mel_band_post`) with
  STFT/ISTFT baked into IR too (using OpenVINO's native `DFT`/`IDFT` ops) --
  proof the whole pipeline including STFT is OpenVINO-convertible, but not a
  checkpoint this project actually uses.
- The other 4 models used in this catalog (Denoise-aufr33, Dereverb-anvuew,
  Karaoke-aufr33_viperx, Inst V2-pcunwa) had no existing ONNX/OpenVINO export
  found; converting them is new work, sharing one recipe since they are all
  the same `MelBandRoformer` PyTorch class (`audio_separator`'s
  `uvr_lib_v5/roformer/mel_band_roformer.py`) with different weights/depth.

### PyTorch to ONNX export (Karaoke, as the first model converted)

The exported subgraph mirrors the native GGML runtime's own split: only the
real-valued freq-index-select -> band-split -> transformer stack -> mask-
estimator path is traced; STFT/ISTFT and complex-dtype masking stay outside
in Python (calling the same `MelBandRoformer.forward()` code, unmodified,
before/after the exported graph).

The legacy TorchScript-based `torch.onnx.export` tracer (`dynamo=False`)
OOM-killed the export process twice at ~26 GiB RSS on this 30 GiB/0-swap
host when tracing at the model's real 801-frame chunk size (naive O(n^2)
attention, since `flash_attn=True`/SDPA is not cleanly ONNX-exportable at
this opset, retained by the legacy tracer's graph-recording overhead far
beyond what the same computation costs in plain eager PyTorch, which does
not OOM at the same size). Switching to the new `torch.export`-based exporter
(`dynamo=True`, needing the `onnxscript` package) fixed this: same 801-frame
real chunk size, `torch.inference_mode()`, no OOM. A separate rotary-embedding
caching gotcha was also found and avoided: `rotary_embedding_torch`'s lazy
`cached_freqs` buffer bakes a fixed-size ONNX constant at whatever sequence
length is used for tracing, so tracing at a small dummy size and relying on
ONNX `dynamic_axes` does not generalize (a 41-frame trace produced a
`41 by 801` broadcast failure at the real 801-frame size) -- trace directly at
the real inference size instead, matching how the native GGML runtime itself
caches one compiled graph per exact `n_frames` rather than using dynamic
shapes.

Checkpoint loading matched exactly (`missing=0, unexpected=0` from
`load_state_dict`), and the exported ONNX graph matched the real
`MelBandRoformer.forward()`'s core stage numerically at the real 801-frame
chunk size: max abs diff `5.215406e-07` (onnxruntime vs. PyTorch).

### ONNX to OpenVINO IR conversion

`openvino.convert_model()` requires a file path in this OpenVINO version
(2026.3.0), not an in-memory ONNX `ModelProto`. `ov.save_model()` defaults to
FP16-compressed weights (`karaoke_core.bin` was 456 MiB vs. the ONNX external
data's 914 MiB, roughly half). A CPU-compiled correctness check against
onnxruntime showed `max abs diff: 3.831867e-03`, `mean abs diff: 6.888575e-07`
-- a small-max/tiny-mean pattern consistent with FP16 precision, not a
correctness bug, and at a level already accepted throughout this document's
FP16-GGUF-based native runtime results.

### GPU device enumeration on this NixOS host

`openvino.Core().available_devices` initially returned only `['CPU']` despite
`/dev/dri/renderD128`/`renderD129` being world-accessible and Level Zero
loader debug tracing (`ZE_ENABLE_LOADER_DEBUG_TRACE=1`) showing the Level
Zero ICD (`libze_intel_gpu.so.1`, matching this host's known
`intel-compute-runtime-26.27.39122.11`) loading successfully. The actual
cause: OpenVINO's GPU plugin depends on `libOpenCL.so.1` (OpenCL, not Level
Zero directly), and this host's default library search path resolves that
name to a **32-bit** build (`ELFCLASS32`, likely from a Steam/Wine 32-bit
compat layer) before any 64-bit one. `clinfo` confirmed the correct 64-bit
OpenCL stack works once explicitly selected. The fix, needed for every
OpenVINO GPU invocation on this host:

```sh
export LD_LIBRARY_PATH="/nix/store/r48746qznwqxxl9qzd8f08ny8mg1dg2y-gcc-15.3.0-lib/lib:/nix/store/fkcbg2c1w29jr5yp9awai9w3v1wvxdk9-zlib-1.3.2/lib:/nix/store/xng8djkzwxw36qmw262pp42swn72bb2c-graphics-drivers/lib:$LD_LIBRARY_PATH"
export OCL_ICD_FILENAMES="/nix/store/6070sr314a07yfy8ql5abdim5gicr6nw-intel-compute-runtime-26.27.39122.11/lib/intel-opencl/libigdrcl.so"
export LD_PRELOAD="/nix/store/cmwjnm3l05msn4lpggns77kh8ds95b9p-ocl-icd-2.3.5/lib/libOpenCL.so.1"
```

With this set, `available_devices` correctly reports `CPU`, `GPU.0`/`GPU.1`
(Arc B580, dGPU, listed twice), and `GPU.2` (the Ryzen 8700G's Radeon 780M
iGPU, `gfx1103`).

### FP16-default GPU compute precision produced NaN on real audio

A single-inference GPU smoke test (`torch.randn` synthetic input, batch of
noise, no real audio) passed cleanly: compiled in 3.49 s, inferred in 0.28 s,
0 NaN/Inf, `max abs diff` vs. CPU IR `4.363488e-03` (same FP16-precision
pattern as the CPU check). This was misleading: feeding real audio -- both a
near-silent opening chunk and a normal-loudness mid-song chunk -- through the
same GPU-compiled IR produced **NaN masks directly from the GPU inference
call itself** (verified by comparing intermediate tensors step-by-step
against the real `MelBandRoformer.forward()`, which had no NaN on the
identical input). Re-converting the IR with `compress_to_fp16=False` did not
fix it (identical NaN, and the CPU-vs-onnxruntime diff was bit-identical to
the FP16-compressed IR's, `3.831867e-03`, suggesting OpenVINO's plugins were
never actually computing at the stored precision). The actual fix was
explicit compute-precision selection at compile time, independent of stored
weight format:

```python
compiled_gpu = core.compile_model(ov_model, "GPU.0", {"INFERENCE_PRECISION_HINT": "f32"})
```

`compiled_gpu.get_property("INFERENCE_PRECISION_HINT")` confirmed
`<Type: 'float32'>` after this, and the NaN disappeared entirely: `max abs
diff: 2.384186e-07` against the PyTorch reference on the same real,
previously-NaN-producing audio chunk. The root cause is that OpenVINO's GPU
plugin defaults to a reduced internal compute precision (independent of the
IR's stored weight precision) for performance, and that reduced precision is
numerically unstable on real audio's structured spectrum specifically --
synthetic i.i.d. random-noise input did not trigger it, which is why the
first GPU smoke test looked clean. Any future OpenVINO GPU validation in this
project must test with real audio, not only synthetic noise, and must set
`INFERENCE_PRECISION_HINT` (or equivalent) explicitly rather than trusting
the plugin default.

### Full-track reliability stress test (in progress)

With the precision fix applied, a Python pipeline chunks the full
305.813333-second Asphodelos track (`chunk_size=352800`, `num_overlap=4`,
Hann-window overlap-add, reflect-padded, 153 chunks) exactly like the native
runtime, calling the OpenVINO GPU IR for the core graph and reusing
`MelBandRoformer`'s own STFT/ISTFT/complex-masking code outside it. This was
launched as a 10-run-in-a-row stress test, mirroring the earlier 11x Dereverb
GGML reliability run, specifically because sustained/repeated GPU load is the
trigger condition named in the known open issue
[openvinotoolkit/openvino#32665](https://github.com/openvinotoolkit/openvino/issues/32665)
(Arc B580 + `xe` driver GPU-plugin VRAM leak leading to a GPU hang).

| Run | Chunks | Elapsed | NaN | Inf | RMS | Peak |
| --- | ---: | ---: | ---: | ---: | ---: | ---: |
| 1 | 153/153 | 105.4 s | 0 | 0 | 0.1549 | 0.9188 |

This section will be updated as the remaining runs complete. GPU inference
alone (excluding CPU-side STFT/ISTFT/overlap-add) was roughly twice as fast
per full track as the native GGML runtime's validated
`--vulkan-no-async --serial-pipeline` + `GGML_VK_DISABLE_COOPMAT=1` path
(105 s vs. ~240-250 s), though this is not a fully apples-to-apples
comparison: this Python pipeline's STFT/ISTFT/overlap-add run on CPU in a
different process/language than the native runtime's, and only one run has
completed so far, versus the native path's 11/11 confirmed reliability.

## Remaining work

- Treat the direct helper as phase-one evidence, not production support. Earlier
  default and conservative runs both caused full-machine hard locks, while the
  serialized direct run changed submission timing and passed.
- Keep batch size at one. The helper rejects larger values before GPU
  initialization; do not weaken that boundary without a materially different
  driver/runtime and a newly authorized isolation plan.
- Do not repeat the Denoise sustained-load test on the current direct
  GGML/Vulkan asynchronous path. It hard-reset on the second batch-one graph
  compute. The explicitly authorized serialized full-track run passed, but that
  result does not qualify asynchronous submission.
- Compare conservative-path stems against the current reference implementation
  with a model-quality metric before promoting the generated GGUF files into
  the user model directory.
- Treat the five-model short matrix as model-load/function evidence only. It
  does not qualify the Intel Vulkan backend for production.
- Reducing overlap can improve speed but changes the separation-quality tradeoff
  and is not part of the same-quality baseline.
- `--vulkan-no-async --serial-pipeline` passed a full-track Denoise run at
  batch one, overlap four (byte-identical output to the fully-serialized
  diagnostic run, 2.22x the speed), but is not a general clearance: the
  identical flags cut host power during a same-length Dereverb run at chunk
  107/159. Treat this mode as per-graph, unverified-until-tested evidence, not
  a default. It is also not a clearance for `--vulkan-fast`/asynchronous
  submission, which independently hard-resets this host on Denoise's second
  batch-one graph compute.
- Do not run `--vulkan-no-async`, `--vulkan-fast`, or any other GPU workload on
  Dereverb through this helper again without new explicit authorization. Two
  independent 2026-08-21 full-track attempts (one chained after Denoise, one
  started fresh with no preceding GPU load) both cut host power, at chunk
  107/159 and chunk 50/159 respectively, with no persisted fault record either
  time; both required a manual power-cycle reboot. The differing failure points
  and the fresh-start attempt failing earlier than the chained one rule out
  cumulative session load as a necessary cause; treat this as a stochastic
  driver-level fault specific to Dereverb's graph under sustained Vulkan
  submission on this host.
- After the second incident, a userspace segfault in `libEGL_mesa.so` (desktop
  compositor output thread, unrelated to this helper's own process) appeared in
  the fresh boot's kernel log. This host's Mesa/Intel Vulkan/EGL stack should be
  treated as broadly fragile right now, not stable outside of this helper's
  workload either.
- `--vulkan-no-async --serial-pipeline` with `GGML_VK_DISABLE_COOPMAT=1`
  (batch one, overlap four) is now reliably validated for Dereverb full-track
  on this host: 11/11 consecutive passes, all byte-identical, including 9
  back-to-back with zero cooldown. All three conditions are required
  together — every tested configuration missing one of the three (async
  enabled, or no `--serial-pipeline`, or COOPMAT enabled) failed with a host
  power loss, at four different non-repeating chunk indices (107, 50, 70, 46).
  This validation is specific to Dereverb with this exact flag set; it does
  not transfer to other graphs or other scheduling combinations.
- Before Speech Runtime receives an Intel Vulkan sustained-load test, resolve or
  avoid the shared GGML/Mesa/`xe` failure class. RMVPE remains last.
