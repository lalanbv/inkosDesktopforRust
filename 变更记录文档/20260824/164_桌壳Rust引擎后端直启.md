# 164 · 桌壳绞杀者终切：Rust 引擎后端直启（inkos-engine-server 取代 Node sidecar 默认位）

## 一、背景与目标

承接 145 号（0.1.0 发布）：Rust 全量引擎（engine-rs，107 端点契约对齐、strangler
duel 8/8）当时只发了**独立包**（inkos-engine-0.1.0 tarball），桌壳 .app 内仍是
Node sidecar（engine/dist + portable Node bootstrap）——绞杀者路线（迁移规划 v1
§4）的最后一步「桌壳切换到 Rust 引擎」尚未落位。本轮目标：**src-tauri 增加 Rust
引擎后端并设为默认**，Node sidecar 降级为回退路径，桌面端从此可零 Node 运行。

三个层面的现状对齐（本轮勘测结论）：

| 线 | 现状（勘测证实） |
| --- | --- |
| 纯 Rust 逻辑（engine-rs） | 16 域全量移植完成，`/api/v1/*` 107 端点 + SSE + 静态面（124 号）+ CORS（125 号）；secrets 读 `{root}/.inkos/secrets.json` 与 Node 同源 |
| Rust 客户端（inkos-engine-server） | bin 契约完备：`INKOS_PORT`/`INKOS_PROJECT_ROOT`/`INKOS_STATIC_DIR`；`/api/v1/health` 恒 200（62 号超集端点）；145 号已独立打包 |
| Rust 桌面客户端（src-tauri） | supervisor 固定 `node engine/dist/index.js studio`；本 round 之前无 Rust 后端路径 |

## 二、实现（src-tauri 侧 9 文件改 + 2 新增 + 1 脚本）

### 1. `engine/rustbin.rs`（新模块）

- `resolve_server_bin(app_data, resource_dir, dev_repo_root) -> Option<PathBuf>`：
  app_data/engine-rust（未来 updater 落点）→ resource_dir/engine-rust（prod 打包）
  → dev `engine-rs/target/{release,debug}`（debug_assertions 分支）。全 miss →
  None，调用方回退 Node 并告警——**启动不阻断**。
- `resolve_static_dir`：resource_dir/engine-rust/static → dev
  `packages/studio/dist`。缺失 = 纯 API 模式（webview 导航 404，告警提示）。
- `build_launch`：`LaunchSpec { program=bin, args=[], env={INKOS_PORT,
  INKOS_PROJECT_ROOT, INKOS_STATIC_DIR?}, cwd=project_root }`。不注入
  `INKOS_LLM_*`——引擎 `effective_router` 热解析 inkos.json+secrets 优先，启动
  env 仅回退端点，桌面场景无需预设。
- `HEALTH_PROBE_PATH = "/api/v1/health"`：恒 200（`/` 仅静态面存在时 200）。

### 2. 后端选择开关（分层配置系统）

- `EngineConfig.backend: EngineBackend`（serde snake_case 裸字符串 `rust`/`node`，
  默认 `rust`）。旧配置无该键 → serde default → rust，**零迁移**。与 `ConfigLayer`
  同理不套 None/Max 占位约定（外部契约值判别器，污染即非法配置面）。
- 选择序：env `INKOS_ENGINE_BACKEND`（rust|node，大小写不敏感，测试/临时切换）
  > 合并配置（ConfigManager.merged()，分层 merge）> 默认 rust。
- 未动 settings.html 表单（TOML 可改；UI 开关留后续，避免 IPC 契约面扩散）。

### 3. supervisor / main 装配

- `health_probe(port, timeout, probe_path)`：新增探测路径参数——Node=`/`（SPA
  恒挂）、Rust=`/api/v1/health`（不依赖静态面）。三处调用点同步。
- `spawn_sidecar_task` 分支：rust 命中 → **跳过 Node bootstrap 全流程**（无首启
  下载、无 PATH 回退）+ rustbin 规格；miss 或显式 node → 原 Node 路径不变。
- secrets 同步、loopback guard、SSE observer、托盘/关窗保活、updater 命令全部
  不变（后端无关面：磁盘格式同源、SSE 端点同名、端口策略同一函数）。

### 4. 诊断回显（生效后端）

- `lifecycle::EngineBackendState`（managed state）：`spawn_sidecar_task` 选定
  **生效**后端（含 miss 回退结果）后写入——与配置意图区分（回退时配置是 rust、
  生效是 node，诊断显示运行态事实）。
- `DiagnosticInfo.engine_backend`（`rust`/`node`/`unknown`——state 未托管 =
  启动前）：诊断命令返回，序列化契约测试覆盖。

### 5. 打包面

- `scripts/desktop-package-rust-engine.sh`（新）：组装 `src-tauri/engine-rust/`
  （release bin + static/ 前端副本 + manifest.json——键与 Node bundle/Rust 全量包
  同约定）。内建 duel 桩闸门（index.html ≤64B 拒绝，防 strangler_duel 的 34 字节
  桩混入发布资源）。幂等（rm -rf 全量重建）。
- `tauri.conf.json` bundle.resources += `"engine-rust"`（tauri build 前必须先跑
  脚本组装，与 engine/ 同约束）。`src-tauri/.gitignore` += `/engine-rust`。

## 三、验证（全部真跑）

| 面 | 结果 |
| --- | --- |
| 单元（rustbin 6 + config 3 新增 + 常量） | 全过（解析优先级/spec env 契约/无静态面不注入空 env/旧配置零迁移/TOML 形态） |
| 集成 `rust_backend_launch`（新） | **2/2**：真实拉起 server——`INKOS_PORT` 契约消费、health JSON `{"ok":true,"version":"0.1.0"}`、纯 API 模式 `/` 404（探测路径依据）、静态面模式 `/`=SPA index、kill_tree 端口释放。二进制/dist 缺失自动 SKIP |
| src-tauri 全量 cargo test | **567 通过 / 0 失败**（终轮含诊断回显；e2e_secrets 等 env 门控项按设计 ignored） |
| cargo clippy `--lib --tests --bins -D warnings` | 零警告 |
| 桌壳端到端冒烟（debug 二进制） | auto 复用项目 → 日志 `引擎后端 = rust（inkos-engine-server: .../target/debug/engine-rust/...）` → **零 node bootstrap 行** → health 407ms 成功 → SIGTERM 整树清理、无孤儿进程 |
| 组装脚本实跑 | engine-rust/ 产出（bin 27.8MB + static + manifest：engine_version=0.1.0 / rust_target=aarch64-apple-darwin / sha256） |

集成测试首跑踩中一处已知缝隙并修复：两测试并行 + `pick_free_port` TOCTOU 同瞬
取同端口（4567），后绑者退出、先绑者以纯 API 形态应答 → SPA 断言失败。修复 =
第二测试端口基座位移到 5600（探测区间不相交），非改动 pick_free_port 契约
（其文档已明示调用方容忍策略）。

## 四、行为变化与兼容性

- **默认后端 = rust**：现网 0.2.0 安装升级后（若打包了 engine-rust）自动切 Rust
  引擎；未打包 engine-rust 的构建 → resolve miss → 告警回退 Node，行为与现状
  完全一致（零回归路径）。
- 回退三通道：配置 `[engine] backend = "node"`、env `INKOS_ENGINE_BACKEND=node`、
  二进制 miss 自动回退（日志可见）。
- dev 工作流：`cd engine-rs && cargo build --release` + studio dist 已构建 →
  `cargo run` 即 Rust 后端；两者皆缺 → 回退 Node（原工作流不变）。

## 五、遗留与后续（不阻断，备案）

1. **engine updater 通道仍为 Node bundle 语义**：`cmd_check_updates` 的 engine
   版本读 Node engine manifest、apply 替换 app_data/engine。Rust 引擎版本经
   `/api/v1/health` 可见、生效后端经诊断命令回显。Rust 引擎 updater 通道
   （tarball 内层目录解包适配 + 版本对齐）独立立项——145 号先例：发布/updater
   属外向节奏，不与切换混轮。
2. settings.html 前端开关（backend 字段表单化）。
3. prod .app 瘦身决策：双引擎资源（engine + engine-rust）并存 vs 只留 rust——
   随 v0.3.0 发布决策（回退期建议双留）。

## 六、关联提交

- 本轮：feat(desktop): 桌壳绞杀者终切——Rust 引擎后端默认直启（164 号）
