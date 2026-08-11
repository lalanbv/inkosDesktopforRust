# 10 — Phase 2 state 域：StateStore fs trait 基座 + durable story progress

> 日期：2026-08-12
> 范围：engine-rs（Rust 化迁移，Strangler-Fig 阶段）
> 关联：`09_Phase2_state域_state-bootstrap纯逻辑子集.md`
> 性质：async 编排层基础设施 + 首个端到端编排函数

## 移植内容

### 1. `state/store.rs`（新增，fs 抽象基座）
定义 [`StateStore`] async trait + 双实现，为 state-bootstrap/runtime-state-store/manager 等
async 编排的共同 fs 基座：

- **trait 方法**（对齐 `node:fs/promises` 子集）：`read_to_string` / `write_string` / `mkdir_p` /
  `list_dir` / `exists`；文件/目录不存在 → `None` / 空 `Vec`（正常业务状态，非错误）
- [`FsStateStore`]：生产实现，`tokio::fs`（NotFound → None/空，其它 → `EngineError::Io`）
- [`InMemoryStateStore`]：测试 mock，`Mutex<HashMap<path, content>>`；`list_dir` 按路径前缀匹配直接子项
- [`join_path`]：路径拼接辅助（对齐 TS `join`）

### 2. `state/state_bootstrap.rs`（async 编排扩展）
移植 `resolveDurableStoryProgress` + `loadDurableArtifactChapterNumbers`（TS 527-547）：

- [`resolve_durable_story_progress`]：`max(连续章节产物前缀, 显式 fallback)`——
  「只信任 durable 产物进度」是关键设计（current_state.chapter 来自 markdown，可能含幻觉数字如 1988→第1988章）
- `load_durable_artifact_chapter_numbers`：合并 `chapters/index.json`（数组 number 字段）+
  `chapters/` 文件名（`N_*` 前缀），两源任一失败 → 空
- 消费 [`StateStore`] trait + 已移植的 [`resolve_contiguous_chapter_prefix`](09 号) + [`normalize_explicit_chapter`]

## 关键技术点

- **「纯内核 + I/O 注入」范式扩展到 async**：编排函数接收 `&dyn StateStore`，单测用 [`InMemoryStateStore`]，
  集成测试用 [`FsStateStore`] + tempfile。与 chapter_word_sync 的纯函数注入一脉相承，现在是 async 版。
- **NotFound 语义统一**：TS `node:fs` 的 try/catch 把 NotFound 当正常；Rust 用 `ErrorKind::NotFound`
  分支显式转 `None`/空，其它错误向上传播为 `EngineError::Io`。
- **InMemoryStateStore 的目录模拟**：无真实目录概念，`list_dir` 按路径前缀 +「下一个 `/` 之前」
  提取直接子项（含子目录名）；`exists` 检查文件键或目录前缀。
- **durable progress 的反幻觉设计**：`max(artifact, fallback)` 确保不被 markdown 幻觉数字污染——
  artifact 来自文件名（`N_*.md`，durable）+ index.json（结构化），都是「真实产物」。
- **index.json 宽松解析**：`Vec<serde_json::Value>` + `get("number")?.as_i64()`，非正整数/非数字自动过滤
  （与 TS 的 `typeof === "number" && isInteger && > 0` 等价）。

## 设计决策

| 决策点 | 选择 | 理由 |
|--------|------|------|
| trait vs 具体类型 | `async_trait` StateStore | 编排逻辑可注入测试；trait 是后续所有 async 编排的共同基座 |
| 两个实现 | Fs + InMemory | Fs 生产、InMemory 单测；tempfile 集成测试验证 Fs 路径正确 |
| trait 方法粒度 | 5 方法（read/write/mkdir/list/exists） | 覆盖 state-bootstrap 全部 fs 操作；避免后续频繁扩展 |
| StateStore 首个消费者 | resolve_durable_story_progress | 相对独立（无 JSON schema 依赖），适合验证 trait 设计 |

## 验证

| 维度 | 结果 |
|------|------|
| 新增单测 | **16 项**（store 9 + durable progress 7）全绿 |
| 集成测试 | FsStateStore + tempfile 3 项（mkdir/write/read、list_dir、read None） |
| 全量 lib 测试 | **378 passed**（上轮 362 + 本次 16） |
| golden 差分测试 | **22 passed**（未触及） |
| clippy 双模式 | 零警告 |
| 新增依赖 | async-trait 0.1（dep）+ tempfile 3（dev-dep） |

## state 域 I/O 编排层进度

async 编排层的基座已就位：
- ✅ **StateStore trait + 双实现**（本次，基础设施）
- ✅ **resolve_durable_story_progress**（本次，首个消费者）
- ✅ 纯逻辑 normalization（09 号）
- ⬜ `bootstrapStructuredStateFromMarkdown`（主入口，4 文件编排 + JSON schema 校验）
- ⬜ `loadOrBootstrap*` 系列（manifest/current_state/hooks/summaries 的读或引导）
- ⬜ `resolveRuntimeLanguage` + `repairHooksStateInput`（JSON unknown 修复需重新设计）

下一目标：移植 `bootstrapStructuredStateFromMarkdown` 主编排（消费 StateStore + story_markdown 解析 + 纯逻辑），
完成 state-bootstrap，解锁 runtime-state-store。
