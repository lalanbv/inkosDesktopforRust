# 411 · 高频消费面协议体检——Rust 引擎 writing-stats 端点缺失修复

日期：2026-09-14　性质：轻量质量面复核（协议级体检）转缺陷修复　前置：406/407 fixture 环境

## 复核方式

fixture 环境（mock LLM + 引擎 + fixture 书）对书籍详情**全部高频端点**
做协议级体检（状态码 + 关键字段）：

```
context-lens/quality-trend/tension-curve/quality-debts/codex/
anti-ai-rules/roster-candidates/promises/series-id → 全部 200 ✓
writing-stats → 404 ✗（本批缺陷）
asset-library/:kind → 400 invalid kind（参数校验正常）✓
task-routing/services → 200 ✓
```

## 发现与修复

**Rust 引擎无 writing-stats 端点**——382 号归档声明的双端异构
（「TS /writing-stats 聚合；Rust /writing-stats-rows 返回行数据由前端
同一纯函数聚合」）只落地了前端注释，Rust 端点从未实现：桌面默认 Rust
引擎下 `WritingStatsCard` 的 `/writing-stats` 请求 404 → catch →
setStats(null) → **写作数据面板恒空**（382 号 R13 面板在默认引擎不可用）。

修复（对齐 382 号声明形态）：

1. Rust `ops_routes::get_writing_stats_rows`：`GET /api/v1/writing-stats-rows`
   （全局遍历各书 chapter index → rows：bookId/updatedAt/wordCount/status/
   totalTokens；status 走 serde rename 输出 TS 字面；单书索引缺失跳过）；
2. mod.rs 注册路由（with_state books）；
3. `WritingStatsCard` catch 分支补回退：fetch `/writing-stats-rows` →
   前端 `aggregateWritingStats(rows, nowIso)` 同一纯函数聚合。

端到端实测：rows → 200，返回 fixture 书两行（status serde 字面
"ready-for-review"/"audit-failed" 准确）。

## 门禁

duel 39 单元 1787 用例 exit 0；studio 97 文件 823 用例 + tsc 净。

## 推送提醒

origin/develop=e555d6f3；剩余 16 笔待推（本批 411 含真实缺陷修复）。
请 Fork 图形端推送并核验追平。
