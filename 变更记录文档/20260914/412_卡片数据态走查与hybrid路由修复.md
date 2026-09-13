# 412 · 书籍详情其余卡片数据态走查——hybrid-search 路由漏挂修复

日期：2026-09-14　性质：轻量质量面复核（真实写作数据态）　前置：407b fixture 环境（含 write-next 真实落盘）

## 走查内容（数据态渲染验证）

利用 fixture 书真实写作数据（resync+write-next 落盘的 runtime trace、
review_metrics、带 kind 台账），浏览器实测书籍详情其余卡片：

1. **上下文透视（ContextLensPanel）✓**：章 2 回放 **12 条来源**逐条渲染——
   层级标签（本书事实/本书规划/本书时序记忆/写法资产）、保护徽标（前 6 条）、
   token 估算、聚合行（12 条来源 · 受保护 6 · 约 1800 tokens · 预算内未压缩）；
   `rules/anti-ai` 种子条目在装配中可见（392/366 链端到端）。
2. **质量趋势（QualityTrendCard）✓**：「审查器未评分，仅记录通过状态」+
   已记录章/未过章计数；承诺紧迫度列表 H03=80(显式,目标4–8)/H01=60/H02=60/
   H04=60(推断)——与后端 curl 逐字段一致。
3. **召回测试（RecallTestCard）✓（修复后）**：检索返回「模式: 词法降级 +
   无命中」正常渲染（fixture 语料小，空结果为预期）；修复前该卡为 404 报错。
4. 其余卡片数据态：张力曲线（第 1–2 章，0 章未评分）、三库种子、新专名确认
   （苏檀/镜灵待确认）均在位。

## 抓到并修复的真缺陷

**hybrid-search 路由漏挂**：`ops_routes::post_hybrid_search`（349 号实现）
存在于代码但 `mod.rs` 从未注册——`POST /books/:id/hybrid-search` 在 Rust
引擎恒 404，召回测试面板报错不可用（与 411 号 writing-stats 同款「实现了
没挂路由」模式）。修复=补挂 `post(...).with_state(books.clone())`；实测
200 + `{"fts":[],"fused":[],"mode":"fts5-fallback","semantic":[]}` 形状正确。

## 门禁

duel 39 单元 1787 用例 exit 0（路由注册零回归）；前端无变更（RecallTestCard
本体未改）。

## 模式沉淀

连续三个同款缺陷（411 writing-stats、412 hybrid-search、及 382 声明的
未实现面）均属「TS 有端点/Rust 有函数但路由漏挂或完全未实现」——后续
新端点验收建议增加「mod.rs 路由注册数 vs ops/books 路由函数数」的机械
对照检查项。
