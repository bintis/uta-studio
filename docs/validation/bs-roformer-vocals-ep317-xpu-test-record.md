# BS-RoFormer Vocals EP317 Intel XPU test record

Date: 2026-08-20; amended 2026-08-21 after a system hard lock

Status: the historical same-process stability verdict remains invalidated after
a black screen and manual hard reboot. The process-isolation mitigation passed
post-fix 12-second XPU smokes for BS-RoFormer and both configured cleanup models
(dereverb and denoise) on 2026-08-21, with one fresh XPU worker per attempt and
no `xe` error during the delayed observation windows. It subsequently passed one
193.333-second real-file, same-parent, three-model chain. A subsequent planned
two-song run hard-locked the machine during the first song, after dereverb and
while denoise was loading. Repeated-song stability failed; process isolation is
not a sufficient fix for this host driver/runtime combination. A temporary
compute-runtime 25.18 rollback then passed minimal and 12-second tests but
hard-locked during the first model of a 248-second real file. Long Battlemage
MDXC work is now bounded to overlapping 12-second, fresh-process windows; that
new workload mitigation has passed non-hardware tests only.

> Safety notice: the post-fix smoke is direct positive evidence for the isolated
> model paths tested, not proof that this Intel Arc B580 software stack is safe
> for unattended or repeated hardware runs. Do not rerun the historical
> device-name probe or another full-file XPU test unattended.

## Scope

This record covers the manual verification of the installed
`bs_roformer_vocals_ep317` catalog model with Uta Studio's managed Python
environment and Intel XPU backend. The checks used the production offline model
adapter and `MdxcTorchRunner`; they did not use the upstream `audio-separator`
download path.

The authorized source media was read only. Lossless 12-second and 24-second
FLAC fixtures and all generated stems were written to uniquely named
directories under `/tmp`. No model, source-media, library, settings, or chart
files were modified.

## Environment

| Item | Observed value |
| --- | --- |
| Managed Python | `/home/bintis/Documents/uta-studio/vendor/venv/bin/python` |
| Analyzer | `/home/bintis/Documents/uta-studio/vendor/analyzer` |
| Models root | `/home/bintis/Documents/uta-studio/models` |
| Catalog version | `2026.08.1` |
| Model ID | `bs_roformer_vocals_ep317` |
| Display name | BS-RoFormer Vocals EP317 |
| Architecture | `mdxc_bs_roformer` |
| PyTorch | `2.8.0+xpu` |
| XPU | Intel(R) Arc(TM) B580 Graphics |
| Requested backend | `torch_xpu` |

The managed venv depends on the runtime libraries supplied by Uta Studio's Nix
environment. For an interactive reproduction from the repository root, enter
the development environment first:

```sh
nix develop
```

Then set the paths used by the checks:

```sh
export UTA_TEST_PYTHON=/home/bintis/Documents/uta-studio/vendor/venv/bin/python
export UTA_TEST_ANALYZER=/home/bintis/Documents/uta-studio/vendor/analyzer
export UTA_TEST_MODELS=/home/bintis/Documents/uta-studio/models
export PYTHONPATH="$UTA_TEST_ANALYZER"
```

## Historical per-invocation checks and results

These results say that a call returned successfully. They do not establish that
the driver remained healthy after the call or across a model transition.

| Check | Direct evidence | Result |
| --- | --- | --- |
| XPU runtime discovery | `torch.xpu.is_available()` returned `True`; device count was `1` | Pass |
| XPU tensor execution | `torch.arange(8, device="xpu").sum().item()` returned `28` | Pass |
| Installed file integrity | Catalog SHA-256 validation succeeded for `model.ckpt` and `config.yaml` | Pass |
| Model construction | Offline adapter constructed the BS-RoFormer MDXC model | Pass |
| Device placement | All model parameters and buffers reported `xpu:0` | Pass |
| Loaded model size | 159,758,092 parameters; 623.4 MiB allocated after load | Pass |
| End-to-end separation | `MdxcTorchRunner` processed 12-second and 24-second FLAC fixtures | Pass |
| Backend selection | Requested and actual backend were both `torch_xpu`; `fallback_from` was `None` | Pass |
| Output contract | Both `extracted_vocal` and `residual_instrumental` artifacts were returned | Pass |
| Audio decode | Both outputs decoded completely with FFmpeg | Pass |

### XPU runtime probe

```sh
"$UTA_TEST_PYTHON" -c 'import torch; print(torch.__version__); print(torch.xpu.is_available()); print(torch.xpu.device_count()); print(torch.xpu.get_device_name(0)); value = torch.arange(8, device="xpu").sum().item(); print(value); assert value == 28'
```

Observed output:

```text
2.8.0+xpu
True
1
Intel(R) Arc(TM) B580 Graphics
28
```

### Model load probe

The load check resolved both installed files through the catalog integrity path,
then used `OfflineSeparator.load_model_from_spec` with
`architecture="mdxc_bs_roformer"` and `torch_backend="torch_xpu"`. After
construction, the test enumerated every parameter and buffer device.

Observed output:

```text
model: BS-RoFormer Vocals EP317
checkpoint: SHA-256 verified
devices: ['xpu:0']
parameters: 159,758,092
XPU allocated: 623.4 MiB
MODEL XPU LOAD: PASS
```

### End-to-end inference

The inference check called the production `MdxcTorchRunner.run` entry point with
the installed catalog model, default resolved model parameters, and this runtime
request:

```python
AudioRuntimeRequest(
    torch_backend="torch_xpu",
    onnx_backend="onnx_cpu",
    precision_policy="fp32",
)
```

The input fixture was a lossless 12-second, 44.1 kHz stereo FLAC excerpt made
from authorized local media. The source file itself was not changed. The runner
was invoked with `step_id="xpu_smoke"` and a unique `/tmp` work directory.

Observed runner output:

```text
[8%] Loading BS-RoFormer Vocals EP317
requested_backend: torch_xpu
actual_backend: torch_xpu
fallback_from: None
elapsed_seconds: 8.78
xpu_peak_mib: 3942.5
artifact extracted_vocal: step_xpu_smoke_vocals.wav
artifact residual_instrumental: step_xpu_smoke_instrumental.wav
MODEL XPU INFERENCE: PASS
```

Output inspection:

| Artifact | Codec | Sample rate | Channels | Duration | Mean volume | Max volume |
| --- | --- | --- | --- | --- | --- | --- |
| Residual instrumental | PCM signed 16-bit LE | 44,100 Hz | 2 | 12.000 s | -11.4 dB | -0.9 dB |
| Extracted vocal | PCM signed 16-bit LE | 44,100 Hz | 2 | 12.000 s | -19.7 dB | -2.0 dB |

The outputs were non-empty, non-silent, and fully decodable. WAV was used only
as the runner's lossless temporary stem format; no lossy bytes were labeled as
FLAC.

### 24-second end-to-end inference

A second lossless fixture was cut from the same authorized source position with
a duration of 24 seconds. It used the same production runner, catalog model,
default resolved parameters, and explicit `torch_xpu` request as the 12-second
run. It was written to a separate unique `/tmp` directory.

Observed runner output:

```text
[8%] Loading BS-RoFormer Vocals EP317
requested_backend: torch_xpu
actual_backend: torch_xpu
fallback_from: None
elapsed_seconds: 9.36
xpu_peak_mib: 3942.5
artifact extracted_vocal: step_xpu_smoke_24s_vocals.wav
artifact residual_instrumental: step_xpu_smoke_24s_instrumental.wav
MODEL XPU 24S INFERENCE: PASS
```

Output inspection:

| Artifact | Codec | Sample rate | Channels | Duration | Mean volume | Max volume |
| --- | --- | --- | --- | --- | --- | --- |
| Residual instrumental | PCM signed 16-bit LE | 44,100 Hz | 2 | 24.000 s | -12.5 dB | -0.9 dB |
| Extracted vocal | PCM signed 16-bit LE | 44,100 Hz | 2 | 24.000 s | -15.8 dB | -1.8 dB |

Both 24-second outputs were non-empty, non-silent, and completely decodable by
FFmpeg. The run stayed on XPU and did not increase the observed peak allocation
above the 12-second run's 3942.5 MiB.

## 2026-08-21 hard-lock evidence and mitigation

Before the final hardware attempt, all seven installed catalog models (14
files) passed full SHA-256 verification. Active PyTorch checkpoints also loaded
strictly on CPU, and the ONNX/OpenVINO graphs passed their available structural
checks. Model-file corruption was therefore ruled out before XPU inference.

A later short run exercised BS-RoFormer followed by MelBand-RoFormer dereverb
in one Python/Level Zero process. Both models completed their inference chunks;
the runner reported zero allocated and reserved XPU memory after cleanup. The
machine then black-screened and stopped producing journal entries, requiring a
manual hard reboot. Earlier affected boots contained `xe` driver evidence for
the same application workload, including an unsuccessful `-EFAULT` response,
`Engine memory CAT error [18] class=ccs`, engine resets, and a timed-out
`uta-studio` compute context. No OOM, thermal, machine-check, or kernel-panic
record explained the resets.

The code now preserves `torch_xpu` execution but runs exactly one Torch audio
model per short-lived helper process. BS/MelBand RoFormer and Demucs each get a
fresh Level Zero process context. Successful output is published before the
worker exits with `os._exit()`, avoiding destructor-driven XPU work; Linux also
arms a parent-death signal so a killed analyzer cannot leave inference orphaned.
The parent no longer initializes XPU merely to inspect or clear allocator state,
and XPU and CPU fallback attempts use separate output directories.

The mitigation passed process-protocol, failure-propagation, output-contract,
BF16 boundary, CPU fallback, full non-hardware Python, Rust, workspace-build,
and Nix package checks before the hardware smoke below.

## 2026-08-21 post-mitigation isolated XPU smokes

The BS-RoFormer and dereverb stages used a generated 12.000-second, 44.1 kHz
stereo `pcm_f32le` WAV. Its tones, filtered pink noise, and multi-delay echo
provided non-silent dry and reverberant content. Denoise was additionally tested
with a second 12.000-second fixture containing two tones, strong pink noise, and
high-frequency white noise. No user source media was read or modified. All test
artifacts were confined to:

```text
/tmp/uta-xpu-isolation-tY7WsUtt/
```

An initial control attempt inside the coding sandbox did not execute XPU work:
the sandbox had no `/dev/dri`, `torch.xpu.is_available()` returned `False`, and
the worker exited before inference. The same visibility check on the host found
the render devices, returned `xpu_available=True`, and reported
`xpu_initialized=False` before the test. No `xpu-smi` or device-name query was
used.

CPU fallback was disabled for all post-mitigation hardware calls. Therefore, a
returned result could not be mistaken for a CPU retry.

| Stage | XPU process | Direct result | Output evidence |
| --- | --- | --- | --- |
| BS-RoFormer vocals | Fresh worker 1 | 2/2 chunks; `actual_backend=torch_xpu`; `precision=bf16` | `extracted_vocal` and `residual_instrumental`, each 4,233,688 bytes |
| MelBand-RoFormer dereverb | Fresh worker 2, started only after worker 1 exited | 2/2 chunks; `actual_backend=torch_xpu`; `precision=bf16` | `dry_audio` and `reverb`, each 4,233,688 bytes |
| MelBand-RoFormer denoise | Fresh worker 4 with the dedicated noisy fixture | 2/2 chunks; `actual_backend=torch_xpu`; `precision=bf16` | `clean_audio` and `noise`, each 4,233,688 bytes |

The second stage consumed the first stage's extracted-vocal WAV. FFmpeg decoded
all outputs listed in the table as complete 12.000-second, 44.1 kHz stereo
`pcm_f32le` audio.

There was one deliberately retained non-pass between the dereverb and final
denoise results. Fresh worker 3 fed denoise with dereverb's already-clean
`dry_audio`. XPU inference completed 2/2 chunks, but audio-separator classified
one semantic stem as near-silent and wrote only the `other` WAV. Uta Studio
correctly rejected the incomplete deterministic output contract. This was not
recorded as a denoise pass, did not fall back to CPU, left no worker behind, and
produced no `xe` error. The strong-noise fixture was then used for worker 4 so
both denoise semantics could be measured directly.

After every attempt, no `audio_processors.xpu_worker` process remained. The
20-second delayed-failure observations after dereverb and after the successful
denoise rerun found no `xe` fault, reset, timeout, CAT error, DRM error, or GPU
hang in the current boot's kernel journal.

This is a pass for the exact short, isolated BS-RoFormer, dereverb, and denoise
paths tested. Runner calls were issued from separate test-parent invocations,
while the production-relevant XPU work happened in a fresh child each time. It
does not establish full-song, same-parent repeated-run, editor-playback, or
long-duration stability.

## 2026-08-21 real-file full-chain XPU run

One attended full-chain run used the original, read-only source file:

```text
/home/bintis/Documents/张含韵 - 相思遥.flac
```

The input was a 40,916,552-byte FLAC containing 193.333333 seconds of 48 kHz,
stereo, 24-bit PCM audio. The installed model data had already passed the full
catalog SHA-256 and structural checks documented above and was not modified
between that verification and this run.

Unlike the short smokes, all three production runner calls were issued from one
persistent analyzer parent process. The runtime request forced `torch_xpu` and
BF16 with `fallback_policy="fail"`; each model still ran in its own fresh XPU
worker process. No build, playback, or other GPU workload was run concurrently.

| Stage | Direct XPU result | Completion time | Output evidence |
| --- | --- | --- | --- |
| `bs_roformer_vocals_ep317` | 25/25 chunks; `actual_backend=torch_xpu`; `precision=bf16` | 35.071 seconds | `extracted_vocal` and `residual_instrumental` |
| `melband_roformer_dereverb_anvuew` | 25/25 chunks; `actual_backend=torch_xpu`; `precision=bf16` | 56.795 seconds cumulative | `dry_audio` and `reverb` |
| `melband_roformer_denoise_aufr33` | 25/25 chunks; `actual_backend=torch_xpu`; `precision=bf16` | 77.727 seconds cumulative | `clean_audio` and `noise` |

The full chain returned successfully after 77.909 seconds. All six outputs were
51,156,044-byte WAV files. FFprobe reported each as complete 193.333333-second,
44.1 kHz, stereo `pcm_s24le` audio, and FFmpeg subsequently decoded every file
from beginning to end without an error. No `audio_processors.xpu_worker` process
remained after completion.

The kernel journal from 11:52:40 through the 11:54:37 delayed observation point
contained no new `xe` fault, reset, timeout, CAT error, DRM error, GPU error, or
hang. The display remained usable throughout the run and observation window.

The first launch did not reach XPU. A production progress callback supplied
`model_id` twice and raised `TypeError` before the worker was started. The
executor now merges runner metadata with the plan identity, and a targeted
regression test passed before the clean rerun above. The temporary test harness
also appended a false `test_failed` event after its valid `test_completed` event
because it caught normal `SystemExit(0)`; the process exit code, summary, output
validation, and kernel evidence all show a pass, and the harness exception
handling was corrected without rerunning the GPU workload.

This establishes one complete real-song execution of the exact three-model
chain in a persistent analyzer parent. It does not establish repeated-song,
unattended, editor-playback, or broader driver/runtime stability.

## 2026-08-21 repeated-song attempt: hard lock

The requested two-song validation used one persistent analyzer parent and the
same forced `torch_xpu`, BF16, no-fallback three-model chain. The sources were
opened read-only and were scheduled sequentially:

```text
/home/bintis/Documents/崔子格 - 卜卦.flac
/home/bintis/Documents/01 - 崩壊の美学.flac
```

The first song started at 11:59:11. BS-RoFormer completed 28/28 chunks after
37.780 seconds. Dereverb completed 28/28 chunks after 62.688 seconds. Denoise
reported its loading event after 63.277 seconds, at 12:00:14, and no later test
event exists. The display then black-screened and the machine required another
hard reboot. The second song never started, so neither song is a pass for this
attempt.

The previous boot's journal stopped at approximately 12:00:01, before the last
userspace test event could be persisted. The next boot began at 12:01:47 and
reported root-filesystem journal recovery, a dirty bit, and that the filesystem
was changed. There was no contemporaneous persisted `xe`, OOM, thermal, panic,
or machine-check record from the lockup window. Journal silence does not turn
this into a pass: the missing completion event, hard reboot, and filesystem
recovery directly establish an unsafe termination.

Post-reboot artifact inspection found both BS-RoFormer intermediates complete
and fully decodable. Although dereverb had emitted its completion event, both
of its copied intermediates were zero bytes after recovery. That stage is not a
durable pass. The executor now fsyncs each intermediate into a same-directory
temporary file, atomically replaces its published path, and fsyncs the parent
directory before emitting the step-completed event. This addresses the observed
power-loss persistence failure; it does not prevent or recover the GPU lockup.
The two atomic-publication regressions and the surrounding audio-runner suite
passed 14/14 without importing or initializing XPU.

All seven installed catalog models, comprising 14 files, passed another full
SHA-256 verification after reboot. The model data remains intact. A strict
FFmpeg decode found that the first source FLAC already contains invalid frame
sync/header data; its July 10 modification time was unchanged and Uta Studio did
not write it. There was no pre-run strict-decode baseline, so this record does
not infer when that source damage occurred. The second source decoded fully.
Malformed input is a test confounder, but the runner had already supplied
decoded PCM to the models and it does not make a whole-machine kernel wedge an
acceptable application outcome.

The installed stack also has a substantial runtime skew: PyTorch is
`2.8.0+xpu` with Intel SYCL runtime `2025.1.1`, while the host Level Zero compute
driver is `26.27.39122.11`. Upstream Intel reports Battlemage compute wedges
under sustained Level Zero work and a separate multi-process regression
beginning with compute-runtime 26.14. Rolling the compute runtime back to 26.05
is evidence-backed for the latter regression, but other reported permanent BMG
wedges also occur on 26.05, so that version must not be represented as a proven
safety fix. No further XPU inference is safe merely to test another
application-level cleanup, cooldown, or unvalidated runtime pin.

Relevant upstream reports:

- [Intel compute-runtime #922](https://github.com/intel/compute-runtime/issues/922)
  records 26.05 as working and 26.14 as the first failing release for a
  multi-process Level Zero initialization regression.
- [Intel compute-runtime #948](https://github.com/intel/compute-runtime/issues/948)
  records permanent Battlemage `ccs`/`bcs` wedges and unsuccessful fault
  responses under sustained Level Zero inference, including on 26.05.
- [PyTorch #179030](https://github.com/pytorch/pytorch/issues/179030) records an
  XPU device-properties crash with newer Intel compute drivers.
- [Intel compute-runtime #966](https://github.com/intel/compute-runtime/issues/966)
  reports host page-cache corruption after Battlemage device-loss/reset storms,
  reinforcing the need to fsync application artifacts before publishing them.

Uta Studio's runtime-setup probe used the affected XPU device-name/property
query even though its subsequent XPU matrix multiplication is the actual
capability check. The property query has been removed while the real tensor
operation and synchronization remain. Its Rust contract test passed without
touching hardware.

## 2026-08-21 compute-runtime 25.18 candidate: hard lock

An explicit, temporary rollback experiment tested Intel compute-runtime
`25.18.33578.6`, the version for which the cited Arc B580 PyTorch report still
completed its device-properties call. This was a candidate experiment, not a
system installation. Official release packages for compute-runtime, gmmlib
`22.7.0`, and IGC `2.11.7` were downloaded into `/tmp`; all four published
SHA-256 values matched, and the extracted libraries had a complete dynamic
dependency closure. No host library, configured model directory, or setting
was replaced.

The host Level Zero loader's `ZE_ENABLE_ALT_DRIVERS` implementation was checked
against its official source before hardware access. With that variable set to
the candidate's absolute driver path, standard driver discovery is not run.
Loader debug output and `/proc/self/maps` then proved that the probe process
mapped exactly:

```text
/tmp/uta-studio-l0-25.18.33578.6/runtime-root/usr/lib/x86_64-linux-gnu/libze_intel_gpu.so.1.6.33578.6
```

It did not map the host `26.27.39122.11` compute driver. A minimal XPU tensor
probe returned `28.0` for `torch.arange(8).sum()`. Three independent 12-second
production-runner smokes also completed on XPU with BF16 and no fallback:

| Model | Direct result | Persisted output |
| --- | --- | --- |
| BS-RoFormer vocals | 2/2 chunks in 10.448 seconds | two complete 12-second FLOAT WAVs |
| MelBand-RoFormer dereverb | 2/2 chunks in 9.867 seconds | two complete 12-second FLOAT WAVs |
| MelBand-RoFormer denoise | 2/2 chunks in 9.931 seconds | two complete 12-second FLOAT WAVs |

All six outputs decoded completely, every worker exited, and the delayed kernel
checks found no persisted `xe` fault, reset, hang, timeout, or CAT error.

The decisive full-file run used the strictly decoded, read-only source:

```text
/home/bintis/Documents/01 - 崩壊の美学.flac
```

Immediately before the run it was 35,116,061 bytes, 248.465624 seconds, and had
SHA-256 `1f571ca5f8540197c34ce3339b0af4efc432134a9c268568cd0ea26de8011480`.
The same three-model, BF16, no-fallback chain started at 14:16:59. Only the
BS-RoFormer loading event was durably recorded; no chunk completion, step
completion, or output file exists. The machine black-screened during that first
long XPU stage and required a hard reboot. The previous boot's journal has no
entry after 14:16:31, and the next boot began at 14:18:07, so no contemporaneous
GPU fault reached persistent journal storage.

After reboot, the source retained the same size, mtime, inode, SHA-256, and
strict full-file decode result. All seven installed catalog models (14 files)
again passed full SHA-256 verification. This rules out source or checkpoint
mutation in the failed run. It also falsifies compute-runtime 25.18 as a
solution for this workload: its short-run passes do not extend to full-song
stability, and it must not be packaged or enabled as an XPU fix.

## Post-lock workload mitigation (not hardware-validated)

The failed boot ran kernel `7.1.5`; the reboot after the 25.18 lock entered a
different NixOS generation with kernel `6.18.40`. Both boots loaded Battlemage
GuC firmware `70.65.0`, so this failure is also direct counterevidence to
treating that firmware alone as a complete fix. The host is an AMD Ryzen 7
8700G and its AMD IOMMU was enabled in translated mode. There was no persisted
AMD-Vi timeout before this lock because the previous journal had already
stopped. An [independent B580 report](https://github.com/intel/compute-runtime/issues/948#issuecomment-5014401602)
found that `amd_iommu=off` changed an AMD host's whole-machine lock into a
recoverable GPU reset, while explicitly stating that it did not fix the
underlying `xe`/GuC fault. Uta Studio did not alter the host boot configuration;
that system-wide virtualization and DMA-security tradeoff is not an
application default.

The application mitigation now keeps XPU execution enabled but changes the
shape of long MDXC work on the affected Intel PCI IDs (`e20b` and `e223`):

- input longer than 12 seconds is divided into 12-second windows with 2-second
  overlap;
- every window runs in a fresh XPU worker and therefore a fresh Level Zero
  context;
- overlapping model output is linearly crossfaded, preserving the exact final
  sample count at both 44.1 and 48 kHz input rates;
- a semantic stem that the separator omits as near-silent in one short window
  is materialized as silence for that window, while a worker that emits no stem
  still fails;
- merged outputs are fsynced and atomically published only after every window
  succeeds; and
- Battlemage workers use the official Unified Runtime controls
  `SYCL_UR_USE_LEVEL_ZERO_V2=0`, `UR_L0_USE_COPY_ENGINE=0`, and
  `UR_L0_USE_IMMEDIATE_COMMANDLISTS=0`. The latter two prevent dedicated BCS
  copy work and immediate command lists in the isolated context. See Intel's
  [DPC++ runtime variable reference](https://intel.github.io/llvm/EnvironmentVariables.html)
  and the [Unified Runtime Level Zero reference](https://oneapi-src.github.io/unified-runtime/core/LEVEL_ZERO.html).

The direct failure motivating this design is concrete: all three 12-second
workers passed, while the 248-second first model hard-locked before one step
completed. Pure CPU/mocked verification passed 14/14 focused window/worker
tests and 33/33 surrounding audio-runner tests. The tests prove window bounds,
fresh-worker dispatch, exact-duration reconstruction, identity-waveform
crossfade, silent-stem handling, failure cleanup, backend honesty, and the
Battlemage sysfs detection path without importing or initializing XPU.

No post-change hardware inference was run after the reboot. The mitigation is
therefore code-complete and non-hardware-tested, not a demonstrated black-screen
fix. Another attended XPU run needs explicit authorization and should first use
the currently booted 6.18.40 kernel, one model, and a source just over the
12-second boundary so the multi-worker path is exercised with the smallest
possible exposure.

## Temporary artifacts

The observed test directories were:

```text
/tmp/uta-bs-roformer-inference-ZYotrL/
/tmp/uta-bs-roformer-inference-24s-RyjGv9/
/tmp/uta-xpu-isolation-tY7WsUtt/
/tmp/uta-xpu-full-real-JBXVQBKc/
/tmp/uta-xpu-full-real-rerun-aFWFGruo/
/tmp/uta-xpu-two-real-8vgvMjRh/
/tmp/uta-studio-l0-25.18.33578.6/
```

The two historical directories each contained an `input.flac` fixture and two
WAV stems. The isolation directory contains the generated PCM WAV fixtures and
the outputs from the four isolated attempts described above. The first
full-real directory retains the test harness and pre-XPU callback failure log;
the rerun directory contains the full event log, summary, and six validated
outputs. The two-real directory retains the last userspace events and recovered
partial outputs from the hard-locking attempt. These are ephemeral evidence,
not repository fixtures or durable baselines. Preserve the two-real directory
and the 25.18 candidate directory while investigating this incident. The latter
contains the verified candidate packages, extracted runtime, minimal and short
smoke logs and outputs, and the full-file log that stops at BS-RoFormer load.

## Conclusions and remaining coverage

The installed checkpoint data is intact. Fresh-process isolation produced valid
short smokes and one valid full real-song chain, but the next full-length attempt
again hard-locked the machine before finishing its first song. Repeated-run
safety has failed, and process isolation alone is not an adequate XPU fix.

The atomic intermediate change protects completed artifacts from the exact
power-loss persistence failure observed here. Actual XPU safety still requires
an Intel kernel/compute-driver fix or a workload-level change that prevents the
sustained queue/context wedge. Uta Studio now implements such a bounded-context
workload change, but it has not yet passed a hardware run. Neither a 26.05 pin
nor the directly tested 25.18 rollback is a verified resolution; 25.18 has now
failed the exact full-file workload. No baseline or release gate is introduced
by this document.
