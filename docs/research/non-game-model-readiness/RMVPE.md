# RMVPE provenance and OpenVINO import research

This documents the existing source-model → OpenVINO IR → LocalImport chain
without running or converting it. Source IDs refer to `SOURCE_LEDGER.md`.

## Canonical identity versus exact artifact identity

Two upstream identities must be retained:

1. `Dream-High/RMVPE` is the official PyTorch implementation of “RMVPE: A
   Robust Model for Vocal Pitch Estimation in Polyphonic Music”; its code
   repository is Apache-2.0 [M1][M2].
2. Uta! Studio's exact 361,688,443-byte `rmvpe.onnx` comes from
   `lj1995/VoiceConversionWebUI`, researched repository revision
   `e6d0c1a17da07c33557852f9dfa2bd44cc75737d`, with hosting metadata SHA-256
   `5370e71ac80af8b4b7c793d27efd51fd8bf962de3a7ede0766dac0befa3660fd`
   and repository metadata `license:mit` [M3]. The local `rmvpe-onnx` 0.2.3
   package records the same hash and attributes the ONNX model to lj1995 under
   MIT [M4].

The wrapper version `0.2.3` in the old local manifest is **not an upstream RMVPE
model release**. It is the locally installed `rmvpe-onnx` package version. The
exact model identity is the hosted ONNX path + HF revision + SHA-256.

### License result

The exact ONNX distribution and the RVC/NewComer frontend lineage are recorded
as MIT [M3][M4][M5]. MIT permits commercial use and redistribution with the
copyright/license notice. The official Dream-High code is Apache-2.0 [M1].
This is not necessarily a legal contradiction—different code/artifact
distributions can use different terms—but packaging must carry the exact ONNX
lineage's MIT notices and must not cite only Dream-High's Apache license.
Runtime Manager's current `review_required` value is incomplete.

## Source tensor/frontend contract

| Field | Exact expectation |
| --- | --- |
| Audio | Decoded/downmixed 16,000 Hz mono float32. |
| Frontend | 1,024-point periodic Hann STFT/window, hop 160 samples (10 ms), reflect centering, 128 HTK mel bins with Slaney-area normalization, 30–8,000 Hz mel range, natural-log floor `1e-5` [M4] plus repository native-frontend record. |
| Tensor | Float32 `[1,128,T]`; frame count follows centered 10 ms hops and is padded to a multiple of 32 for the model. |
| Model output | 360 salience classes per frame, accepted as `[1,T,360]` or `[1,360,T]` by the Uta worker. |
| F0 decoding | Maximum salience is confidence; frequency is a salience-weighted local average over ±4 classes in 20-cent bins, with cents offset `1997.3794`; approximate representable centers are 31.7 Hz through 2.0 kHz. It is continuous F0, not direct MIDI rounding. |
| Voicing | Uta marks voiced at confidence ≥ 0.03 but still emits continuous Hz/confidence. This threshold is Uta/runtime behavior; callers must not reinterpret every Hz value as a note. |
| Timeline | One frame every 10 ms; evidence times are `frame * 0.01 s`. |

The selected model estimates vocal F0 in polyphonic music [M2]. Uta normally
feeds its analysis-ready lead vocal, which is consistent with the pitch role
but still needs real-singing product validation.

## Existing conversion/import chain

### Source acquisition

A user/packager must possess the exact `rmvpe.onnx` bytes identified above.
Use an explicit source path such as:

`$UTA_STUDIO_RMVPE_SOURCE`

The model is not bundled and must be acquired/imported only after explicit
confirmation. The source host publishes both size and SHA-256, so source
acquisition can be deterministic [M3].

### Conversion

Repository-owned `native-inference/openvino-worker/convert-rmvpe-to-ir.sh`
expects:

- source SHA-256 `5370e71a…`;
- source-built OpenVINO 2026.3.0 at commit
  `8a17657b995fd3b4a52f8484acfcf2bb61214623` [M6];
- conversion recipe SHA-256
  `ac3df548a9e51d36b5d5817ba6988eeaaa29f168d121588fd088daf91dbdf876`;
- immutable new destination, never replacement of an existing generation.

It produces OpenVINO IR v11 static buckets for 32 through 1,024 feature frames
in steps of 32, with a shared `rmvpe.bin`. The worker windows long input with
128-frame overlap (896-frame stride) and discards half-overlap boundaries when
stitching. OpenVINO GPU uses f32 accuracy mode and
`GPU_ENABLE_LOOP_UNROLLING=NO`; there is no production CPU fallback.

### Expected IR identity

A converted bucketed generation is referenced in this document as:

`$UTA_STUDIO_RMVPE_IR_DIR`

Its manifest records every XML/BIN hash. Repository constants additionally pin:

- manifest SHA-256 `cdaf2775d8e17796daad2415bdaf7b3c915c4142fd92587c023e8d7b1b3d39fb`;
- shared weights SHA-256
  `d284ea1b4a0908072b6f0a5a1298cb510a65752db7a287e48da6eab1246be67b`.

The older single-static-shape directory is historical/superseded and must not
be imported as the current generation.

### Runtime Manager LocalImport

`runtime-manager/README.md` documents the intended two-step flow: explicitly
convert the verified source, then run `uta-runtime import model:rmvpe` against
the completed bucketed directory. Runtime Manager verifies the pinned manifest
and every file, stages an immutable generation, verifies its install manifest,
and atomically publishes it.

This means the current host's acquisition blocker is largely catalog/provenance
and lifecycle reconciliation, not missing bytes. A clean host still needs the
external exact ONNX artifact and explicit conversion action.

## Current validation conclusion

The raw validation journal has been removed. The durable conclusion is now in `docs/KEY_CONCLUSIONS.md`: RMVPE's exact source/converted identities, bucketed OpenVINO worker path, LocalImport lifecycle and ProductionPinned routing are established in current source. Current acceptance must be judged from current source/tests rather than this 2026-08-22 research snapshot.

## Repository discrepancy audit

| Field | Classification | Detail |
| --- | --- | --- |
| repository | MISSING IN REPO | Catalog source repository is `None`; exact artifact host [M3] and algorithm source [M1] should be separate fields. |
| revision | CONFLICT | Catalog stores the conversion recipe digest as source revision; it is not the ONNX host revision. |
| filename | CONFLICT | Catalog says `manifest.json`, describing converted IR rather than source model; source filename is `rmvpe.onnx`. |
| sha256 | CONFLICT | Catalog source hash field stores IR-manifest hash, while source ONNX hash is only in runtime constants/manifests. |
| license | MISSING IN REPO | `advisory`; exact artifact distribution is recorded MIT [M3][M4]. |
| source format | MATCH for installed resource, MISSING for source | `openvino_ir_v11_bucketed` is correct installed format; source ONNX needs separate identity. |
| acquisition | MATCH | LocalImport of audited IR directory. Source acquisition/conversion should remain explicit. |
| runtime ID/commit | MATCH | `openvino_2026_3`, exact commit/recipe recorded [M6]. |
| validation evidence | MATCH identity, CONFLICT scope | Evidence ID points to exact worker record, but remaining acceptance prevents treating smoke alone as complete Production proof. |
| estimated installed size | APPROXIMATE | Catalog 400 MB is plausible but not an exact receipt; manifest files should drive installed-byte reporting. |

## Required 20-question status

| # | Status | Answer |
| ---: | --- | --- |
| 1 | KNOWN | Algorithm is RMVPE; exact ONNX distribution separately identified. |
| 2 | KNOWN | Official code [M1] and exact artifact host [M3]. |
| 3 | KNOWN | Exact artifact-host revision `e6d0c1a…`; no official numbered model release is needed for identity. |
| 4 | KNOWN | `rmvpe.onnx`. |
| 5–6 | KNOWN | Exact ONNX distribution records MIT; redistribution/commercial use with notice. Preserve distinct Apache official-code notice where used. |
| 7 | KNOWN | Hosting metadata publishes exact SHA-256. |
| 8 | KNOWN | ONNX source; bucketed OpenVINO IR v11 production format. |
| 9–10 | KNOWN | Uta OpenVINO worker, OpenVINO 2026.3.0 commit and recipe identities. |
| 11 | KNOWN | 16 kHz mono. |
| 12 | KNOWN | `[1,128,T]` log-mel input; 360-class frame salience. |
| 13 | KNOWN | Continuous Hz/confidence/voiced evidence at 10 ms. |
| 14–15 | KNOWN | Frontend, overlap stitching, local-average F0 and Uta voicing threshold. |
| 16 | KNOWN | Repository-owned audited ONNX→bucketed IR route is documented. |
| 17 | KNOWN | Deterministic explicit source acquisition, conversion and LocalImport can be authored from collected metadata. A clean host still must obtain source bytes. |
| 18–19 | KNOWN | Exact source/IR/runtime evidence located and identity-matched. |
| 20 | KNOWN | Yes for complete Production product acceptance, but the unresolved work is OpenVINO GPU/CPU-reference quality and process behavior—not Vulkan. |

## Deterministic later checklist

Without Vulkan, a later agent can:

1. add separate algorithm-source, exact ONNX-host, source revision/file/hash,
   MIT notices, and converted-IR identity to catalog/install receipts;
2. make explicit conversion/LocalImport UI and CLI status recognize the
   already-present bucketed generation without replacing user data;
3. validate manifest/atomic-import contracts with isolated fixtures;
4. perform the remaining bounded real-singing/cancellation/repeat/package work
   through OpenVINO GPU, with CPU only as an explicit reference lane.

No conversion/import/validation action above was performed by this research.
