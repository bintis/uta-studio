# utz

The independent definition and reference implementation of the Uta song
package (`.utz`) format.

Uta 歌曲包格式（`.utz`）的独立定义与参考实现。

This project owns only interoperability:

- the ZIP container and versioned `manifest.json` contract;
- safe native and WebAssembly-compatible Rust parsing;
- deterministic package writing and asset integrity checks;
- schemas, conformance fixtures, and compatibility tests.

本项目只负责互操作层：

- ZIP 容器与带版本的 `manifest.json` 契约；
- 安全的原生与 WebAssembly 兼容 Rust 解析；
- 确定性的包写入与资产完整性校验；
- 模式文件、一致性样例与兼容性测试。

It does not own AI generation, playback UI, microphone capture, or game balance.
Those belong to Uta! Studio and uta-ruleset respectively.

AI 生成、播放 UI、麦克风采集与游戏平衡不在本项目范围内，分别属于 Uta! Studio
与 uta-ruleset。

```sh
cargo test
```

Format documentation and schemas in `format/` are CC0-1.0. The Rust reference
implementation is MIT licensed so independent producers and games can adopt
the interoperability layer without taking a dependency on Studio's GPL code.

`format/` 下的格式文档与模式文件采用 CC0-1.0。Rust 参考实现采用 MIT 许可，
使独立制作方与游戏可以采用互操作层而无需依赖 Studio 的 GPL 代码。

UTZ 0.2 makes a versioned vocal chart the authoritative playable document.
Lyrics, phrases, singer tracks, note timing, pitch targets, vocal mode, bonus
state, scoring intent, and UltraStar-style duet parts live together in that
chart. Chart time zero is the first sample of the instrumental. Analyzer
output such as frame-level pitch evidence is optional and never overrides
authored notes.

UTZ 0.2 以带版本的人声谱面作为权威可播放文档。歌词、乐句、声部 track、音符
时间、音高目标、演唱模式、加分状态、打分意图以及 UltraStar 风格的对唱声部都
集中在这份谱面里。谱面时间零点是器乐音轨的第一个采样。帧级音高证据等分析器
输出是可选的，永远不会覆盖制谱音符。

`format/fixtures/` holds the shared conformance suite: `valid/` documents
every implementation must accept and `invalid/` documents every
implementation must reject. `tests/conformance.rs` runs it against the
reference crate.

`format/fixtures/` 存放共享的一致性样例集：`valid/` 下是所有实现都必须接受的
文档，`invalid/` 下是都必须拒绝的文档。`tests/conformance.rs` 用参考 crate
运行该样例集。

The reference crate reads both UTZ 0.1 and 0.2 packages. New producers should
write 0.2; compatibility exporters may continue writing 0.1 while older games
are still in use.

参考 crate 可读取 UTZ 0.1 与 0.2 两种包。新制作方应输出 0.2；在旧版游戏仍在
使用期间，兼容导出器可以继续输出 0.1。
