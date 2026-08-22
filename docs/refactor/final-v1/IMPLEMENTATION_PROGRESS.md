# Native inference refactor — implementation progress

Updated: 2026-08-22

This is the mutable progress record. The files ending in `FINAL` are the
checksum-verified implementation contract and are intentionally not edited.
A checked item here means implemented and covered by the stated evidence; it
does not replace the final hardware acceptance checklist.

## Complete

- [x] Final design package extracted under `docs/refactor/final-v1/` and hashes verified.
- [x] Baseline branch, commit, existing changes, API catalogue, Python files, and source-size hotspots audited.
- [x] Versioned Workflow domain with instance identity, typed ports, analyzer bindings, execution policy, priority, and layout/execution separation.
- [x] Capability registry covers audio roles, duplicate transformations, analyzers, transcript/evidence fusion, Candidate Graph, and Canonical Singing Track.
- [x] Workflow validation covers duplicate identity, unknown ports, type mismatch, hard dependencies, cycles, analyzer sources, conditionals, and terminal output.
- [x] Workflow compiler produces a real `AnalysisGraphSpec` and immutable execution snapshot; exact typed Artifact bindings, analyzer attachments, execution policy, priority, model/runtime resolution, and recipe digest are retained while layout is excluded from the execution digest.
- [x] Per-song workflow persistence and legacy audio-settings migration.
- [x] Generic workflow audio artifact metadata and compiled-node provenance.
- [x] Versioned native NDJSON protocol, exact coordinator/runtime-lock handshake, fail-closed router, runtime-lock parsing/digests, selected-component resolution from the compiled Workflow, bounded output validation, timeout, cancellation, crash handling, and clean worker teardown.
- [x] Canonical singing domain: sparse 10 ms Evidence Timeline, versioned score calibration, correlation/dependency discount, Transcript Fusion, word-boundary fusion, duration-aware segment decoding, Canonical Singing Track, uncertainty, and Review Regions.
- [x] Canonical Singing Track projects into the Editor's read-only evidence/review contract.
- [x] Processing Studio first-class route with save/validate/run, audio transformation reorder, Processing/Graph/Editor navigation, and typed UI command coverage.
- [x] Editor Evidence/Review/Suggestion/Artifact-source structures, undo-preserving suggestion acceptance, Candidate/Authored protection, and multi-artifact audition wiring.
- [x] Native Models & runtime status/setup semantics and explicit-download policy.
- [x] Zero tracked `.py`/`.pyi`; active Python/uv/venv runtime references removed.
- [x] API decisions recorded in `API_CHANGE_LOG.md`; API catalogue uniqueness test passes.
- [x] EN/zh-CN/ja key parity.
- [x] All app-owned source files are at or below 2000 lines.
- [x] Nix package definition includes the native analyzer, RMVPE OpenVINO worker, third-party notices, and canonical UI assets.
- [x] Native RMVPE implementation: packaged-FFmpeg decode, Rust 10 ms log-mel frontend, explicit source-verified ONNX→bucketed IR v11 installation, production IR-only OpenVINO GPU inference, continuous F0 evidence, overlap stitching, and CPU fail-closed behavior.
- [x] Separate Qwen ASR and Forced Aligner NDJSON Workers with pinned runtime/model manifests, Vulkan-only engine invocation, GPU-required aligner patch, bounded child output, parent-death cleanup, atomic Evidence JSON, and real Japanese speech/singing smokes.

## Verified gates

- [x] `cargo fmt --all -- --check`
- [x] `cargo check --workspace --all-targets --locked`
- [x] `cargo test --workspace --all-targets --locked`
- [x] `cargo clippy --workspace --all-targets --locked -- -D warnings`
- [x] `cargo xtask docs check`
- [x] Native analyzer NDJSON smoke.
- [x] Real UTZ and UltraStar smoke exports; UTZ metadata/hash inspection; exported audio decode.
- [x] Sustained native audio audition with PipeWire inspection.
- [x] `nix build path:.#uta-studio --print-build-logs`.
- [x] Wrapped Wayland launch with no Python/uv process.

The full Rust/Nix gates above passed before the latest singing-domain addition.
Focused core Workflow/Fusion/supervisor suites and Desktop UI API contract tests
have since passed after the latest additions. Full workspace and package gates
will be rerun at final handoff.

## In progress / not accepted

- [ ] Complete all model runtimes. RMVPE, FCPE, Basic Pitch, and full FireRed AED pass OpenVINO GPU smokes; both Qwen exceptions pass Vulkan Worker smokes; all five RoFormer candidates pass the documented conservative 12-second Vulkan matrix but remain non-production after historical sustained failures. GAME has no resolvable public model identity, and STARS' official forward contains non-exportable CPU Viterbi control flow; both require an explicit architecture/model decision.
- [ ] Qwen full-song singing-quality acceptance remains; the pinned ASR Worker short-speech smoke passes.
- [ ] Qwen complete-lyrics full-song alignment acceptance remains; the pinned Aligner Worker 12.8-second singing smoke passes.
- [ ] Native coordinator execution of the complete compiled Workflow. The packaged coordinator currently fails closed instead of creating synthetic outputs.
- [ ] Conditional expert scheduling over disagreement windows through the production scheduler.
- [ ] Real Chinese and non-Chinese end-to-end runs from Processing Studio through Canonical Singing Track, Editor, and both exporters.
- [ ] Final Intel Arc resource-contention, repeated-run, cancellation, teardown, and no-reset matrix.
- [ ] Third-party runtime source/license/notice packaging for every selected worker.

## Important status rule

A pinned recipe is not automatically `ProductionPinned`. Qwen recipes remain
benchmark candidates until app integration and full-song quality gates pass.
Missing or unaccepted native components fail closed; there is no Python, CPU,
or HTTP fallback.
