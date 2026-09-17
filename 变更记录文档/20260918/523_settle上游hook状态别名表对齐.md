# 523 号：settle 上游 hook 状态语义对齐——Rust 镜像 TS 全量状态别名表，H03 工件分歧清偿

日期：2026-09-18　分支：develop　基线：4ae9cf31（522 号）

## 背景

522 号工件面备案清单中的实质语义分歧：fixture 预置 H03 状态 `pressured`，落盘后 TS 侧 `progressing`、Rust 侧 `open`（`pending_hooks.md` 主文件与快照传导两处）。本号追链定因并修复。

## 根因链

1. fixture 直写 `pending_hooks.md` 的 H03 状态单元格为 **`pressured`**（非标准状态词）。
2. TS 侧 `HOOK_STATUS_ALIASES`（hook-lifecycle.ts）含全量别名表：`pressured/advanced/confirmed/命中/已推进…` → `progressing`，`dormant/休眠/未激活…` → `deferred` 等约 60 个别名。
3. Rust 侧 `parse_hook_status`（story_markdown.rs）仅收少量词，`pressured` 不在表 → 回退 `Open`。
4. 双端 bootstrap/重写后伏笔池状态分叉。reducer merge 逻辑逐字同构（522 已证），上游归一是唯一分叉点。

## 修复

`engine-rs/src/utils/story_markdown.rs`：`parse_hook_status` 镜像 TS `HOOK_STATUS_ALIASES` 全表——progressing 组（pressured/advanced/progress/confirmed 系/命中系/推进系 + Rust 既有 ongoing）、deferred 组（dormant/sleeping/inactive/not_started 系/休眠系/未推进系全量）、resolved 组（paid_off 系/兑现系 + Rust 既有 complete/completed/已关闭）、open 组（TS 全表）；未命中回退 Open（TS `normalizeStoredHookStatus` 同语义）。注释标注 523 号与差分器实证依据。

## 验证

- 差分器活体：`pending_hooks.md` 主文件与 `snapshots/1/pending_hooks.md` 两处分歧**消失**（工件备案清单 7 项收敛至 5 项：resync 留痕 ×1、index.json 元数据、0001 尾换行形态、intent/plan/rule-stack 措辞 ×3）；**41 对照项 0 分歧**保持。
- engine-rs 全量 cargo test **1805 绿**；`pnpm clippy:gate` 双 crate 0 告警。
- node-fallback-smoke 双引擎 13/13 绿；export-epub-smoke 双引擎绿；`gate:ts:fast` 全绿。
- `pnpm bench:gate` 9 基准零回退（-1.0%~-24.2%；负载守卫两轮拦截后静候回落）。
- TS 侧零改动。

## 工件备案清单状态（522 建立）

| 项 | 状态 |
|---|---|
| pending_hooks.md H03 状态（×2 处） | **✅ 本号清偿** |
| 仅 node：runtime/chapter-0001 intent/plan/rule-stack | 备案（resync 留痕面） |
| runtime/chapter-0002 intent/plan/rule-stack 措辞 ×3 | 备案（prompt 模板对齐专项） |
| chapters/index.json 元数据 | 备案（工件序列化专项） |
| 0001 尾换行 1B（fixture 直写形态） | 备案（非管线产物差异） |

## 关联

- 522（工件面建立与定性）、519（reducer merge 同构证明——本号把分歧点上移定因）、520（差分器投影面）、409（payoff_timing canonical 同型修复）、R23/394（hook kind 归一先例——本号为其 status 同型）。
