# 1601 顶层 services 同族扫仓修复轮（2026-11-20）

## 同族扫仓抓虫（1600 姊妹面=第四形态）：顶层 services object-map Rust 忽略绿 / Node strict 拒

1600 修复引出的自然追问按修洞按族纪律扫仓：顶层 `services` 归一化段（688 号注释「fork 时代允许顶层 services」）条件 `Array.isArray(config.services)`——**object-map 形态不归一不删键**，漏到 `ProjectConfigSchema.parse`（strict）被 unrecognized key 拒。688 号只修了顶层数组形态。

漂移事实核实：Rust `ProjectConfig`（models/project.rs:432）**无顶层 services 字段且无 deny_unknown_fields**→serde 忽略任意形态顶层键（绿）。同款文件 Rust 绿/Node 拒启=1600 同族第四形态（709/1429/1600 之后）。

## 修复（effective-llm-config.ts 顶层归一化段）

条件 `Array.isArray(config.services)` 放宽为 `config.services !== undefined`——归一化统一走 `normalizeServiceEntries`（数组/object-map 两形态），顶层键一律消隐（strict 不容）；llm 层在场时仍 llm 层优先（回填条件不变）。

## 验证链（红→绿→活体→全链）

1. **红拍**：新用例（顶层 object-map 展平回填 llm.services 且顶层键消隐）修复前 1 failed——现状精确复现。
2. **绿拍**：修复后 24/24（23+1 新）。
3. **core 全量 1079 passed**（1600 后 1078+1，无面外回归）。
4. **dist 重烤+Node 腿活体**：顶层 object-map 夹具（services map+llm 段 studio 模式）起服 SPA 200+profiles 200（修复前此形态拒启）。
5. **gate:ts 十腿全绿**（node-fallback-smoke/engine-contract-diff 吃修复后 dist）。
6. **duel 真跑 10/10**（1 真跑+9 豁免记账同 1600 形态）。

## 清场

端口释放+tmp 清理。

## 后续

- 下号自 1602 起；用户侧待办不变：① Fork 图形端推送（955-1601，含 1600/1601 两轮 schema 漂移修复建议优先推送）② 真实 Tauri build 打包验证。
