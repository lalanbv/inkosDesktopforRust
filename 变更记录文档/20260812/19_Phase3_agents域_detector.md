# 19 — Phase 3 agents 域：detector（AIGC 检测）

> 日期：2026-08-12
> 范围：engine-rs（Rust 化迁移，Strangler-Fig 阶段）
> 关联：`18_Phase3_agents域_style-analyzer.md`

## 移植内容

### `agents/detector.rs`
移植 `detector.ts`（120 行）——外部 AIGC 检测 API 调用（GPTZero/Originality/custom）：

- **纯解析内核**（可单测）：[`parse_gptzero_score`]（`documents[0].completely_generated_prob`）/
  [`parse_originality_score`]（`score.ai`）/[`parse_custom_score`]（顶层 `score` 须 number）
- **HTTP 编排**（[`detect_ai_content`]）：reqwest::Client 注入，三 provider 分派（X-Api-Key / Bearer 头）
- [`DetectionResult`] / [`DetectError`]（Http/Parse/Transport）

## 关键技术点

- **纯内核 + HTTP 编排分层**：三个 provider 的「JSON 响应 → score」提取为纯函数（load-bearing，
  缺字段/类型错 → 0.0，对齐 TS `?? 0`）；HTTP 编排单独函数。纯函数可单测，HTTP 需 mock。
- **`reqwest::Client` 注入**：`detect_ai_content(client: &reqwest::Client, ...)` 便于测试替换 client
  （与 StateStore trait 注入同思路）。本项目第二个 HTTP 客户端（streaming_client 之后）。
- **api_key/detected_at 注入**：TS 从 `process.env[config.apiKeyEnv]` 读 key、`new Date()` 读时间——
  Rust 由调用方注入（纯内核不读 env/时钟，对齐 chapter_workspace/style_analyzer 范式）。
- **DetectError 分层**：Http（非 2xx，含 status+body）/ Parse（json 反序列化失败）/ Transport（网络）。
  `Transport(#[from] reqwest::Error)` 服务 `send().await?`，`.json()` 失败用 `map_err(|e| Parse{...})`
  显式分类（语义区分网络故障 vs 响应解析故障）。
- **json 用 `reqwest::Response::json`**：失败返 reqwest::Error（非 serde_json::Error），故 `Parse.source` 类型是 reqwest::Error。

## 设计决策

| 决策点 | 选择 | 理由 |
|--------|------|------|
| 架构 | 纯解析 + HTTP 分层 | 纯函数可单测（7 测试覆盖 load-bearing 解析）；HTTP 编排需 mock 单独处理 |
| client | reqwest::Client 注入 | 可测试性（注入 mock client）；复用 crate 已有 reqwest(rustls) dep |
| env/时钟 | 调用方注入 | 纯内核范式；不依赖系统环境 |
| 测试范围 | 仅纯解析（7 测试） | HTTP 路径需 wiremock/mockito，本轮未引入（留待 HTTP 测试基础设施统一建立） |

## 验证

| 维度 | 结果 |
|------|------|
| 新增单测 | **7 项**（gptzero/originality/custom 解析 + 缺省/类型错 + 错误显示）全绿 |
| 全量 lib 测试 | **462 passed**（上轮 455 + 本次 7） |
| golden 差分测试 | **22 passed** |
| clippy 双模式 | 零警告 |
| 新增依赖 | 无（复用 reqwest） |

## agents 域进度

纯逻辑 + HTTP 叶子：
- ✅ detection_insights / settler_parser / settler_delta_parser / ai_tells / style_analyzer
- ✅ **detector**（本次，首个 HTTP agent）
- ⬜ radar-source（HTTP 爬取，多个平台） / writer-parser / sensitive-words

agents 域已移植 6 个叶子。HTTP 类 agent 的测试基础设施（mock server）是后续批量移植 radar-source 等 HTTP agent 的前置——可作为下一独立子目标。
