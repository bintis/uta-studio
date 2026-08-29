# 21E — AI Judgment Fusion Architecture Closure

**State:** `READY`

**Parent:** card 21 final design-parity audit

**Task class:** source/test/documentation convergence; no model inference required by default
**Authority:** `docs/design/audio-analysis/UTA_AI_JUDGMENT_FUSION_MODE_v1.0.md`

## Mission

Converge the current AI-judgment fusion prototype onto the approved architecture without removing the feature or weakening existing fail-closed/evidence guarantees.

The final product supports explicit Stage 4 `Algorithm` and `AI judgment` modes. AI judgment may use a networked provider, but the AI may only select from Engine-produced real candidates. Runtime Manager owns the Fusion Agent Adapter executable/resource path.

## A. Runtime Manager tool ownership

- [x] Add canonical `tool:fusion_agent_adapter` catalog/status support.
- [x] Add explicit external-tool path configure/clear persistence owned by Runtime Manager.
- [x] Resolve/validate the executable through Runtime Manager; expose usable/missing/unusable status.
- [x] Keep any environment override inside Runtime Manager discovery, not Engine.
- [x] Add focused Runtime Manager CLI/API tests for configure/status/resolve/clear and non-executable paths.

## B. Remove raw path ownership from Studio/AnalyzeRequest

- [x] Remove `fusion_agent_executable` raw path from Studio-owned analysis intent/draft wire.
- [x] Remove the raw adapter `PathBuf` from `AnalyzeRequest` execution policy.
- [x] Keep only typed `fusion_mode` and stable tool/resource identity where needed.
- [x] Ensure Preview/queue identity carries exactly the same mode/resource intent as execution.
- [x] Add process-boundary tests proving no Studio-authored backend executable path crosses to Engine.

## C. Engine adapter resolution and hard-fail behavior

- [x] Engine obtains the Runtime Manager-resolved Fusion Agent Adapter before AI-judgment execution.
- [x] Preserve direct process launch, bounded stdin/stdout, timeout, cancellation and process-tree cleanup.
- [x] Preserve strict verbatim candidate-membership validation.
- [x] Preserve canonical final-track validation.
- [x] Remove all silent or direct Engine-side Algorithm/path fallback behavior.
- [x] Add hard-failure tests for unresolved adapter, spawn, timeout, cancellation, non-zero exit, malformed/oversized response, fabricated candidate and invalid final path.

## D. Provenance and cache semantics

- [x] Record `decision_mode=ai_judgment`.
- [x] Record `adapter_resource=tool:fusion_agent_adapter` and adapter protocol version.
- [x] Retain resolved adapter identity/version when Runtime Manager provides it.
- [x] Record candidate-set digest, selected candidate IDs and adapter-response digest.
- [x] Do not request/store chain-of-thought.
- [x] Include mode/stable adapter identity in request/workflow identity so Algorithm and AI judgment never alias.
- [x] Prevent implicit deterministic-cache reuse of a fresh AI decision; only explicitly preserved prior analysis/artifact revisions may reuse an old AI decision.
- [x] Add repeat tests showing two valid different AI selections cannot be conflated as the same deterministic decision artifact.

## E. Network/privacy contract

- [x] Keep the v1 adapter payload limited to candidate/fusion metadata required for selection.
- [x] Do not send source audio bytes, arbitrary project files, library DB content, model files or unrelated user content.
- [x] Keep provider credentials outside argv and analysis artifacts.
- [x] Document that the configured external adapter may use the network and executes with the user's OS permissions.

## F. Product UX and localization

- [x] Models & runtime owns Fusion Agent Adapter status, choose and clear actions.
- [x] Processing Studio Stage 4 keeps Algorithm default and exposes AI judgment only when the adapter is usable.
- [x] AI judgment shows concise external-provider/candidate-metadata disclosure.
- [x] Plan Preview shows exact decision mode plus resolved adapter resource/readiness and never invokes the provider.
- [x] Add actionable missing/unusable/protocol/timeout/cancel failure copy.
- [x] Synchronize EN / zh-CN / ja catalogs.
- [x] Update canonical user-guide sources and regenerate `docs/USER_GUIDE.md` plus `desktop/assets/docs/docs.bundle.json`.

## G. Verification

Run focused non-inference checks:

```text
bash dev.sh -c cargo test -p uta-runtime-manager
bash dev.sh -c cargo test -p uta-analysis-engine
bash dev.sh -c cargo test -p uta-studio-core
bash dev.sh -c cargo test -p uta-studio-desktop
cargo xtask docs check
cargo fmt --all -- --check
git diff --check (excluding retained test evidence only where explicitly justified)
```

Also verify:

```text
no uta_analysis_engine:: / uta_runtime_manager:: imports under app-core/desktop
no raw fusion-agent executable path in Studio-owned workflow or AnalyzeRequest wire
all changed application Rust files <= 2000 lines
AI judgment failure never executes Algorithm
Preview never starts the adapter/provider
```

## Verification outcome — 2026-08-28

Runtime Manager owns and validates `tool:fusion_agent_adapter`; Studio and `AnalyzeRequest` carry typed mode/resource intent only. Engine request/response I/O is bounded and supervised under timeout/cancellation, both selector paths share final path validation, AI decisions retain adapter/candidate/selection/response provenance with preserved-revision-only reuse, and every failure remains fail-closed without Algorithm fallback. Preview/readiness/privacy/error copy is synchronized across EN / zh-CN / ja and the generated user guide.

Current focused suites pass for Runtime Manager, Analysis Engine, app-core and Desktop; process-boundary/raw-path/source-size/format/docs checks are clean. The later Card 21 revision-6 audit found and closed separate Export/Editor follow-ups 21F–21H without reopening this AI convergence result.

## Ready condition

Set 21E to `READY` only when sections A–G are complete and current source/tests match `UTA_AI_JUDGMENT_FUSION_MODE_v1.0.md`. Then rerun card 21 as a new audit revision and update `tasks/remaining-models/STATE.md` plus `docs/KEY_CONCLUSIONS.md`.
