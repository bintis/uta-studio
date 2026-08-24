# 17 — Lead / Backing / Harmony Partition Contract

**Precondition:** Phase A model cards 01–13 are terminal and card 05 reports `integration_ready=yes`. Card 14 may be `SKIPPED_PRECONDITION` for unrelated Production-only blockers.
**Task class:** semantic audio capability closure; OpenVINO execution only if a narrowly bounded already-accepted model check is strictly required
**Owner:** Analysis Engine separation semantics + Studio local workflow-role/wire mapping only

## Read

```text
AGENTS.md
docs/agent-tasks/MODEL_GPU_WORK_POLICY.md
tasks/final-features/PROCESS_BOUNDARY_RULES.md
tasks/final-features/STUDIO_BACKEND_UI_PARITY.md
tasks/final-features/17_LEAD_BACKING_HARMONY_PARTITION.md
docs/KEY_CONCLUSIONS.md
tasks/remaining-models/STATE.md
```

Read the current Harmony conclusion first. Do not assume a semantic output that current source/state did not prove.

## Problem

Current Engine explicitly rejects requested `BackingVocal` / `HarmonyVocal` stems with `MissingCapability(audio.lead_partition)`. Final design requires independent vocal lanes while preserving truthful semantics.

Current capability state before this card:

```text
audio.lead_isolate    implemented
audio.lead_partition  not implemented
```

## Hard semantic rule

A model that merely emits “karaoke”, “residual”, “other vocals”, or a second audio file does not automatically prove separate Backing and Harmony semantics.

This card must establish a documented, tested mapping from accepted model/runtime outputs to the exact Engine semantic roles it claims:

```text
LeadVocal
BackingVocal
HarmonyVocal
```

If the accepted model only proves Lead + one undifferentiated residual, do not split that residual into Backing/Harmony by naming convention. Either:

1. implement only the exact truthful subset and keep the unsupported semantic role blocked, then create a targeted follow-up model/algorithm card; or
2. use an already-selected/accepted source-verified model/output that genuinely distinguishes the roles.

Do not invent a new model in this feature card without a new focused task card and source/license/runtime audit.

## Engine outcome

When semantics are proven, implement `audio.lead_partition` as a real Analysis Engine stage and make requested stem behavior truthful:

```text
requested BackingVocal -> actual backing semantic artifact
requested HarmonyVocal -> actual harmony semantic artifact
```

Artifacts must preserve:

```text
source timeline
sample rate/channels appropriate to the accepted separation route
role identity
producer/provenance
model/runtime generation where model-derived
atomic publication
lossless storage policy when source/generated audio is lossless
```

No requested role may be satisfied by relabeling the same bytes under two names unless the product contract explicitly defines them as aliases, which final-v1 does not.

## Workflow local-domain alignment

The current Studio Workflow domain uses local audio roles such as `BackVocal`, while the Analysis Engine wire contract has `BackingVocal` and `HarmonyVocal`.

Reconcile this without sharing backend crates:

```text
Studio Workflow local domain
  -> explicit local wire mapping
  -> AnalyzeRequest/workflow extension JSON role
  -> Engine AudioRole
```

If the local Workflow domain needs distinct `BackingVocal` and `HarmonyVocal`, add them with versioned persistence/migration so existing stored workflows remain readable. Do not import Engine `AudioRole` into app-core.

Editor chart-track `Adlib` is not automatically an audio stem role. Preserve existing Editor Adlib behavior; do not invent an `Adlib` audio model unless the design explicitly requires one.

## Tests

Use deterministic audio fixtures and model-output fixtures where possible. If one bounded OpenVINO semantic check is required, it is one model/workload at a time and Vulkan remains forbidden.

Required tests:

```text
unsupported semantic role fails closed
lead/backing/harmony roles cannot alias accidentally
role mapping round-trips across Studio local DTO -> CLI JSON -> Engine contract
stored Workflow migration preserves old BackVocal data
requested stem result contains exact semantic role
source timeline preserved
cancellation leaves no partial stem
Processing Studio independent lane graph type-checks
Desktop still consumes app-core only
```

## Capability gate

Set:

```text
audio.lead_partition implementation_exists=true
```

only when at least the product-declared partition semantics are truly executable through `uta-analyze` and requested artifacts can be emitted. Do not green the capability for a placeholder adapter.

## Durable completion update

Set card 17's current state/result in `tasks/remaining-models/STATE.md` and update `docs/KEY_CONCLUSIONS.md` if the lead/backing/harmony semantic contract changes. Do not create a completion log under `docs/`.

If semantic evidence is insufficient, finish `NEEDS_REVIEW` with the exact missing role/model/algorithm requirement; do not fake full design completion.

Stop after this card.
