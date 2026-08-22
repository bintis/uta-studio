# Uta Studio Native Inference Refactor — Final Design Index v1.0

**状态：FINAL**  
**Repository baseline:** `bintis/uta-studio@native-inference`  
**Baseline commit:** `56fdbec50444939360caf2832a7b1d958941fe6b`

本目录是本轮重构的唯一 final design pack。历史 draft 不作为 agent 执行输入。

## Authoritative files

1. `05-AGENT-IMPLEMENTATION-GUIDE-v1.0-FINAL.md` — 实施顺序与硬性规则
2. `01-AUDIO-PROCESSING-ARCHITECTURE-v1.0-FINAL.md` — 音频/唱声/Runtime/Workflow 系统语义
3. `04-NATIVE-RUNTIME-LOCK-v1.0-FINAL.json` — 两个 Qwen runtime pin 与通用 runtime policy
4. `02-PROCESSING-STUDIO-UX-v1.0-FINAL.md` — Workflow 用户交互
5. `03-EDITOR-INTEGRATION-v1.0-FINAL.md` — Editor 保留/增强与 Candidate/Authored
6. `06-API-CHANGE-LEDGER-v1.0-FINAL.md` — API 复用/新增审计
7. `07-UI-REFERENCE-NOTES-v1.0-FINAL.md` — PNG 使用说明
8. `08-FINAL-ACCEPTANCE-CHECKLIST-v1.0-FINAL.md` — 最终一次性验证

## UI references

- `ui-reference/processing-studio-dark.png`
- `ui-reference/processing-studio-light.png`
- `ui-reference/workflow-lanes-dark.png`

这些图是概念参考，Editor 行为以当前代码 + final Editor document 为准。
