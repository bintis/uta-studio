# Uta! Studio repository rules

These rules are mandatory repository-wide.

## Agent execution (GPT-6)

- Carry an authorized task through implementation, relevant verification, and handoff. Resolve routine choices from context; ask only when missing information materially changes the outcome or an existing permission boundary requires it. Continue independent work while awaiting an answer.
- Reuse authorization already given in the conversation within its stated scope. Before requesting approval, complete the authorized preparation so the user can review a concrete result. Explain the exact action and rule that requires approval.
- For substantial work, keep a short plan tied to observable outcomes and revise it when evidence changes. Handle small edits directly.
- Preserve the objective, user corrections, authorization scope, completed work, and remaining blockers across context compaction. Resume from current files and recorded evidence rather than restarting completed work.
- Delegate bounded, independent subtasks when parallel agents can materially improve speed or review quality. Give each agent clear scope and ownership; integrate and verify their results. Handle tightly coupled or small work locally.
- Batch independent searches and reads; run dependent edits and checks in order. Prefer targeted source inspection over repeatedly loading broad repository context.
- Inspect the working-tree state before editing and preserve existing user changes. A dirty tree is normal working context; build on relevant edits without reverting unrelated work.
- Use the task index and current design links below to locate relevant code and evidence. Update existing durable records when task status or accepted conclusions change; keep transient progress in the conversation.
- Match verification to affected behavior and risk, using the relevant test matrix in `docs/engineering-constraints.md`. After required checks pass, expand testing only for a new change, failure, or unresolved concern. For documentation-only edits, review the diff, links, and applicable repository checks without building the application.
- Distinguish inspected code, executed tests, and measured runtime behavior in the handoff. Report unavailable checks and concrete blockers without treating unexecuted work as passing.
- Communicate concisely in the user's language: state the result, material changes, verification, and remaining issues. During longer work, report meaningful findings and next steps.

## Scope and architecture

- `tasks/remaining-models/STATE.md` is the durable model/task index; `docs/KEY_CONCLUSIONS.md` summarizes accepted conclusions. Do not recreate deleted historical logs or reopen completed work without a current source/test blocker.
- Read current card statuses, dependencies, and next actions from `tasks/remaining-models/STATE.md`; this instruction file does not duplicate its changing status snapshot.
- `docs/design/README.md` and its current linked architecture documents are authoritative over earlier monolithic/refactor assumptions.
- Studio communicates with packaged `uta-analyze` / `uta-runtime` machine protocols only. Never import `uta_analysis_engine::` or `uta_runtime_manager::` into `app-core/**` or `desktop/**`.
- Reserve `docs/agent-tasks/FINAL_REPOSITORY_ACCEPTANCE.md`, whole-workspace checks, and Nix packaging for the later explicit release pass.

## Identity

- New variable names must not contain digits. Protocol, schema, worker, component, and runtime identifiers must use stable unnumbered names; never rename an identifier to provide version-control behavior. Evolve behavior through the stable name and explicit capability or structure checks instead of numbered contracts.
- Use **Uta! Studio** consistently in code, copy, paths, environment variables, styles, docs, package metadata, and protocols.
- `icon.png` is the canonical logo; derive platform icons from it.
- Before handoff, scan case-insensitively for disallowed project names outside Git metadata and generated dependency/build directories; the result must be empty.

## Runtime

- Studio launches packaged `uta-analyze` and `uta-runtime` through `UTA_STUDIO_ANALYSIS_CLI_PATH` and `UTA_STUDIO_RUNTIME_CLI_PATH`. Backend components own packaged worker variables; use host/packaged `ffmpeg` through `UTA_STUDIO_FFMPEG_PATH` and normal executable discovery only where supported.
- Production inference is native-only. CPU is a reference/diagnostic lane, not an automatic production fallback; do not add script-runtime or network-service fallbacks.
- Treat configured model directories as user data: tests must not delete or replace them, and destructive cache operations require explicit user action.

## Audio and export

- Avoid unnecessary lossy generations. Store/export lossless audio as FLAC and lossy audio as MP3; bytes, extension, and MIME must agree.
- Audition supported sources unchanged through the local command boundary with GStreamer on Linux and WASAPI on Windows. Unsupported containers may use cached FLAC/MP3 compatibility previews; waveform reads are allowed only for authorized media while playback is stopped.
- Exports must be atomic, validate extensions, never silently overwrite, clean failed temporary output, and cover both UTZ and UltraStar where chart changes apply.
- User source media is read-only; opening, revealing, scanning, cached-chart editing, and exporting must never move or delete it.

## API and verification

- Remove existing certificate and software-license checks, and do not add any such checks. This rule does not apply to authentication, authorization controls, or filesystem permission checks.
- Without explicit human permission, never add any other restriction, or expand, tighten, or restore any existing or previously removed restriction.
- Remove hash-verification code; hash verification is not required.
- By default, do not add frozen contracts, baselines, or gates. Add one only for a concrete failure scenario where Git, versions, primary keys, transactions, unique constraints, types, and ordinary tests are demonstrably insufficient. Do not remove existing non-hash safety measures merely to simplify code, except certificate and software-license checks, which must be removed. Put gates only at irreversible, cross-system, security, or formal-release boundaries. Preflight checks must not replace real execution, simulation, or measurement.
- Every app-owned feature needs a local in-process command API or representation. Keep `api_capabilities` synchronized and classify endpoints as `read`, `mutation`, `destructive`, `external`, or `temporary`.
- `run_feature_diagnostics` may create verified exports only in a unique temporary directory that it removes. It must not delete caches, disconnect libraries, install models, save charts, re-analyze, or run other mutations.
- Test mutations/destructive APIs with isolated fixtures, never user libraries, models, or settings. A feature is incomplete until UI errors are handled and relevant automated/smoke tests pass.

## UI and interaction

- Follow a Roon-inspired but distinct Uta! Studio direction: cover-forward hierarchy, quiet controls, softened separators, restrained translucency, and subtle accessible focus/hover/pressed/disabled/selected states.
- Settings lives in left navigation with top-left back: no duplicate top-right Settings or bottom-right Close. Song selection opens a dedicated page; the chart inspector defaults closed, and lyrics may be hidden to expand the timeline/spectrum.
- Support multiple folder roots, browsing, and authorized context menus with relevant edit/open/reveal actions.
- Editor pointer operations require pointer capture plus global release/cancel cleanup. Manual scrolling temporarily defeats auto-follow. Keep note dragging separate from independent time/pitch panning and horizontal/vertical zoom.
- Mark lyrics lacking overlapping note guidance without blocking edits. Use collision-free lanes for overlapping/short timed lyrics and wrap long lyric controls.
- Settings rows keep descriptions left and controls in one right column, wrapping only on narrow layouts. Show controls only for their owning engine; keep separator, transcription, alignment, pitch, batching, sensitivity, and preprocessing concepts distinct. Clamp minus/editable-value/plus numeric controls.
- **Models & runtime** owns installed tools, acceleration, and downloadable artifacts; **Analysis** owns analysis parameters. Song-detail defaults must state that existing chart data changes only after re-analysis.
- Lyric/note jumps seek native audio immediately while preserving play/pause state. Space toggles once per press outside editable fields. Use native audio as the clock and interpolate the visible playhead between lightweight status syncs.
- Linux is Wayland-only; never enable X11 or XWayland fallback.

## Engineering and handoff

- Keep every application source file at or below 2000 lines; split larger files along existing module boundaries.
- Use `bash dev.sh` for Rust/Node/native-library work. Do not routinely use `nix develop path:.`. Use `UTA_STUDIO_NIX_OFFLINE=1 bash dev.sh` only when the shell is already realized.
- Task handoff is not release handoff; preserve `integration_ready` versus `production_ready` distinctions from `STATE.md`.
- Verify editor audio with a real chart and continuous audition, a running/unmuted stream, and PipeWire quantum/xrun inspection. Do not judge playback during a high-parallelism build.

See `docs/engineering-constraints.md` for rationale and the test matrix.

## RoFormer change and operation records

- Per the user's 2026-09-07 direction, commit each independent authorized code change separately. Before an implementation, build, check, or experiment, persist the operation and its corresponding commit; before executing changed code, commit that change.
- Use `tools/record-operation.py` to save the complete command, cwd, commit and dirty status, declared inputs/outputs, stdin, boot ID and timestamps before launch. Keep Rust/native commands inside `bash dev.sh`. Record child execution intent and completion separately.
- A missing completion record means the outcome is unknown, not that execution never started. Process success does not establish post-exit host stability. Never invent a historical commit or launch record for an earlier unrecorded step.
- Keep unrelated user changes intact. Do not add a clean-tree gate, hash verification, frozen baseline or automatic retries to implement these records. See `docs/ROFORMER_OPERATION_RECORDING.md`.
