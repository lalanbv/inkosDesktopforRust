# 694 号：minimax 元数据语义仲裁（688 号 §五.7 裁决：双端目录本已收敛，fork 继承形态为分歧残影）

日期：2026-10-15。类型：docs/audit（零代码变更）。前置：693 号（5af415c8）。

## 一、仲裁结论

688 号 §五.7 立项的「minimax 元数据语义分歧仲裁」经三面比对**裁决为已收敛**：双引擎的实际发射面（/services/models 列表 + chat 时模型卡）在本就共享的 endpoint 目录上逐字段一致；所谓分歧只存在于 fork 侧 resolver 的 pi 注册表继承形态（reasoning=true、provider="minimax"）——那是**从未匹配过 Rust 行为的 TS 独有残影**，2.0 合并恰好消除了它。fork 回归测试（已随 689-693 清偿退役）断言的正是该残影，维持退役。

## 二、三面比对证据

| 面 | engine-rs（产品契约权威） | TS 2.0（合并后） | 判定 |
|---|---|---|---|
| **/services/models 列表** | service_routes.rs:1378-1390：`{id,name,maxOutput,contextWindow}`，minimax 条目 maxOutput=131072/contextWindow=204800（providers_bank.rs:195） | server.ts:3897 `listModelsForService` 走 endpoints/minimax.ts:28 同值（131072/204800，releasedAt 亦同 2026-03-18） | **逐字段一致** |
| **chat 时 provider 归属** | service_routes.rs:1680 `llm.provider = family`（ProviderFamily::Openai → "openai"） | 上游 resolver：piProvider 兜底 "openai"（service-resolver.test 锁定 provider==="openai"） | **一致**（fork 残影 "minimax" 仅存在于 earendil 注册表继承链，两引擎真实面均不发射） |
| **chat 时 reasoning 行为** | providers_bank.rs minimax 全系 `capabilities: None` → 不请求 reasoning | endpoint 目录无 reasoning 字段 + resolver reasoning=false → 不请求 reasoning | **行为等价** |

上游自测锁定的语义（provider="openai"、reasoning=false、contextWindow=204800、maxTokens=131072）与 engine-rs 目录**四字段全部吻合**——上游测试锁定的恰是双端已收敛的形态。

## 三、历史脉络还原（分歧残影的成因）

fork 629/630 号把 TS pi 依赖对齐 @earendil-works 0.87.1 时，`resolveServiceModel` 的元数据继承改为读 earendil 内置注册表（MiniMax-M2.7 携带 reasoning=true、provider="minimax"）——这与 engine-rs providers_bank 的 capabilities-based 行为（无 reasoning）**在当时就已经分叉**，只是 fork 回归测试（R30b 期）把 TS 残影锁成了「契约」，而 /services/models 可见面两引擎同值（口径不同未见分歧），contract-diff 未覆盖 resolver 内部层。2.0 合并后 TS 改走 endpoint 目录（与 Rust providers_bank 同源镜像），残影随 llm/ 整体切换消失，分叉反而归零。

## 四、处置

- 上游 service-resolver.test.ts（锁定收敛语义）保留绿 ✓（689 号轮已实跑确认）。
- fork service-resolver-regression.test.ts（断言残影）维持退役，不复活。
- engine-rs 零触碰（providers_bank 即契约权威，与 TS endpoint 目录镜像同源）。
- contract-diff 三域重组时，/services/:service/models 面按「双端目录同源」语义正常入列即可，无需豁免。

## 五、教训

1. **「语义分歧」要区分「面分歧」与「实现层残影」**：resolver 内部层的元数据来源不同 ≠ 引擎行为不同——仲裁前先问「这个字段在哪个可观测面发射、两引擎在该面的实际值是什么」。本号三面比对把一项疑似分歧定性为残影，避免了把残影「移植」给 Rust 的反向错误。
2. **契约测试锁的可能是残影**：fork 回归测试锁的是 TS 实现行为而非双端契约——测试红绿只证「实现与测试一致」，不证「实现与对端一致」；这正是 contract-diff（对端活体差分）存在的理由，也是它缺席的层（resolver 内部）潜藏分叉的原因。
