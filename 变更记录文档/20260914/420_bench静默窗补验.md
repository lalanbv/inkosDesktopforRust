# 420 · bench:gate 静默窗补验——409–414 基线复验通过

日期：2026-09-14　性质：性能基线复验（419 号备案执行）

## 复验环境

负载静默窗实测 13.8%/核（< 20% 门限）后立即执行；
`pnpm bench:gate -- --sample-size 30`（410 号先例的逃生口，Bash 单命令
窗口上限下压缩采样；低采样更严非更松，协议偏离沿用已备案先例）。

## 结果（9 基准全带内，✓ 门禁通过）

| 基准 | 漂移 |
| --- | --- |
| chapter_index/parse_200 | +4.7%（最大漂移，仍远低于 +30% 阈值） |
| title_dedup/resolve_duplicate_hit_200 | +2.8% |
| title_dedup/resolve_no_duplicate_200 | +1.0% |
| sse_broadcast/dispatch/32 · /8 | +1.4% · +0.4% |
| paragraph_scan/detect_shape_3000zh | −0.7% |
| sensitive_words/scan_chapter · builtin_only | −0.6% · −0.1% |
| sse_broadcast/dispatch/1 | −6.8% |

**结论：409–414 六批（投影重建/原子写/路由注册/死代码清理）零性能回归。**
覆盖说明：hybrid-search 路由补挂为注册面变更，非热路径。

## 待办状态更新

- ~~bench 静默窗补验 409–414 基线~~ → 本批闭环；
- 剩余全部为用户侧动作：**Fork 图形端推送 24 笔积压**（origin=e555d6f3 →
  本地 0d89f675+本批）并核验一致；R16/R19 真实长跑数据项；两项默认值与
  ja A/B 答复。
