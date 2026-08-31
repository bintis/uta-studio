# Uta! Studio — AI Judgment Fusion Mode v1.0

**Status:** Approved and implemented; 21J hard-pool protocol convergence under final review

**Date:** 2026-08-28

**Scope:** Stage 4 candidate-path decision only
**Related authority:** `UTA_SEPARATED_ARCHITECTURE_DESIGN_v1.0.md`, `UTA_EXPERT_FUSION_POLICY_AND_REPAIR_v1.0.md`, `UTA_STUDIO_CLI_PROCESS_BOUNDARY_v1.0.md`

## 1. Decision

Uta! Studio supports two explicit Stage 4 decision modes:

```text
Algorithm   default deterministic Engine decoder
AI judgment explicit non-default external AI-assisted candidate selection
```

Normal Studio analysis still uses `RuntimePolicy::Production`. `AI judgment` is allowed in a Production analysis when the required fusion-agent tool is resolved and usable. This does **not** make the external AI provider a `ProductionPinned` model route; Runtime Manager validation states continue to describe models/runtimes/tools, while `fusion_mode` is product/Engine execution intent.

There is never an automatic transition between the two modes. The user's selected mode is part of the exact queued request/workflow identity.

## 2. Ownership

The frozen component rule remains:

```text
Runtime Manager = what verified resources/tools can run
Analysis Engine = how evidence is analyzed and validated
Studio          = what the user wants to do
```

The fusion-agent executable path is therefore **Runtime Manager ownership**, not Studio workflow authority and not an arbitrary path carried in `AnalyzeRequest`.

The canonical external resource is:

```text
tool:fusion_agent_adapter
```

Runtime Manager owns:

- configured executable path and persistence;
- executable/readiness validation;
- external-tool status/resolve metadata;
- the resolved executable returned to Analysis Engine;
- future adapter identity/version metadata when available.

Studio owns only the user-visible selection and configuration action. Studio may request `fusion_mode=ai_judgment` and may ask Runtime Manager to configure/clear the adapter tool, but it must not serialize a raw executable path into the workflow or Analysis request.

Analysis Engine consumes the resolved adapter executable as an execution dependency. Direct Engine fallback to a separate environment-variable path is not part of the final contract; any supported environment override belongs to Runtime Manager's external-tool discovery layer.

## 3. Adapter contract, not arbitrary coding-agent CLI

The Engine-facing executable is a **Fusion Agent Adapter** implementing Uta!'s bounded stdin/stdout contract. The adapter may internally call Codex, Claude, another hosted provider, or a local model.

A raw general-purpose coding-agent executable is not automatically compatible merely because it is an AI CLI. It is supported only when it directly implements, or is wrapped by something that implements, the Uta fusion-agent protocol.

Required process properties:

- direct executable launch; no shell command string;
- one bounded version-4 JSON request on stdin containing a compact index-addressed candidate decision projection, canonical lyrics, the complete-pool digest, and the normalized caller-hard boundary set;
- one bounded version-4 JSON response on stdout containing selected candidate indices;
- human/provider diagnostics on stderr only;
- timeout and cancellation terminate the adapter process tree;
- malformed, oversized, polluted, or non-zero-exit responses fail closed.

## 4. What the AI is allowed to decide

AI judgment is a **selector, not an evidence generator**.

The Engine constructs the same real `SegmentCandidate` pool used by the algorithmic path. The adapter may select only candidates that were present in that pool. It may not invent or edit:

- note boundaries;
- MIDI targets;
- confidence values;
- evidence values;
- source identities;
- timestamps;
- model/provider provenance.

Every returned candidate index must identify an input candidate exactly. The Engine retains the same complete Candidate Pool used by Algorithm, including the normalized pool-level caller-hard boundary set, and maps selected indices back to immutable full candidates. It then applies the same selector-independent membership, ordering, exact voiced-component coverage, hard-boundary, timeline, and canonical-output validation required for the algorithmic path.

If the correct answer is absent from the candidate pool, the AI is not permitted to fabricate a repair. The run must fail or surface review/degraded evidence according to the surrounding Engine contract. A future generative-repair feature would require a separate versioned design and artifact/provenance contract.

## 5. Network and privacy boundary

AI judgment may use a networked provider. This is an explicit property of the mode and must be disclosed in UI/help text.

The Uta -> adapter payload is intentionally bounded to fusion decision data. The version-4 contract includes a compact candidate projection, canonical lyrics, the complete-pool digest, and the normalized caller-hard boundary set needed to choose among candidates, but must not include source audio bytes, arbitrary project files, the library database, model files, or unrelated user content.

The adapter writes only `candidates.json`, `lyrics.json`, and `hard_boundaries.json` into a fresh temporary working directory. The provider prompt names those relative paths instead of embedding their contents, and the directory is removed on every adapter exit path. The full typed Candidate Pool remains inside Analysis Engine and is never copied into the provider prompt or temporary provider workspace.

The adapter/provider may receive the compact candidate metadata and canonical lyrics over the network according to the user's configured third-party provider. Provider credentials remain owned by the user's adapter/provider environment; Uta! Studio must not place provider secrets in command-line arguments or persist them in analysis artifacts.

The configured third-party adapter executes with the user's OS permissions. Uta! Studio constrains what it sends over the protocol, but cannot claim that an arbitrary external executable is sandboxed.

## 6. Failure semantics

When `fusion_mode=ai_judgment`, all of the following are hard analysis failures for the affected run:

```text
adapter tool unresolved/unusable
spawn failure
protocol/version mismatch
stdout pollution or malformed JSON
response too large
provider/adapter non-zero exit
timeout
cancellation
empty selection
selection containing a fabricated/modified candidate
invalid/non-ordered/overlapping final path
selection that gaps represented voiced coverage or crosses a caller-hard boundary
canonical singing-track validation failure
```

There is **no silent fallback to Algorithm**. A fallback would make the produced artifact's decision provenance disagree with the user's explicit request.

## 7. Provenance and reproducibility

AI judgment is not assumed deterministic. The same candidate pool may produce a different valid selection after provider/model changes.

Every AI-judgment result must therefore retain at least:

```text
decision_mode = ai_judgment
adapter_resource = tool:fusion_agent_adapter
adapter_protocol_version
resolved adapter identity/version when Runtime Manager can provide it
input Candidate Pool digest, including normalized caller-hard boundaries
selected candidate ids
adapter response digest
```

Do not request, store, or expose the provider's private chain-of-thought.

The deterministic upstream evidence/candidate artifacts remain cacheable normally. The AI decision stage must **not** be implicitly reused merely because the same deterministic execution fingerprint is seen again. Reuse is allowed only through an explicitly preserved prior analysis/artifact revision whose AI decision provenance is retained. A fresh AI-judgment run is a new external decision event.

The request/workflow fingerprint must still include `fusion_mode` and the stable adapter resource/protocol identity so Algorithm and AI-judgment requests can never alias.

## 8. Processing Studio and Settings UX

### Models & runtime

Models & runtime owns the Fusion Agent Adapter configuration and status:

```text
Fusion Agent Adapter
status: configured / missing / unusable
Choose executable…
Clear
```

The UI should describe this as an external tool, not a model.

### Processing Studio Stage 4

Stage 4 exposes:

```text
Decision mode
[Algorithm] [AI judgment]
```

Algorithm remains the default. AI judgment is enabled only when Runtime Manager reports the adapter tool usable. Selecting AI judgment shows a concise disclosure that compact candidate metadata and canonical lyrics may be sent to the configured external AI provider.

### Plan Preview

Preview must show the exact decision mode and, for AI judgment, the resolved tool resource/readiness. Preview remains read-only and must not invoke the AI provider.

### Localization and user guide

All new Settings, Processing Studio, Preview, failure, and network-disclosure copy must be present in EN / zh-CN / ja and documented in the user guide.

## 9. Process-boundary contract

The Studio-owned workflow wire may carry the typed stable value:

```text
fusion_mode = algorithm | ai_judgment
```

It must not carry the resolved executable path.

`AnalyzeRequest` must not carry a Studio-selected `PathBuf` for the adapter. Engine obtains the resolved tool through Runtime Manager-owned resolution before execution.

The Analysis Engine remains responsible for:

- candidate-pool construction;
- adapter request shaping;
- strict response validation;
- candidate membership checks;
- final canonical validation;
- lifecycle/error reporting;
- AI decision provenance.

Runtime Manager does not perform fusion and the adapter does not become a Runtime Manager algorithm.

## 10. Current implementation convergence

Follow-up `tasks/final-features/followups/21E_AI_JUDGMENT_FUSION_CLOSURE.md` is `READY`. Current source matches this ownership and execution design:

- Runtime Manager persistently configures, validates, reports and resolves `tool:fusion_agent_adapter` through an executable-specific Uta protocol manifest;
- Studio and `AnalyzeRequest` carry typed `fusion_mode` and stable resource intent, never a raw adapter executable path;
- Engine launches only the Runtime Manager-resolved adapter and supervises bounded request/response I/O, timeout, cancellation and process-tree cleanup;
- Algorithm and AI judgment use the same complete Candidate Pool and selector-independent exact-coverage/hard-boundary/canonical validation, with no fallback between modes;
- Fusion Agent protocol version 4 carries a compact index-addressed candidate projection, canonical lyrics, and the exact normalized caller-hard boundary set under one bounded full-pool digest; the adapter presents them as three scoped temporary JSON files rather than embedding them in the provider prompt;
- AI result and SingingAnalysis artifacts retain mode-specific adapter, full-pool, selected-ID and response provenance plus preserved-revision-only reuse semantics;
- Settings, Processing Studio, Plan Preview, actionable errors, network/privacy disclosure and the generated user guide are synchronized across EN / zh-CN / ja.

Card 21 revision 6 re-audited this implementation against the approved design. The reserved whole-workspace/Nix/final packaged acceptance remains a later explicit release pass.

## 11. Acceptance

AI judgment is design-complete only when focused tests prove:

- Studio sends intent/resource identity, never an executable path;
- Runtime Manager configures, reports, and resolves the adapter tool;
- Algorithm remains the default and never silently becomes AI judgment;
- AI judgment never silently falls back to Algorithm;
- the adapter cannot fabricate or modify candidates;
- Algorithm and AI judgment receive the same normalized hard-boundary set and cannot gap voiced coverage or cross a hard boundary;
- timeout/cancel/non-zero/protocol failures are hard failures;
- Plan Preview is read-only and does not contact the provider;
- AI decision provenance is retained and deterministic-cache aliasing is impossible;
- network/data disclosure is visible;
- EN/zh-CN/ja copy is synchronized;
- current process-boundary and source-size gates remain clean.
