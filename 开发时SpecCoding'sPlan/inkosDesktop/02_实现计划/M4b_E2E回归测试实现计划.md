# M4b E2E 回归测试实现计划

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 关键路径自动化回归测试，CI 门禁防破坏性变更

**Architecture:** Rust `#[test]` + 真实 binary（`cargo build --release`）+ CI 三平台并行

**Tech Stack:** 
- Rust std test framework
- `assert_cmd` (binary 测试)
- `tempfile` (隔离环境)
- GitHub Actions (desktop-e2e.yml)

## Global Constraints

- 测试超时：单测试 120s（sidecar 启动 + 健康探测）
- 隔离：每测试独立 temp 目录（避免并发冲突）
- 幂等：可重复运行，不依赖外部服务（mock GitHub API）
- 三平台：macOS / Linux / Windows（CI 并行验证）

---

## Task 1: 启动冒烟测试

**Files:**
- Create: `src-tauri/tests/e2e_startup.rs`
- Modify: `src-tauri/Cargo.toml` (添加 dev-dependencies)

**Interfaces:**
- Consumes: `cargo build --release` 产物（target/release/inkos-desktop）
- Produces: 启动成功断言（exit code 0 + 日志包含 "inkosDesktop 启动"）

- [ ] **Step 1: 添加测试依赖**

修改 `Cargo.toml`:
```toml
[dev-dependencies]
assert_cmd = "2.0"
predicates = "3.0"
tempfile = "3.8"
```

- [ ] **Step 2: 创建启动冒烟测试骨架**

```rust
use assert_cmd::Command;
use std::time::Duration;
use tempfile::TempDir;

#[test]
#[ignore] // 需要 release binary，CI 专用
fn test_startup_smoke() {
    let temp_dir = TempDir::new().unwrap();
    
    // 启动 app（headless，30s 超时）
    let mut cmd = Command::cargo_bin("inkos-desktop").unwrap();
    cmd.env("INKOS_APP_DATA_DIR", temp_dir.path())
        .timeout(Duration::from_secs(30));
    
    // 预期：启动成功（exit 0）
    cmd.assert().success();
}
```

- [ ] **Step 3: 运行测试验证框架**

```bash
cargo build --release
cargo test --test e2e_startup test_startup_smoke -- --ignored
```

预期：PASS（binary 启动 + 正常退出）

- [ ] **Step 4: 增强断言（日志验证）**

```rust
#[test]
#[ignore]
fn test_startup_logs_version() {
    let temp_dir = TempDir::new().unwrap();
    let log_dir = temp_dir.path().join("logs");
    
    let mut cmd = Command::cargo_bin("inkos-desktop").unwrap();
    cmd.env("INKOS_APP_DATA_DIR", temp_dir.path())
        .timeout(Duration::from_secs(30));
    
    cmd.assert().success();
    
    // 验证日志文件创建
    let log_files: Vec<_> = std::fs::read_dir(&log_dir)
        .unwrap()
        .filter_map(Result::ok)
        .collect();
    assert!(!log_files.is_empty(), "日志文件应被创建");
    
    // 验证日志包含版本号
    let log_content = std::fs::read_to_string(log_files[0].path()).unwrap();
    assert!(log_content.contains(env!("CARGO_PKG_VERSION")));
}
```

- [ ] **Step 5: 提交**

```bash
git add tests/e2e_startup.rs Cargo.toml
git commit -m "test(m4b): add startup smoke test"
```

---

## Task 2: Updater 契约测试

**Files:**
- Create: `src-tauri/tests/e2e_updater.rs`
- Create: `src-tauri/tests/fixtures/mock_release.json` (GitHub API mock)

**Interfaces:**
- Consumes: `updater::engine` 模块
- Produces: 版本解析 + SHA256 校验 + 原子替换验证

- [ ] **Step 1: 创建 mock GitHub release**

```json
{
  "tag_name": "v0.4.0-test",
  "assets": [
    {
      "name": "engine-v0.4.0-darwin-aarch64.tar.gz",
      "browser_download_url": "file:///path/to/mock.tar.gz"
    }
  ]
}
```

- [ ] **Step 2: 编写版本解析测试**

```rust
use inkos_desktop::updater::engine::parse_latest_version;

#[test]
fn test_parse_github_release() {
    let mock_json = include_str!("fixtures/mock_release.json");
    let version = parse_latest_version(mock_json).unwrap();
    assert_eq!(version, "0.4.0-test");
}
```

- [ ] **Step 3: 编写 SHA256 校验测试**

```rust
use inkos_desktop::updater::engine::verify_checksum;

#[test]
fn test_checksum_verification() {
    let test_file = tempfile::NamedTempFile::new().unwrap();
    std::fs::write(&test_file, b"test content").unwrap();
    
    // 计算预期 SHA256
    let expected = "sha256:...";
    
    let result = verify_checksum(test_file.path(), expected);
    assert!(result.is_ok());
}
```

- [ ] **Step 4: 集成测试（下载 + 解压 + 替换）**

```rust
#[test]
#[ignore]
fn test_engine_update_flow() {
    let temp_dir = TempDir::new().unwrap();
    let engine_dir = temp_dir.path().join("engine");
    
    // 1. 模拟当前版本
    create_mock_engine(&engine_dir, "0.3.0");
    
    // 2. 执行更新（mock URL）
    let result = apply_engine_update(&engine_dir, "file:///mock.tar.gz");
    assert!(result.is_ok());
    
    // 3. 验证新版本
    let manifest = read_manifest(&engine_dir).unwrap();
    assert_eq!(manifest.version, "0.4.0-test");
}
```

- [ ] **Step 5: 提交**

```bash
git add tests/e2e_updater.rs tests/fixtures/
git commit -m "test(m4b): add updater contract tests"
```

---

## Task 3: Secrets 契约测试

**Files:**
- Create: `src-tauri/tests/e2e_secrets.rs`

**Interfaces:**
- Consumes: `secrets::store::KeyringStore`
- Produces: 跨平台 keychain 读写删验证

- [ ] **Step 1: 编写 keychain 写入测试**

```rust
use inkos_desktop::secrets::store::{KeyringStore, SecretStore};

#[test]
fn test_keychain_write() {
    let store = KeyringStore::new("test-service", "test-user");
    let result = store.write("test-key", "test-value");
    assert!(result.is_ok());
}
```

- [ ] **Step 2: 编写 keychain 读取测试**

```rust
#[test]
fn test_keychain_read() {
    let store = KeyringStore::new("test-service", "test-user");
    store.write("test-key", "test-value").unwrap();
    
    let value = store.read("test-key").unwrap();
    assert_eq!(value, "test-value");
}
```

- [ ] **Step 3: 编写 keychain 删除测试**

```rust
#[test]
fn test_keychain_delete() {
    let store = KeyringStore::new("test-service", "test-user");
    store.write("test-key", "test-value").unwrap();
    
    let result = store.delete("test-key");
    assert!(result.is_ok());
    
    // 验证已删除
    let read_result = store.read("test-key");
    assert!(read_result.is_err());
}
```

- [ ] **Step 4: 集成测试（完整流程）**

```rust
#[test]
fn test_secrets_full_cycle() {
    let store = KeyringStore::new("inkos-e2e", "test-user");
    
    // 写入
    store.write("api-key", "sk-test-123").unwrap();
    
    // 读取
    let value = store.read("api-key").unwrap();
    assert_eq!(value, "sk-test-123");
    
    // 删除
    store.delete("api-key").unwrap();
    
    // 验证删除
    assert!(store.read("api-key").is_err());
}
```

- [ ] **Step 5: 提交**

```bash
git add tests/e2e_secrets.rs
git commit -m "test(m4b): add secrets contract tests"
```

---

## Task 4: CI 集成（desktop-e2e.yml）

**Files:**
- Create: `.github/workflows/desktop-e2e.yml`

**Interfaces:**
- Consumes: Task 1-3 的测试
- Produces: PR 门禁（三平台 × E2E 全绿才允许合并）

- [ ] **Step 1: 创建 workflow 文件**

```yaml
name: Desktop E2E Tests

on:
  pull_request:
    paths:
      - 'src-tauri/**'
      - 'engine/**'
  push:
    branches: [master]

jobs:
  e2e:
    name: E2E (${{ matrix.platform }})
    runs-on: ${{ matrix.os }}
    strategy:
      fail-fast: false
      matrix:
        platform:
          - { os: macos-14, target: aarch64-apple-darwin }
          - { os: ubuntu-22.04, target: x86_64-unknown-linux-gnu }
          - { os: windows-2022, target: x86_64-pc-windows-msvc }

    steps:
      - uses: actions/checkout@v4
      
      - name: Setup Rust
        uses: dtolnay/rust-toolchain@stable
      
      - name: Rust cache
        uses: swatinem/rust-cache@v2
        with:
          workspaces: src-tauri
      
      - name: Build release binary
        run: cargo build --release --manifest-path src-tauri/Cargo.toml
      
      - name: Run E2E tests
        run: cargo test --release --manifest-path src-tauri/Cargo.toml --test 'e2e_*' -- --ignored --test-threads=1
        timeout-minutes: 10
```

- [ ] **Step 2: 添加平台特定设置**

macOS:
```yaml
      - name: macOS setup
        if: matrix.platform.os == 'macos-14'
        run: |
          # 授予 keychain 访问权限（CI 环境）
          security unlock-keychain -p "" ~/Library/Keychains/login.keychain-db
```

Linux:
```yaml
      - name: Linux setup
        if: matrix.platform.os == 'ubuntu-22.04'
        run: |
          # 安装依赖
          sudo apt-get update
          sudo apt-get install -y libwebkit2gtk-4.1-dev libssl-dev
```

- [ ] **Step 3: 添加失败时上传日志**

```yaml
      - name: Upload logs on failure
        if: failure()
        uses: actions/upload-artifact@v4
        with:
          name: e2e-logs-${{ matrix.platform.os }}
          path: |
            /tmp/inkos-e2e-*/logs/
            target/release/*.log
```

- [ ] **Step 4: 本地验证 workflow 语法**

```bash
# 安装 actionlint
brew install actionlint  # macOS
# or: go install github.com/rhysd/actionlint@latest

# 验证语法
actionlint .github/workflows/desktop-e2e.yml
```

- [ ] **Step 5: 提交并推送触发 CI**

```bash
git add .github/workflows/desktop-e2e.yml
git commit -m "ci(m4b): add E2E regression workflow for 3 platforms"
git push origin HEAD
```

预期：CI 在 GitHub Actions 页面显示三平台并行执行

---

## Task 5: 文档化测试运行指引

**Files:**
- Create: `src-tauri/tests/README.md`

**Interfaces:**
- Consumes: Task 1-4
- Produces: 本地运行 + CI 调试指引

- [ ] **Step 1: 编写测试运行指引**

```markdown
# E2E 测试指引

## 本地运行

### 前置条件
```bash
# 构建 release binary
cargo build --release --manifest-path src-tauri/Cargo.toml
```

### 运行所有 E2E 测试
```bash
cargo test --release --test 'e2e_*' -- --ignored --test-threads=1
```

### 运行单个测试
```bash
cargo test --release --test e2e_startup test_startup_smoke -- --ignored
```

## CI 验证

E2E 测试在 PR 时自动触发（`.github/workflows/desktop-e2e.yml`）。

### 查看 CI 日志
1. 打开 PR 页面
2. 点击 "Checks" 标签
3. 选择 "Desktop E2E Tests"
4. 查看对应平台的日志

### 失败时调试
- 下载上传的 artifacts（`e2e-logs-<platform>`）
- 检查 `logs/inkos-*.log` 查看详细日志
- 本地复现：`cargo test --release ...`

## 常见问题

### keychain 权限错误（macOS）
```bash
security unlock-keychain -p "" ~/Library/Keychains/login.keychain-db
```

### 超时失败
- 检查 sidecar 是否正常启动（端口占用？）
- 增加超时：`timeout(Duration::from_secs(120))`

### 并发冲突
- 确保每测试独立 temp 目录
- 使用 `--test-threads=1` 串行运行
```

- [ ] **Step 2: 提交**

```bash
git add src-tauri/tests/README.md
git commit -m "docs(m4b): add E2E testing guide"
```

---

## Task 6: 验收与归档

**Files:**
- Create: `变更记录文档/20260806/M4b_E2E回归测试.md`

**Interfaces:**
- Consumes: Task 1-5 全部完成
- Produces: M4b 完成证明 + 验收报告

- [ ] **Step 1: 本地验收**

```bash
# 1. 构建
cargo build --release --manifest-path src-tauri/Cargo.toml

# 2. 运行所有 E2E 测试
cargo test --release --test 'e2e_*' -- --ignored --test-threads=1

# 3. 验证全绿
echo "预期：所有测试 PASS，无 FAIL"
```

- [ ] **Step 2: CI 验收**

1. 推送分支触发 CI
2. 验证三平台全绿
3. 检查 PR 门禁状态（Required checks）

- [ ] **Step 3: 编写变更记录**

记录：
- 新增的 3 个测试文件
- CI workflow 配置
- 测试覆盖范围（启动/updater/secrets）
- 验收结果（本地 + CI）

- [ ] **Step 4: 提交变更记录**

```bash
git add 变更记录文档/20260806/M4b_E2E回归测试.md
git commit -m "docs(m4b): M4b E2E regression complete"
```

---

## 验收标准

- [ ] 本地运行 `cargo test --release --test 'e2e_*' -- --ignored` 全绿
- [ ] CI 三平台（macOS / Linux / Windows）全绿
- [ ] PR 门禁配置生效（E2E 失败 → 阻止合并）
- [ ] 测试文档完整（README.md 可指导他人运行）
- [ ] 变更记录归档

---

## 依赖

- M4a（observability）→ 日志验证依赖 tracing 输出
- Tauri 2.11.5 → binary 启动行为
- GitHub Actions → CI runner 环境
