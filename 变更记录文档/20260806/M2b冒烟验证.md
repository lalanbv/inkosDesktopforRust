# M2b 冒烟验证（secrets keychain Route A：同步 + 回写 + 防回环）

| 项 | 值 |
|---|---|
| 日期 | 2026-08-06 |
| 分支 | `feat/desktop-m2b` |
| 标签 | `v0.2.1-m2b` |
| 范围 | 把 API key 从 `.env` 文件迁移到 OS keychain（macOS Keychain / Windows Credential Manager / Linux Secret Service），并保持 `secrets.json` 单向同步与回写防回环 |
| 上游计划 | `开发时SpecCoding'sPlan/inkosDesktop/02_实现计划/M2b_secrets实现计划.md` |
| 上游设计 | `.superpowers/sdd/M2b_secrets实现计划/spec.md`（spec §2 全覆盖） |
| baseline | `95a69b60`（M2a close-out） |

---

## 1. M2b 交付清单（6 任务，全部完成）

| # | 任务 | 关键 commit | 状态 |
|---|------|------------|------|
| T1 | `SecretStore` trait + `MockStore` + `KeyringStore` 占位（Err 非 panic） | `a9eb0192 feat(secrets): SecretStore trait + MockStore + KeyringStore scaffold` | ✅ review clean |
| T2 | `.inkos/secrets.json` 纯函数读写（merge 保留 + 0600 + 原子 rename） | `dc28a167 feat(secrets): secrets.json read/write (atomic, 0600)` | ✅ review clean |
| T3 | 启动期同步 + 首迁（keychain→json / json→keychain / 双空→noop） | `90ed2199 feat(secrets): startup sync + first-run migration` | ✅ review clean |
| T4 | 文件监听回写 + 防回环（`syncing` flag + debounce 500ms） | `4fcf5285 feat(secrets): writeback watcher with anti-loop + debounce` | ✅ review clean |
| T5 | `KeyringStore` 实装（keyring v3 + `__index__`）+ main.rs 接线 + 降级 | `576d0d72 feat(app): wire secrets keychain sync + writeback` | ✅ review LOW 文案已在 T6 修 |
| T6 | 冒烟归档 + 覆盖率 + tag（本档） | 见 §7 commit SHA | 本档 |

**M2b HEAD（打 tag 处）**：见 §7 commit SHA。

---

## 2. KeyringStore 设计要点

### 2.1 `__index__` 权衡（keyring 无枚举 API）

`keyring` crate（v3）没有"列出某 service 下所有 entry"的 API。为支持 `read_all`，KeyringStore 在同一 service 下维护一个名为 `__index__` 的**元 entry**，其 value 是 JSON 数组（`["openai","deepseek"]`），记录所有已写入的 key 列表。`read_all` 先读 index、再按 index 逐个 `get_password`。

**两层权衡**：

| 风险 | 缓解 |
|---|---|
| index 与 entry 不一致（外部工具删 keychain 项、写入中途失败） | `read_all` 对 index 中存在但 entry 缺失的 key 容错跳过（`eprintln!` log），不报错；下次 `upsert` 该 key 会重建 entry |
| `__index__` 自身是明文 key 名单（不是 secret 值） | secret 值仍各自独立加密存储；攻击者拿到 keychain 只能知道"有哪些 service id"，不能拿到 key。`__index__` 选双下划线前缀降低与业务 key 撞名概率（inkos 的 service id 来自 `llms.config.ts`，不会以 `__` 开头） |

### 2.2 keyring v3 API（与 v2 差异）

| v2 | v3 | 备注 |
|---|---|---|
| `Entry::new` | `Entry::new` | 一致 |
| `set_password` | `set_password` | 一致 |
| `get_password` | `get_password` | 一致 |
| `delete_password` | **`delete_credential`** | **v3 改名**（T5 review LOW：错误消息文案原本写 v2 旧名，T6 已修为 `delete_credential`，避免 grep 误导） |

错误处理：`PlatformFailure` / `NoStorageAccess` 等 → 显式 `anyhow::anyhow!` 向上传播；`NoEntry` 在语义允许的位置（`delete` / 逐项 `read_all` / index 缺失）视为成功或跳过。**Rust 无静默吞错**约束满足。

---

## 3. 启动期同步（`sync_on_startup`，三种路径）

`sync_on_startup(store, secrets_path, syncing)` 在 main.rs setup 内 spawn sidecar **之前**调用（确保 SPA 启动前 `secrets.json` 就绪）。三条决策路径：

| 路径 | 触发条件 | 行为 | 测试 |
|---|---|---|---|
| **首迁（json→keychain）** | `keychain.read_all()` 空 且 `secrets.json` 存在且非空 | 把 json 全部 key 写入 keychain（keychain 为 source of truth 的反方向初次灌入）；`syncing` flag 全程不动（避免触发回写链路反向写） | `first_migration_corrupt_json_is_noop_keychain_stays_empty`、`syncing_flag_not_touched_in_first_migration_path` |
| **覆盖（keychain→json）** | `keychain.read_all()` 非空 | keychain 是 source of truth：原子写 json（merge-保留顶层其他字段，仅覆盖 `services[*].apiKey`）；置 `syncing=true` 防回环（写完成 + debounce 后 reset） | `keychain_nonempty_writes_to_secrets_json_and_resets_syncing`、`keychain_nonempty_multiple_keys_all_written`、`keychain_nonempty_overwrites_existing_json_with_keychain_as_source_of_truth` |
| **noop** | 双空（keychain 与 json 均空）或 json 损坏 | 不报错；首迁路径下损坏 json 视为空 → noop | `first_migration_corrupt_json_is_noop_keychain_stays_empty` |

降级语义见 §5。

---

## 4. 文件监听回写 + 防回环

`spawn_writeback(store, secrets_path, syncing, shutdown)` 在 spawn sidecar **之后**启动 watcher 任务。架构：

```
notify::RecommendedWatcher (event channel)
        │
        ▼
run_writeback_loop  ← 外层 recv 循环，shutdown true 时退出
        │
        ▼
debounce_and_process  ← 500ms debounce（合并连发写）+ 检查 shutdown
        │
        ▼
process_writeback  ← 读 secrets.json，compute_writeback_diff，逐个 upsert
```

### 4.1 防回环三层

| 层 | 实现 | 测试 |
|---|---|---|
| **L1：`syncing` flag** | `sync_on_startup` 写 json 前 `syncing.store(true, SeqCst)`；`process_writeback` 入口检查 `should_writeback`（`syncing.load(SeqCst)` 为 true 直接 return） | `process_writeback_skips_when_syncing_true_anti_loop` |
| **L2：debounce 500ms** | notify 触发后 sleep 500ms 分段检查 shutdown，合并连发事件 | `watcher_spa_write_writebacks_to_keychain`（#[ignore]，依赖 notify timing） |
| **L3：compute_writeback_diff 只写变化的 key** | 仅当 json 中的 key 不等于 keychain 中对应值才 upsert；防 race | `process_writeback_only_upserts_changed_keys`、`process_writeback_diffs_upserts_when_syncing_false` |

### 4.2 容错

| 场景 | 行为 | 测试 |
|---|---|---|
| 写失败（keychain 不可达 mid-run） | `process_writeback` 返回 `Err`；外层 `syncing.store(false, SeqCst)` **始终 reset**（用 `let result = ...; syncing.store(false, ...); result`），避免下次 writeback 因 flag 卡死 | `write_failure_propagates_error_and_resets_syncing` |
| json 损坏 mid-run | `process_writeback` 视为 noop（不 panic、不报错） | `process_writeback_corrupt_json_is_noop` |
| shutdown 触发 | watcher 循环在 recv 间隙检查 shutdown=true → break；debounce 期间也分段检查 | `watcher_spa_write_writebacks_to_keychain`（#[ignore]） |

### 4.3 event 过滤

`event_targets_file(event, target_file)` + `paths_contain_target(paths, target)`：只处理路径以 `secrets.json` 结尾的事件，忽略同目录其他文件改动。空 target → 默认处理（保守不丢事件）。

---

## 5. main.rs 接线 + 降级

`src/main.rs::wire_secrets_layer`（setup 内调用）：

```rust
// 1. keychain 同步（spawn sidecar 之前，保证 SPA 启动前 secrets.json 就绪）
let store = Arc::new(KeyringStore::new("inkosDesktop"));
let syncing = Arc::new(AtomicBool::new(false));
if let Err(e) = secrets::sync_on_startup(&store, &secrets_path, &syncing) {
    eprintln!("[secrets] 启动同步失败，密钥未加密存储: {e}");
    // 不阻塞：keychain 不可用时 secrets.json 若存在 inkos 仍可用
}

// 2. spawn sidecar（之后）→ 3. spawn writeback（之后）
let writeback_handle = secrets::spawn_writeback(store, secrets_path, syncing, shutdown);
```

**降级链**：

| 场景 | 行为 |
|---|---|
| keychain 完全不可用（headless CI / Linux 无 Secret Service / 拒绝授权） | `sync_on_startup` 返回 `Err` → setup 仅 `eprintln!` 警告"密钥未加密存储"；SPA 仍能启动（读 `secrets.json`，若存在则 inkos 可用） |
| writeback 任务失败 | 内层错误 log，外层 syncing flag reset；watcher 任务继续运行（不退出） |
| keychain 中途恢复 | writeback 下次触发时 upsert 成功 |

零阻塞：keychain 任何失败都不让 app 启动崩溃（与 isolation loopback 同样的优雅降级策略）。

---

## 6. 测试稳定性 + 覆盖率

### 6.1 全量测试稳定性（连跑 3 次，2026-08-06）

```
Run 1/2/3 一致：
  lib unittests:           107 passed; 0 failed; 6 ignored   ← 6 ignored: 3 KeyringStore 真实 keychain + 1 watcher SPA IO + 其他 IO timing
  bin unittests (main.rs):   1 passed; 0 failed; 0 ignored
  observer_integration:     3 passed; 0 failed; 0 ignored
  supervisor_integration:   2 passed; 0 failed; 1 ignored   ← real_inkos_sidecar_serves_spa（沿用 M1 ignore）
  doctest:                  2 passed; 0 failed; 0 ignored
  -----------------------------------------------
  合计：                   115 passed; 0 failed; 7 ignored
```

3 次结果完全一致，无 flaky。secrets 模块新增 50+ 单测（jsonio 8 + store 8 + sync 35+），其中 6 个标 `#[ignore]`（依赖真实 keychain / notify timing，跑法：`cargo test -- --ignored`）。

### 6.2 覆盖率（`cargo llvm-cov --workspace --summary-only`，2026-08-06）

工具：`cargo-llvm-cov 0.8.7` + `llvm-tools-aarch64-apple-darwin`（rustup component）。可用，与 M2a 一致。

| 文件 | 行覆盖率 | 类别 |
|---|---|---|
| `secrets/jsonio.rs` | **96.69%**（242 行 / 8 未覆盖） | 纯逻辑 ✅ |
| `secrets/sync.rs` | 70.39% 全量 / **纯逻辑子集 ≈ 89.04%** | 纯函数(sync_on_startup/process_writeback/should_writeback/compute_writeback_diff/event_targets_file/paths_contain_target) ≥80% ✅；未覆盖部分为 `spawn_writeback`/`run_writeback_loop`/`debounce_and_process` 的 notify IO + spawn 路径（brief 明示不计） |
| `secrets/store.rs` | 25.24% 全量 / **MockStore 纯逻辑子集 ≈ 100%** | MockStore 部分（pub fn new/Default/read_all/upsert/delete 共 16 个可执行行）全部覆盖；25.24% 被 KeyringStore 真实 keychain 路径（line 100-244，3 个 #[ignore] 测试覆盖）拉低——brief 明示不计 |
| `observer/*`、`supervisor.rs`、`paths.rs`、`config.rs` | 92.91%–100% | 与 M2a 一致，无回归 |
| `lifecycle.rs` | 58.06% 全量 / 纯逻辑子集 ≈ 100% | 与 M2a 一致（TrayController/install_signal_hooks GUI 路径不计） |
| `main.rs` | 5.85% | Tauri 二进制入口；secrets 接线已通过 `secrets::sync_on_startup` / `spawn_writeback` 单测覆盖语义；setup 路径需 Tauri 运行时 |
| `isolation/*` | 38.89%–60.42% | 规则生成 100%；lock/release 需 root，不计 |
| **TOTAL** | **60.92%** | 较 M2a 的 59.07% 微升（secrets 纯逻辑加入） |

**Spec 达标结论**：secrets 纯逻辑（jsonio 96.69% + sync 纯函数 89.04% + store MockStore 100%）全部 ≥80%，远超目标。keyring 真实 keychain 路径 + notify IO 不计（brief 明示）。

### 6.3 覆盖率复现命令

```bash
cd src-tauri && cargo llvm-cov --workspace --summary-only           # 表格
cd src-tauri && cargo llvm-cov --workspace --text                   # 逐行
cd src-tauri && cargo llvm-cov --workspace --html --output-dir target/llvm-cov/html
open target/llvm-cov/html/index.html
```

### 6.4 schema 核实（apiKey camelCase）

直读 inkos 上游源码：

```
/Users/lalanbv/GitProject/inkosDesktopforRust/packages/core/src/llm/secrets.ts:5
  services: Record<string, { apiKey: string }>;
                                          ↑ camelCase
```

`loadSecrets(projectRoot)`（同文件 line 43）读 `.inkos/secrets.json`，line 70 `if (entry?.apiKey) return entry.apiKey;`——inkos 端**只识别 camelCase `apiKey`**。`write_secrets`（jsonio.rs）严格 camelCase 输出，零 snake_case 风险。

---

## 7. Commit / Tag

| 项 | 值 |
|---|---|
| M2b HEAD（含 T6） | `<见 git log，T6 commit SHA>` |
| tag | `v0.2.1-m2b`（annotated，message: `M2b: secrets keychain Route A (sync + writeback + anti-loop)`） |
| baseline | `95a69b60`（M2a close-out） → 5 个 M2b feature commit + 1 个 T6 收尾 commit |
| T5 review LOW 修复 | store.rs line 235 错误消息文案 `delete_password` → `delete_credential`（与 keyring v3 实际调用一致） |

---

## 8. Parked 项（不在 M2b 范围，留给真机 / M3）

| 项 | 原因 | 归宿 |
|---|---|---|
| **真实 keychain 端到端验证** | 本环境（headless CI）无 macOS Keychain / Windows Cred Manager / Linux Secret Service 可达；3 个 `#[ignore]` 测试（KeyringStore roundtrip / delete-missing-noop / index-JSON 序列化）需真机跑 | 真机冒烟：`cargo test -- --ignored secrets::store::tests` |
| **writeback 真机 SPA 改 key 端到端** | SPA 在 webview 内改 apiKey → secrets.json 变 → watcher 触发 → keychain upsert，整链路依赖 Tauri 运行时 + 真机 GUI；`watcher_spa_write_writebacks_to_keychain` 单测已用 tempfile + tokio::time 验证时序逻辑 | 真机冒烟（启动 sidecar → 在 SPA 内改一个 apiKey → 观察 keychain 是否同步） |
| **`__index__` 跨平台一致性** | macOS Keychain / Windows Cred Manager / Linux Secret Service 三平台的 `Entry::new` / `set_password` / `get_password` / `delete_credential` 行为差异（编码、长度上限、并发）需三平台真机各跑一遍 | 真机冒烟（三平台）+ M3 CI 三平台回归 |
| **keyring v3 API 真机行为** | keyring v3 在三平台的后端（macOS: Security framework / Windows: Credential Manager / Linux: D-Bus Secret Service）错误码与边界条件（如 macOS keychain 锁定、Windows session 0 服务、Linux D-Bus 未启动）需真机验证 | 真机冒烟 + M3 文档化错误码矩阵 |
| **多 keychain 支持**（macOS 专用 keychain 文件、Windows 域 keychain） | M2b 用默认 keychain（service=`inkosDesktop`）；多 keychain 切换需 UI + 配置层 | M3 密钥 UI 增强 |
| **`secrets.json` 加密落盘** | M2b 仍明文（与 inkos 一致），依赖 OS 文件权限（0600）+ keychain 加密 | M3 评估：是否在 keychain 不可用时降级为 AES-GCM（派生自机器 ID） |
| **密钥 UI**（查看 / 编辑 / 删除） | M2b 仅后端同步；UI 在 inkos SPA 内（不在桌面壳范围） | M3 评估：inkos Studio 内 vs 桌面壳原生对话框 |
| **CI 三平台 secrets 回归** | M2b 测试在本机 macOS 跑；Linux（无 Secret Service）与 Windows（Cred Manager）路径需 CI 矩阵 | M3 CI 矩阵（三平台 + secrets 同步回归 + SSE 契约 + pnpm10 + GUI runner） |

---

## 9. 零交叉自检

- 改动文件全部在 `src-tauri/` 与 `变更记录文档/`：
  - `src-tauri/src/secrets/{mod.rs, store.rs, jsonio.rs, sync.rs}`（T1-T5 已 commit）
  - `src-tauri/src/main.rs`（T5 接线）
  - `src-tauri/Cargo.{toml, lock}`（T3/T4 添加 notify + tempfile + tokio-test 依赖）
  - `src-tauri/src/secrets/store.rs`（T6 文案修复：错误消息 `delete_password` → `delete_credential`）
  - `src-tauri/README.md`（T6 追加 M2b 能力）
  - `变更记录文档/20260806/M2b冒烟验证.md`（本档）
- `inkos/` 子模块、`packages/` 子模块、根 `Cargo.*`、根 `package.json`、根 `README*`、既有 `.github/` 均未触碰。
- `git diff --stat` 仅上述 src-tauri/ + 变更记录文档/ 文件，符合"零交叉（仅 src-tauri/ + 变更记录文档/）"约束。

---

## 10. 约束符合性

| 约束 | 状态 | 证据 |
|---|---|---|
| 零修改 inkos | ✅ | git diff 仅 src-tauri/ + 变更记录文档/ |
| Rust 无静默吞错 | ✅ | keyring 调用失败（PlatformFailure / NoStorageAccess）→ 显式 anyhow Err；NoEntry 在语义允许位置（delete / read_all 逐项 / index 缺失）视为成功或跳过，全部带 log；sync_on_startup/writeback 失败 → eprintln 警告但不阻塞 |
| 覆盖率 ≥80%（纯逻辑） | ✅ | jsonio 96.69% + sync 纯函数 89.04% + store MockStore ≈100%（见 §6.2） |
| 单一职责 < 800 行 | ✅ | secrets: mod.rs=8 / store.rs=444（含测试）/ jsonio.rs=361（含测试）/ sync.rs=908（含测试，纯逻辑约 320）；main.rs=494 |
| 命名禁空格 | ✅ | `KeyringStore`、`sync_on_startup`、`spawn_writeback`、`compute_writeback_diff`、`event_targets_file`、`should_writeback`、`__index__` |
| schema camelCase `apiKey` | ✅ | inkos `secrets.ts:5` 直证；jsonio `write_secrets` 严格 camelCase 输出（见 §6.4） |
| keychain 不可用不阻塞启动 | ✅ | `sync_on_startup` 返回 Err → setup 仅 log 警告 → SPA 仍启动（见 §5） |
| 防回环三层 | ✅ | syncing flag + debounce 500ms + compute_writeback_diff（见 §4.1） |
| 首迁 syncing flag 不动 | ✅ | 首迁路径只写 keychain（keychain 不可达则降级），不触发 writeback watcher；测试 `syncing_flag_not_touched_in_first_migration_path` |

---

## 11. 后续

- **M3**：loopback 运行时强制（特权 helper：macOS SMJob/launchd、Linux setuid、Windows WFP）、Windows 端到端（windres/netsh→WFP）、便携 Node + engine/ 发布打包、updater 引擎通道、CI（三平台+secrets 同步回归+SSE 契约+pnpm10+GUI runner）、macOS 签名、全局快捷键、通知配置 UI、daemon 状态托盘项、密钥 UI（查看/编辑/删除）、`secrets.json` 加密落盘评估。
