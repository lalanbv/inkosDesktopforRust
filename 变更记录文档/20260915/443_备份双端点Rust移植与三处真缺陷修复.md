# 443 号：R18 备份双端点 Rust 移植（386 预授权翻案）+ 三处真缺陷修复 + 备份/写作数据卡真机走查

- **日期**：2026-09-15
- **类型**：feat(engine) + fix(studio) —— 死 UI 面激活移植 + 端到端断裂修复
- **关联**：386 号（R18 备份，TS-only 备案）、164 号（Rust 默认引擎）、382/411 号（R13 写作数据）
- **提交**：engine-rs（backup_routes.rs 新模块 + mod.rs 注册 + Cargo.toml tar 依赖）+ BackupPanel/WritingStatsCard + 本记录

## 1. 选题与翻案理由

443 号按内部盘点选 R18 BackupPanel 真机走查，读码即发现：备份端点只存在于 TS studio server（server.ts:6764/6813），Rust 引擎无注册——**桌面默认引擎（164 号）下备份卡整体死亡**（导出/导入均 404）。386 备忘 §4 曾预授权「备份为纯本地 UI 面，仅 TS 承担」，该决策前提（TS 服桌面）已随 164 号切换失效——443 号推翻预授权并移植。

## 2. 移植实现（`server/backup_routes.rs`，线格式对齐 TS）

- **导出** `GET /api/v1/backup/export[?includeSecrets=1]`：staging 聚合 `books/`+`inkos.json`+`.inkos/`（secrets 缺省排除）+`prompt/` → `backup-manifest.json`（version/generator/scope/fileCount/totalBytes）→ `inkos-backup/` 前缀 tar.gz（新依赖 `tar = "0.4"` + 既有 flate2）→ attachment 下载。
- **导入** `POST /api/v1/backup/import[?confirm=1]`：gzip 魔数自探 → 安全三则（`..`/绝对路径拒绝、仅普通文件、512MB 解压上限；不安全条目整包 400 逐字 `unsafe entries rejected: ...`）→ manifest 先验 → 预览 `{preview:{fileCount,overwriting,newFiles}}` → 确认写回+覆盖项快照至 `backups/pre-restore-<ms>.tar`。
- 导入路由 `DefaultBodyLimit` 放宽至 512MB（axum 缺省 2MB 不够真实项目包）。

## 3. 过程中坐实的 TS 侧三处「产后搁浅」断裂（R18 从未端到端可用的实证）

1. **manifest 查找永不命中**：TS 导出以 `inkos-backup/` 前缀打包，导入按裸名 `backup-manifest.json` 精确查找——自家导出包导入必 400。Rust 按「能工作的契约」：前缀名优先+裸名兜底。
2. **写回落点错位**：TS 按归档名原样写回（`root/inkos-backup/books/...`）——恢复永远落不进真实书目。Rust 剥前缀写回项目根；manifest 元数据不落盘，`restored` 计实际写回数。
3. **面板预览线格式错配**：服务端返回 `{preview:{...}}` 嵌套，BackupPanel 按平铺读 `fileCount`——预览数字全 undefined。面板改为解包 `result.preview`。

（1/3 的 TS 侧同语义修复留待 TS 回退引擎需要时跟进；桌面默认路径已由 Rust 承担。）

## 4. 真机走查（Rust 引擎 + mock 走查根）

- 面板渲染：备份与恢复卡+双按钮（导入为内嵌文件输入 label）✓（截图）
- 导出：HTTP 200 application/gzip 14704B；44 文件含 manifest/book.json/runtime 工件；缺省 secrets=0、`includeSecrets=1`=1；manifest createdAt 格式修复后 `2026-09-14T18:37:07.835Z` ✓
- 两段式：改动版包（改 1 文件+新增 1 文件重打包）→ 预览 `fileCount:46 / overwriting:44（剥前缀真实路径）/ newFiles:2` → 确认 `{ok:true, restored:45, snapshot:true}` ✓
- **快照回滚保险**：`backups/pre-restore-*.tar` 含全部原版文件，抽验 `current_state.md` 为原内容；现役文件已含恢复标记 ✓
- IAB 自动化限制备案：文件选择器不可程序化（Codex/IAB 对齐），导入 UI 侧由 API 全链验证；`window.open` 导出在新标签自动化下无可观察产物（外壳行为），端点以 curl 字节级验证。

## 5. 附带：R13 写作数据卡走查 + 柱状图渲染缺陷修复

走查截图暴露 WritingStatsCard 缺陷：`daily` 契约为**稀疏桶**（writing-stats.ts 头注「无产出的日期不出现」），卡片却按 30 密集槽渲染——单日数据时单根 `flex-1` 柱撑满全宽（DOM 实测 1 根 766px×60px 实心块）。修复：卡片自补 30 个固定日槽（UTC 对齐聚合窗口），零产出日 2px 基线柱。复验：30 槽/29 基线+今日 60px 柱（截图）✓。

## 6. 门禁

- 备份模块 7 单测（roundtrip/secrets 开关/两段式/manifest 四态/不安全整包拒绝/gzip 层）全绿
- cargo test 39 套件 1797 用例全绿；INKOS_DUEL=1 duel 真跑 10 测试 41.8s 全绿；clippy:gate 双 crate 0 告警（`repeat().take()` lint 修正）；bindings 186 不变
- studio：vitest 97 文件 827 用例全绿（首跑单例 action.test 抖动，单文件+全量复跑均绿）+ tsc 净 + dist 重建
- **引擎静态面备案**：引擎内存持有启动时 dist/index.html——重建 dist 后须重启引擎，否则旧 hash 引用 404 白屏（走查中实测，非本批缺陷）

## 7. 遗留

- **待推送：433–443 共 11 笔**，请用户在 Fork 图形端推送后核验 origin/develop。
- 四轮 P0 面板真机走查：R21 ✓、R20 ✓、R13 ✓（本号附带）、R18 ✓（本号）；Dashboard 其余卡与次要面板留内部盘点。
- 下一个编号自 **444** 起。
