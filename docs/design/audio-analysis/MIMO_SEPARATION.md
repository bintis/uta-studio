# Sony MIMO separation — feasibility and inspection

**Reviewed 2026-09-11. Status: feasible as a new native model integration, not an
inference-time switch over existing weights. MIMO execution is not implemented.**

The user requested an opt-in slower/higher-quality separation method in Analysis
settings and evaluation data in the DAG context-menu Inspect view. This record
keeps those requests distinct from the partial implementation below.

## Inspected upstream evidence

- [Official repository](https://github.com/SonyResearch/mimo-audio-separation),
  inspected revision `9169bbbf9e6fdca5b22d5735b4244fbad7ab40b5`.
- [Paper](https://arxiv.org/abs/2609.07226).
- [MIMO iteration/projection](https://github.com/SonyResearch/mimo-audio-separation/blob/9169bbbf9e6fdca5b22d5735b4244fbad7ab40b5/src/model/mimo_base.py).
- [Multi-source RoFormer](https://github.com/SonyResearch/mimo-audio-separation/blob/9169bbbf9e6fdca5b22d5735b4244fbad7ab40b5/src/model/backbone/bsroformer/bsroformer.py).
- [Large-model configuration](https://github.com/SonyResearch/mimo-audio-separation/blob/9169bbbf9e6fdca5b22d5735b4244fbad7ab40b5/configs/model/bsroformer-2src_base_71M.yaml).
- [Evaluation method](https://github.com/SonyResearch/mimo-audio-separation/blob/9169bbbf9e6fdca5b22d5735b4244fbad7ab40b5/docs/evaluation.md).
- [Published checkpoints](https://github.com/SonyResearch/mimo-audio-separation/releases/tag/v1.0.0).

The official README reports these MUSDB18-HQ test-set **museval median-of-medians**
SDRs, not measurements made by Uta! Studio and not scores for an arbitrary song:

| Matched upstream group | Vocals SDR | Accompaniment SDR |
| --- | ---: | ---: |
| Small ordinary BS-RoFormer | 9.64 dB | 16.80 dB |
| Small MIMO BS-RoFormer | 10.17 dB | 17.64 dB |
| Large ordinary BS-RoFormer | 11.40 dB | 18.48 dB |
| Large MIMO BS-RoFormer | 11.62 dB | 19.04 dB |

The large comparison is +0.22 / +0.56 dB. It does **not** demonstrate superiority
over our XE90 checkpoints, whose training and evaluation conditions differ. Three
iterations increase backbone work relative to the same model's single iteration;
we have not measured its wall time, memory, listening quality or performance
relative to XE90. No universal "slower but better" product claim is justified yet.

## Why an existing-model toggle would be incorrect

The inspected large model uses two stereo sources flattened to **four input
channels**, two output sources with cross-source mask weighting, Fourier/MLP time
embeddings prepended on both attention axes, and learned value-residual mixing.
It is trained for three iterations with time conditions 0, 0.5 and 1. The wrapper
peak-normalizes the mixture, initializes each source as mixture / source count,
runs the learned multi-source backbone, applies mixture-consistency projection
and restores the original scale. Its direct-output projection is:

```text
estimate[source] += (mixture - sum(estimates)) / source_count
```

This projection is only one part of the trained method. Repeated XE90 inference,
repeated residual subtraction, ordinary overlap changes or adding this projection
alone are **not Sony MIMO**. Our existing vocal + mixture-residual outputs already
reconstruct the mixture by construction; good reconstruction is not proof of good
separation.

Current native RoFormer frontends in both GGML and LibTorch consume stereo input
and expose a single estimated waveform through the worker route. They do not
implement the Sony multi-source graph/time embeddings/value residuals. The
upstream distribution contains `backbone_model.pth` plus `config.yaml`; the large
MIMO release ZIP is 266,337,935 bytes. Release metadata was read, but no checkpoint
was downloaded, installed, deserialized or executed in this task.

## Remaining native integration work

1. Implement the Sony graph and frontend with faithful multi-source STFT/iSTFT,
   mask summation, time conditioning, value residuals, normalization, ordered
   overlap-add and mixture projection. Preserve cancellation and worker ownership.
   Use native execution, never launch upstream Python as a production fallback.
2. Add native checkpoint import/container preparation via the Rust tooling and
   a Runtime Manager resource with stable unnumbered identity, such as
   `bs_roformer_mimo`. Installation remains an explicit Models & runtime action.
   Do not relabel XE90 weights or depend on model files under test evidence.
3. Connect the provider to the existing dual-output separation invocation,
   plan/request snapshot, cache identity, progress and errors. No silent switch
   back to XE90 when MIMO is missing or fails; no automatic CPU inference.
4. Implement the requested **Analysis > Audio preparation > MIMO iterative
   separation (experimental)** switch, default off. Off retains the user's
   ordinary strategy; on explicitly selects the installed MIMO resource. State
   the extra computation and unqualified per-song benefit. Existing charts/stems
   change only on explicit re-analysis. Models & runtime owns the model download,
   not this Analysis parameter. No nonfunctional switch is exposed before routing
   exists.
5. Validate synthetic primitives, complete trained outputs against the official
   implementation, source duration/stereo/lossless publication, cancellation,
   cache separation, settings persistence/error states and DAG provenance. Then
   perform an explicitly authorized bounded native GPU/reference-stem comparison
   with operation/host observations. Prior GPU pauses and excluded AMD separation
   experiments are not resumed by this feasibility review.

The requested MIMO switch and model remain **open**, not integration-ready or
production-ready. The task is not blocked on another permission for ordinary
implementation; the concrete missing work is the native model/import/catalog
implementation and qualified execution evidence.

## Implemented: truthful DAG Inspect measurements

The Engine retains `diagnostics.separation_quality` for each actual dual-output
separation execution. Each entry carries its presentation node, model, measurement
method and both run-relative artifact paths. Statistics come from the existing
FLAC output decode **before** lead isolation/denoise/dereverb:

- sample rate, channels, frames, duration and sample count;
- finite-sample status;
- peak and RMS in linear amplitude;
- near-full-scale sample proportion (existing decoder threshold `|sample| >= 0.999`);
- silent-sample proportion (existing decoder threshold `|sample| <= 0.0001`).

These are descriptive diagnostics, not new acceptance gates, calibrated
perceptual scores or a guarantee of separation quality. No model or audio arithmetic
is changed. The current result manifest/history owns them; they are not looked up
from today's active artifacts.

`inspect_separation_quality` is a discoverable **read** in-process API. It consumes
the exact Engine run snapshot, checks the request identity and selects only the
requested node. DAG Inspect calls the same API, shows provenance/units and surfaces
parse errors. Pending/reused/unmeasured nodes explicitly show no measurements;
no current cache lookup or waveform read occurs in the render path. Current
implementation records them in the final successful result: an earlier separation
followed by a later failed stage does not yet persist this report independently.

**SDR, SI-SDR, SIR and SAR require aligned ground-truth stems.** The application
currently has no reference-stem evaluation input or evaluator for this feature, so
these are explicitly unavailable, not zero and not fabricated. The original mix,
a separated guide vocal, another model estimate or residual reconstruction is not
ground truth. A future evaluator must record reference identities, alignment,
metric definition, window/aggregation method and undefined silent/perfect cases;
plain waveform error ratios must not be labeled as museval/BSS Eval SDR.

## Verification and limits

- Operation `20260911T184622-3c702ca5d882`: focused separation tests passed,
  Engine 7, app-core 8, desktop 5. Includes fake-native lossless decode/publication
  and cache/progress tests, not trained-model inference.
- The first Bevy render-state smoke compile failed on a `CommandQueue` namespace;
  corrected to `bevy::ecs::world::CommandQueue` before re-execution. Operation
  `20260911T185007-02a6a114dd90` passes four inspector tests including real in-process
  entity/text construction for measured, pending and malformed-result states.
- No GPU inference, runtime/model installation, source/library/cache mutation,
  audio audition, reference SDR measurement, application packaging or release pass.
  Render-state tests are not physical-pointer or Wayland visual qualification.

Research/download receipts and source snapshots: `test-artifacts/mimo-research/`.
Per-operation records: `test-artifacts/operations/`. Missing completion records
remain unknown; process exit never establishes post-exit host stability.
