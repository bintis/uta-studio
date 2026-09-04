<div align="center">
  <img src="icon.png" alt="Uta! Studio" width="128" />
  <h1>Uta! Studio</h1>

  **[English](#english)** | **[中文](#中文)** | **[日本語](#日本語)**

  [![License: CC BY-NC-ND 4.0](https://img.shields.io/badge/UI%20License-CC%20BY--NC--ND%204.0-lightgrey.svg)](LICENSE)
  [![License: AGPL-3.0](https://img.shields.io/badge/Algorithm%20License-AGPL--3.0-blue.svg)](LICENSE)
  [![Platform](https://img.shields.io/badge/Platform-Linux%20%7C%20Windows-green.svg)](#)
  [![Rust](https://img.shields.io/badge/Built%20with-Rust%20%2B%20Bevy-orange.svg)](#)
</div>

---

# English

## Why Uta! Studio?

There are already many AI-powered automatic music generation tools out there — but almost all of them depend on **Python** and **CUDA**. If you're on an Intel GPU (XPU), you'll run into driver bugs and black-screen issues that make them unusable.

Uta! Studio was built from scratch to break free from both Python and CUDA. By adopting the **latest SOTA models** (as of September 2026) and **rewriting the inference engine in native code** (Rust + GGML/Vulkan + OpenVINO), we've created an automatic karaoke chart authoring tool that:

- ✅ **No Python required** — fully native Rust application, zero Python runtime dependency
- ✅ **No CUDA required** — runs on Vulkan and OpenVINO, works on Intel/AMD/NVIDIA GPUs alike
- ✅ **No Intel XPU black-screen** — sidestepped the driver bugs entirely through native Vulkan compute
- ✅ **One-click karaoke generation** — a brand-new algorithm pipeline designed so that even people with zero musical training can generate playable karaoke charts from any song with AI assistance

> Cannot guarantee the accuracy of generated outputs (though I'm trying my best) — it's just a fun personal toy for self-entertainment!

## Features

### 🎵 AI-Powered Analysis Pipeline
- **Vocal separation** — AI-based stem separation isolating lead vocals and instrumentals (BSRoformer via GGML/Vulkan)
- **Speech recognition** — Qwen3-ASR 1.7B for multilingual lyric transcription
- **Forced alignment** — Qwen3-ForcedAligner 0.6B for word-level timing (11 languages: zh, en, ja, ko, fr, de, it, pt, ru, es, yue)
- **Pitch detection** — RMVPE for precise fundamental frequency extraction
- **Note segmentation** — GAME-based semantic note-region analysis with DSP post-processing
- **Expert fusion** — multi-expert candidate system with deterministic Algorithm fusion or optional AI-judgment mode

### 🎤 Chart Editor
- Decoded waveform display with pitch trace overlay
- Multi-note marquee selection, group move/transpose/resize
- Note split, merge, and clipboard operations
- Configurable quantization grid with Live Tap Timing mode
- Phrase and word boundary editing
- UltraStar note types (normal, golden, freestyle, rap, golden rap)
- Multi-track & duet authoring (Lead, Harmony, Backing, Adlib roles with automatic P1/P2 duet derivation)
- Intelligent language syllabization (Japanese moraic kana, Chinese Han, Korean Hangul, Latin vowel-group)
- Instant A/B audition between stems (e.g. Guide Vocals vs. Original Mix) without resetting playback
- Synthesized pitch preview — audible sine/harmonic tones to verify note pitches against vocal track
- Global gap correction
- Chart-issue inspector with conservative automatic timing repairs
- Full named undo/redo history

### 🔍 Lyrics Workbench
- Concurrent multi-provider online lyrics search (LRCLIB, QQ Music, Kugou, NetEase Cloud Music)
- Support for timed LRC, plain text, translations, and romanization
- In-place normalization, timestamp stripping, and one-click Save + Align

### 📦 Export Formats
- **UTZ** (`.utz`) — self-contained package for compatible karaoke runtimes
- **UltraStar** (`.txt`) — UTF-8 UltraStar 1.1 format with sibling media files, multi-track duet support
- Batch export mode for entire libraries
- CLI export tool for automation (`uta-studio-export`)

### 🖥️ Platform Support
- **Linux** — native Wayland, GStreamer + PipeWire/Pulse audio
- **Windows 10/11 x86-64** — portable ZIP, WASAPI audio output
- FLAC, MP3, WAV, Ogg/Vorbis, AAC/MP4 input format support

### 🧠 Native Inference (No Python!)
- **GGML + Vulkan** — GPU-accelerated inference for RoFormer, RMVPE, and Qwen models
- **OpenVINO 2026.3** — GPU with manifest-pinned CPU islands
- CPU as diagnostic/reference lane only (not an automatic fallback)
- All models managed through **Settings > Models & runtime** with explicit user confirmation

### 🎼 Processing & Workflow
- Visual DAG pipeline with node-card representation and drag-to-reorder
- Pre-flight plan preview with validity checking before analysis
- Immutable artifact revisions with pin/compare/diff/selective merge
- Durable processing queue with reorder, cancel, and rerun
- Per-run isolated JSONL analysis logs with live tailing viewer

### 🌍 Internationalization
- Interface languages: English, Simplified Chinese, Japanese
- Select in **Settings > General > Interface language**

### 🎨 UI Design
- Cover-forward visual hierarchy and clean layout
- Dark mode & light mode theme switching
- Multi-folder library with cover art browsing and metadata editing
- Offline documentation center with full-text CJK search and context-sensitive F1 help
- Context menus with edit/open/reveal actions
- Settings with left navigation and organized categories

## Build

```sh
bash dev.sh
cargo xtask docs check
cargo test --workspace
cargo check --workspace
```

Run the desktop app:

```sh
cargo desktop dev
```

Build a release binary:

```sh
./build.sh
```

Release packages: `nix build path:.#uta-studio`

The Linux desktop uses Wayland directly. Uta! Studio does not enable an X11 backend and does not fall back to XWayland.

## Runtime Manager CLI

`uta-runtime` is the scriptable frontend to the Runtime Manager library:

```sh
uta-runtime list --output json
uta-runtime status --check
uta-runtime plan model:qwen3_asr_1_7b --policy benchmark
uta-runtime verify --output json
```

Mutations require explicit confirmation (`--yes` for non-interactive use).

## TODO

1. Fix editor flicker while scrolling with the mouse wheel.
2. Update model download and removal capabilities in Settings.
3. Add automatic download capability for GGUF models.
4. Add capability to upload user-customized/edited models to Hugging Face.
5. Add a super-efficiency mode that runs GPU, integrated GPU, and CPU inference concurrently.
6. Fix slight timing offsets in generated MIDI.
7. Improve lyric download capabilities.

## Documentation

- **[User Guide](docs/USER_GUIDE.md)** — installation, setup, analysis, editing, export
- **[Architecture](docs/design/README.md)** — system design and component boundaries
- **[I18N Guide](docs/I18N.md)** — locale resolution and catalog maintenance
- **[Engineering Constraints](docs/engineering-constraints.md)** — product rules and test matrix

---

# 中文

## 为什么做 Uta! Studio？

现在有很多人开发了 AI 自动生成音乐的软件，但是很可惜，它们几乎全部都依赖 **Python** 和 **CUDA**。如果你用的是 Intel 的 GPU（XPU），会遇到驱动 bug 和黑屏问题，根本没法用。

Uta! Studio 趁此机会，从零开始开发，彻底摆脱了 Python 和 CUDA 的束缚。通过采用**最新的 SOTA 模型**（截至 2026 年 9 月）并**用原生代码重写推理引擎**（Rust + GGML/Vulkan + OpenVINO），我们实现了一款全新的自动卡拉 OK 创作软件：

- ✅ **不需要 Python** — 纯原生 Rust 应用，零 Python 运行时依赖
- ✅ **不需要 CUDA** — 基于 Vulkan 和 OpenVINO 运行，Intel/AMD/NVIDIA 显卡通吃
- ✅ **Intel XPU 不会黑屏** — 通过原生 Vulkan 计算完全绕开了驱动 bug
- ✅ **一键生成卡拉 OK** — 全新算法流水线，让完全不懂音乐的人也能依靠 AI，从任意歌曲自动生成可播放的卡拉 OK 曲谱

> 无法保证产物的准确性（虽然我在尽量做了），纯粹当个个人自娱自乐的小玩具还是挺好玩的！

## 功能特性

### 🎵 AI 驱动的分析流水线
- **人声分离** — 基于 AI 的音轨分离，隔离主唱和伴奏（BSRoformer，GGML/Vulkan 加速）
- **语音识别** — Qwen3-ASR 1.7B 多语言歌词转写
- **强制对齐** — Qwen3-ForcedAligner 0.6B 词级时间对齐（支持 11 种语言：中、英、日、韩、法、德、意、葡、俄、西、粤）
- **音高检测** — RMVPE 精准基频提取
- **音符分割** — 基于 GAME 的语义音符区域分析 + DSP 后处理
- **专家融合** — 多专家候选系统，支持确定性算法融合或可选的 AI 判断模式

### 🎤 曲谱编辑器
- 解码波形显示 + 音高轨迹叠加
- 多音符框选、批量移动/移调/调整大小
- 音符拆分、合并、剪贴板操作
- 可配置的量化网格 + 实时敲击定时模式
- 乐句和词边界编辑
- UltraStar 音符类型（普通、金色、自由、说唱、金色说唱）
- 多轨道 & 二重唱创作（主唱、和声、伴唱、即兴角色，自动 P1/P2 二重唱派生）
- 智能语言音节化（日语假名拍分割、中文汉字、韩语音节块、拉丁语元音组）
- 即时 A/B 试听切换（如主唱 vs. 原始混音），不中断播放
- 合成音高预览 — 可听的正弦/谐波音调，用于对照人声验证音符音高
- 全局间隙校正
- 曲谱问题检查器 + 保守自动时序修复
- 完整的命名式撤销/重做历史

### 🔍 歌词工作台
- 并发多源在线歌词搜索（LRCLIB、QQ 音乐、酷狗、网易云音乐）
- 支持定时 LRC、纯文本、翻译和罗马音
- 就地规范化、时间戳剥离、一键保存 + 对齐

### 📦 导出格式
- **UTZ**（`.utz`）— 自包含的卡拉 OK 运行时包
- **UltraStar**（`.txt`）— UTF-8 UltraStar 1.1 格式 + 同级媒体文件，支持多轨二重唱
- 批量导出模式，可处理整个曲库
- CLI 导出工具用于自动化（`uta-studio-export`）

### 🖥️ 平台支持
- **Linux** — 原生 Wayland，GStreamer + PipeWire/Pulse 音频
- **Windows 10/11 x86-64** — 便携 ZIP 包，WASAPI 音频输出
- 支持 FLAC、MP3、WAV、Ogg/Vorbis、AAC/MP4 输入格式

### 🧠 原生推理引擎（告别 Python！）
- **GGML + Vulkan** — GPU 加速推理，支持 RoFormer、RMVPE 和 Qwen 模型
- **OpenVINO 2026.3** — GPU 运行 + 清单锁定的 CPU 岛
- CPU 仅作为诊断/参考通道（不会自动回退）
- 所有模型通过 **设置 > 模型与运行时** 管理，需用户明确确认

### 🎼 处理与工作流
- 可视化 DAG 流水线，节点卡片表示，支持拖拽排序
- 分析前预检计划预览，含有效性检查
- 不可变产物修订版本，支持固定/比较/差异/选择性合并
- 持久化处理队列，支持重排/取消/重试
- 每次运行独立的 JSONL 分析日志，支持实时追踪查看

### 🌍 国际化
- 界面语言：英语、简体中文、日语
- 在 **设置 > 通用 > 界面语言** 中选择

### 🎨 界面设计
- 封面优先的视觉层级与整洁布局
- 深色/浅色模式主题切换
- 多文件夹曲库 + 封面浏览 + 元数据编辑
- 内置离线文档中心，支持 CJK 全文搜索和 F1 上下文帮助
- 右键菜单支持编辑/打开/定位操作
- 设置页面左侧导航，分类清晰

## 构建

```sh
bash dev.sh
cargo xtask docs check
cargo test --workspace
cargo check --workspace
```

运行桌面应用：

```sh
cargo desktop dev
```

构建发布版：

```sh
./build.sh
```

发布包：`nix build path:.#uta-studio`

Linux 桌面使用原生 Wayland。Uta! Studio 不启用 X11 后端，不回退到 XWayland。

## 运行时管理 CLI

`uta-runtime` 是运行时管理库的脚本化前端：

```sh
uta-runtime list --output json
uta-runtime status --check
uta-runtime plan model:qwen3_asr_1_7b --policy benchmark
uta-runtime verify --output json
```

变更操作需要明确确认（非交互模式使用 `--yes`）。

## 待办事项

1. 修复鼠标滚轮滚动时编辑器闪烁问题。
2. 更新设置中的模型下载和删除功能。
3. 增加 GGUF 模型的自动下载能力。
4. 增加上传用户编辑/微调后的模型至 Hugging Face 的功能。
5. 添加超级效率模式，同时运行 GPU、集成显卡和 CPU 推理。
6. 修复生成的 MIDI 中的轻微时序偏移。
7. 改进歌词下载功能。

## 文档

- **[用户指南](docs/USER_GUIDE.md)** — 安装、设置、分析、编辑、导出
- **[架构设计](docs/design/README.md)** — 系统设计和组件边界
- **[国际化指南](docs/I18N.md)** — 语言环境解析和目录维护
- **[工程约束](docs/engineering-constraints.md)** — 产品规则和测试矩阵

---

# 日本語

## なぜ Uta! Studio を作ったのか？

現在、AIによる自動音楽生成ソフトウェアを開発している人は多くいますが、残念ながらそのほぼ全てが **Python** と **CUDA** に依存しています。Intel GPU（XPU）を使っている場合、ドライバーのバグやブラックスクリーン問題に遭遇し、まともに使えません。

Uta! Studio はこの機会を活かし、ゼロから開発することで Python と CUDA の束縛から完全に脱却しました。**最新の SOTA モデル**（2026年9月時点）を採用し、**推論エンジンをネイティブコードで書き直す**（Rust + GGML/Vulkan + OpenVINO）ことで、まったく新しい自動カラオケ制作ソフトウェアを実現しました：

- ✅ **Python 不要** — 完全ネイティブの Rust アプリケーション、Python ランタイム依存ゼロ
- ✅ **CUDA 不要** — Vulkan と OpenVINO で動作、Intel/AMD/NVIDIA GPU すべて対応
- ✅ **Intel XPU でブラックスクリーンなし** — ネイティブ Vulkan コンピュートでドライバーバグを完全に回避
- ✅ **ワンクリックでカラオケ生成** — 音楽の知識がまったくない人でも、AI の力を借りて任意の楽曲からプレイ可能なカラオケ譜面を自動生成できる、まったく新しいアルゴリズムパイプライン

> 生成結果の正確性は保証できません（できる限りの努力はしていますが……）、個人で楽しむおもちゃ・暇つぶしとしては十分面白いです！

## 機能

### 🎵 AI 駆動の分析パイプライン
- **ボーカル分離** — AI ベースのステム分離で、リードボーカルとインストゥルメンタルを分離（BSRoformer、GGML/Vulkan 加速）
- **音声認識** — Qwen3-ASR 1.7B による多言語歌詞の文字起こし
- **強制アライメント** — Qwen3-ForcedAligner 0.6B による単語レベルのタイミング合わせ（11言語対応：中・英・日・韓・仏・独・伊・葡・露・西・粤）
- **ピッチ検出** — RMVPE による精密な基本周波数抽出
- **ノートセグメンテーション** — GAME ベースの意味的ノート領域分析 + DSP 後処理
- **エキスパートフュージョン** — マルチエキスパート候補システム、決定論的アルゴリズム融合またはオプションの AI 判断モード

### 🎤 譜面エディタ
- デコード波形表示 + ピッチトレースオーバーレイ
- マルチノートマーキー選択、グループ移動/移調/リサイズ
- ノートの分割、結合、クリップボード操作
- 設定可能なクオンタイズグリッド + リアルタイムタップタイミングモード
- フレーズ・単語境界の編集
- UltraStar ノートタイプ（ノーマル、ゴールデン、フリースタイル、ラップ、ゴールデンラップ）
- マルチトラック＆デュエット制作（Lead、Harmony、Backing、Adlib ロール、P1/P2 デュエット自動導出）
- 高度な言語別音節化（日本語モーラ・促音・長音・独立「ん」分割、中国語漢字、韓国語ハングル、ラテン語母音グループ）
- ステム間の瞬時 A/B 試聴切り替え（例：ガイドボーカル vs. 原曲ミックス、再生を止めずに切り替え）
- シンセピッチプレビュー — ボーカルトラックに対してノート音高を聴覚確認できるサイン波/高調波トーン
- グローバルギャップ補正
- 譜面問題インスペクタ + 保守的な自動タイミング修復
- 操作名付きの完全な Undo/Redo 履歴

### 🔍 歌詞ワークベンチ
- 複数プロバイダの並列オンライン歌詞検索（LRCLIB、QQ 音楽、酷狗、網易雲音楽）
- タイムタグ付き LRC、プレーンテキスト、翻訳、ローマ字に対応
- インプレース正規化、タイムスタンプ除去、ワンクリック「保存」および「保存＋アライメント」

### 📦 エクスポート形式
- **UTZ**（`.utz`）— 互換カラオケランタイム用の自己完結型パッケージ
- **UltraStar**（`.txt`）— UTF-8 UltraStar 1.1 フォーマット + 同階層メディアファイル、デュエット対応
- ライブラリ全体の一括バッチエクスポート
- 自動化用 CLI エクスポートツール（`uta-studio-export`）

### 🖥️ プラットフォーム対応
- **Linux** — ネイティブ Wayland、GStreamer + PipeWire/Pulse オーディオ
- **Windows 10/11 x86-64** — ポータブル ZIP、WASAPI オーディオ出力
- FLAC、MP3、WAV、Ogg/Vorbis、AAC/MP4 入力形式対応

### 🧠 ネイティブ推論エンジン（Python 不要！）
- **GGML + Vulkan** — GPU アクセラレーテッド推論（RoFormer、RMVPE、Qwen モデル対応）
- **OpenVINO 2026.3** — GPU 実行 + マニフェスト固定の CPU アイランド
- CPU は診断/参照レーンのみ（自動フォールバックなし）
- すべてのモデルは **設定 > モデルとランタイム** で管理、ユーザーの明示的な確認が必要

### 🎼 処理パイプラインとワークフロー
- ノードカードによる視覚的 DAG パイプライン表示（ドラッグで順序変更可能）
- 解析実行前の事前プランプレビュー（整合性・モデル準備状態チェック）
- 世代管理されたイミュータブルな成果物リビジョン（ピン留め/比較/差分/選択的マージ）
- 順序変更・キャンセル・再実行に対応した永続的処理キュー
- ランごとに分離された JSONL 解析ログ（ライブストリーミングビューア付き）

### 🌍 国際化
- インターフェース言語：英語、簡体字中国語、日本語
- **設定 > 一般 > インターフェース言語** で選択

### 🎨 UI デザイン
- カバーアート優先の視覚的階層とすっきりしたレイアウト
- ダークモード / ライトモードテーマ切り替え
- マルチフォルダライブラリ + カバーアートブラウジング + メタデータ編集
- CJK 全文検索および F1 コンテキストヘルプ対応のオフラインドキュメントセンター
- コンテキストメニュー（編集/開く/場所を表示アクション）
- 設定は左ナビゲーション、カテゴリ別に整理

## ビルド

```sh
bash dev.sh
cargo xtask docs check
cargo test --workspace
cargo check --workspace
```

デスクトップアプリの起動：

```sh
cargo desktop dev
```

リリースビルド：

```sh
./build.sh
```

リリースパッケージ：`nix build path:.#uta-studio`

Linux デスクトップはネイティブ Wayland を使用します。Uta! Studio は X11 バックエンドを有効化せず、XWayland にフォールバックしません。

## ランタイムマネージャ CLI

`uta-runtime` はランタイムマネージャライブラリのスクリプタブルフロントエンドです：

```sh
uta-runtime list --output json
uta-runtime status --check
uta-runtime plan model:qwen3_asr_1_7b --policy benchmark
uta-runtime verify --output json
```

変更操作には明示的な確認が必要です（非対話モードでは `--yes` を使用）。

## TODO

1. マウスホイールスクロール時のエディタフリッカーを修正。
2. 設定でのモデルダウンロードと削除機能を更新。
3. GGUF モデルの自動ダウンロード機能を追加。
4. 編集・ファインチューニングしたモデルを Hugging Face へアップロードする機能を追加。
5. GPU、内蔵GPU、CPU推論を同時実行するスーパー効率モードを追加。
6. 生成された MIDI の微細なタイミングオフセットを修正。
7. 歌詞ダウンロード機能を改善。

## ドキュメント

- **[ユーザーガイド](docs/USER_GUIDE.md)** — インストール、セットアップ、分析、編集、エクスポート
- **[アーキテクチャ](docs/design/README.md)** — システム設計とコンポーネント境界
- **[国際化ガイド](docs/I18N.md)** — ロケール解決とカタログメンテナンス
- **[エンジニアリング制約](docs/engineering-constraints.md)** — 製品ルールとテストマトリックス

---

## Acknowledgements

Uta! Studio thanks the following projects for technical and interface references:

- **[BSRoformer.cpp](https://github.com/yasoukyoku/BSRoformer.cpp)** for the
  RoFormer graph and DSP technical reference used by the packaged native
  implementation. Current RoFormer execution is provided only by the packaged
  GGML/Vulkan worker with batch size 1, synchronous submission, and a serial
  pipeline; OpenVINO routing is rejected for all five RoFormer resources.
- **[transcribe.cpp](https://github.com/handy-computer/transcribe.cpp)** and
  **[qwen3-asr.cpp](https://github.com/predict-woo/qwen3-asr.cpp)** for the two
  separately pinned Qwen native runtime recipes. Their exact commits, GGML
  revisions, and model identities are locked in `native-inference/runtime-lock.json`.
- **[USKMaker](https://github.com/walterfr/UltraStarKaraokeMaker)**,
  **[Yass](https://github.com/SarutaSan72/Yass)**, and
  **[UltraStar Play](https://github.com/UltraStar-Deluxe/Play)** for editor
  interaction patterns, karaoke workflow references, and format conventions.
  USKMaker and UltraStar Play are MIT-licensed; Yass is GPL-3.0-or-later.
  Uta! Studio keeps a seconds/MIDI internal source model and applies export-time
  quantization only when writing targets.
- **[NextFire MMS karaoke-tuned model](https://huggingface.co/NextFire/mms-300m-ForcedAligner-karaoke-ja-Latn)**.
  This AGPL-3.0 model is not shipped by Uta! Studio; users install it explicitly
  in **Settings > Models & runtime**. For aligned timing, use
  **Settings > Analysis > Word timing & alignment** to enable **MMS Karaoke
  (Japanese)**, then configure it in **Models & runtime > Word timing &
  alignment**.
- **Roon's public product UI** as an interaction-direction reference for the
  cover-first information layout and command-area flow in this application's
  music-library and charting environment.
- **Root `icon.png`** in this repository is the canonical brand artwork.
  Packaged desktop icons and the square derivative are generated from this file.
- **[Nightingale](https://github.com/rzru/nightingale/)** for additional
  inspiration around lightweight audio-centric tooling and charting workflow
  patterns.

## License

- **Interface & UI Design:** [CC BY-NC-ND 4.0](https://creativecommons.org/licenses/by-nc-nd/4.0/)
- **Algorithms & Core Analysis Engine:** AGPL-3.0
- **Third-Party & Pretrained Models:** All integrated or downloadable models follow their respective upstream licenses (e.g. Apache-2.0, MIT, AGPL-3.0; see individual model sources and notices for details). / 其他模型遵循各自上游的 License。 / その他利用モデルは各アップストリームのライセンスに従います。
