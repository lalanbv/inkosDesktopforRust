# 1035 号 · earendil 浅层十文件运行时行为面走查轮

> 1034 出口执行（1022 归并的 10 文件消费面——1024 只审了双栈交界+pi-dispatch
> 类型面，本轮=**运行时行为面**逐文件走查）。零代码改动轮。

## 一、10 文件逐面定性

| 文件 | earendil 消费 | 定性 |
|---|---|---|
| forecast-tools.ts | type-only（AgentTool/AgentToolResult/UpdateCallback）+Type/Static（typebox alias 面） | 类型擦除后零运行时栈差异 ✓ |
| skill-tool.ts | type-only + sanitize（AgentMessage 参数） | 1024 已验核心字段同形 ✓ |
| session-transcript.ts | type-only（AgentMessage） | 零运行时 ✓ |
| pi-env-keys.ts | 零消费（自持镜像，注释溯源 0.87） | ✓ |
| telemetry/inkos-ai-request.ts | NOOP/InMemory 运行时值+re-export | 1.5.2 telemetry 4/4 符号兼容 ✓ |
| pi-dispatch.ts | **运行时值**（normalizeContext+per-API streamSimple） | **行为探针 SAME:true** ✓ |

## 二、normalizeContext 行为探针（真面排除实锤）

仓内现装 0.87.1 与 1.5.2（/tmp 探针目录）**双版直调同输入**：折叠输出**逐字节一致**（systemPrompt 折叠为首 system 消息+timestamp:0 形态同）——1.5.2 的折叠语义无漂移。该面是 mock 测试不敏感的真实 API 面（prompt 结构变化只在真实 API 显形）——运行时行为探针=唯一可靠验证手段。

**earendil 1.5.2 归并的 10 文件运行时行为面=全部净。**

## 三、下一号自 1036 起

出口=earendil-mz 最终单包合并（远期）/ 下一巡检周期 / 用户侧待办=Fork 推送（含 955-1035）。
