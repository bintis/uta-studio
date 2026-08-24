# Uta Studio repository rules

These rules are mandatory for AI coding agents and apply to the whole repository.

For the current analysis/runtime/Studio work, `docs/agent-tasks/CURRENT_AGENT_TASKS.md` is the task index. Exactly one coding task is active:

```text
docs/agent-tasks/TASK_ALL_MODEL_REPAIRS.md  # one agent completes every remaining model repair in one continuous task
```

All model work follows `docs/agent-tasks/MODEL_GPU_WORK_POLICY.md`. `docs/agent-tasks/TASK_ALL_MODEL_REPAIRS.md` is the active single-agent implementation task: the same agent repairs every unresolved model and updates `tasks/remaining-models/STATE.md` plus `docs/KEY_CONCLUSIONS.md` as durable state. Long-form completion/validation logs under `docs/` are intentionally not retained. Cards 15–21 and 20A must obey both `tasks/final-features/PROCESS_BOUNDARY_RULES.md` and `tasks/final-features/STUDIO_BACKEND_UI_PARITY.md`: Studio remains decoupled from backend implementation crates and communicates only through packaged CLI machine protocols.

Current durable conclusions are summarized in `docs/KEY_CONCLUSIONS.md`. In particular, RMVPE is ProductionPinned, GAME has an accepted technical Production path, and license restrictions are recorded but do not block technical Production when the exact model/checkpoint is available under an explicit open-source/open-model license. Optional experts remain non-substituting, and current source already contains Qwen schema-2 transcript/alignment consumers. Qwen3-ASR-1.7B and Qwen3 Forced Aligner remain pinned GGML/C++ exceptions and are not OpenVINO migration targets. The non-GAME research pack remains under `docs/research/non-game-model-readiness/**`.

Studio/backend final integration foundations are already present. Do not reopen broad historical passes merely because their logs were removed; create focused work only for a current source/test blocker. Post-model work must not solve Workflow/feature gaps by importing `uta_analysis_engine::` or `uta_runtime_manager::` into `app-core/**` or `desktop/**`.

Architecture/design authority starts at `docs/design/README.md`. The separated architecture and current Audio Analysis Framework named there supersede earlier monolithic/refactor-phase design assumptions; supporting integration/Editor/UI documents may refine their own domains but must not override those boundaries.

`docs/agent-tasks/FINAL_REPOSITORY_ACCEPTANCE.md` is reserved for a later explicit final integration/release pass. It must evaluate current source/tests, current task state and durable conclusions rather than require deleted historical log files.

## Product identity

- The project is named **Uta Studio**. Keep this identity consistent in code, copy, paths, environment variables, CSS, docs, package metadata, and protocols.
- Before handing off a change, run a case-insensitive repository scan for disallowed project-name terms. The result must be empty outside Git metadata and generated dependency/build folders.
- `icon.png` at the repository root is the canonical logo. Generated platform icons must derive from it.

## Runtime and downloads

- Prefer packaged native workers and host or packaged `ffmpeg` through `UTA_STUDIO_NATIVE_ANALYZER_PATH`, component-specific native worker variables, and `UTA_STUDIO_FFMPEG_PATH`, with normal executable discovery only where explicitly supported.
- Production inference is native-only. Non-Qwen calls that directly or indirectly create a Vulkan or Level Zero context require explicit user permission before execution. Qwen-family runtimes are exempt from that permission requirement. CUDA, HIP, Metal, OpenCL and other backends are not restricted by this repository GPU policy; OpenVINO is unrestricted unless the selected device path invokes Level Zero. Historical GPU incidents and archived stop decisions are evidence, not current execution gates.
- CPU is an explicit reference/diagnostic lane, never an automatic production fallback. Do not introduce a script-runtime or network-service fallback.
- Never download tools, packages, or AI models merely because the application launched, a page rendered, or a diagnostic ran.
- Model/runtime installation requires an explicit user action and confirmation in **Settings > Models & runtime**. When unavailable, analysis controls must be disabled and explain where setup lives.
- Existing configured model directories are user data. Do not delete or replace them in tests. Destructive cache operations require an explicit user action.

## Audio and export

- Do not introduce avoidable lossy generations. Lossless source or generated audio is stored/exported as FLAC; lossy audio is stored/exported as MP3.
- Never label lossy bytes as FLAC or a MIME type that does not match the actual payload.
- The editor auditions supported sources through the local in-process command boundary with native GStreamer playback on Linux and WASAPI playback on Windows. It plays the chosen source unchanged; unsupported containers may get a cached FLAC/MP3 compatibility preview following the rule above. Waveform visualization may read authorized local media separately while playback is stopped.
- Every export is atomic, validates its target extension, does not silently overwrite user files, and cleans failed temporary output.
- UTZ and UltraStar are both first-class outputs. Changes to chart authoring must be covered by both exporters where relevant.

## API and verification

- Every app-owned feature must have a local in-process command API or be represented by one. Keep `api_capabilities` synchronized with the operations exposed by the native desktop shell.
- Classify each endpoint as `read`, `mutation`, `destructive`, `external`, or `temporary`.
- `run_feature_diagnostics` must remain safe for user data. It may create verified exports only in a uniquely named temporary directory and must remove that directory. It must never run cache deletion, library disconnection, model installation, chart saves, re-analysis, or other destructive/mutating workflows.
- Mutation endpoints are tested through unit/contract tests and isolated fixtures. Do not prove that a destructive API works by using the user's actual library, models, or settings.
- A feature is not complete until its error path is handled in the UI and its relevant automated and smoke tests pass.

## UI and interaction

- The visual direction is Roon-inspired, not a pixel copy: clean information hierarchy, quiet controls, familiar navigation, cover-forward content, softened separators, and restrained translucent surfaces.
- Preserve Uta Studio's own identity through modern transparency, authoring-focused controls, and the canonical logo.
- Avoid bright focus boxes and loud hover fills. Focus, hover, pressed, disabled, and selected states must remain perceptible and accessible through subtle contrast, opacity, type weight, or a restrained indicator.
- Do not add a duplicate Settings control in the top-right. Settings uses the left navigation and the top-left back action; no bottom-right Close button.
- Song selection opens a dedicated route/page, not a permanent right-side detail pane. The chart inspector is optional and closed by default. Lyrics can be hidden so the timeline/spectrum expands.
- Folders support multiple roots, browsing, and authorized context-menu actions. Song and file context menus must expose the relevant edit/open/reveal actions.
- Editor pointer interactions must use pointer capture with global release/cancel cleanup. Manual scrolling temporarily wins over playhead auto-follow.
- The chart viewport must pan independently in time and pitch. Keep note dragging distinct from viewport panning, support mouse/trackpad time and pitch navigation, and allow horizontal and vertical zoom so users can reach pitches beyond the current notes.
- Lyrics without overlapping note guidance must be visibly marked without preventing editing.
- Settings rows keep their description on the left and align selects, switches, sliders, and actions to one consistent right-hand column. Only narrow layouts may wrap the whole control below the description.
- Show model-specific controls only while their owning engine is selected. Keep separator, transcription, alignment, and shared preprocessing concepts distinct. Bounded numeric model controls use minus / editable value / plus and clamp invalid input.
- **Models & runtime** owns installed tools, acceleration, and downloadable artifacts. **Analysis** owns separator, transcription, alignment, pitch, batching, and sensitivity parameters. Song detail may expose the same analysis defaults for convenient tuning, but must state that existing chart data changes only after re-analysis.
- Editor lyric and note jumps use an immediate accurate native-audio seek and preserve the current play/pause intent. Space toggles transport once per key press whenever focus is outside an editable field.
- Treat native audio position as the clock source, but interpolate the visible playhead on animation frames between lightweight status syncs. Timed lyrics with overlapping or very short ranges must use collision-free visual lanes; long lyric controls wrap instead of overlapping adjacent content.
- The Linux desktop is Wayland-only. Do not enable an X11 window backend or use XWayland as a fallback.

## File size

- A single source file (`.rs`, `.py`, or other application code) must stay at or under 2000 lines. Larger files must be split or refactored along existing module boundaries before handoff.

## Safety and completion checks

- Never expose an unauthenticated HTTP control server. Feature APIs stay inside the local process unless the user explicitly requests and approves a different security design.
- Keep user source media read-only. Opening, revealing, scanning, editing cached chart data, and exporting must never move or delete source songs.
- Use the repository's lightweight dev shell through `bash dev.sh` for Rust/Node/native-library work when needed. It uses the independently locked `nix/dev-shell` flake; do not use `nix develop path:.` for routine development because the mutable repository root (especially `target/`) must not be copied into the Nix store on every change. The pinned shell closure is stable across Git commit/dirty changes and reuses realized store paths. `UTA_STUDIO_NIX_OFFLINE=1 bash dev.sh` is only for an already-realized shell because Nix offline mode disables substituters and can otherwise force source builds. Nix environment realization is never a request to download models.
- Task handoff and repository-wide release handoff are different. The active repair agent must follow `docs/agent-tasks/TASK_ALL_MODEL_REPAIRS.md`, `docs/agent-tasks/MODEL_GPU_WORK_POLICY.md`, and `tasks/remaining-models/STATE.md`, persist results while continuing through the complete repair queue, and distinguish `integration_ready` from `production_ready`. Non-Qwen Vulkan/Level Zero execution requires explicit user permission; Qwen is exempt. Feature work must preserve Studio -> `uta-analyze` / `uta-runtime` process boundaries and must not reintroduce backend Cargo dependencies into Studio. Whole-workspace checks and Nix packaging remain reserved for `docs/agent-tasks/FINAL_REPOSITORY_ACCEPTANCE.md`.
- Editor audio is not verified merely because a PipeWire stream exists. Audition a real chart continuously, confirm the stream is running/unmuted, inspect PipeWire quantum errors/xruns, and keep waveform/timeline rendering from blocking playback. Do not judge playback while a high-parallelism build is saturating the machine.
- The final packaged artifact must be produced by `nix build path:.#uta-studio` and smoke-launched from its wrapped executable.

See `docs/engineering-constraints.md` for the human-readable rationale and test matrix.
