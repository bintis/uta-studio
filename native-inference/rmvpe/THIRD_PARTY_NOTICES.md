# Third-party notices

## RMVPE model and algorithm

The native graph follows the RMVPE E2E0 architecture published by
[Dream-High/RMVPE](https://github.com/Dream-High/RMVPE), licensed under
Apache-2.0. The converted ONNX weight lineage is the `rmvpe.onnx` distribution
from [lj1995/VoiceConversionWebUI](https://huggingface.co/lj1995/VoiceConversionWebUI/tree/e6d0c1a17da07c33557852f9dfa2bd44cc75737d),
whose repository records the MIT license. Model provenance and algorithm
provenance remain distinct in the Runtime Manager catalog.

## RoFormer STFT reference

`src/stft.h` is based on `native-inference/roformer/src/stft.h`, which was
itself adapted from `yasoukyoku/BSRoformer.cpp`; local changes are limited to
the RMVPE integration and warning-clean size handling.

Copyright (c) 2026 沉默の金 <cmzj@cmzj.org>

Licensed under the MIT License. Permission is hereby granted, free of charge, to
any person obtaining a copy of this software and associated documentation files
(the "Software"), to deal in the Software without restriction, including without
limitation the rights to use, copy, modify, merge, publish, distribute, sublicense,
and/or sell copies of the Software, and to permit persons to whom the Software is
furnished to do so, subject to inclusion of the copyright and permission notice.

THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS OR
IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY, FITNESS
FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE AUTHORS OR
COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER LIABILITY, WHETHER IN
AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM, OUT OF OR IN CONNECTION
WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE SOFTWARE.

## dr_wav

`third_party/dr_libs/dr_wav.h` is vendored unmodified from
[mackron/dr_libs](https://github.com/mackron/dr_libs) and retains its upstream
public-domain-or-MIT dual license notice in the source file.

## GGML/Vulkan CLI plumbing

`cli/main.cpp`'s Vulkan environment-toggle and physical-device-enumeration
helpers, and `src/diagnostics.cpp`'s durable logging helpers, are copied from
`native-inference/roformer`'s own CLI and diagnostics code (this repository's
code, not third-party) -- generic GGML/Vulkan plumbing with no RoFormer-
specific logic, duplicated here because each `native-inference/<model>` crate
in this repository is its own standalone CMake project.
