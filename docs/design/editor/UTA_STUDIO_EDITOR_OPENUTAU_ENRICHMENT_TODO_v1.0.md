# Uta! Studio Editor × OpenUtau 借鉴 — 执行 TODO v1.0

**Current state (2026-08-28):** implementation checklist complete in current source; focused app-core/Desktop automated suites pass. A manual running-UI pass for the four interaction paths is still recommended before release handoff, but it is no longer an implementation TODO.

**配套设计文档：** `docs/design/editor/UTA_STUDIO_EDITOR_OPENUTAU_ENRICHMENT_DESIGN_v1.0.md`（每个改动的背景/取舍原因都在那里，本文件只列具体动作，不重复讲道理）
**给执行 agent 的第一句话：** 你只能改 `app-core/src/editor/**` 和 `desktop/src/studio/editor/**` 下的文件。清单里每一项都已经标好绝对路径；如果某一步看起来需要碰这两个目录之外的文件，先停下来重新读一遍设计文档对应小节，而不是直接改。
**推荐实现顺序：** Feature 1 → Feature 3 → Feature 2 → Feature 4（Feature 4 依赖 Feature 2 的 `reading` 字段，其余相互独立）。

---

## 全局"不要碰"清单

不管做哪个 feature，以下文件都不应该被这份 TODO 逼着去改：

- `vendor/utz/**` —— 共享谱面格式，只读引用。
- `analysis-engine/**`
- `app-core/src/lib.rs` —— 所有新逻辑都做成 `EditorDocument` 的方法，不新增自由函数，所以这个具名 re-export 列表不需要动。
- `app-core/src/editor/mod.rs` —— 同理。
- `desktop/src/studio/startup.rs` —— 所有新的响应式逻辑都塞进已注册的 system 或用 `.observe()`，不新增 system 注册。
- `desktop/src/studio/commands.rs`、`desktop/src/studio/actions_content.rs` —— Feature 1 的 Accept/Ignore 接线已经是对的，不需要改。
- `desktop/src/studio/editor/actions.rs` —— 四个特性都不需要新增 `EditorAction` 变体（这也是这份 plan 刻意绕开它的原因：它现在 1605/2000 行，是唯一紧张的文件）。
- `desktop/src/studio/editor/view/chrome.rs` —— Accept/Ignore/Prev/Next 按钮已经存在且逻辑正确。

---

## Feature 1 —— 证据驱动的建议

- [x] `app-core/src/editor/suggestions.rs`：扩大顶部 `use` 引入 `EvidenceKind, EvidenceTrack, ReviewReason, SingingEvidenceBundle`。
- [x] 同文件：新增具名常量 `const MIN_REGION_CONFIDENCE: f32 = 0.5;`（数值可调，但要是一个有名字的常量，不要写死在条件里）。
- [x] 同文件：新增私有函数 `fn median_evidence_midi(track: &EvidenceTrack, start: f64, end: f64) -> Option<f64>`，文档注释里写明"假设 `.value` 是 Hz，尚未在本仓库被任何 producer 证实"这条待确认事项（见设计文档 §4.4）。
- [x] 同文件：新增 `impl EditorDocument { pub fn derive_evidence_suggestions(&self, evidence: &SingingEvidenceBundle) -> Vec<EditorSuggestion> { ... } }`，完整实现见设计文档 §4.2。
- [x] 同文件：新增 `#[cfg(test)] mod tests`，覆盖以下用例（自建一个本文件内部的最小 chart fixture，不要跨文件复用 `document/tests.rs` 里的私有 helper）：
  - [x] 没有 `FusedF0` 轨道 → 空结果
  - [x] 有 `PitchDisagreement`、证据音高与当前音符的**取整后 MIDI 不同** → 产出一条 `ChangePitch`，`note_index`/`midi`/`evidence_refs` 都对（与设计文档 §4.2/§4.3 的实际门槛一致，不使用额外的“≥1 半音原始 delta”规则）
  - [x] 证据音高取整后与音符一致 → 不产出建议
  - [x] `reasons` 里没有 `PitchDisagreement` → 不产出建议
  - [x] `region.reviewed == true` → 跳过
  - [x] `region.confidence` 低于阈值 → 跳过
  - [x] 没有任何音符与该区域重叠 → 跳过
  - [x] 区间内有一个离群点时，中位数依然稳健
- [x] `desktop/src/studio/editor/audition.rs`：在 `finish_native_editor_load` 里，evidence bundle 和 technique evidence 都合并完之后，加一行：
  ```rust
  editor.suggestions = editor.document.derive_evidence_suggestions(&editor.evidence);
  ```
- [x] **不要改：** `app-core/src/editor/mod.rs`、`app-core/src/lib.rs`、`desktop/src/studio/commands.rs`、`desktop/src/studio/actions_content.rs`、`desktop/src/studio/editor/view/chrome.rs`、`desktop/src/studio/editor/state.rs`（`suggestions` 字段已经存在，不用新增）。

---

## Feature 3 —— Technique 证据点详情

（排在 Feature 2 之前做，因为它完全独立且体量最小）

- [x] `desktop/src/studio/editor/state.rs`：给 `NativeEditor` 加 `pub(crate) selected_technique_point: Option<usize>,`，`NativeEditor::new` 里默认值给 `None`。
- [x] `desktop/src/studio/editor/view/timeline.rs`：把渲染 technique chip 的 `for group in track.points.chunks(9)` 循环改成 `.enumerate()`，算出被选中点的 flat index；去掉该节点上的 `Pickable::IGNORE`；加一个 `.observe(On<Pointer<Click>>)` 闭包，把 flat index 写进 `editor.selected_technique_point` 并 `invalidated.invalidate(UiDirtyRegion::Editor)`——完全照抄歌词行空白区域点击那个 observer 的写法，不要走 `Button`/`Interaction` 轮询路径（`handle_editor_pointer_capture` 已经 16 个参数，不要再往里加）。
- [x] `desktop/src/studio/editor/panels.rs`：在 `spawn_editor_inspector` 里新增一个独立的条件区块（不依赖当前 note/word 选中状态，因为点 chip 和选中音符是两件事），门控在 `editor.selected_technique_point.is_some()`，展示：分类标签（复用 timeline.rs 里已经在用的 `point.label` 按 " · " 分割取值的写法）、`point.value`、"uncalibrated" 措辞（照抄 timeline.rs 现有的文案风格）、`point.time` 作为一个时间戳（不是区间——`EvidencePoint` 没有区间起止字段，不要为了"看起来更完整"给它加字段）。
- [x] **不要改：** `input.rs`（`handle_editor_pointer_capture` 保持不变）、`desktop/src/studio/commands.rs`、`startup.rs`。

---

## Feature 2 —— 歌词读音（reading）覆写

- [x] `app-core/src/editor/document/types.rs`：给 `ChartLyric` 加 `pub reading: Option<String>,`。
- [x] `app-core/src/editor/document/notes.rs`（**注意不是 `lyrics.rs`**——`track_lyrics()` 定义在这里）：在它构造 `ChartLyric { ... }` 的字面量里加一行 `reading: token.reading.clone(),`。
- [x] `app-core/src/editor/syllabize.rs`：把 `fn is_han`、`fn is_kana`、`fn is_hangul` 的可见性从私有改成 `pub(crate) fn`。
- [x] `app-core/src/editor/document/lyrics.rs`：
  - [x] 加 `use crate::editor::syllabize::{is_han, is_hangul, is_kana};`
  - [x] 新增 `pub fn set_lyric_reading(&mut self, address: LyricAddress, reading: Option<String>) -> bool`（完整实现见设计文档 §5.2，形状照抄 `set_lyric_text`）
  - [x] 新增 `pub fn lyric_uses_cjk_script(&self, address: LyricAddress) -> bool`（完整实现见设计文档 §5.3）
- [x] `app-core/src/editor/document/tests.rs`：
  - [x] `set_lyric_reading`：正常设置；设成相同值时不产生变化（`revision()` 不变）；空字符串归一化为 `None`；真正变化时 `revision()` 才递增
  - [x] `lyric_uses_cjk_script`：汉字/假名/谚文文本 → `true`；纯拉丁文本 → `false`；一份整体标记为拉丁语言、但这一个词是 CJK 外来词的谱面 → `true`（直接编码设计文档 §5.3 的那条取舍：按词判断，不按谱面语言判断）
- [x] `desktop/src/studio/editor/state.rs`：
  - [x] 新增 `pub(crate) struct EditorWordReadingInput(pub(crate) WordSelection);`
  - [x] 新增一个小 helper `selected_editor_word_reading(document: &app_core::EditorDocument, selection: WordSelection) -> Option<String>`（不要改动 `selected_editor_word` 的既有签名——它已经被其他地方调用，改签名会牵连不该动的调用点）
- [x] `desktop/src/studio/editor/commands.rs`：新增 `pub(crate) fn update_editor_word_reading(document: &mut app_core::EditorDocument, selection: WordSelection, reading: Option<String>) -> bool { document.set_lyric_reading(selection, reading) }`
- [x] `desktop/src/studio/editor/panels.rs`：在 Inspector 单词分支里、既有音节文本框之后，加一个新的 `EditableText` 字段，门控在 `editor.document.lyric_uses_cjk_script(selection)`，绑定 `EditorWordReadingInput(selection)` 标记（不是 `EditorWordInput`），样式（`Node`/`TextCursorStyle`/`BorderColor`）照抄现有字段，初始值取自 `selected_editor_word_reading`。
- [x] `desktop/src/studio/editor/input.rs`：给已经注册的 `sync_editor_word_input` 加一个 `Query<(Ref<EditableText>, &EditorWordReadingInput)>` 参数，镜像既有那一段"检测变化 → checkpoint → 写回 → 判断是否真的有变化"的循环。**不要新增 system，不要改 `startup.rs`。**

---

## Feature 4 —— 歌词快速连续输入 + 内联读音显示

（在 Feature 2 之后做——内联读音显示这一半依赖 Feature 2 的 `ChartLyric.reading`）

- [x] `app-core/src/editor/document/lyrics.rs`：新增 `pub fn advance_lyric_edit(&mut self, from: LyricAddress, forward: bool) -> Option<LyricAddress>`（完整实现见设计文档 §7.2——注意它完全基于已有的 `resolve`/`locate_note`/`lyric_slots`/`phrase_flat_range`/`address_of_note`/`add_lyric_to_note`，不要重新写一遍"下一个词槽位在哪"的逻辑）。
- [x] `app-core/src/editor/document/tests.rs`：
  - [x] 在一个 phrase 内正向/反向推进
  - [x] 目标音符还没有歌词时，会新建一个空的，且能通过 `address_of_note` 反查回来
  - [x] 到达 phrase 首/尾时返回 `None`，且不产生任何变化（`revision()` 不变）
  - [x] 会跳过只带 continuation token 的音符
  - [x] `from` 已经无法 resolve 时返回 `None`
- [x] `desktop/src/studio/editor/commands.rs`：新增 `pub(crate) fn advance_editor_lyric_edit(document: &mut app_core::EditorDocument, from: WordSelection, forward: bool) -> Option<WordSelection> { document.advance_lyric_edit(from, forward) }`
- [x] `desktop/src/studio/editor/input.rs`：扩展已经注册的 `finish_inline_lyric_edit`：
  - [x] 把它的 query 参数从"仅检测存在"加宽成能取到 `&EditableText`（需要读 `is_composing()`）
  - [x] 在既有 Enter/Escape 分支之外加一个 Tab 分支：先判断 `editable.is_composing()`，为真直接 `return`（IME 组字中不拦截 Tab，见设计文档 §7.3）
  - [x] Tab 分支：`checkpoint("Edit lyric")` → 调用 `advance_editor_lyric_edit`（方向由 Shift 是否按下决定）→ 用 `document.revision()` 变化与否判断真的改了还是要把 checkpoint 弹掉 → 有返回目标地址时，**先** `select_only_word(target)` **再** 设置 `word_edit_focus = Some(target)`（顺序不能反——选中变化会清空 `word_edit_focus`，参照 `EditNoteLyric` 现有 handler 的顺序）
  - [x] 到达 phrase 边界返回 `None` 时：停留在当前 `from`，不做任何事
- [x] `desktop/src/studio/editor/state.rs`：给 `ChartLyricView` 加 `reading: Option<String>,`；在 `chart_lyrics()` 的 `.map()` 闭包里从 `lyric.reading` 填充（这一步依赖 Feature 2 的 `ChartLyric.reading` 已经存在）。
- [x] `desktop/src/studio/editor/view/menus.rs`：在 `spawn_editor_lyrics` 每个词的子节点里，`lyric.reading.is_some()` 且当前不是正在被内联编辑的那个词时，加一个小号字体、`theme.muted_foreground`、`Pickable::IGNORE` 的副行文本节点（furigana 风格，纯展示）。检查一下 `lane_height`/行内边距要不要顺带留一点余量，避免一个词既有 reading 又处在歌词密度最高的一行时被裁切——如果需要调整，就在这一步顺手做，不要单独立项。
- [x] **不要改：** `action_input.rs`（全局 `Tab → select_next_note` 的分支保持不变——`handle_editor_keyboard` 已经会在任何 `EditableText` 有焦点时提前 return，两边天然互斥，不需要协调）、`desktop/src/studio/commands.rs`、`startup.rs`，以及——特别注意——**不要碰 `desktop/src/studio/editor/actions.rs`**（四个 feature 全程都不需要新的 `EditorAction`）。

---

## 完成后的收尾（可选，不是必做）

- [x]（可选）如果想让新文档更容易被发现，可以在 `docs/design/README.md` 的 "Editor" 小节下补一行指向 `UTA_STUDIO_EDITOR_OPENUTAU_ENRICHMENT_DESIGN_v1.0.md` 的链接——这个文件不在 `app-core/src/editor/**`/`desktop/src/studio/editor/**` 范围内，是否要动，留给执行者自己判断。

## 验证方式

- `app-core/src/editor/**` 里新增的每一个纯方法都必须有对应的 `#[cfg(test)]` 用例，风格照抄所在文件已有的测试。
- `desktop/src/studio/editor/**` 里的改动全部是 Bevy system body（`ResMut`/`Query` 形状），这一层在本代码库里本来就没有单元测试先例（只有 `action_input.rs`/`state.rs` 各有几个纯逻辑测试）——不要为了这次改动去发明一套 Bevy 测试基础设施。四个特性落地后，用 `run` skill 实际跑起来，走一遍每个特性对应的手动操作路径确认效果。
- 全部改完后跑一遍 `cargo build`/既有测试套件，确认没有破坏 `app-core/src/editor/**` 和 `desktop/src/studio/editor/**` 现有的测试。
