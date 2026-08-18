# 102 号变更记录：import 链中途截断（章粒度安全点，101 号同族收口）

## 一、背景

101 号偏差备案第 2 条：TS `importChapters` 每章 + 分析后有检查点（runner.ts L2826/L2858/L2896），Rust `import_chapters_chain_with_resume` 未接中止。TS 侧聊天面 `import_chapters` 工具同样经 `runPipelineWithAbortSignal` 注入 signal（agent-tools.ts L1274）——与 writer 委托同族。

TS 检查点勘测：**入口（L2826，锁前）/ 每章回放头（L2858）/ 分析后落盘前（L2896）**。章粒度安全点语义：检查点间以"章"为原子单位——此前各章已完整落盘（章节文件 + 真相面 + 索引 + 快照），中止即当前章分析与产物全部丢弃、零残留、已落盘章全部保留。

## 二、交付

### 1. `server/book_create_routes.rs`：链内三检查点

- `import_chapters_chain_with_resume` 增 `abort: Option<&AbortHandle>` 参数；`import_check_aborted` 助手 + `IMPORT_ABORTED_MESSAGE` 英文锚文本常量（与 101 号 `WriteNextError::Aborted` 同一文案源——执行器据此识别并本地化）。
- 三检查点逐位对齐 TS：①fn 头（load_book_config 前）②逐章循环头（analyze 前）③分析后、本章落盘前。
- `import_chapters_chain` 包装（REST `/books/:id/import`）传 None——TS REST 端点同样无 signal 面。

### 2. 聊天面接线（`interaction/import_chapters_tool.rs` + `agent_route.rs`）

- `tool_import_chapters` 增 `abort` 参数直通链；`ImportDeps` 增 `abort` 字段，`agent_route` 装配注入聊天轮 `abort_flag`（与 agent loop 轮询同一句柄——abort 端点 scope=chat 即可中止导入链）。
- 中止错误经聊天面透传英文锚文本（工具错误面与本文件既有英文 TS 逐字文案一致）。

### 3. E2E（`sub102_e2e`，1 测试）

- `chat_import_abort_mid_replay_keeps_landed_chapters_and_drops_current`：既有书（第 1 章 + 地基）续放第 2、3 章；两段节奏 analyzer mock（第 2 章即刻回包落盘、第 3 章标记后睡 600ms）→ 测试在**第 2 章已落盘、第 3 章分析在途**时（此刻断言锁死前提）POST abort → 断言：200 + `（无回复内容）`（66 号聊天面中止契约）+ 工具卡 `status=error` + `error` 逐字英文锚文本 + **第 1/2 章保留、第 3 章零落盘、索引恰两章、既有地基逐字未动**。

## 三、parity 要点

- 三检查点相位逐位对齐 TS L2826/L2858/L2896；Step 1（地基生成）内部无检查点（TS 同样——循环头检查覆盖）。
- 聊天面 signal 注入对齐 TS `runPipelineWithAbortSignal`；REST 面不接（TS 一致）。
- 错误锚文本与 101 号 write_next 链统一（`Operation aborted: the user requested to stop this task.`）。

## 四、偏差备案

1. **无确认面导入执行器**：TS studio 确认意图族里导入走聊天面工具（Rust `RequestedIntent::ContinuationImport` 枚举存在但无执行器装配，`unreachable` 守卫），两侧一致非缺口。
2. **导入中止错误在聊天面不本地化**：TS 聊天面 AbortError 呈现取决于运行时 DOMException 文案（Node "This operation was aborted"），Rust 用链内英文锚文本——呈现文本本就无 TS 逐字基准，锚文本选择与 101 号统一。

## 五、暂缓件（滚动）

- 101/102 号后写作/导入两链检查点族**全清**；P1/P2 全清。
- 候选余量：迁移收官审计轮（全备案复核 + 94/98 基线终版 + 迁移总结文档）、P3 精修批（层 3 secrets 保序 / responses 传输 / provider 特判族 / 模型卡元数据）。

## 六、验证基线

| 项 | 结果 |
|---|---|
| `cargo test --lib` | 1141 过 |
| `cargo test --test golden_leaf` | 76 过 |
| `cargo test --test e2e_write_next_contract` | **167** 过（+1：sub102；首轮批跑出现 1 例负载偶发失败，随后连续三次全绿复跑确认） |
| `cargo test --features export-bindings --lib` | 1300 过 |
| `cargo clippy --lib --tests --bins` | 零警告 |
| TS `packages/core` vitest | 185 文件 / 1798 测试全过 |

## 七、影响面与下一步（103 号候选）

1. **首选：迁移收官审计轮**——30-102 号全部偏差备案逐条销账复核、94/98 基线终版（检查点族全清后的状态刷新）、strangler 切换总结文档（迁移完成度盘点 + 切换就绪度结论）。功能面已无已知缺口，这是切换前最后一块。
2. 其次：P3 精修批（层 3 secrets 保序 / responses 传输 / provider 特判族 / pi-ai 模型卡元数据）。
3. 或：E2E 偶发失败治理（本轮观察到 1 例负载偶发——时序类测试的 deadline 预算放宽或串行化标记）。
