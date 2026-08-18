# 127 号变更记录：`log` SSE 事件——流水线阶段日志机制（回调 + 主链五阶段 + 广播装配）

## 一、勘测结论（决定实现范围）

TS `log` 事件的真实形态：

- **源**：`buildPipelineConfig` 构造 `createLogger({tag:"studio", sinks:[scopedSseSink, consoleSink]})` 挂入 PipelineConfig——SSE 负载 `{sessionId?, executionId?, level, tag, message}`（无 timestamp/ctx）。
- **发射点**：pipeline runner 的 `logStage`（"阶段：…" 前缀 info）/`logInfo`/`logWarn` 双语阶段叙事（审核轮次、基础设定评分等 30+ 点位）。
- **`inkos.log` 为残留面**：`/api/v1/logs` 读该文件但全仓**无写入方**（createJsonLineSink 仅导出未接线）——持久日志面两侧同为空转，不构成迁移项。

结论：逐字复刻 30+ 点位不经济也无必要（UI 日志面板渲染任意字符串）；**补机制 + 主链阶段边界**为等价形态的部分对齐（诚实备案，非逐字）。

## 二、交付

### 1. 回调机制（WriteNextConfig）

- `PipelineLogFn = Arc<dyn Fn(&str /*level*/, &str /*message*/) + Send + Sync>`；`WriteNextConfig.on_log`（Default/from_project None）。
- `stage_log(config, language, zh, en)`：TS `logStage` 同构——"阶段：{msg}" / "Stage: {msg}" 前缀、info 级、按书语言双语。

### 2. 主链五阶段发射（write_next_chapter_locked）

规划与上下文编排 → 撰写章节草稿 → 审核环（auto 路径）→ 真相结算与校验 → 章节落盘（manual 链四阶段，审核环跳过）。

### 3. 广播装配（books_routes）

`with_event_broadcasts(config, hub)`（127 号抽出，任意 hub 可复用——write-next 路由运行时/测试）：挂 `on_context_compression`（126 号）+ `on_log` → `{level, tag:"studio", message}` 广播（books 面无会话标记，TS 同面）。`write_next_config_with_events` 委托之。

## 三、测试（+2：lib 1168 / E2E 187）

- `stage_logs_emit_ordered_stage_messages`（lib 全链：manual 链四阶段**有序**断言 + info 级）。
- `with_event_broadcasts_emits_log_events`（e2e：hub 事件名 + 三字段负载形态 + 无标记）。

## 四、验证基线（127 号时点）

| 项 | 结果 |
|---|---|
| `cargo test --lib` | **1168** 过 |
| `cargo test --test golden_leaf` | 76 过 |
| `cargo test --test e2e_write_next_contract` | **187** 过 |
| `cargo test --features export-bindings --lib` | **1327** 过 |
| `cargo clippy --lib --tests --bins` | 零警告 |
| TS `packages/core` vitest | 185 文件 / 1798 全过 |
| `INKOS_DUEL=1 cargo test --test strangler_duel` | 7/7 |

过程勘误：stage_log 插入点曾落在既有 `#[allow(too_many_arguments)]` 与其函数之间致属性孤儿（clippy 抓获）——归位修复。

## 五、事件词汇表终态

Rust 59 + session:title/llm:progress/context:compression/log = **63/66**；余 thinking:start/delta/end（UI hub 未订阅——agent 响应流另道，备案）。

## 六、下一步候选

1. SSE 事件面双端对跑（duel 第 8 测试——同根双进程订阅 /api/v1/events，比对共享词汇事件负载形态）。
2. thinking:* 三事件（若勘测确认 UI 消费路径）。
3. 实切条件到位后真实流量切换。
