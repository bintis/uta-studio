# B580 non-XE model verification of the declared fused attention backend

## Scope and source identity

This supplements `ROFORMER_B580_FLOATING_STORAGE.md` with independently recorded
F32/mixed-storage numerical coverage and matched Harmony, Denoise and Dereverb
model pairs. The declared backend is patch eight (`8aef559`) plus floating K/V
storage patch nine (`ba0d5ab`), not a new model-specific inference implementation.

The final runs load `test-artifacts/attention-query-residency-study/runtime-combined/lib`.
Its correspondence to the nine-patch recipe is recorded in
`test-artifacts/attention-query-residency-study/declared-source-identity.json`.
The separate inline-loader experiment and its wide-group build under
`test-artifacts/attention-multimodel-study/` are retained as experimental artifacts;
they are not substituted for the declared implementation in the final results.
No installed runtime, model file or user media was replaced.

F32 K/V storage is kept in the graph. The fused shader loads it into bounded
shared tiles using the same F16 cooperative operand conversion as the prior
cooperative decoder, with F32 score/output accumulation. There is no model-side
cast, probability-matrix allocation, shortened attention context or precision
change to the surrounding graph. Dispatch is based on device, H64 shape,
accumulator and K/V storage types, not a whitelist of model names.

## Final matched real-model pairs

All six cases use the **same combined library** and recorded test executable
`test-artifacts/attention-multimodel-study/attention-tests`. The candidate uses
the default eight-group path; the control sets
`UTA_STUDIO_GGML_FA_QUERY_OWNED=0`. `UTA_STUDIO_GGML_FA_SHARED_GROUPS` is unset in
both. Each process loads the same installed model and performs two complete
passes over the same six-second, stereo, 44.1 kHz input. The measured operation
includes the production Rust graph, WAV frontend and synthesis, but excludes
worker codec/publication. Installed model overlap settings are unchanged.

Values below are the second complete pass. Kernel times are weighted per-call
GPU timestamps, not model wall times. The time-attention shape has
78,839,930,880 QK+PV FLOPs; effective TFLOPS = FLOPs / GPU seconds / 1e12.

| Model | Time attention: control -> candidate | Kernel latency reduction | Candidate TFLOPS | Frequency attention: control -> candidate | Complete processing: control -> candidate | Processing reduction |
| --- | --- | --- | --- | --- | --- | --- |
| MelBand Harmony | 20.79210 -> 8.17280 ms | 60.69% | 9.647 | 2.21522 -> 1.22585 ms | 4.94539 -> 4.70990 s | 4.76% |
| MelBand Denoise aufr33 | 20.69730 -> 8.22417 ms | 60.26% | 9.586 | 2.21599 -> 1.22581 ms | 4.94798 -> 4.70011 s | 5.01% |
| MelBand Dereverb anvuew | 20.71865 -> 8.17987 ms | 60.52% | 9.638 | 2.21779 -> 1.22756 ms | 3.33884 -> 3.17635 s | 4.87% |

The order was candidate/control for Harmony, control/candidate for Denoise, and
candidate/control for Dereverb. These are bounded pairs, not a statistical
whole-song benchmark. No sampled external Xe CCS activity was observed in any
of the six final cases. Aggregate sampled CPU maxima were 7.24%-8.78%.
Visibility is partial; this is not proof of exclusive GPU access or fixed clocks.

Earlier four-group measurements remain in `paired-summary.json`. Their host
records detected external CCS activity in Denoise and the Dereverb control.
Those observations are exploratory, not the basis for the final speed claims.
They were not deleted, filtered away or silently relabeled as isolated results.

## Numerical and waveform evidence

All outputs contain 529,200 finite interleaved samples. Each final process's
first and second WAVs are byte-identical. Cross-control/candidate differences
were measured on every sample of the second output, not a subset:

| Model | Maximum absolute difference | SNR versus control | Cross-kernel exactness |
| --- | --- | --- | --- |
| Harmony | 0.0009318292141 | 67.04445 dB | Not bit-identical |
| Denoise | 0.0005028992891 | 74.41915 dB | Not bit-identical |
| Dereverb | 0.005778998137 | 48.74247 dB | Not bit-identical |

In a separate declared-library Dereverb run, both complete output WAVs match
the four-group floating-storage candidate byte for byte. Enlarging the shared
workgroup did not add to the measured Dereverb discrepancy. The difference is
between the prior cooperative organization and query-owned attention, not
between the four/eight-group scheduling variants. This is not a perceptual
parity claim; Dereverb's lower SNR remains an explicit listening/quality gate.

The 24 floating/mixed-storage fixtures were rerun against the declared library:
all pass, with maximum NMSE 3.1445150974831876e-7 versus the existing full-context
F64 reference and unchanged 5e-4 tolerance. Coverage includes F32/F32, F32/F16,
F16/F32, truly non-half-representable F32 values, both sides of the selection
threshold, masked tails, poisoned padding, GQA, sharp score rescaling and H128
fallback. The same 24 also passed the independent four-group experiment.
These are 24 cases run twice, not 48 distinct fixtures. The broader 61-case
combined-backend coverage is documented in `ROFORMER_B580_FLOATING_STORAGE.md`.

## Reproduction and evidence locations

The study root is `test-artifacts/attention-multimodel-study/`:

- `final-paired-summary.json`: complete timing and waveform comparison.
- `final-observation.json`: exit status, boot observations, CPU/external CCS
  samples and exact repeat comparisons for all six final cases.
- `declared-case-review.json`: numerical maxima, earlier contention and the
  declared-versus-four-group Dereverb byte comparisons.
- `<model>-<control|candidate>-final/`: command, stdout/stderr, process/host
  samples, completion record and both diagnostic WAVs.

Operation records are under `test-artifacts/operations/`:

| Operation | Record |
| --- | --- |
| Declared floating numeric regression | `20260910T044656-96285b0f58a3` |
| Declared Dereverb supplementary model | `20260910T044701-94fabba735d2` |
| Final Harmony pair | `20260910T044809-5112d845127f` |
| Final Denoise pair | `20260910T044846-1d4ddc6e233b` |
| Final Dereverb pair | `20260910T044939-da00123bedc8` |
| Final timing/audio summary | `20260910T045012-978fe71c5086` |
| Final host/completion review | `20260910T045012-f48f8dcba2b7` |

The read-only summarizer is `tools/summarize-attention-model-pairs.py`; it
understands IEEE-float and extensible IEEE-float WAV, rejects truncated/nonfinite
samples, reports weighted per-call timings, and creates a new output file rather
than overwriting evidence. The first ad-hoc analyzer rejected extensible WAV;
its failure is preserved at `20260910T043934-11861faffa7a`. That parser failure
was not an inference failure and did not cause a GPU retry.

## Remaining limits

The separately measured XE90 result remains about 12.69 TFLOPS, not 20 TFLOPS.
The same XE90 shape would require approximately 27.328 ms per time-attention
operation to reach 20 TFLOPS. These non-XE results establish acceleration for
three actual MelBand models, not every installed model or head geometry.

The supported device path here remains Intel Xe2/native SIMD32/M8/H64. Other
head dimensions and vendors keep their previous paths; absence of a code change
is not an AMD/NVIDIA regression pass. Whole-song, Windows, AMD/NVIDIA, perceptual
and formal Nix release qualification of this combination remain outstanding.
No installed artifact replacement or remote push was performed. All observed
processes exited zero with no observer errors and unchanged boot IDs; that is
not a guarantee of post-exit or universal host stability.
