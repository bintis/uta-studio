# 20A — Studio ↔ Backend UI Parity Closure

**Precondition:** cards 15–19 = `READY`
**Task class:** CPU/UI/control-plane parity closure; no model inference
**Owners:** Studio/app-core presentation + local wire/domain mapping only; backend changes only for narrowly identified contract exposure defects

## Read

```text
AGENTS.md
docs/agent-tasks/MODEL_GPU_WORK_POLICY.md
tasks/final-features/PROCESS_BOUNDARY_RULES.md
tasks/final-features/STUDIO_BACKEND_UI_PARITY.md
tasks/final-features/20A_STUDIO_BACKEND_UI_PARITY_CLOSURE.md
```

Read completion records for cards 15–19. Inspect current Studio/backend source rather than historical UI screenshots.

## Mission

Close every known mismatch where:

```text
UI says a control/state exists
but backend does not consume it

backend capability exists
but Studio cannot express/reach it

UI readiness/status is coarser or different from backend truth

Processing Studio domain cannot represent a backend semantic role/policy
```

This card is not a visual redesign. Preserve the current Studio structure and visual language unless a small UI change is required to make behavior truthful/reachable.

## 1. Settings > Analysis exact strategy readiness

Known defect before this card:

```text
Vocal extraction strategy -> ModelDownloadTarget::RoFormer bundle
Instrumental strategy     -> same RoFormer bundle
```

The bundle covers multiple RoFormer resources. Specific provider rows must not infer exact readiness from family/bundle health.

Fix through app-core local Runtime CLI APIs/DTOs so UI can render the exact current resource/capability state, e.g. the semantic equivalent of:

```text
Vocal extraction      -> model:bs_roformer_vocals_ep317 / audio.extract_vocals
Instrumental          -> model:melband_roformer_inst_v2 / audio.extract_instrumental
Lead isolation        -> model:melband_roformer_harmony / audio.lead_isolate
Pitch                 -> model:rmvpe / pitch.track
GAME                  -> model:game / notes.game
```

Desktop must not call `uta-runtime` directly. Do not duplicate Runtime Manager policy in Studio; consume returned facts.

Aggregate family/component health may remain on Models & runtime if clearly labeled as aggregate.

## 2. Analysis Advanced controls must not be enabled no-ops

Audit these current interactive controls:

```text
Segment size
Overlap
Batch size
Output normalization
Voiced sensitivity
```

For the final Engine/compiled-Workflow path, each must be:

```text
A. mapped to a versioned Workflow/node parameter and consumed by the owning Engine/native stage;
or
B. clearly legacy-only and disabled/hidden when the canonical Engine path is used;
or
C. retired.
```

Do not keep a clickable control whose value is ignored by the queued AnalyzeRequest/Workflow snapshot.

If mapped into Workflow parameters, Preview and queued execution must show/use the exact same resolved parameter values.

## 3. Processing Studio execution-policy UI completeness

Known defect before this card:

The domain can represent:

```text
Always
OnDisagreement
DisagreementWindows
MaximumOnly
Disabled
```

but the current Policy button cycle cannot select `MaximumOnly`.

Replace the lossy cycle behavior with an explicit selector/menu or another interaction that exposes the complete supported policy set without ambiguity.

After card 16, each visible policy must correspond to real backend scheduler behavior. Do not expose a condition that the Engine treats as decorative metadata.

UI copy should distinguish:

```text
execution condition
priority
hard dependency
```

Priority must never be described as dependency.

## 4. Workflow audio-role parity

After card 17, verify Processing Studio can represent and route the exact accepted role set.

Known pre-card mismatch:

```text
Engine: LeadVocal / BackingVocal / HarmonyVocal
Workflow: LeadVocal / BackVocal only
Editor: Lead / Harmony / Backing / Adlib
```

Required:

- distinct accepted Backing and Harmony Workflow role/port types if backend semantics exist;
- versioned migration for stored Workflow data;
- node cards / analyzer source labels / Advanced Graph render the distinct roles;
- app-core maps local roles to local wire DTOs without importing Engine types;
- Editor Adlib remains a chart role unless a real audio-stem contract exists.

Do not collapse two accepted backend roles into one Studio `BackVocal` label.

## 5. Quantization UI parity

After card 19:

If Engine quantization is implemented:

```text
Settings > Analysis toggle -> exact request field -> real Engine stage -> typed Candidate result
```

and UI copy must make clear that continuous F0 is untouched.

If Engine quantization was retired in favor of Editor-only authoring quantize, remove/disable the Engine-facing toggle and any Plan Preview claim consistently.

No enabled inert toggle.

## 6. Optional-expert status/copy parity

Audit hard-coded descriptions such as:

```text
FCPE · diagnostic only
BenchmarkCandidate until promoted
FireRed optional challenger
STARS experimental
Basic Pitch optional
```

After cards 08–13, UI labels/badges must reflect current backend validation state without hard-coded stale claims.

Rules:

- role semantics (baseline vs optional/challenger) may remain product policy copy;
- validation/readiness must come from current Runtime facts;
- a promoted `ProductionPinned` expert must not still display “BenchmarkCandidate” because of stale static copy;
- a still-blocked expert must not get an enabled Production-looking control;
- optional expert availability must not block unrelated exact requests.

## 7. Technique UI parity

After card 18, ensure technique evidence has a reachable Studio/Editor presentation:

```text
read-only evidence layer / review context
source/provenance visible enough for inspection
not represented as extra MIDI notes
raw logits not labeled calibrated confidence
```

Do not require a new top-level screen if the existing Evidence/Review surfaces can represent it truthfully.

## 8. Plan Preview / Workflow Run parity

Verify:

- Plan Preview remains the request-specific readiness authority;
- Processing Studio `Run` cannot bypass exact Engine preview/readiness;
- Workflow run preview includes the exact workflow id/revision/digest or equivalent execution identity;
- blockers/resources/output artifacts shown correspond to the exact queued request;
- Analyze/Run cannot be enabled when backend preview says blocked;
- Models & runtime navigation does not install anything automatically.

The existing Plan Preview request-specific behavior is largely correct; preserve it.

## 9. Models & runtime parity

Verify the page remains lifecycle-only:

```text
install/import/verify/status/repair/remove
```

No Analysis intent/profile/provider business logic may migrate into Models & runtime.

Keep truthful distinctions:

```text
ProductionPinned
BenchmarkCandidate
Experimental
Unsupported
```

Aggregate component health must be labeled aggregate and must not override request-specific readiness.

## 10. Candidate / Review / Editor reachable UI

Verify the UI reaches the features already present in domain/backend:

```text
Candidate open
Candidate/Authored compare/merge
Review Queue navigation
Suggestion acceptance + undo
Evidence read-only
technique evidence
Lead/Harmony/Backing/Adlib chart tracks
artifact source/waveform selection
A/B audition
```

Do not assume symbol presence is enough. Focused UI/action tests should prove the controls are actually reachable and wired to the intended app-core actions.

## 11. Decoupling

Hard zero gates:

```text
app-core/Cargo.toml -> uta-analysis-engine
app-core/Cargo.toml -> uta-runtime-manager
desktop/Cargo.toml -> either backend implementation crate
app-core/** -> uta_analysis_engine:: / uta_runtime_manager::
desktop/** -> uta_analysis_engine:: / uta_runtime_manager::
desktop/** direct uta-analyze or uta-runtime process launch
```

All new status/query behavior goes through app-core local clients/DTOs.

## Tests

CPU/UI/local only. Include focused tests for at least:

```text
specific RoFormer strategy row is not gated by unrelated RoFormer bundle member
all supported execution policies are selectable, including MaximumOnly
Advanced controls either affect compiled/resolved parameters or are disabled/retired
Backing and Harmony roles remain distinct through Workflow UI/domain/wire mapping
quantization toggle matches chosen card-19 contract
optional-expert copy/badge follows current validation fact
Plan Preview/Run remains disabled on exact backend blocker
technique evidence is reachable but read-only
Desktop uses app-core only
```

Do not run model inference or final package acceptance.

## Durable completion update

Set card 20A's current state/result in `tasks/remaining-models/STATE.md`. If parity work changes a durable Studio/backend/UI contract, update `docs/KEY_CONCLUSIONS.md`. Do not create a completion log under `docs/`.

Include a matrix:

```text
UI surface | app-core intent/fact | wire/backend owner | prior mismatch | final behavior | test
```

Stop after this card.
