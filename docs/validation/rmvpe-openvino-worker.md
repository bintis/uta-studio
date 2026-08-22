# RMVPE native OpenVINO worker validation

Date: 2026-08-22

## Contract

- Worker: `uta-openvino-worker`
- Backend: OpenVINO `GPU` (explicit; no CPU fallback)
- Source model SHA-256: `5370e71ac80af8b4b7c793d27efd51fd8bf962de3a7ede0766dac0befa3660fd`
- Production model format: OpenVINO IR v11, static 32-frame buckets from 32 to
  1024 frames, one shared pinned weights file, 128-frame overlap
- IR manifest SHA-256: `cdaf2775d8e17796daad2415bdaf7b3c915c4142fd92587c023e8d7b1b3d39fb`
- Input: decoded lossless/unchanged source artifact through packaged FFmpeg,
  normalized to 16 kHz mono float32
- Frontend: native Rust, 1024-point Hann STFT, 160-sample hop, 128 HTK/Slaney
  mel bins, natural-log floor `1e-5`
- Output: 10 ms continuous F0/confidence Evidence JSON
- OpenVINO: source-built 2026.3.0, commit
  `8a17657b995fd3b4a52f8484acfcf2bb61214623`
- Runtime: GPU plus IR inference; the ONNX frontend exists only in the explicit
  native conversion tool. CPU, NPU, Python bindings, and unused frontends are
  disabled; the source-built OpenCL ICD loader is bundled.
- GPU compatibility: static bucket shape, f32 accuracy mode, and
  `GPU_ENABLE_LOOP_UNROLLING=NO`
- Generic Worker/runtime recipe SHA-256:
  `bd349389e6d0d0b742ae103892c1e5774599dd8733460aec80cb74bcf20ddab6`
- RMVPE bucket-conversion recipe SHA-256:
  `ac3df548a9e51d36b5d5817ba6988eeaaa29f168d121588fd088daf91dbdf876`
- Tested source-build runtime manifest SHA-256:
  `fa767fbea026b74e91abc01228c7f94551e64fd7dfcb0314b638276492b2a774`
  (the manifest pins all five runtime library hashes and is emitted into output
  provenance; a different audited system toolchain may produce a different
  binary manifest under the same source recipe)

## Automated evidence

- Native mel floor/frame-count test.
- Frame-major to channel-major/post-log-zero padded tensor layout test.
- Local salience averaging test proving output is not direct MIDI rounding.
- Worker build and clippy with warnings denied.
- NDJSON ready/progress/output/done contract remained machine-clean during
  real inference.

## Intel Arc smoke

Device: Intel Arc B580, OpenVINO GPU plugin.

A two-second stereo 48 kHz PCM 440 Hz fixture was decoded by packaged FFmpeg,
processed by the native frontend, and inferred from the pinned 224-frame
OpenVINO IR bucket. Production inference did not parse ONNX.

- Output frames: 201
- Backend reported: `openvino_gpu`
- Mean voiced F0 over 0.2–1.8 seconds: 440.717109 Hz
- Mean confidence over that range: 0.658511
- Worker exited cleanly after `done` and `quit`.
- A 12-second two-window run emitted exactly 1,201 ordered frames with a
  439.755724 Hz interior mean, validating overlap stitching and tail bucketing.

No Python process or script runtime participated. Existing installed model data
was read only.

## Remaining acceptance

This smoke validates the implementation path, not the full product lane:

- full-song real singing golden comparison;
- voiced/unvoiced fixture suite;
- repeated full-song runs;
- cancellation during frontend and GPU inference;
- coordinator Artifact commit/provenance integration;
- Vulkan/OpenVINO contention sequencing;
- packaged worker/runtime launch and license inspection.
