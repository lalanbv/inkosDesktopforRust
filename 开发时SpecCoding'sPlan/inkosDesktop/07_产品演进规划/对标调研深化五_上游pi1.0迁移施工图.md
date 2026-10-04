# 对标调研深化五：上游 pi 1.0 迁移施工图

> 立项：629 号（2026-10-05）。上游 @earendil-works 已于 2026-10-01 发布 1.0.0、10-04 发布 1.0.2（三天三补，活跃稳定期），本仓 629 号已对齐补丁线 0.87.1；本施工图为 0.87.1→1.0.2 迁移专项的可执行依据。**证据全部来自三版本 tarball 对勘**（/tmp 差分现场已清，结论固化于此）。

## 一、版本时间线与决策锚

| 版本 | 发布日 | 性质 |
|---|---|---|
| 0.87.0 | 09-21 | 本仓 548 号（R30）迁移落点，服役至 629 |
| 0.87.1 | 09-22 | 补丁：openai-completions 空 text 块过滤 + anthropic UA 串更新 + 模型注册表刷新（629 号已落） |
| 0.99.0–0.99.2 | 09-29~30 | 1.0 前奏 |
| **1.0.0/1.0.1/1.0.2** | 10-01~04 | **本施工图目标线** |

**裁决依据**：1.0.2 类型面相对 0.87.0 对本仓消费符号**纯加法**（下表实证）；但传递依赖 openai SDK 6.40.0→7.19.0 为大版本跳升，且 mock 门禁覆盖不到真实 provider 行为——须按本图验证协议走完整活体回归，不得裸 bump。

## 二、消费符号清单与 1.0.2 兼容性实证（逐一比对完成）

本仓全量导入面（grep 全仓枚举，packages/core+cli+studio）：

### pi-agent-core 主入口
| 符号 | 1.0.2 状态 |
|---|---|
| `Agent`（类） | ✅ 保留；仅加 `onProviderStreamEvent` 运行时选项透传 |
| `AgentEvent` | ✅ 保留；新增 `tool_execution_update` 发射路径重构（事件名/载荷形状逐字段等价：toolCallId/toolName/args/partialResult） |
| `AgentMessage` / `AgentTool` / `AgentToolResult` / `AgentToolUpdateCallback` | ✅ 保留；AgentToolResult 加可选 `structuredContent`/`isError`，AgentTool 加可选 `outputSchema`（全加法） |

**index 移除面（破坏性）**：`harness/*`（含 compaction 全族 shouldCompact/findCutPoint/compact 等）、`search/*`、`uuidv7`、pi-telemetry 再导出族（InMemoryTelemetryContext/NOOP_TELEMETRY_CONTEXT 等从 agent-core 入口消失）。**本仓零消费实证**：本仓压缩为自建 `session-compaction.ts`（R32 三体系分工裁决），telemetry 直依 `@earendil-works/pi-telemetry` 包本体，无任何 `pi-agent-core/harness|search|node` 子路径导入。

### pi-ai（主入口 + 3 个 api 子路径 + providers/all）
| 符号 | 1.0.2 状态 |
|---|---|
| `Api`/`Context`/`Message`/`Model`/`UserMessage`/`AssistantMessage`/`SimpleStreamOptions` | ✅ 核心面零破坏；`Model` 重构为 extends `BaseModel`（字段同集）；AssistantMessage 加可选 `thinkingLevel` |
| `AssistantMessageEventStream`/`createAssistantMessageEventStream` | ✅ 签名逐字一致 |
| `normalizeContext`（utils/transcript） | ✅ 签名逐字一致：`(context: Context) => TranscriptContext` |
| `isContextOverflow` / `validateToolArguments` | ✅ 签名逐字一致 |
| `getBuiltinModel`（providers/all） | ✅ 保留；providers/data 模型注册表全量刷新（新模型入列+价格更新——**迁移后须复核 service-presets/service-resolver 引用的具体模型 ID 仍可解析**） |
| `Type`/`Static`（typebox 再导出） | ✅ 依赖 typebox 双版本同为 1.3.27 |

**pi-ai index 移除面**：`./images-models` 再导出移除 + Images 族类型重命名（ImagesApi→ImageApi 等）。**本仓零 Images 面消费**（上表即全部）。

### pi-telemetry
| 符号 | 状态 |
|---|---|
| `InMemoryTelemetryContext`/`NOOP_TELEMETRY_CONTEXT`/`TelemetryContext`/`TelemetrySpan` | ✅ **1.0.2 tarball 对勘完成（629 号补录）**：0.87.1→1.0.2 差分 10 文件**全为 .map**（source maps），.js/.d.ts 零语义变化，四消费符号逐一在位（memory.d.ts/noop.d.ts/index.d.ts 实证）——迁移时随队 bump 零风险 |

## 三、适配器行为差分（本仓直连三适配器）

| 适配器 | 差分规模 | 语义归类 |
|---|---|---|
| `api/openai-completions.js` | 15 行 | ①空 text 块过滤（0.87.1 已入）②`resolveSamplingParams` 接入 `samplingParamsByThinkingLevel` ③`onProviderStreamEvent` 观察钩子 |
| `api/anthropic-messages.js` | 201 行 | ①**PiAnthropic 子类：禁 SDK 默认凭据链**（ANTHROPIC_PROFILE 配置文件/federation env 不再被暗中交换）②新增 ANTHROPIC_* 联合身份 env 认证面 ③beta 头换代 `mid-conversation-tool-changes-2026-07-01`→`inline-tools-2026-09-15` ④auth 断言重构（等价）⑤claudeCodeVersion UA 串更新 |
| `api/openai-responses.js` | 43 行 | 待迁移时逐行复核（规模小，预期同类：sampling/onProviderStreamEvent） |

**底层传递跳升**：`openai` 6.40.0→7.19.0（大版本）、`@anthropic-ai/sdk` 0.124.0→0.129.0。这是 mock 门禁盲区所在——两 SDK 的重试/超时/流式解析默认值变化不在任何单测可见面。

## 四、本仓影响面

1. **transcript 持久化**：agent-core 1.0.2 的 agent-loop 会给最终 AssistantMessage 注入 `thinkingLevel` 字段 → 若该对象整体进 transcript JSON，则新增字段。session-transcript-schema 为本仓自建，**迁移时核实 additive 字段容忍性**（对齐 526 号 statusRaw「无键形态」教训：宁加 skip 序列化也不要让中间态进持久层，除非产品需要）。
2. **engine-contract-diff 48 对照**：Rust 引擎自有解析器零影响；但差分器 SSE/事件维度以 TS 侧行为为对照基线，迁移后必须复跑确认 0 分歧。
3. **service-presets/service-resolver**：模型注册表刷新可能增删模型 ID，预置服务引用的模型须逐一仍可解析。
4. **Rust 引擎**：零改动（pi 系为 TS 侧依赖）。

## 五、迁移施工序（单号完成，建议 630+）

1. ~~**telemetry 1.0.2 tarball 对勘**~~（**629 号已补录完成**：差分 10 文件全为 .map，四消费符号在位，随队 bump 零风险）+ 三包 1.0.2 指纹核验协议（629 号先例：pnpm-workspace.yaml overrides 与 minimumReleaseAgeExclude 双点同步改，遗漏 overrides 会被静默压回旧版——629 号实证坑）。
2. bump：pnpm-workspace.yaml（overrides+exclude 清单含 chord）+ packages/core/cli package.json（specifier+cli overrides 字段）→ `pnpm install --registry https://registry.npmjs.org/`（镜像滞后绕行备案 628 号）→ 符号链接指向核验（.pnpm 残留旧版目录无害，以 readlink 为准）。
3. 适配器差分逐行复核：anthropic-messages 201 行重点核 PiAnthropic 凭据链变化对本仓 header 式 auth 的影响（本仓走显式 apiKey，预期净受益：行为更可预测）。
4. **transcript thinkingLevel 落盘裁决**（第四节第 1 条）。
5. 门禁矩阵：gate:ts 七步全量 + 双引擎活体差分（含 SSE 维度）+ mock-llm 活体走查（发送/中止/换向三场景，624 号方法论）+ cargo:testgate（src-tauri 面若有 E2E 触达 TS 包则必跑）。
6. **真 provider 冒烟**（若可配 key）：至少一个 openai 兼容端点+一个 anthropic 端点各一轮真实往返——openai SDK 7 传递风险的唯一直接覆盖手段。**工具已入库（630 号）**：`node scripts/provider-smoke.mjs --service <name> --model <id>`（A 非流式+B 流式双链，key 从 .inkos/secrets.json 或 env 解析；key 缺失退出码 2 给可行动指引；成功路径接线已经 mock 活体验证）。三适配器覆盖矩阵见脚本头注释。
7. bench:gate 零回退复核（SSE parse 面在 627 号已优化，确认 pi-ai 升级不引入 TS 侧回退——TS 侧无基准，以 vitest 时长漂移备案即可）。

## 六、风险登记与路线裁决（630 号更新）

| 风险 | 等级 | 缓解 |
|---|---|---|
| openai SDK 7 默认行为漂移（重试/超时/流式） | 高 | 施工序第 6 步真 provider 冒烟（脚本已入库）；不可得则迁移改双号（先 0.99 观察线） |
| anthropic 凭据链语义变化 | 中 | PiAnthropic 禁默认链对本仓显式 key 形态净受益；复核 federation env 未设 |
| 模型注册表刷新破坏 service-presets 引用 | 中 | 施工序第 3 步逐 ID 解析探针 |
| transcript 新字段污染持久层 | 低 | 落盘裁决先行（第 4 步） |
| 镜像滞后（npmmirror） | 低 | --registry npmjs 绕行（628 号备案） |

**路线裁决（2026-10-05，630 号）——缓行**：
1. **真 provider 冒烟不可行实证**：仓/家目录均无 .inkos/secrets.json，pi-env-keys 全表 provider env（ANTHROPIC/OPENAI/DEEPSEEK/ZAI_API_KEY 等）实测全 unset，~/.zai 仅 MCP 日志——无任何可用真实 key。
2. **0.99 观察线降险预设被实测否定**：0.99.2 tarball 实测 dependencies 已带 `openai@7.19.0`（SDK 大版本跳升在 0.99 线即发生）——观察线=同等传输风险+两天即被 1.0 取代的死线孤儿 pin，缓释价值为零。
3. **缓行代价可控**：pi-ai 仅服务 Node 回退端/CLI（桌面默认 Rust 引擎不触达 provider 传输）；0.87.1 已含空 text 保护补丁（629 号）；注册表刷新/anthropic 凭据链改进对回退端非关键。
4. **复启条件**：任一真实 key 可得（env 或 secrets.json）→ 先跑 `scripts/provider-smoke.mjs` 三适配器矩阵（anthropic-messages/openai-completions/openai-responses 各一端点）→ 全绿后按本图七步施工序执行 1.0.2 迁移专项。

---
629 号摸底全证据链：三包 tarball 对勘（pi-ai 三版本/pi-agent-core·pi-telemetry 双版本，diff 输出逐条归类于上文）、本仓导入面全量 grep 枚举、安装指纹核验（readlink 实链）。0.87.1 补丁升级已在本号落地并过类型门禁。
