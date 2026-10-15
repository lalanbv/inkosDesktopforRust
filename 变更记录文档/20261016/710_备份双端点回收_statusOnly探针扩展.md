# 710 号：备份双端点回收（第八批，回收清单 25→23；BackupPanel 活消费断链修复）

> 日期：2026-10-16（707-709 批量回收模式续号）
> 范围：packages/studio（server.ts 两路由 + 行为测试 4 测）/ scripts（contract-diff 域表 + statusOnly 探针扩展）

## 一、批次构成（BackupPanel 活消费，Rust 443 号移植的对偶回归）

`BackupPanel` 在位消费：export=浏览器 `window.open` 下载（GET），import=`fetchJson` POST 两段式（preview → `?confirm=1`）。Rust `backup_routes.rs` 为 443 号对 fork 的逐字移植（444 号「能工作的契约」：前缀剥离+manifest 双查找）——**TS 侧嫁接 fork 原实现（即 444 修正版）即与 Rust 线格式对齐**，无真分歧需修。

1. **GET /api/v1/backup/export**：staging 聚合 `books/`+`inkos.json`+`.inkos/`（secrets 默认排除，`includeSecrets=1` 显式含）+`prompt/` → manifest（version/generator/scope/fileCount/totalBytes）→ `inkos-backup/` 包根前缀 tar.gz 下载；sessions/ 默认排除（scope 仅记录）。staging 导出后清理（测试锁定无残留）。
2. **POST /api/v1/backup/import[?confirm=1]**：untar 安全三则（`..`/绝对路径拒绝、仅普通文件、512MB 解压上限）→ manifest 先验（version=1+generator 前缀 `inkos-desktop`）→ preview 盘点（overwriting/newFiles）→ confirm 写回+`pre-restore-<ms>.tar` 快照（回滚保险=确认时刻现网内容）。manifest 是元数据不落盘；restored 计实际写回数。

工具链零新移植：`buildTarArchive`/`listArchiveFiles`（server.ts 348-363，2.0 合并存活）、`lib/tar-read.ts`（untar entries/rejected 安全拒绝面）、`gzipSync` import 全在树；仅补 `cp`（node:fs/promises 递归复制）。

## 二、探针机制第三次扩展：statusOnly 标记

SHARED_FACE 条目新增 `statusOnly: true`——**二进制容器面**（gzip/tar）双腿压缩器不同（Node gzipSync vs Rust flate2），字节级 body 对比不可行；与严格模式 `STATUS_ONLY_ENDPOINTS`（686 号 film 便携包同口径）对偶，status-only 锁「路由在位+状态对称」。探针形态族谱：703 方法感知（写面路由存在性证明）→ 705 body 快乐路径 → 710 statusOnly（二进制容器）。import 空载荷探针=无 manifest 守卫（400/400），不传真实归档。

## 三、门禁终态

| 门禁 | 结果 |
| --- | --- |
| studio vitest | 668/668（+4：export 聚合/manifest/前缀/secrets 排除+staging 清理；includeSecrets；两段式 roundtrip（预览不动现网→篡改→confirm 写回+快照含确认时刻内容）；守卫面 400 族+`..` 整包拒绝无一落盘） |
| gate:ts 八步 | 全绿 exit 0 |
| contract-diff 独立对跑 | 共享面 44 对比 / 0 分歧（新入 export 200/200 statusOnly、import 400/400）；fork-only 23/23 |
| rust-bin | 与 engine-rs 源码同源（本轮零 Rust 改动） |

## 四、教训

1. **roundtrip 断言要对准契约语义**——pre-restore 快照=「确认时刻现网内容」（回滚保险），不是导出原件（原件在 tar 包里）；第一版断言写反，roundtrip 测试的价值正在于逼出语义误解。
2. **二进制容器面的契约验证是状态+结构双轨**——HTTP 层 statusOnly（探针），载荷层 untar 结构化断言（行为测试）；latin1 字符串包含断言对 UTF-8 中文必假阴。
3. **TS 5.7 泛型 Uint8Array 与 BodyInit**——`Uint8Array<ArrayBufferLike>`（Buffer 衍生）不再赋值 BodyInit，测试工具函数返回类型钉 `Uint8Array<ArrayBuffer>`。
4. **Rust 移植面回收即回归验证**——443/444 号 Rust 已按 fork 移植并带 6 测；TS 侧嫁接 fork 444 修正版后，contract-diff 活体对跑即完成「移植保真」的双端互证。
