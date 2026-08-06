# Phase 4：项目管理系统实现

> 日期：2026-08-06
> 提交：`9c82c574`（M6 整体）、`527211e7`（M6d）、`7b5d9526`（M6e）等
> 设计文档：`开发时SpecCoding'sPlan/inkosDesktop/01_设计文档/Phase4_项目管理系统设计.md`

## 目标

为 inkosDesktop 构建商业级项目管理：项目索引、类型识别、文件系统扫描、健康检查、生命周期管理，并接入已发布的 picker 前端。

## 交付物（src-tauri/src/project/）

| 模块 | 职责 | 单元测试 |
|---|---|---|
| `types.rs` | ProjectMeta / ProjectHealth / Capability 核心类型 | 7 |
| `index.rs` | SQLite 持久化（CRUD + 查询，细粒度锁，外键级联） | 13 |
| `detector.rs` | 7 类项目识别（特征文件 → 类型 + 名称） | 15 |
| `scanner.rs` | walkdir 遍历，排除 node_modules/target/隐藏目录 | 12 |
| `health.rs` | 依赖/配置/环境/权限/磁盘检查 | 9 |
| `manager.rs` | 生命周期管理 + 写穿缓存 + 双引擎分派 | 17 |
| `bridge.rs` | projects.json ↔ 索引合并（picker 展示增强） | 9 |
| `commands.rs` | 13 个 Tauri 命令 + AppState | 2 |

集成测试：`tests/project_commands.rs`（12 用例，真实 SQLite，覆盖排序/缓存/级联）

## 关键技术决策

### 1. SQLite 索引（rusqlite 0.32 + bundled）
- `ProjectIndex` 内部用 `Mutex<Connection>` 收敛锁粒度到单语句，避免「MutexGuard 跨 await」导致的运行时阻塞
- 开启 `PRAGMA foreign_keys = ON`，project_health 的 `ON DELETE CASCADE` 才生效
- 预编译索引加速 path/workspace/favorite 查询

### 2. 「最近打开」排序键（修真实 bug）
`last_opened_at` 是秒级，同一秒内连续打开两个项目会并列，`list_recent` 顺序不确定；NTP 回拨还会错乱。
新增库内单调自增 `open_seq`，`mark_opened` 在单条 UPDATE 里原子分配（`MAX(open_seq)+1`），排序一律用 `open_seq`。`last_opened_at` 仅用于展示。
回归测试 `recent_order_is_exact_within_the_same_second` 钉住该场景。

### 3. projects.json ↔ 索引合并（bridge.rs）
应用实际发布的前端是 `picker/index.html`，数据源是 `projects.json`（path+name）。M6 索引是独立的第二份存储（类型/收藏/标签/健康）。
`enrich_recents()` 把两者合并为 `EnrichedRecent`：索引命中则附类型徽章+收藏+ID，未命中退化为增强前展示——既有用户零迁移。纯函数，索引查询以闭包注入，无需真实数据库即可单测。

### 4. cmd_choose_project 同步索引
选项目时 `ensure_indexed()` 把路径写入索引（幂等），否则索引在真实使用中永远为空。失败只记日志，不阻断启动。

### 5. 前端集成（picker/index.html）
picker 的最近列表现显示类型徽章 + 收藏星标；收藏切换走 `toggle_favorite` 命令，失败回滚图标。纯 DOM 构建（textContent，无 innerHTML 注入）。

## 顺带修复的既有缺陷

- **keyring v3 未开平台后端**：`keyring = "3"` 缺 feature 时静默退化为进程内 mock，密钥重启即丢。显式开启 `apple-native`/`windows-native`/`sync-secret-service`/`crypto-rust`。
- **config watcher 路径未规范化**：macOS 下 notify 上报 `/private/var/...`，监听表存 `/var/...` 符号链接路径，事件永不匹配（4 个测试失败）。两侧统一规范化，并修掉一处 `watched_paths` 持锁期间获取 `debounce_map` 的锁序隐患。
- **crash dump 文件名精度到秒**：同秒两次 panic 互相覆盖写出非法 JSON。改为纳秒精度，抽出纯函数 `write_crash_dump` 便于测试。
- **logging 全局 subscriber 测试**：`.init()` 进程只能设一次，两个测试调用第二个必 panic。拆出 `build_subscriber`，测试用 `tracing::subscriber::with_default` 线程局部 subscriber。
- **config/reload.rs API 不匹配**：调用 `validate_config`/`set_workspace_config`/`get_merged_config`，但实际 API 是 `validate()`/`set_workspace()`/`merged()`。恢复并对齐。
- **LoggingConfig 幻影字段**：测试引用 `max_file_size_mb`/`max_backups`，规范定义只有 `level`+`retention_days`。清理测试。
- **配置合并覆盖语义**：`merge_into` 无条件复制每个标量，导致上层只设某字段时把下层已设值打回默认。改为「字段 != 默认值」判定「该层显式设置」。

## 验证

- `cargo test --lib`：311 passed / 0 failed
- `cargo test --test project_commands`：12 passed（真实 SQLite + 并发场景）
- `cargo clippy --all-targets -- -D warnings`：零告警

## 性能/0GC

- SQLite 连接锁粒度到单语句，避免外层粗锁
- 写穿缓存（HashMap<id, ProjectMeta>）减少 DB 查询
- `enrich_recents` 零分配合并（闭包注入，无中间 Vec）
- `ProjectType::parse_str` 返回 `Option`，无 Result 分配
