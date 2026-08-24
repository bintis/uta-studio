# Studio ↔ Backend ↔ UI Parity Matrix

**Status:** mandatory shared audit for final-v1 feature closure
**Applies to:** cards 15–21

This document prevents a feature from being considered complete when backend capability, Studio intent, and visible UI semantics disagree.

## 1. Frozen ownership / decoupling

```text
Desktop
  -> app-core only
      -> AnalysisCliClient -> uta-analyze -> Analysis Engine
      -> RuntimeCliClient  -> uta-runtime -> Runtime Manager
```

Hard gates:

```text
app-core/** and desktop/** must not Cargo-link uta-analysis-engine
app-core/** and desktop/** must not Cargo-link uta-runtime-manager
no uta_analysis_engine:: imports in Studio code
no uta_runtime_manager:: imports in Studio code
Desktop must not launch uta-analyze or uta-runtime directly
Studio owns local wire DTOs; backend owns implementation DTOs
Studio must not duplicate Engine planning or Runtime Manager lifecycle/policy logic
```

Studio owns user intent, queue/history, source authorization, artifact revisions, review/editor, and export. Analysis Engine owns analysis plan/execution/fusion/finalization. Runtime Manager owns resource lifecycle and Production usability.

## 2. Settings > Analysis parity

The page may expose only settings that have a truthful execution meaning.

Required:

- Global defaults affect future requests through the Global -> Song -> Run precedence chain.
- `Preserve continuous pitch` maps to the Engine request and never turns F0 into target notes.
- `Quantize candidate notes` must either map to a real versioned Engine capability or be disabled/retired; a clickable no-op is forbidden.
- Analysis does not install/download models.
- Provider labels are descriptive/automatic unless a typed provider-selection contract exists.
- Readiness labels must not claim a specific capability is ready from a coarser family/bundle status.

Card 20A closure:

```text
Vocal extraction       -> model:bs_roformer_vocals_ep317 / audio.extract_vocals
Instrumental           -> model:melband_roformer_inst_v2 / audio.extract_instrumental
Lead isolation         -> model:melband_roformer_harmony / audio.lead_isolate
Pitch                  -> model:rmvpe / pitch.track
Note boundaries        -> model:game / notes.game
```

Each Analysis strategy row consumes the exact Runtime Manager fact through app-core; aggregate RoFormer bundle health remains lifecycle-only and cannot gate an individual row.

### Advanced controls

Card 20A retired the legacy segment size, overlap, separator/ASR batch size, output normalization and voiced-sensitivity controls from the compiled canonical UI because they are not encoded into `AnalyzeRequestV1` / the compiled Workflow. Models & runtime now states that packaged parameters are not user-editable. No editable control remains whose value is ignored by the canonical path.

## 3. Settings > Models & runtime parity

This page is lifecycle-only.

Required:

- install/import/verify/status/repair/remove semantics come from `uta-runtime` facts;
- no analysis profile or provider-selection business logic lives here;
- `BenchmarkCandidate` / `Experimental` show Production blocked truthfully;
- aggregate component health never overrides exact request readiness;
- no automatic download on page open/status/preview;
- license/source/size confirmation remains explicit before acquisition.

The current explanatory copy that Plan Preview is request-specific authority is correct and must remain true.

## 4. Plan Preview parity

Plan Preview is the authoritative request-specific readiness surface.

Required:

```text
same serialized AnalyzeRequest snapshot used by Preview and Execution
request_id + digest identity stable
exact required capabilities/resources shown
exact requested artifacts shown
blockers come from Engine/Runtime facts, not desktop guesses
Analyze action enabled only when exact request is ready
Manage models action is navigation only; it must not install implicitly
```

If Workflow execution is requested, Plan Preview must display the exact compiled Workflow identity/revision/digest or an equivalent versioned Engine execution-plan identity. A Processing Studio `Run` must not bypass the exact preview/readiness contract.

## 5. Processing Studio / compiled Workflow parity

UI graph semantics must equal backend executable semantics.

Required:

- dynamic transform reorder changes only legal audio topology;
- invalid type drop is rejected;
- cycles are rejected;
- duplicate transforms are supported where the capability permits them;
- analyzer attachment selects the exact audio artifact, not “latest vocal”;
- priority changes scheduling preference only and never creates a dependency edge;
- Advanced Graph displays the exact compiled DAG/snapshot that execution uses;
- `Run` executes through app-core -> AnalysisCliClient -> `uta-analyze`, never the legacy native-analyzer compatibility fallback;
- failed compiled execution is surfaced as a typed blocker/error, never fake success.

### Execution-policy UI

Card 20A replaces the lossy Policy cycle with explicit choices for `Always`, `OnDisagreement`, `DisagreementWindows`, `MaximumOnly`, and `Disabled`. Processing cards label execution condition, scheduling priority and hard dependencies separately.

Conditional policy shown in UI must correspond to real scheduler behavior after card 16; metadata-only conditions are not sufficient.

## 6. Audio lane / role parity

Final-v1 executable audio semantics are:

```text
Vocal/BGM separation
Lead isolation -> LeadVocal + VocalResidual
```

`audio.lead_partition` means partitioning simultaneous foreground singers and is future/optional. Processing Studio must not advertise it as a Backing/Harmony transformation, and independent BackingVocal/HarmonyVocal stem requests must fail closed.

Editor roles remain a separate authoring domain:

```text
Lead
Harmony
Backing
Adlib
```

Workflow schema persistence may retain distinct BackingVocal, HarmonyVocal and VocalResidual identities for future compatibility, but only `VocalResidual` is produced by current lead isolation. Do not infer an executable audio stem from an Editor role, and do not relabel the residual as Backing or Harmony.

## 7. Optional expert parity

After promotion/conditional scheduling:

```text
FireRed != automatic Qwen replacement
FCPE != RMVPE replacement
Basic Pitch != GAME replacement
STARS/ROSVOT dependency-conditioned evidence != independent vote
```

UI must represent these as optional/conditional experts unless product policy explicitly changes. Do not show an enabled Production toggle for a backend still marked `BenchmarkCandidate`.

If an expert runs only on disagreement windows, UI/graph/provenance should make that conditional execution visible enough to diagnose why it ran.

## 8. Technique parity

With card 18's real `technique.analyze` route:

- Studio/Editor shows technique evidence as evidence/review, not extra MIDI notes;
- raw STARS logits are not labeled calibrated confidence;
- technique evidence remains read-only until explicit suggestion/authoring action;
- optional style/global attributes must have a typed contract before UI is added.

## 9. Quantization parity

With card 19's Engine quantization retained in final-v1:

- Settings toggle maps to a real request field;
- Planner node maps to a real execution stage;
- result artifact/field is typed and visible to Studio;
- Candidate symbolic timing changes are distinguishable from raw Candidate and continuous F0;
- Editor authoring quantize remains a separate human-authoring command.

Missing explicit song BPM/grid must block Preview rather than guess a tempo.

## 10. Candidate / Review / Editor parity

Required end-to-end:

```text
Engine Candidate -> Studio Candidate revision
Candidate opens in Editor
Evidence layers are read-only
Review Queue navigates real ReviewRegions
Suggestion acceptance is explicit + undoable
Candidate/Authored compare/merge works
re-analysis creates a new Candidate and never overwrites Authored
A/B audition plays the selected artifact/revision
Lead/Harmony/Backing/Adlib chart tracks remain preserved
```

The Editor already has these structures/actions in code; cards 20/21 must verify the actual user path, not assume presence of symbols equals feature completion.

## 11. Export parity

Export remains Studio-owned.

Required:

- Engine returns typed artifacts; it does not own UTZ/UltraStar UX;
- app-core validates the selected Candidate/Authored revision before export;
- UTZ/UltraStar export paths use the intended chart revision and semantic tracks;
- exported audio MIME/extension matches bytes;
- source media remains read-only;
- temporary failures clean up atomically.

Do not move export logic into Analysis Engine merely because `AnalysisEngine::export()` has/once had a placeholder method.

## 12. UI completeness acceptance

Card 20 must exercise, and card 21 must statically audit, at least:

```text
Settings > Analysis
Settings > Models & runtime
Plan Preview
Processing Studio
Advanced Graph
Analysis activity/error/blocker presentation
Candidate/Review Editor
Artifact source picker / A-B audition
Export actions
```

For every visible control, answer:

```text
What app-core intent does it mutate?
What exact wire field or local Studio action consumes that intent?
What backend capability/resource owns the behavior?
What happens when the backend says unavailable/blocked?
Can the UI display a state the backend cannot actually perform?
```

Any visible enabled no-op, stale provider selector, family-level readiness presented as exact capability readiness, or backend feature with no reachable UI path is a parity defect.
