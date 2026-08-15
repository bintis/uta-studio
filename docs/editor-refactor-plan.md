# 编辑器重构计划：对齐 utz 0.2，参考 Karedi 能力

状态：执行中（2026-08-15）。目标是让编辑器直接编辑 utz 0.2 的
`VocalChartV1` 授权模型，并吸收 Karedi 的架构模式与编辑能力。

**范围决定**：不考虑 utz 0.1 兼容，也不做向前兼容。因此分析器产出的
transcript / pitch_notes 只作为**导入源**，不再是需要同步维护的投影；
0.1 导出路径不实现。

**进度**：阶段 0、阶段 1 已完成（含 UltraStar 导出改造）。下一步是阶段 2
的命令系统。

参考材料：

- utz 0.2 规范：`../utz/format/utz-v0.2.md`（vendored crate `vendor/utz` 已是 0.2.0）
- Karedi 源码：`/tmp/Karedi`（Java/JavaFX UltraStar 编辑器）

---

## 一、现状诊断

1. **编辑器仍在编辑遗留投影，而不是 vocal chart。**
   `ChartDocument`（`app-core/src/chart.rs`）中 `vocal_chart: VocalChartV1`
   名义上是 authoritative，但 Bevy 编辑器（`NativeEditor`，
   `desktop/src/studio.rs`）实际修改的是 `transcript` / `pitch_notes` 两个
   `serde_json::Value`。保存路径 `save_chart` 会把编辑后的 JSON 重新跑一遍
   `migrate_legacy_chart` 启发式重建 chart，**每次保存都是有损往返**：
   - 多轨（duet/harmony/backing）无法表达，永远只产出一条 `lead`；
   - `LyricJoin`、`reading`/`phonemes`、`cents`、`scoring.weight`、显式
     `Continuation` token、手工 phrase 结构在往返中丢失或被启发式覆盖；
   - 音符↔歌词归属靠时间重叠猜测（`vocal_chart.rs`），编辑器里对好的词
     在保存时被重新"猜"一次。
2. **`studio.rs` 是 2 万行单体。** 模型变更（约 30 个直接改 JSON 的函数）、
   选择、视口、输入、渲染混在一个文件，无法单测，也无法叠加新能力。
3. **撤销是全文档快照**（`ChartSnapshot`），无命令语义、无法合并连续
   拖拽、没有历史面板。

## 二、Karedi 能力清单 → utz 0.2 映射

吸收的核心是**架构模式**，不是 UltraStar 语义。

| Karedi 能力 | 对应到 uta-studio / utz 0.2 |
|---|---|
| `Command` + `CommandComposite` + 装饰器 + `History`（历史列表） | 类型化命令系统，直接作用于 `VocalChartV1`，替换快照撤销 |
| `Song → SongTrack → SongLine → Note` 分层模型 | 对应 utz 的 `track → phrase → note`，note 拥有 lyric tokens |
| `problem/` 框架：类型化 Problem、severity、`Solvable` 自动修复、面板 | 升级 `analyze_chart_issues`/`repair_editor_chart` 为类型化校验器，规则来自 `VocalChartV1::validate` + 编辑质量规则 |
| `KarediActions` 注册表（约 110 个 action）+ 键位映射 | 用 action 注册表替换 `handle_editor_keyboard` 巨型分支；命令、快捷键、菜单、诊断 API 共用一个入口 |
| Syllabizer（EN/JA/PL/ES 音节切分） | "lyric token 再切分"：一个 text token 拆成多个带 `join_before` 的 token；日语联动 MMS 假名管线填 `reading` |
| Lyrics 编辑器双向同步、`ROLL_LYRICS_LEFT/RIGHT`、`INSERT_SPACE/MINUS` | token 流编辑：向左/右滚动歌词重新分配 token 到相邻 note、token 与 `Continuation` 互转 |
| MIDI 音高试听（选区/可见区/全部；audio、MIDI、混合；选区前/后） | 本地合成音高试听，按 note 的 MIDI 目标发声，走 native-audio 输出 |
| `TAP_NOTES` 跟播打点 | 播放中按键生成/对齐 note 时值，适配 `rhythm`/`spoken` 段落 |
| 选择模型（next/prev、扩大/缩小、选可见/全部）、`FIT_TO_SELECTION` | 补全 marquee：键盘遍历、按 phrase 选择、fit 视口命令 |
| 多轨 duet：加/删轨、轨道切换、`FillBar` 覆盖率条 | **utz 0.2 关键新能力**：`VocalTrack` 的 lead/duet/harmony/backing/ad-lib 需要轨道 UI 才能真正用上 |

**不照搬**：BPM 工具、medley、tags 表——UltraStar beat 语义；utz 0.2 用
1 MHz 整数 timebase，无 BPM。beat 量化只保留在 UltraStar 导出边界。

## 三、目标架构

```
app-core/
  src/editor/              ← 新模块：UI 无关、纯逻辑、可单测
    document.rs            EditorDocument：VocalChartV1 + ID 索引 + 修订号
    command.rs             EditorCommand trait（apply → 逆命令）、Composite、合并策略
    commands/              move/resize/split/merge/set_pitch/set_mode/lyrics/track/phrase…
    selection.rs           Selection（track/phrase/note/token 四级）
    problems.rs            类型化 Problem + severity + 可自动修复标记
    syllabize.rs           token 切分（先日语/英语）
    history.rs             undo/redo 栈 + 命令标签（供历史面板）
  src/vocal_chart.rs       只保留"分析结果 → chart 首次迁移"和"→UltraStar 投影"
desktop/src/
  studio.rs                只剩路由/library/设置
  editor/
    mod.rs  state.rs       NativeEditor 变薄：视口、指针捕获、播放状态
    view.rs input.rs       渲染与输入分离；input 只把手势翻译成 action
    actions.rs             action 注册表 + 键位表
    panels.rs              问题面板、历史面板、轨道条、检查器
    audition.rs            MIDI 音高试听、tap 模式
```

关键决策：

- **`EditorDocument` 内部一律用 u64 timebase 单位**，不是秒。规范要求整数
  位置防浮点漂移；只在渲染和音频 seek 边界转 `f64` 秒。
- **命令 = 结构化编辑 + 自动逆命令**；拖拽过程中同一命令合并
  （`merge_with`），指针释放才落一条历史。
- **重叠策略**：utz 0.2 规定轨内 note 不得重叠，`validate()` 会拒绝。编辑
  期允许存在但立即在 Problems 面板标红；**保存/导出前必须通过
  `chart.validate()`**，硬错误存在时禁用保存并指向问题面板。
- **`Continuation` 引用完整性**由命令层维护：删除 text token 时级联处理
  引用它的 continuation（规范要求同轨内可解析）。
- **保存只走 `save_vocal_chart`**：编辑器模型直接持久化 + 派生 legacy 投影
  供分析器/UltraStar 兼容。JSON 版 `save_chart` 在 UI 切换完成后删除。
  `migrate_legacy_chart` 只在缓存无 vocal_chart.json 时执行一次。

## 四、分阶段任务清单

每阶段结束满足 AGENTS.md 验收（`cargo test -p uta-studio-core --lib`、
`cargo check --workspace`、真实 UTZ + UltraStar 冒烟导出、双导出器覆盖、
项目名扫描），应用全程可用，无"大爆炸"切换。

### 阶段 0 — 固化基线 ✅

- [x] 把未提交的 0.2 工作按语义落盘（commit `a22d788`）。
- [x] 修复 `studio-diagnostics` 未跟进 0.2 manifest 枚举导致的工作区
      编译失败；诊断冒烟导出现在会校验导出包内的 vocal chart。
- [x] 补齐分析器导入边界测试：重叠拒绝、continuation 生成、note kind
      映射。测试当场抓到并修复了投影往返丢失 `join_before: Space` 的
      真实 bug。

### 阶段 1 — 模型切换：编辑器直接编辑 `VocalChartV1` ✅

commit `dd82c3f`（编辑器）与 `ed4b018`（导出）。

- [x] 新建 `app-core/src/editor/`：`EditorDocument` 持有 `VocalChartV1`，
      内部一律用整数 timebase 单位，提供扁平索引的 note / lyric 只读视图，
      渲染层不必了解 track→phrase→note 的嵌套。
- [x] studio.rs 的全部 JSON 编辑函数移植为 `EditorDocument` 类型化操作，
      语义逐条保留（最小时长、拆分/合并规则、量化、带钳制的位移、剪贴板
      几何），并补上 JSON 模型无法表达的 continuation 维护：
  - [x] note 系：move / resize / insert / remove / split / merge /
        quantize / shift / cycle-kind（`vocal_mode` + `bonus` + `scoring`
        三元组）/ copy / paste
  - [x] 词系 → token 操作；**歌词 token 归属于 note**（格式要求），所以
        「无引导歌词」＝「无 pitch 目标的 note」，拆分音节即拆分其 note
  - [x] phrase 系：真正修改 `VocalPhrase` 结构
  - [x] 保守自动修复（排序、最小时长、分离重叠）下沉到模型层并加测试
- [x] `ChartDocument` 移除 `transcript` / `pitch_notes`，只保留权威 chart
      与作为可选证据的 `pitch_track`。
- [x] 保存只写 `save_vocal_chart`；`.utz` 与 UltraStar 导出都读取已保存的
      chart（原先 `.utz` 导出会重新迁移，直接丢弃编辑成果）。
- [x] 重新转写 / 重新对齐 / 重新分析音高时失效已保存 chart，避免旧 chart
      遮蔽新分析结果。
- [x] note confidence 退出编辑器：它是分析证据，utz 0.2 不把证据放进
      授权 chart。
- [x] 验收：app-core 86 项、desktop 22 项测试通过；studio.rs 减少约 1 900 行。

### 阶段 2 — 命令系统与撤销

- [ ] `EditorCommand { label(); apply(&self, doc) -> Result<Inverse>;
      merge_with(...) }`；阶段 1 的操作改造成命令，`Composite` 覆盖多选
      批量操作。
- [ ] `History`：带标签 undo/redo 栈、拖拽合并；历史面板（点条目跳状态，
      对应 Karedi `HistoryController`）。
- [ ] `actions.rs` 注册表：命令、快捷键、菜单项、`api_capabilities` 诊断
      入口统一由 action 表驱动，替换 `handle_editor_keyboard` 巨型分支。
- [ ] 验收：每个命令一个 "apply→invert→apply" 性质测试；撤销不再是
      全量 JSON 快照。

### 阶段 3 — 拆分 `studio.rs`

- [ ] 机械移动到 `desktop/src/editor/{state,view,input,panels,audition,actions}.rs`。
- [ ] 目标：`studio.rs` < 5k 行，editor 各文件 < 2k 行。
- [ ] 无行为变化；手工清单守护 AGENTS.md 交互规则（指针捕获、手动滚动
      优先于播放头跟随、Space 转移等）。

### 阶段 4 — Karedi 能力增量（每项独立可交付，按价值排序）

- [ ] **Problems 面板**（替换 issue inspector）：轨内重叠（阻断保存）、
      pitch 模式缺 pitch 目标、无歌词可打分 note、孤儿 continuation、
      异常短音（<30ms）、phrase 间无间隙、golden 占比异常；每条可点击
      定位，可修复项一键修（走命令系统，可撤销）。
- [ ] **多轨支持**：轨道条 UI（增/删轨、角色、singer、`scoring_enabled`）、
      当前轨切换、其他轨半透明显示、fill bar 覆盖率条；"移动选区到轨 X"
      命令（规范推荐的重叠合法化路径）。
- [ ] **MIDI 音高试听**：播放选区/可见区，模式 = 音频 / 合成音高 / 混合；
      播放选区之前/之后用于校对衔接。合成音是独立流，原音频不变。
- [ ] **Tap-to-time**：播放中按键打点生成/重定时 note，落成可撤销命令；
      配合 `rhythm` scoring 做说唱段落。
- [ ] **Syllabizer**：选中词按语言切分多 token（日语按拍/假名、英语按
      音节规则），`join_before` 自动设置；日语复用 MMS 管线填 `reading`。
- [ ] **歌词滚动**（roll left/right）与 token 级歌词编辑器（文本框与
      note 双向同步）。

### 阶段 5 — 导出收尾

- [x] UltraStar 导出直接从 `VocalChartV1` 生成（已在阶段 1 完成）。
- [ ] 多轨 → UltraStar duet（P1/P2），依赖阶段 4 的多轨能力。
- ~~utz 0.1 兼容导出~~ —— 按范围决定不实现。
- [ ] pitch evidence：分析器帧级 f0 以 `pitch-evidence` 资产随包导出
      （可选项）；编辑器把它当背景参考渲染，绝不回写 note。
- [ ] 全量验收：AGENTS.md 完整清单 + `nix build path:.#uta-studio` +
      打包产物冒烟启动。

## 五、风险与顺序注意点

- **最大风险在阶段 1**：词/phrase 编辑从"segment 文本重建"变为"token
  结构编辑"，细节（空格、CJK 紧凑拼接 `compact_lyric_language`、词拆分
  边界）先用测试钉住旧行为再迁移。先移植 note 系命令，再移植词/phrase 系。
- **不要跳过阶段 1 直接加 Karedi 功能**：多轨、syllabizer、continuation
  编辑在 legacy JSON 模型上无法表达，先换地基。
- **性能**：`VocalChartV1` 克隆比 JSON 快照便宜，但渲染层不要每帧重建
  note 视图——用 `EditorDocument` 修订号做失效判断，接现有
  `UiInvalidated` 机制。
- 阶段 2 起 `run_feature_diagnostics` 以只读方式枚举 action 表，保持 API
  注册契约测试同步（AGENTS.md 要求）。
