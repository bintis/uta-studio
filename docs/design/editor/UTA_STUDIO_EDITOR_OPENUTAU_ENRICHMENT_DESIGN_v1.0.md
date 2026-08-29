# Uta! Studio Editor × OpenUtau 借鉴设计 — v1.0

**文档版本：** v1.0
**日期：** 2026-08-27
**状态：** Implemented in current source; focused automated tests pass; manual running-UI interaction review recommended before release handoff
**代码审计基线：** `bintis/uta-studio@feature/split-audio-model-studio`
**范围边界：** 仅 `app-core/src/editor/**` 与 `desktop/src/studio/editor/**`。不得修改 `vendor/utz/**`、`analysis-engine/**`，或 `app-core/src/`、`desktop/src/studio/` 下的任何其他文件。
**编辑器权威设计：** `docs/design/editor/UTA_STUDIO_EDITOR_INTEGRATION_DESIGN_v1.0.md`
**配套执行清单：** `docs/design/editor/UTA_STUDIO_EDITOR_OPENUTAU_ENRICHMENT_TODO_v1.0.md`

---

# 1. 结论

研究 [OpenUtau](https://github.com/openutau/OpenUtau)（UTAU 兼容的语音合成 piano-roll 编辑器）后，找到四个可以在**不触碰编辑器以外任何代码**的前提下落地的增强点。它们不是照搬 OpenUtau 的功能，而是把它验证过的编辑体验，套进 Uta! Studio 自己的数据模型和产品定位——一个**评分/导出用的歌唱谱面authoring 工具**，不是语音合成器。

四个特性中有三个是"把已经写好但从未接上的管线接通"，而不是从零发明新架构：

1. **证据驱动的建议（headline）**——建议系统的数据结构、应用逻辑、undo 接入、Accept/Ignore 按钮全部已经存在且正确，唯一缺的是从来没有任何代码构造过一个 `EditorSuggestion`。
2. **歌词读音（reading）覆写**——格式里早就有 `reading` 字段，`syllabize.rs` 也在用它，但编辑器从未把它暴露给用户去纠正。
3. **Technique 证据点详情**——STARS technique 证据（vibrato/glissando/falsetto 等）已经作为只读 chip 渲染在时间轴上，只是不可点击查看具体分数。
4. **歌词快速连续输入 + 内联读音显示**——用户明确指出当前歌词编辑"太弱小"；根因是逐词双击编辑、没有任何键盘链式录入路径，`Tab` 目前只绑定到"选中下一个音符"而非"提交并跳到下一个歌词槽位"，这正是 OpenUtau 最基础的工作流（双击开始输入、Tab 提交并跳下一个）。

---

# 2. OpenUtau 研究摘要

未 clone 仓库（plan mode 禁止在 plan 文件之外写文件）；改用 GitHub 只读 API（`gh api repos/openutau/OpenUtau/contents/...`）、[OpenUtau Wiki](https://github.com/openutau/OpenUtau/wiki/Getting-Started) 与两篇 DeepWiki 架构页做研究。两个项目代码零共享（Rust/Bevy vs. C#/Avalonia），本来就是产品/体验层面的借鉴，不是移植。

OpenUtau 确认存在的编辑器能力：

```text
Pen 工具单手势创建+调整音符大小（Alt 拖拽同时影响相邻音符）
Vibrato 编辑器（图标切换开关 + 拖拽调整深度/频率/相位）
Pitchbend 曲线编辑（点击加点、拖拽移动、右键选形状）
Phoneme timing 拖拽手柄（preutter/overlap/offset）
Transformers 批量编辑菜单（AutoLegato、Transpose、QuantizeNotes、FixOverlap、BakePitch）
Note 级 expression 参数（dyn/pitd/clr/vel/vol/atk/dec/gen/bre）
双击开始输入歌词，Tab 提交并跳到下一个音符
```

**为什么大部分不能直接搬：** OpenUtau 的核心是驱动一个 resampler 去合成音频，所以它大量功能是"如何让合成结果好听"（phoneme timing 决定 resampler 何时触发辅音、pitchbend 曲线决定音高过渡的平滑度、expression 参数是 resampler 的调音旋钮）。Uta! Studio **不合成音频**，Editor 产出的是给评分/UltraStar 导出用的谱面（离散音符 + 歌词 + 时间），格式里根本没有连续 pitch 曲线、没有 resampler 相关参数。把这些概念硬套进来意味着要给 `vendor/utz` 加字段——直接违反"只改编辑器"的约束，所以被过滤掉了。

真正能落地的，是筛选后剩下的、和"人工谱面校对"这件事本质相关的体验：**模型给出候选、人一键采纳或忽略**（对应 BakePitch 的精神）、**纠正模型识别错的文字/读音**（对应 phoneme override 的精神）、**快速连续输入**（OpenUtau 最基础也最被验证过的工作流）。

---

# 3. 贯穿全部特性的边界

- **允许改动：** `app-core/src/editor/**`、`desktop/src/studio/editor/**`。
- **禁止改动：** `vendor/utz/**`（共享谱面格式）、`analysis-engine/**`，以及 `app-core/src/`、`desktop/src/studio/` 下除 editor 子目录外的任何文件——哪怕只是加一个看起来无害的 helper。可以**使用**这些地方已经导出的公共类型/函数，不能**编辑**它们。
- **文件行数上限：** 仓库规则要求单文件 ≤2000 行。`desktop/src/studio/editor/actions.rs` 现在 1605/2000 行（约 80%），是唯一真正紧张的文件。四个特性的设计都刻意绕开了它——没有一个需要新增 `EditorAction` 变体。
- **核心原则**（引自 `UTA_STUDIO_EDITOR_INTEGRATION_DESIGN_v1.0.md`）：*"模型负责提出最好的解释；编辑器负责让用户拥有最后决定权。"* 四个特性都只通过一次显式的、进入 undo 历史的用户操作来修改文档，不存在任何自动生效的路径。

## 3.1 三条改变了具体接线方式的关键发现

这三条不是"锦上添花"的备注，而是直接决定了下面每个特性该怎么写：

1. **`app-core/src/lib.rs` 用的是逐个具名的 re-export 列表，不是 glob。** 任何在 `app-core/src/editor/**` 下新增的**自由函数**，desktop 侧都看不到，除非把它加进 `lib.rs` 的具名列表——但 `lib.rs` 是 crate root，在允许改动的范围之外。**所以本设计里 desktop 需要调用的新逻辑，一律做成 `EditorDocument` 的方法**（`EditorDocument` 类型本身已经被导出，方法不需要单独 re-export），从不新增自由函数。
2. **Bevy 的 system 注册全部集中在 `desktop/src/studio/startup.rs`**，同样在允许范围之外。**所以本设计不新增任何 Bevy system 注册**——新的响应式逻辑要么塞进已经注册过的既有 system（`sync_editor_word_input`、`finish_inline_lyric_edit`），要么用 Bevy 的 `.observe()`（挂在 spawn 时的实体上，不需要系统注册）。
3. **`desktop/src/studio/editor/input.rs` 里的 `handle_editor_pointer_capture` 已经有 16 个 system 参数**——从代码本身能看出这已经是作者事实上的上限（作者已经把两个 resource 合并成一个 `#[derive(SystemParam)]` struct，专门为了不超过 16 个）。Technique chip 点击这个特性因此**没有**往这个函数里加参数，而是改用 `.observe()`，彻底绕开这个约束。

---

# 4. Feature 1（headline）—— 证据驱动的建议

## 4.1 现状

`app-core/src/editor/suggestions.rs` 已经定义好：

```rust
pub enum EditorSuggestionKind {
    ChangePitch { note_index: usize, midi: f64 },
    MoveBoundary { note_index: usize, start: f64, end: f64 },
    BindLyric { lyric: LyricAddress, note_index: usize },
    ChangeTrackRole { track_index: usize, role: TrackRole },
    InspectEvidence,
}

pub struct EditorSuggestion {
    pub id: String,
    pub start: f64,
    pub end: f64,
    pub confidence: f32,
    pub suggestion: EditorSuggestionKind,
    pub evidence_refs: Vec<ArtifactRef>,
}
```

`apply_editor_suggestion(...)` 是一个已经写好、会正确参与 undo 历史的 mutation dispatcher。desktop 侧的 Accept/Ignore 按钮（`desktop/src/studio/actions_content.rs:1058-1096`）也早就接好了，而且逻辑是对的：接受前先 `checkpoint`，返回 no-op/失败就把 checkpoint 弹掉，忽略时只做 `.retain()`，绝不触碰文档。这条链路满足设计文档 §23 里"suggestion accept is undoable"和"suggestion ignore does not mutate chart"两条测试要求——**已经满足，不用动**。

全仓库搜索 `EditorSuggestion{`/`EditorSuggestionKind::` 的构造点：**除了 `apply_editor_suggestion` 自己的 `match` 分支，没有任何地方构造过一个 `EditorSuggestion`。** 按钮因此永远拿到空 vec，永远是死的。

与此同时，Review Region（`ReviewRegion`，来自分析证据的 pitch/boundary 分歧标记）已经在正常流转，而且已经可以通过既有的 Prev/Next 工具栏按钮导航——这条链路是活的，不是本特性要修的东西。

## 4.2 设计

在 `suggestions.rs` 新增一个纯函数方法：

```rust
impl EditorDocument {
    /// 只读地把已经存在的证据 disagreement 投影成用户可以一键采纳/忽略的建议。
    /// 不重新实现任何 Fusion/分析逻辑——只是拿现成的证据数字去和现成的音符数字比较。
    pub fn derive_evidence_suggestions(
        &self,
        evidence: &SingingEvidenceBundle,
    ) -> Vec<EditorSuggestion> {
        let Some(fused_f0) = evidence
            .tracks
            .iter()
            .find(|t| t.kind == EvidenceKind::FusedF0)
        else {
            return Vec::new();
        };
        let notes = self.notes();
        evidence
            .review_regions
            .iter()
            .filter(|r| !r.reviewed && r.confidence >= MIN_REGION_CONFIDENCE)
            .filter(|r| r.reasons.contains(&ReviewReason::PitchDisagreement))
            .filter_map(|region| {
                let note = notes
                    .iter()
                    .find(|n| n.pitched && n.start < region.end && n.end > region.start)?;
                let suggested = median_evidence_midi(fused_f0, region.start, region.end)?;
                (suggested.round() != note.midi.round()).then(|| EditorSuggestion {
                    id: format!("evidence-pitch-{}-{}", region.id, note.index),
                    start: region.start,
                    end: region.end,
                    confidence: region.confidence,
                    suggestion: EditorSuggestionKind::ChangePitch {
                        note_index: note.index,
                        midi: suggested.round(),
                    },
                    evidence_refs: region.evidence_refs.clone(),
                })
            })
            .collect()
    }
}

/// 假设，尚未在本仓库中被任何 producer 证实（见下方"待确认事项"）：
/// FusedF0 的 EvidencePoint.value 是 Hz。
fn median_evidence_midi(track: &EvidenceTrack, start: f64, end: f64) -> Option<f64> {
    let mut midis: Vec<f64> = track
        .points
        .iter()
        .filter(|p| p.time >= start && p.time <= end)
        .filter_map(|p| {
            let hz = f64::from(p.value);
            (hz.is_finite() && hz > 0.0).then(|| 69.0 + 12.0 * (hz / 440.0).log2())
        })
        .collect();
    if midis.is_empty() {
        return None;
    }
    midis.sort_by(f64::total_cmp);
    Some(midis[midis.len() / 2])
}
```

`ChartNote.midi` 与 `EditorSuggestionKind::ChangePitch.midi` 均已在源码中确认为 `f64`（本设计定稿前直接读取 `document/types.rs`、`suggestions.rs` 验证过，不是猜测），上面的类型是对的。

**为什么放在 `EditorDocument` 的方法里而不是自由函数：** 见 §3.1 第 1 条。
**为什么放进 `suggestions.rs` 而不是 `document/*.rs`：** 它是 `apply_editor_suggestion` 的读侧对应物，且需要 `evidence.rs` 里的 `SingingEvidenceBundle`/`EvidenceKind`/`EvidenceTrack`/`ReviewReason`——证据到建议的推导逻辑放在同一个文件比拆开更内聚。

### 取舍与理由

- **只处理 `PitchDisagreement → ChangePitch`**，`BoundaryDisagreement → MoveBoundary` **明确不做**：没有一条"融合后的"边界信号可采样，只有三条互相打架的原始模型边界（`GameBoundary`/`QwenWordBoundary`/`FireRedWordBoundary`）。要在三者间选一个，等于自己动手做 Fusion 该做的事，越界。其余每一种 `ReviewReason`（`VoicingConflict`/`LeadHarmonyLeak`/`TechniqueAmbiguous`/`WordNoteMismatch`/`LyricBoundaryLowConfidence`/`LowConfidence`）都交给已经在工作的 Review Region Prev/Next 流程，不强行编一个低置信度的建议出来。
- 已经被标记 `reviewed` 的区域跳过——用户已经用 Mark Reviewed 处理过的东西再给一次建议是噪音。
- 建议的音高吸附到最近的整数半音，且仅在与当前音符**取整后**的音高不同才给出——置信门槛是"取整后不一样"，不是一个可调的任意 delta。
- 用区间内证据点的**中位数**（不是均值），对离群点更稳健。
- `evidence_refs` 直接复用 `ReviewRegion.evidence_refs`——溯源数据已经现成，不需要额外接线。

## 4.3 接线

在 `desktop/src/studio/editor/audition.rs` 的 `finish_native_editor_load` 里，证据加载完成之后加一行：

```rust
editor.suggestions = editor.document.derive_evidence_suggestions(&editor.evidence);
```

这一个调用点同时覆盖 `load_native_editor` 和 `start_editor_merge_load_job`（两者都会走到 `finish_native_editor_load`）。可以无条件调用——旧谱面/空 evidence bundle 只会得到空 vec，顺带满足设计文档 §23 里"老谱面在没有 EvidenceBundle 时也能正常打开"的测试要求。

**为什么 `chrome.rs` 不需要改：** 工具栏 `editor.suggestions.first()` 的门控逻辑已经是"一次处理一个"——Accept/Ignore 都会把已处理的建议从 vec 前端移除，下一次 `.first()` 自然显示下一个。不存在需要额外补的导航缺口。

## 4.4 待确认事项（不是缺陷，是明确标注的假设）

`EvidencePoint.value` 在 `FusedF0` 轨道里到底是不是 Hz，本仓库目前无法证实——搜索全仓库（含 `analysis-engine`），`FusedF0` 只出现在 `evidence.rs`/`state.rs` 两处已知位置，**目前没有任何代码生产这种证据**。本设计按照"与仓库里其他 Hz→MIDI 转换一致、符合 F0 的通用惯例"做了一个合理假设，但明确标注为待验证——一旦真正的 producer 出现，需要回来确认这一个函数的假设是否成立。

---

# 5. Feature 2 —— 歌词读音（reading）覆写

## 5.1 现状

`vendor/utz`（只读，不可修改）里的 `LyricTextToken` 早就有：

```rust
pub struct LyricTextToken {
    pub id: String,
    pub text: String,
    pub join_before: LyricJoin,
    pub reading: Option<String>,
    pub phonemes: Option<String>,
}
```

`reading` 是对齐器（aligner）识别出的假名读音，`app-core/src/editor/syllabize.rs` 靠它把汉字正确切分成 mora（`japanese_syllables(text, reading)`）。`syllabize.rs` 自己的文档注释就承认这套启发式"会在外来语和人名上出错"——而一个识别错的读音（常见于多音字、人名）会直接连锁产生错误的音节切分，且用户目前**没有任何办法纠正它**。

## 5.2 设计

**新字段：** `app-core/src/editor/document/types.rs` 的 `ChartLyric` 加一个 `pub reading: Option<String>`；在 `app-core/src/editor/document/notes.rs` 的 `track_lyrics()` 里，`ChartLyric { ... }` 字面量加一行 `reading: token.reading.clone(),` 完成填充（`track_lyrics` 尽管处理的是歌词，实际定义在 `notes.rs` 而非 `lyrics.rs`——这是执行时容易踩的一个小坑，已在 TODO 里标出）。

**新 setter**（`app-core/src/editor/document/lyrics.rs`，完全照抄 `set_lyric_text` 的形状）：

```rust
pub fn set_lyric_reading(&mut self, address: LyricAddress, reading: Option<String>) -> bool {
    let Some(token) = self.token_mut(address) else {
        return false;
    };
    let reading = reading.filter(|value| !value.trim().is_empty());
    if token.reading == reading {
        return false;
    }
    token.reading = reading;
    self.touch();
    true
}
```

**是否应该自动重新分音节？** 设计取舍是：**只存不联动。** 理由：(a) 与本 crate 现有分工完全一致——`set_lyric_text` 重打字时也会清空 `reading`/`phonemes`，但从不自动重新分音节，分音节永远是用户显式触发的独立操作 `syllabize_lyrics`；(b) 自动联动意味着用户刚改完一个读音的 typo，笔画间的音符边界就被静默重新切分——正是设计原则里"不得静默覆盖人工内容"要防的事；(c) 保持这是一个纯粹、单一职责、容易测试的方法。

## 5.3 显示门控：按"这个词"而不是按"这份谱面的语言"

新的 Inspector 输入框只在**这个具体的词**含有 CJK 字符（汉字/假名/谚文）时才显示，而不是按谱面整体的 `language` 字段门控。新增方法：

```rust
/// 判断 address 处的歌词是否含有 syllables() 会当作 CJK 处理的字符——
/// 也就是存储的 reading 真正会影响分音节结果的那些脚本。按词判断，
/// 不按谱面语言判断：一份标记为拉丁语言的谱面仍可能夹杂个别 CJK
/// 外来词（syllables() 自己的混合脚本处理就证明了这一点），这正是
/// 这个字段最该出现的场景。
pub fn lyric_uses_cjk_script(&self, address: LyricAddress) -> bool {
    self.lyric_text(address)
        .is_some_and(|text| text.chars().any(|c| is_han(c) || is_kana(c) || is_hangul(c)))
}
```

`is_han`/`is_kana`/`is_hangul` 目前是 `syllabize.rs` 里的私有函数，只需要把可见性提升到 `pub(crate)`（`app-core` crate 内部可见，因为 Rust 的模块可见性按祖先/后代关系判断，不按"是否同级文件"判断，所以不需要碰 `mod.rs`/`lib.rs` 的 re-export）。

## 5.4 Desktop 接线

在 Inspector 的单词分支（`desktop/src/studio/editor/panels.rs`，现有的音节文本框旁边）新增一个受 `lyric_uses_cjk_script` 门控的 `EditableText` 输入框；提交路径复用已经注册好的 `sync_editor_word_input`（`input.rs`），多加一个 query 分支即可——不需要新 system，不需要动 `startup.rs`。

---

# 6. Feature 3（最小）—— Technique 证据点详情

## 6.1 现状

STARS technique 证据（`bubble`/`breathe`/`pharyngeal`/`vibrato`/`glissando`/`mixed`/`falsetto`/`weak`/`strong` 九分类——这是本仓库自己的 task 追踪里刚刚 `READY` 的能力）已经作为只读文字 chip 渲染在 `desktop/src/studio/editor/view/timeline.rs` 的时间轴上，但完全不可交互——看不到 chip 背后具体的分数。

## 6.2 设计

用 Bevy 的 `.observe(On<Pointer<Click>>)`（歌词行空白区域点击已经在用的同一种模式）让每个 chip 可点击，而不是走 `Interaction`/`Button` 的轮询路径——理由见 §3.1 第 3 条（`handle_editor_pointer_capture` 已经 16 个参数封顶）。

点击后把该点的 flat index 存进新字段：

```rust
// desktop/src/studio/editor/state.rs, NativeEditor 上新增
pub(crate) selected_technique_point: Option<usize>,
```

在 Inspector 新增一个**全新的**详情区块（已确认：Inspector 目前没有任何通用的"证据详情"区域可以复用），只读展示：分类标签、原始分数、本代码库其他地方已经在用的"uncalibrated"措辞、以及一个**时间戳**——不是时间区间，因为 `EvidencePoint` 根本没有区间起止可以展示（wire 格式里的区间在 `technique_evidence_track()` 里就已经被折叠成中点了；给共享的 `EvidencePoint` 加区间字段属于影响面很广的 schema 改动，且目前没有任何 producer 会填这些字段，无论如何都超出范围）。

这个特性不新增任何 mutation 路径。"technique 证据永远不会创建、拆分或移动 MIDI 音符"这条既有注释里写明的不变量，原样保留。

---

# 7. Feature 4 —— 歌词快速连续输入 + 内联读音显示

这是用户在本次设计过程中**明确追加、并要求同等重视**的一项："现在的歌词编辑太弱小了"。

## 7.1 现状与根因

编辑一个歌词目前必须双击进入内联编辑，且没有任何"提交并跳到下一个"的路径。`Tab` 目前只绑定到 `select_next_note`/`select_previous_note`（**选中**，不是**编辑**）——这正是 OpenUtau 最基础的工作流要消除的那种摩擦（"双击开始输入歌词，Tab 提交并切换到下一个音符"）。

**需要先排除的一个误判：** 底层文本输入控件本身没问题。`EditableText` 是 Bevy 引擎自带的 widget（来自 `bevy_ui_widgets`/`bevy_text`，通过 `PreeditCursor` 支持 IME 组字），不是本应用写的代码。问题完全在于这个应用自己的输入处理逻辑里缺一条"链式跳转"的路，跟文本框素质无关，也不需要（不允许）去改 `EditableText` 本身。

## 7.2 设计

新方法，完全复用已有的 crate 内部 helper，不重新发明"下一个词槽位在哪"的逻辑：

```rust
/// 把内联歌词编辑推进到同一 phrase 里下一个（forward）或上一个 eligible
/// 槽位；如果目标音符还没有歌词，就地新建一个空的（复用
/// add_lyric_to_note 的机制）。到达 phrase 首/尾时返回 None——Tab 不会
/// 跨越 phrase 边界，让"换行"始终是一个用户能看见的、主动的步骤，而
/// 不是一次静默的跳转。
pub fn advance_lyric_edit(&mut self, from: LyricAddress, forward: bool) -> Option<LyricAddress> {
    let note_index = self.resolve(from)?;
    let (phrase, offset) = self.locate_note(note_index)?;
    let slots = self.lyric_slots(phrase);
    let position = slots.iter().position(|candidate| *candidate == offset)?;
    let next_offset = if forward {
        slots.get(position + 1).copied()?
    } else {
        slots.get(position.checked_sub(1)?).copied()?
    };
    let range = self.phrase_flat_range(phrase)?;
    let target_note = range.start + next_offset;
    self.address_of_note(target_note)
        .or_else(|| self.add_lyric_to_note(target_note))
}
```

`resolve`/`locate_note`/`lyric_slots`/`phrase_flat_range`/`address_of_note`/`add_lyric_to_note` 全部已经存在（`pub(crate)` 或 `pub`），因此天然会跳过 continuation-only 的音符——不需要重新写一遍"哪些位置算真正的词槽位"。

## 7.3 Desktop 接线

扩展已经注册好的 `finish_inline_lyric_edit`（`input.rs`），在既有的 Enter/Escape 分支之外加第三个 Tab 分支：Tab 提交并前进，Shift+Tab 后退；**先检查 `EditableText::is_composing()`，组字中直接放行不拦截**（已经确认 Bevy 自带 widget 在 IME 组字期间会主动把 Tab 让给 IME 自己处理——如果提前抢走会打断日语输入的组字过程）。

**不会和全局的"Tab 选中下一个音符"冲突：** `handle_editor_keyboard` 本来就会在任何 `EditableText` 拿到焦点时提前 return，两个 Tab 处理逻辑在结构上互斥，不需要调整调度顺序。

**只在歌词行的内联输入框生效，不含 Inspector 里的单词输入框：** 这不是简化后的妥协，而是确认了代码里已经存在的一个刻意区分——只有行内输入框携带 Enter/Escape 处理逻辑所依赖的那个 marker 组件，Inspector 里的输入框本来就不参与这套逻辑。

## 7.4 配套：内联读音显示

一旦 Feature 2 的 `reading` 字段存在，就在歌词行里（`view/menus.rs` 的 `spawn_editor_lyrics`）把它显示成一行小号的、furigana 风格的副行——纯展示，不新增任何交互。这样一个识别错的读音在**扫视整首歌**的时候就能看见，而不是只能在 Inspector 里一个词一个词地发现。

## 7.5 明确排除的范围（已核实不是缺口，不是"没做完"）

- 重写文本输入控件——它是 Bevy 引擎组件，不在改动范围内也没必要。
- 歌词范围内的查找/替换。
- 整行重打字时词数与音符数不匹配的"智能"处理——**已核实现有实现已经优雅处理**：多出来的词会堆到最后一个音符上而不是消失（`document/lyrics.rs` 里已有的文档注释明确写着"retyping a longer line never silently drops words"）。

---

# 8. 四个特性之间的依赖关系

四个特性基本互相独立，只有一处真实耦合：**Feature 4 的内联读音显示依赖 Feature 2 的 `ChartLyric.reading` 字段先存在**（`ChartLyricView`/`chart_lyrics()` 和渲染都需要读它）。Feature 4 里 Tab 链式输入的那一半没有这个依赖。

推荐实现顺序：**Feature 1 → Feature 3 → Feature 2 → Feature 4**（headline 优先；Feature 3 完全独立且最小；Feature 2 先于 Feature 4 满足唯一的真实依赖；Feature 4 最后，因为它是最新追加的，涉及文件也最多）。

---

# 9. 测试约定（已核实，非假设）

- **`app-core/src/editor/**`**：这里每个被改动的文件都已经有很重的 `#[cfg(test)] mod tests` 覆盖（`document/tests.rs`、`evidence.rs`、`syllabize.rs` 皆是如此）——新增的每一个纯方法都应该照着所在文件已有的测试风格补测试。
- **`desktop/src/studio/editor/**`**：约定明显更窄——只有 `action_input.rs`（4 个纯 chord 匹配测试）和 `state.rs`（1 个纯节流计时测试）有 `#[test]`，没有任何文件对一个真实运行的 Bevy `App`/schedule 做测试。**本设计里新增的 desktop 侧胶水代码（`input.rs`/`timeline.rs`/`panels.rs`/`menus.rs` 的改动）都不适合补单元测试**——它们都是 `ResMut<EditorUiState>`/`Query` 形状的 Bevy system body，这也正是既有约定不去测这一层的原因。四个特性的正确性最终落在"app-core 方法有测试 + 用 `run` skill 跑一遍手动验证"上，不要为了测试而发明一套这个代码库里本来就没有的 Bevy 测试基础设施。

---

# 10. 未纳入本次范围的事项

- `docs/design/README.md` 的 "Editor" 小节已加入本文档和配套完成清单；该 discoverability 收尾已完成。
- OpenUtau 的 pitch-bend 曲线编辑、vibrato 曲线编辑、phoneme timing 手柄、expression 参数系统——均因需要给 `vendor/utz` 加字段（连续曲线数据、resampler 相关参数）而被排除，不属于"只改编辑器"能覆盖的范围，也不符合本工具"不合成音频"的产品定位。
