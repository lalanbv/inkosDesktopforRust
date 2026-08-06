# E2E 测试指引

## 概述

E2E（End-to-End）回归测试验证关键路径完整性：
- **启动冒烟**：app 启动 + 日志初始化
- **Updater 契约**：版本解析 + SHA256 校验 + 原子替换
- **Secrets 契约**：跨平台 keychain 读写删

## 本地运行

### 前置条件

1. **构建 release binary**
```bash
cargo build --release --manifest-path src-tauri/Cargo.toml
```

2. **平台特定设置**

**macOS**:
```bash
# 授予 keychain 访问权限（首次运行可能需要）
security unlock-keychain -p "" ~/Library/Keychains/login.keychain-db
```

**Linux**:
```bash
# 安装 Tauri 依赖
sudo apt-get update
sudo apt-get install -y \
  libwebkit2gtk-4.1-dev \
  libssl-dev \
  libgtk-3-dev \
  libayatana-appindicator3-dev \
  librsvg2-dev
```

**Windows**:
```powershell
# 确保 MSVC 工具链已安装（Visual Studio Build Tools）
```

### 运行所有 E2E 测试

```bash
cargo test --release \
  --manifest-path src-tauri/Cargo.toml \
  --test 'e2e_*' \
  -- --ignored --test-threads=1 --nocapture
```

**参数说明**:
- `--release`: 使用 release binary（E2E 测试标记为 `#[ignore]`，需要 release 模式）
- `--test 'e2e_*'`: 只运行 E2E 测试文件
- `--ignored`: 运行标记为 `#[ignore]` 的测试
- `--test-threads=1`: 串行运行（避免并发 keychain/temp 目录冲突）
- `--nocapture`: 显示测试中的 `println!` 输出

### 运行单个测试文件

```bash
# 启动冒烟测试
cargo test --release --test e2e_startup -- --ignored --nocapture

# Updater 测试
cargo test --release --test e2e_updater -- --ignored --nocapture

# Secrets 测试
cargo test --release --test e2e_secrets -- --ignored --nocapture
```

### 运行特定测试用例

```bash
cargo test --release \
  --test e2e_startup \
  test_startup_logs_version \
  -- --ignored --nocapture
```

## CI 验证

E2E 测试在以下情况自动触发：
- **Pull Request**：修改 `src-tauri/**` 或 `engine/**`
- **Push to master**：合并后回归验证
- **手动触发**：GitHub Actions → "Desktop E2E Tests" → "Run workflow"

### 查看 CI 日志

1. 打开 PR 或 commit 页面
2. 点击 **Checks** 标签
3. 选择 **Desktop E2E Tests**
4. 点击对应平台查看详细日志：
   - `E2E (macOS ARM64)`
   - `E2E (Linux x64)`
   - `E2E (Windows x64)`

### 失败时调试

CI 失败时，自动上传两类 artifacts（保留 7 天）：

1. **日志文件**（`e2e-logs-<platform>`）：
   - 包含 `logs/inkos-*.log`（tracing 输出）
   - 包含 `target/release/*.log`（构建日志）

2. **崩溃 dump**（`e2e-crashes-<platform>`）：
   - 包含 `crashes/crash-*.json`（panic dump）

**下载步骤**:
1. GitHub Actions 页面 → 失败的 workflow run
2. 滚动到底部 **Artifacts** 区域
3. 下载对应平台的 zip 文件

**本地复现**:
```bash
# 设置相同环境变量
export RUST_BACKTRACE=1
export RUST_LOG=info

# 运行失败的测试
cargo test --release --test e2e_<name> <test_function> -- --ignored --nocapture
```

## 已知问题

### macOS keyring 测试失败

**现象**: `e2e_secrets` 测试失败，`KeyringStore::upsert` 返回成功但 `read_all` 读不到数据。

**根因**: keyring crate 3.6.3 在 macOS 上存在已知问题——`Entry::set_password` 在内存中缓存密码但未真正写入系统 keychain。同一 Entry 实例可以读取，但新实例读不到。

**影响**: 
- Secrets E2E 测试在 macOS 上失败
- 实际应用中 secrets 功能可能受影响（需进一步验证生产场景）

**临时方案**:
- macOS 上跳过 `e2e_secrets` 测试
- Linux/Windows 平台测试 keyring 功能
- 或使用 `security` 命令行直接操作 keychain（绕过 keyring crate）

**长期方案**:
1. 升级到 keyring v4+ （API 变更需适配）
2. 或实现 macOS 专用后端直接调用 Security Framework
3. 或切换到其他 keyring 库（如 `security-framework` crate）

详见 GitHub Issue: [待创建]

## 常见问题

### 1. keychain 权限错误（macOS）

**错误**: `keyring error: No password found`

**解决**:
```bash
# 解锁 keychain
security unlock-keychain -p "" ~/Library/Keychains/login.keychain-db

# 或：允许测试 app 访问 keychain（首次运行时弹窗，点击"始终允许"）
```

### 2. 超时失败

**错误**: `test result: FAILED. 0 passed; 1 failed; 0 ignored; timeout`

**原因**:
- Sidecar 启动慢（端口占用？Node 下载慢？）
- 系统资源不足

**解决**:
```bash
# 1. 检查端口占用
lsof -i :3000  # 默认 sidecar 端口

# 2. 清理旧进程
pkill -f inkos-desktop
pkill -f node

# 3. 增加超时（修改测试代码）
.timeout(Duration::from_secs(120))  // 默认 30s → 120s
```

### 3. 并发冲突

**错误**: `已锁定` 或 `文件正在使用`

**原因**: 多个测试并发写入同一 temp 目录或 keychain

**解决**:
```bash
# 强制串行运行
cargo test --release --test 'e2e_*' -- --ignored --test-threads=1
```

### 4. Windows: `taskkill` 失败

**错误**: `ERROR: 找不到进程 "inkos-desktop.exe"`

**原因**: Windows 进程管理延迟

**解决**: 测试会自动重试 `taskkill`，通常第二次成功。如持续失败，手动清理：
```powershell
taskkill /F /IM inkos-desktop.exe /T
```

### 5. Linux: Secret Service 不可用

**错误**: `Error: No secret service available`

**原因**: headless Linux 环境无 GNOME Keyring/KWallet

**解决**:
```bash
# 安装并启动 gnome-keyring
sudo apt-get install -y gnome-keyring
eval $(dbus-launch)
echo -n "test" | gnome-keyring-daemon --unlock
```

**CI 环境**: 使用 file-based fallback（已在代码中实现）

## 测试覆盖范围

| 测试文件 | 覆盖模块 | 关键场景 |
|---------|---------|---------|
| `e2e_startup.rs` | main.rs, observability | 启动成功 + 日志创建 + 版本记录 |
| `e2e_updater.rs` | updater::engine | 版本解析 + SHA256 + 原子替换 + 回滚 |
| `e2e_secrets.rs` | secrets::store | keychain 读写删 + 多 key + Unicode |

## 添加新测试

1. **创建测试文件**:
```bash
touch src-tauri/tests/e2e_<feature>.rs
```

2. **添加测试标记**:
```rust
#[test]
#[ignore]  // E2E 测试必须标记 #[ignore]
fn test_new_feature() {
    // ...
}
```

3. **本地验证**:
```bash
cargo test --release --test e2e_<feature> -- --ignored
```

4. **更新此文档**: 添加新测试到"测试覆盖范围"表格

## 性能基准

参考执行时间（macOS M1 Max）：
- `e2e_startup`: ~5s
- `e2e_updater`: ~3s
- `e2e_secrets`: ~2s
- **总计**: ~10s

CI 环境可能慢 2-3 倍（虚拟机性能）。

## 维护清单

- [ ] 每次添加新功能后，评估是否需要新 E2E 测试
- [ ] 每月检查 CI artifacts 是否有遗留（应保留 7 天后自动删除）
- [ ] 版本发布前，手动运行三平台 E2E（`workflow_dispatch`）
- [ ] 更新此文档当添加新测试或修改运行方式
