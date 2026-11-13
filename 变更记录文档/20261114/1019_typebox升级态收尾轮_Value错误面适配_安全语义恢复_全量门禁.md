# 1019 号 · typebox 升级态收尾轮——Value 错误面适配 + 安全语义恢复 + 全量门禁

> 1018 台账操作序执行：三行清单（alias+mz 0.73.1）→ install → Value.Errors
> 1.x 运行时适配 → 全量门禁。**升级态正式落地**（typebox 1.4.7 + mz 0.73.1）。

## 一、Value.Errors 1.x 错误面适配（toolArgumentIssues 重写）

探针全谱（四形态）：
- **type 错误**：`keyword:"type"`, `params:{type:"integer"|"array"|...}`；
- **const 错误**：`keyword:"const"`, `params:{allowedValue:X}`；
- **Union-of-Literal**：1.x **展开为多条 const**（每字面量一条）+可选 anyOf 汇总条；
- instancePath 格式与 0.x path 同形（`/a/0`）；**1.x 错误对象不带 value**。

适配层（tool-arguments.ts）：
- `path ← instancePath`；`type ← keyword`；`expectedType ← keyword==="type" 的 params.type`；
- **allowedValues 重建**：按 instancePath 聚合多条 const 的 allowedValue（恢复 0.x enum 语义，≤32 截断与 1024 字节上限保留）；
- **received 重建**：1.x 不带 value → `valueAtPath(value, instancePath)` 从调用方原值定位（RFC 6901 JSON Pointer 分段，含 `~0/~1` 转义与数组下标）；
- 输出契约不变（path/type/message/actualType/expectedType?/allowedValues?/instruction?——给 LLM 的结构化修复提示格式零变化）。

## 二、安全语义恢复：Check-before-Decode（防线被清理语义绕过的实锤与修复）

work-initial-production.flow 恰红定性：**typebox 1.x 的 `Value.Decode` 带「清理语义」**（探针实锤：`Decode({maxShots,projectId}, additionalProperties:false)` → `{maxShots}`——projectId 越权字段被静默剥除）。bound-production 的 `WORK_TARGET_BOUND` 防线（`"projectId" in params`）依赖 additional 字段存活→**防线被绕过**（越权 retarget 静默降级为忽略）。

修复（capability-registry.ts）：**Check-before-Decode**——Decode 前先 `Value.Check`（1.x Check 对 additional 严格拒绝，探针实锤 false+additionalProperties 明细），Check 不过走 794 号错误明细路径=fail-loud。防线恢复 ✓（work-initial 复绿）。

**教训**：校验库的 Decode/Check 分离语义（1.x Decode=clean+decode）是安全防线形态的隐性依赖——升级后防线必须逐条复核「字段在场性假设」。

## 三、其余红点清零

- capability-registry:114：错误明细串 `issue.path` → `instancePath`（1.x 形态）；
- agent-tools:664：逆变位注解不可行（同 1018 work-creation 定案）→ 体内 `p = params as ProposeActionParamsType` 一次具体化（体内引用全改）；
- worker-agent:377：`details: submitted as Static<TParameters>`（0.73.1 AgentToolResult.details 非可选；分支前置 throw 保证非空）；
- agent-session:1433：**双栈边界 cast 参数面第二点**（assistantInvokesSkill 同为 earendil 型签名）——1018 同族先例形扩展。

## 四、测试校准（行为面变化的记录性改动）

- stream-contract.flow：Union-of-Literal 在 1.x 展开多条（type/const/anyOf 分列）→ issues 断言改**语义面 find-by-path**（不再逐索引假设条数顺序）；chapterIssue 键集合六键断言保留；**attempts 字段缺失备案**（0.67.1 为 3——0.73.1 的 beforeToolCall 抛错 fallback 形态差异，语义面 code+工具+issues 反馈不变，诊断字段差异记录）。

## 五、门禁终态（升级态）与 node 回退端写链回归 → 回滚决策

- **升级态**：typecheck 0 红 + core 1062/1062 单跑全绿——但 gate:ts 十腿中 **export-epub-smoke ✗**（隔离复跑实锤：**node 回退端 write-next 300s 未落第 2 章**——写链回归；node-fallback-smoke/engine-contract-diff 三活体腿同轮 320s 巨慢=同负载态）。Rust 主引擎侧全绿不受影响（testgate 2114+593+clippy 双零+engine-contract-diff/work-alternation 绿）。
- **定性**：mz 0.67.1→0.73.1 的 agent loop 行为变化（1019 已录 beforeToolCall 抛错 fallback 形态/attempts 缺失）连锁 mock 编排序列（按 0.67.1 行为编排）在 node 端写链不收敛——**node 回退端写链诊断=独立水面**（mock 序列 vs 新 loop 语义），非适配层缺陷。
- **决策=回滚升级态保绿基线**（0.34+0.67.1），**全部清偿成果以双兼容形态保留**（SchemaLike/pi-compat/边界 cast/Composite 改写/toolArgumentIssues 双版本归一适配层/Check-before-Decode——0.34 基线 typecheck 0 红+三包全绿 1062+893+81 ✓）。升级态落地顺延 1020（node 写链诊断轮先行）。
- **备案**：capability-registry 的 Check-before-Decode 在 0.34 下为纯冗余加固（0.34 Decode 保留 additional，Check 恒过）——无害，1.x 下为安全语义必需。

## 六、升级总账（1013→1019 七轮链）

1013 评估定钉 → 1014 勘察实装 diff → 1016 node_min 前置 → 1017 归并方案+试探 → 1018 A/B/D/C 清偿 → 1019 Value 适配+安全语义恢复+**node 写链回归拦截**（回滚保绿）。双栈现状：mz 0.67.1（运行时，0.73.1 待 node 写链诊断）+ earendil 0.87.1（浅层）——typebox 1.4.7 归并方案与全部兼容层已就绪。

## 七、下一号自 1020 起

出口=**node 回退端 write-next 写链回归诊断轮**（export-epub-smoke 复跑=验收）/ typebox 升级态落地收尾（诊断后）/ 用户侧待办=Fork 推送（含 955-1019）。
