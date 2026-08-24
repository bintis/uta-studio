# 21A — Analysis CLI-Only Product Execution Closure

**State:** `READY`
**Parent:** card 21 final-v1 design parity audit
**Task class:** focused app-core/Desktop process-boundary closure; no model inference

## Gap

The exact Plan Preview path queues `EngineQueueIntent` and executes through `AnalysisCliClient`, but other reachable product actions still call `enqueue_one` / `enqueue_all` without that intent. `spawn_worker` then falls through to `process_song`, which launches `uta-native-analyzer` through `analyzer/server.rs`. Current examples include Analyze all, auto-analyze after scan, re-analysis actions and granular node/freeze/bypass/configure actions.

This violates the frozen final boundary:

```text
Desktop -> app-core -> AnalysisCliClient -> uta-analyze
```

It also makes API entries such as `run_analysis_request` claim exact execution while using the legacy loose protocol.

## Scope

1. Inventory every non-test product caller that can enqueue or start analysis.
2. For behavior representable by the current Engine contract, compile and persist an exact request/Workflow snapshot, obtain exact Preview/Plan identity and execute only through `AnalysisCliClient`.
3. For legacy Freeze/Bypass/granular behavior not representable by Engine v1, disable or retire the action with an explicit typed explanation; never route it to the compatibility analyzer.
4. Make Analyze all and auto-analyze create independently validated exact per-song intents, preserving request-specific blockers without installing resources or silently changing outputs/policy.
5. Ensure the production queue has no fall-through path to `process_song` / `uta-native-analyzer`. Legacy code may remain only if statically unreachable from product/API entry points pending separate deletion.
6. Synchronize `api_capabilities`, typed UI APIs, error handling and history/cancellation semantics.
7. Keep Desktop free of direct CLI launch and keep app-core free of backend implementation dependencies/imports.

## Focused acceptance

- Static call-graph/namespace tests prove every reachable analysis action queues an exact Engine intent or fails explicitly before enqueue.
- Real `uta-analyze` process fixtures cover Analyze song, Analyze all/auto queue projection, re-analysis and supported node actions.
- Unsupported legacy controls never spawn `uta-native-analyzer` and never claim success.
- Preview snapshot/digest equals queued execution snapshot/digest.
- Cancellation reaps the active Engine process group and publishes no partial artifact.
- No model inference, accelerator context, download or Nix build.
- Rerun card 20 process/control-plane and cancellation bubbles, then rerun card 21.

## Completion

**Result:** `READY`

All reachable product analysis starts now create a unique exact Engine request, validate it through `uta-analyze`, persist its request/Plan snapshot as `EngineQueueIntent`, and execute through `AnalysisCliClient`. This covers Analyze song, Analyze all, scan/startup auto-analysis, transcript/alignment/pitch/full re-analysis, force transcription, and saved-lyrics realignment. Per-song bulk blockers are retained as failed queue rows with their exact Preview error.

The production worker no longer falls through when an intent is absent: it marks the queue/history entry failed and requires a new Plan Preview. Startup recovery applies the same rule. Active cancellation uses the request-correlated Engine cancel handle; the shared compatibility analyzer server has no production caller and its module plus `process_song` are test-gated only.

Arbitrary node-only/downstream execution, Disable, Freeze, Bypass, one-run node configuration, and preprocessed-audio capture controls were removed from Desktop commands, dialogs, typed UI APIs, and the app-core API catalogue because Engine v1 cannot represent them exactly. Supported node menu actions map only to typed Engine artifact targets; unsupported stem/preprocessing nodes expose no execution action.

### Evidence

- `uta-studio-core`: **467 passed**, including real `uta-analyze` ready/validate/requirements/plan, Workflow projection, quantization projection, protocol pollution/correlation/size/exit failure, request-correlated cancellation, exact snapshot/digest persistence, unique automatic request IDs, retired API assertions, and missing-intent fail-closed queue coverage.
- `uta-studio-desktop`: **238 passed**, including typed UI reachability/dispatch and exact node-menu action inventory.
- `uta-analysis-engine`: **125 passed, 2 ignored** (the ignored tests require explicitly authorized local advanced-note inference); cancellation kills/reaps stalled decoder and worker groups and publishes no partial Candidate artifact.
- Standalone `uta-analyze` CLI tests: **2 passed**; stdout remained correlated machine-only NDJSON.
- `cargo check -p uta-studio-core -p uta-studio-desktop --locked` passed without warnings after the final cleanup.
- Process-boundary scans found no backend implementation imports/dependencies and no Desktop direct CLI launch. No application source exceeded 2000 lines; `git diff --check` passed; no Worker/ffmpeg process remained.

No model inference, model download, accelerator context, Vulkan/OpenVINO execution, Nix build, or whole-workspace release pass was run.
