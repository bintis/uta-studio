# 21 — Final-v1 Design Parity Audit

**State:** `NEEDS_REVIEW`
**Audit revision:** 6 (full current-source parity rerun after follow-ups 21E–21H)
**Precondition:** cards 15–19, 20A, and 20 are terminal; card 20A and card 20 should be `READY` for a green implementation result
**Task class:** static/audit closure; no model inference
**Owner:** active implementation/review agent

## Read

```text
AGENTS.md
docs/engineering-constraints.md
tasks/final-features/PROCESS_BOUNDARY_RULES.md
tasks/final-features/STUDIO_BACKEND_UI_PARITY.md
tasks/final-features/21_FINAL_DESIGN_PARITY_AUDIT.md
docs/design/architecture/UTA_SEPARATED_ARCHITECTURE_DESIGN_v1.0.md
docs/design/audio-analysis/UTA_ANALYSIS_ENGINE_AUDIO_ANALYSIS_FRAMEWORK_v2.1_RC.md
docs/design/audio-analysis/UTA_EXPERT_FUSION_POLICY_AND_REPAIR_v1.0.md
docs/design/audio-analysis/UTA_AI_JUDGMENT_FUSION_MODE_v1.0.md
docs/design/audio-analysis/UTA_ANALYSIS_ENGINE_AUDIO_SEPARATION_PLAN_v1.1.md
docs/design/audio-analysis/UTA_AUDIO_ANALYSIS_COVERAGE_CHECKLIST_v1.0.md
docs/design/architecture/UTA_STUDIO_CLI_PROCESS_BOUNDARY_v1.0.md
docs/design/integration/UTA_STUDIO_ANALYSIS_RUNTIME_INTEGRATION_DESIGN_v1.0.md
docs/design/integration/UTA_STUDIO_ANALYSIS_SETTINGS_MODEL_SELECTION_EXECUTION_UX_DESIGN_v1.0.md
docs/KEY_CONCLUSIONS.md
tasks/final-features/followups/21D_ANALYSIS_EXPERT_SYSTEM_COMPLETION.md (while 21D is active or completed)
tasks/final-features/followups/21E_AI_JUDGMENT_FUSION_CLOSURE.md (while 21E is active or completed)
tasks/final-features/followups/21F_EXPORT_AND_EDITOR_EVIDENCE_CLOSURE.md (while 21F is active or completed)
tasks/final-features/followups/21G_FINAL_AUDIT_RUNTIME_AND_AUDITION_CLOSURE.md (while 21G is active or completed)
tasks/final-features/followups/21H_EDITOR_ARTIFACT_AUDITION_LIFECYCLE_CLOSURE.md (while 21H is active or completed)
tasks/final-features/followups/21I_STEP4_FINAL_FUSION_POLICY_CONVERGENCE.md (while 21I is active or completed)
tasks/final-features/followups/21J_MELODY_PATH_SCORE_COHERENCE.md (while 21J is active or completed)
tasks/final-features/22_UI_WORKFLOW_EXECUTION_UX_CONVERGENCE.md (while 22 is active or completed)
```

Use current source/tests plus `tasks/remaining-models/STATE.md` for prior card outcomes; deleted completion logs are not audit inputs.

## Mission

Answer one question from current source, not historical claims:

> After model closure and feature closure, is every final-v1 product capability either implemented and evidenced, or explicitly outside the final design / retained as an accepted blocker?

This is not the expensive release/Nix pass. It is the final design-completeness gate before the later explicit release pass reserved by `AGENTS.md`.

## Audit categories

### Repository/process architecture

Verify:

```text
Studio/backend implementation crates remain decoupled
Studio uses AnalysisCliClient / RuntimeCliClient only
Desktop -> app-core only
no HTTP inference/control service
no production Python/PyTorch/Transformers/uv/venv path
source media read-only
explicit model download/install policy
```

### Workflow / Processing Studio

Verify real implementation/evidence for:

```text
dynamic audio transformation reorder
type-invalid drop blocked
cycle blocked
duplicate processing nodes
Vocal/BGM/Lead/VocalResidual executable lanes, with Backing/Harmony retained only as chart roles/future audio identities
analyzer binds exact artifact
priority is not dependency
Always/OnDisagreement/MaximumOnly/Disabled execution
compiled DAG executes, not only previews
Advanced Graph matches compiled execution snapshot
Preview == queued execution snapshot/digest
```

### Analysis / experts / Fusion

Verify current capability registry against real call sites and result artifacts:

```text
transcript/alignment/pitch/note/acoustic evidence
optional experts
STARS/ROSVOT dependency-aware evidence
technique evidence
fusion.transcript
fusion.alignment
fusion.singing
fusion.candidate_graph
finalize.vocal_chart
rhythm.quantize contract, if retained
```

No `implementation_exists=true` without a real execution path.

### Audio semantics

Verify:

```text
Original/Vocal/Guide/Lead/CleanLead/Instrumental/Backing/Harmony semantics are not relabeled guesses
lossless/lossy encoding policy matches payload
source timeline preserved
no implicit CPU/Vulkan fallback
```

### Editor / Evidence Workbench

Verify:

```text
existing notes/lyrics editing preserved
Lead/Harmony/Backing/Adlib chart tracks preserved
Candidate opens
Authored saves
re-analysis never silently overwrites Authored
Candidate/Authored compare/merge
Evidence layers read-only
Review Queue navigation
Suggestion accept undoable
Artifact source/waveform/playback selection
A/B audition
technique evidence presentation
```

### Export

Verify Studio/app-core owns user export and real code exists for:

```text
UTZ
UltraStar
exported audio validation
staging cleanup
```

Do not flag an unimplemented backend `AnalysisEngine::export()` placeholder as a product gap when Studio-owned export is the final architecture.

### Contracts / packaging readiness

Verify static state for:

```text
API capability catalogue completeness/uniqueness
i18n EN/zh-CN/ja parity
source file <= 2000 lines
runtime-lock identity/recipes/notices
packaging definitions include required CLIs/workers/notices
canonical env vars for analysis/runtime CLIs
```

The actual final package build is deferred to the later explicit release pass reserved by `AGENTS.md`.

## Dynamic follow-up rule

If this audit finds a concrete implementation gap, do not hide it and do not patch an unrelated subsystem inside the audit.

Finish:

```text
NEEDS_REVIEW
```

and provide a minimal proposed follow-up card name/scope.

Record a focused follow-up Markdown card under, when useful:

```text
tasks/final-features/followups/
```

The same active agent implements the follow-up, verifies it, then reruns the affected focused bubble and card 21 with a new audit revision/completion record.

Do not proceed to final repository acceptance while card 21 is `NEEDS_REVIEW`.

## Static checks

Allowed and expected:

```text
focused cargo tests/checks for changed packages if useful
capability registry scans
Cargo dependency scans
implementation namespace scans
project-name/source-size/i18n/docs static gates where cheap
process tree inspection
git diff --check
```

Do not run model inference or `nix build` in this static audit; accelerator authorization remains governed by `AGENTS.md` for any separately justified dynamic follow-up.

## Green result

Card 21 is `READY` only when:

```text
cards 01–13 are terminal, with each model completion distinguishing integration_ready from production_ready
cards 15–19 are READY
card 20A Studio/backend/UI parity closure is READY
card 20 product E2E feature bubble is READY
no known final-v1 coding capability is still a placeholder/fail-closed stub
process-boundary decoupling gates are clean
no false capability/model Production claim exists
```

If a model/license limitation is genuinely outside code control, report it explicitly rather than calling the implementation incomplete; release acceptance still decides whether such a retained blocker is acceptable.

## Durable completion update

Set card 21's current state/result in `tasks/remaining-models/STATE.md`. Update `docs/KEY_CONCLUSIONS.md` with any final durable design/parity conclusion. Do not create a completion log under `docs/`.

Include a concise parity matrix:

```text
design area | implementation | evidence | decoupling status | remaining blocker
```

Stop after this audit.

## Historical audit result — revision 3

**State:** `NEEDS_REVIEW`

Audit revision 3 reread the current source against the authoritative `docs/design` set after the repository-owner policy admitted every packaged model's effective non-CPU route to Production. Model validation labels are no longer a standing blocker, but concrete implementation gaps remain in Candidate/SingingAnalysis contracts, global pitch-alternative decoding, transcript escalation, GAME conditioning, expressive DSP, instrumental/topology quality, Analysis settings/run-sheet parity and typed lifecycle events.

Follow-up `tasks/final-features/followups/21D_ANALYSIS_EXPERT_SYSTEM_COMPLETION.md` is the single active serial checklist. Its first source tranche has already corrected normal Production-policy requests, Unsupported policy admission, FCPE-primary baseline review, unresolved optional-cleanup handling, Engine-side F0-fallback validation, standalone conditional defaults, optional technique artifact declaration and read-only Preview lyric behavior. Card 21 returns to `READY` only after 21D closes and this audit is rerun against current source/tests.

## Current audit result — revision 4 (2026-08-26)

**Result:** `READY`

Follow-up 21D is closed with every phase A–J item implemented. The current-source reread found no remaining product/design parity blocker. One stale static runtime-lock policy statement found during the reread was corrected before closure: the lock/spec now match Runtime Manager's repository-owner Production admission, with a model-pinned native backend, Vulkan for GGUF RoFormer/Qwen, OpenVINO for current IR models, CPU diagnostics only and no automatic fallback.

| Audit area | Current evidence | Result |
|---|---|---|
| Process ownership | Studio sends semantic request and capability/provider intent through independently owned wire DTOs; app-core/Desktop do not link backend implementation crates. | PASS |
| Runtime truth | Runtime Manager owns resolution. Every effective non-CPU route is `ProductionPinned`; RoFormer catalog artifacts identify the effective GGUF/Vulkan payloads, current IR models identify OpenVINO artifacts, and `native-inference/runtime-lock.json` plus the design lock spec match the catalog. Historical quality/license findings remain advisory and do not create fallback routes. | PASS |
| Processing Studio | Legal reorder/duplicate/analyzer attachment and separate Vocal/BGM provider intent are preserved without serializing backend node IDs, executable recipes or private worker parameters. Stage 04 remains managed fusion policy. | PASS |
| Plan Preview | Preview is read-only, Engine-resolved and request-exact; workflow identity includes `definition_digest`, and queued execution persists the exact preview request. | PASS |
| Analysis outputs | The run sheet independently selects Candidate chart, pitch evidence, transcript, alignment and Instrumental; empty requests fail closed and partial requests compile only required artifacts. | PASS |
| Expert contracts and fusion | Candidate/SingingAnalysis strict contracts, global pitch alternatives, transcript escalation, GAME conditioning, typed fusion policy, deterministic fallback and degraded/review metadata are implemented and tested. | PASS |
| Audio/topology quality | Generated-artifact separation checks, retained lead/residual evidence, Instrumental routing and typed uncalibrated topology are current; singer partitioning remains explicitly future-only. | PASS |
| Lifecycle/progress | Correlated typed events, one live node per invocation, measured-only percentages, indeterminate overall progress and structured failure history are present. Model lifecycle mutations are confined to Models & runtime. | PASS |
| Settings/UI/localization | Six Analysis sections, reanalysis copy, Candidate quantization default-on, control-to-action mapping, EN/zh-CN/ja parity and user-guide lifecycle copy are current. | PASS |
| Verification | Focused suites pass for Runtime Manager, Analysis Engine, app-core, Desktop and UTZ; formatting, unstaged diff, process-boundary, identity, source-size and disallowed-name scans pass. | PASS |

The reserved whole-workspace/Nix/final packaged acceptance remains a later explicit release pass and is not implied by this card-level `READY` result.

## Current audit result — revision 5 (2026-08-28)

**Result:** `NEEDS_REVIEW`

Revision 4 remains a valid historical READY result for the authoritative design set that existed on 2026-08-26. On 2026-08-28 the repository owner approved a new product-level Stage-4 capability, `docs/design/audio-analysis/UTA_AI_JUDGMENT_FUSION_MODE_v1.0.md`. The new mode is explicit/non-default, permitted in normal Production analysis, may use a networked provider, and is constrained to selecting verbatim real Engine candidates; it never fabricates measured evidence and never silently falls back to Algorithm.

Current source already contains a substantial AI-judgment prototype, but it does not yet match the approved ownership/provenance contract: Studio/AnalyzeRequest still carry a raw adapter executable path, Engine still has direct path/environment fallback, Runtime Manager does not yet own persistent `tool:fusion_agent_adapter` configure/status/resolve, and complete non-deterministic decision provenance/cache semantics plus EN/zh-CN/ja/user-guide disclosure are not yet closed.

Follow-up `tasks/final-features/followups/21E_AI_JUDGMENT_FUSION_CLOSURE.md` is the single convergence checklist. Do not treat revision 5 as a regression in the revision-4 feature set; it is a new design delta that must converge before Card 21 returns to `READY`. The reserved whole-workspace/Nix/final packaged acceptance remains separate.

## Current audit result — revision 6 (2026-08-28)

**Result:** `READY`

The full current-source rerun closed the revision-5 AI-judgment delta through follow-up 21E and then applied the dynamic follow-up rule to every concrete High/Medium finding discovered during independent audit rereads. Follow-up 21F closed UltraStar publication and the Engine SingingAnalysis-to-Editor evidence boundary; 21G closed adapter request supervision, already-semantic lead materialization and artifact A/B/waveform selection; 21H closed artifact-audition authorization, backing-file and native-playback lifecycle edges. All four follow-ups are `READY`, and the final focused reread found no remaining High/Medium issue.

| Design area | Implementation | Evidence | Decoupling status | Remaining blocker |
|---|---|---|---|---|
| Process ownership | Studio sends semantic requests through App Core clients to packaged `uta-analyze` / `uta-runtime`; Runtime Manager owns `tool:fusion_agent_adapter`. | Cargo dependency/process scans; App Core runtime lifecycle smoke; Runtime Manager `67 + 10` tests. | App Core/Desktop do not link backend implementation crates; no HTTP inference service. | None. |
| Workflow / Processing Studio | Exact compiled DAG, conditions, duplicate/reorder/attachment semantics, preprocessing defaults and request-exact Preview/queue identity are current. | App Core `397 passed / 0 failed / 1 ignored`; Desktop plan/Processing Studio tests. | Studio owns intent and wire DTOs; Engine owns executable plan. | None. |
| Analysis / Fusion | Algorithm and explicit AI judgment select real candidates; adapter I/O is bounded and supervised; shared path validation, no fallback and truthful mode-specific provenance/reuse semantics are enforced. | Analysis Engine `205 passed / 0 failed / 2 ignored` plus CLI integration `4 passed`; cancellation/backpressure/oversize/privacy/fingerprint tests. | Runtime resolves tools; Engine launches the verified endpoint; Studio never carries a raw executable path. | None. |
| Audio semantics | Ordinary preprocessing is optional/Off by default; explicit LeadVocal output is forced without relabeling guesses; already-semantic Lead/CleanLead is losslessly materialized without another model pass. | Planner/workflow/materialization tests; exact analyzer-route Preview tests. | Source media remains read-only; generated FLAC artifacts stay under authorized output roots. | None. |
| Editor / Evidence | Current SingingAnalysis is independently projected with strict candidate/provenance/digest/units validation; immutable artifact A/B, historical revisions and independent waveform selection are typed and lifecycle-safe. | App Core evidence/tamper/backing-file tests; Desktop `175 passed / 0 failed`; native audio `10 passed / 0 failed / 1 ignored`; final focused reread green. | Desktop consumes App Core DTOs/APIs only; immutable artifacts and source media are read-only. | None. |
| Export | Studio/App Core own UTZ and UltraStar export; UltraStar uses sibling staging, no-replace publication, assets-first/chart-last commit semantics, rollback and cleanup. | UTZ `13 + 3 + 5` tests; UltraStar race/rollback tests in App Core. | No backend export placeholder is treated as product ownership. | None. |
| Runtime / models | Runtime catalogue, persistent configuration, canonical adapter manifest identity, Production-pinned native routes and no automatic CPU fallback remain current. | Runtime Manager full suite; runtime-lock/catalog static reread. | Models & runtime owns lifecycle; Analysis owns analysis parameters. | None. |
| UI / localization / docs | Fresh readiness gating, exact Preview/no-provider-contact disclosure, actionable typed errors, A/B controls and EN / zh-CN / ja parity are current. | Desktop full suite, i18n key/placeholder checks, generated-doc check. | UI dispatches typed App Core/runtime actions only. | None. |
| Packaging readiness | Required CLIs/workers/notices and canonical environment variables remain present; source files stay within 2000 lines. | Static Nix/source-size/identity scans and formatting checks. | Actual Nix/package execution remains intentionally deferred. | Later explicit release pass only. |
| Repository hygiene | Current task diff, docs bundle and process/raw-path scans are clean. | `cargo fmt --all -- --check`; `cargo xtask docs check`; unstaged `git diff --check`. | One unrelated pre-existing staged whitespace finding remains in historical test evidence. | Not a Card 21 product blocker. |

The reserved whole-workspace/Nix/final packaged acceptance remains a later explicit release pass and is not implied by this card-level `READY` result.
