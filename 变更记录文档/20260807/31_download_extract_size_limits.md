# 安全：下载与解压全链路体积上限（防空盘 + gz bomb）

> 日期：2026-08-07
> 范围：`src/updater/engine.rs`、`src/engine/node/mod.rs`、`src/engine/node/tests.rs`
> commit：`89ed8bea`

## 三处无界写入

### 1. `updater::download_to`

两个缺陷叠加：

- `max_bytes == 0`（`.sha256` / `.sig` 大小未知）时**完全无上限**——本意是
  「大小未知」，实际等于「无限制」。这两个是数十至数百字节的小文本文件，
  无界写入没有正当理由。
- 仅有 **Content-Length 预检**。该头可缺失（chunked transfer）或撒谎；
  服务器不发即绕过全部检查。

修复：`limit` 恒 `> 0`（已知 → `max_bytes * 2`，未知 → `SMALL_ASSET_MAX_BYTES = 1 MiB`），
预检 + **流式累计**两道。

### 2. `node::BootstrappingResolver::download`

Node tarball 下载**零**上限检查（连 Content-Length 预检都没有）。被劫持或
恶意 mirror 可无限写入撑满磁盘。加预检 + 累计（`MAX_TARBALL_BYTES`）。

### 3. `extract_archive`

`archive.unpack(dest)` 无法在中途按累计体积中止——gz bomb（30MB 压缩包
膨胀成数百 GB）会一路写满磁盘。改为逐条目 `unpack_in` + 累计校验
（`MAX_EXTRACTED_BYTES = 512 MiB`）；zip 路径（Windows）同样处理。

## 为何 extract_archive 必须有上限：两条路径的信任模型不同

| 路径 | 哈希来源 | 强度 |
|------|---------|------|
| Node bootstrap | **独立**官方 nodejs.org SHASUMS，与 mirror 分离 | 强——需先攻破 nodejs.org |
| updater bundle | `.sha256` 与 bundle **同源**于一个 GitHub release | 弱——只防传输损坏 |

updater 侧：攻击者若能改 release（泄露 token、被入侵的 CI），可同时替换
bundle 与 `.sha256`。真正的来源认证是 Ed25519 签名，而它在
`SigAction::WarnPass`（公钥未配置的过渡期）下**被跳过**。

此时未验签的 bundle 直接进 `extract_archive` → 无来源认证，必须靠解压期
上限兜底。plugin 侧的 `safe_extract_tar_gz` 早已有完整防护
（`MAX_EXTRACTED_BYTES` + `MAX_TAR_ENTRIES` + symlink 拒绝），
`extract_archive` 此前完全没有——同一代码库内两套解压逻辑防护不对等。

## 测试 +2

- **`extract_archive_preserves_layout_and_exec_bit`**：`unpack()` → 逐条目
  `unpack_in` 是行为替换，必须验证等价。`bin/node` 丢掉可执行位会让
  bootstrap **静默**产出不可用的 node（`is_file()` 仍为真）。断言顶层目录
  结构保留 + `mode & 0o111 == 0o111`。
- **`extract_archive_rejects_oversized_payload`**：单条目声明 600 MiB > 512 MiB
  上限被拒。

### 构造踩坑

首版用两个各声明 400 MiB 的条目（累计 800 MiB）。tar-rs 读第二条目前需
先跳过第一条声明的 400 MiB 数据，内容为空 → `unexpected EOF during skip`,
在体积检查生效**前**就失败，测试断言的是错误的失败原因。

改单条目：首次累计检查即中止，不触发跳过逻辑。

## 验证

- `cargo test`：**537 passed / 0 failed**（427 lib + 110 集成/bin）
- `cargo clippy --all-targets -- -D warnings`：零警告
