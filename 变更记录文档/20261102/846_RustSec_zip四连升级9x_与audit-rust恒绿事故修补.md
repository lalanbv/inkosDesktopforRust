# 846 号：RustSec zip 四连升级 9.x + audit:rust 恒绿事故修补

日期：2026-11-02。前置：845 号。本轮由例行巡检快检升级为实质轮。

## 一、巡检快检（维持项）

上游 `fetch --prune` 仍 **0**；serde_json 锁 **1.0.151** 在位（crates.io 最新仍 1.0.154 = 回退线，解除条件 >1.0.154 未满足）；静默窗 0.159/0.054 两次满足。

## 二、发现

### 2.1 RustSec：zip 2.6.1 四条新 advisory（含 9.1 critical）

直连刷新 advisory-db 后手跑 `cargo audit`，engine-rs 与 src-tauri **各 4 条**，全部落在直接依赖 `zip = "2"`（2.6.1）：

| ID | 严重度 | 面 |
|---|---|---|
| RUSTSEC-2026-0369 | **9.1 critical** | `extract()` 经符号链接施加 Unix 权限、目标不受限 |
| RUSTSEC-2026-0429 | 7.5 high | `extract()` 遇超大声明尺寸的 symlink 条目 abort 进程 |
| RUSTSEC-2026-0367 | 7.3 high | 解压保留 SUID/SGID 位 |
| RUSTSEC-2026-0368 | — | `extract()` 创建目标在目的目录外的符号链接 |

解法统一 `>=9.0.0`。**可达性定性**：四条全在 `ZipArchive::extract()` / `ZipStreamReader::extract()` 高层 API；本仓 engine-rs 只用 `ZipWriter` 写 EPUB + `by_name` 手动读 EPUB，src-tauri 仅 `cfg(windows)` 下对 SHA256 预校验的 Node 发行包做**手工加固解压**（`enclosed_name` 防 zip-slip + 字节上限防炸弹，不走 `extract()`）——实际可达面为零。但直接依赖且有修复版，不走 ignore，升级。

### 2.2 audit:rust 门禁恒绿事故（更深一层）

最初假设「DB 经代理拉取失败→陈旧 DB 假绿」。逐流实测推翻：shell 内**根本没有代理变量**，GitHub 直连通；真正根因是 **cargo-audit 0.22 把汇总行 `error: N vulnerabilities found!` 打在 stderr**（stdout 只有条目细节），而旧脚本 `sh -c "cargo audit 2>/dev/null"` 整段丢弃 stderr → `vulnMatch` 永远匹配不到 → **任何情况下都「✓ 0 漏洞」exit 0**（DB 拉取失败时 cargo-audit 不扫描直接 exit 1，也被 catch 后解析空 stdout 吞成绿）。570 号接入 testgate 以来的「RustSec 双 0」自 cargo-audit 0.22 起即为常量回声。

## 三、实施

- **zip 2→9.2.2 双 crate**：`default-features = false, features = ["deflate"]`——EPUB 规范仅允许 Stored/Deflate、Node 官方 .zip 仅 Deflate；裁掉 aes/bzip2/lzma/xz/zstd/deflate64 族，engine-rs 锁 -20 crate、src-tauri 锁 -11 crate（src-tauri 原本与 tauri-plugin-updater 各持 2.6.1/9.2.1 两份 zip，现统一一份）。不启 `time` → `SimpleFileOptions::default()` 时间戳恒 `DateTime::default()`（1980-01-01），**scratch crate 实证间隔 1.5s 两次导出字节恒等**。
- **API 断裂仅 1 处**：`FileOptions<'_, ()>` 在 9.x 多一个生命周期参数 → `export.rs` 改跨版本稳定别名 `SimpleFileOptions`。
- **Windows 专属代码验证**：`extract_zip` 在 mac 上不编译，不靠"大概兼容"——函数体逐字复制到 /tmp scratch crate 对 9.2.2 编译通过，并构造含 `../evil.txt` 条目的归档验证 zip-slip 防护行为仍成立。
- **audit-rust.mjs 重写**：stdout/stderr 分开捕获，汇总行从 stderr 解析；**非零退出永不放行**（无漏洞汇总的非零退出按"非预期失败"报 stderr 尾）；DB 拉取失败（stderr `couldn't fetch advisory database`）默认硬失败 exit 1，`--allow-stale-db` 显式逃生（改 `-n` 本地 DB 扫描并标注窗口）；输出 `advisory-db 最新提交 N 天前` 作新鲜度证据；条目摘取按空行分块剔除 `Warning:` 块；yank 超时噪声折叠计数。**四场景实证**：旧漏洞锁 → ✗ 4 个漏洞（此前永远 0）；坏代理 → ✗ exit 1；坏代理+逃生门 → ⚠ 本地扫描 exit 0 且警告计数正确；默认环境 → 双 ✓ 0 漏洞 exit 0。
- 自我纠正一处：重写时顺手加了 `--no-yanked`，属政策变更非 bug 修复，撤回保留 yank 信号。

## 四、门禁

clippy:gate 双 0（首跑一次瞬态红=`--all-targets` dev-dep 拉取撞上宕机的 ustc 镜像，复跑两次稳定绿）；cargo:testgate 审计步**双 crate 0 漏洞（DB 0 天、警告 2/9 条可见）** + engine-rs 1982 + src-tauri 584；gate:ts 八步全绿（rust-bin 以 zip 9 增量重建、export-epub-smoke、engine-contract-diff 导出工件面恒等）；静默窗 bench:gate 通过无回退。

## 五、环境备忘

- **ustc crates 镜像当日宕机**（直连 000/超时，与代理无关）：一次性 `--config 'source.ustc.registry="sparse+https://rsproxy.cn/index/"'` 把源名 ustc 临时指向 rsproxy（0.21s），不改用户 `~/.cargo/config.toml`；同名源改指 index.crates.io 会被 cargo 判为与 crates-io 冲突。ustc 宕机也是 yank 核验 300+/700+ 次超时的来源（审计步变慢但不失败）。
- `cargo update -p zip` 在 src-tauri 需 `zip@2.6.1` 精确 spec（与 updater 的 9.2.1 并存时歧义）。

## 六、教训

1. **门禁解析器要对着被解析工具的当前版本实测流向**：cargo-audit 升到 0.22 后汇总行换到 stderr，丢 stderr 的解析器悄然退化为常量函数——「恒绿」与「真绿」在日志上长得一模一样，只能靠**故意喂一个已知红的输入**（旧漏洞锁）来区分。门禁脚本应配一个"必红用例"。
2. **假设要逐层证伪**：「代理拦 DB」是顺手的解释，逐流落盘一看就不成立；归档写错根因会让下一个人修错地方。
3. **非零退出永不放行**是解析型门禁的底线——解析失败时退到退出码而不是退到"0"。
4. 可达面为零 ≠ 不升级：直接依赖、有修复版、断裂面 1 处，升级成本低于长期维护 ignore 清单。

## 七、出口

维持：上游数值分协议扩展 / serde_json >1.0.154 解除 / @streamdown/math katex 0.18 / pi1.0 key / 新真实需求。**用户侧待办：Fork 推送本地累积 46+ 提交。**
