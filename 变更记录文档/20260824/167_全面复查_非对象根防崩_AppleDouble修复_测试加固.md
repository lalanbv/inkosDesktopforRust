# 167 · 全面复查：非对象根防崩 + AppleDouble 修复 + 测试加固

- **日期**：2026-08-24
- **性质**：代码审查驱动的缺陷修复 + 测试加固（无功能面变更）
- **范围**：engine-rs（project 配置域写侧）、src-tauri（updater Rust 通道）、studio（ChatPage）、scripts（打包脚本）、engine-rs 测试
- **触发**：用户目标「检查代码确保没有 BUG，逻辑正确严谨最优解，测试全量通过，可正常使用」

## 一、审查结论（164~166 号全部改动面）

| 审查项 | 结论 |
|---|---|
| updater/engine.rs BundleFlavor 三分流 + 回滚契约 | ✅ 正确（展平/预检/验签均有测试锚定） |
| main.rs updater 接线（active_flavor/版本源/目录组） | ✅ 正确（启动前默认 rust 已注释契约） |
| rustbin.rs 三级解析 + env 契约 | ✅ 正确 |
| 165 get_project 读侧三态 | ✅ 正确（8 测锚定） |
| ChapterReader 分屏 / usePanelWidth | ✅ 正确（flex-1 min-w-0 + shrink-0 + 贴右缘取宽） |
| 全仓 `as_object_mut().unwrap()` 分级排查 | 其余 14 处均安全（本地构造 `json!` / 测试代码 / 已有 is_object 前置守卫，如 book_session_store.rs:317） |

## 二、修复的缺陷

### 1. engine-rs：非对象根 inkos.json 使全部 PUT/POST **panic**（12 处）

`load_raw_config` 返回任意合法 JSON；根为数组/数字/字符串（用户手改即得）时，
`raw.as_object_mut().unwrap()` panic 打断 hyper 连接任务。Node 对应语义为
strict-mode 对原始值赋属性 TypeError → 500。

**修复**：12 处改 `let Some(obj) = raw.as_object_mut() else { return …500 }`，
错误形状沿用各 handler 既有面（internal_error / flat_internal）。
**回归**：`non_object_root_puts_return_500_not_panic`（8 端点循环）+
`non_object_root_post_language_returns_500_not_panic`；**真实二进制冒烟**：
`[1,2]` 根 PUT/POST → 500 + 服务存活（health 200）。

### 2. src-tauri：macOS AppleDouble（`._*`）污染使 Rust 引擎更新**永远装不上**（真 BUG，新管线契约测试挖出）

bsdtar 打包带 xattr 的源文件（cargo 产物自带 `com.apple.provenance`，实测
`engine-rs/target/release/inkos-engine-server` 就有）会向 tarball 写 `._*`
条目 → 桌端解压后顶层条目数 ≠1 → `flatten_single_top_dir` 静默不展平 →
`bundle_health` 拒绝 → 回滚。**生产 tarball 必然复现**（打包机为 cargo 环境）。

**修复（双层）**：
- 源头：`scripts/package-rust-engine.sh` tar 前缀 `COPYFILE_DISABLE=1`；
- 解压端兜底：`flatten_single_top_dir` 展平前 `strip_appledouble`（顶层 + 内层），
  第三方/旧包免疫。
**回归**：`rust_bundle_real_tar_pipeline_contract`（系统 tar 真实管线：打包带
xattr 源 → extract_archive → 展平 → Rust 健康预检 + 可执行位断言，修复前
FAILED）+ `flatten_strips_appledouble_junk`。附带确认 exec 位保留已有锚定
（node/tests.rs `extract_archive_preserves_layout_and_exec_bit`）。

### 3. studio：ChatPage 打字机聚焦**跨会话残留**

切会话时 `activeMessage` 索引无意义残留，新会话消息被无端降暗。
**修复**：`useEffect` 依赖 `activeSessionId` 复位为 null。

### 4. engine-rs 测试：`radar_scan_and_history_full_chain` **日期脆弱断言**

硬编码 `starts_with("scan-2026-08-1")`（写于 8 月 1X 日），8 月 20 日后必挂。
**修复**：日期无关断言（`scan-` 前缀 + ≠预置历史名；新扫描身份已由
marketSummary 断言锚定）。

## 三、验证矩阵（全绿）

| 套件 | 结果 |
|---|---|
| src-tauri `cargo test` | 23 套件全过 0 失败（lib 466 含新增 2 测） |
| src-tauri `cargo clippy --all-targets -D warnings` | 零警告 |
| engine-rs `cargo test` | 1210 + 194 + 70 + 8 全过（含新增 3 测） |
| engine-rs `cargo clippy --all-targets -D warnings` | 零警告 |
| studio `tsc --noEmit` | 零错 |
| studio `vitest run` | 720/720 |
| studio `playwright test` | 36/36 |
| 真实二进制冒烟（非对象根） | PUT/POST 500 + 存活 |
| ops72 连跑 5 次 | 无抖动 |

## 四、遗留与说明

- AppleDouble 修复只影响更新链路；已装好的 engine-rust 目录不受影响（无 `._` 清理需求，打包副本走 resource 直拷）。
- Windows `.exe` 命名（`inkos-engine-server.exe`）与 rust_triple 映射已存在，但 Windows 打包/冒烟仍为用户侧设备依赖项（与 166 号遗留一致）。
