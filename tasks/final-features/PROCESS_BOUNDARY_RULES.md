# Final Feature Closure — Process Boundary Rules

These rules apply to every post-model feature card under `tasks/final-features/`.

## Frozen ownership

```text
Studio / app-core / desktop
  owns user intent, local source authorization, queue/history, local artifact revisions,
  review/editor/export UX, workflow editing, and local wire DTOs.

uta-analyze + Analysis Engine
  owns request validation, Engine Plan, workflow execution semantics, analysis algorithms,
  worker orchestration, fusion/candidate/finalization, typed analysis artifacts, and cancellation.

uta-runtime + Runtime Manager
  owns resource catalog, acquisition/import, immutable generations, integrity, runtime/backend
  policy, validation state, usability, repair/remove/status/resolve.
```

Studio/backend communication is process-only:

```text
Desktop
  -> app-core
      -> AnalysisCliClient
          -> uta-analyze worker --stdio-json
              -> Analysis Engine
      -> RuntimeCliClient
          -> uta-runtime ... --output json/ndjson
              -> Runtime Manager
```

## Absolute decoupling gates

Final Studio must not Cargo-link:

```text
uta-analysis-engine
uta-runtime-manager
```

Final `app-core/**` and `desktop/**` must contain zero implementation imports:

```text
uta_analysis_engine::
uta_runtime_manager::
```

Desktop must not launch backend CLIs directly. It calls app-core APIs only.

Studio must not copy Engine planner/fusion/scheduler logic or Runtime Manager lifecycle/policy logic. It may define small local versioned wire DTOs and translate product-domain state into requests.

If a backend contract changes, keep each side independently owned:

```text
app-core local wire DTO
  <--- versioned JSON/NDJSON --->
uta-analyze / Analysis Engine backend DTO
```

Do not solve schema sharing by adding a Studio dependency on an implementation crate.

## Machine protocol

- stdout is machine-only JSON/NDJSON.
- stderr is human diagnostics/logs only; never parse it as lifecycle truth.
- request IDs and protocol versions are validated.
- bounded frames and output paths are validated.
- cancellation is typed and must reap the actual worker/process group.
- Preview/Plan and execution must refer to the same serialized request/workflow snapshot and digest.

Canonical executable environment variables remain:

```text
UTA_STUDIO_ANALYSIS_CLI_PATH
UTA_STUDIO_RUNTIME_CLI_PATH
```

Do not introduce a new in-process backend seam, HTTP inference/control service, Python runtime fallback, or automatic CPU model fallback.

## Artifact authority

- Engine-generated VocalChart is Candidate authority only.
- Studio authoring creates/updates Authored revisions.
- Re-analysis never silently overwrites Authored.
- continuous F0 remains evidence and is never silently converted into target MIDI notes.
- evidence layers are read-only to authoring unless an explicit suggestion/merge command creates an Authored edit.
- source media remains read-only.

## Export ownership

UTZ / UltraStar and user-facing representation export remain Studio/app-core responsibilities. Do not move product export into Analysis Engine merely because `AnalysisEngine::export()` contains a placeholder. Engine owns typed analysis artifacts; Studio owns user export workflows.

## Runtime ownership

Feature cards may query resource state only through `uta-runtime`/Runtime Manager backend code as appropriate. Studio must not implement hashing, generation publication, model acquisition, repair, or validation-policy decisions.

## Safety

Non-Qwen Vulkan or Level Zero calls require explicit user permission. Qwen is exempt; other accelerator calls have no repository GPU restriction.

## Required decoupling acceptance for every feature card

At handoff, the implementing agent must verify or explicitly preserve:

```text
no new uta-analysis-engine dependency under app-core/desktop
no new uta-runtime-manager dependency under app-core/desktop
zero uta_analysis_engine:: under app-core/desktop
zero uta_runtime_manager:: under app-core/desktop
Desktop -> app-core only
Studio backend calls use AnalysisCliClient / RuntimeCliClient
stdout machine protocol remains clean
```

Do not run whole-repository/Nix final acceptance from an individual feature card unless that card explicitly says so.
