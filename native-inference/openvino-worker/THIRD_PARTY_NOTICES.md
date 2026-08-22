# Uta Studio OpenVINO worker — third-party notices

The Uta Studio worker source is distributed under GPL-3.0-only. It interfaces
with the following independently licensed components:

- **OpenVINO Runtime 2026.3.0**, its bundled OpenCL ICD loader, and
  `openvino-rs` — Apache License 2.0; Copyright Intel Corporation, Khronos
  Group, and contributors. Runtime source and third-party license texts are
  copied by the explicit source-build recipe.
- **rustfft** — dual-licensed MIT OR Apache-2.0; Copyright its contributors.
- **RMVPE reference algorithm and model** — MIT; reference implementation
  copyright 2023 liujing04, 源文雨, and Ftps; model lineage copyright 2022
  lj1995. The model is not bundled and is installed only after explicit user
  confirmation.
- **FFmpeg** — used as a separately packaged or explicitly configured native
  executable under the license terms of that build.

Full dependency license texts are available from their upstream distributions
and installed runtime license directory. This notice does not alter those
licenses.
