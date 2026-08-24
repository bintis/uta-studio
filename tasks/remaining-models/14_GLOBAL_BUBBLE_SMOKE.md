# Subtask 14 — Global Model/Runtime Bubble Smoke

**State:** READY
**Purpose:** after every model/resource card has passed, run a small cross-model smoke suite that exercises the assembled Runtime Manager + native workers + Analysis Engine contracts without turning final validation into another full stress campaign.
**Accelerator policy:** non-Qwen Vulkan/Level Zero calls require explicit user permission; Qwen is exempt; other accelerator calls are unrestricted.

## 1. Start gate — all prior model cards must have passed

Do not run this task merely because card 13 finished.

The active agent runs this aggregate check after the unresolved repair set has current effective results. A fully green Production bubble expects cards 01–13 to be one of:

```text
READY
SKIPPED_ALREADY_CLOSED
```

and there are **no** prior states:

```text
BLOCKED
FAILED_SAFE
NEEDS_REVIEW
RUNNING
PENDING
```

If that precondition is not satisfied, mark card 14 `SKIPPED_PRECONDITION` with the exact blocking card IDs.

## 2. Scope

This is an aggregate smoke, not a new quality campaign.

Do not:

```text
reconvert models
redownload models
change model validation state to make a smoke pass
run long/full-song stress merely for confidence
run a matrix in parallel
run non-Qwen Vulkan/Level Zero without explicit user permission
run whole-workspace/Nix release acceptance here
```

Use already-approved exact artifacts/generations and project-owned/local test media. Keep individual audio workloads short and bounded, normally about 6–12 seconds unless an existing exact model window requires another already-approved bounded size.

If any smoke exposes a real defect, fail the smoke and report the owning model/runtime/Engine layer. Do not patch unrelated components inside this smoke card.

## 3. Bubble A — Runtime Manager whole-catalog resolution

Goal: prove the final catalog/install/runtime graph is coherent after all individual model work.

Using `uta-runtime` machine-readable output, inspect every final model resource one at a time through the applicable non-mutating commands such as:

```text
list
show
status
paths
verify
resolve
```

Requirements:

```text
JSON/NDJSON stdout remains protocol-clean
no lifecycle truth is inferred from stderr
resolved generation/content digests are non-empty
runtime generation/recipe identities are present where required
source-vs-converted identity remains distinct
Production policy agrees with each final completion record
no model silently resolves to another model/resource
Qwen resolves to its dedicated pinned C++/GGML runtime, not OpenVINO
non-Qwen models resolve to their selected source-verified native path; all five RoFormer resources resolve only to GGML/Vulkan and cannot resolve OpenVINO
```

This bubble must include at least:

```text
bs_roformer_vocals_ep317
melband_roformer_harmony
melband_roformer_inst_v2
melband_roformer_denoise_aufr33
melband_roformer_dereverb_anvuew
qwen3_asr_1_7b
qwen3_forced_aligner_0_6b
rmvpe
game
firered_asr2_aed
fcpe
basic_pitch
stars
rosvot
```

No inference is required for Bubble A.

## 4. Bubble B — RoFormer stereo preparation/separation chain

Goal: catch integration regressions that individual model conversion tests cannot see.

Use one short project-owned/local real stereo mix. Every RoFormer stage must use its exact GGML/Vulkan route with batch size 1, no async submission and a serial pipeline; no RoFormer may launch OpenVINO:

```text
original mix
  -> BS-RoFormer Vocals
  -> MelBand Harmony / lead-isolation path
  -> Denoise on the appropriate vocal/lead output
  -> Dereverb on the appropriate clean-vocal path
```

Separately on the same original mix:

```text
original mix
  -> MelBand Inst V2
  -> Instrumental stem
```

Validate after every stage:

```text
44.1 kHz stereo semantics preserved where the product contract requires it
output duration matches source within the existing Engine tolerance
output decodes completely
all samples/metadata required by the contract are finite/valid
output is not accidentally empty/silent unless the fixture semantics allow it
semantic role matches the selected model contract
no overwrite of an existing artifact
temporary worker files are cleaned
worker exits before the next model begins
```

For Harmony, re-check the final accepted semantic contract. A technically valid karaoke/residual output must not be mislabeled as `lead_vocal` unless card 05 established that mapping.

## 5. Bubble C — Singing evidence → dependency-aware Fusion → Candidate chart

Goal: exercise the assembled singing-analysis stack across several accepted experts.

Use one short project-owned/local vocal fixture with canonical lyric/phoneme/alignment fixture data already accepted by the repository:

```text
Acoustic DSP                         CPU
RMVPE                                OpenVINO
GAME                                 OpenVINO
STARS                                OpenVINO, if card 12 READY
ROSVOT                               OpenVINO, if card 13 READY
```

Then run the CPU-only Analysis Engine evidence adapters/Fusion/Candidate stages on the resulting typed evidence.

Required assertions:

```text
canonical transcript/alignment fixture is not silently rewritten
RMVPE remains continuous F0 evidence, not target MIDI notes
GAME notes remain explicit GAME evidence
STARS records RMVPE + lyric/phoneme/alignment dependencies
ROSVOT records RMVPE + alignment/RWBD dependencies as applicable
Fusion correlation/dependency discount prevents double-counting conditioned experts
missing/unknown confidence remains unknown rather than fabricated zero or 0.8/0.9
raw STARS/ROSVOT logits are not treated as calibrated cross-model probability
candidate graph emits finite ordered non-overlapping note states
SingingAnalysis validates
Candidate VocalChart validates
Candidate authority remains Candidate and never overwrites Authored
continuous pitch remains separately represented from target notes
artifact hashes/media types/provenance validate
```

This is the main cross-model bubble for the singing stack.

## 6. Bubble D — Optional experts

Goal: ensure the optional OpenVINO paths still work after the later model/runtime changes.

Run sequential short bounded fixtures for:

```text
FireRed ASR2-AED
FCPE
Basic Pitch
```

Use the exact final accepted fixed-window/bucket contract for each model. Do not invent a longer shape just to make the test more realistic.

Validate:

```text
FireRed produces non-empty typed transcript evidence on its accepted golden fixture
FCPE produces finite/nullable secondary F0 according to its final unvoiced/NaN contract
Basic Pitch produces finite onset/note/contour activation evidence
none becomes a required baseline dependency merely because the smoke passes
FireRed does not override canonical caller lyrics automatically
FCPE does not substitute for RMVPE
Basic Pitch does not substitute for GAME
all workers exit cleanly
```

## 7. Bubble E — lifecycle/cancellation/recovery

Goal: ensure the assembled native runtime remains controllable after several model families have been installed and validated.

Run two bounded cancellation probes, sequentially:

```text
one GGML/Vulkan RoFormer separation task, with explicit user permission
one OpenVINO evidence/expert task
```

Choose already-validated models that start quickly enough to observe an active child without requiring a long/full-track workload.

For each probe:

```text
start one task
observe the child/model worker exists
request cancellation through the normal control boundary
receive typed cancellation/failure state
reap the process group
verify no child/worker remains
verify no partial committed output was published
```

Then run one tiny already-approved smoke on the same selected backend to prove each runtime remains usable after cancellation. The RoFormer recovery probe must remain batch-size-one/no-async/serial, must not use OpenVINO, and requires explicit user permission.

## 8. Qwen / full-candidate live smoke policy

Qwen is exempt from the Vulkan/Level Zero permission rule. The bubble may use live Qwen execution or the following lower-cost checks according to what best proves the current repair:

```text
Runtime Manager identity/status/verify/resolve
worker/runtime packaging/static contract checks
replay of exact previously validated typed Qwen transcript/alignment evidence into CPU Fusion integration where useful
```

The live extension is:

```text
Qwen ASR short live fixture
  -> worker exits
Qwen Forced Aligner short live fixture
  -> worker exits
one short black-box Full Candidate uta-analyze run
```

## 9. Protocol and result-integrity assertions across every bubble

For every native process exercised:

```text
stdout machine protocol only
stderr diagnostics only
no parser reads stderr as success/lifecycle truth
no model task leaks a worker after completion
no fallback to CPU product inference occurs silently
no source/user media is modified
no existing managed generation is overwritten
```

## 10. Completion criteria

`READY` requires all mandatory bubbles A–E to pass under the current safety policy.

A model-specific failure means this card is `FAILED_SAFE` and must update the owning resource's current state explicitly; do not hide the regression behind an older READY conclusion.

On completion, update:

```text
tasks/remaining-models/STATE.md
docs/KEY_CONCLUSIONS.md
```

Keep only the durable aggregate conclusion: READY/FAILED_SAFE, which bubbles passed/failed, models/backends materially exercised, any permission-sensitive non-Qwen Vulkan/Level Zero execution, and the exact current regression/blocker. Do not commit fixture paths, command transcripts, process narration or a verbose smoke report under `docs/`.

Do not proceed into the repository-wide release pass reserved by `AGENTS.md` from this card; the active task records the handoff after accepting the result.
