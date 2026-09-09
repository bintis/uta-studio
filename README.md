<div align="center">
  <img src="icon.png" alt="Uta! Studio" width="128" />
  <h1>Uta! Studio</h1>

  **[English](#english)** | **[中文](#中文)** | **[日本語](#日本語)**
</div>

---

# English

Uta! Studio is a native desktop application for preparing, reviewing, editing, auditioning, and exporting karaoke charts. The application and current model graphs are implemented in Rust; model execution uses a pinned upstream GGML shared-library runtime with Vulkan.

## Current capabilities

- Library browsing across multiple folder roots
- Cover-forward song detail and chart editor
- Native audio audition and waveform/pitch display
- Visual processing workflow and exact Engine plan preview
- Leap XE90 vocal/instrumental separation in one invocation
- Optional PolarFormer separation experiment
- MelBand-RoFormer lead isolation, denoise, and dereverb
- RMVPE primary F0 plus Maximum-mode FCPE secondary evidence and Acoustic DSP context
- Qwen3 singing transcription and word-level forced alignment, with an optional FireRed transcript challenger
- GAME note evidence with optional Basic Pitch, JBM555, STARS, and ROSVOT challengers, plus STARS technique analysis
- Candidate graph, deterministic fusion, review, and editing
- UTZ and UltraStar export with atomic publication
- English, Simplified Chinese, and Japanese UI
- Linux Wayland and Windows support

Caller-provided lyrics remain supported and bypass generated transcription entirely.

## Native inference boundary

Every model runs on one backend. Rust owns each model graph and calls shared libraries built from pinned upstream `ggml-org/ggml` revision `8c63e70982c95ceb862e3a1073a2c1beef75d60a`. The catalog and each model's product role are listed in [`docs/AUDIO_MODELS.md`](docs/AUDIO_MODELS.md).

The package contains no app-owned C/C++ model graph, shim, model CLI, Python model runtime, or inference subprocess, and the repository tracks no model conversion or model rewrite script; container migration is the Rust `cargo xtask gguf` command. The runtime is upstream GGML plus exactly the backend patches its recipe declares. Vulkan remains the default model route. CPU is available only as an explicitly selected experimental reference mode; GPU and integrated-GPU requests never fall back to it. FFmpeg remains an audio codec boundary.

Models are managed in **Settings > Models & runtime**. Startup, browsing, status checks, diagnostics, and workflow compilation do not download models. Configured model directories are user data.

## Build and test

Use the repository development shell for Rust, Node, and native-library work:

```sh
bash dev.sh
```

From that shell:

```sh
cargo test --locked -p uta-analysis-engine
cargo test --locked -p uta-studio-core
cargo test --locked -p uta-runtime-manager
cargo test --locked -p uta-studio-desktop
```

Build the product:

```sh
bash build.sh
```

Linux is Wayland-only; Uta! Studio does not enable X11 or XWayland fallback.

## Runtime Manager CLI

```sh
uta-runtime list --output json
uta-runtime status --check
uta-runtime plan model:rmvpe --policy production
uta-runtime verify --output json
```

Mutations require explicit confirmation.

## Documentation

- [Current model/task state](tasks/remaining-models/STATE.md)
- [Key conclusions](docs/KEY_CONCLUSIONS.md)
- [Architecture](docs/design/README.md)
- [Engineering constraints](docs/engineering-constraints.md)
- [User guide](docs/USER_GUIDE.md)

---

# 中文

Uta! Studio 是一款原生桌面卡拉 OK 曲谱工具，可用于歌曲管理、分析、审核、编辑、试听和导出。应用及当前模型图均由 Rust 实现；模型推理通过固定版本的上游 GGML 共享库和 Vulkan 执行。

## 当前能力

- 多文件夹曲库、封面式歌曲详情和曲谱编辑器
- 原生音频试听、波形与音高显示
- 可视化处理工作流和精确 Engine 计划预览
- Leap XE90 单次推理同时输出人声与伴奏残差
- 可显式选择的 PolarFormer 实验分离策略
- MelBand-RoFormer 主唱隔离、降噪和去混响
- RMVPE 主 F0、Maximum 模式 FCPE 次级证据与 Acoustic DSP 上下文
- Qwen3 歌唱转写与词级强制对齐，并可选启用 FireRed 转写挑战者
- GAME 音符证据，可选 Basic Pitch、JBM555、STARS、ROSVOT 挑战者，以及 STARS 技巧分析
- Candidate 图、确定性融合、审核与编辑
- 原子化 UTZ 和 UltraStar 导出
- 英文、简体中文、日文界面
- Linux Wayland 与 Windows 支持

调用者提供的歌词仍可直接使用，并完全跳过生成式转写。

## 原生推理边界

全部模型统一使用同一个后端。Rust 直接构造每个模型图，并调用固定上游 `ggml-org/ggml` revision `8c63e70982c95ceb862e3a1073a2c1beef75d60a` 构建的共享库。目录内容与各模型的产品角色见 [`docs/AUDIO_MODELS.md`](docs/AUDIO_MODELS.md)。

产品包不含自有 C/C++ 模型图、shim、模型 CLI、Python 模型运行时或推理子进程，仓库中也不保留任何模型转换或模型重写脚本。Vulkan 是默认推理路线；CPU 仅能作为实验参考模式显式选择，GPU/集显请求不会自动回退 CPU。FFmpeg 仅用于音频编解码。

模型由 **设置 > 模型与运行时** 管理。启动、浏览、状态检查、诊断和工作流编译不会下载模型；配置的模型目录属于用户数据。

## 构建与测试

```sh
bash dev.sh
```

进入开发环境后运行相关 Cargo 测试；产品构建使用：

```sh
bash build.sh
```

Linux 仅支持 Wayland，不启用 X11 或 XWayland 回退。

## 文档

- [当前模型与任务状态](tasks/remaining-models/STATE.md)
- [关键结论](docs/KEY_CONCLUSIONS.md)
- [架构](docs/design/README.md)
- [工程约束](docs/engineering-constraints.md)
- [用户指南](docs/USER_GUIDE.md)

---

# 日本語

Uta! Studio は、カラオケ譜面の準備・確認・編集・試聴・書き出しを行うネイティブデスクトップアプリです。アプリと現在のモデルグラフは Rust で実装され、固定した upstream GGML 共有ライブラリを Vulkan 経由で使用します。

## 現在の機能

- 複数フォルダーのライブラリ管理と譜面エディター
- ネイティブ音声試聴、波形、ピッチ表示
- ビジュアルワークフローと Engine プラン確認
- Leap XE90 の単一実行によるボーカル／伴奏残差分離
- 明示的に選択する PolarFormer 実験ルート
- リード分離、ノイズ除去、残響除去
- RMVPE の主 F0、Maximum モードの FCPE 補助エビデンス、Acoustic DSP
- Qwen3 による歌唱文字起こしと単語単位の強制アラインメント、任意の FireRed チャレンジャー
- GAME のノートエビデンスと、任意の Basic Pitch／JBM555／STARS／ROSVOT チャレンジャー、STARS の歌唱技巧解析
- Candidate グラフ、決定論的融合、レビュー、編集
- アトミックな UTZ／UltraStar 書き出し
- 英語・簡体字中国語・日本語 UI
- Linux Wayland と Windows

利用者が提供した歌詞は引き続き利用でき、生成的な文字起こしを完全に迂回します。

## 推論境界

すべてのモデルが同一のバックエンドで動作します。Rust が各モデルグラフを所有し、固定した upstream `ggml-org/ggml` revision `8c63e70982c95ceb862e3a1073a2c1beef75d60a` の共有ライブラリを直接呼び出します。カタログと各モデルの役割は [`docs/AUDIO_MODELS.md`](docs/AUDIO_MODELS.md) に記載しています。

製品には独自 C/C++ モデルグラフ、shim、モデル CLI、Python モデルランタイム、推論サブプロセスを含めず、リポジトリにもモデル変換・書き換えスクリプトを残しません。Vulkan が既定です。CPU は明示的に選ぶ実験参照モードに限られ、GPU／統合 GPU 要求から CPU へ自動フォールバックしません。FFmpeg は音声コーデック境界としてのみ使用します。

## ビルド

```sh
bash dev.sh
bash build.sh
```

Linux は Wayland 専用で、X11／XWayland フォールバックは有効にしません。

## ドキュメント

- [現在のモデル／タスク状態](tasks/remaining-models/STATE.md)
- [主要な結論](docs/KEY_CONCLUSIONS.md)
- [アーキテクチャ](docs/design/README.md)
- [エンジニアリング制約](docs/engineering-constraints.md)
- [ユーザーガイド](docs/USER_GUIDE.md)
