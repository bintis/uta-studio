# Changelog

## 0.2.1 — 2026-08-15

迭代聚焦于“可直接编辑 UTZ 0.2 + 多轨能力 + 编辑体验细化”。

- 采用 UTZ 0.2 人声图为主编辑模型，支持直接编辑 UTZ 0.2 曲目数据。
- 重构编辑器到统一动作注册表，统一命令路径，补齐可读的撤销步命名与最近编辑展示。
- 新增编辑能力：逐拍点按计时、音调试听、整行重录、按语言拆分音节、问题位置信息回报，以及多轨（含 duet）曲绘能力。
- 导出端新增“由 vocal chart 构建 UltraStar 多轨 Duet”能力，并将分析音高证据打包到导出结果中。
- 分析链路补齐 MMS/Karaoke 对齐参数与文案说明，优化后端分析脚本结构。
- 持续拆分桌面前端为路由模块，补强工程结构；Windows 发布流程补齐 toolchain 锁定行为。

## 0.2.0 — 2026-08-14

Native desktop refactor and authoring-workflow restoration.

- Replaced the legacy web/Tauri shell with a pure Wayland Rust/Bevy desktop UI.
- changelog: Bevy/Tauri UI refresh
- Restored library covers, search, activity and analysis views, song pages,
  settings controls, version information, and contextual file actions.
- Rebuilt the editor with native GStreamer audition, waveform and pitch guides,
  direct lyric editing, multi-selection, resizing, note operations, and safe
  UTZ/UltraStar authoring.
- Added a Roon-inspired library transport with queue, previous/next, shuffle,
  repeat, seeking, volume, and unchanged-source playback.
- Added collision-safe batch export for every authoring-ready chart.
- Added an optional Japanese MMS Karaoke forced-alignment backend with
  FA-Kara-style pronunciation mapping, silence-aware timing and explicit model
  installation/licensing confirmation.
- Added a GitHub Actions release workflow that publishes a self-contained
  x86_64 Linux binary, DEB and RPM packages, plus a Windows x86_64 ZIP.
- The Windows build provides the native Bevy authoring UI, but editor audio
  audition remains Linux-only in 0.2.0.
- Improved narrow-window wrapping, card/side-navigation clipping, title-bar
  integration, canonical branding, and light/dark visual hierarchy.

## 0.1.0 — 2026-08-13

Initial Uta Studio release.

- Local music-library browsing with multiple folders and contextual actions.
- Configurable analysis pipeline with explicit runtime and model setup.
- Dedicated chart editor with native audio audition, waveform, lyric timing,
  pitch-note authoring, collision-free lyric lanes, and smooth playhead motion.
- Atomic UTZ and UltraStar export with configurable output locations.
- Native Bevy command API catalogue and safe feature diagnostics.
- Nix package for the Linux desktop application.
