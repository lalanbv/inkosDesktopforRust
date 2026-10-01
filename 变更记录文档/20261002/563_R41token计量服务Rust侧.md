# 563 号：R41 token 计量服务——Rust 侧（计量核心+聊天挂线+观测端点+span schema 计量面）

日期：2026-10-02
类型：feat(engine+core)，v7 P1 上下文管理三件套首件（544 号 §5 深化设计落地）

## 选题来路

v7 路线 P0（R38/R39/R40）与 P1 之 R42（spill）已清，R41 为 P1 剩余首项：
长篇主链缺 token 计量观测面——启发式估算散在 context_assembly（无 usage
锚点校准），会话面无任何计量出口。依赖项 R33 span schema（551/552）已落，
计量字段有容器。

## 实施

- **计量核心** `engine-rs/src/utils/token_meter.rs`：
  - `TokenMeter`（Mutex<SurfaceFold>）：追加 O(1)（节点 push+计数累加）、
    `measure()` 克隆节点表 O(surface) 即取即弃——快照值语义，锁内不执行
    用户代码（零共享可变状态惯例）；
  - 启发式复用 `estimate_text_tokens`（CJK 1:1/其余 4:1，双端已锁）；
  - usage 锚点：最近成功调用 usage 总量÷记锚时启发式总量=校准比
    （clamp 0.25–4.0）；收缩防御=新总量较前锚腰斩且表不缩→拒绝换锚
    （dsh「总量不低于路由定价锚点时复用」的确定性化）；
  - 覆盖率=锚时启发式/现启发式（min 1.0，4 位舍入）；usage total 缺省
    回退 input+output；窗口未知（0）恒不判超窗；
  - `estimate_prepared_surface`：写作链输入准备面计量纯函数。
- **双端 golden** `packages/core/src/__tests__/golden/token-meter-vectors.json`
  （12 case：三态/clamp/回退/超窗/UTF-16 代理对/空表/锚先于追加）+ Rust
  回放测试 `tests/golden_token_meter_diff.rs`（TS 消费 564 号）。
- **聊天挂线** `agent_loop.rs`：`LoopUsage` 加 `model`（serde skip——响应面
  键形零漂移；去 Copy）；`run_agent_loop` 加 `Option<&TokenMeter>`——开局
  入表 system+历史+指令、每轮 usage 入锚+assistant 面入表（内容+tool_calls
  参数串）、工具结果入表（8000 截断与请求面同口径）；None 零行为。
- **路由面** `agent_route.rs`：RouterLoopChat 透出 endpoint.model；每请求
  新建 meter（表语义=当前请求面，跨轮不叠计），轮末快照留痕注册表
  （sessionId→末轮快照，cap 64 逐出最旧，成功/失败/中止同点尽力而为）；
  `GET /api/v1/context-meter?sessionId=`（最小 utility 路由——无状态依赖；
  无记录 404/缺参 400）+ 端点测试（camelCase 键面锁）。
- **写作链** `write_next.rs` prepare_write_input 返回处
  estimate_prepared_surface 计量 + tracing::info 留痕——观测不阻断；超窗
  告警已有 budget notes 通道（ContextLens 消费）不重复建设（备案）。
  **偏差备案**：初版走 stage_log 被 `stage_logs_emit_ordered_stage_messages`
  当场拦截——127 号阶段流是边界叙事契约，计量非阶段，改 tracing。
- **span schema 计量面** `inkos-ai-request-schema.json` end 加 4 可选键
  `inkos.usage.{prompt,completion,total}_tokens+source`（加法式，version
  保持 1——552 先例）；TS 构造器 `InkosAiRequestUsage`+usage 缺省零键；
  `ModelChainRunOptions` 泛型化+`usageOf` 提取器（缺省 undefined→零行为，
  调用方接线随 564 studio 面同批）；双端键集测试同步（Rust EXPECTED_END_KEYS
  2→6+source 枚举对齐 MeterSource）。

## 门禁

- clippy 双 crate 0；cargo:testgate engine-rs 43 目标 1887 passed
  （+1 目标=golden_token_meter_diff；duel INKOS_DUEL=1 注入内）+ src-tauri
  23 目标 576 passed；
- gate:ts 七步全绿（typecheck/test/audit:npm/build/node-fallback-smoke/
  engine-contract-diff/export-epub-smoke——活体差分器含 tool-catalog 32 件）。

## 教训

- 观测面不得混入叙事契约通道：stage_log（阶段边界）≠tracing（引擎日志），
  挂点选择先查通道语义既有测试锁。
- 全局静态注册表的测试污染：server 测试插桩后 retain 清理（debug/tools
  只读无此问题，快照注册表是首个带写入的全局态端点）。
- 表语义裁决要有明确边界：meter 每请求新建（当前请求面）而非跨轮持久——
  append-only 折叠表下"表面缩回"不可能存在，收缩拒绝只在一轮内 usage
  腰斩时触发（测试从"表面缩回再入账"幻觉子场景修正）。
