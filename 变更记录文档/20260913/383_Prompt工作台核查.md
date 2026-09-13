# 383 号：R14 核查——Prompt 工作台已存在（纯核查，零开发）

- 日期：2026-09-13
- 类型：纯核查（无代码变更）
- 规划依据：[v3 规划 §3 R14](../../开发时SpecCoding'sPlan/inkosDesktop/07_产品演进规划/对标调研分析与改进规划_v3.md)（Prompt 工作台：浏览/试运行/差分）

## 核查结论

**Prompt 工作台已存在**，无需新开发：

1. **UI**：ProjectSettings 页面已有完整 prompt 编辑区（`useApi("/prompt-packs")`
   列表 + `promptDraft` 编辑 + 保存/删除，见 ProjectSettings.tsx:103/399/411）；
2. **TS 端点**：`GET /api/v1/prompt-packs`（列表）、
   `PUT/DELETE /api/v1/prompt-packs/:promptId`（server.ts:4934/4944/4963）；
3. **Rust 端点**：`/api/v1/prompt-packs` 路由已注册（server/mod.rs:505，60 号
   轻域）；
4. **防漂移诉求**：R8 行为评测集（369 号）已覆盖 continuity/reviser/settler
   的 prompt 锚点断言——R14 的"差分回归"诉求由评测集承担。

## 过程（诚实记录）

本批曾实现新增端点+面板，随后查重发现 ProjectSettings 既有编辑区与双端
prompt-packs 路由**早已存在**——立即 git checkout 撤销全部重复实现，恢复
基线并全门禁回归确认（studio 93/805、core 233/2062、duel 33 目标、
bindings 180 全绿）。教训入记忆：**新功能开发前必须先 grep 既有实现与
UI 面，重复造轮子比缺口更贵。**

## 影响面

- 仅新增本文档；零代码变更。基线恢复后全门禁回归绿。

## 里程碑

- **R14 核查闭合（结论：已存在，关闭）。** 三轮 P1 剩 R15 三库市场包；
  P2 剩 R16–R19。
