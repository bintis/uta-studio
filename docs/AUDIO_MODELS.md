# Audio processing models

Uta Studio 0.5 introduces a fixed, offline Model Catalog for vocal
separation, accompaniment, karaoke, denoise, dereverb, and six-stem
Demucs. Analysis never downloads models. Installation happens only from
**Settings > Models & runtime** after the user confirms the model name,
source, size, and license.

## First catalog

| Model ID | Purpose | Architecture | Runner |
| --- | --- | --- | --- |
| `bs_roformer_vocals_ep317` | Vocal extraction | BS-RoFormer / MDXC | PyTorch (`torch_cuda` / `torch_xpu` / `torch_cpu`) |
| `melband_roformer_inst_v2` | High-quality accompaniment | MelBand-RoFormer / MDXC | PyTorch |
| `htdemucs_6s` | Six-stem separation | Demucs | PyTorch |
| `melband_roformer_denoise_aufr33` | Vocal denoise | MelBand-RoFormer / MDXC | PyTorch |
| `melband_roformer_dereverb_anvuew` | Vocal dereverb | MelBand-RoFormer / MDXC | PyTorch |
| `uvr_mdxnet_karaoke_2` | Karaoke accompaniment | MDX ONNX | OpenVINO / ONNX Runtime in a helper process |
| `melband_roformer_karaoke_aufr33_viperx` | Default analysis karaoke | MelBand-RoFormer / MDXC | PyTorch |

Checkpoint filenames are catalog internals. Saved settings store only
stable model IDs.

## Integrity and licenses

Every catalog file has a full SHA-256. UVR's short MD5 is stored only as
`uvr_metadata_hash` for MDX metadata lookup and is never used as an
integrity check.

Weights are not packaged with Uta Studio. Users download them from the
recorded sources after confirmation. Model licenses are recorded per
entry in `app-core/analyzer/audio_models/catalog.yaml` and remain
separate from the MIT-licensed reference code.

The `audio-separator` integration keeps its MIT copyright and license
text. Uta Studio does not copy the reference project's online model
directory into the runtime. Production inference goes through
`app-core/analyzer/audio_separator_adapter/`, which loads already-installed
files via `load_model_from_spec`, honors an explicit device (including
Intel XPU), and never calls `download_model_files`.

The default analysis karaoke model is
`melband_roformer_karaoke_aufr33_viperx`. If that checkpoint already
exists under `models/audio_separator/`, Settings install copies and
verifies it instead of downloading it again.

## Developer import

```sh
python3 tools/import_uvr_audio_catalog.py --print-catalog
```

This tool is not invoked by application startup, Settings rendering,
analysis, or diagnostics.
