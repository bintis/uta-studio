# 17A — Backing / Harmony Semantic Route Audit

**State:** `SKIPPED_ALREADY_CLOSED`
**Blocks:** none; authoritative final-v1 design keeps lead partition future/optional
**Task class:** focused model/algorithm source, license, runtime and semantic audit

## Problem

The accepted `melband_roformer_harmony` route proves only:

```text
all vocals -> lead_vocal + vocal_residual
```

The residual reconstructs the input with the lead, but no accepted source evidence or labeled result proves that it is specifically `BackingVocal`, specifically `HarmonyVocal`, or a valid partition into both roles.

## Required outcome

Select and audit an exact source-verified native model or deterministic native algorithm that genuinely distinguishes:

```text
BackingVocal
HarmonyVocal
```

The audit must define how unison doubles, responses/ad-libs, choir layers and pitched harmony are classified. It must not infer semantics from filenames or relabel one residual under two roles.

Before implementation, record:

- exact model/checkpoint or algorithm identity;
- upstream output taxonomy and source revision;
- explicit license identity;
- immutable conversion/runtime recipe where model-derived;
- supported native backend and bounded execution contract;
- representative labeled fixtures containing independently audible backing and harmony material.

## Acceptance

- Each requested role produces a distinct, role-correct artifact or the request fails closed.
- Lead, backing and harmony artifacts preserve source timeline and declared audio facts.
- Provenance identifies producer, model/runtime generation where applicable, and semantic role.
- Atomic publication and active cancellation leave no partial artifacts.
- CPU/fake fixtures cover alias rejection; any accelerator check follows repository permission policy.
- Studio continues to use local wire DTOs through `AnalysisCliClient`; no backend implementation crate is linked into app-core or desktop.

Do not mark `audio.lead_partition implementation_exists=true` without an executable route. Card 17 may close final-v1 by correcting scope to the authoritative future/optional contract, which does not claim this capability.

## Current audit result

No current Catalog resource satisfies the originally proposed BackingVocal-versus-HarmonyVocal audio contract.

The exact accepted Karaoke checkpoint is a one-target model (`num_stems=1`, target `Vocals`). UVR source semantics identify that target as lead removal and call its complement backing in the application workflow, but Uta's framework contract correctly requires residual quality/classification before semantic promotion. Neither the checkpoint nor the current Worker distinguishes Harmony from other support vocals, doubles, ad-libs, choir layers or separation residue. Its checkpoint license also remains unresolved.

A text/metadata-only audit then screened MedleyVox without downloading weights:

- official source revision: `jeonchangbin49/MedleyVox@a185cd5eb4f1306600afba474acf04ea7bd6f3c7`;
- paper taxonomy: unison, duet, main-vs-rest and N-singing separation;
- screened converted/checkpoint host: `Cyru5/MedleyVox@5c9e4e0d909e5a006c992b3422901ed416f4e57f`;
- exact main-vs-rest candidate: `multi_singing_librispeech/vocals.pth`, 232,997,719 bytes, hosted SHA-256 `5c8ff43108ee58ffc9555359fa05727534d4448d95d58b563332026487de43c8`;
- source config: 24 kHz, two outputs, permutation-invariant Conv-TasNet/STFT, trained for generic main-vs-rest/multiple-singer separation.

This candidate is rejected for card 17 as currently published:

1. its outputs are individual/permutation or main-vs-rest sources, not distinct BackingVocal and HarmonyVocal roles;
2. it has no classifier defining unison doubles, ad-libs and choir parts versus pitched harmony;
3. the official source repository publishes no license, while CC-BY-4.0 appears only on the separate Hugging Face model card, so exact weight licensing is not authoritative;
4. no immutable native conversion/runtime recipe, Runtime Manager entry or packaged worker exists;
5. no accepted labeled fixture proves the final product taxonomy.

A source-verified future implementation would still require an authoritative multiple-foreground checkpoint/algorithm, identity constraints and labeled fixtures. However, comparison with the current authoritative design established that this is not a final-v1 blocker: `audio.lead_partition` means partitioning simultaneous foreground singers and is explicitly future/optional. Editor Harmony/Backing remain chart-track roles, not promises of corresponding separated audio stems.

Card 17 therefore corrected the executable contract instead of inventing a model route: the accepted Karaoke recipe remains `audio.lead_isolate` with `LeadVocal + VocalResidual`; Processing Studio does not advertise `audio.lead_partition`; independent Backing/Harmony stem requests fail closed. This audit is `SKIPPED_ALREADY_CLOSED` for final-v1 and remains useful future-route research.

No model bytes, package, worker, GPU or inference context was created during this audit.
