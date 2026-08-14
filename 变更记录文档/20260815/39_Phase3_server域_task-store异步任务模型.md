# 39 号｜Phase 3 server 域：task-store 异步任务模型（write-next 端点基建）

> 里程碑：`.inkos/tasks/{sessionId}.json` 任务快照存储全量移植——write-next /
> book-create 等异步端点的任务模型基建。write-next 的 Node 侧契约是
> fire-and-forget + SSE 推送（`write:start`/`write:complete`/`write:error`），
> 响应 `{status:"writing", bookId}`；任务快照承担跨会话的状态可恢复性。

## 背景

38 号后 write-next 只剩 runner 主体（chapter-analyzer 634 行 + `_writeNextChapterLocked`
+ 通知/webhook）与端点基建。按依赖顺序先把 server 侧的任务模型落地——它是
`/api/v1/books/:id/write-next` 与 `/create-status` 类端点的公共依赖，
且完全自包含（112 行、零 LLM 依赖）。

## 改动

### 新建 `engine-rs/src/server/task_store.rs`（~330 行，含测试）

- `StudioTaskSnapshot` / `StudioTaskExecution` / `StudioTaskStage`（camelCase
  序列化；version 固定 1）
- `save_studio_task_snapshot` / `load_studio_task_snapshot` /
  `delete_studio_task_snapshot`：`.inkos/tasks/` 下按会话存取删
- **同路径写入串行化**：TS 的 per-path promise 队列（writeQueues 链式追加，
  完成后队尾自查清理）→ Rust 的 per-path `tokio::sync::Mutex` 等价还原
  （16 并发写测试验证无撕裂）
- `js_encode_uri_component`：JS `encodeURIComponent` 保留集
  （`A-Za-z0-9-_.!~*'()`）逐字对齐，中文/特殊字符转大写十六进制百分号转义
- `parse_studio_task_snapshot`：字段级校验（version=1 / execution 必备 /
  status 枚举 / startedAt 数值 / logs 全 string），坏载荷 → None

## parity 要点（移植难点）

1. **快照解析的宽松语义**：TS 对 `requestedIntent` 只验 string（不验枚举
   值）——Rust 侧保持 String 字段而非强类型枚举，拒绝"顺手收紧"
2. **文件名编码**：sessionId 经 JS 组件编码（`会话/1` →
   `%E4%BC%9A%E8%AF%9D%2F1.json`），保留集与转义形态逐字节对齐
3. **写队列的清理语义差异**：TS 在队尾仍是自己时删除队列条目；Rust 保留
   per-path 锁条目（清理会破坏排队语义——新写入者拿新锁绕过队列；
   studio 会话数有限，常驻无泄漏风险，此处备案）
4. **写入格式**：`JSON.stringify(value, null, 2) + "\n"` → pretty + 尾随换行
5. **读/删前的队列等待**：TS `await writeQueues.get(path)` → 先取 per-path
   锁再操作，读写互斥

## 验证

- **lib 单测**：836 passed / 0 failed（+5 新单测：快照往返含 pretty+换行 /
  缺失静默 / 16 并发写串行化 / URI 组件编码矩阵 / 六种坏载荷拒收）
- **golden 差分**：75 域全绿（无新域——studio 包不在 core dump 面；
  task-store 由 Rust 单测对齐）
- **export-bindings**：995 passed
- **TS 全量**：185 文件 / 1798 测试全绿
- **clippy**：`cargo clippy --lib --tests` 零警告

## 下一步

- ⬜ **40 号**：chapter-analyzer.ts（634 行，依赖已全就绪：writer-parser /
  governed-working-set / memory-retrieval / outline-paths）+ runner 的
  `buildPersistenceOutput`（settler 输出 → 持久化产物装配）。
- ⬜ **41 号**（write-next 收官）：`_writeNextChapterLocked` 主体
  （prepareWriteInput[plan 持久化复用] → writeChapter → review-cycle[37 号] →
  promotion pass → buildPersistenceOutput[40 号] → truth-validation[38 号] →
  persistChapterArtifacts[37 号] → 通知/webhook）+ state-store book 层
  （loadBookConfig/getNextChapterNumber/章节索引）+ SSE 广播面 +
  `/api/v1/books/:id/write-next` 路由挂载（复用本号 task-store）。
