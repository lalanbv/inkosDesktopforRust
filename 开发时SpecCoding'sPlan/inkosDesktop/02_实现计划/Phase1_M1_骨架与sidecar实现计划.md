# inkosDesktop Phase 1 M1 实现计划：骨架、sidecar 与 supervisor

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 交付首个可用的 inkosDesktop 桌面 app——Tauri 窗口启动 → supervisor 拉起 inkos sidecar → WebView 加载 `http://127.0.0.1:<port>` 显示原版 Studio UI → loopback 加固已生效。

**Architecture:** γ 旁路增强派（见架构设计 v1.1 §3）。Tauri 2.x（Rust 壳）作 sidecar supervisor；inkos 作为零修改 git submodule，M1 阶段**就地运行已构建的 submodule**（系统 Node 22 + `packages/cli/dist/index.js`），发布打包（便携 Node + engine/）推迟到 M3。loopback 用 OS 防火墙把 sidecar 端口锁回 127.0.0.1。

**Tech Stack:** Rust + Tauri 2.x、tokio（异步）、reqwest（健康探测）、anyhow/thiserror（错误）、tempfile（测试）、inkos v1.6.3（submodule，Node 22 + pnpm）。

## Global Constraints（源自架构 v1.1，逐项逐字）

- **零修改不变量**：永不修改 `packages/` 下 inkos 源码（mono-repo：本仓即 inkos fork）
- **仓库模型（mono-repo）**：桌面壳直接建在本 fork 仓（inkos 在仓库根 `packages/`）。壳产物只进 `src-tauri/`、`scripts/desktop-*`、新增 `.github/workflows/desktop-*.yml`；**永不改** inkos 根 `package.json`/`README*`/既有 `.gitignore`/既有 `.github/`——上游同步用 `git merge upstream/master`，靠"零文件交叉"实现近零冲突
- **Rust 模块**：不可变优先（返回新对象）、单一职责、文件 <400 行、无静默吞错
- **端口**：默认 `4567`，由 `INKOS_STUDIO_PORT` 控制，被占则递增选空闲端口
- **数据目录**：经 `INKOS_PROJECT_ROOT` env 指向用户项目目录（非 `HOME`/`INKOS_HOME`）
- **绑定加固**：inkos 默认绑 `0.0.0.0`，必须由 OS 防火墙/沙箱锁回 `127.0.0.1`（见架构 §8）
- **SPA baseURL**：同源相对 `/api/v1`，换端口自动跟随，**禁止任何重写**
- **Node**：≥22.0.0（启用 `node:sqlite`；低版本 inkos 自动降级 Markdown，不阻塞）
- **测试覆盖**：≥80% 行覆盖
- **协议**：AGPL-3.0（主仓库）
- **命名**：路径/文件名禁空格，驼峰或下划线

### 环境前置（执行者本机）

- Rust toolchain（`rustup`，stable）+ `cargo`
- Node 22+、pnpm 9+
- macOS：Xcode Command Line Tools；Windows：WebView2 + MSVC build tools；Linux：`webkit2gtk` 等Tauri系统依赖
- `gh` CLI（仅 fork 步骤需要，M1 可先用 upstream 跳过）

---

## File Structure

| 路径 | 职责 |
|---|---|
| `src-tauri/Cargo.toml` | Rust 依赖（tauri/tokio/reqwest/anyhow/thiserror/serde/tempfile dirs） |
| `src-tauri/tauri.conf.json` | Tauri 应用配置（窗口、productName、bundle） |
| `src-tauri/src/main.rs` | Tauri 入口，`tauri::Builder` + setup hook 拉起 sidecar |
| `src-tauri/src/config.rs` | 常量（端口/超时/CLI 入口相对路径） |
| `src-tauri/src/paths.rs` | `PathResolver` trait + `AppPaths`（项目根/引擎/日志目录） |
| `src-tauri/src/supervisor.rs` | `LaunchSpec`、`pick_free_port`、`build_launch`、`spawn`、`health_probe` |
| `src-tauri/src/isolation.rs` | `LoopbackGuard` trait + 平台派发 |
| `src-tauri/src/isolation/macos.rs` / `linux.rs` / `windows.rs` | 平台防火墙实现 |
| `src-tauri/tests/supervisor_integration.rs` | 拉起真实 inkos 的集成测（`#[ignore]`，需本机构建） |
| `scripts/desktop-build-inkos.sh` | 仓库根执行 `pnpm install && pnpm build`（mono-repo，无 submodule） |
| `packages/`（inkos 工作区，已在仓库根） | inkos fork 本体，零修改；上游经 `git merge upstream/master` 同步 |
| `.gitignore` | 排除 `inkos/node_modules`、`inkos/packages/*/dist`、`target` |

---

## Task 1: 仓库骨架 + 常量模块

**Files:**
- Create: `src-tauri/Cargo.toml`、`src-tauri/src/main.rs`、`src-tauri/src/config.rs`、`src-tauri/src/lib.rs`
- Create: `.gitignore`、`README.md`
- Test: `src-tauri/src/config.rs`（内联单测）

**Interfaces:**
- Produces: `config::DEFAULT_STUDIO_PORT: u16`、`config::HEALTH_PROBE_TIMEOUT: std::time::Duration`、`config::CLI_ENTRY_REL: &str`（相对 submodule 根的 CLI 入口路径 `packages/cli/dist/index.js`）

- [ ] **Step 1: 建特性分支 + Tauri 骨架目录（mono-repo：本仓即 inkos fork，无需 git init/submodule）**

```bash
cd /Users/lalanbv/GitProject/inkosDesktopforRust
git checkout -b feat/desktop-m1
# 桌面壳产物只放 src-tauri/ 及新增文件，永不改动 inkos 根文件
mkdir -p src-tauri/src
```

> **零修改纪律**：本仓 = 你的 inkos fork（`lalanbv/inkosDesktopforRust`）。桌面壳的所有产物只进 `src-tauri/`、`scripts/desktop-*`、新增 `.github/workflows/desktop-*.yml`。**永不修改** `packages/`、根 `package.json`、`README*`、既有 `.gitignore`、既有 `.github/`——这样 `git merge upstream/master` 近零冲突，"完美同步"以 merge 方式兑现。

- [ ] **Step 2: 写 `Cargo.toml`**

```toml
[package]
name = "inkos-desktop"
version = "0.1.0"
edition = "2021"

[build-dependencies]
tauri-build = { version = "2", features = [] }

[dependencies]
tauri = { version = "2", features = [] }
tokio = { version = "1", features = ["full"] }
reqwest = { version = "0.12", features = ["blocking"] }
anyhow = "1"
thiserror = "1"
serde = { version = "1", features = ["derive"] }
dirs = "5"

[dev-dependencies]
tempfile = "3"
```

- [ ] **Step 3: 写失败的常量测试 `src-tauri/src/config.rs`**

```rust
pub const DEFAULT_STUDIO_PORT: u16 = 4567;
pub const HEALTH_PROBE_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(30);
pub const HEALTH_PROBE_INTERVAL: std::time::Duration = std::time::Duration::from_millis(200);
pub const CLI_ENTRY_REL: &str = "packages/cli/dist/index.js";

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn default_port_matches_inkos() {
        assert_eq!(DEFAULT_STUDIO_PORT, 4567);
    }
    #[test]
    fn cli_entry_is_studio_dist() {
        assert_eq!(CLI_ENTRY_REL, "packages/cli/dist/index.js");
    }
}
```

- [ ] **Step 4: 跑测试验证通过**

Run: `cd src-tauri && cargo test config -- --nocapture`
Expected: PASS（2 tests）

- [ ] **Step 5: 写最小 `main.rs` + `lib.rs` 占位**

```rust
// src-tauri/src/lib.rs
pub mod config;
```

```rust
// src-tauri/src/main.rs
fn main() { println!("inkos-desktop scaffold"); }
```

- [ ] **Step 6: Commit**

```bash
git add src-tauri .gitignore README.md
git commit -m "feat(scaffold): tauri rust shell + config constants"
```

---

## Task 2: paths 模块（跨平台目录解析）

**Files:**
- Create: `src-tauri/src/paths.rs`
- Modify: `src-tauri/src/lib.rs`（加 `pub mod paths;`）
- Test: `src-tauri/src/paths.rs`（内联单测）

**Interfaces:**
- Consumes: 无
- Produces: `trait PathResolver { fn project_root(&self) -> &Path; fn submodule_root(&self) -> &Path; fn log_dir(&self) -> PathBuf; }`、`struct AppPaths::new(project_root: PathBuf, submodule_root: PathBuf) -> anyhow::Result<Self>`

- [ ] **Step 1: 写失败的测试**

```rust
// src-tauri/src/paths.rs
use std::path::{Path, PathBuf};

pub trait PathResolver {
    fn project_root(&self) -> &Path;
    fn submodule_root(&self) -> &Path;
    fn log_dir(&self) -> PathBuf;
}

pub struct AppPaths {
    project_root: PathBuf,
    submodule_root: PathBuf,
    app_data: PathBuf,
}

impl AppPaths {
    pub fn new(project_root: PathBuf, submodule_root: PathBuf) -> anyhow::Result<Self> {
        let app_data = dirs::data_dir()
            .ok_or_else(|| anyhow::anyhow!("无法解析平台 data 目录"))?
            .join("inkosDesktop");
        Ok(Self { project_root, submodule_root, app_data })
    }
}

impl PathResolver for AppPaths {
    fn project_root(&self) -> &Path { &self.project_root }
    fn submodule_root(&self) -> &Path { &self.submodule_root }
    fn log_dir(&self) -> PathBuf { self.app_data.join("logs") }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn resolves_project_and_submodule_roots() {
        let p = AppPaths::new(PathBuf::from("/tmp/proj"), PathBuf::from("/tmp/inkos")).unwrap();
        assert_eq!(p.project_root(), Path::new("/tmp/proj"));
        assert_eq!(p.submodule_root(), Path::new("/tmp/inkos"));
    }
    #[test]
    fn log_dir_under_app_data() {
        let p = AppPaths::new(PathBuf::from("/tmp/proj"), PathBuf::from("/tmp/inkos")).unwrap();
        assert!(p.log_dir().ends_with("inkosDesktop/logs"));
    }
}
```

- [ ] **Step 2: 跑测试验证通过**

Run: `cd src-tauri && cargo test paths`
Expected: PASS（2 tests）

- [ ] **Step 3: 注册模块**

```rust
// src-tauri/src/lib.rs
pub mod config;
pub mod paths;
```

- [ ] **Step 4: Commit**

```bash
git add src-tauri/src/paths.rs src-tauri/src/lib.rs
git commit -m "feat(paths): cross-platform path resolver"
```

---

## Task 3: 构建 inkos（已在仓库根，无 submodule）

**Files:**
- Create: `scripts/desktop-build-inkos.sh`
- Create: `src-tauri/.gitignore`（Rust 忽略项，**不改** inkos 根 .gitignore）

**Interfaces:**
- Produces: 已构建的 `packages/cli/dist/index.js`、`packages/studio/dist/index.html`（SPA）、根 `node_modules`（inkos 工作区本体，已在仓库根）

> mono-repo：inkos 工作区即仓库根本身，无需 submodule。构建在仓库根执行。inkos 自带 `.gitignore` 已忽略 `node_modules`、`packages/*/dist`，无需重复。

- [ ] **Step 1: 写构建脚本 `scripts/desktop-build-inkos.sh`**

```bash
#!/usr/bin/env bash
# 在 inkos fork 仓库根构建（mono-repo）。零修改 inkos 源码。
set -euo pipefail
ROOT="$(git rev-parse --show-toplevel)"
cd "$ROOT"
echo "Installing deps ..."
pnpm install --frozen-lockfile
echo "Building inkos (core + cli + studio) ..."
pnpm build
test -f packages/cli/dist/index.js     || { echo "cli dist missing"; exit 1; }
test -f packages/studio/dist/index.html || { echo "studio SPA dist missing"; exit 1; }
echo "OK: inkos built in place (repo root)."
```

```bash
chmod +x scripts/desktop-build-inkos.sh
```

- [ ] **Step 2: 运行构建并验证产物**

Run: `./scripts/desktop-build-inkos.sh`
Expected: 退出码 0；`packages/cli/dist/index.js` 与 `packages/studio/dist/index.html` 存在

- [ ] **Step 3: 建 `src-tauri/.gitignore`（Rust 忽略项，独立文件，不碰 inkos 根 .gitignore）**

```gitignore
/target/
```

- [ ] **Step 4: 手动冒烟——验证 sidecar 可直接跑（inkos 在根）**

Run:
```bash
INKOS_STUDIO_PORT=4567 INKOS_PROJECT_ROOT=/tmp/inkos-demo node packages/cli/dist/index.js studio &
sleep 3 && curl -sI http://127.0.0.1:4567/ | head -1
kill %1
```
Expected: `HTTP/1.1 200 OK`（证明 CLI 入口 + 单端口 SPA 服务成立）；kill 后 `curl` 应失败

- [ ] **Step 5: Commit**

```bash
git add scripts/desktop-build-inkos.sh src-tauri/.gitignore
git commit -m "build(desktop): inkos build script (mono-repo) + smoke verified"
```

---

## Task 4: supervisor 核心逻辑（端口选择 + LaunchSpec 构造，纯函数 TDD）

**Files:**
- Create: `src-tauri/src/supervisor.rs`
- Modify: `src-tauri/src/lib.rs`
- Test: 内联单测（无 spawn，纯函数）

**Interfaces:**
- Consumes: `paths::PathResolver`、`config::{DEFAULT_STUDIO_PORT, CLI_ENTRY_REL}`
- Produces: `struct LaunchSpec { program: String, args: Vec<String>, env: HashMap<String,String>, cwd: PathBuf, port: u16 }`、`fn pick_free_port(start: u16) -> Option<u16>`、`fn build_launch<R: PathResolver>(paths: &R, port: u16, node_bin: &str) -> LaunchSpec`

- [ ] **Step 1: 写失败的测试（端口选择 + launch 构造）**

```rust
// src-tauri/src/supervisor.rs
use crate::config::{CLI_ENTRY_REL, DEFAULT_STUDIO_PORT};
use crate::paths::PathResolver;
use std::collections::HashMap;
use std::net::TcpListener;
use std::path::PathBuf;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LaunchSpec {
    pub program: String,
    pub args: Vec<String>,
    pub env: HashMap<String, String>,
    pub cwd: PathBuf,
    pub port: u16,
}

/// 从 start 起递增找首个可绑定的端口（含 start）。
pub fn pick_free_port(start: u16) -> Option<u16> {
    (start..start.saturating_add(1000))
        .find(|p| TcpListener::bind(("127.0.0.1", *p)).is_ok())
}

pub fn build_launch<R: PathResolver>(paths: &R, port: u16, node_bin: &str) -> LaunchSpec {
    let cli_entry = paths.submodule_root().join(CLI_ENTRY_REL);
    let mut env = HashMap::new();
    env.insert(
        "INKOS_PROJECT_ROOT".to_string(),
        paths.project_root().to_string_lossy().into_owned(),
    );
    env.insert("INKOS_STUDIO_PORT".to_string(), port.to_string());
    LaunchSpec {
        program: node_bin.to_string(),
        args: vec![
            cli_entry.to_string_lossy().into_owned(),
            "studio".to_string(),
            "--port".to_string(),
            port.to_string(),
        ],
        env,
        cwd: paths.project_root().to_path_buf(),
        port,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct DummyPaths { proj: PathBuf, sub: PathBuf }
    impl PathResolver for DummyPaths {
        fn project_root(&self) -> &std::path::Path { &self.proj }
        fn submodule_root(&self) -> &std::path::Path { &self.sub }
        fn log_dir(&self) -> PathBuf { self.proj.join("log") }
    }

    #[test]
    fn pick_free_port_returns_bindable_port() {
        let p = pick_free_port(4567).expect("应找到空闲端口");
        // 返回的端口确实可绑定（再次 bind 成功说明未被占）
        assert!(TcpListener::bind(("127.0.0.1", p)).is_ok());
    }

    #[test]
    fn build_launch_sets_project_root_and_port_env() {
        let paths = DummyPaths { proj: PathBuf::from("/tmp/proj"), sub: PathBuf::from("/tmp/inkos") };
        let spec = build_launch(&paths, 4567, "/usr/bin/node");
        assert_eq!(spec.env.get("INKOS_PROJECT_ROOT").unwrap(), "/tmp/proj");
        assert_eq!(spec.env.get("INKOS_STUDIO_PORT").unwrap(), "4567");
    }

    #[test]
    fn build_launch_invokes_studio_with_port() {
        let paths = DummyPaths { proj: PathBuf::from("/tmp/proj"), sub: PathBuf::from("/tmp/inkos") };
        let spec = build_launch(&paths, 4567, "/usr/bin/node");
        assert_eq!(spec.program, "/usr/bin/node");
        assert!(spec.args[0].ends_with("packages/cli/dist/index.js"));
        assert_eq!(&spec.args[1..], &["studio", "--port", "4567"]);
    }
}
```

- [ ] **Step 2: 跑测试验证通过**

Run: `cd src-tauri && cargo test supervisor`
Expected: PASS（3 tests）

- [ ] **Step 3: 注册模块并 Commit**

```rust
// src-tauri/src/lib.rs
pub mod config;
pub mod paths;
pub mod supervisor;
```

```bash
git add src-tauri/src/supervisor.rs src-tauri/src/lib.rs
git commit -m "feat(supervisor): port selection + launch spec construction"
```

---

## Task 5: supervisor spawn + health probe（mock 集成测 + 真实 inkos 集成测）

**Files:**
- Modify: `src-tauri/src/supervisor.rs`（加 spawn、health_probe）
- Create: `src-tauri/tests/supervisor_integration.rs`

**Interfaces:**
- Consumes: `LaunchSpec`、`config::HEALTH_PROBE_TIMEOUT`/`HEALTH_PROBE_INTERVAL`
- Produces: `fn spawn(spec: &LaunchSpec) -> anyhow::Result<std::process::Child>`、`async fn health_probe(port: u16, timeout: Duration) -> bool`

- [ ] **Step 1: 加 spawn + health_probe 实现**

```rust
// 追加到 src-tauri/src/supervisor.rs
use std::process::{Child, Command};
use std::time::{Duration, Instant};

pub fn spawn(spec: &LaunchSpec) -> anyhow::Result<Child> {
    Command::new(&spec.program)
        .args(&spec.args)
        .envs(spec.env.iter())
        .current_dir(&spec.cwd)
        .spawn()
        .map_err(Into::into)
}

/// 轮询 http://127.0.0.1:port/ 直到 200 或超时。
pub async fn health_probe(port: u16, timeout: Duration) -> bool {
    let url = format!("http://127.0.0.1:{}/", port);
    let deadline = Instant::now() + timeout;
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(2))
        .build()
        .unwrap();
    while Instant::now() < deadline {
        if client.get(&url).send().await.map(|r| r.status().is_success()).unwrap_or(false) {
            return true;
        }
        tokio::time::sleep(crate::config::HEALTH_PROBE_INTERVAL).await;
    }
    false
}
```

- [ ] **Step 2: 写 mock 集成测——验证 health_probe 对一个临时 HTTP 服务返回 true**

```rust
// src-tauri/tests/supervisor_integration.rs
use std::time::Duration;
use tokio::net::TcpListener;

#[tokio::test]
async fn health_probe_succeeds_when_server_up() {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
    tokio::spawn(async move {
        loop {
            if let Ok((mut sock, _)) = listener.accept().await {
                use tokio::io::AsyncWriteExt;
                let _ = sock.write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 0\r\n\r\n").await;
            }
        }
    });
    assert!(inkos_desktop::supervisor::health_probe(port, Duration::from_secs(3)).await);
}

#[tokio::test]
async fn health_probe_times_out_when_no_server() {
    let port = 9; // 黑洞端口
    assert!(!inkos_desktop::supervisor::health_probe(port, Duration::from_millis(500)).await);
}
```

注：`Cargo.toml` 的 `[lib]` 需暴露 crate 给集成测，确保 `src-tauri/src/lib.rs` 存在（Task 1 已建）。集成测用 crate 名 `inkos_desktop`。

- [ ] **Step 3: 跑测试验证通过**

Run: `cd src-tauri && cargo test --test supervisor_integration`
Expected: PASS（2 tests）

- [ ] **Step 4: 写真实 inkos 集成测（标记 `#[ignore]`，需本机已 `./scripts/desktop-build-inkos.sh`）**

```rust
// 追加到 src-tauri/tests/supervisor_integration.rs
use inkos_desktop::{config, paths::AppPaths, supervisor::{build_launch, spawn, health_probe}};
use std::time::Duration;

#[tokio::test]
#[ignore]
async fn real_inkos_sidecar_serves_spa() {
    // 集成测从 src-tauri/ 跑；inkos 工作区根 = 仓库根 = src-tauri 的上一级
    let inkos_root = std::env::current_dir().unwrap().parent().unwrap().to_path_buf();
    let paths = AppPaths::new(
        std::env::temp_dir().join("inkos-m1-demo"),
        inkos_root,
    ).unwrap();
    let port = config::DEFAULT_STUDIO_PORT;
    let spec = build_launch(&paths, port, "node");
    let mut child = spawn(&spec).expect("spawn inkos");
    let ok = health_probe(port, Duration::from_secs(60)).await;
    let _ = child.kill();
    assert!(ok, "inkos sidecar 应在 60s 内于 :4567 提供 SPA");
}
```

- [ ] **Step 5: 跑真实集成测**

Run: `cd src-tauri && cargo test --test supervisor_integration real_inkos -- --ignored --nocapture`
Expected: PASS（sidecar 60s 内健康）

- [ ] **Step 6: Commit**

```bash
git add src-tauri/src/supervisor.rs src-tauri/tests/supervisor_integration.rs
git commit -m "feat(supervisor): spawn + health probe with mock and real integration tests"
```

---

## Task 6: Tauri 接线——setup hook 拉起 sidecar + WebView 加载探测 URL

**Files:**
- Modify: `src-tauri/src/main.rs`、`src-tauri/tauri.conf.json`、`src-tauri/Cargo.toml`（加 `tauri-build`）
- Create: `src-tauri/build.rs`

**Interfaces:**
- Consumes: `supervisor::{pick_free_port, build_launch, spawn, health_probe}`、`paths::AppPaths`、`config::DEFAULT_STUDIO_PORT`

- [ ] **Step 1: 建 stub 占位前端目录**

```bash
mkdir -p src-tauri/stub && echo '<!doctype html><meta charset="utf-8"><title>inkosDesktop</title>' > src-tauri/stub/index.html
```

- [ ] **Step 2: 写 `tauri.conf.json`（最小窗口，运行时导航到 sidecar URL）**

```json
{
  "$schema": "https://schema.tauri.app/config/2",
  "productName": "inkosDesktop",
  "version": "0.1.0",
  "identifier": "com.inkos.desktop",
  "build": { "frontendDist": "../src-tauri/stub" },
  "app": {
    "windows": [{ "label": "main", "title": "inkosDesktop", "width": 1280, "height": 800 }],
    "security": { "csp": null }
  },
  "bundle": { "active": true, "targets": "all" }
}
```

> 注：M1 不加载 Tauri 自己的前端，而是运行时把主窗口 `eval`/导航到 `http://127.0.0.1:<port>`。`frontendDist` 指向一个 stub 占位目录（建一个 `stub/index.html`）。

- [ ] **Step 3: 写 `build.rs`**

```rust
fn main() { tauri_build::build() }
```

- [ ] **Step 4: 写 `main.rs`——setup 中拉起 sidecar、健康后导航窗口**

```rust
fn main() {
    tauri::Builder::default()
        .setup(|app| {
            let window = app.get_webview_window("main").expect("主窗口缺失");
            tauri::async_runtime::spawn(async move {
                let paths = inkos_desktop::paths::AppPaths::new(
                    std::env::temp_dir().join("inkos-m1-demo"),
                    std::env::current_dir().expect("无 cwd"), // mono-repo：submodule_root = 仓库根 = inkos 工作区根
                ).expect("解析路径失败");
                let port = inkos_desktop::supervisor::pick_free_port(
                    inkos_desktop::config::DEFAULT_STUDIO_PORT,
                ).expect("无空闲端口");
                let spec = inkos_desktop::supervisor::build_launch(&paths, port, "node");
                let _child = inkos_desktop::supervisor::spawn(&spec).expect("拉起 sidecar 失败");
                let url = format!("http://127.0.0.1:{}/", port);
                if inkos_desktop::supervisor::health_probe(port, inkos_desktop::config::HEALTH_PROBE_TIMEOUT).await {
                    let _ = window.eval(&format!("window.location.replace('{}')", url));
                } else {
                    let _ = window.eval("document.title='inkos 启动失败，见日志'");
                }
            });
            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("启动 Tauri 失败");
}
```

> 说明：Tauri 2 自带事件循环，`main` 不加 `#[tokio::main]`；异步任务用 `tauri::async_runtime::spawn`（内部即 tokio）。`WebviewWindow` 为 `Send + Sync`，可 move 进异步块。

- [ ] **Step 5: 手动冒烟——运行桌面 app 看到 Studio UI**

Run: `cd src-tauri && cargo tauri dev`
Expected: 窗口打开 → 数秒后导航到 inkos Studio 仪表盘（原版 SPA，零改）

- [ ] **Step 6: Commit**

```bash
git add src-tauri
git commit -m "feat(app): wire supervisor into tauri setup, load SPA via health probe"
```

---

## Task 7: loopback 加固——验证暴露 + 平台防火墙

**Files:**
- Create: `src-tauri/src/isolation.rs`、`src-tauri/src/isolation/macos.rs`、`linux.rs`、`windows.rs`
- Modify: `src-tauri/src/lib.rs`、`src-tauri/src/main.rs`（setup 中调 guard）

**Interfaces:**
- Consumes: 选定端口
- Produces: `trait LoopbackGuard { fn lock(&self, port: u16) -> anyhow::Result<()>; fn release(&self, port: u16) -> anyhow::Result<()>; }`、`fn platform_guard() -> Box<dyn LoopbackGuard>`

- [ ] **Step 1: 先实测确认默认 0.0.0.0 暴露（消解架构 §14.1）**

Run: 启动 sidecar（`INKOS_STUDIO_PORT=4567 ... node packages/cli/dist/index.js studio`），取本机 LAN IP（`ipconfig getifaddr en0`），另一台机器或手机 `curl http://<LAN-IP>:4567/`
Expected: 能访问 → 证实默认绑 0.0.0.0（证据归档到变更记录）。若已无法访问，记录环境差异。

- [ ] **Step 2: 写平台 guard trait + macOS 实现（pf anchor）**

```rust
// src-tauri/src/isolation.rs
pub mod macos;
pub mod linux;
pub mod windows;

pub trait LoopbackGuard {
    fn lock(&self, port: u16) -> anyhow::Result<()>;
    fn release(&self, port: u16) -> anyhow::Result<()>;
}

#[cfg(target_os = "macos")]
pub fn platform_guard() -> Box<dyn LoopbackGuard> { Box::new(macos::PfGuard) }
#[cfg(target_os = "linux")]
pub fn platform_guard() -> Box<dyn LoopbackGuard> { Box::new(linux::IptablesGuard) }
#[cfg(target_os = "windows")]
pub fn platform_guard() -> Box<dyn LoopbackGuard> { Box::new(windows::FirewallGuard) }
```

```rust
// src-tauri/src/isolation/macos.rs
use super::LoopbackGuard;
use std::process::Command;

pub struct PfGuard;

impl LoopbackGuard for PfGuard {
    fn lock(&self, port: u16) -> anyhow::Result<()> {
        // 拒绝外部到本端口的入站，放行 loopback
        let rule = format!("block in on en0 proto tcp from any to any port {port}");
        Command::new("/sbin/pfctl").args(["-ef", "/dev/stdin"])
            .stdin(std::process::Stdio::piped())
            .spawn()?.stdin.take()
            .ok_or_else(|| anyhow::anyhow!("无 stdin"))?
            .write_all(format!("{}\n", rule).as_bytes())?;
        Ok(())
    }
    fn release(&self, _port: u16) -> anyhow::Result<()> {
        Command::new("/sbin/pfctl").args(["-d"]).status()?;
        Ok(())
    }
}
use std::io::Write;
```

> 注：M1 的 pf 规则为占位实现（需 admin 权限且规则需精化）。**Step 1 实测后**，若 pf 方案在用户环境不可行（需 sudo），改用「Tauri App Sandbox 网络限制」或「文档化风险 + 默认禁用外部监听」。此决策在执行时记入变更记录。

- [ ] **Step 3: 写最小单元测——platform_guard 在当前平台可获取**

```rust
// src-tauri/src/isolation.rs 末尾
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn platform_guard_constructible() {
        let _g = platform_guard(); // 不实际 lock（需权限），仅验证派发
    }
}
```

- [ ] **Step 4: 跑测试 + 在 main.rs setup 中调 guard（lock 在 spawn 前、release 在退出时）**

Run: `cd src-tauri && cargo test isolation`
Expected: PASS

- [ ] **Step 5: 手动冒烟——加 guard 后 LAN 访问被拒**

Run: 加 guard 启动 app，再次从 LAN IP `curl http://<LAN-IP>:4567/`
Expected: 超时/拒绝；`curl http://127.0.0.1:4567/` 仍 200

- [ ] **Step 6: Commit**

```bash
git add src-tauri/src/isolation.rs src-tauri/src/isolation src-tauri/src/main.rs
git commit -m "feat(isolation): loopback firewall guard for sidecar port"
```

---

## Task 8: M1 冒烟归档 + 打 tag

**Files:**
- Modify: `README.md`（M1 运行说明）
- Create: `变更记录文档/20260806/M1冒烟验证.md`

- [ ] **Step 1: 写 README「如何运行 M1」**

```markdown
## 运行（M1）

1. `git submodule update --init inkos`
2. `./scripts/desktop-build-inkos.sh`
3. `cd src-tauri && cargo tauri dev`
```

- [ ] **Step 2: 跑全量测试 + 覆盖率检查**

Run: `cd src-tauri && cargo test && cargo install cargo-tarpaulin && cargo tarpaulin --skip-clean --out Html`
Expected: 全绿；行覆盖 ≥80%

- [ ] **Step 3: 归档 M1 冒烟结论到 `变更记录文档/20260806/M1冒烟验证.md`**

记录：单端口 SPA 加载成功、loopback 实测结论（pf 或替代方案）、§14.1 loopback 项消解。

- [ ] **Step 4: 打 tag**

```bash
git add README.md 变更记录文档
git commit -m "docs: M1 milestone complete"
git tag -a v0.1.0-m1 -m "M1: scaffold + sidecar + supervisor + loopback"
```

---

## Self-Review（已完成）

**1. Spec 覆盖**（架构 v1.1 Phase 1 MVP 针对 M1 范围）：
- Tauri 壳 + 三平台窗口 → Task 1/6（三平台构建/签名留 M3）
- inkos sidecar（submodule + 预构建）→ Task 3（M1 就地构建，发布打包 M3）
- supervisor 启动/监督/重启 → Task 4/5（重启/看门狗留 M2 随 lifecycle）
- WebView 加载 + initialScript 桥 → Task 6（initialScript 桥本体留 M2）
- isolation loopback 加固 → Task 7
- `INKOS_PROJECT_ROOT` 数据目录 → Task 4 env
- 基础测试 → 各 Task TDD + Task 8 覆盖率
- macOS 签名 → M3（M1 用 dev 模式，明确推迟）
- 未覆盖（M2/M3）：secrets、observer、updater、CI、签名——已在里程碑表声明

**2. 占位符扫描**：Task 7 pf 规则为「占位实现 + 实测后决策」，属必要的不确定项处理（架构 §14.1 要求实测），非偷懒占位；其余步骤均含可执行代码/命令。

**3. 类型一致性**：`LaunchSpec`、`PathResolver`、`AppPaths`、`LoopbackGuard` 跨任务签名一致；`pick_free_port`/`build_launch`/`spawn`/`health_probe` 名称全程统一。

---

## M2/M3 预告（后续计划，本文件不展开）

- **M2**：secrets（keychain→`.inkos/secrets.json`，路线 A）、observer（旁路 `/api/v1/events`→原生通知/托盘角标）、lifecycle（托盘保活/快捷键）、supervisor 看门狗重启、initialScript 桥本体
- **M3**：updater 引擎通道（跟随上游 release + 原子回滚）、便携 Node + `engine/` 发布打包、CI（三平台构建 + 同步回归 + SSE 契约测）、macOS 签名/公证
