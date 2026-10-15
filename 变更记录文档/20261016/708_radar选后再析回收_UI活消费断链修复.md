# 708 号：radar 选后再析两面回收（第六批，回收清单 29→27；node 回退腿 UI 断链修复）

> 日期：2026-10-16（707 号同日续号）
> 范围：packages/core（agents/radar.ts + pipeline/runner.ts + index）/ packages/studio（server.ts 两路由 + 测试）/ scripts（contract-diff 域表）

## 一、为什么是这一批（用户可见断链，非清单顺序 picking）

`RadarView.tsx`（2.0 UI）在位消费 `POST /radar/rankings` 与 `POST /radar/analyze`（G14a/335「选后再析」：免费扫榜 → 勾选范围 → LLM 分析），而两面 fork-only（Rust 在位、TS 2.0 server 404）——**node 回退腿市场雷达页核心交互整链断**（164 号桌壳默认 Rust 直启、Node 为回退腿，回退态 UI 退化）。本批修复后双引擎雷达页全功能对齐。

## 二、2.0 合并裁撤面考古与增量恢复（保留 2.0 harness 形态）

688 号合并把 fork `agents/radar.ts` 的选析 API 整体裁撤（RadarSelection 接口 / scanRankings / analyzeRankings / confidence+crowding+differentiation 信号卡 / fetchRankings / filterRankingsBySelection），2.0 侧只留 submitStructured 证据落地形态 `scan()`；Rust `radar.rs` 全量保留对偶（`RadarSelection`/`filter_rankings_by_selection`/`run_radar_analyze`，serde Value 镜像）。恢复策略=**增量并集，双路径各自保真**：

1. **radar.ts**：fork 五件全量恢复（逐字）；`RadarRecommendation.confidence` 定为可选——2.0 `scan()`（submitStructured + RadarResultToolSchema）不产出 confidence，Rust 侧本就是 `serde(default) Value`，可选是正确的跨引擎镜像；空榜单回退语（「未能获取到实时排行数据…」）**内联进 analyzeRankings** 而非改 `formatRankingsForPrompt` 本体——2.0 `scan()` 依赖空文本守卫抛「无源证据」错误，本体加回退会静默废掉该守卫。
2. **runner.runRadar(options)**：selection 在位 → fork 原版 `scanRankings+analyzeRankings`（自带系统提示词，**不做** 2.0 市场 Skill 门槛——fork analyze 提示词自包含，Rust 对偶亦无 skill 检查）；缺省 → 2.0 原版路径原样（Skill 激活 + submitStructured，daemon/tick 不动）。第一版曾把 skill 检查提到双路径之前，自查纠正。
3. **server.ts 两路由** fork 逐字（rankings 动态 import fetchRankings；analyze body.selection → runRadar({selection}) + saveRadarScan + radar:start/complete/error 广播）。
4. index 导出补 `fetchRankings/filterRankingsBySelection/RadarSelection`（fork index 原有）。

## 三、contract-diff 探针设计（两面环境依赖性分诊）

- `rankings` POST 探针安全：双端榜单源同构容错（TS catch-skip 恒 resolve / Rust 5s 预算客户端跳过）→ 离线亦 200/200 稳定。**status-only**（响应体平台名/条目依赖真实榜单数据，不可比）。
- `analyze` 带 `{selection:{}}` 探针：显式 selection 对象走**嫁接分析路径**（无 body 会落 2.0 scan() 分支，探不到 graft 面）；mock 无市场分析师分派 → 回退文本无 JSON → 双端解析失败 500/500（状态码对称=路由在位，703 号写面口径）。
- **活体独立对跑：共享面 38 对比 / 0 分歧（新入 rankings 200/200、analyze 500/500）；fork-only 27/27 声明确认；上游新增 9/9+2；exit 0。**

## 四、门禁终态

| 门禁 | 结果 |
| --- | --- |
| core vitest | 927/927（含新增 filterRankingsBySelection 7 测：三维度独立/大小写不敏感/子串包含/全空不过滤/空平台剔除） |
| studio vitest | 661/661（含新增路由 3 测：rankings stub-fetch 双源解析 / analyze 无 LLM 配置降级 500 / scan 既有面回归锁定） |
| typecheck | 四包 0 错误 |
| gate:ts 八步 | 全绿 exit 0 |
| contract-diff 独立对跑 | 38 面 0 分歧 exit 0 |
| rust-bin | 与 engine-rs 源码同源（本轮零 Rust 改动） |

## 五、教训

1. **回收清单排序应让位 UI 活消费**——清单是 Rust↔TS 面差账本，但「UI 正在调用且回退腿 404」的面是用户可见断链，优先级高于清单顺序；先 grep 消费方再排批次。
2. **增量恢复要识别「守卫语义寄生在共享 helper 上」**——空榜单回退语寄生在 formatRankingsForPrompt 本体上会废掉 2.0 scan() 的无证据守卫；fork 逐字不等于 helper 本体逐字，要分辨「行为」与「行为所依赖的前置条件」。
3. **可选性由最弱产出方决定**——confidence 在 fork 路径必产、2.0 路径不产、Rust serde(default)，三源合议=可选；把 fork 时代的必填照搬过来会让 2.0 scan() 产物类型撒谎。
4. **探针要打到被测 graft 路径**——analyze 无 body 探针会走 2.0 scan() 分支（LlmConfig 失败 500，看似对称实则零 graft 覆盖）；`{selection:{}}` 才路由进嫁接面。
