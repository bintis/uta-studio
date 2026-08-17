# Uta Studio User Guide / 用户说明书 / ユーザーガイド

**Applies to:** Uta Studio 0.5.x  
**Document revision:** 2026-08-17  
**License:** Documentation distributed with the GPL-3.0 project.

[English](#english) · [简体中文](#简体中文) · [日本語](#日本語)

> Uta Studio is a local desktop authoring application. It reads source media from folders you choose, creates generated working data separately, and exports karaoke charts as UTZ or UltraStar packages. Model downloads begin only after explicit confirmation.

---

## English

### 1. What Uta Studio does

Uta Studio turns local audio or video into an editable karaoke chart. Its normal workflow is:

1. Add one or more watched folders.
2. Scan supported local media into the library.
3. Configure and run the four-stage analysis pipeline.
4. Review lyrics, timing, and pitch in the built-in editor.
5. Save the authored chart.
6. Export either an **Uta package (`.utz`)** or **UltraStar 1.1 (`.txt`)** bundle.

Uta Studio does not move or delete source media. Generated stems, models, previews, charts, and temporary authoring data are stored separately.

### 2. Installation

Download the package for your system from the project’s GitHub Releases page. Release 0.4.0 provides Windows x86-64 ZIP, Debian, RPM, and portable Linux packages, together with SHA-256 checksum files.

#### Windows

1. Download `uta-studio-0.4.0-x86_64-windows.zip` and its checksum file.
2. Extract the ZIP to a writable folder.
3. Start `uta-studio.exe` from the extracted folder.
4. Keep the extracted files together; do not run only a copied executable without its packaged assets.

#### Debian / Ubuntu

```sh
sudo apt install ./uta-studio_0.4.0-1_amd64.deb
```

#### Fedora / RHEL-compatible systems

```sh
sudo dnf install ./uta-studio-0.4.0-1.x86_64.rpm
```

#### Portable Linux build

```sh
chmod +x uta-studio-0.4.0-x86_64-linux.bin
./uta-studio-0.4.0-x86_64-linux.bin
```

The Linux desktop is Wayland-native. It does not enable an X11 backend or XWayland fallback.

#### Verify a download

Use the matching `.sha256` file before installing an artifact obtained through a mirror or shared storage:

```sh
sha256sum -c uta-studio-0.4.0-linux-deb.sha256
```

Use the checksum file matching the package type you downloaded.

### 3. First launch

#### 3.1 Choose the interface language

Open **Settings → General → Interface language** and choose:

- **System default** — follows a locale supplied by the operating environment; if no supported locale is available, English is used.
- **English**
- **简体中文**
- **日本語**

The selection is saved in Uta Studio’s configuration. Developers and portable-launch scripts may override it with `UTA_STUDIO_LOCALE=en`, `zh-CN`, or `ja`.

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

Uta Studio may reuse compatible local `ffmpeg`, `uv`, Python, and existing model files. It does not download models merely because the application was launched.

### 4. Quick-start workflow

1. Add a watched folder.
2. Run **Rescan all** and wait for the library scan to finish.
3. In **Settings → Models & runtime**, complete runtime setup.
4. In **Settings → Analysis**, choose the engines and quality profile.
5. Open a song and select **Analyze**.
6. Review the resulting lyrics and chart.
7. Select **Edit chart** and correct timing, syllables, and note pitches.
8. Save the chart.
9. Export `.utz` for the `uta!` game or `.txt` for an UltraStar-compatible workflow.

### 5. Library and folders

#### Library views

The library provides views for all music, analysis progress, completed charts, video sources, artists, albums, playlists, and folders. Search can filter tracks, artists, albums, and playlists.

#### Watched folders

- Adding a folder starts or enables scanning.
- **Rescan all** refreshes the merged library index.
- Removing a watched folder only removes that location from the index. It does not delete source media.
- The Folders page can browse watched roots and the configured output folder.

#### Playback queue

Library playback includes previous/next, pause/play, repeat modes, shuffle, mute, and volume. Playback is for review and authoring; scoring belongs to the separate `uta!` player.

### 6. Analysis pipeline

Uta Studio uses four explicit stages.

#### 01 · Vocal separation

Creates vocal and instrumental stems before recognition. Available choices depend on platform and configured runtime, and may include UVR Karaoke, Demucs, or an Intel/OpenVINO path.

Use a balanced profile first. Memory-saving profiles reduce peak use; quality profiles usually take longer and can require more memory.

#### 02 · Lyrics transcription

Recognizes lyrics from the vocal source. Whisper and Parakeet-family options may be available, depending on the configured runtime and downloaded files.

A larger recognition model can improve difficult material but costs more memory and processing time. Confirm the selected model is installed on **Models & runtime**.

#### 03 · Word timing and alignment

Refines recognized or supplied lyrics into editable word timings. Supported backends can include WhisperX, CTC forced alignment, Qwen forced alignment, and the optional Japanese MMS Karaoke backend.

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

Use this format for the independent `uta!` game and for a self-contained, versioned package. The package can include chart data and the media/artifacts required by that workflow.

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

1. Close Uta Studio.
2. Back up `~/.uta-studio` or the configured data root.
3. Back up authored/exported `.utz` and UltraStar bundles.
4. Keep original source media separately.
5. Restore the data root before launching the new installation when practical.

Do not rely on generated cache as the only copy of finished work; retain exported packages.

### 11. Logs and diagnostics

In **Settings → General**:

- **Application log → View log** opens the current log when one exists.
- **Feature API diagnostics → Run checks** verifies local APIs, native audio, and real temporary UTZ/UltraStar exports. The diagnostic temporary folder is removed after the check.

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

Uta Studio is GPL-3.0. Optional third-party models and tools retain their own licenses. Review the confirmation shown before downloading separately licensed artifacts.

---

## 简体中文

### 1. Uta Studio 的用途

Uta Studio 可将本地音频或视频制作成可编辑的卡拉 OK 谱面。标准流程如下：

1. 添加一个或多个监视文件夹。
2. 扫描本地媒体并建立曲库索引。
3. 配置并运行四阶段分析流水线。
4. 在内置编辑器中校对歌词、时间和音高。
5. 保存人工制作的谱面。
6. 导出 **Uta 包（`.utz`）**或 **UltraStar 1.1（`.txt`）**包。

Uta Studio 不会移动或删除源媒体。生成的分轨、模型、预览、谱面和临时制作数据会单独存放。

### 2. 安装

请在项目的 GitHub Releases 页面下载适合系统的安装包。0.4.0 版本提供 Windows x86-64 ZIP、Debian、RPM 和 Linux 便携包，同时提供对应的 SHA-256 校验文件。

#### Windows

1. 下载 `uta-studio-0.4.0-x86_64-windows.zip` 及对应校验文件。
2. 将 ZIP 解压到可写文件夹。
3. 从解压后的文件夹运行 `uta-studio.exe`。
4. 请保留包内文件的相对结构，不要只复制可执行文件单独运行。

#### Debian / Ubuntu

```sh
sudo apt install ./uta-studio_0.4.0-1_amd64.deb
```

#### Fedora / RHEL 兼容系统

```sh
sudo dnf install ./uta-studio-0.4.0-1.x86_64.rpm
```

#### Linux 便携版

```sh
chmod +x uta-studio-0.4.0-x86_64-linux.bin
./uta-studio-0.4.0-x86_64-linux.bin
```

Linux 桌面端原生使用 Wayland，不启用 X11 后端，也不回退到 XWayland。

#### 校验下载文件

从镜像或共享存储取得文件时，建议在安装前使用对应 `.sha256` 文件校验：

```sh
sha256sum -c uta-studio-0.4.0-linux-deb.sha256
```

校验文件必须与下载的包类型一致。

### 3. 首次启动

#### 3.1 选择界面语言

打开 **设置 → 常规 → 界面语言**，可选择：

- **跟随系统**：采用运行环境提供的区域设置；无法识别时回退为英语。
- **English**
- **简体中文**
- **日本語**

选择会保存到 Uta Studio 配置中。开发者或便携启动脚本也可使用 `UTA_STUDIO_LOCALE=en`、`zh-CN` 或 `ja` 强制覆盖。

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

Uta Studio 会尽量复用兼容的本地 `ffmpeg`、`uv`、Python 和已有模型文件。应用启动本身不会自动下载模型。

### 4. 快速上手

1. 添加监视文件夹。
2. 运行**全部重新扫描**，等待曲库扫描结束。
3. 在**设置 → 模型与运行环境**中完成运行环境设置。
4. 在**设置 → 分析**中选择引擎和质量配置。
5. 打开歌曲并选择**分析**。
6. 检查生成的歌词与谱面。
7. 选择**编辑谱面**，修正时间、音节和音符音高。
8. 保存谱面。
9. 为 `uta!` 游戏导出 `.utz`，或为 UltraStar 兼容工作流导出 `.txt`。

### 5. 曲库与文件夹

#### 曲库视图

曲库提供全部音乐、分析进度、已完成谱面、视频来源、艺术家、专辑、播放列表和文件夹等视图。搜索可筛选歌曲、艺术家、专辑和播放列表。

#### 监视文件夹

- 添加文件夹后会启动或启用扫描。
- **全部重新扫描**会刷新合并后的曲库索引。
- 移除监视文件夹只会从索引中移除该位置，不会删除源媒体。
- “文件夹”页面可以浏览监视根目录和已配置的输出文件夹。

#### 播放队列

曲库播放支持上一首/下一首、播放/暂停、循环模式、随机播放、静音和音量。这里的播放用于检查与制作；评分属于独立的 `uta!` 播放器。

### 6. 分析流水线

Uta Studio 使用四个明确阶段。

#### 01 · 人声分离

在识别前创建人声与伴奏分轨。可用选项取决于平台和运行环境，可能包括 UVR Karaoke、Demucs 或 Intel/OpenVINO 路径。

建议先使用“均衡”配置。节省内存配置可降低峰值占用；高质量配置通常更慢，并需要更多内存。

#### 02 · 歌词转录

从人声源识别歌词。根据运行环境和已下载文件，可使用 Whisper 或 Parakeet 系列选项。

更大的识别模型可能改善困难素材，但会增加内存和处理时间。请在**模型与运行环境**中确认所选模型已安装。

#### 03 · 单词时间与对齐

将识别或手工提供的歌词细化为可编辑的逐词时间。后端可能包括 WhisperX、CTC 强制对齐、Qwen 强制对齐，以及可选的日语 MMS Karaoke 后端。

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

用于独立的 `uta!` 游戏，也适合保存自包含、带版本信息的包。该包可包含目标工作流所需的谱面数据和媒体/产物。

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

1. 关闭 Uta Studio。
2. 备份 `~/.uta-studio` 或已配置的数据根目录。
3. 备份已制作/导出的 `.utz` 与 UltraStar 包。
4. 单独保留原始源媒体。
5. 条件允许时，在启动新安装前恢复数据根目录。

不要将生成缓存当作成品的唯一副本；请保留导出包。

### 11. 日志与诊断

在**设置 → 常规**中：

- **应用日志 → 查看日志**：在日志存在时打开当前日志。
- **功能 API 诊断 → 运行检查**：验证本地 API、原生音频以及真实的临时 UTZ/UltraStar 导出；诊断完成后会删除临时目录。

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

Uta Studio 使用 GPL-3.0。可选第三方模型与工具保留各自许可证；下载单独授权的产物前，请阅读确认信息。

---

## 日本語

### 1. Uta Studio について

Uta Studio は、ローカルの音声・動画から編集可能なカラオケ譜面を作成するデスクトップアプリです。基本的な流れは次のとおりです。

1. 監視するフォルダーを1つ以上追加する。
2. 対応するローカルメディアをスキャンしてライブラリに登録する。
3. 4段階の解析パイプラインを設定・実行する。
4. 内蔵エディターで歌詞、タイミング、ピッチを確認・修正する。
5. 制作した譜面を保存する。
6. **Uta パッケージ（`.utz`）**または **UltraStar 1.1（`.txt`）**として書き出す。

Uta Studio が元メディアを移動・削除することはありません。生成したステム、モデル、プレビュー、譜面、一時制作データは別に保存されます。

### 2. インストール

プロジェクトの GitHub Releases ページから、お使いの環境に合うパッケージをダウンロードしてください。0.4.0 では Windows x86-64 ZIP、Debian、RPM、Linux ポータブル版と、それぞれの SHA-256 チェックサムが提供されています。

#### Windows

1. `uta-studio-0.4.0-x86_64-windows.zip` と対応するチェックサムをダウンロードします。
2. ZIP を書き込み可能なフォルダーへ展開します。
3. 展開先の `uta-studio.exe` を起動します。
4. パッケージ内の相対配置を保ち、実行ファイルだけを別の場所へコピーして起動しないでください。

#### Debian / Ubuntu

```sh
sudo apt install ./uta-studio_0.4.0-1_amd64.deb
```

#### Fedora / RHEL 互換環境

```sh
sudo dnf install ./uta-studio-0.4.0-1.x86_64.rpm
```

#### Linux ポータブル版

```sh
chmod +x uta-studio-0.4.0-x86_64-linux.bin
./uta-studio-0.4.0-x86_64-linux.bin
```

Linux デスクトップ版は Wayland ネイティブです。X11 バックエンドや XWayland フォールバックは有効にしていません。

#### ダウンロードの検証

ミラーや共有ストレージから取得した場合は、インストール前に対応する `.sha256` ファイルで検証してください。

```sh
sha256sum -c uta-studio-0.4.0-linux-deb.sha256
```

ダウンロードしたパッケージ種別と同じチェックサムファイルを使用します。

### 3. 初回起動

#### 3.1 表示言語を選ぶ

**設定 → 一般 → 表示言語**を開き、次から選択します。

- **システム既定**：実行環境が提供するロケールに従います。対応ロケールが得られない場合は英語になります。
- **English**
- **简体中文**
- **日本語**

選択内容は Uta Studio の設定に保存されます。開発用・ポータブル起動スクリプトでは `UTA_STUDIO_LOCALE=en`、`zh-CN`、`ja` で上書きできます。

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

Uta Studio は互換性のあるローカルの `ffmpeg`、`uv`、Python、既存モデルを再利用できます。アプリを起動しただけでモデルを自動ダウンロードすることはありません。

### 4. クイックスタート

1. 監視フォルダーを追加します。
2. **すべて再スキャン**を実行し、完了を待ちます。
3. **設定 → モデルとランタイム**でセットアップを完了します。
4. **設定 → 解析**でエンジンと品質プロファイルを選びます。
5. 楽曲を開いて**解析**を選びます。
6. 生成された歌詞と譜面を確認します。
7. **譜面を編集**を開き、タイミング、音節、音程を修正します。
8. 譜面を保存します。
9. `uta!` 用には `.utz`、UltraStar 互換ワークフローには `.txt` を書き出します。

### 5. ライブラリとフォルダー

#### ライブラリ表示

すべての楽曲、解析状況、完成済み譜面、動画ソース、アーティスト、アルバム、プレイリスト、フォルダーなどの表示があります。検索では楽曲、アーティスト、アルバム、プレイリストを絞り込めます。

#### 監視フォルダー

- フォルダーを追加するとスキャンが開始または有効になります。
- **すべて再スキャン**は統合ライブラリ索引を更新します。
- 監視フォルダーを削除しても索引から外れるだけで、元メディアは削除されません。
- フォルダーページでは監視ルートと設定済み出力フォルダーを参照できます。

#### 再生キュー

前へ/次へ、再生/一時停止、リピート、シャッフル、ミュート、音量を利用できます。ここでの再生は確認・制作のためのもので、採点は別の `uta!` プレイヤーが担当します。

### 6. 解析パイプライン

Uta Studio は4つの明示的な段階を使用します。

#### 01 · ボーカル分離

認識前にボーカルと伴奏のステムを作ります。利用可能な選択肢はプラットフォームとランタイムにより異なり、UVR Karaoke、Demucs、Intel/OpenVINO 系などがあります。

まずはバランス設定を推奨します。省メモリ設定はピーク使用量を下げ、高品質設定は通常より長い処理時間と多くのメモリを必要とします。

#### 02 · 歌詞文字起こし

ボーカルソースから歌詞を認識します。ランタイムとダウンロード済みファイルに応じて Whisper または Parakeet 系を利用できます。

大きい認識モデルは難しい素材を改善する場合がありますが、メモリと処理時間が増えます。**モデルとランタイム**で選択モデルがインストール済みか確認してください。

#### 03 · 単語タイミングとアラインメント

認識済みまたは入力済み歌詞を、編集可能な単語タイミングに整えます。WhisperX、CTC 強制アラインメント、Qwen 強制アラインメント、任意の日本語 MMS Karaoke バックエンドなどがあります。

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

独立した `uta!` ゲーム向けで、自己完結型かつバージョン管理されたパッケージとして保存できます。ワークフローに必要な譜面データやメディア/成果物を含められます。

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

1. Uta Studio を終了する。
2. `~/.uta-studio` または設定済みデータルートをバックアップする。
3. 制作・書き出し済み `.utz` と UltraStar パッケージをバックアップする。
4. 元メディアを別途保管する。
5. 可能であれば新しいインストールを起動する前にデータルートを復元する。

完成作品の唯一のコピーを生成キャッシュに置かず、書き出しパッケージを保管してください。

### 11. ログと診断

**設定 → 一般**で：

- **アプリログ → ログを表示**：ログが存在する場合に開きます。
- **機能 API 診断 → チェックを実行**：ローカル API、ネイティブ音声、実際の一時 UTZ/UltraStar 書き出しを検証し、診断後に一時フォルダーを削除します。

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

Uta Studio は GPL-3.0 です。任意の第三者モデル・ツールにはそれぞれのライセンスが適用されます。別ライセンスの成果物をダウンロードする前に確認内容を読んでください。
