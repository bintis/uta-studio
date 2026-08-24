# Non-GAME model and runtime readiness research pack

Date: 2026-08-22

This pack records durable upstream provenance and technical contracts for Uta Studio's non-GAME model families. Machine-local inventory snapshots, execution logs and handoff journals are intentionally not retained.

## Scope and authority

- GAME is excluded by task scope. Where the planner requires it, this pack says
  only: **GAME excluded from this research pack by task scope**.
- Research does not promote any resource or change Runtime Manager state.
- Repository manifests/current source are evidence only within the limits they establish; current readiness is summarized in `docs/KEY_CONCLUSIONS.md` and `tasks/remaining-models/STATE.md`.
- `SOURCE_LEDGER.md` records every external source used, retrieval date, source
  class, and supported facts. Source IDs such as `[Q1]` refer to that ledger.
- `KNOWN`, `MISSING`, `CONFLICT`, and `NOT APPLICABLE` are used literally. A
  local artifact can be present while its clean-install recipe or provenance is
  still missing.

## No-execution rule

This task performed documentation/metadata retrieval and read-only local file
inventory only. It did not execute inference, create a Vulkan/OpenVINO/CPU model
context, run a worker, convert a model, import/install/repair a resource,
download model weights, benchmark, build, or test code.

## How to use the pack

1. Use `P0_REQUIRED_MODELS.md` for route criticality and gap triage.
2. Use the family documents for exact provenance and technical contracts.
3. Use `SOURCE_LEDGER.md` when auditing an upstream claim or retrieving metadata.
4. Use `docs/KEY_CONCLUSIONS.md` plus current source/state for present implementation readiness.
5. Re-check mutable upstream `main` references before authoring a recipe; pin exact revisions rather than branch names.

## Files

- `P0_REQUIRED_MODELS.md` — compact P0 route/resource matrix.
- `ROFORMER.md` — separator/cleanup provenance and contracts.
- `QWEN.md` — ASR and Forced Aligner provenance/contracts.
- `RMVPE.md` — source ONNX and OpenVINO import contract research.
- `OPTIONAL_EXPERTS.md` — FireRed, FCPE, Basic Pitch, cleanup, STARS and ROSVOT research.
- `SOURCE_LEDGER.md` — primary/secondary external source ledger.

No resource is newly Production-ready as a result of this pack.
