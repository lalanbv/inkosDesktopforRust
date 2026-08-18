# 133 号（Phase3）：think 剥离器——内联思考块剥离

日期：2026-08-18 · 前置：130 号备案项（e7c04465 引入的自包含小面）

## 背景

部分 OpenAI 兼容服务（MiniMax M2.x、网关代理的 DeepSeek-R1 类）把思考内容
以 `<think>...</think>` 内联在 content 开头返回（issue #329）——不剥会混进
章节/对话正文。TS `think-tag-stripper.ts`（三态流式剥离器）+ provider 传输
接入，Rust 此前缺失。

## 实现（TS think-tag-stripper.ts 逐字）

- `LeadingThinkTagStripper`（streaming_client.rs）：三态机
  Detecting/InsideThink/Passthrough——
  - Detecting：跳过 JS 空白（`\s` 语义含 `\u{feff}`，复用 length_metrics
    的 `is_js_whitespace` pub 化）后判断是否 `<think>` 前缀；不定则缓冲
    零发射（思考内容根本不会被发出，而非先展示再消失）；一旦偏离即
    passthrough 全量吐出（含前导空白）。
  - InsideThink：等待 `</think>`；闭合后吐出去除开头空白的内容；未闭合
    继续吞。
  - flush：缓冲剩余原样返回——**未闭合块不剥离**（正文根本没生成，剥掉
    会丢数据）。
- `strip_leading_think_block`（非流式 = push 全文 + flush）。
- 接入三处：
  1. 流式 chat 路径：Delta 增量先过剥离器再并入正文/喂 monitor（空产出
     不发射——monitor 字数与 UI 增量都不见思考内容）；push 与 finish 两个
     解析循环同款。
  2. 流结束 flush 回填（monitor.finish 前，只并正文不发增量）。
  3. 非流式 chat 路径：content 提取即剥；顺带补齐 TS 非流式同款
     output-limit 守卫（finish_reason=length/max_tokens → "model reached
     the output limit"，此前 Rust 非流式无此守卫）。
- responses 传输不接（TS 同——stripper 为 chat completions 传输专属）。

## 测试（+8）

- TS 九例单测移植（strip_leading 五例 + 流式四例：跨块抑制/前缀偏离回吐/
  passthrough 中文字样/未闭合 flush 原样）。
- 真 HTTP mock 集成三例：跨块 think 块整体吞（content=纯正文）；**未闭合
  块 + 正常 [DONE] → flush 回填数据不丢失**；非流式裸 JSON 剥离。

## 验收基线（终态）

| 项 | 结果 |
|---|---|
| `cargo test --lib` | **1184** 过（+8） |
| `cargo test --test e2e_write_next_contract` | **190** 过 |
| `cargo test --test golden_leaf` | **70** 过 |
| `cargo test --features export-bindings --lib` | **1342** 过（+8） |
| `cargo clippy --all-targets` | 零警告 |
| duel 对跑（INKOS_DUEL=1） | **8/8** |

## 备案（后续轮）

1. runner harness 类重构（agentCtxFor/worker-agent/production harness）与
   skills 生产模式绑定——合并 e7c04465 剩余主体。
2. writeProductionRunSnapshot（生产运行快照持久化）。
3. trajectory 遥测头（agentTrajectoryHeaders——LLM 请求观测头）。
