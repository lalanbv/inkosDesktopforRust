# 799 号：hook 准入语义 rust 对齐——未知 id upsert 创建化（promises timeline id 收敛）

日期：2026-11-01。前置：798 号登记（promises timeline hook id 语义——TS reducer 对 candidates 派生 hook-N vs rust 接受显式 id upsert）。

## 一、根因精确化（比 798 定性更深一层）

798 号将分歧定性为「reducer 派生 id vs 显式 id」——本轮逐层定位发现真正的拒绝点**不在 reducer**（TS reducer state-reducer.ts:85 对未知 id upsert 本就创建），而在**上游两道 TS 独有守卫**：
1. `writer.validateSettlementHookIds`：settle 工具 validate 回调拒 upsert/mention/resolve/defer 中一切非台账 id；
2. `hook-arbiter.arbitrateRuntimeStateDeltaHooks`：arbiter 对未知 id upsert 硬抛 "Hook upsert references unknown hook id"。

rust 语义：reducer `apply_hook_ops` 对新 id upsert 走 `evaluate_hook_admission` 准入评估（duplicate family 同族合并/拒）后创建——LLM 显式 id 是合法的新钩子创建通道。

## 二、修复（rust 语义镜像）

1. **writer 守卫收窄**：`validateSettlementHookIds` 只校验**引用类操作**（mention/resolve/defer 必须指向台账既有 id——指向不存在状态引用无意义不可创建）；upsert 放行（新 id 创建=创建可见可审，hallucination 面收窄合理化）。
2. **arbiter 创建化**：未知 id upsert 不再硬抛——走创建（reducer.ts:85 同效；duplicate-family 准入评估为 rust 侧增强，TS 侧缺口登记 695）。
3. **mock settle 支显式 id**：settle 工具支 hookOps.upsert 携带模板 H01/H02/H03 全记录（与 rust 文本协议逐字同源），弃 795 时代的 candidates 形态。

## 三、活体验证

- /tmp/wtest9：write-next 后 promises timeline ids = **H02/H03/H01（显式 H id，与 rust 同族）**——798 时代的 hook-N 派生 id 消失；currentChapter 4（含探针双写污染行，真实流 3）。
- diff 报告模式：promises 分歧从「长度 6 vs 4 + id 全不同」收窄到「长度 3 vs 4 + H01/H02/H03 同族对齐（rust 多 H04=其内部 analyzer 创建）」。currentChapter node 3 = rust 3 ✓ 双腿一致。
- core vitest **979/979** + tsc 归零 + 权威 pnpm -r test 全绿（176/134/23 文件）。

## 四、剩余 worklist（695 主工程）

- promises timeline 长度 3 vs 4：rust 多 H04（rust 链内部 analyzer 创建，mock 未覆盖其创建路径）——mock 状态追踪分支补 H04 或定性 rust 内部行为。
- quality-trend currentChapter 0：审计指标记录移植（listReviewMetrics 账本）。
- roster-candidates / detect：生成/存储移植（维持 795 定性）。
- reject 回滚级联（node 清空章节 vs rust 保留）：rollback 语义对齐（797 定性维持）。

## 五、教训

1. **守卫的层级决定语义**：同一个「未知 hook id」拒绝，放在 writer validate 层 vs reducer 准入层，语义完全不同（前者把 LLM 的新建钩子通道整个堵死，后者只做同族去重）——对齐时要找**拒绝发生的实际层级**，不是消息文本。
2. **错误消息是溯源锚**："Unknown hook id(s)"（writer 层）与 "Hook upsert references unknown hook id"（arbiter 层）两条消息定界了两道守卫——好消息是错误文本本身就是 grep 锚点。
