# Uta Package Format (`.utz`) 0.2

Status: implementable draft. The Rust implementation in this repository is the
reference implementation for this version.

状态：可实现草案。本仓库中的 Rust 实现是该版本的参考实现。

UTZ 0.2 is a breaking development-line revision of UTZ 0.1. It replaces the
three parallel required chart assets with one authoritative vocal chart.
Frame-level analyzer output becomes optional evidence.

UTZ 0.2 是 UTZ 0.1 的破坏性开发线修订。它以单一权威人声谱面取代原先三个并列
的必需谱面资产；帧级分析器输出降级为可选的证据数据。

## Container and integrity ｜ 容器与完整性

An `.utz` file is a ZIP archive. `manifest.json` MUST exist at the archive root
and MUST be UTF-8 JSON conforming to `manifest-v0.2.schema.json`.

`.utz` 文件是一个 ZIP 归档。`manifest.json` 必须（MUST）位于归档根目录，且必须是
符合 `manifest-v0.2.schema.json` 的 UTF-8 JSON。

Every content path MUST be relative, use `/` separators, and contain only
normal path components. Absolute paths, empty paths, `.`/`..` components,
backslashes, path components ending in `.` or a space, and the Windows
reserved device names (`CON`, `PRN`, `AUX`, `NUL`, `COM1`–`COM9`,
`LPT1`–`LPT9`, in any case, with or without an extension) are invalid.
Paths MUST be unique after Unicode-aware lowercasing, so a package extracts
safely onto case-insensitive file systems. Readers MUST defend against
duplicate names, path traversal, excessive file counts, and excessive
uncompressed size before exposing package contents.

每个内容路径必须是相对路径，使用 `/` 分隔符，且只包含普通路径成分。以下均为非法：
绝对路径、空路径、`.`/`..` 成分、反斜杠、以 `.` 或空格结尾的路径成分，以及 Windows
保留设备名（`CON`、`PRN`、`AUX`、`NUL`、`COM1`–`COM9`、`LPT1`–`LPT9`，不区分大小写、
带不带扩展名均算）。路径在按 Unicode 规则转为小写后必须唯一，以保证包能安全解压到
大小写不敏感的文件系统上。读取方在暴露包内容前，必须防御重名条目、路径穿越、
过多文件数与过大解压体积。

Every package asset MUST be declared by the manifest with its MIME type, byte
count, and lowercase SHA-256. Readers MUST verify all three before parsing an
asset. A 0.2 package MUST NOT contain ZIP entries that the manifest does not
declare (other than `manifest.json` itself); readers MUST reject a package
containing an undeclared entry. Package extensions use the manifest's
`extensions` asset map rather than undeclared ZIP entries.

包内每个资产必须在清单中声明其 MIME 类型、字节数和小写 SHA-256。读取方在解析
资产前必须校验这三项。0.2 包中不得（MUST NOT）出现清单未声明的 ZIP 条目
（`manifest.json` 自身除外）；读取方必须拒绝含未声明条目的包。包扩展应通过清单
的 `extensions` 资产映射实现，而不是塞进未声明的 ZIP 条目。

## Package identity ｜ 包身份

`package_id` is the stable identity of a song package across revisions.
Producers SHOULD use either reverse-DNS under a namespace they control
(`org.example.artist.song`) or a lowercase-hex UUIDv4. Producers are
responsible for uniqueness inside their namespace; libraries deduplicate by
`package_id`.

`package_id` 是歌曲包跨修订版本的稳定身份。制作方应当（SHOULD）使用自己控制的
命名空间下的反向域名（`org.example.artist.song`）或小写十六进制 UUIDv4。制作方
负责保证其命名空间内的唯一性；曲库以 `package_id` 去重。

`revision` is a monotonically increasing edition counter for the same
`package_id`. A producer MUST increase it whenever any package content
changes. A library encountering a known `package_id` with a higher revision
SHOULD treat it as an update.

`revision` 是同一 `package_id` 下单调递增的版次计数。任何包内容发生变化时，制作方
必须递增它。曲库遇到已知 `package_id` 且 revision 更高的包时，应当视为更新。

## Time model ｜ 时间模型

All chart positions use integer units declared by `timebase` (units per
second). Version 1 uses a default and recommended timebase of 1,000,000 units
per second. This preserves audio-clock authority while avoiding
floating-point round-trip drift. Values MUST stay within JavaScript's exact
integer range.

谱面中所有时间位置都使用 `timebase`（每秒单位数）声明的整数单位。版本 1 的默认
且推荐 timebase 为每秒 1,000,000 单位。这既保持音频时钟的权威地位，又避免浮点
往返漂移。数值必须落在 JavaScript 精确整数范围内。

**Chart time zero is the first sample of the instrumental audio asset.**
There is no chart-to-audio offset in 0.2; producers bake any needed shift
into note times when exporting. Guide vocals and the original mix MUST be
sample-aligned with the instrumental. The optional background video is
aligned by `visuals.video_offset_seconds`: the instrumental time at which
video time zero is presented (negative values start the video earlier).

**谱面时间零点定义为器乐音轨的第一个采样。** 0.2 中不存在谱面到音频的偏移量；
制作方在导出时把任何需要的平移直接写进音符时间。导唱与原唱混音必须与器乐音轨
采样对齐。可选的背景视频通过 `visuals.video_offset_seconds` 对齐：其含义是视频
时间零点呈现时对应的器乐时间（负值表示视频先于器乐开始）。

The 0.1 field `audio_offset_seconds` does not exist in 0.2. A 0.1-to-0.2
converter MUST resolve it into the exported note times.

0.1 的 `audio_offset_seconds` 字段在 0.2 中不存在。0.1 到 0.2 的转换器必须把它
消解进导出的音符时间里。

## Required logical content ｜ 必需逻辑内容

- one instrumental audio asset;
- one vocal chart conforming to `vocal-chart-v1.schema.json`.

- 一个器乐音频资产；
- 一个符合 `vocal-chart-v1.schema.json` 的人声谱面。

Guide vocals, the original mix, cover artwork, background video, pitch
evidence, and extension assets are optional.

导唱、原唱混音、封面、背景视频、音高证据和扩展资产均为可选。

The vocal chart is authoritative for playback and scoring. A game MUST NOT
re-analyze audio or replace authored note targets with pitch evidence.

人声谱面是播放与打分的唯一权威。游戏不得重新分析音频，也不得用音高证据替换
制谱者给定的音符目标。

## Vocal chart ｜ 人声谱面

The vocal chart media type is
`application/vnd.uta.vocal-chart+json;version=1`. The current chart version
is 1.1. Readers accept any 1.x chart and MUST ignore members introduced by a
newer minor version; new minor versions MUST only add ignorable members.

人声谱面的媒体类型为 `application/vnd.uta.vocal-chart+json;version=1`。当前谱面
版本为 1.1。读取方接受任何 1.x 谱面，且必须忽略更新的次版本引入的成员；新的
次版本只允许添加可忽略的成员。

The hierarchy is fixed: track → phrase → note. **A phrase is one displayed
lyric line**; phrases do not nest. A note owns its lyric tokens, so derived
phrase text is never duplicated. Separate tracks represent lead, harmony,
backing, or ad-lib parts.

层级是固定的：track → phrase → note。**一个 phrase 就是一行显示歌词**；phrase
不嵌套。音符持有自己的歌词 token，因此派生的整句文本永远不需要重复存储。不同
track 表示主唱、和声、伴唱或即兴声部。

Every note MUST carry at least one lyric token. A text lyric token has a
stable chart-local ID (at most 64 bytes), Unicode text, and an explicit join
policy. A continuation token references a text token instead of encoding a
magic `~` string. References MUST resolve inside the same track.

每个音符必须至少携带一个歌词 token。文本歌词 token 拥有稳定的谱面局部 ID
（至多 64 字节）、Unicode 文本和显式的拼接策略。延音 token 引用一个文本 token，
而不是编码魔法字符串 `~`。引用必须在同一 track 内可解析。

Optional `reading` and `phonemes` preserve pronunciation without changing
display text. `reading` uses the language's conventional phonetic script:
hiragana for Japanese, pinyin with tone marks for Mandarin. `phonemes` is an
IPA transcription with segments separated by spaces.

可选的 `reading` 与 `phonemes` 在不改变显示文本的前提下保留发音信息。`reading`
使用该语言约定俗成的注音方案：日语用平假名，普通话用带声调的拼音。`phonemes`
使用 IPA，音段之间以空格分隔。

Pitched, rap, spoken, and freestyle vocal modes are independent from normal
or golden bonus state. Scoring intent is explicit:

- `pitch` compares the singer with the authored MIDI target;
- `rhythm` scores temporal participation without inventing a pitch target;
- `none` displays the lyric and note but does not score it.

pitched、rap、spoken、freestyle 四种演唱模式与 normal/golden 加分状态相互独立。
打分意图是显式的：

- `pitch` 将歌手与制谱的 MIDI 目标比较；
- `rhythm` 只按时间参与度打分，不虚构音高目标；
- `none` 只显示歌词和音符，不打分。

`pitch` MAY be null for non-pitch scoring. A note using pitch scoring MUST
have a pitch target. MIDI is an integer from 0 through 127; `cents` is an
optional integer offset from -99 through 99.

非音高打分的音符其 `pitch` 可以为 null。使用音高打分的音符必须有音高目标。MIDI
为 0 到 127 的整数；`cents` 为 -99 到 99 的可选整数偏移。

Within a track, phrases and notes MUST be time ordered and MUST NOT overlap.
Simultaneous lead/harmony content belongs in separate tracks.

同一 track 内，phrase 与音符必须按时间排序且不得重叠。同时发声的主唱/和声内容
放在不同的 track 中。

## Duet parts ｜ 对唱声部

A track MAY carry a `part` number counted from 1, following the UltraStar
`P1`/`P2` convention. `part: 1` belongs to player one, `part: 2` to player
two, and so on up to 9. Assigned part numbers MUST form a contiguous set
starting at 1 — a chart with parts 1 and 3 but no part 2 is invalid. A track
without `part` is not assigned to a specific player (typical for backing or
ad-lib tracks).

track 可以携带从 1 开始计数的 `part` 编号，遵循 UltraStar 的 `P1`/`P2` 约定。
`part: 1` 属于一号玩家，`part: 2` 属于二号玩家，依此类推至 9。已分配的 part
编号必须构成从 1 开始的连续集合——只有 part 1 和 3、缺少 part 2 的谱面非法。
没有 `part` 的 track 不归属特定玩家（伴唱、即兴 track 通常如此）。

Sections sung by everyone are duplicated into each part's tracks. There is
no "both" marker; UltraStar's legacy `P3` notation demonstrated why such a
marker should not exist. The `singer` field records the display name of the
original performer of a part, mirroring UltraStar's `#P1`/`#P2` headers.

所有人齐唱的段落将音符复制进每个 part 的 track。不存在"both"标记——UltraStar
遗留的 `P3` 写法已经证明这种标记不该存在。`singer` 字段记录该声部原唱者的显示
名称，对应 UltraStar 的 `#P1`/`#P2` 头部。

A chart is a duet exactly when it assigns two or more distinct parts. Track
`role` stays purely musical: lead, harmony, backing, or ad-lib.

当且仅当谱面分配了两个及以上不同 part 时，它才是对唱谱。track 的 `role` 保持
纯音乐语义：lead、harmony、backing 或 adlib。

## Library metadata ｜ 曲库元数据

`song` carries optional library fields consumers can rely on:

- `preview_start_seconds` — instrumental time where song-select preview
  playback should begin, typically the hook;
- `title_sort` / `artist_sort` — collation keys (kana reading for Japanese,
  pinyin for Chinese, article-stripped text for English);
- `genre`, `year`, `creator` (chart author, not recording artist), `tags`;
- `audio.loudness` — advisory EBU R 128 integrated loudness per stem
  (LUFS, -70 to 0) so players can gain-match songs and balance guide vocals
  against the instrumental.

`song` 携带消费者可以依赖的可选曲库字段：

- `preview_start_seconds` —— 选歌预览的起播器乐时间，通常是副歌；
- `title_sort` / `artist_sort` —— 排序键（日语用假名读音，中文用拼音，英语去除
  冠词后的文本）；
- `genre`、`year`、`creator`（制谱者，不是原唱歌手）、`tags`；
- `audio.loudness` —— 每条音轨的建议性 EBU R 128 积分响度（LUFS，-70 到 0），
  供播放器做歌曲间音量匹配及导唱与器乐的平衡。

## Scoring hints ｜ 打分提示

`scoring` is optional and advisory. It names the scoring engine the chart
was authored against so a matching consumer can pick identical parameters. A
consumer MUST NOT reject a package because it does not recognize the engine;
scoring semantics that a consumer must understand belong in
`required_features` instead.

`scoring` 是可选且建议性的。它记录制谱时参照的打分引擎，使匹配的消费者可以选用
一致的参数。消费者不得因为不认识该引擎而拒绝包；消费者必须理解的打分语义应放进
`required_features`。

## Pitch evidence ｜ 音高证据

Pitch evidence is optional and uses media type
`application/vnd.uta.pitch-evidence+json;version=1` with
`pitch-evidence-v1.schema.json`.

音高证据是可选的，媒体类型为
`application/vnd.uta.pitch-evidence+json;version=1`，模式为
`pitch-evidence-v1.schema.json`。

Evidence stores a fixed-hop frequency and confidence series plus optional
model provenance. `null` frequency represents an unvoiced frame. Evidence is
an editor aid and legacy visualization source; it is never the scoring chart.
A future tempo-map extension will have the same standing: an editing aid,
never a second source of note timing.

证据存储固定步长的频率与置信度序列，外加可选的模型来源信息。`null` 频率表示
清音帧。证据是编辑器辅助与旧版可视化数据源，永远不是打分谱面。将来的节拍图
扩展也将处于同等地位：编辑辅助，绝不构成音符时间的第二来源。

## Feature negotiation ｜ 特性协商

`required_features` lists semantics that a consumer must understand to use
the package correctly. A consumer MUST reject a package containing an
unknown required feature. `optional_features` may be ignored. Feature names
are lowercase slash-version identifiers such as `vocal-chart/1`; the version
component is the major version only, because minor revisions are ignorable
by construction.

`required_features` 列出消费者要正确使用该包必须理解的语义。消费者必须拒绝含有
未知必需特性的包。`optional_features` 可以忽略。特性名是小写的"名称/版本"标识
符，如 `vocal-chart/1`；版本部分只写主版本号，因为次版本修订按构造即可忽略。

This boundary prevents an older game from silently treating rhythm or future
note semantics as ordinary pitch notes. File hashes, source-control
versions, database constraints, and ordinary unit tests cannot provide this
protection because the failure occurs when an independently versioned
consumer opens a valid package.

这道边界防止旧版游戏把节奏型或未来的音符语义悄悄当作普通音高音符处理。文件
哈希、版本控制、数据库约束和普通单元测试都提供不了这种保护，因为故障发生在
一个独立发版的消费者打开一个合法包的时刻。

## Versioning and UTZ 0.1 compatibility ｜ 版本策略与 UTZ 0.1 兼容

UTZ 0.2 readers SHOULD also accept 0.1 packages. Producers may expose a 0.1
compatibility export that derives `transcript`, `pitch_track`, and
`pitch_notes` from the 0.2 authoring model. Conversion MUST write a new
package and MUST NOT modify an existing user package in place.

UTZ 0.2 读取方应当同时接受 0.1 包。制作方可以提供 0.1 兼容导出，从 0.2 创作模型
派生 `transcript`、`pitch_track` 和 `pitch_notes`。转换必须写出新包，不得就地
修改用户已有的包。

The `format_version` remains semantic-version shaped. Development-line
readers accept only minor versions they explicitly understand. After format
1.0, readers reject unsupported major versions and may accept compatible
minor and patch revisions.

`format_version` 保持语义化版本形态。开发线阶段的读取方只接受其明确理解的次
版本。格式 1.0 之后，读取方拒绝不支持的主版本，可以接受兼容的次版本与补丁
修订。

## Media guidance ｜ 媒体建议

The manifest MIME type, not a filename extension guess, is authoritative.
Producers should prefer Opus in Ogg or MP3 for broadly decodable audio,
WebP/JPEG/PNG for covers, and H.264/AAC MP4 or WebM for optional video.

清单中的 MIME 类型是权威依据，而不是靠文件扩展名猜测。制作方应优先选用 Ogg 封装
的 Opus 或 MP3 作为广泛可解码的音频，WebP/JPEG/PNG 作为封面，H.264/AAC MP4 或
WebM 作为可选视频。

## Conformance fixtures ｜ 一致性样例

`format/fixtures/` contains documents every implementation must agree on:
`valid/` documents MUST parse and validate; `invalid/` documents MUST be
rejected. Manifest fixtures exercise structural rules only — asset hashes in
them are placeholders and are checked at the package level, not here. The
reference test suite (`tests/conformance.rs`) runs the full set.

`format/fixtures/` 包含所有实现必须达成一致的文档：`valid/` 下的文档必须解析并
通过校验；`invalid/` 下的文档必须被拒绝。清单类样例只检验结构规则——其中的资产
哈希是占位符，真实校验发生在包级别而非此处。参考测试套件
（`tests/conformance.rs`）会运行完整样例集。

## Copyright and provenance ｜ 版权与来源

The format does not grant permission to redistribute included media.
Producers should record source and rights notes in `provenance`;
distribution systems may apply stricter policies.

本格式不授予再分发所含媒体的权利。制作方应在 `provenance` 中记录来源与权利
说明；分发系统可以施加更严格的政策。
