# 144 号（Phase3）：runner harness 类形态对齐——收益评估与备案关闭

日期：2026-08-18 · 性质：评估轮（无代码改动）· 前置：130-143 号

## 评估问题

TS e7c04465 把管线 agent 装配收进类形态（`PipelineRunner.agentCtxFor` →
`AgentContext` + `BaseAgent` 类层次 + `runWorkerAgent` 适配层）。Rust 以
显式参数传递 + task-local 作用域承载。是否值得把 Rust 重构为同款类形态？

## 差异面逐项对照（TS AgentContext 八字段）

| TS 字段 | Rust 承载 | 等价证据 |
|---|---|---|
| client | AgentRouter.resolve(agent)（109 号 effective_router：inkos.json 服务项 + secrets 热解析） | duel 模型配置域 8/8 |
| model | 同上（ResolvedEndpoint） | 同上 |
| projectRoot | WriteNextCtx / BooksRuntime 直传 | lib 1202 |
| bookId | 各链参数 | e2e 194 |
| logger | tracing 宏（target 分面） | 形态差异、零行为差 |
| onStreamProgress | ChatCompletionParams.progress（126 号 llm:progress 链） | SSE 事件面 66/66 |
| signal | WriteNextConfig.abort + check_aborted 检查点 | 131 号备案的传递时机差异（非类化引入） |
| activatedSkills | OPERATION_SKILLS task-local + router 出口注入（139 号） | sub139 e2e |

`runWorkerAgent` 本身是 TS 把 chatCompletion 升级到 pi-agent 运行时的
**内部适配层**——Rust 无 pi-agent 层（直连 OpenAI 兼容流式），其行为契约
（请求/流守卫/think 剥离/终态判定/trajectory 头）已在 130-134 号逐字对齐。
类形态是 TS 为补偿 AsyncLocalStorage 隐式传播的设计；Rust 的 task-local +
显式参数是更简洁的等价承载。

## 结论：正式关闭（纯结构重构，无净收益）

1. **行为**：零差异——八字段全部等价承载，无未移植行为面。
2. **契约**：duel 8/8（含全灰模拟）证明双端等价。
3. **可维护性**：**负收益风险**——Rust 参数显式传递获得编译期完整检查，
   是惯用法；类化将使 100+ 测试构造点 churn、偏离各 agent 模块现状，
   且引入与 TS ALS 补偿设计同构的间接层。
4. **回归风险**：大面积构造点改动换零行为变化——不符合
   "最好最安全可靠的可实施执行"目标约束。

后续如出现**新行为面**需求（如 agent 级 context 字段扩展），按需在现有
显式传递面上增补，不预铺类层次。

## 验收（评估轮——既有基线复核）

| 项 | 结果 |
|---|---|
| `cargo test --lib` | **1202** 过 |
| `cargo test --test e2e_write_next_contract` | **194** 过 |
| `cargo test --test golden_leaf` | **70** 过 |
| `cargo test --features export-bindings --lib` | **1360** 过 |
| `cargo clippy --all-targets` | 零警告 |
| duel 对跑（INKOS_DUEL=1） | **8/8** |

## 备案状态

**todo 清零**。合并 d3ba425e 的全部漂移面（130-143 十四轮）已消化完毕，
最后一项结构对齐备案经本评估正式关闭。后续轮次回归"按需轮"：用户指定
功能缺口、上游新合并、或实切条件（三项用户资源条件）就绪时的真实流量
切换演练。
