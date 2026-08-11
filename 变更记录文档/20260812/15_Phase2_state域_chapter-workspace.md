# 15 — Phase 2 state 域：chapter-workspace + StateStore remove_file

> 日期：2026-08-12
> 范围：engine-rs（Rust 化迁移，Strangler-Fig 阶段）
> 关联：`14_Phase2_state域_runtime-state-store主链贯通.md`

## 移植内容

### 1. `state/store.rs`（StateStore 扩展）
trait 新增 [`remove_file`]（文件不存在 → no-op，对齐 TS `rm(path, {force:true})`）。
FsStateStore（tokio::fs，NotFound→Ok）+ InMemoryStateStore（HashMap::remove）双实现。

### 2. `state/chapter_workspace.rs`（新增）
移植 `chapter-workspace.ts`（163 行）——章节级 fs 编排：

- [`read_chapter_user_brief`] / [`save_chapter_user_brief`]（空串删文件）/ [`read_chapter_plan_document`]
- [`archive_chapter_version`]：归档版本，返回 [`ChapterVersion`] 元信息
- [`list_chapter_versions`]（按 createdAt 降序）/ [`read_chapter_version`]（非法 id → Constraint）
- [`ChapterVersionSource`] 枚举（manual/agent/revision/regeneration/restore）+ [`ChapterVersion`]
- `parse_version_id` 正则：`^(\d{13})_(source)_([0-9a-f-]{36})$`

## 关键技术点

- **时间戳/UUID 注入（纯内核范式）**：TS `archiveChapterVersion` 用 `new Date()` + `crypto.randomUUID()`；
  Rust 由调用方注入 `now_millis`（13 位毫秒）+ `created_at_iso`，内核不依赖时钟。UUID 部分用 `uuid` crate v4
  （与 TS `crypto.randomUUID()` 对齐）。这是 chapter_word_sync 范式的 async 继承。
- **millis_to_iso 手写公历算法**：TS `new Date(ms).toISOString()` 需毫秒→ISO8601。
  Rust 无 chrono 依赖，用 Howard Hinnant 的 `civil_from_days` 算法（Unix day count → 年月日）+
  手写时分秒/毫秒格式化。3 个测试验证（含已知日期 2023-01-01、毫秒精度）。
- **character_count UTF-16 对齐**：TS `.length` 按 UTF-16 码元（emoji 计 2）；
  Rust 用 `content.encode_utf16().count()`（与项目既有经验一致，见 memory `rust-migration-pivot`）。
- **version id 正则严格校验**：13 位时间戳 + 5 个固定 source + 标准 UUID（36 字符 hex+dash）。
  非法 id 在 `read_chapter_version` 直接 Constraint 错误（对齐 TS throw）。
- **saveChapterUserBrief 的空串语义**：trim 后空 → `remove_file`（不是写空文件），与 TS `rm(force:true)` 一致。

## 设计决策

| 决策点 | 选择 | 理由 |
|--------|------|------|
| 时间来源 | 调用方注入 now_millis + created_at_iso | 纯内核不依赖时钟；可测试（注入固定值）；与 chapter_word_sync 范式一致 |
| UUID | uuid crate v4 | 与 TS crypto.randomUUID 同语义；crate 已成熟，避免手写随机 |
| ISO 格式化 | 手写 civil_from_days | 避免引入 chrono 重依赖；算法紧凑且有测试守护；日期范围 1970+ 足够 |
| chapter-delete | 本轮不移植 | 依赖 `rollbackToChapter`（review-reject 流程，Phase 3 业务运行时），独立移植会留空壳 |

## 验证

| 维度 | 结果 |
|------|------|
| 新增单测 | **10 项**（version id 解析/拒绝、ISO 日期、user brief 往返、plan、archive、list 排序、非法 id、章节号校验）全绿 |
| 全量 lib 测试 | **419 passed**（上轮 409 + 本次 10） |
| golden 差分测试 | **22 passed** |
| clippy 双模式 | 零警告 |
| 新增依赖 | uuid 1（v4 feature） |

## state 域进度

- ✅ I/O 编排主链全量贯通（14 号：state_bootstrap + runtime_state_store）
- ✅ **chapter_workspace**（本次，章节级 fs 编排）
- ⬜ chapter-delete（116 行）—— 依赖 review-reject 的 `rollbackToChapter`（Phase 3）
- ⬜ manager（821 行）—— 大编排（Phase 3 高层，依赖 LLM client）

state 域**可独立移植的部分已全部完成**。剩余 chapter-delete/manager 依赖业务运行时（Phase 3），
下一阶段转向 Phase 3 业务运行时（agents / pipeline）或 134 HTTP 端点接线。
