# Uta Studio RoFormer runtime

This helper loads the audited FP16 RoFormer GGUF models and executes their graphs
through GGML directly. It does not invoke or package a third-party RoFormer CLI.
The graph and audio adaptation started from the MIT-licensed implementation noted
in `THIRD_PARTY_NOTICES.md` and now lives behind Uta Studio's own process boundary.

The build deliberately requires an existing, audited GGML checkout. It never
downloads GGML, tools, or models as a side effect of configuring or launching:

```sh
cmake -S native-inference/roformer -B build/uta-roformer-vulkan \
  -DUTA_GGML_SOURCE_DIR=/path/to/ggml \
  -DGGML_VULKAN=ON \
  -DGGML_CUDA=OFF \
  -DGGML_SYCL=OFF \
  -DGGML_NATIVE=OFF
cmake --build build/uta-roformer-vulkan -j2 --config Release
```

The accepted GGML revision for the current phase-one run is
`8c63e70982c95ceb862e3a1073a2c1beef75d60a` (GGML 0.20.2). Apply
`patches/ggml-vulkan-durable-submit-log.patch` to that checkout for the diagnostic
build. The patch only adds opt-in submission-boundary messages; it does not change
shader selection or math.

For a crash-recoverable diagnostic run, put the log on persistent storage:

```sh
uta-roformer-runtime model-fp16.gguf input.wav output.wav \
  --enable-all-vulkan-features \
  --vulkan-diagnostics \
  --diagnostic-log /persistent/path/roformer.log
```

`--enable-all-vulkan-features` removes inherited `GGML_VK_DISABLE_*` overrides.
`--vulkan-diagnostics` serializes submissions only for diagnosis and logs every
fence wait boundary. It also disables the asynchronous Vulkan path. Every Uta
Studio diagnostic event is appended through a
synchronous file descriptor and explicitly synced before execution continues.
The normal path initializes Vulkan device 0 explicitly and fails if it is not
available; it never turns a requested GPU run into an unnoticed CPU run.

`--vulkan-no-async` is the intermediate performance-isolation mode. It sets
`GGML_VK_DISABLE_ASYNC=1` while clearing serialized-submission, submit, memory,
performance, synchronization, and debug-marker diagnostics. Combine it with
`--serial-pipeline` to retain strict CPU-preprocess -> GPU-compute ->
CPU-postprocess ordering and durable per-stage events without a fence and disk
sync for every Vulkan submission. This mode requires its own authorized smoke;
the passing serialized runs do not qualify it automatically.

Use `--vulkan-fast` after the serialized diagnostic has reproduced or cleared a
failure. It clears all feature-disable overrides and restores asynchronous GGML
Vulkan submission while retaining durable per-chunk upload, compute, download,
postprocess, progress, and save events. It does not change model precision,
chunk size, overlap, or graph math. Because it also restores the KHR cooperative-
matrix path, it is a diagnostic opt-in on Intel Arc rather than the production
stability default.

`--serial-pipeline` disables the three-stage overlap and processes one chunk as
CPU preprocessing, GPU graph execution, then CPU postprocessing. This separates
Vulkan submission failures from host-side pipeline concurrency.

`--chunk-size`, `--overlap`, `--batch-size`, and `--vulkan-device` are runtime
arguments. Vulkan device defaults to 0. Effective values are written to the
durable log before model initialization. Batch size is fixed to one on the
current runtime: a 12-second batch-two smoke produced the same WAV as batch one,
but a sustained batch-two run enlarged the compute buffer from 623,780,352 to
1,857,807,360 bytes and hard-reset the Arc B580 host at 60.4%. A second batch-two
run with cooperative matrices disabled hard-reset at 18.25%. Values above one
are rejected before GPU initialization.

Batch one is not a blanket stability guarantee for every graph. A chained
MelBand-RoFormer Denoise full-track test loaded successfully and completed its
first batch-one chunk, then hard-reset the same host at the second graph
compute. The current phase therefore records EP317 batch one as tested and the
Denoise sustained path as failed; neither result qualifies the helper for
production support.

This failure resembles the public Intel Arc GGML/Vulkan cooperative-matrix issue
in [IGCIT #1330](https://github.com/IGCIT/Intel-GPU-Community-Issue-Tracker-IGCIT/issues/1330),
but its reported `GGML_VK_DISABLE_COOPMAT=1` workaround was both substantially
slower and ineffective for the sustained batch-two load on this host. The helper
does not expose that failed workaround as a runtime option.
