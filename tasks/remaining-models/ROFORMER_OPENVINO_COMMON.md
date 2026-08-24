# RoFormer → Intel/OpenVINO — Common Technical Contract

**Purpose:** shared instructions for task cards 03–07 only
**Accelerator authorization:** follow `docs/agent-tasks/MODEL_GPU_WORK_POLICY.md`

Read this file only when executing one RoFormer card. Do not preload all model cards.

## 1. Architecture

Preferred production architecture:

```text
44.1 kHz stereo PCM
  -> deterministic native chunking / STFT / mel-band packing
  -> source-verified OpenVINO neural graph
  -> deterministic native band scatter / complex-mask application / iSTFT
  -> overlap-add
  -> 44.1 kHz stereo semantic stem
```

The current generic OpenVINO `decode_mono()` path is not valid for RoFormer separation.

Do not force STFT/iSTFT into IR merely to make the graph monolithic. Correctness, parity, bounded memory, and auditability take precedence.

## 2. Source policy

Do not convert from GGUF. GGUF is historical/native-runtime evidence, not the canonical OpenVINO source.

For the current model card:

```text
obtain/reuse exact checkpoint and YAML/config
verify revision + filename + size + SHA-256 before loading
record checkpoint license separately
never infer architecture parameters from weights when config provides them
```

The user's existing GGUFs/configs and `/tmp` source snapshots may be read as reference evidence, but OpenVINO conversion must trace to the exact source checkpoint.

Only download the exact checkpoint named by the current model card when it is not already local. Do not snapshot-download unrelated model files.

## 3. Reference baseline

Before ONNX/OpenVINO conversion, establish a **small** CPU/PyTorch or otherwise authoritative reference for the current exact checkpoint/config. A historical full-shape PyTorch/export attempt for MelBand-RoFormer reached about 22.9 GiB anonymous RSS and was OOM-killed on this host; therefore **do not begin with the model's exact full time dimension merely because the card names that shape**.

Mandatory memory-safety sequence before any exact-shape reference/export:

```text
1. isolate every heavy phase in a separate process;
2. measure available host RAM immediately before the phase;
3. reserve at least 8 GiB for the compositor/OS/other host processes;
4. place the heavy process under an enforceable memory ceiling when the host supports it
   (cgroup/systemd MemoryMax or an equivalent process limit), with the ceiling no higher
   than min(16 GiB, available_RAM - 8 GiB);
5. start with a tiny representative time dimension and record peak RSS;
6. increase the representative shape only in bounded steps while peak memory remains safely
   below the ceiling; account for attention's non-linear/quadratic growth rather than assuming
   linear scaling;
7. only run an exact full-time PyTorch reference after the measured/projection bound shows it
   can fit under the same ceiling with headroom;
8. exit the reference process completely before export; exit the exporter before ORT; exit ORT
   before OpenVINO validation.
```

The memory ceiling is a safety stop, not a substitute for fixing the exporter. If a phase hits or approaches the ceiling, stop that strategy and redesign it; do not raise the ceiling toward the historical OOM envelope, add swap, or simply retry.

Capture at least:

```text
input audio identity
STFT packed tensor shape/selected values
band membership/config
neural mask output shape/selected values
final semantic output shape/timeline/channel count
```

Use existing `/tmp/uta-native-crispasr/tools/reference_backends/mel_band_roformer.py` or a source-equivalent audited reference where applicable.

Do not use historical Vulkan output as the sole oracle.

## 4. Graph/export policy

Prefer tensor-only neural graph boundaries.

For MelBand-RoFormer, the neural island generally includes:

```text
band split
transformer stack
mask estimator
```

Native host code may own deterministic audio/DSP transforms.

Record:

```text
source checkpoint SHA-256
config SHA-256
export wrapper identity
PyTorch/ONNX/OpenVINO versions
input/output names, shapes, dtypes
ONNX SHA-256
IR XML/BIN SHA-256
conversion recipe SHA-256
```

First parity baseline should be FP32. Evaluate FP16 only after FP32 semantics are accepted.

### Mandatory export lifecycle for large RoFormer graphs

Never keep full PyTorch weights/activations, ONNX external-data tensors, an ORT session, and an OpenVINO model resident in the same process. The default safe lifecycle is:

```text
small-shape/dynamic-export feasibility probe -> exit
bounded dynamic or neural-island ONNX export -> fsync/hash artifact -> exit
ONNX checker / ORT validation -> exit
OpenVINO conversion -> fsync/hash XML+BIN -> exit
OpenVINO CPU parity -> exit
only then bounded GPU validation
```

Prefer dynamic/reshape-capable time axes exported from a small representative `T` when exact semantics permit it. A small export shape is acceptable only if the produced graph executes the exact semantic time dimension without truncating context or changing attention/receptive-field behavior. If that cannot be proven, split the model into audited tensor-only neural islands and persist intermediate tensors to disk; do not fake safety by chunking across a transformer attention/context boundary.

### Exact-context split policy for oversized MelBand graphs

For Inst V2 and future models with the same failure mode, preserve the configured time context and split the neural graph instead of reducing `T`:

```text
native CPU STFT / gather
  -> manifest-pinned CPU band-split IR
  -> per-layer GPU time-transformer IR, microbatched only across independent bands
  -> per-layer GPU frequency-transformer IR, microbatched only across independent frames
  -> bounded manifest-pinned CPU mask-estimator IR groups
  -> native CPU scatter / complex mask / iSTFT / overlap-add
```

Microbatching is valid only on an axis whose items do not attend to or otherwise interact with one another. The complete attention sequence must remain inside each invocation. Padding may be added only to the independent batch axis and must be discarded before the next semantic stage.

CPU execution in this topology is intentional, typed placement—not fallback. Every island, device, shape, layer/band range, XML/BIN identity, precision, and microbatch contract must be pinned in the converted-artifact manifest. The worker must require both CPU and GPU plugins and fail closed if either is unavailable; do not use `AUTO`, opaque `HETERO`, or silent device substitution.

Do not keep all GPU compiled islands resident merely because their weight total appears small: compiled attention graphs may reserve substantial device buffers. Measure aggregate residency and, when it exceeds the device envelope, use rolling compilation/residency or a disk-backed layer-major pipeline. Future similarly structured oversized models must follow this exact-context split method unless model-specific parity proves a safer bounded topology.

## 5. Parity sequence

Per model, strictly serial:

```text
A. source/config/hash/license audit
B. reference CPU semantics
C. ONNX export/checker
D. ONNX Runtime parity
E. OpenVINO CPU parity/reference
F. smallest Intel GPU compile/smoke
G. bounded real-audio semantic result
H. repeat/cancellation/process cleanup
I. Runtime Manager import/status/resolve
J. Engine separation route
```

Do not skip directly to GPU because `ovc` produced XML/BIN.

## 6. Accelerator authorization

Non-Qwen Vulkan or Level Zero calls require explicit user permission. Other accelerator calls have no repository GPU restriction. Record the backend/API used as part of the result evidence.

## 7. Worker/output contract

RoFormer needs a stereo separation worker/path, not the mono evidence worker contract.

Required:

```text
44.1 kHz stereo input
bounded chunk size and overlap from exact config
one model loaded at a time
progress/cancellation
semantic output role fixed by model card
no stdout log pollution if NDJSON worker is used
atomic output publication
source timeline preserved
output stereo semantics preserved
no silent CPU fallback
```

Lossless generated separation output should be FLAC; do not invent a lossy intermediate.

## 8. Runtime Manager

A successful OpenVINO path should distinguish:

```text
source checkpoint identity
converted OpenVINO artifact identity
conversion recipe identity
OpenVINO runtime identity
LocalImport/generation identity
integrity state
validation state
Production usability
license/distribution state
```

Existing Vulkan candidate evidence should remain historical rather than being deleted.

## 9. Engine integration

Only set/keep a capability implemented when the real Engine route calls the selected OpenVINO separation path with the correct semantic output.

No false substitutions between:

```text
vocals
lead vocal
instrumental
denoised clean vocal
dereverbed clean vocal
```

The current model card defines the exact semantic role.

## 10. Validation status vs license

Technical readiness and distribution legality are separate.

A model may finish:

```text
technical READY
release/package BLOCKED_LICENSE
```

Do not fabricate checkpoint license terms from source-code/runtime licenses.

## 11. Allowed checks

Use package-local checks only for modified backend crates/native components. No whole-workspace or final Nix build.

Non-Qwen Vulkan/Level Zero execution requires explicit user permission under `docs/agent-tasks/MODEL_GPU_WORK_POLICY.md`.

## 12. Completion

Write only the completion record named by the current model task card, stop all model/conversion processes, and return to the master. Never open the next RoFormer card yourself.
