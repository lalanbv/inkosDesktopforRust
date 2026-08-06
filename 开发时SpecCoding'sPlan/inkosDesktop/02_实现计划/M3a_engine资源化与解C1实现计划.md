# M3a 实现计划：engine 资源化 + 解 C1 + paths/config cleanup

> **执行方式**：Inline（本会话 TDD）。每 Task：RED→GREEN→REFACTOR→commit。
> 前置：M1+M2（HEAD `3431e6c0`）+ M3 总体设计 §4 M3a。

**Goal:** 打包后二进制能经 Tauri `resource_dir()` 找到自包含 inkos engine（不再依赖编译期 `CARGO_MANIFEST_DIR`），解 C1；并收口 paths/config，为 M3b-M3f 铺路。

**Architecture:** engine/ 为自包含 inkos 运行时（dist/{cli,core,studio} + 数据资产 + prod node_modules），dev 置 repo_root/engine（node 向上解析借用 repo node_modules），prod 作 Tauri resource 打包。`resolve_engine_dir` 纯函数优先 resource_dir/engine，回退 repo_root/engine。

**Tech Stack:** Rust（serde/anyhow/thiserror/sha2）、Tauri 2 resource API、bash 打包脚本。

## Global Constraints（M3 总体设计 §9 + 架构 v1.2）

- 零修改 inkos（`packages/`、根 `package.json`/`README*`/既有 `.gitignore`/`.github/` 不碰）
- 桌面壳产物只进 `src-tauri/`、`scripts/desktop-*`、新增 `.github/workflows/desktop-*.yml`
- Rust 模块：不可变、单一职责、文件 <400 行、无静默吞错、枚举 None=0/Max
- 测试覆盖 ≥80%、clippy --all-targets 0 警告
- 路径名禁空格、驼峰/下划线

## File Structure

| 路径 | 职责 |
|---|---|
| `src-tauri/src/config.rs` | 收口全部常量（端口/超时/相对路径/URL/版本） |
| `src-tauri/src/paths.rs` | PathResolver trait（rename + 新增 engine/runtime/projects/updates 方法）+ AppPaths |
| `src-tauri/src/engine/mod.rs` | engine 模块入口 + `resolve_engine_dir` 纯函数 |
| `src-tauri/src/engine/manifest.rs` | EngineManifest 结构 + read/write/validate |
| `src-tauri/src/supervisor.rs` | build_launch 用 engine_dir 解析 cli_entry |
| `src-tauri/src/main.rs` | resource_dir 解析 engine_dir（解 C1）+ repo_root 解析 |
| `scripts/desktop-package-engine.sh` | 组装 engine/（dist + 资产 + manifest；prod node_modules 经 pnpm deploy） |
| `src-tauri/tauri.conf.json` | bundle.resources 含 engine/ |
| `.gitignore`（src-tauri 内已存在？根 .gitignore 不碰） | 加 `/engine/`（构建产物） |

---

## Task 1: config 集中化（TDD inline）

**Files:** Modify `src-tauri/src/config.rs`

**Interfaces:**
- Produces: `DEFAULT_STUDIO_PORT`、`HEALTH_PROBE_*`、`CLI_ENTRY_REL="dist/cli/index.js"`（改）、新增 `ENGINE_DIR_NAME="engine"`、`RUNTIME_DIR_NAME="runtime"`、`NODE_DIR_NAME="node"`、`PROJECTS_FILE_NAME="projects.json"`、`UPDATES_STAGING_DIR_NAME="updates/staging"`、`ENGINE_MANIFEST_FILE="manifest.json"`、`APP_DATA_DIR_NAME="inkosDesktop"`、`SECRETS_DIR_NAME=".inkos"`、`SECRETS_FILE_NAME="secrets.json"`

- [ ] Step 1: 改 config.rs：CLI_ENTRY_REL 改为 `"dist/cli/index.js"`；新增上述 NAME 常量（收口散落字符串，零硬编码）。保留现有 2 测试 + 加 `cli_entry_is_engine_relative` 断言新值。

- [ ] Step 2: `cargo test config` → PASS。

## Task 2: paths — submodule_root→repo_root + 新增目录方法（TDD）

**Files:** Modify `src-tauri/src/paths.rs`、`src-tauri/src/supervisor.rs`（DummyPaths 测试 trait impl 改名）、`src-tauri/src/main.rs`（AppPaths::new 调用）

**Interfaces:**
- PathResolver trait：`project_root()`、`repo_root()`（rename from submodule_root）、`engine_dir()`、`runtime_dir()`、`log_dir()`、`projects_path()`、`updates_staging_dir()`
- AppPaths::new(project_root, repo_root) —— repo_root 用于 dev engine 回退；engine_dir/runtime_dir/projects_path/updates_staging 由 app_data 派生（engine_dir 默认 app_data/engine，**但实际 engine 由 resolve_engine_dir 在 main 决定**；PathResolver.engine_dir() 返回 app_data 下的默认位，仅 M3d updater 用）

> 设计澄清：PathResolver 的 `engine_dir()` 返回 **app_data/engine**（updater 替换目标，运行态 engine 副本）；而 `resolve_engine_dir(repo_root, resource_dir)` 返回**启动用 engine 源**（prod=resource/engine，dev=repo_root/engine）。两者解耦：启动用源 engine（只读），updater 维护 app_data/engine（可写替换）。M3a 先把启动流接到 resolve_engine_dir；app_data/engine 由 M3d updater 引入。

- [ ] Step 1: 改 paths.rs：trait 方法 rename + 新增（engine_dir/runtime_dir/projects_path/updates_staging_dir 全基于 app_data + config NAME 常量）。AppPaths 加 `repo_root` 字段（rename）。更新 2 个既有测试 + 加新测试断言 engine_dir/runtime_dir/projects_path 等于 app_data 对应子目录。

- [ ] Step 2: 改 supervisor.rs DummyPaths impl：`submodule_root`→`repo_root`（+ 加新 trait 方法 stub）。

- [ ] Step 3: `cargo test paths supervisor` → PASS。

## Task 3: engine 模块 — manifest + resolve_engine_dir（TDD）

**Files:** Create `src-tauri/src/engine/mod.rs`、`src-tauri/src/engine/manifest.rs`；Modify `src-tauri/src/lib.rs`（加 `pub mod engine;`）；Cargo.toml 加 `sha2 = "0.10"`、`serde_json`（已有）

**Interfaces:**
- `engine::manifest::EngineManifest { engine_version: String, engine_sha256: String, node_min: String, built_at: String }` + `read(path: &Path) -> Result<Self>`、`write(&self, path: &Path) -> Result<()>`（原子写 0600，复用 tempfile）、`validate_sha256(dir: &Path) -> Result<bool>`
- `engine::resolve_engine_dir(repo_root: &Path, resource_dir: Option<&Path>) -> PathBuf`（纯函数：resource_dir/engine 存在则用之，否则 repo_root/engine）

- [ ] Step 1: 写 manifest.rs 测试（RED）：EngineManifest serde round-trip（temp file）；read 缺失文件 Err；write 后权限 0600（Unix）；validate_sha256 对 temp dir 算哈希。

- [ ] Step 2: 写 resolve_engine_dir 测试（RED）：resource_dir/engine 存在→返回它；不存在→repo_root/engine；resource_dir=None→repo_root/engine。

- [ ] Step 3: 实现 manifest.rs（serde + atomic write + sha2）+ mod.rs（resolve_engine_dir 纯逻辑）。

- [ ] Step 4: `cargo test engine` → PASS。

- [ ] Step 5: 注册 `pub mod engine;` 到 lib.rs。

## Task 4: supervisor build_launch — engine_dir 解析 cli_entry

**Files:** Modify `src-tauri/src/supervisor.rs`

**Interfaces:** build_launch 改用 `paths.engine_dir().join(CLI_ENTRY_REL)`…… 

> 等等：Task 2 澄清里 engine_dir() = app_data/engine（updater 目标），但启动应用 resolve_engine_dir 的结果。build_launch 需要**启动 engine 路径**，不是 app_data/engine。故 build_launch 的 paths 须携带启动 engine 路径。

**修正设计**：PathResolver 加 `launch_engine_dir()`（= resolve_engine_dir 结果，由 main 注入 AppPaths）；build_launch 用 `paths.launch_engine_dir().join(CLI_ENTRY_REL)`。engine_dir()（app_data/engine）留 M3d。

- [ ] Step 1: PathResolver 加 `launch_engine_dir()`；AppPaths 持 `launch_engine_dir: PathBuf`（new 加参数 或 builder）。更新 DummyPaths。

- [ ] Step 2: build_launch 用 `paths.launch_engine_dir().join(CLI_ENTRY_REL)`；更新 3 个 build_launch 测试（DummyPaths 给 launch_engine_dir）。

- [ ] Step 3: `cargo test supervisor` → PASS。

## Task 5: desktop-package-engine.sh + .gitignore + dev 验证

**Files:** Create `scripts/desktop-package-engine.sh`；Modify 根 `.gitignore`？**禁止**——改在 `src-tauri/.gitignore`（已存 /target/）加 `../engine/`？不行（gitignore 不跨目录）。方案：engine/ 是仓库根级构建产物，根 `.gitignore` 是 inkos 既有文件（零修改）。改用 **`scripts/.gitignore`？** 也不行。**最终方案**：engine/ 不入 git——靠它本就是构建产物 + 在新文件 `scripts/desktop-package-engine.sh` 内生成；为防误提交，新建 **`.gitignore` 不动**，改在 **`src-tauri/.gitignore`** 不可达根 engine/。 → **正确解**：把 engine/ 生成到 `src-tauri/engine/`（而非仓库根），这样 `src-tauri/.gitignore` 加 `/engine/` 即忽略，且 Tauri resources 相对 src-tauri 解析更自然。

> 决策：engine/ 生成到 **`src-tauri/engine/`**（非仓库根）。dev 与 prod 同位。`src-tauri/.gitignore` 加 `/engine/`。

**Interfaces:** 脚本产出 `src-tauri/engine/{dist/{cli,core,studio}, 数据资产, manifest.json}`，dev 下 `node src-tauri/engine/dist/cli/index.js studio` 可跑（node 向上解析借 src-tauri/engine → … → repo node_modules）。

- [ ] Step 1: 写 `scripts/desktop-package-engine.sh`：
  - 仓库根 `pnpm install --frozen-lockfile && pnpm build`
  - `rm -rf src-tauri/engine && mkdir -p src-tauri/engine/dist`
  - `cp -R packages/cli/dist src-tauri/engine/dist/cli`（+ cli 的数据资产目录 genres/skills/prompts，实测 packages/cli 下有哪些就 cp 哪些）
  - `cp -R packages/core/dist src-tauri/engine/dist/core`
  - `cp -R packages/studio/dist src-tauri/engine/dist/studio`（含 SPA index.html + assets）
  - 写 `src-tauri/engine/manifest.json`（engine_version=`node -p "require('./packages/cli/package.json').version"`、engine_sha256 占位/算、node_min="22.0.0"、built_at）
  - 退出码 0 + 校验 `src-tauri/engine/dist/cli/index.js` 存在

- [ ] Step 2: `src-tauri/.gitignore` 加 `/engine/`。

- [ ] Step 3: 跑脚本 + dev 冒烟：`node src-tauri/engine/dist/cli/index.js studio --port 4567 &` → `curl -sI http://127.0.0.1:4567/ | head -1` 应 200；kill。

## Task 6: tauri.conf.json resources + main.rs resource_dir 解析（解 C1）

**Files:** Modify `src-tauri/tauri.conf.json`、`src-tauri/src/main.rs`

**Interfaces:**
- tauri.conf.json `bundle.resources = ["engine"]`（相对 src-tauri；打包含 engine/）
- main.rs：`let repo_root = CARGO_MANIFEST_DIR.parent()`；`let resource_dir = app.path().resource_dir().ok()`；`let launch_engine = resolve_engine_dir(repo_root, resource_dir.as_deref())`；构造 AppPaths 时注入 launch_engine_dir；移除 C1 TODO 注释。

- [ ] Step 1: tauri.conf.json 加 `bundle.resources`。注意 frontendDist 当前 `../src-tauri/stub`（M1）——保留；resources 相对 src-tauri 工作区。

- [ ] Step 2: main.rs setup 内：解析 repo_root（保留 CARGO_MANIFEST_DIR.parent()，**仅 dev 回退与诊断用**）+ resource_dir（`app.path().resource_dir()`）→ resolve_engine_dir → AppPaths::new(project_root, repo_root, launch_engine)。移除 2 处 TODO(M3) 之 C1 注释。

- [ ] Step 3: `cargo build` 通过（resource_dir 用 `tauri::Manager::path()`，app handle 已有）。

## Task 7: 全量测试 + clippy + commit + 变更记录

- [ ] `cargo test`（全绿）、`cargo clippy --all-targets -- -D warnings`（0 警告）
- [ ] 变更记录 `变更记录文档/20260806/M3a_engine资源化.md`
- [ ] commit：`feat(m3a): engine 资源化 + resource_dir 解 C1 + paths/config cleanup`

---

## Self-Review

- **Spec 覆盖**：M3 总体设计 §4 M3a 全覆盖（engine 结构、manifest、resource_dir、cleanup）✓
- **C1 解除**：main.rs 用 resource_dir（prod）+ repo_root 回退（dev）替代 CARGO_MANIFEST_DIR 烘焙 ✓
- **类型一致**：launch_engine_dir 跨 Task 2/4/6 一致；EngineManifest 字段跨 Task 3 一致 ✓
- **零修改**：engine/ 在 src-tauri/ 内（新目录），脚本只读 packages/，根 .gitignore 不碰 ✓
- **Task 4 设计修正**：launch_engine_dir（启动源）vs engine_dir（app_data，M3d updater 目标）解耦，已在 Task 2/4 注明 ✓
