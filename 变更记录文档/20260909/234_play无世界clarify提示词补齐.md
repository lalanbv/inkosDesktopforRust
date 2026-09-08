# 234 号：play 无世界 clarify 提示词补齐（231 号遗漏点自查）

- 日期：2026-09-09
- 分支：develop
- 关联：230/231 号（提示词移植——本批补其遗漏分支）、80 号（play 有世界专属提示词保留）
- 推送核验：origin/develop 仍停 6bc65564——**201–233 共 33 提交待推送**；本批次后本地领先 34。两项默认值无新答复。

## 一、缺口

TS `buildPlayPrompt` 有三分支：confirmedStart（确认后——Rust 架构下 agent_production 直接执行，不可达不移植）、**playWorldExists=false（无世界 clarify）**、有世界（80 号 `play_chat_system_prompt` 已覆盖）。230/231 号移植时 Play 无世界会话兜底成了 chat 提示词——丢失了专属引导：play_start 确认卡纪律（worldContract/visualContract 提炼、initialScene 纯叙事铁律、mode=open/guided、不编造事实「刚入门不扩写成入门三年」）。

## 二、修复

`chat_prompts.rs` 增 `build_play_prompt_no_world(is_zh)`（zh/en 逐字）；`build_system_prompt` 的 `SessionKind::Play` 分支由 chat 兜底改为专属 clarify 提示词（有世界仍由调用方先行覆盖）。mock 锚点「Play 助手」命中无需适配。

## 三、验证

- engine：lib **1315**（dispatch 测试扩展 play 无世界 zh/en 断言）、集成 **196**、clippy 零告警、INKOS_DUEL=1 duel **10/10 真跑**。
- TS 零改动；core 1917 / studio 793 / 双 typecheck 已绿。
