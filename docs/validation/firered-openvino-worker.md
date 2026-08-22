# FireRedASR2-AED OpenVINO Worker smoke

Date: 2026-08-22

- Source artifact: `42ailab/FireRedASR2-AED-ONNX`, revision
  `13f950858934f7b6a0d3ce52bae65af0dc022258`, Apache-2.0.
- Source graph SHA-256:
  - encoder `0fe4038f5e5cd340171535b7b5f2e184482e90e22aeb2ed0f7abe81af10783f9`;
  - decoder `aeef22670d95aa90d78a1927242c2a6e4fbb8b44c1af8d3ae988c46fd67ae833`;
  - CTC `8881d31c17bca30a7972299d5395daaa6424da6328a818ba496719c3118c32b4`.
- Production process loaded OpenVINO IR only. The smoke set contains a static
  230-frame encoder, 58-frame CTC graph, and decoder cache buckets 0–10 with
  one shared immutable weights file.
- Frontend: native Rust Kaldi-compatible 80-bin fbank and binary CMVN parser;
  no script runtime.
- Fixture: upstream `hello_zh.wav` at FireRedTeam commit
  `4e7d9aaf4482a47cec1724807026b9b151926eb5`, SHA-256
  `e09abc88000e7186e9e11b4ba9ae04ea79af2173e2fc583cf8b25e0d36199061`.
- Hardware/backend: Intel Arc B580, OpenVINO `GPU`, f32 accuracy hint, no CPU
  fallback.
- Result: encoder, CTC, and full autoregressive AED loop completed. Tokens
  `[1202, 2246, 1019, 4710]` decoded to exact golden text `你好世界`.
- Worker stdout remained NDJSON-only and ended with `done: ok` and clean quit.

This is a fixed-shape runtime smoke. Variable-length bucket generation,
confidence/timestamp parity, Chinese singing quality, cancellation, and
full-song stability remain separate acceptance gates.
