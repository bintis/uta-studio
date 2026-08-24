# 16 — Conditional Expert Scheduler

**Precondition:** card 15 = `READY`
**Task class:** CPU/control-plane scheduler closure; real model inference is forbidden in implementation acceptance
**Owner:** Analysis Engine / uta-analyze

## Read

```text
AGENTS.md
docs/agent-tasks/MODEL_GPU_WORK_POLICY.md
tasks/final-features/PROCESS_BOUNDARY_RULES.md
tasks/final-features/STUDIO_BACKEND_UI_PARITY.md
tasks/final-features/16_CONDITIONAL_EXPERT_SCHEDULER.md
```

Inspect relevant Workflow execution policy, Engine candidate/review, and expert contracts only.

## Goal

Make Workflow execution policies real:

```text
Always
OnDisagreement
MaximumOnly
Disabled
```

The current compiler already represents and validates conditional nodes. This card makes the backend production scheduler execute them truthfully rather than treating conditionals as decorative metadata or Always nodes.

## Required behavior

### Always

Execute when dependencies are satisfied and the node is requested/reachable.

### Disabled

Never execute. Downstream required inputs that have no valid producer must fail during planning/validation rather than at arbitrary runtime.

### MaximumOnly

Execute only for Maximum quality/profile when the node is otherwise reachable and the optional expert is usable. It must not become a Fast/Balanced baseline requirement.

### OnDisagreement

Execute only after baseline evidence has produced a typed disagreement/review region relevant to that expert.

Examples include:

```text
RMVPE vs GAME pitch/note disagreement -> FCPE optional pitch challenger
GAME/other boundary disagreement -> Basic Pitch / STARS / ROSVOT as defined by Workflow
transcript disagreement -> FireRed challenger where the Workflow explicitly includes it
```

Do not hard-code these examples by model name when the compiled Workflow already identifies the capability/node/dependencies. Model-specific adapters may map typed evidence into an expert request, but scheduling policy stays generic.

## Windowed execution

`OnDisagreement` is intended to avoid running optional experts over the entire song unnecessarily.

Required design:

```text
baseline evidence
  -> review/disagreement regions
  -> merge/coalesce bounded regions deterministically
  -> authorize exact source artifact + canonical time range
  -> run optional expert on only those ranges when its contract supports it
  -> map local output back to canonical source timeline
  -> dependency-aware Fusion
```

If an expert cannot safely consume a bounded range, do not silently run the whole song. The node must either have an explicit full-input policy encoded in the backend contract or remain unavailable for `OnDisagreement` scheduling.

No silent clipping/truncation.

## Correlation/dependency rules

Conditional experts must preserve dependency metadata. STARS and ROSVOT are conditioned on RMVPE and/or lyric/alignment evidence and must not be counted as independent confirmations of their own inputs.

Unknown confidence remains unknown. Raw logits/sigmoid values from one expert are not cross-model calibrated probabilities.

## Runtime policy

The scheduler asks the backend Runtime Manager for current usability through existing backend integration. Studio does not decide whether an optional resource is Production-usable.

Missing optional expert behavior must be explicit:

```text
optional conditional node unavailable -> typed degraded/blocked node state according to Workflow intent
baseline required node unavailable -> fail closed
```

Do not globally block a baseline request merely because a non-required conditional expert is absent.

## No model execution in this card

Use fake deterministic expert executors / typed evidence fixtures. Prove scheduling semantics without creating OpenVINO/Vulkan contexts.

Required tests:

```text
Always executes exactly once
Disabled never executes
MaximumOnly runs only in Maximum
OnDisagreement does not run with no disagreement
OnDisagreement runs only for relevant bounded regions
nearby regions coalesce deterministically
disjoint regions remain distinct
canonical time mapping survives window extraction/reinsertion
optional unavailability does not fabricate evidence
required dependency loss fails closed
conditional result dependency/correlation metadata survives Fusion
priority changes order but not dependency graph
cancellation during optional expert scheduling stops later windows and reaps task state
```

## Process boundary

No scheduler logic may be added to Studio/app-core. Studio edits the Workflow execution policy and displays Plan/runtime state; `uta-analyze`/Engine owns the actual conditional decision/execution.

## Durable completion update

Set card 16's current state/result in `tasks/remaining-models/STATE.md`. Update `docs/KEY_CONCLUSIONS.md` only for a durable scheduling/process-boundary conclusion. Do not create a completion log under `docs/`.

Include typed scheduler state, region/window contract, fixture coverage, degraded/fail-closed rules, and process-boundary verification.

Stop after this card.
