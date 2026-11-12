# E2E 测试指引

## 概述

E2E（End-to-End）回归测试验证关键路径完整性：
- **启动冒烟**：app 启动 + 日志初始化
- **Updater 契约**：版本解析 + SHA256 校验 + 原子替换
- **Secrets 契约**：跨平台 keychain 读写删
- **Watchers 端到端**：secrets.json 文件监听回写（防回环 + debounce）
- **Supervisor**：真实 sidecar 拉起 + 健康探测 + 干净退出

> 993 号对齐：本文档与 988-992 三轮挂起清偿后的真实形态同步——全部挂起用例
> （23 枚，含 lib 单测与全部集成目标）已收编一键跑法，`#[ignore]` 不再等于
> 「不跑」。

## 本地运行

### 一键跑法（推荐）

```bash
pnpm cargo:ignored-sweep
```

全清单 23 枚六 scope（lib 7 + sync_watcher_io 3 + e2e_updater 1 +
supervisor_integration 1 + e2e_startup 2 + e2e_secrets 9），带：
- **清单地板卫兵**：实测集合与登记清单精确匹配，静默缩水（用例被删/改名/
  过滤器失效）即红；
- **超时哨兵**（992 号）：每 scope 按面定预算，keychain 锁定（dark wake）等
  无限挂死变有限红报（含根因指引与孤儿清理命令）；
- supervisor scope 前自动跑 `desktop-package-engine.sh` 组装引擎（幂等）。

约束：勿与其它 `cargo test` 并行跑（同机双实例 = target 锁 + securityd
跨进程串行化，分钟级停滞假象，988 号实测）；supervisor 需 :4567 空闲；
keychain 面需桌面解锁会话。

### 手动单跑

release binary **仅 e2e_startup 需要**（`Command::cargo_bin` 找 release 产物；
sweep 会自动触发 `--release` 编译）。其余目标 debug 即可：

```bash
# 启动冒烟测试（唯一需要 release 的目标）
cargo test --release --test e2e_startup -- --ignored --nocapture

# Updater 测试（is_newer 纯逻辑 + SHA256 + 原子替换；无需 release）
cargo test --test e2e_updater -- --ignored --nocapture

# Secrets 测试（990 号起 unique_service：无 ACL 弹框；无需 release）
cargo test --test e2e_secrets -- --ignored --test-threads=1 --nocapture

# Watcher 端到端（988 号起探针自同步：无固定 sleep 竞态）
cargo test --test sync_watcher_io -- --ignored --nocapture

# 真实 sidecar（需先 pnpm cargo:ignored-sweep 或手动跑
# ./scripts/desktop-package-engine.sh 组装 src-tauri/engine）
cargo test --test supervisor_integration -- --ignored real_inkos --nocapture
```

**参数说明**:
- `--ignored`: 运行标记为 `#[ignore]` 的测试
- `--test-threads=1`: keychain 面串行（securityd 并发抖动防护）
- `--nocapture`: 显示测试输出（含 `[secrets-writeback]` watcher 日志）

### 平台构建依赖（构建 src-tauri 时需要，跑测试本身不需要）

**Linux**:
```bash
sudo apt-get update
sudo apt-get install -y \
  libwebkit2gtk-4.1-dev \
  libssl-dev \
  libgtk-3-dev \
  libayatana-appindicator3-dev \
  librsvg2-dev
```

**Windows**: MSVC 工具链（Visual Studio Build Tools）。

**macOS**: 无额外依赖（keychain 系统自带；测试用 unique_service 不弹授权框）。

### 运行特定测试用例

```bash
cargo test --release \
  --test e2e_startup \
  test_startup_logs_version \
  -- --ignored --nocapture
```

## 失败时调试

本仓无 CI（2026-08-23 起移除全部 GitHub Actions/workflows，回归走本地脚本
+ `pnpm cargo:ignored-sweep` 常备）。失败时本地复现：

```bash
# 设置相同环境变量
export RUST_BACKTRACE=1
export RUST_LOG=info

# 运行失败的测试
cargo test --test e2e_<name> <test_function> -- --ignored --nocapture
```

sweep 超时哨兵红报（992 号）自带诊断路径：预算值 → 高概率根因（keychain
锁定 / securityd 异常 / 编译僵死）→ 孤儿清理 pkill 命令。

## 已知问题（历史误诊备案，993 号勘误）

旧版本文档记载「keyring 3.6.3 在 macOS 上 set_password 只写内存不落盘，
跨实例读不到」——**988/990 两轮实测否定**：lib 四枚 keyring 真实测试
（roundtrip 跨实例读回 / 并发 20 线程无丢键）与 e2e_secrets 九枚全部绿
（990 号 9/9 ×2 活体）。当时「读不到」的真实机制是**固定 service 名跨构建
ACL 授权框**：keychain 条目按创建时二进制签名授权访问者，重构建后的测试
二进制访问旧条目 → securityd 拒绝/弹框被误读为「没写进去」。990 号
unique_service（PID+纳秒唯一命名空间）修复后问题不复存在。

当前唯一真实环境门控：**keychain 锁定（dark wake/锁屏）时 securityd 调用
阻塞**——由 sweep 超时哨兵兜底（挂死变有限红报），解锁后重跑即可。

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

### 1. keychain 问题（macOS）

**错误**: `keyring error: No password found` 或 sweep 超时哨兵红报

**语义（990 号后）**: 测试用 unique_service 唯一命名空间，无跨构建 ACL
弹框；真实风险只剩 **keychain 锁定**（dark wake/锁屏时 securityd 阻塞）。

**解决**: 解锁屏幕（保持桌面会话活跃）后重跑；sweep 哨兵红报自带孤儿清理
命令。

### 2. 超时失败

**错误**: sweep 红报 `超时哨兵触发（预算 Xms 被 SIGKILL）`

**原因**:
- keychain 锁定 / securityd 异常（secrets、lib keyring 面）
- Sidecar 启动慢 / 端口占用（supervisor 面）
- release 缓存陈旧触发长编译（startup 面，预算 1200s 最宽）

**解决**: 按红报根因指引处理；孤儿清理：
```bash
pkill -f inkos-desktop; pkill -f inkos-engine-server
lsof -i :4567  # supervisor 端口预检也会暴露残留
```

### 3. 并发冲突

**错误**: `已锁定` 或 `文件正在使用`，或同机双 cargo 出现分钟级停滞

**原因**: 多个测试并发写 keychain；或双 cargo 实例 target 锁 +
securityd 跨进程串行化（988 号实测假象）

**解决**:
```bash
# sweep 已内置串行与单跑约束；手动跑时：
cargo test --test 'e2e_*' -- --ignored --test-threads=1
# 且勿与其它 cargo test 并行
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

## 测试覆盖范围（挂起清偿面全量，988-990 三轮收编）

| Scope | 覆盖模块 | 关键场景 | 枚数 |
|-------|---------|---------|------|
| `--lib` | isolation/pfctl、secrets/keyring、updater 双通道、engine/node | C5 bail、keychain 真实四枚、GitHub 探测、node 下载校验 | 7 |
| `sync_watcher_io` | secrets::sync | 防回环中途断言、SPA 回写+删除传播、shutdown 切片 | 3 |
| `e2e_updater` | updater | is_newer 版本比较（纯逻辑，990 勘误去假挂起） | 1 |
| `supervisor_integration` | supervisor | 真实 sidecar 拉起→SPA 可达→kill_tree 端口释放 | 1 |
| `e2e_startup`（release） | main、observability | 启动冒烟 + 版本日志（闪现窗口 ~5s×2 预期） | 2 |
| `e2e_secrets` | secrets::store | keychain 契约面（unique_service，990 号修复） | 9 |

## 添加新测试

1. **创建测试文件**:
```bash
touch src-tauri/tests/e2e_<feature>.rs
```

2. **添加测试标记**:
```rust
#[test]
#[ignore = "环境门控理由（须写明触发条件与跑法）"]
fn test_new_feature() {
    // ...
}
```

3. **登记进 sweep 清单**: 把用例名加入 `scripts/cargo-ignored-sweep.mjs`
   的 `INVENTORY` 对应 scope（清单地板卫兵会让「有测试不登记」显形为红），
   并按面定 `timeoutMs` 预算。

4. **本地验证**:
```bash
pnpm cargo:ignored-sweep
```

5. **更新此文档**: 添加新测试到"测试覆盖范围"表格

## 性能基准（993 号对齐 988-990 实测，macOS M 系热缓存）

- `--lib`（7 枚）: ~33s（含 keychain 并发用例与网络两枚）
- `sync_watcher_io`（3 枚）: ~6s
- `e2e_updater`（1 枚）: 0.00s（纯逻辑）
- `supervisor_integration`（1 枚）: ~1s（引擎已组装时）
- `e2e_startup`（2 枚，release 热缓存）: ~10s
- `e2e_secrets`（9 枚，串行）: ~19s
- **sweep 全量**: ~60-80s（release 缓存陈旧时另计编译）

## 维护清单

- [ ] 每次添加新功能后，评估是否需要新 E2E 测试（并登记 sweep 清单）
- [ ] 挂起用例增删须同步 `scripts/cargo-ignored-sweep.mjs` INVENTORY（地板
      卫兵强制）与本表格
- [ ] sweep 哨兵红报出现时按根因处理，禁为绕红放宽预算洗哨兵
- [ ] 更新此文档当添加新测试或修改运行方式
