# Uta! Studio Fusion Agent Adapters

This crate provides the native `uta-fusion-agent-pi`,
`uta-fusion-agent-codex`, and `uta-fusion-agent-claude` executables. Each
adapter implements the bounded `uta.fusion_agent_request` /
`uta.fusion_agent_response` version 4 protocol owned by Analysis Engine.

The adapter receives a compact candidate decision projection, canonical lyrics,
and normalized hard boundaries only. For each invocation it writes those values
as `candidates.json`, `lyrics.json`, and `hard_boundaries.json` in a fresh
temporary working directory. The provider prompt contains only these relative
paths; the directory is removed on every exit path. Source audio, the Studio
library database, project files, and model files are never placed there.

Provider sessions, extensions, and context files are disabled where supported.
Only the provider's read-only file tool is enabled so it can read the three
scoped JSON files. Provider credentials and network policy remain owned by the
provider CLI.

The provider executable is found through the normal `PATH` at invocation time.
The sibling `.uta-fusion-adapter.json` files are protocol compatibility
metadata for Runtime Manager; they do not assert provider authentication or
publisher trust.
