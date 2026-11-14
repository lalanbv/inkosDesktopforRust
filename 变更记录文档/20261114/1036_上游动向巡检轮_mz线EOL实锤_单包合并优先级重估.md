# 1036 号 · 上游动向巡检轮——mz 线 EOL 实锤，单包合并优先级重估

> 1027 备案的「cache-warming 前置观察（上游 mz 线 sync 动向）」履行。零代码
> 改动轮。**重大发现：双名发布并未并行——上游已实质单线化。**

## 一、npm 上游动向实锤

| 包 | latest | time.modified | 状态 |
|---|---|---|---|
| @mariozechner/pi-agent-core | 0.73.1 | **2026-05-07** | **半年冻结（EOL）** |
| @mariozechner/pi-ai | 0.73.1 | **2026-05-07** | **半年冻结（EOL）** |
| @earendil-works/pi-agent-core | 1.5.2 | 2026-11-10（4 天前） | **活跃主线** |
| typebox | 1.4.7 | =仓内现版 | 对齐 |

## 二、定性反转与优先级重估

1. **1022 的「双栈终态：两线各自 latest」定性修正**——实态=mz 线 **EOL 冻结**，earendil 线独活——**上游已实质单线化**（badlogic 迁 org 后旧名只留终版）。
2. **cache-warming 触发条件更新**：「mz 线 sync 该能力」**永不命中**（mz EOL）——唯一路径=writer 链迁移 earendil 1.5.2（单包合并）。
3. **mz EOL 供应链风险面**：mz 包的安全修复不再来（advisory 监控独立于 earendil 线）——长期风险随 mz 使用面（38 文件）存续。
4. **earendil-mz 单包合并优先级：远期→中期**。规模重估=38 文件 mz 消费面迁移（writer/agent-session/harness 全家）+Type/Static 已统一 typebox（1022）+earendil 1.5.2 仓内符号 13/14 兼容（1014）+兼容层就绪（1018-1019）——**迁移条件成熟度高于 1014 时点**。writer 链迁移的已知行为面=cache-warming 缺失（earendil 1.5.2 有——迁移后反而获得）+agent loop 语义（1020 的 kind 投影/Clean 防线为 earendil 迁移铺垫）。

## 三、下一号自 1037 起

出口=**earendil-mz 单包合并专项（优先级升中期，建议先行勘察轮：mz 38 文件消费面分类+迁移序）** / 下一巡检周期 / 用户侧待办=Fork 推送（含 955-1036）。
