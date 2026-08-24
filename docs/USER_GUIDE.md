# Uta! Studio User Guide / 用户说明书 / ユーザーガイド

**Applies to:** Uta! Studio 0.6.0
**Document revision:** 2026-08-24
**License:** Documentation distributed with the GPL-3.0 project.

[English](#english) · [简体中文](#简体中文) · [日本語](#日本語)

> This file is generated from `docs/user-guide/*.md`. Do not edit it directly.

---

## English

### 1. What Uta! Studio does

Uta! Studio turns local audio or video into an editable karaoke chart. Its normal workflow is:

1. Add one or more watched folders.
2. Scan supported local media into the library.
3. Configure and run the four-stage analysis pipeline.
4. Review lyrics, timing, and pitch in the built-in editor.
5. Save the authored chart.
6. Export either an **Uta package (`.utz`)** or **UltraStar 1.1 (`.txt`)** bundle.

Uta! Studio does not move or delete source media. Generated stems, models, previews, charts, and temporary authoring data are stored separately.

### 2. Installation

Download the package for your system from the project’s GitHub Releases page. Release 0.6.0 provides Windows x86-64 ZIP, Debian, RPM, and portable Linux packages, together with SHA-256 checksum files.

#### Windows

1. Download `uta-studio-0.6.0-x86_64-windows.zip` and its checksum file.
2. Extract the ZIP to a writable folder.
3. Start `bin\uta-studio.exe` from the extracted folder.
4. Keep the extracted files together; do not run only a copied executable without its packaged assets.

Windows 10/11 x86-64 is supported. Editor and library audition use the system WASAPI output and do not require a separately installed codec pack for FLAC, MP3, WAV, Ogg/Vorbis, or common AAC/MP4 inputs.

#### Debian / Ubuntu

```sh
sudo apt install ./uta-studio_0.6.0-1_amd64.deb
```

#### Fedora / RHEL-compatible systems

```sh
sudo dnf install ./uta-studio-0.6.0-1.x86_64.rpm
```

#### Portable Linux build

```sh
chmod +x uta-studio-0.6.0-x86_64-linux.bin
./uta-studio-0.6.0-x86_64-linux.bin
```

The Linux desktop is Wayland-native. It does not enable an X11 backend or XWayland fallback.

#### Verify a download

Use the matching `.sha256` file before installing an artifact obtained through a mirror or shared storage:

```sh
sha256sum -c uta-studio-0.6.0-linux-deb.sha256
```

Use the checksum file matching the package type you downloaded.

### 3. First launch

#### 3.1 Choose the interface language

Open **Settings → General → Interface language** and choose:

- **System default** — follows a locale supplied by the operating environment; if no supported locale is available, English is used.
- **English**
- **简体中文**
- **日本語**

The selection is saved in Uta! Studio’s configuration. Developers and portable-launch scripts may override it with `UTA_STUDIO_LOCALE=en`, `zh-CN`, or `ja`.

**Interface language is not the same as song analysis language.** Interface language changes menus and messages. Song analysis language controls transcription/alignment for an individual song and is edited from that song’s language action.

#### 3.2 Add music folders

Open **Settings → Storage** or **Folders**, then select **Add folder**. You can add multiple roots; their contents are merged into one library index.

Recommended folder layout:

```text
Music/
  Artist/
    Album/
      Song.flac
      Song.mp4
```

The layout is optional, but good metadata and consistent folders make browsing easier.

#### 3.3 Choose a default export folder

In **Settings → Storage → Default export folder**, choose where Save As dialogs should open first. Each export can still choose another destination.

Batch export requires a default export folder so files can be written without opening one dialog per song.

#### 3.4 Set up models and runtime

Open **Settings → Models & runtime**.

1. Select the acceleration target: CPU, NVIDIA CUDA, or Intel Arc where supported.
2. Review **Runtime status**.
3. Choose **Set up…** or **Reconfigure…**.
4. Read the confirmation, including model size and license notices.
5. Confirm the setup explicitly.

Uta! Studio uses packaged native workers plus compatible local or packaged `ffmpeg` and existing model files. It does not download runtime components or models merely because the application was launched, a page opened, or diagnostics ran.

#### 3.5 Open the Documentation Center

The user guide is also available inside the application. Open **Settings → General → Open user guide**, choose **Documentation** in Settings navigation, or press **F1**. See [Documentation Center](guide:documentation). Analysis outputs are inspected from the song’s analysis graph; see [Analysis artifacts](guide:artifacts).

### 4. Quick-start workflow

1. Add a watched folder.
2. Run **Rescan all** and wait for the library scan to finish.
3. In **Settings → Models & runtime**, complete runtime setup.
4. In **Settings → Analysis**, choose the engines and quality profile.
5. Open a song and select **Analyze**.
6. Review the resulting lyrics and chart.
7. Select **Edit chart** and correct timing, syllables, and note pitches.
8. Save the chart.
9. Export `.utz` for compatible karaoke runtimes or `.txt` for an UltraStar-compatible workflow.

### 5. Library and folders

#### Library views

Browse contains all music and analysis progress. My Library contains charts, video sources, playlists, and folders. Search can filter tracks, artists, albums, and playlists.

#### Watched folders

- Adding a folder starts or enables scanning.
- **Rescan all** refreshes the merged library index.
- Removing a watched folder only removes that location from the index. It does not delete source media.
- The Folders page can browse watched roots and the configured output folder.

#### Playback queue

Library playback includes previous/next, pause/play, repeat modes, shuffle, mute, and volume. Playback is for review and authoring; scoring belongs to a separate compatible player.

### 6. Analysis pipeline

Uta! Studio executes an explicit node-based DAG. Generated files are typed **analysis artifacts**; inspect revisions, lineage, real node progress, and editor routing from the song’s analysis graph. See [Analysis artifacts](guide:artifacts).

#### 01 · Vocal and BGM separation

Runs independent vocal and BGM separation branches. Each branch selects its own separation model, followed by two ordered post-processing slots; each slot can be Off, denoise, or dereverb. The BGM output feeds chart construction directly, while the vocal output feeds pitch and lyrics analysis.

Available choices include validated RoFormer vocal/BGM separation, lead isolation, denoise, and dereverb models. Catalog models are installed only from **Settings > Models & runtime** after you confirm the name, source, size, and license. Analysis stays local; existing chart data changes only after explicit re-analysis.

Use a balanced profile first. Memory-saving profiles reduce peak use; quality profiles usually take longer and can require more memory.

#### 02 · Lyrics transcription

FireRedASR2-AED and Qwen3-ASR produce independent transcript evidence. Uta! Studio fuses their token evidence into Canonical Lyrics instead of silently choosing one complete transcript.

A larger recognition model can improve difficult material but costs more memory and processing time. Confirm the selected model is installed on **Models & runtime**.

#### 03 · Word timing and alignment

The pinned Qwen3 Forced Aligner consumes Canonical Lyrics and selected lead-vocal audio. Its boundaries remain evidence until fusion and can be reviewed in the Editor.

The optional NextFire MMS Karaoke model is separately licensed under AGPL-3.0 and is downloaded only after a dedicated confirmation. Use it only when its license and Japanese-specific behavior fit your project.

#### 04 · Pitch analysis

Extracts pitch evidence and MIDI note targets for chart authoring. The editor remains authoritative: inspect and correct notes instead of treating automatic output as final.

#### Re-analysis rules

Changing an analysis setting does not silently rewrite an existing authored chart. Existing stems or charts change only after the corresponding re-analysis action. This protects manual edits and makes destructive changes explicit.

#### Automatic analysis

When **Auto-analyze** is enabled, newly scanned, unanalyzed songs are queued automatically. Leave it off when model setup is incomplete or when you want to review files before using compute resources.

### 7. Lyrics and language

#### Replace or edit lyrics

From a song, open the lyrics action to:

- enter plain lyrics;
- enter timed LRC;
- search LRCLIB;
- review a candidate before saving;
- optionally queue alignment after saving.

Always verify spelling, repeated lines, punctuation, and omitted vocalizations before alignment.

#### Set song analysis language

Use the song’s **Language** action to choose automatic detection or an explicit language. Saving with reprocessing enabled queues the required analysis again.

This setting affects the song’s recognition/alignment pipeline. It does not change the application interface.

### 8. Chart editor

Open an analyzed song and select **Edit chart**. The editor supports waveform and pitch evidence, lyric/phrase boundaries, note bars, multiple tracks, and named undo history.

Common operations include:

- play, pause, seek, and audition selected material;
- edit lyrics and phrase boundaries;
- drag note timing and pitch;
- select multiple notes with a marquee;
- move, transpose, resize, split, merge, duplicate, copy, and paste;
- apply quantization;
- use tap timing against playback;
- assign UltraStar note types such as normal, golden, freestyle, rap, and golden rap;
- inspect chart problems and apply conservative timing repairs;
- use Lock mode to prevent accidental dragging;
- author lead/backing or duet tracks.

Save after meaningful edit groups. Named undo entries help verify what will be reverted.

#### Safe editing practices

- Keep Lock enabled while only reviewing.
- Audition timing at phrase boundaries and after quantization.
- Inspect low-confidence pitch regions rather than bulk-accepting them.
- Re-open the exported chart in the target player before publishing.

### 9. Export

#### Uta package (`.utz`)

Use this format for compatible karaoke runtimes and for a self-contained, versioned package. The package can include chart data and the media/artifacts required by that workflow.

#### UltraStar (`.txt`)

Exports UTF-8 UltraStar 1.1 text plus sibling media. Export preserves normal, golden, freestyle, rap, and golden-rap note markers and supports multi-track/duet output where authored.

#### Export safety

- Exports are written to a user-selected destination.
- Batch export uses collision-safe behavior rather than silently overwriting another song.
- Source media is not modified.
- Test the final package in its target application.

### 10. Storage, cache, and backup

**Settings → Storage** reports generated storage by songs, models, and other data.

- **Clear generated cache** removes generated stems, charts/previews, and temporary authoring data covered by that cache action. It does not delete source media.
- **Clear models** removes downloaded model artifacts and makes runtime status report them as missing until setup is run again.

The default settings/data root is `~/.uta-studio`, unless a different data location is configured. Before migrating or reinstalling:

1. Close Uta! Studio.
2. Back up `~/.uta-studio` or the configured data root.
3. Back up authored/exported `.utz` and UltraStar bundles.
4. Keep original source media separately.
5. Restore the data root before launching the new installation when practical.

Do not rely on generated cache as the only copy of finished work; retain exported packages.

### 11. Logs and diagnostics

In **Settings → General**:

- **Application log → View log** opens the current log when one exists.
- **Feature API diagnostics → Run checks** verifies local APIs, native audio, and real temporary UTZ/UltraStar exports. The diagnostic temporary folder is removed after the check.

Each analysis run writes one detailed JSONL file under `analysis-logs/`. A DAG node’s **View logs** action opens that run and filters by `node_id`; legacy runs clearly report that no dedicated log exists. Analysis progress, model output, and tracebacks stay out of `app.log`. Confirmed history clearing also removes only the referenced files inside `analysis-logs/`.

Include the application version, platform, selected runtime, relevant log excerpt, and reproducible steps in a bug report. Do not attach copyrighted source media unless you have permission.

### 12. Troubleshooting

#### “Setup required” or missing model

Open **Settings → Models & runtime**, press **Check again**, then install or repair the missing stage. Reconfigure after changing CPU/CUDA/Intel acceleration.

#### Analysis button is disabled

Complete runtime/model setup first. Also confirm the song is local and the source path remains readable.

#### A scan finds no music

Confirm the folder is still watched, permissions allow reading it, and the files are supported local audio/video. Run **Rescan all**.

#### Poor lyrics

Verify the analysis language, try a more suitable transcription model, provide corrected lyrics manually, then run alignment rather than repeating the entire pipeline unnecessarily.

#### Poor timing

Use a backend suited to the language, check that vocal separation is clean, then correct word/phrase boundaries in the editor. For Japanese, evaluate the optional MMS Karaoke backend and its separate license.

#### Poor pitch notes

Audition the vocal source, inspect the pitch trace, and correct MIDI notes manually. Harmony, vibrato, noise, and residual accompaniment can confuse automatic pitch extraction.

#### Linux window does not start

Confirm the session is Wayland, not an X11-only session, and that the graphics stack supports the packaged renderer. Capture the application log for reporting.

#### Language remains English

Choose a language manually in **Settings → General → Interface language**. System default depends on locale variables exposed by the launch environment. Also check whether `UTA_STUDIO_LOCALE` is overriding the saved setting.

### 13. Privacy and licenses

Analysis runs locally with the configured runtime. LRCLIB search is an explicit network-facing lyrics lookup. Model setup is explicit and may contact model hosts after confirmation.

Uta! Studio is GPL-3.0. Optional third-party models and tools retain their own licenses. Review the confirmation shown before downloading separately licensed artifacts.

### 14. Documentation Center

The Documentation Center is an offline, native page. It does not open a browser, fetch remote pages, or run scripts.

Open it from:

- **Settings → General → Open user guide**;
- **Documentation** in the Settings navigation;
- **F1**, which opens context help for the current page or selected analysis node;
- a DAG node **Help** action, or an artifact menu **About this artifact** action.

The document language follows **Settings → General → Interface language**. The article body is not re-translated at runtime; Uta! Studio selects the matching English, Simplified Chinese, or Japanese source first. Viewer chrome still uses the interface catalogue.

#### Browse and search the guide

Search is local to the embedded guide. Headings rank higher than body lines. CJK search uses character substring matching, so no extra tokenizer is required. A result jumps to the matching section.

On a wide window the page uses a contents column, the article, and search or history. On a narrow window those columns stack. Back and Forward remember the sections you opened.

If **F1** is pressed from the chart editor while the chart has unsaved changes, Uta! Studio asks before leaving the editor.

#### Context help and safe links

Stable deep links include `guide:getting-started`, `guide:analysis`, `guide:lyrics`, `guide:editor`, `guide:export`, `guide:documentation`, and `guide:artifacts`. Node help (`node:lyrics.align`) opens the matching workflow chapter. Artifact help (`artifact:TimedTranscript`) opens this Artifact Workbench chapter.

Only in-guide, `guide:`, `node:`, `artifact:`, `problem:`, and `https://` links are followed. Remote images, `file://` links, and scripts are not executed.

### 15. Analysis artifacts

Analysis writes generated files into Uta! Studio’s cache, not over your source media. The Artifact Workbench inspects those files as typed revisions from the song’s analysis graph.

#### Revisions, Active, and Pin

- A **revision** is one immutable snapshot. A later analysis creates another revision; it does not rewrite the old bytes.
- **Active** is the revision later actions use by default.
- **Pin** protects a revision from delete and cache cleanup. Unpin it first if you really want it removed.
- Older runs that predate exact binding records are labelled **Legacy / untracked**. Uta! Studio does not invent missing lineage from a filename or modification time.
- Temporary preprocess audio is labelled **Ephemeral** unless you explicitly capture it.

#### Inspect Node I/O

Open a song, open its analysis graph, and select a node. The Node I/O workbench has **Overview**, **Inputs**, **Outputs**, **Attempts**, **Logs**, and **Help**.

The banner says **Exact run bindings** when the selected history record stored attempt-specific input and output rows. Otherwise it says the inspector is using the current inventory as a fallback, and that exact run lineage was not recorded.

Inputs and outputs distinguish the node’s declared slots from the concrete files bound to the selected run.

#### Artifact menu actions

Right-click an artifact node for the actions that are valid for that kind and state:

- Preview or Play;
- Open in a compatible editor, when the kind supports it;
- Set Active;
- Compare with Active, or Compare a candidate chart with the authored chart;
- Pin or Unpin;
- Lineage;
- Impact;
- Inspect provenance;
- Reveal in the file manager;
- About this artifact;
- Invalidate;
- Delete, unless the revision is pinned.

Actions that cannot succeed are hidden instead of shown as errors. Graph export boxes show readiness, the last recorded destination when one exists, and a menu to validate, re-export, or reveal that file. Export packages are not analysis Artifact revisions.

#### Edit without overwriting analysis output

- **LyricsInput** and **RecognizedText** can open the lyrics editor. Saving writes a new user revision. Analyzer output is not overwritten in place.
- **TimedTranscript** opens a timing surface that keeps word-level times and unknown JSON fields. Historical transcript bytes stay unchanged.
- **CandidateChart** can be compared with **AuthoredChart**. Merge actions can replace the working copy, take candidate lyrics timing, take candidate pitch, replace the selected phrase, or replace the selected note range. Phrase and range merges use the current chart-editor selection. Save the authored chart first if it has unsaved edits.
- **AuthoredChart** opens the selected revision, not a silent substitute for “whatever is current”. Saving creates a new authored revision. **Replace Authored** asks for confirmation. **Keep Authored** leaves the saved chart unchanged. A pinned authored chart cannot be replaced until it is unpinned.
- **PitchTrack** and **PitchNoteCandidates** are evidence. They can provide context in the chart editor; the evidence file itself is not rewritten.

**Save Only** writes the new revision and does not queue analysis. **Save and Run Downstream** shows an impact preview first. Confirming that preview is what queues work.

#### Lineage panel and impact preview

**Lineage** can stay on the main analysis graph. Turn it on from the VIEW row or from an artifact menu. Upstream, downstream, and full scope highlight the matching nodes and edges and fade the rest. MINI view keeps only compute nodes; lineage then highlights those producers and consumers. Missing legacy links appear as explicit gaps. Edge labels show the artifact kind and a short revision id. Selecting a revision opens that revision.

**Impact** is a read-only preview built from one frozen analysis plan. It includes the current song profile, staged Freeze / Bypass / Disable intents, and Pin. Groups are will run, will reuse, will become stale, will be blocked, will remain preserved, and exports that need regeneration. **Queue this plan** submits that same request. Cancel leaves the library unchanged.

#### Capture preprocessed audio

Preprocessed audio stays ephemeral on ordinary runs. From the relevant node or artifact menu you can request a one-shot or persistent capture of the real preprocessed FLAC on the next run. The confirmation states storage and privacy implications. A successful one-shot request clears itself. Failed capture leaves the request armed.

#### Analysis node reference

- `preflight` — checks that the local source and runtime are usable before work starts.
- `music.analysis` — key, rhythm, and descriptor analysis used later for timing and charting.
- `stems.separate` — MINI-view aggregate derived from the real stem child nodes; it is not an executable Extract step.
- `stems.vocals` — runs the selected vocal separation model.
- `vocals.denoise` / `vocals.dereverb` — optional vocal post-processing in the configured slot order.
- `stems.instrumental` — runs the independent BGM separation model.
- `instrumental.denoise` / `instrumental.dereverb` — optional BGM post-processing in the configured slot order.
- `stems.bind_analysis_outputs` — validates and binds the final vocal and BGM products for downstream consumers.
- `pitch.extract` — pitch curve and note-candidate evidence.
- `lyrics.preprocess` (Vocal Preprocessing) — vocal audio prepared for recognition and alignment; ephemeral unless captured.
- `lyrics.transcribe` — recognized text.
- `lyrics.align` — word-timed transcript.
- `lyrics.import_timed` — imported timed lyrics.
- `chart.build_candidate` — builds a **CandidateChart** without replacing the authored chart.

#### Artifact kind reference

- **SourceMedia** — the read-only local song or video. Never moved or deleted by analysis.
- **MusicAnalysis**, **KeyAnalysis**, **RhythmAnalysis**, **AudioDescriptors** — music-analysis JSON used by later nodes.
- **VocalStem** and **InstrumentalStem** — separated audio.
- **PreprocessedAudio** — ephemeral recognition input unless you capture it as FLAC.
- **RecognizedText** and **AsrSegments** — transcription evidence.
- **LyricsInput** — user-supplied or promoted lyrics.
- **TimedTranscript** — word-timed lyrics used by alignment and charting.
- **PitchTrack** and **PitchNoteCandidates** — pitch evidence.
- **CandidateChart** — analyzer-proposed chart. Distinct from the authored chart.
- **AuthoredChart** — the chart you edit and export.

---

## Processing Studio and evidence review

Open **Processing Studio** from a song page to edit the machine workflow. Audio transformations rewrite real typed dataflow; the executable audio lanes are Vocal, BGM, Lead, and Vocal Residual. Backing and Harmony remain chart-authoring roles until a future audio partition capability exists. Analyzer attachments choose a concrete audio artifact, while analyzer order only changes ready-node priority. Invalid types, missing hard dependencies, and cycles cannot be saved. **Advanced Graph** displays the exact compiled DAG.

A completed run creates a replaceable Candidate revision. The Editor keeps authored notes visually and semantically dominant, exposes read-only evidence and a disagreement-first Review queue, and applies accepted suggestions through normal undo history. Re-analysis never silently replaces an Authored revision. Use Compare or Merge when a newer Candidate is available.

---

## 简体中文

### 1. Uta! Studio 的用途

Uta! Studio 可将本地音频或视频制作成可编辑的卡拉 OK 谱面。标准流程如下：

1. 添加一个或多个监视文件夹。
2. 扫描本地媒体并建立曲库索引。
3. 配置并运行四阶段分析流水线。
4. 在内置编辑器中校对歌词、时间和音高。
5. 保存人工制作的谱面。
6. 导出 **Uta 包（`.utz`）**或 **UltraStar 1.1（`.txt`）**包。

Uta! Studio 不会移动或删除源媒体。生成的分轨、模型、预览、谱面和临时制作数据会单独存放。

### 2. 安装

请在项目的 GitHub Releases 页面下载适合系统的安装包。0.6.0 版本提供 Windows x86-64 ZIP、Debian、RPM 和 Linux 便携包，同时提供对应的 SHA-256 校验文件。

#### Windows

1. 下载 `uta-studio-0.6.0-x86_64-windows.zip` 及对应校验文件。
2. 将 ZIP 解压到可写文件夹。
3. 从解压后的文件夹运行 `bin\uta-studio.exe`。
4. 请保留包内文件的相对结构，不要只复制可执行文件单独运行。

Uta! Studio 正式支持 Windows 10/11 x86-64。编辑器和曲库试听使用系统 WASAPI 输出；FLAC、MP3、WAV、Ogg/Vorbis 及常见 AAC/MP4 输入无需另装解码包。

#### Debian / Ubuntu

```sh
sudo apt install ./uta-studio_0.6.0-1_amd64.deb
```

#### Fedora / RHEL 兼容系统

```sh
sudo dnf install ./uta-studio-0.6.0-1.x86_64.rpm
```

#### Linux 便携版

```sh
chmod +x uta-studio-0.6.0-x86_64-linux.bin
./uta-studio-0.6.0-x86_64-linux.bin
```

Linux 桌面端原生使用 Wayland，不启用 X11 后端，也不回退到 XWayland。

#### 校验下载文件

从镜像或共享存储取得文件时，建议在安装前使用对应 `.sha256` 文件校验：

```sh
sha256sum -c uta-studio-0.6.0-linux-deb.sha256
```

校验文件必须与下载的包类型一致。

### 3. 首次启动

#### 3.1 选择界面语言

打开 **设置 → 常规 → 界面语言**，可选择：

- **跟随系统**：采用运行环境提供的区域设置；无法识别时回退为英语。
- **English**
- **简体中文**
- **日本語**

选择会保存到 Uta! Studio 配置中。开发者或便携启动脚本也可使用 `UTA_STUDIO_LOCALE=en`、`zh-CN` 或 `ja` 强制覆盖。

**界面语言与歌曲分析语言不同。** 界面语言只改变菜单和消息；歌曲分析语言控制单首歌曲的转录和对齐，应从该歌曲的语言操作中设置。

#### 3.2 添加音乐文件夹

打开 **设置 → 存储**或**文件夹**，选择**添加文件夹**。可以添加多个根目录，内容会合并到同一个曲库索引中。

建议的文件夹结构：

```text
Music/
  Artist/
    Album/
      Song.flac
      Song.mp4
```

目录结构不是硬性要求，但完整元数据和一致的文件组织有助于浏览。

#### 3.3 设置默认导出文件夹

在 **设置 → 存储 → 默认导出文件夹**中选择“另存为”对话框默认打开的位置。每次导出仍可改选其他目录。

“全部导出”必须配置默认导出文件夹，才能避免为每首歌曲逐个打开对话框。

#### 3.4 设置模型与运行环境

打开 **设置 → 模型与运行环境**。

1. 选择加速目标：CPU、NVIDIA CUDA，或受支持时使用 Intel Arc。
2. 查看**运行环境状态**。
3. 选择**设置…**或**重新配置…**。
4. 阅读确认信息，包括模型大小和许可证提示。
5. 明确确认后才开始设置。

Uta! Studio 使用打包的原生 Worker、兼容的本地或打包 `ffmpeg`，并复用已有模型文件。应用启动、打开页面或运行诊断都不会自动下载运行时或模型。

#### 3.5 打开文档中心

用户说明书也可以在应用内阅读。打开**设置 → 常规 → 打开用户指南**，在设置导航中选择**文档**，或按 **F1**。详见[文档中心](guide:documentation)。分析产物在歌曲的分析图中查看，详见[分析产物](guide:artifacts)。

### 4. 快速上手

1. 添加监视文件夹。
2. 运行**全部重新扫描**，等待曲库扫描结束。
3. 在**设置 → 模型与运行环境**中完成运行环境设置。
4. 在**设置 → 分析**中选择引擎和质量配置。
5. 打开歌曲并选择**分析**。
6. 检查生成的歌词与谱面。
7. 选择**编辑谱面**，修正时间、音节和音符音高。
8. 保存谱面。
9. 为兼容的卡拉 OK 运行时导出 `.utz`，或为 UltraStar 兼容工作流导出 `.txt`。

### 5. 曲库与文件夹

#### 曲库视图

“浏览”包含全部音乐和分析进度；“我的曲库”包含谱面、视频来源、播放列表和文件夹。搜索可筛选歌曲、艺术家、专辑和播放列表。

#### 监视文件夹

- 添加文件夹后会启动或启用扫描。
- **全部重新扫描**会刷新合并后的曲库索引。
- 移除监视文件夹只会从索引中移除该位置，不会删除源媒体。
- “文件夹”页面可以浏览监视根目录和已配置的输出文件夹。

#### 播放队列

曲库播放支持上一首/下一首、播放/暂停、循环模式、随机播放、静音和音量。这里的播放用于检查与制作；评分属于独立的兼容播放器。

### 6. 分析流水线

Uta! Studio 使用按节点执行的明确 DAG。生成文件是带类型的**分析产物**；可在歌曲分析图中查看修订、来源、真实节点进度和编辑器入口。详见[分析产物](guide:artifacts)。

#### 01 · 人声与 BGM 分离

人声与 BGM 使用彼此独立的分离分支。每个分支单独选择分离模型，之后有两个按顺序执行的后处理槽；每个槽可设为关闭、降噪或降回声。BGM 产物直接进入谱面构建，人声产物则进入音高与歌词分析。

可用选项包括经过验证的 RoFormer 人声/BGM 分离、主唱隔离、降噪和去混响模型。目录模型只能在 **设置 > 模型与运行环境** 中确认名称、来源、体积和许可后安装。分析保持本地运行；已有制谱数据只会在明确重新分析后改变。

建议先使用“均衡”配置。节省内存配置可降低峰值占用；高质量配置通常更慢，并需要更多内存。

#### 02 · 歌词转录

FireRedASR2-AED 与 Qwen3-ASR 分别生成独立转写证据。Uta! Studio 在 token 层融合为 Canonical Lyrics，不会静默选择某个模型的整段结果。

更大的识别模型可能改善困难素材，但会增加内存和处理时间。请在**模型与运行环境**中确认所选模型已安装。

#### 03 · 单词时间与对齐

固定版本的 Qwen3 Forced Aligner 消费 Canonical Lyrics 和选定的主唱音频。边界在融合前始终是证据，并可在编辑器中复核。

NextFire MMS Karaoke 模型单独采用 AGPL-3.0 许可证，只有在专门确认后才会下载。请仅在其许可证和日语专用行为符合项目需求时使用。

#### 04 · 音高分析

提取谱面制作所需的音高依据与 MIDI 音符目标。编辑器中的人工结果始终具有最终权威性；请检查并修正音符，不要将自动结果直接视为成品。

#### 重新分析规则

更改分析设置不会静默重写已人工制作的谱面。现有分轨或谱面只有在执行对应重新分析操作后才会改变，从而保护手工编辑并明确标示破坏性变化。

#### 自动分析

启用**自动分析**后，新扫描到且尚未分析的歌曲会自动加入队列。模型设置尚未完成，或希望先检查文件再占用算力时，建议保持关闭。

### 7. 歌词与语言

#### 替换或编辑歌词

从歌曲的歌词操作中可以：

- 输入纯文本歌词；
- 输入带时间的 LRC；
- 搜索 LRCLIB；
- 在保存前检查候选结果；
- 保存后按需加入对齐队列。

对齐前请检查拼写、重复段落、标点和漏掉的吟唱内容。

#### 设置歌曲分析语言

使用歌曲的**语言**操作选择自动检测或明确语言。保存时启用重新处理，会将所需分析重新加入队列。

此设置影响歌曲的识别与对齐流水线，不会改变应用界面语言。

### 8. 谱面编辑器

打开已分析歌曲并选择**编辑谱面**。编辑器支持波形、音高依据、歌词/乐句边界、音符条、多轨道和具名撤销历史。

常用操作包括：

- 播放、暂停、定位和试听所选内容；
- 编辑歌词和乐句边界；
- 拖动音符时间与音高；
- 使用框选选择多个音符；
- 移动、转调、改变长度、拆分、合并、复制副本、复制和粘贴；
- 应用量化；
- 跟随播放进行敲击定时；
- 设置普通、黄金、自由、说唱和黄金说唱等 UltraStar 音符类型；
- 检查谱面问题并应用保守的时间修复；
- 使用锁定模式防止误拖动；
- 制作主唱、和声或合唱轨道。

完成一组有意义的编辑后及时保存。具名撤销记录可帮助确认将要回退的内容。

#### 安全编辑建议

- 只查看时保持锁定。
- 在乐句边界和量化后试听时间。
- 对低置信度音高区域逐段检查，不要批量接受。
- 发布前在目标播放器中重新打开并测试导出谱面。

### 9. 导出

#### Uta 包（`.utz`）

用于兼容的卡拉 OK 运行时，也适合保存自包含、带版本信息的包。该包可包含目标工作流所需的谱面数据和媒体/产物。

#### UltraStar（`.txt`）

导出 UTF-8 UltraStar 1.1 文本及同级媒体文件。导出会保留普通、黄金、自由、说唱和黄金说唱标记，并在已制作时支持多轨/合唱输出。

#### 导出安全

- 导出始终写入用户选择的目标位置。
- 批量导出采用避免冲突的行为，不会静默覆盖另一首歌曲。
- 不修改源媒体。
- 请在目标应用中测试最终包。

### 10. 存储、缓存与备份

**设置 → 存储**会按歌曲、模型和其他数据报告生成数据用量。

- **清除生成缓存**会删除该操作覆盖的生成分轨、谱面/预览和临时制作数据，不会删除源媒体。
- **清除模型**会删除已下载模型；再次运行设置前，运行环境状态会将其报告为缺失。

默认设置/数据根目录为 `~/.uta-studio`，除非另行配置数据位置。迁移或重装前：

1. 关闭 Uta! Studio。
2. 备份 `~/.uta-studio` 或已配置的数据根目录。
3. 备份已制作/导出的 `.utz` 与 UltraStar 包。
4. 单独保留原始源媒体。
5. 条件允许时，在启动新安装前恢复数据根目录。

不要将生成缓存当作成品的唯一副本；请保留导出包。

### 11. 日志与诊断

在**设置 → 常规**中：

- **应用日志 → 查看日志**：在日志存在时打开当前日志。
- **功能 API 诊断 → 运行检查**：验证本地 API、原生音频以及真实的临时 UTZ/UltraStar 导出；诊断完成后会删除临时目录。

每次分析运行都会在 `analysis-logs/` 下写入一个详细 JSONL 文件。DAG 节点的**查看日志**会打开所选运行并按 `node_id` 过滤；旧版运行会明确提示没有独立日志。分析进度、模型输出和 traceback 不会进入 `app.log`。确认清空分析历史时，只会同步删除 `analysis-logs/` 内由记录引用的日志文件。

提交问题时，请包含应用版本、平台、所选运行环境、相关日志片段和可复现步骤。没有授权时，不要附带受版权保护的源媒体。

### 12. 故障排查

#### 显示“需要设置”或模型缺失

打开**设置 → 模型与运行环境**，选择**重新检查**，再安装或修复缺失阶段。切换 CPU/CUDA/Intel 加速后需要重新配置运行环境。

#### 分析按钮不可用

先完成运行环境与模型设置，并确认歌曲来自本地且源路径仍可读取。

#### 扫描不到音乐

确认文件夹仍在监视、应用具有读取权限，且文件属于受支持的本地音频/视频格式，然后运行**全部重新扫描**。

#### 歌词质量较差

检查分析语言，尝试更合适的转录模型，或手工提供修正歌词；随后只重新执行对齐，避免不必要地重复完整流水线。

#### 时间对齐较差

选择适合语言的后端，检查人声分离是否干净，再在编辑器中修正单词和乐句边界。日语素材可评估可选的 MMS Karaoke 后端及其独立许可证。

#### 音符音高较差

试听人声源、查看音高轨迹并手工修正 MIDI 音符。和声、颤音、噪声和残留伴奏都可能干扰自动音高提取。

#### Linux 窗口无法启动

确认当前为 Wayland 会话，而非仅 X11 会话，并确认图形栈支持打包的渲染器。记录应用日志用于报告。

#### 界面仍为英语

在**设置 → 常规 → 界面语言**中手动选择语言。“跟随系统”依赖启动环境暴露的区域设置变量；同时检查 `UTA_STUDIO_LOCALE` 是否覆盖了已保存设置。

### 13. 隐私与许可证

分析使用已配置的本地运行环境执行。LRCLIB 搜索是明确触发的联网歌词查询。模型设置在确认后可能访问模型托管服务。

Uta! Studio 使用 GPL-3.0。可选第三方模型与工具保留各自许可证；下载单独授权的产物前，请阅读确认信息。

### 14. 文档中心

文档中心是离线的原生页面。它不会打开浏览器、下载远程页面或执行脚本。

打开方式：

- **设置 → 常规 → 打开用户指南**；
- 设置导航中的**文档**；
- **F1**，按当前页面或已选分析节点打开上下文帮助；
- 分析图节点的**帮助**，或产物菜单中的**关于此产物**。

文档语言跟随**设置 → 常规 → 界面语言**。正文不会在运行时再翻译一遍；应用会先选定对应的英语、简体中文或日语源文。界面框架仍使用界面文案目录。

#### 浏览与搜索说明书

搜索只针对内嵌说明书。标题的排序高于正文。中日韩搜索按字符子串匹配，不需要额外分词。结果会跳到对应章节。

宽窗口使用目录、正文和搜索/历史三列；窄窗口会改为纵向堆叠。前进和后退会记住你打开过的章节。

如果在谱面编辑器里有未保存更改时按下 **F1**，离开编辑器前会先询问。

#### 上下文帮助与安全链接

稳定深链接包括 `guide:getting-started`、`guide:analysis`、`guide:lyrics`、`guide:editor`、`guide:export`、`guide:documentation` 和 `guide:artifacts`。节点帮助（如 `node:lyrics.align`）打开对应的工作流章节。产物帮助（如 `artifact:TimedTranscript`）打开“分析产物”这一章。

只会跟随说明书内部、`guide:`、`node:`、`artifact:`、`problem:` 和 `https://` 链接。远程图片、`file://` 链接和脚本都不会执行。

### 15. 分析产物

分析会把生成文件写入 Uta! Studio 缓存，而不会改写源媒体。产物工作台从歌曲的分析图中，按类型查看这些修订。

#### 修订、当前选用与固定

- **修订**是一份不可变快照。之后的分析会生成新修订，而不会改写旧字节。
- **当前选用（Active）**是后续操作默认使用的修订。
- **固定（Pin）**保护修订不被删除和缓存清理。真正要删除时，需要先取消固定。
- 早于精确绑定记录的旧运行会标为**旧版/未跟踪**。Uta! Studio 不会根据文件名或修改时间编造缺失的来源。
- 临时预处理音频标为**临时（Ephemeral）**，除非你明确捕获它。

#### 查看节点输入输出

打开歌曲及其分析图，选中一个节点。节点 I/O 工作台包含**概览**、**输入**、**输出**、**尝试**、**日志**和**帮助**。

当所选历史记录保存了按尝试区分的输入输出行时，横幅会显示**精确运行绑定**。否则会说明检查器正在使用当前清单作为回退，且没有记录精确运行来源。

输入和输出会区分节点声明的槽位，以及绑定到所选运行的具体文件。

#### 产物菜单操作

右键产物节点，只显示对该类型和状态有效的操作：

- 预览或播放；
- 在兼容编辑器中打开（若该类型支持）；
- 设为当前选用；
- 与当前选用比较，或将候选谱面与已制作谱面比较；
- 固定或取消固定；
- 来源关系；
- 影响预览；
- 查看出处；
- 在文件管理器中显示；
- 关于此产物；
- 作废；
- 删除（已固定的修订不会出现此项）。

不会成功的操作会被隐藏，而不是点进去再报错。图上的导出框会显示就绪状态、上次记录的目标（若有），并提供校验、重新导出和显示该文件的菜单。导出包不是分析产物修订。

#### 编辑时不覆盖分析输出

- **LyricsInput** 和 **RecognizedText** 可打开歌词编辑器。保存会写入新的用户修订，不会就地覆盖分析器输出。
- **TimedTranscript** 打开时间轴界面，保留词级时间和未知 JSON 字段。历史转录字节保持不变。
- **CandidateChart** 可与 **AuthoredChart** 比较。合并操作可以替换工作副本、采用候选歌词时间、只采用候选音高、替换所选乐句，或替换所选音符范围。乐句和范围合并使用谱面编辑器里的当前选择。若已制作谱面有未保存修改，请先保存。
- **AuthoredChart** 打开所选修订，而不会悄悄换成“当前文件”。保存会创建新的已制作修订。**用候选替换**会先确认。**保留已制作谱面**不会改动已保存谱面。已固定的已制作谱面在取消固定前不能被替换。
- **PitchTrack** 和 **PitchNoteCandidates** 是证据。它们可以在谱面编辑器里提供上下文，证据文件本身不会被改写。

**仅保存**写入新修订，不会排队分析。**保存并运行下游**会先显示影响预览；确认预览后才会真正入队。

#### 来源面板与影响预览

**来源关系**可以显示在主分析图上。从 VIEW 行或产物菜单打开。上游、下游和完整范围会高亮对应节点和边，并淡化其余部分。MINI 视图只保留计算节点，来源高亮仍落在这些生产者和消费者上。缺失的旧版链接显示为明确缺口。边标签显示产物类型和修订短号。选中修订会打开该修订。

**影响**是只读预览，来自一份冻结的分析计划。它包含当前歌曲配置、已暂存的冻结 / 旁路 / 禁用意图，以及固定状态。分组包括将运行、将复用、将变旧、将被阻塞、将保持保留，以及需要重新生成的导出。**排队此计划**提交同一份请求。取消不会改动曲库。

#### 捕获预处理音频

普通运行不会保留预处理音频。可从相关节点或产物菜单请求在下一次运行时一次性或持续捕获真实的预处理 FLAC。确认信息会说明存储和隐私影响。一次性请求成功后会自动清除；失败则保持待捕获。

#### 分析节点参考

- `preflight` — 在开始工作前检查本地源文件和运行环境是否可用。
- `music.analysis` — 调性、节奏和描述符分析，供后续时间和谱面使用。
- `stems.separate` — MINI 视图中由真实分轨子节点派生的聚合节点，不是可执行的 Extract 步骤。
- `stems.vocals` — 执行所选人声分离模型。
- `vocals.denoise` / `vocals.dereverb` — 按配置槽位顺序执行的可选人声后处理。
- `stems.instrumental` — 执行独立的 BGM 分离模型。
- `instrumental.denoise` / `instrumental.dereverb` — 按配置槽位顺序执行的可选 BGM 后处理。
- `stems.bind_analysis_outputs` — 校验并绑定最终人声与 BGM 产物，供下游节点使用。
- `pitch.extract` — 音高曲线和音符候选证据。
- `lyrics.preprocess`（人声预处理） — 把人声收成识别/对齐用的音频；除非捕获，否则为临时文件。
- `lyrics.transcribe` — 识别文本。
- `lyrics.align` — 带词级时间的转录。
- `lyrics.import_timed` — 导入的带时间歌词。
- `chart.build_candidate` — 生成 **CandidateChart**，不会替换已制作谱面。

#### 产物类型参考

- **SourceMedia** — 只读的本地歌曲或视频。分析不会移动或删除它。
- **MusicAnalysis**、**KeyAnalysis**、**RhythmAnalysis**、**AudioDescriptors** — 供后续节点使用的音乐分析 JSON。
- **VocalStem** 和 **InstrumentalStem** — 分离后的音频。
- **PreprocessedAudio** — 识别用输入；除非捕获为 FLAC，否则为临时文件。
- **RecognizedText** 和 **AsrSegments** — 转录证据。
- **LyricsInput** — 用户提供或提升后的歌词。
- **TimedTranscript** — 供对齐和谱面使用的词级时间歌词。
- **PitchTrack** 和 **PitchNoteCandidates** — 音高证据。
- **CandidateChart** — 分析器提出的谱面，与已制作谱面不同。
- **AuthoredChart** — 你编辑并导出的谱面。

---

## Processing Studio 与证据复核

从歌曲页打开 **Processing Studio** 编辑机器工作流。音频 Transformation 会重写真实的类型化数据流；当前可执行音频 lane 是 Vocal、BGM、Lead 与 Vocal Residual。Backing 和 Harmony 在未来音频分流能力实现前仍是制谱轨角色。Analyzer attachment 选择具体音频 Artifact，而 Analyzer 排序只改变 ready-node 优先级。类型不匹配、缺少 hard dependency 或 cycle 的工作流不能保存。**Advanced Graph** 显示精确的 compiled DAG。

完成的运行会生成可替换的 Candidate revision。编辑器始终让人工音符保持最高视觉与语义优先级，并提供只读 Evidence 和 disagreement-first Review queue。接受建议会进入正常 undo 历史；重新分析绝不会静默替换 Authored revision。出现新 Candidate 时请使用 Compare 或 Merge。

---

## 日本語

### 1. Uta! Studio について

Uta! Studio は、ローカルの音声・動画から編集可能なカラオケ譜面を作成するデスクトップアプリです。基本的な流れは次のとおりです。

1. 監視するフォルダーを1つ以上追加する。
2. 対応するローカルメディアをスキャンしてライブラリに登録する。
3. 4段階の解析パイプラインを設定・実行する。
4. 内蔵エディターで歌詞、タイミング、ピッチを確認・修正する。
5. 制作した譜面を保存する。
6. **Uta パッケージ（`.utz`）**または **UltraStar 1.1（`.txt`）**として書き出す。

Uta! Studio が元メディアを移動・削除することはありません。生成したステム、モデル、プレビュー、譜面、一時制作データは別に保存されます。

### 2. インストール

プロジェクトの GitHub Releases ページから、お使いの環境に合うパッケージをダウンロードしてください。0.6.0 では Windows x86-64 ZIP、Debian、RPM、Linux ポータブル版と、それぞれの SHA-256 チェックサムが提供されています。

#### Windows

1. `uta-studio-0.6.0-x86_64-windows.zip` と対応するチェックサムをダウンロードします。
2. ZIP を書き込み可能なフォルダーへ展開します。
3. 展開先の `bin\uta-studio.exe` を起動します。
4. パッケージ内の相対配置を保ち、実行ファイルだけを別の場所へコピーして起動しないでください。

Windows 10/11 x86-64 は正式対応です。エディターとライブラリの試聴にはシステムの WASAPI 出力を使い、FLAC、MP3、WAV、Ogg/Vorbis、一般的な AAC/MP4 入力に別途コーデックパックは不要です。

#### Debian / Ubuntu

```sh
sudo apt install ./uta-studio_0.6.0-1_amd64.deb
```

#### Fedora / RHEL 互換環境

```sh
sudo dnf install ./uta-studio-0.6.0-1.x86_64.rpm
```

#### Linux ポータブル版

```sh
chmod +x uta-studio-0.6.0-x86_64-linux.bin
./uta-studio-0.6.0-x86_64-linux.bin
```

Linux デスクトップ版は Wayland ネイティブです。X11 バックエンドや XWayland フォールバックは有効にしていません。

#### ダウンロードの検証

ミラーや共有ストレージから取得した場合は、インストール前に対応する `.sha256` ファイルで検証してください。

```sh
sha256sum -c uta-studio-0.6.0-linux-deb.sha256
```

ダウンロードしたパッケージ種別と同じチェックサムファイルを使用します。

### 3. 初回起動

#### 3.1 表示言語を選ぶ

**設定 → 一般 → 表示言語**を開き、次から選択します。

- **システム既定**：実行環境が提供するロケールに従います。対応ロケールが得られない場合は英語になります。
- **English**
- **简体中文**
- **日本語**

選択内容は Uta! Studio の設定に保存されます。開発用・ポータブル起動スクリプトでは `UTA_STUDIO_LOCALE=en`、`zh-CN`、`ja` で上書きできます。

**表示言語と楽曲の解析言語は別の設定です。** 表示言語はメニューやメッセージを変更します。解析言語は楽曲ごとの文字起こし・アラインメントに使われ、各楽曲の言語操作から設定します。

#### 3.2 音楽フォルダーを追加する

**設定 → ストレージ**または**フォルダー**を開き、**フォルダーを追加**を選択します。複数のルートを追加でき、内容は1つのライブラリ索引に統合されます。

推奨フォルダー構成：

```text
Music/
  Artist/
    Album/
      Song.flac
      Song.mp4
```

この構成は必須ではありませんが、整ったメタデータと一貫した配置はブラウズに役立ちます。

#### 3.3 既定の書き出しフォルダー

**設定 → ストレージ → 既定の書き出しフォルダー**で、「名前を付けて保存」が最初に開く場所を選択します。個別の書き出し時には別の場所も選べます。

一括書き出しには、楽曲ごとにダイアログを開かず保存するための既定フォルダーが必要です。

#### 3.4 モデルとランタイムを設定する

**設定 → モデルとランタイム**を開きます。

1. CPU、NVIDIA CUDA、対応環境では Intel Arc からアクセラレーション先を選びます。
2. **ランタイム状態**を確認します。
3. **セットアップ…**または**再構成…**を選びます。
4. モデル容量とライセンスを含む確認内容を読みます。
5. 明示的に確定してからセットアップを開始します。

Uta! Studio は同梱ネイティブ Worker、互換性のあるローカルまたは同梱 `ffmpeg`、既存モデルを使用します。起動、ページ表示、診断だけでランタイムやモデルをダウンロードすることはありません。

#### 3.5 ドキュメントセンターを開く

ユーザーガイドはアプリ内でも読めます。**設定 → 一般 → ユーザーガイドを開く**、設定ナビゲーションの**ドキュメント**、または **F1** を使います。詳細は[ドキュメントセンター](guide:documentation)を参照してください。解析の生成物は楽曲の解析グラフから確認します。詳細は[解析成果物](guide:artifacts)を参照してください。

### 4. クイックスタート

1. 監視フォルダーを追加します。
2. **すべて再スキャン**を実行し、完了を待ちます。
3. **設定 → モデルとランタイム**でセットアップを完了します。
4. **設定 → 解析**でエンジンと品質プロファイルを選びます。
5. 楽曲を開いて**解析**を選びます。
6. 生成された歌詞と譜面を確認します。
7. **譜面を編集**を開き、タイミング、音節、音程を修正します。
8. 譜面を保存します。
9. 互換カラオケランタイム用には `.utz`、UltraStar 互換ワークフローには `.txt` を書き出します。

### 5. ライブラリとフォルダー

#### ライブラリ表示

「ブラウズ」にはすべての楽曲と解析状況があり、「マイライブラリ」には譜面、動画ソース、プレイリスト、フォルダーがあります。検索では楽曲、アーティスト、アルバム、プレイリストを絞り込めます。

#### 監視フォルダー

- フォルダーを追加するとスキャンが開始または有効になります。
- **すべて再スキャン**は統合ライブラリ索引を更新します。
- 監視フォルダーを削除しても索引から外れるだけで、元メディアは削除されません。
- フォルダーページでは監視ルートと設定済み出力フォルダーを参照できます。

#### 再生キュー

前へ/次へ、再生/一時停止、リピート、シャッフル、ミュート、音量を利用できます。ここでの再生は確認・制作のためのもので、採点は別の互換プレイヤーが担当します。

### 6. 解析パイプライン

Uta! Studio はノード単位で実行される明示的な DAG を使用します。生成ファイルは型付きの**解析成果物**です。版、由来、実際のノード進捗、エディターへの導線は楽曲の解析グラフから確認します。詳細は[解析成果物](guide:artifacts)を参照してください。

#### 01 · ボーカルと BGM の分離

ボーカルと BGM は独立した分離ブランチで処理します。各ブランチで分離モデルを個別に選び、その後に順番どおり実行される2つの後処理スロットを設定します。各スロットはオフ、ノイズ除去、残響除去から選べます。BGM 成果物は譜面構築へ直接渡り、ボーカル成果物はピッチ・歌詞解析へ渡ります。

検証済みの RoFormer ボーカル/BGM 分離、リード分離、ノイズ除去、残響除去モデルを利用できます。カタログモデルは **設定 > モデルとランタイム** で名前、出典、サイズ、ライセンスを確認したあとだけインストールできます。解析はローカルで行われ、既存譜面は明示的な再解析後にだけ変わります。

まずはバランス設定を推奨します。省メモリ設定はピーク使用量を下げ、高品質設定は通常より長い処理時間と多くのメモリを必要とします。

#### 02 · 歌詞文字起こし

FireRedASR2-AED と Qwen3-ASR は独立した転写エビデンスを生成します。Uta! Studio は一方の全文を選ぶのではなく、token 単位で Canonical Lyrics に融合します。

大きい認識モデルは難しい素材を改善する場合がありますが、メモリと処理時間が増えます。**モデルとランタイム**で選択モデルがインストール済みか確認してください。

#### 03 · 単語タイミングとアラインメント

固定された Qwen3 Forced Aligner が Canonical Lyrics と選択したリードボーカル音声を処理します。境界は融合前のエビデンスとして保持され、エディターで確認できます。

NextFire MMS Karaoke モデルは別途 AGPL-3.0 で提供され、専用確認後にのみダウンロードされます。ライセンスと日本語向け動作がプロジェクトに合う場合だけ使用してください。

#### 04 · ピッチ解析

譜面制作のためのピッチ根拠と MIDI ノート目標を抽出します。最終的な正解はエディターでの編集結果です。自動出力をそのまま完成品とせず、確認・修正してください。

#### 再解析の規則

解析設定を変更しても、制作済み譜面を自動で書き換えません。既存のステムや譜面は、対応する再解析を明示的に実行した後だけ変わります。これにより手動編集を保護し、破壊的変更を明確にします。

#### 自動解析

**自動解析**を有効にすると、新しくスキャンされた未解析楽曲が自動的にキューへ追加されます。モデル設定が未完了の場合や、計算資源を使う前にファイルを確認したい場合は無効のままにします。

### 7. 歌詞と言語

#### 歌詞の置き換え・編集

楽曲の歌詞操作から次を行えます。

- プレーン歌詞を入力する。
- タイム付き LRC を入力する。
- LRCLIB を検索する。
- 保存前に候補を確認する。
- 保存後、必要に応じてアラインメントをキューへ追加する。

アラインメント前に、綴り、繰り返し、句読点、抜けた発声を確認してください。

#### 楽曲の解析言語

楽曲の**言語**操作で自動検出または明示言語を選択します。再処理を有効にして保存すると、必要な解析が再びキューへ追加されます。

この設定は文字起こし・アラインメントに影響し、アプリの表示言語は変更しません。

### 8. 譜面エディター

解析済み楽曲を開いて**譜面を編集**を選びます。波形、ピッチ根拠、歌詞/フレーズ境界、ノートバー、複数トラック、名前付き Undo 履歴を利用できます。

主な操作：

- 再生、一時停止、シーク、選択範囲の試聴。
- 歌詞とフレーズ境界の編集。
- ノートのタイミングと音程のドラッグ。
- マーキーによる複数ノート選択。
- 移動、移調、長さ変更、分割、結合、複製、コピー、貼り付け。
- クオンタイズ。
- 再生に合わせたタップタイミング。
- 通常、ゴールデン、フリースタイル、ラップ、ゴールデンラップの UltraStar ノート種別。
- 譜面問題の確認と保守的なタイミング修復。
- 誤ドラッグを防ぐロックモード。
- リード、バッキング、デュエットトラックの制作。

意味のある編集単位ごとに保存してください。名前付き Undo 履歴で、どの操作が戻るか確認できます。

#### 安全な編集のヒント

- 確認だけのときはロックを有効にします。
- フレーズ境界とクオンタイズ後のタイミングを試聴します。
- 信頼度の低いピッチ領域は一括採用せず個別に確認します。
- 公開前に、対象プレイヤーで書き出した譜面を再度開いてテストします。

### 9. 書き出し

#### Uta パッケージ（`.utz`）

互換カラオケランタイム向けで、自己完結型かつバージョン管理されたパッケージとして保存できます。ワークフローに必要な譜面データやメディア/成果物を含められます。

#### UltraStar（`.txt`）

UTF-8 UltraStar 1.1 テキストと同階層のメディアを書き出します。通常、ゴールデン、フリースタイル、ラップ、ゴールデンラップのマーカーを保持し、制作済みの場合は複数トラック/デュエットにも対応します。

#### 書き出しの安全性

- ユーザーが選択した場所にのみ書き出します。
- 一括書き出しは、別楽曲を無言で上書きしない衝突回避動作を使います。
- 元メディアは変更しません。
- 完成パッケージを対象アプリでテストしてください。

### 10. ストレージ、キャッシュ、バックアップ

**設定 → ストレージ**では生成データを楽曲、モデル、その他に分けて表示します。

- **生成キャッシュを消去**は対象の生成ステム、譜面/プレビュー、一時制作データを削除します。元メディアは削除しません。
- **モデルを消去**はダウンロード済みモデルを削除します。再セットアップまでランタイム状態では不足として表示されます。

既定の設定/データルートは `~/.uta-studio` です。別のデータ場所を設定した場合はそちらが使われます。移行・再インストール前には：

1. Uta! Studio を終了する。
2. `~/.uta-studio` または設定済みデータルートをバックアップする。
3. 制作・書き出し済み `.utz` と UltraStar パッケージをバックアップする。
4. 元メディアを別途保管する。
5. 可能であれば新しいインストールを起動する前にデータルートを復元する。

完成作品の唯一のコピーを生成キャッシュに置かず、書き出しパッケージを保管してください。

### 11. ログと診断

**設定 → 一般**で：

- **アプリログ → ログを表示**：ログが存在する場合に開きます。
- **機能 API 診断 → チェックを実行**：ローカル API、ネイティブ音声、実際の一時 UTZ/UltraStar 書き出しを検証し、診断後に一時フォルダーを削除します。

解析ごとに `analysis-logs/` 配下へ詳細な JSONL ファイルを1つ記録します。DAG ノードの**ログを表示**は選択中の解析ログを開き、`node_id` で絞り込みます。旧解析に専用ログがない場合は明示します。解析進捗、モデル出力、traceback は `app.log` に書きません。解析履歴の消去を確認した場合も、`analysis-logs/` 内で履歴が参照するログだけを削除します。

不具合報告には、アプリのバージョン、プラットフォーム、選択ランタイム、関連ログ、再現手順を含めてください。権利がない場合、著作権で保護された元メディアを添付しないでください。

### 12. トラブルシューティング

#### 「セットアップが必要」またはモデル不足

**設定 → モデルとランタイム**を開いて**再確認**し、不足ステージをインストールまたは修復します。CPU/CUDA/Intel を変更した場合はランタイムを再構成します。

#### 解析ボタンが無効

ランタイムとモデルのセットアップを完了してください。楽曲がローカルにあり、元ファイルのパスを読み取れることも確認します。

#### スキャンしても楽曲が見つからない

フォルダーが監視中で、読み取り権限があり、対応するローカル音声/動画であることを確認し、**すべて再スキャン**を実行します。

#### 歌詞の品質が悪い

解析言語を確認し、より適した文字起こしモデルを試すか、修正歌詞を手動入力します。その後、必要なアラインメントだけを再実行し、不要な全工程の繰り返しを避けます。

#### タイミングが悪い

言語に適したバックエンドを選び、ボーカル分離が十分か確認し、エディターで単語/フレーズ境界を修正します。日本語では任意の MMS Karaoke バックエンドと別ライセンスを検討します。

#### 音程ノートが悪い

ボーカルソースを試聴し、ピッチ軌跡を確認して MIDI ノートを手動修正します。ハーモニー、ビブラート、ノイズ、残留伴奏は自動ピッチ抽出を乱すことがあります。

#### Linux でウィンドウが起動しない

X11 専用ではなく Wayland セッションであること、グラフィック環境が同梱レンダラーを扱えることを確認し、報告用にアプリログを取得してください。

#### 表示が英語のまま

**設定 → 一般 → 表示言語**で手動選択します。システム既定は起動環境が公開するロケール変数に依存します。`UTA_STUDIO_LOCALE` が保存設定を上書きしていないかも確認します。

### 13. プライバシーとライセンス

解析は設定済みのローカルランタイムで実行されます。LRCLIB 検索は明示的に実行するネットワーク歌詞検索です。モデルセットアップは確認後にモデルホストへ接続する場合があります。

Uta! Studio は GPL-3.0 です。任意の第三者モデル・ツールにはそれぞれのライセンスが適用されます。別ライセンスの成果物をダウンロードする前に確認内容を読んでください。

### 14. ドキュメントセンター

ドキュメントセンターはオフラインのネイティブ画面です。ブラウザを開いたり、遠隔ページを取得したり、スクリプトを実行したりしません。

開き方:

- **設定 → 一般 → ユーザーガイドを開く**
- 設定ナビゲーションの**ドキュメント**
- **F1**（現在の画面または選択中の解析ノード向けの文脈ヘルプ）
- DAG ノードの**ヘルプ**、または成果物メニューの**この成果物について**

文書の言語は**設定 → 一般 → 表示言語**に従います。本文は実行時に再翻訳されません。先に英語・簡体字中国語・日本語のいずれかの原文が選ばれます。画面枠は従来の UI カタログを使います。

#### ガイドの閲覧と検索

検索は埋め込みガイドの中だけで行われます。見出しは本文より高く順位付けされます。CJK は文字単位の部分一致なので、追加のトークナイザは不要です。結果を選ぶと該当節へ移動します。

広いウィンドウでは目次、本文、検索/履歴の列を使います。狭いウィンドウでは縦に積みます。戻る/進むは開いた節を記憶します。

譜面エディターに未保存の変更がある状態で **F1** を押すと、エディターを離れる前に確認します。

#### 文脈ヘルプと安全なリンク

安定したディープリンクには `guide:getting-started`、`guide:analysis`、`guide:lyrics`、`guide:editor`、`guide:export`、`guide:documentation`、`guide:artifacts` があります。ノードヘルプ（`node:lyrics.align`）は対応する作業章を開きます。成果物ヘルプ（`artifact:TimedTranscript`）はこの解析成果物の章を開きます。

ガイド内、`guide:`、`node:`、`artifact:`、`problem:`、`https://` のリンクだけが辿られます。遠隔画像、`file://` リンク、スクリプトは実行されません。

### 15. 解析成果物

解析は生成ファイルを Uta! Studio のキャッシュへ書き、ソースメディアは上書きしません。成果物ワークベンチは、楽曲の解析グラフからそれらのファイルを型付きリビジョンとして検査します。

#### リビジョン、Active、Pin

- **リビジョン**は不変のスナップショットです。後の解析は別リビジョンを作り、古いバイトは書き換えません。
- **Active** は後続操作が既定で使うリビジョンです。
- **Pin** は削除とキャッシュ掃除から守ります。本当に消すときは先に解除します。
- 正確なバインド記録より前の実行は **Legacy / untracked** と表示されます。ファイル名や更新時刻から欠けた由来を捏造しません。
- 一時的な前処理音声は、明示的にキャプチャしない限り **Ephemeral** です。

#### ノード入出力の確認

楽曲とその解析グラフを開き、ノードを選びます。ノード I/O ワークベンチには **Overview**、**Inputs**、**Outputs**、**Attempts**、**Logs**、**Help** があります。

選択した履歴に試行単位の入出力行があるときは、見出しが **Exact run bindings** になります。そうでなければ、現在のインベントリをフォールバックとして使っており、正確な実行由来が記録されていないと表示します。

入力と出力は、ノードが宣言したスロットと、選択した実行に束縛された実ファイルを区別します。

#### 成果物メニューの操作

成果物ノードを右クリックすると、その種類と状態で有効な操作だけが出ます。

- プレビューまたは再生
- 対応するエディターで開く（種類が対応している場合）
- Active にする
- Active と比較、または候補譜面をオーサリング譜面と比較
- Pin / Unpin
- 由来（Lineage）
- 影響（Impact）
- 出所を調べる
- ファイルマネージャーで表示
- この成果物について
- 無効化
- 削除（Pin 済みなら出ません）

必ず失敗する操作はエラーとして出さず、隠します。グラフ上の書き出し枠は準備状態と、記録があれば前回の出力先を示し、検証・再書き出し・そのファイルの表示ができます。書き出しパッケージは解析成果物のリビジョンではありません。

#### 解析出力を上書きせずに編集する

- **LyricsInput** と **RecognizedText** は歌詞エディターを開けます。保存は新しいユーザーリビジョンを作り、解析器出力をその場で上書きしません。
- **TimedTranscript** は単語タイミングと未知の JSON フィールドを保つタイミング画面を開きます。過去の転写バイトは変わりません。
- **CandidateChart** は **AuthoredChart** と比較できます。マージは作業コピーの置換、候補の歌詞タイミング、候補ピッチのみ、選択中フレーズの置換、選択ノート範囲の置換ができます。フレーズと範囲のマージは譜面エディターの現在の選択を使います。制作済み譜面に未保存の変更があるときは先に保存します。
- **AuthoredChart** は選んだリビジョンを開きます。「今のファイル」へ黙って差し替えません。保存は新しいオーサリングリビジョンを作ります。**Replace Authored** は確認します。**Keep Authored** は保存済み譜面を変えません。Pin 済みのオーサリング譜面は、解除するまで置換できません。
- **PitchTrack** と **PitchNoteCandidates** は証拠です。譜面エディターで文脈には使えますが、証拠ファイル自体は書き換えません。

**Save Only** は新しいリビジョンを書き、解析をキューしません。**Save and Run Downstream** は先に影響プレビューを出します。プレビューを確認したときだけ作業がキューされます。

#### 由来パネルと影響プレビュー

**Lineage** はメインの解析グラフ上でも使えます。VIEW 行または成果物メニューからオンにします。上流・下流・全体の範囲で該当ノードとエッジを強調し、それ以外を薄くします。MINI ビューは計算ノードだけを残し、由来ハイライトもその生産者・消費者に載ります。欠けた旧リンクは隙間として明示されます。エッジラベルは成果物の種類と短いリビジョン ID を示します。リビジョンを選ぶとそのリビジョンが開きます。

**Impact** は 1 つの凍結済み解析計画から作る読み取り専用プレビューです。現在の楽曲プロファイル、staged された Freeze / Bypass / Disable、Pin を含みます。グループは実行予定、再利用、陳腐化、ブロック、保全、再生成が必要な書き出しです。**Queue this plan** はその同じリクエストを投入します。キャンセルしてもライブラリは変わりません。

#### 前処理音声のキャプチャ

通常の実行では前処理音声は一時的なままです。関連ノードまたは成果物メニューから、次回実行で実際の前処理 FLAC を一度だけ、または継続的に残すよう依頼できます。確認画面は容量とプライバシーの影響を示します。ワンショットが成功すると依頼は消えます。失敗した依頼は残ります。

#### 解析ノード一覧

- `preflight` — 作業開始前にローカルソースとランタイムが使えるか確認します。
- `music.analysis` — 後のタイミングと譜面に使うキー、リズム、記述子解析です。
- `stems.separate` — MINI 表示で実ノードから派生する集約ノードです。実行可能な Extract 処理ではありません。
- `stems.vocals` — 選択したボーカル分離モデルを実行します。
- `vocals.denoise` / `vocals.dereverb` — 設定したスロット順で実行する任意のボーカル後処理です。
- `stems.instrumental` — 独立した BGM 分離モデルを実行します。
- `instrumental.denoise` / `instrumental.dereverb` — 設定したスロット順で実行する任意の BGM 後処理です。
- `stems.bind_analysis_outputs` — 最終ボーカル・BGM 成果物を検証し、下流ノードへ束縛します。
- `pitch.extract` — ピッチ曲線とノート候補の証拠です。
- `lyrics.preprocess`（Vocal Preprocessing） — 認識・アライメント用に整えたボーカル音声です。キャプチャしない限り一時的です。
- `lyrics.transcribe` — 認識テキストです。
- `lyrics.align` — 単語タイミング付き転写です。
- `lyrics.import_timed` — 取り込み済みのタイミング付き歌詞です。
- `chart.build_candidate` — **CandidateChart** を作り、オーサリング譜面は置き換えません。

#### 成果物の種類

- **SourceMedia** — 読み取り専用のローカル楽曲または動画です。解析は移動も削除もしません。
- **MusicAnalysis**、**KeyAnalysis**、**RhythmAnalysis**、**AudioDescriptors** — 後続ノードが使う音楽解析 JSON です。
- **VocalStem** と **InstrumentalStem** — 分離後の音声です。
- **PreprocessedAudio** — 認識入力です。FLAC としてキャプチャしない限り一時的です。
- **RecognizedText** と **AsrSegments** — 文字起こしの証拠です。
- **LyricsInput** — ユーザーが渡した、または昇格した歌詞です。
- **TimedTranscript** — アラインメントと譜面が使う単語タイミング付き歌詞です。
- **PitchTrack** と **PitchNoteCandidates** — ピッチ証拠です。
- **CandidateChart** — 解析器が提案する譜面で、オーサリング譜面とは別です。
- **AuthoredChart** — 編集して書き出す譜面です。

---

## Processing Studio とエビデンス確認

曲ページから **Processing Studio** を開き、機械処理ワークフローを編集します。音声 Transformation は実際の型付きデータフローを書き換え、現在実行可能な音声 lane は Vocal、BGM、Lead、Vocal Residual です。Backing と Harmony は将来の音声分割機能が実装されるまで譜面トラックの役割です。Analyzer attachment は具体的な音声 Artifact を選択し、Analyzer の並び順は ready-node の優先度だけを変更します。型不一致、hard dependency の欠落、cycle を含むワークフローは保存できません。**Advanced Graph** は正確な compiled DAG を表示します。

完了した実行は再生成可能な Candidate revision を作成します。エディターでは人が編集した音符が常に最優先で、読み取り専用 Evidence と disagreement-first Review queue を利用できます。提案の適用は通常の undo 履歴に入り、再解析が Authored revision を暗黙に置き換えることはありません。新しい Candidate は Compare または Merge で確認します。
