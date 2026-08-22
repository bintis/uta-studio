# Uta Studio Final Acceptance Checklist — v1.0 FINAL

此清单只在 Agent Guide Phase 15 执行。

## Repository
- [ ] Branch/diff reviewed.
- [ ] No unrelated user changes overwritten.
- [ ] All app-owned source files ≤ 2000 lines.
- [ ] Zero tracked `.py` / `.pyi`.
- [ ] No active Python/uv/venv references.
- [ ] API ledger complete.
- [ ] `API_CAPABILITIES` complete and unique.
- [ ] EN/zh-CN/ja i18n parity.
- [ ] Source media remains read-only.
- [ ] Explicit-download policy preserved.
- [ ] No HTTP control/inference server.

## Rust
- [ ] `cargo fmt --all -- --check`
- [ ] `cargo check --workspace --all-targets --locked`
- [ ] `cargo test --workspace --all-targets --locked`
- [ ] `cargo clippy --workspace --all-targets --locked -- -D warnings`
- [ ] `cargo xtask docs check`

## Native runtimes
- [ ] RoFormer build/test.
- [ ] Generic OpenVINO runtime build/test.
- [ ] Qwen3-ASR transcribe.cpp build/test.
- [ ] Qwen3 Forced Aligner predict-woo build/test.
- [ ] Runtime-lock identity visible in diagnostics.
- [ ] stdout NDJSON clean.
- [ ] cancel/crash/timeout handling.

## Package
- [ ] `nix build path:.#uta-studio --print-build-logs`
- [ ] wrapped Wayland smoke launch.
- [ ] no Python in process tree.
- [ ] licenses/notices packaged.

## Hardware
- [ ] Intel Arc RoFormer full-song stability.
- [ ] FireRed OpenVINO.
- [ ] Qwen ASR Vulkan pinned recipe.
- [ ] Qwen Aligner Vulkan pinned recipe.
- [ ] RMVPE OpenVINO.
- [ ] other production experts.
- [ ] repeated runs.
- [ ] cancellation.
- [ ] clean device/process teardown.
- [ ] no black screen/device reset.

## Workflow
- [ ] Processing Studio dynamic reorder.
- [ ] invalid type drop blocked.
- [ ] cycle blocked.
- [ ] duplicate processing node supported.
- [ ] Vocal/BGM lanes.
- [ ] Harmony branch.
- [ ] analyzer attaches to selected artifact.
- [ ] priority does not create dependency.
- [ ] conditional experts.
- [ ] compiled DAG matches workflow.
- [ ] Advanced Graph shows exact compiled graph.

## Editor
- [ ] Existing note/lyric functions preserved.
- [ ] Lead/Harmony/Backing/Adlib preserved.
- [ ] Candidate opens.
- [ ] Authored saves.
- [ ] upstream rerun does not overwrite Authored.
- [ ] Candidate/Authored compare/merge.
- [ ] Evidence layers read-only.
- [ ] Review Queue navigation.
- [ ] Suggestion accept undoable.
- [ ] Artifact playback/waveform picker.
- [ ] A/B audition.
- [ ] sustained native playback no xruns.

## End-to-end
- [ ] Chinese song.
- [ ] Non-Chinese song.
- [ ] Canonical Lyrics.
- [ ] Forced Alignment.
- [ ] Canonical Singing Track.
- [ ] ReviewRegions.
- [ ] Editor authoring.
- [ ] UTZ export.
- [ ] UltraStar export.
- [ ] exported audio decodes.
- [ ] temporary files cleaned.
