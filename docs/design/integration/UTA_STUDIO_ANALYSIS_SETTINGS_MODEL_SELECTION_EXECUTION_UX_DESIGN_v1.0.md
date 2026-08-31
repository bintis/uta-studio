# Uta! Studio — Analysis Settings, Model Selection & Execution Plan UX Design v1.0

**Status:** implementation design
**Date:** 2026-08-22
**Scope:** `Settings > Analysis`, song-level analysis profile, per-run overrides, Processing Studio, Plan Preview, and `Settings > Models & runtime`
**Companion architecture:** `docs/design/integration/UTA_STUDIO_ANALYSIS_RUNTIME_INTEGRATION_DESIGN_v1.0.md`
**Current closure index:** `tasks/remaining-models/STATE.md`
**Studio/backend process contract:** `docs/design/architecture/UTA_STUDIO_CLI_PROCESS_BOUNDARY_v1.0.md`
**Visual index:** `docs/design/ui/analysis-settings/README.md`

**Architecture authority:** `docs/design/architecture/UTA_SEPARATED_ARCHITECTURE_DESIGN_v1.0.md` and `docs/design/audio-analysis/UTA_ANALYSIS_ENGINE_AUDIO_ANALYSIS_FRAMEWORK_v2.1_RC.md` are authoritative for component boundaries and analysis semantics. This document owns only the Studio-facing settings/model-selection/execution UX contract.

---

# 1. Purpose

This document closes the product/UX gaps left intentionally open by the Studio reintegration design.

It freezes how a user chooses analysis behavior, how model preferences are represented, what may be reordered, how execution order is presented, where advanced audio-analysis controls live, how global/song/run overrides interact, and how Runtime Manager policy vetoes an unavailable or unvalidated model without silently changing user intent.

The goal is not to expose implementation details. The goal is to make the application predictable while preserving the architecture:

```text
User intent / preference
        ↓
Studio analysis settings + workflow
        ↓
app-core product APIs / serialized request
        ↓
uta-analyze machine protocol
        ↓
Engine planner + backend Runtime Manager resolution
        ↓
Resolved execution plan

Resource lifecycle UI
        ↓
app-core RuntimeCliClient
        ↓
uta-runtime machine protocol
```

The central UX rule is:

> **Users choose analysis behavior and, where useful, a stable expert preference. They do not choose checkpoint filenames, paths, worker binaries, tensors, or unvalidated runtime routes.**

---

# 2. Visual implementation references

The following implementation-oriented diagrams accompany this specification:

```text
docs/design/ui/analysis-settings/01-analysis-settings-page.svg
docs/design/ui/analysis-settings/02-model-preference-resolution.svg
docs/design/ui/analysis-settings/03-execution-plan-preview.svg
docs/design/ui/analysis-settings/04-page-responsibility-map.svg
docs/design/ui/analysis-settings/05-run-analysis-dialog.svg
docs/design/ui/analysis-settings/06-profile-inheritance.svg
```

They are wireframes, not pixel-perfect screenshots. The current Roon-inspired visual language, typography, spacing system, controls, accessibility behavior, and `AGENTS.md` interaction rules remain authoritative.

---

# 3. Product mental model

There are four distinct concepts and they must not be merged into one settings surface.

## 3.1 Analysis defaults

`Settings > Analysis` answers:

```text
How should new analysis runs normally behave?
```

Examples:

- Fast / Balanced / Maximum quality profile;
- default vocal and accompaniment preparation strategy;
- optional cleanup policy;
- transcription strategy;
- alignment strategy;
- continuous-pitch strategy;
- optional challenger behavior;
- model-owned quality/memory parameters;
- auto-analyze behavior.

It does **not** install models.

## 3.2 Processing Studio

Processing Studio answers:

```text
What product-level processing topology should this workflow use?
```

It owns:

- audio preparation topology;
- ordering of reorderable audio transformations;
- optional branches;
- conditional execution policies;
- semantic artifact routing.

It must not become a raw model graph editor.

## 3.3 Models & runtime

`Settings > Models & runtime` answers:

```text
What resources exist locally and are they actually usable?
```

It owns installation, import, verification, repair, removal, provenance, license/source display, backend validation and production-usability state.

It does **not** choose the user's analysis strategy.

## 3.4 Run Analysis / Plan Preview

The per-run preview answers:

```text
What exactly will happen for this song if I run now?
```

It shows the exact compiled request, effective overrides, resolved execution nodes, required resources, actual model/backend resolution, blocked reasons and requested outputs before the request is queued.

---

# 4. Model choice is a preference, not a file path

Model selection must use stable product/resource identities.

Allowed user-facing identity examples:

```text
Automatic
BS-RoFormer Vocals EP317
MelBand-RoFormer Inst V2
Qwen3-ASR-1.7B
Qwen3 Forced Aligner 0.6B
RMVPE
FCPE
GAME
Basic Pitch
```

Forbidden stable user settings include:

```text
/path/to/model.onnx
model.xml
model.bin
checkpoint.ckpt
worker executable path
OpenVINO compiled blob path
GGUF filename as workflow identity
```

A resource ID such as `model:rmvpe` may be stored internally because Runtime Manager owns that stable identity, but user-facing persisted workflow semantics should prefer capability/strategy meaning where possible.

---

# 5. Selection modes

This section defines the **target UX vocabulary**. The current contract has two distinct lanes:

```text
standalone AnalyzeRequestV1
    Automatic and valid optional Off semantics only

versioned Processing Studio WorkflowExecutionV1 extension
    stable provider intent is allowed per concrete capability card
    when Engine validation recognizes that provider/capability pairing
```

Processing Studio provider intent is a stable model/resource identity, never a file, recipe, executable or backend path. It participates in workflow validation, exact Preview, execution fingerprint and provenance. A visible explicit choice must block rather than silently substitute when unavailable. General Settings/Song/Run capability preferences remain gated until a separately versioned request contract exists.

Every capability that may have more than one eligible implementation ultimately uses one of three selection modes.

## 5.1 Automatic — recommended

`Automatic` means:

```text
Use the Engine's preferred production-validated implementation
for the selected quality profile and capability.
```

Automatic may choose among production-usable providers as Engine policy evolves.

It may not:

- silently use BenchmarkCandidate in a Production run;
- silently change the requested capability;
- install anything;
- use a script fallback;
- cross from Production to Benchmark policy.

## 5.2 Explicit provider preference

An explicit provider means:

```text
For this capability, prefer this stable provider/resource.
```

Examples:

```text
Pitch primary = RMVPE
Transcript primary = Qwen3-ASR-1.7B
Vocal extraction = BS-RoFormer Vocals EP317
```

An explicit choice is **sticky intent**. If the resource is not production-usable, the run is blocked with a precise reason.

Do not silently fall back to another model after the user made an explicit choice.

## 5.3 Off / Disabled

`Off` is available only for optional capabilities or optional transformations.

Examples:

```text
Vocal denoise = Off
Secondary pitch expert = Off
Basic Pitch challenger = Off
```

Required capabilities do not expose Off unless the requested artifact set can still be valid without that capability.

For example:

```text
Candidate VocalChart + notes.game required
=> GAME cannot be switched Off for that request

PitchEvidence only
=> GAME is not in the request at all
```

---

# 6. Do not show fake selectors

If a capability has exactly one eligible production provider, do not present a dropdown that pretends there is meaningful choice.

Preferred UI:

```text
Primary pitch
Automatic · RMVPE
```

with a compact status/detail action.

Only render a selector when at least one of these is true:

1. `Automatic` plus one or more explicit providers are meaningful choices;
2. multiple providers are production-eligible;
3. the capability is optional and `Off` is meaningful;
4. a developer/benchmark surface explicitly exposes non-production candidates.

This avoids a settings page full of one-item dropdowns.

---

# 7. Runtime Manager always has policy veto

The resolution hierarchy is:

```text
Run override
    ↓
Song profile override
    ↓
Global Analysis default
    ↓
Engine quality/profile policy
    ↓
Engine capability requirements
    ↓
Runtime Manager Production policy
    ↓
actual resolved resource/backend
```

The final line is a **veto**, not another preference tier.

All packaged models in the current release have a `ProductionPinned` effective non-CPU route, so no current analysis action is blocked solely by a model validation label. The following rule remains a regression requirement for future catalog/provider changes.

If a selected provider is installed but not production-usable:

```text
Preference: GAME
Installed: yes
Validation: BenchmarkCandidate
Production usable: no

Result: BLOCKED
```

The UI must not reinterpret this as:

```text
Installed => usable
```

or select another provider without user action.

---

# 8. Quality profile

`Fast`, `Balanced`, and `Maximum` are the top-level analysis quality profiles.

Place the control near the top of `Settings > Analysis`.

Recommended presentation:

```text
Analysis quality       [ Fast | Balanced | Maximum ]
```

Default:

```text
Balanced
```

## 8.1 Fast

Intent:

- minimum validated baseline work needed for requested artifacts;
- no optional disagreement experts unless required by a future contract;
- conservative memory/latency defaults;
- no optional cleanup unless explicitly selected.

## 8.2 Balanced

Intent:

- baseline providers plus useful conditional experts;
- optional experts run only where Engine policy says they add value;
- recommended normal authoring mode.

## 8.3 Maximum

Intent:

- enable eligible quality-improving cleanup and challenger capabilities;
- permit higher-cost analysis;
- still only use resources allowed by Production policy;
- no license/network/install side effects.

## 8.4 Profile does not override explicit Off/preference blindly

Example:

```text
Quality = Maximum
Vocal dereverb = Off
```

The explicit user Off wins for that optional transformation.

Example:

```text
Quality = Balanced
Pitch primary = RMVPE
```

Balanced may still add an optional secondary pitch expert, but it must not replace RMVPE as the explicitly selected primary.

---

# 9. Global, song and run inheritance

Preserve and generalize the existing three-tier concept:

```text
Global Defaults
      ↓
Song Profile
      ↓
Run Override
```

Resolution priority:

```text
Run Override > Song Profile > Global Default
```

This inheritance applies to product settings and preferences, not Runtime Manager validation state.

## 9.1 Global Defaults

Stored in application configuration.

Used for new songs and songs without overrides.

## 9.2 Song Profile

Stored per `file_hash` in Studio DB.

Song Detail may expose the same Analysis controls with explicit copy:

```text
Overrides global Analysis defaults for this song.
Existing analysis data changes only after re-analysis.
```

Changing a song profile does not automatically rerun analysis.

## 9.3 Run Override

Lives only in the exact run preview/request.

The control should visually show that it is temporary:

```text
This run only
```

It does not mutate the song profile or global defaults unless the user explicitly chooses a separate “Save as song profile” action.

## 9.4 Effective source indicator

Where useful, show a quiet metadata label:

```text
Source: Global default
Source: Song override
Source: This run
```

The existing inspector concept should remain consistent with this.

---

# 10. Analysis settings page information architecture

The final `Settings > Analysis` page uses this order.

```text
ANALYSIS
Configure defaults for future runs.
Existing chart data changes only after explicit re-analysis.

01 QUALITY & OUTPUT BEHAVIOR
02 AUDIO PREPARATION
03 LYRICS & ALIGNMENT
04 PITCH, NOTES & FUSION
05 ADVANCED PERFORMANCE / MODEL-OWNED PARAMETERS
06 AUTOMATION
```

Do not organize the page around installation packages.

---

# 11. Section 01 — Quality & output behavior

Controls:

### Analysis quality

```text
Fast | Balanced | Maximum
```

### Preserve continuous pitch

Default: On.

Copy:

```text
Keep continuous F0 as independent evidence instead of reducing it to target notes.
```

### Quantize candidate notes

Default: On for normal candidate-chart analysis.

This controls Engine `enable_quantization` where applicable.

### Default analysis target

Do not make a global “always generate every artifact” switch.

The artifact request is primarily determined by the action the user invokes:

```text
Analyze song
Generate candidate chart
Refresh transcript
Refresh pitch evidence
Generate instrumental
```

Global settings shape **how**, not silently broaden **what**.

---

# 12. Section 02 — Audio preparation

The page configures defaults. Processing Studio owns topology for custom workflows.

Recommended rows:

```text
Vocal extraction         Automatic / BS-RoFormer Vocals EP317
Lead isolation           Automatic / MelBand-RoFormer Lead / Back
Instrumental extraction  Automatic / MelBand-RoFormer Inst V2
Vocal cleanup            Auto by quality / Off / Custom
Instrumental cleanup     Auto by quality / Off / Custom
```

Because current Runtime Manager validation may make some resources non-production-usable, every row must render an independent status.

Example:

```text
Vocal extraction
Automatic · BS-RoFormer Vocals EP317
Not production-usable · View in Models & runtime
```

The setting remains user intent; status is runtime truth.

## 12.1 Cleanup custom mode

When Custom is selected, show ordered slots or redirect to Processing Studio depending on complexity.

For the existing two-slot implementation:

```text
Vocal processing 1    Off / Denoise / Dereverb
Vocal processing 2    Off / Denoise / Dereverb

BGM processing 1      Off / Denoise / Dereverb
BGM processing 2      Off / Denoise / Dereverb
```

Duplicate use of the same semantic capability should only be allowed if the workflow representation and Engine path can truthfully execute duplicate instances. Until then keep current deduplication behavior.

## 12.2 Audio branches stay independent

The UI must visually reinforce:

```text
Vocal branch != Instrumental branch
```

Selecting a vocal model must not imply the instrumental model is required.

---

# 13. Section 03 — Lyrics & alignment

Recommended controls:

```text
Transcription strategy
Alignment strategy
Language override
Advanced transcription
```

## 13.1 Transcription strategy

Initial options:

```text
Automatic (recommended)
Qwen3-ASR-1.7B
```

Do not expose FireRed as a required baseline choice while it remains only an optional challenger under the current Engine architecture.

If a future Engine-supported transcript-fusion strategy is production-ready, it can appear as:

```text
Fusion · Qwen + FireRed
```

but the stable setting is the strategy, not “run this checkpoint file”.

## 13.2 Alignment strategy

Initial production intent:

```text
Automatic · Qwen3 Forced Aligner
```

When canonical lyrics are supplied, Engine planning should not require ASR merely because the global transcription preference exists.

## 13.3 Language override

Keep language override a Studio/user input hint.

It must be compiled into the request only where the Engine contract supports it.

---

# 14. Section 04 — Pitch, notes & fusion

Keep these concepts separate in UI and data.

```text
Continuous pitch primary
Note/boundary primary
Optional pitch challenger
Optional note challenger
Fusion behavior
```

## 14.1 Continuous pitch primary

Initial:

```text
Automatic · RMVPE
```

Possible future explicit choices may include a validated alternative.

## 14.2 Note/boundary primary

Initial semantic role:

```text
GAME
```

Current release presentation:

```text
Note & boundary expert
Automatic · GAME
ProductionPinned · Ready when installed and its runtime is available
```

A future unavailable required note provider blocks only requests that require note/boundary evidence.

## 14.3 Optional challengers

Balanced/Maximum may request optional challenger capabilities.

Expose a compact strategy control rather than a long list of checkboxes by default:

```text
Additional experts
Automatic by quality
Off
Custom…
```

Custom may expose:

```text
Secondary pitch      FCPE
Secondary notes      Basic Pitch
Transcript challenger FireRed
```

Every candidate must show its validation class. Production runs cannot execute a candidate unless and until Runtime Manager policy allows it.

If a custom choice is not Production-usable, the preview should state that it will not execute or, if the user explicitly made it required, block the run. Do not quietly pretend it ran.

## 14.4 Stage-4 decision mode

Stage 4 has a separate decision-mode choice from expert/provider selection:

```text
Decision mode
Algorithm      default deterministic HSMM/Viterbi
AI judgment    explicit external AI-assisted selection
```

AI judgment is permitted in normal Production analysis only as an explicit user choice. It does not promote the AI provider into a `ProductionPinned` model. Models & runtime owns readiness/configuration for `tool:fusion_agent_adapter`; Analysis settings/Processing Studio must not store or transmit a raw executable path in the analysis request.

When AI judgment is selected, show that compact fusion candidate metadata and canonical lyrics may be sent to the configured external provider. Plan Preview shows the selected mode and adapter resource/readiness but remains read-only and must not contact the provider. Any adapter/provider/protocol/timeout/validation failure blocks the run; there is no silent fallback to Algorithm. See `UTA_AI_JUDGMENT_FUSION_MODE_v1.0.md`.

---

# 15. Section 05 — Advanced parameters

Advanced controls must be scoped to the capability/provider that owns them.

Follow the repository rule:

> Show model-specific controls only while their owning engine is selected.

Do not show every parameter all the time.

## 15.1 Separation advanced controls

Current controls remain valid Studio-side analysis behavior:

```text
Segment size       64–1024
Overlap            2–32
Batch size         1–8
Output normalization 1–100%
```

They must only be enabled when the selected separation provider actually consumes them.

If Engine v1 does not yet expose these controls in the canonical contract, retain them for the compatible Studio/Processing workflow only and do not imply they affect the new Engine route until an explicit typed contract exists.

## 15.2 Transcription advanced controls

Current:

```text
Search breadth
Transcript batch size
```

Only show them for a strategy/provider whose Engine adapter supports those parameters.

If Qwen's public Engine request does not accept beam width, hide/disable that control for the Engine route rather than silently ignoring it.

## 15.3 Pitch advanced controls

Current:

```text
Voiced sensitivity 0–60%
```

Keep continuous-F0 sensitivity separate from note-boundary/fusion thresholds.

Do not label one slider as generic “accuracy”.

## 15.4 Numeric control layout

Use the existing settings rule:

```text
description on left
minus / editable value / plus on right
```

Clamp invalid input.

Narrow layouts may wrap the entire control below the description.

---

# 16. Section 06 — Automation

Keep:

```text
Auto-analyze
Restore defaults
```

Auto-analyze means:

```text
After a library scan, queue eligible analysis according to configured product behavior.
```

It does not mean:

- install required models;
- accept licenses;
- change resource policy;
- use benchmark candidates;
- reconfigure the user's workflow.

If resources are missing, auto-analysis remains blocked and the UI reports why.

---

# 17. Processing Studio execution-order rules

Processing Studio may expose ordering only where order is product-semantic and safe to change.

## 17.1 Reorderable

Examples:

```text
Vocal denoise / dereverb order
Instrumental denoise / dereverb order
other future role-preserving audio transformations
```

## 17.2 Not arbitrarily reorderable

Do not allow a user to drag these into invalid dependency order:

```text
Decode after inference
Alignment before required lyric/audio input
Fusion before evidence
Finalization before candidate graph
Instrumental extraction as a dependency of lead-vocal analysis when not required
```

Engine dependency order is authoritative.

## 17.3 Capability nodes, not model-file nodes

Processing Studio cards should emphasize:

```text
Vocal Extraction
Denoise
Dereverb
Transcription
Alignment
Pitch Tracking
Note/Boundary Evidence
Fusion
```

The resolved provider may be shown as secondary metadata:

```text
Vocal Extraction
Automatic → BS-RoFormer Vocals EP317
```

Do not make the model filename the primary node identity.

## 17.4 Conditional execution

Existing policies such as:

```text
Always
On disagreement
Disagreement windows
Maximum only
Disabled
```

may remain when the Engine contract can represent the resulting semantics.

A persisted condition that Engine v1 cannot execute must be reported unsupported rather than silently ignored.

## 17.5 Final card interaction contract

Every visible card is a persisted capability instance. The card order must be the compiled semantic execution order, not an unrelated layout list.

- A role-preserving transformation card exposes a pointer drag handle plus keyboard-accessible Earlier/Later actions.
- Drag uses pointer capture and global release/focus-loss/Escape cleanup. A drop may move the card only inside the same semantic audio branch and only when the resulting workflow compiles; invalid/cross-branch drops are rejected without mutation.
- After a legal drop, the visible card order updates from the rewritten dataflow immediately. Save is still explicit.
- Optional cards expose Delete when bypass/removal leaves a valid graph. Required source, baseline, dependency, managed fusion and finalization cards show Delete as unavailable with the reason.
- Stages 01–03 expose product-approved Add/Restore actions. These actions create a typed node, analyzer attachment and evidence edge together; they are not an arbitrary raw graph-node constructor.
- Stage 04 is one required Engine fusion-policy card. Users choose typed evidence ownership there, but may not add, delete, duplicate or drag Engine-internal normalization, candidate-graph, decode or finalization stages.

## 17.6 Provider presentation per card

The card title remains capability-first, while configured provider intent is always visible as secondary metadata. Cards with multiple truly interchangeable Engine-recognized providers expose a selector after selection; cards with one eligible provider show that fixed provider without a fake dropdown.

A multi-output semantic transformation owns one provider slot per independently generated output. In particular, Vocal / BGM separation displays both:

```text
Vocal output provider
BGM / Instrumental output provider
```

The package may currently have only one eligible Production provider in either slot; the UI must show that truth rather than pretending there are additional choices. Continuous-pitch and note-evidence cards must include the configured model in the visible card heading/metadata so two cards with the same capability label remain distinguishable. Exact resolved generation/backend stays in Plan Preview.

## 17.7 Four-step live DAG

The Advanced DAG is the execution projection of the same four Processing Studio steps, rendered as four horizontal rows:

```text
01  Pre-processing          → model/audio operations left to right
02  Lyrics                  → transcription, challenger, fusion, alignment
03  F0 & singing experts    → one node per concrete expert/model execution
04  Engine fusion policy    → fusion, candidate graph, decode/quantize/finalize
```

Each row has a persistent Step label and each DAG node represents exactly one concrete model or Engine-native processing operation. A multi-output card that invokes two models, such as Vocal / BGM separation, expands into separate Vocal extraction and Instrumental extraction DAG nodes while remaining one product-semantic card in Processing Studio. The exact Engine Plan marks an unrequested concrete model node as `Not requested`; a completed sibling must never make it look executed.

During execution, `uta-analyze` emits typed, request-correlated `node_started`, measured `node_progress`, `node_completed`, `node_failed`, `artifact`, `warning`, and `degraded` frames. Frames carry raw Engine node ID, optional Processing Studio presentation-node ID, capability ID, model ID and event timestamp. Studio stores raw and presentation identities separately, highlights only the active node, shows the configured/actual model, and displays a percentage only when the worker supplied a measured fraction/work-unit total. Native stages without measured units remain visibly indeterminate rather than receiving invented stage-order percentages.

---

# 18. Full candidate execution order

A typical original-mix Candidate request should be presented conceptually as:

```text
TrueSource / Original Mix
        ↓
Decode + source validation
        ↓
Vocal extraction
        ↓
Lead isolation
        ├──────────────→ Transcription → Transcript fusion
        │
        ├──────────────→ Alignment
        │
        ├──────────────→ Continuous F0 / RMVPE
        │
        └──────────────→ GAME note/boundary evidence
                               │
Acoustic evidence ─────────────┤
Transcript/alignment ──────────┤
Pitch evidence ────────────────┤
                               ↓
                         Singing Fusion
                               ↓
                         Candidate Graph
                               ↓
                       Candidate VocalChart
```

If Instrumental is requested:

```text
TrueSource / Original Mix
        ↓
Decode
        ↓
Instrumental extraction
```

That branch is independent and should appear as a parallel branch in preview.

---

# 19. Partial-analysis execution order

The preview must be request-specific.

## 19.1 Transcript only

```text
TrueSource
→ Decode
→ Vocal extraction if needed
→ Lead isolation if needed
→ Qwen ASR
→ Transcript
```

No GAME or RMVPE unless actually required by the request.

## 19.2 Canonical-lyrics alignment only

```text
TrueSource
→ Decode
→ lead preparation if needed
→ Qwen Forced Aligner + canonical lyrics
→ Alignment
```

No ASR requirement.

## 19.3 Pitch evidence only

```text
TrueSource
→ Decode
→ lead preparation if needed
→ RMVPE
→ PitchEvidence
```

No GAME requirement.

## 19.4 Instrumental only

```text
TrueSource
→ Decode
→ MelBand Instrumental
→ InstrumentalStem
```

No vocal extraction, lead isolation, ASR, aligner, RMVPE or GAME.

---

# 20. Run Analysis dialog

Before queueing a meaningful analysis run, Studio should expose a compact run sheet.

Recommended structure:

```text
Analyze “Song Title”

OUTPUTS
[x] Candidate chart
[x] Pitch evidence
[x] Transcript / alignment
[ ] Instrumental

QUALITY
Balanced                       [This run only ▾]

LYRICS
Generate lyrics / Use supplied lyrics / Timed lyrics

EFFECTIVE PROFILE
Global default + 1 song override + 0 run overrides

READINESS
Candidate chart    Ready
Pitch evidence     Ready
Transcript         Ready

[View execution plan]
[Manage models…]

Cancel                                  Analyze
```

If the requested output set contains a blocked required capability, disable Analyze and show the specific blocker.

Do not automatically uncheck an output to make the button available.

---

# 21. Plan Preview

The Plan Preview is read-only and represents the exact serialized request that will be queued.

It has four layers.

## 21.1 Request summary

Show:

```text
TrueSource
source role
lyrics mode
quality
requested artifacts
Production policy
```

## 21.2 Execution graph

Show Engine capability nodes and dependency order.

Status values:

```text
Ready
Optional
Blocked
Not requested
```

Do not use “Installed” as an execution-node status.

## 21.3 Resolved implementation

For each relevant node:

```text
Capability: pitch.track
Provider: RMVPE
Backend: OpenVINO
Validation: ProductionPinned
Status: Ready
```

A future blocked route is rendered from the exact Runtime Manager fact, for example:

```text
Capability: notes.game
Provider: GAME
Status: Blocked · model not installed
```

## 21.4 Output list

Show declared outputs and where Studio will commit them conceptually:

```text
Candidate VocalChart
PitchEvidence
Transcript
Alignment
InstrumentalStem
```

Do not expose the run-temp path unless in diagnostics.

---

# 22. No silent fallback rules

The following table is normative.

| User selection | Selected provider unavailable | Behavior |
| --- | --- | --- |
| Automatic | Preferred provider unavailable, another production-approved provider exists | Engine may resolve the approved alternative; preview shows it |
| Explicit provider | Provider unavailable | Block; do not substitute |
| Optional Auto | No optional provider available | Continue without it and state not selected/unavailable as appropriate |
| Optional explicit provider marked required for this run | Unavailable | Block |
| Benchmark-only provider in Production | Installed | Still blocked/not executed |

Any Engine fallback that changes result identity must be fingerprinted and visible in provenance.

---

# 23. What “Automatic” must display

Avoid opaque magic.

A resolved Automatic row should show both intent and result:

```text
Primary pitch
Automatic
Resolved: RMVPE · OpenVINO
```

Before resolution:

```text
Primary pitch
Automatic
Engine will choose a Production-validated provider
```

If blocked:

```text
Automatic
No Production-usable provider available
```

---

# 24. Models & runtime page relationship

The resource page must be navigable from blocked settings/preview rows, but must not mutate Analysis preferences merely because an install completes.

Example:

```text
Analysis preference: Automatic
required model not installed
→ user opens Models & runtime
→ user explicitly installs it
→ Automatic can resolve it on the next preview
```

Example explicit choice:

```text
Pitch primary: RMVPE
RMVPE removed
→ preference remains RMVPE
→ run blocked until the user repairs/reinstalls or changes preference
```

This persistence is important: resource state is not user intent.

---

# 25. Model status presentation

Use restrained badges/text, not loud green/red tiles.

Recommended vocabulary:

```text
Ready
Not installed
Legacy
Integrity failed
Runtime unavailable
Production blocked
Candidate
Experimental
Unsupported
```

When a resource is locally present but not usable, show both facts.

Current packaged model routes display `ProductionPinned`; readiness still reports installation, runtime and structural state independently.

A future catalog may again contain Candidate/Experimental routes. This status vocabulary avoids equating downloaded bytes with usable Production inference.

---

# 26. Song Detail analysis overrides

Song Detail may expose a compact “Analysis profile” section rather than duplicating the entire Settings page.

Recommended controls:

```text
Profile: Inherit global / Fast / Balanced / Maximum
Lyrics language: Inherit / ...
Audio preparation: Inherit / Custom…
Experts: Inherit / Custom…
```

A “Configure…” action may open a song-scoped version of the same Analysis settings UI.

Every row must state:

```text
Existing chart data changes only after re-analysis.
```

Do not re-run as a side effect of changing a profile.

---

# 27. Run override interaction

From Plan Preview or the node context menu, a supported control may be overridden for one run.

The UI must differentiate:

```text
Change for this run
Save as song profile
Change global default
```

Never overload one Save button to perform all three.

Run overrides disappear after queueing/execution and are serialized into the exact request/Studio request metadata.

---

# 28. Settings storage model

The current `AppConfig` contains legacy concrete fields such as:

```text
separator
asr_engine
align_backend
pitch_model
```

Do not proliferate more string fields of this shape indefinitely.

Introduce a versioned Studio-side product configuration structure, for example conceptually:

```text
AnalysisDefaultsV1
  quality_profile
  audio_preparation
    vocal_extraction_preference
    lead_isolation_preference
    instrumental_preference
    vocal_cleanup_policy
    instrumental_cleanup_policy
  speech
    transcription_strategy
    alignment_strategy
  singing
    pitch_primary_preference
    note_primary_preference
    optional_experts_policy
  advanced
    provider-scoped parameter sets
  auto_analyze
```

Exact Rust names may follow repository conventions, but the schema must be versioned and serializable.

During migration:

- read existing config;
- map unambiguous old values to the new semantic settings;
- retain compatibility fields only as long as required by the release migration strategy;
- do not turn an invalid old model name into an arbitrary new provider.

---

# 29. Provider preference representation

Recommended conceptual type:

```text
ProviderPreference
  Automatic
  Explicit(ResourceRef)
```

For optional roles:

```text
OptionalProviderPreference
  Automatic
  Off
  Explicit(ResourceRef)
```

Persist stable Runtime Manager resource identities, not paths.

Validation when loading settings:

- syntactically invalid resource ID → fall back to Automatic with a migration warning/log;
- unknown-but-well-formed resource from a future catalog → preserve the stored value where possible and show unresolved, rather than silently rewriting it;
- never mutate settings simply because the resource is temporarily uninstalled.

---

# 30. Engine contract boundary

Do not push Studio settings wholesale into Engine.

The compiler should produce only Engine-supported semantics.

For `AnalyzeRequestV1`, that currently includes:

```text
analysis.profile
track_target
preserve_continuous_pitch
enable_quantization
requested artifacts
lyrics mode/tokens
musical context
execution policy
```

Provider preferences and advanced parameters that are not represented by v1 must **not** be hidden in ad-hoc `extensions` and treated as real execution controls.

Options are:

1. keep them on the legacy/Processing Studio route until a typed Engine contract exists;
2. add a reviewed versioned Engine contract extension/version;
3. hide/disable the control for the Engine-backed route.

Do not silently ignore a visible setting.

---

# 31. Provider preference contracts

Processing Studio now carries stable card-level provider intent through its versioned `WorkflowExecutionV1` extension. The remaining future work in this section applies to general Settings/Song/Run preferences outside a persisted workflow.

When multiple production providers exist outside Processing Studio, add a versioned typed mechanism rather than model-name strings scattered through Studio.

A future request could conceptually carry:

```text
capability_preferences:
  pitch.track: Automatic | model:rmvpe
  speech.transcribe: Automatic | model:qwen3_asr_1_7b
```

Requirements:

- capability-keyed;
- resource ID typed;
- Runtime Manager policy remains authoritative;
- explicit preference failure is distinguishable from automatic resolution failure;
- included in deterministic fingerprint;
- reflected in plan/provenance;
- contract versioning rules followed.

Do not implement this shape in v1 without formally versioning the contract.

---

# 32. UI state model

Each analysis-setting row should derive these independent facts:

```text
preference
preference source (global/song/run)
resolved provider if known
resource state
production usability
blocked reason
parameter applicability
```

Do not compress them into one Boolean.

Conceptual view model:

```text
AnalysisPreferenceRow
  label
  description
  effective_preference
  preference_source
  resolved_provider
  runtime_status
  available_options
  enabled
  blocked_reason
  manage_resource_action
```

The renderer remains dumb: it should not inspect model directories.

---

# 33. Request-specific readiness on Analysis settings

A settings page itself is not “ready” or “not ready”.

At most it shows informational status per capability.

The actual run action computes readiness from the exact requested artifacts.

For example, if GAME is later unavailable:

```text
Analysis settings
Pitch primary       RMVPE · Ready
Note primary        GAME · model not installed
Alignment           Qwen · status...
```

but:

```text
Refresh pitch
=> may still be enabled
```

This rule must be covered by UI tests.

---

# 34. Error and blocker copy

Use capability/resource-specific messages.

Good:

```text
Candidate chart is unavailable because the required GAME note/boundary expert is not installed.
```

Good:

```text
Pitch evidence is unavailable because RMVPE cannot be resolved under Production policy.
```

Good:

```text
Your explicit RMVPE preference cannot be used. Change the preference or repair RMVPE in Models & runtime.
```

Bad:

```text
AI unavailable
Runtime missing
Model error
```

when a more precise reason is known.

---

# 35. Preview exactness

If a user changes any field after a preview was generated, invalidate the preview and recompute.

Examples:

```text
quality profile changed
requested artifact changed
lyrics mode changed
run override changed
provider preference changed
Processing Studio topology changed
source identity changed
```

The Analyze button must queue the exact request represented by the visible preview.

Do not reconstruct from mutable settings after confirmation.

---

# 36. UI visual hierarchy

Follow the existing design direction:

- clean hierarchy;
- quiet controls;
- restrained translucent surfaces;
- softened separators;
- no bright focus boxes;
- selected state through subtle indicator/type weight/contrast;
- status badges restrained;
- settings controls aligned to one right-hand column;
- no duplicate Settings control.

Recommended hierarchy per stage:

```text
EYEBROW / STAGE
Title
One-sentence purpose                         [status]
-----------------------------------------------------
Preference row                               [control]
Optional strategy                            [control]
Advanced tuning                          [Show advanced]
```

Do not turn Settings into a node-editor canvas. Processing Studio already serves that purpose.

---

# 37. Accessibility and keyboard behavior

All selectors, segmented controls, toggles and numeric controls must be keyboard reachable.

Focus must be visible without a loud rectangular highlight.

Disabled controls must retain readable labels and state.

Do not hide the reason for disabled state only in hover text.

Status semantics must not depend solely on color.

---

# 38. Mobile/narrow window behavior

Uta! Studio desktop may still be resized narrow.

Under narrow width:

- setting descriptions may wrap;
- the whole right-side control may move below the description;
- stage headers may wrap actions below status;
- execution graph preview may switch from horizontal DAG to vertical list;
- do not squeeze selectors below usable width.

The underlying hierarchy must remain the same.

---

# 39. Required UI tests

For the Engine v1 integration, add tests for at least:

```text
Balanced is the default profile
single-provider capability does not render a fake multi-option selector
Automatic shows resolved provider separately
Automatic may show an alternate only when Engine plan actually resolves it
BenchmarkCandidate is not production-ready
GAME blocker does not disable pitch-only action
canonical alignment does not display ASR as required
instrumental-only plan does not display vocal models as required
advanced controls hide when owning provider/strategy does not support them
run override wins over song profile over global
changing preview-affecting setting invalidates preview
Models & runtime navigation does not mutate preference
changing Analysis settings does not trigger installation
changing Song Profile does not start re-analysis
```

For general Settings/Song/Run preferences, after a versioned provider-preference contract exists, additionally test:

```text
explicit provider remains selected while resource is missing
explicit provider does not silently fall back
explicit provider blocker is distinct from Automatic resolution
```

---

# 40. Required compiler tests

Cover semantic compilation independently from Bevy UI.

```text
Global Balanced → AnalyzeRequestV1 Balanced
Song Fast override → Fast
Run Maximum override → Maximum
preserve continuous pitch maps correctly
quantization maps correctly
transcript-only output request does not broaden to Candidate
instrumental-only output request stays independent
canonical lyrics maps to LyricsMode::Canonical
settings with unsupported Engine-only parameter return explicit unsupported state rather than silent omission
```

---

# 41. Required resource-preference tests

For the future general Settings/Song/Run provider-preference contract, cover:

```text
Automatic + primary ready
Automatic + primary unavailable + approved alternate
Explicit ready
Explicit unavailable
Explicit BenchmarkCandidate under Production
unknown preserved resource id
malformed resource id migration
preference included in fingerprint
resolved provider recorded in provenance
```

These general preference tests remain deferred until that request contract exists. Processing Studio card-provider tests are valid now because `WorkflowExecutionV1` independently versions and validates that intent.

---

# 42. Required Processing Studio tests

Cover:

```text
reordering role-preserving cleanup changes topology order
vocal and instrumental branches remain independent
invalid dependency reorder is impossible/rejected
node primary identity is capability, provider is metadata
unsupported Engine semantics are surfaced as unsupported
workflow does not override Runtime Manager Production validation
```

---

# 43. Suggested code boundaries

Do not implement all behavior inside `desktop/src/studio/settings/analysis.rs`.

Recommended separation follows the four-agent ownership protocol:

```text
LANE C — app-core integration/domain seam

app-core/src/analysis_experience.rs
    versioned product settings
    inheritance resolution
    migration from legacy AppConfig

app-core/src/analysis_engine_adapter.rs
    effective analysis intent
    AnalyzeRequest compiler
    exact request snapshot

app-core/src/analysis_plan_projection.rs
    request-specific readiness
    resolved provider/status projection

LANE D — desktop UX

desktop/src/studio/settings/analysis.rs
    rendering only + UX actions

desktop/src/studio/settings/analysis_view_model.rs
    desktop-only row/layout derivation from Lane C data

desktop/src/studio/song_detail/...
    song-profile surface

desktop/src/studio/analysis_...
    Run Analysis / Plan Preview presentation

desktop/src/studio/processing_studio/...
    capability topology editing
```

Current work ownership and repository rules are defined by `AGENTS.md`, while `tasks/remaining-models/STATE.md` records closure state; any app-core/Desktop work must still preserve the process boundary defined here.

Use existing repository module boundaries where they already serve these responsibilities; the names above are architectural suggestions, not a requirement to create every file verbatim.

---

# 44. Migration from current Analysis UI

Current code already has useful controls and should evolve incrementally.

Preserve:

```text
separation advanced tuning
transcription advanced tuning where truly supported
pitch sensitivity
vocal/BGM independent cleanup defaults
auto-analyze
Manage models… navigation
settings row alignment
```

Change:

```text
one global hard-coded runtime status
→ per-capability/request-specific status

model filenames as primary workflow identity
→ capability/strategy + resolved provider metadata

fake one-item dropdowns
→ fixed resolved row or Automatic display

FireRed + Qwen hard-coded baseline copy
→ Qwen baseline plus optional challenger semantics matching Engine plan
```

Do not remove existing settings until the replacement path is implemented and migrated.

---

# 45. Immediate implementation checkpoint

The Studio integration agent may implement the following before the full Engine execution gate:

```text
[ ] versioned Studio AnalysisDefaults model
[ ] Global/Song/Run inheritance resolver
[ ] quality segmented control
[ ] semantic capability preference view models
[ ] per-capability Runtime Manager status rows
[ ] remove fake one-option selectors from presentation where practical
[ ] Run Analysis dialog request-output selection
[ ] Plan Preview fixture UI
[ ] exact preview invalidation rules
[x] Processing Studio capability-first labels with visible provider metadata
[x] legal same-branch pointer drag plus immediate semantic-order refresh
[x] optional Add/Restore/Delete card lifecycle for stages 01–03
[x] fixed Engine-owned Stage 04 fusion-policy presentation
[ ] UI tests using Engine/Runtime Manager fixtures
```

Do not claim provider preferences affect real Engine v1 execution unless the versioned Engine contract actually supports them.

---

# 46. Final UX invariant

The finished product should be explainable in four sentences:

> **Analysis chooses how Uta! Studio should analyze. Processing Studio chooses the allowed product-level processing topology. Models & runtime manages what is locally installed and production-usable. Plan Preview shows exactly what the Engine will actually run before the user commits.**

And one safety rule:

> **An explicit user model preference may block when unavailable; it must never be silently replaced by a different model.**
