# 949 号：backup 导出审查修复轮——TS 排序缺陷清偿 + 双端隔离/目录过滤对齐

> 前置：948 号（backup/export 前 per-work source_sync 双端）。本轮=goal 驱动的全量审查轮：审查 948 新增逻辑→发现缺陷→修复→补回归用例→全门禁验收。

## 审查结论（948 语义正确性确认面）

- **sync 调用语义正确**：`writes=[]` + `accept=false` 并非死代码——`sync_work_source_artifacts` 仍执行真实补齐：扫盘 `source/` → 缺失 artifact 补登 candidate 修订 → current 修订缺 `snapshotPath` 回填快照 → 总是重写 work.json（双端镜像）。accept=false 保证不翻转 accept 面（currentRevisionId 不变、状态只 candidate）。
- **上下文外安全**：TS `syncWorkSourceArtifacts` 尾部的 `recordExecutionEvidence`/`updateExecutionWork` 在无执行上下文时为 no-op（AsyncLocalStorage getStore 可选链）——studio 备份路径调用安全。
- **淘汰检测双端恒等**：Rust `accept_paths_excludes` unwrap_or(false) 与 TS `(acceptPaths && !includes)` 真值表逐分支等价。
- **并发面备案（非本轮患）**：source_sync 无 per-work 锁，backup 时点 sync 与活跃管线写同一 work 存在 last-writer-wins 窗口——但两写均原子（staging+rename），快照文件只增不删，丢失的 manifest 条目由下次 sync 自源目录幂等重建：有界自愈，与既有并发 episode 竞态同族。备份为冷路径用户显式动作，窗口极窄。

## 发现的缺陷（948 引入/暴露）与修复

1. **TS 排序缺陷（严重，948 目的落空）**：TS export 处理器中 staging 复制（含 works/ 整目录）在 sync **之前**执行——sync 补齐的 revisions 快照与重写的 work.json 全部不进备份包，948 的「备份完整性窗口」在 TS 端完全没有闭合。Rust 端顺序正确（sync 先于 `spawn_blocking(run_export)`）。**修复**：sync 块前移至 staging rm/mkdir 之前。新回归用例直接逮住此缺陷（首跑失败：磁盘断言过、归档断言无 revisions）——测试先于宣告的价值实证。
2. **TS fail-fast 循环（948 形态缺陷）**：`readdir` 返回全部条目（含文件），单条目 sync 抛错（如 Finder `.DS_Store`、手工坏 work.json）即中止整循环且 `catch {}` 零日志静默吞错——后续 work 全部跳过补齐且不可见（静态吞错违背审计教训：静默失败=不可见失败）。**修复**：逐 work try/catch + `console.warn`（带前缀惯例），循环继续；双端隔离语义对齐。
3. **非目录条目噪声（双端）**：works/ 内杂散文件（`.DS_Store` 等）被当 work 处理：TS 抛错触发缺陷 2；Rust 每次备份产生 validate 失败 warn 噪声。**修复**：双端只遍历目录条目（TS `withFileTypes`+`isDirectory`，Rust `file_type().is_dir()`；跳过不告警，与 `list_archive_files` 的 `.DS_Store` 跳过同族先例）。
4. **Rust 循环零测试覆盖（948 遗留）**：既有 Rust 测试直接调 `run_export`（不含 sync 循环）。**修复**：提取 `sync_works_for_backup(root)` 供处理器与测试共用（处理器行为不变），补 `backup_sync_covers_manual_works_and_isolates_failures` 单元用例。
5. **既有种子 manifest 非法（测试债）**：双端测试种子的 work.json 缺 schema/serde 必填面（version/lineage/metadata/时间戳）——948 sync 在种子上从未成功过（948 提交信息「works 种子断言=sync 集成实证」实际不成立：条目入包靠整体复制非 sync；TS 端 sync 抛错被外层 catch 吞掉）。**修复**：双端种子 manifest 合法化 + 建 `works/w1/source/` 目录——既有全部备份用例此后真实走 sync 成功路径。

## 回归用例（双端同形）

- TS `backup-routes.test.ts` 新用例「export: 打包前 per-work sync 快照补齐」：手工放置 source 文件（manifest 未注册）→ export → 断言 work.json 补登 candidate 修订 + currentRevisionId 保持 null（accept 面不翻转）+ 快照落盘 + **归档含 `inkos-backup/works/w1/revisions/`**（排序修复的直接证据）+ `.DS_Store` 与坏 work 目录不阻断（200 响应）。
- Rust `backup_routes.rs` 新用例 `backup_sync_covers_manual_works_and_isolates_failures`：同形四断言（补登/accept 面/快照落盘）+ 杂散文件与坏 work 隔离。

## 验证

- TS backup-routes 7/7；Rust backup_routes 10/10
- testgate：engine-rs 2057（+1）/ src-tauri 584 全绿（含 audit 必红自检活体）
- typecheck 净；gate:ts 十步全绿（build/typecheck/test/audit:npm/rust-bin/node-fallback-smoke/engine-contract-diff/export-epub-smoke/work-alternation/check-routes）
- clippy:gate 双 crate 0 告警；audit:rust 双 crate 0 漏洞（unmaintained/yanked 警告维持既有）
- src-tauri 无 backup/export 面（仅 migration.rs 域外快照语义）——备份域确为双栈，修复面完备
- U+FFFD 扫描净

## 净结论

948 的 sync 语义正确但 TS 实现存在排序缺陷（补齐产物不进包=目的落空）+ fail-fast 静默吞错；本轮三缺陷全清偿 + 双端隔离/目录过滤对齐 + 循环首次获得双端测试覆盖。备份完整性双时点双保险从「声称」落到「归档实证」。

**下一号自 950 起；出口清空回归常备；用户侧待办=Fork 推送（含 948 与本轮提交）**
