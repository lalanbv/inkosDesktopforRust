# 变更记录：M3a engine 资源化 + 解 C1 + paths/config cleanup

| 项 | 值 |
|---|---|
| 日期 | 2026-08-06 |
| 类型 | feat（M3 第 1 子里程碑） |
| 范围 | engine 模块 + paths/config 收口 + 解 C1 + 打包脚本 |
| 测试 | 159 通过（+8 新增），clippy --all-targets 0 警告，engine 端到端冒烟绿 |
| 设计 | `03_M3设计/M3_总体设计.md` §4 M3a；计划 `02_实现计划/M3a_*.md` |

## 解除的阻断

- **C1**（审计延 M3）：`main.rs` 不再用编译期 `CARGO_MANIFEST_DIR.parent()`（prod 用户机不存在→分发崩）。改用 `engine::resolve_engine_dir(resource_dir, CARGO_MANIFEST_DIR)`：prod 优先 `resource_dir/engine`（tauri.conf.json `bundle.resources=["engine"]`），dev 回退 `src-tauri/engine`。

## 主要变更

### 新增 `src-tauri/src/engine/` 模块
- `engine/mod.rs`：`resolve_engine_dir(resource_dir, dev_engine_root) -> PathBuf` 纯函数（prod/dev 双分支，3 单测）。
- `engine/manifest.rs`：`EngineManifest { engine_version, engine_sha256, node_min, built_at }` + `read/write`（原子 tempfile+persist）+ `sha256_file`（64KiB 流式，M3d updater 校验用）；5 单测（round-trip / 缺失 / 非法 JSON / SHA256 已知向量）。

### `paths.rs` 收口（审计 minor M5）
- 移除 `submodule_root`（M3a 后无 Rust 代码再用 inkos 仓库根；比重命名死代码更干净）。
- 新增 `launch_engine_dir`（启动源，resolve_engine_dir 注入），与 `engine_dir`（app_data/engine，M3d updater 运行态副本目标）解耦。
- 收口 `runtime_dir/projects_path/updates_staging_dir`（app_data 子目录，消除散落 join），为 M3b/M3c/M3d 铺路。
- `AppPaths::new(project_root, launch_engine_dir)`；`with_app_data` 测试构造。

### `config.rs` 集中化（审计 minor M5）
- `CLI_ENTRY_REL = "dist/index.js"`（engine 相对；原 `packages/cli/dist/index.js`）。
- 收口目录/文件名常量：`ENGINE_DIR_NAME/RUNTIME_DIR_NAME/NODE_DIR_NAME/LOGS_DIR_NAME/PROJECTS_FILE_NAME/SECRETS_DIR_NAME/SECRETS_FILE_NAME/UPDATES_DIR_NAME/STAGING_DIR_NAME/ENGINE_BAK_DIR_NAME/ENGINE_MANIFEST_FILE/APP_DATA_DIR_NAME`。
- 3 单测（端口 / CLI 入口 / 单段名校验）。

### `supervisor.rs` build_launch
- `cli_entry = paths.launch_engine_dir().join(CLI_ENTRY_REL)`；DummyPaths 同步精简。

### `main.rs`
- engine 经 `resolve_engine_dir` 解析（解 C1）；secrets 路径改用 `config::SECRETS_*_NAME`。
- 移除 C1 TODO（保留 C2 TODO 标 M3b）。

### 打包脚本 `scripts/desktop-package-engine.sh`
- 组装 `src-tauri/engine/`：`packages/cli/dist`→`engine/dist` + `packages/cli/package.json`→`engine/package.json` + `engine/node_modules`→`packages/cli/node_modules`（符号链接）。
- 镜像 packages/cli 结构，使 `node engine/dist/index.js` 与 `node packages/cli/dist/index.js` 解析等价（cliPackageRoot/ESM import/studio.ts 候选 #1 同时命中）。
- 条件化构建（dist 已在则跳过 pnpm install/build）；写 manifest.json。
- **端到端冒烟验证**：`curl /`→200（SPA）、`curl /api/v1/events`→200（SSE）、日志 "InkOS Studio running"。

### `tauri.conf.json` + `src-tauri/.gitignore`
- `bundle.resources = ["engine"]`（prod 随包分发）；`.gitignore` 加 `/engine`（构建产物）。

## 依赖

- 新增 `sha2 = "0.10"`（EngineManifest SHA256）。

## 兼容性 / 扩展性 / 安全

- **兼容**：dev/prod 同构 engine 布局；resolve_engine_dir 纯函数双分支。
- **扩展**：`PathResolver` trait 可 mock；`EngineManifest` 为 M3d updater 校验预留；config 收口为零硬编码铺路。
- **安全**：engine manifest 原子写（崩溃安全）；SHA256 流式校验预留。
- **零修改**：engine/ 只进 `src-tauri/`；脚本只读 `packages/`；根 `.gitignore`/`package.json` 未碰。

## 延 M3e（已识别，非 M3a 阻断）

- **prod 自包含 node_modules**：dev 用 `packages/cli/node_modules` 符号链接；prod 需真实副本（pnpm deploy inject 或 npm pack 装配），M3e 打包构建 CI 验证。
- **pnpm 版本**：inkos 锁 `pnpm ≥9`（`pnpm.overrides` 在 v10+ 弃用）；M3e CI 用 `pnpm/action-setup@v4 version:9`（与 inkos ci.yml 一致）。

## 下一步

M3b：项目选择窗口 + 解 C2（pre-sidecar 原生窗口 + projects.json 持久化）。
