# Remaining Models — Repair Wave Policy

**Purpose:** a first-pass `FAILED_SAFE` / technical `BLOCKED` result is not permanent when the defect is concretely repairable.

## 1. Effective result rule

Only the current effective result is retained as durable state. Older execution logs may be deleted once their still-relevant conclusion has been merged into `tasks/remaining-models/STATE.md` or `docs/KEY_CONCLUSIONS.md`.

A later repair may supersede the **effective** readiness of the same resource:

```text
original: FAILED_SAFE / integration_ready=no
repair:   READY / integration_ready=yes
-----------------------------------------
effective integration state = READY
```

The active all-model repair agent computes current effective state from current source/tests and the durable state documents, not from deleted historical logs.

Production and integration remain separate, but the repository license metadata policy is intentionally permissive:

```text
repair technically succeeds + explicit open-source/open-model license
  -> license does not block production_ready

repair technically succeeds + license missing/unknown/ambiguous/non-open
  -> integration_ready=yes
  -> production_ready may still be no
```

For this project, attribution, share-alike, non-commercial, commercial-use, and redistribution conditions on an otherwise explicit open license are retained as license/release metadata, not technical Production blockers. This is a project readiness policy, not a claim that every such license is OSI-approved.

## 2. Which failures get repair cards

Create a repair card when the blocker is a concrete engineering problem with a materially different bounded approach available, for example:

```text
OOM caused by exporter/reference memory strategy
conversion graph decomposition bug
missing worker adapter
incorrect runtime/catalog wiring
manifest/import defect
shape/bucket strategy defect
semantic adapter bug with known source truth
```

Do not describe external-only conditions as code repairs:

```text
checkpoint/source license identity is missing, ambiguous, or non-open only
missing authoritative model identity with no source selected
```

## 3. Repair scheduling

The active agent uses current source/tests, `tasks/remaining-models/STATE.md`, `docs/KEY_CONCLUSIONS.md`, and any retained pending repair card, then implements every unresolved repair in the same continuous task before aggregate evaluation.

Repair cards live under:

```text
tasks/remaining-models/repairs/
```

The active agent may add focused repair instructions when useful, but no separate ownership, delegation, or completion-log file is required.

## 4. Repair acceptance

A repair is `READY` only when it closes the exact technical blocker and satisfies the original task's relevant acceptance gates. A safer build or a generated file alone is not success.

A repair's durable conclusion must update `STATE.md` and, when cross-cutting, `docs/KEY_CONCLUSIONS.md` with the material facts:

```text
root cause
materially different repair strategy
memory/GPU/process bound when relevant
artifact identities/hashes when relevant
semantic validation conclusion
Runtime Manager state
Engine integration state
integration_ready yes/no
production_ready yes/no
license identity/note; licensing remains advisory and non-blocking
```

Do not retain command transcripts or process-by-process completion journals under `docs/`.

## 5. No repeated dangerous strategy

A repair card must not simply rerun the exact workload that already caused OOM/runaway memory or machine instability.

For OOM specifically:

```text
separate heavy phases into different processes
release model/graph/activations between phases
avoid retaining full reference tensors + export graph simultaneously
establish an explicit memory budget/stop threshold before the heavy phase
prefer disk-backed/intermediate artifacts over duplicate in-RAM copies
characterize memory first on a tiny representative shape
never make exact full-shape PyTorch reference/export the first heavy action after an OOM
exit PyTorch before ORT; exit ORT before OpenVINO conversion/validation
```

On this host, an OOM repair must reserve at least 8 GiB for the OS/compositor/other processes and, when an enforceable process/cgroup limit is available, keep any single heavy phase at or below `min(16 GiB, available_RAM - 8 GiB)`. A `MemoryMax`/equivalent ceiling is a safety stop, not permission to keep the old lifecycle: do not raise it toward the historical failure peak, add swap, or extend timeout merely to make the old workload finish.

If attention or another dimension has non-linear growth, a small-shape measurement must not be extrapolated linearly. Increase the representative shape only in bounded steps and require a conservative exact-shape projection before attempting the exact reference. If the new strategy cannot be bounded below the previous failure envelope, finish `FAILED_SAFE` again rather than retrying blindly.

## 6. Card 14 / Phase B evaluation

Card 14 and later integration gates use **effective resource state**, not the first attempt's raw row.

Example:

```text
03 FAILED_SAFE
R03 READY integration_ready=yes

=> Denoise is integration-ready for later feature work.
```

If `production_ready=no` remains for technical/external validation , the Production aggregate bubble may still be skipped/blocked only for technical or external-validation reasons while later feature implementation proceeds under the integration/Production dual-gate policy. Restrictive conditions on an explicit open license alone do not block technical Production.
