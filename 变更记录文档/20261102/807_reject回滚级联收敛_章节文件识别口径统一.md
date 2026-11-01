# 807 号：reject 回滚级联收敛——章节文件识别口径统一（legacy 连字符章随回滚删除）

日期：2026-11-02。前置：797/803 登记（共享面尾批 /chapters/1/reject node=404 vs rust=200；node 章节清空回基线 lastApplied 0 而 rust 保留）。

## 一、取证（--keep 现场 + 日志时间线）

- 主表 reject(1) 双腿 200（第 59 行）；尾批 plan→compose→approve(1)→**reject(1) node 404 vs rust 200**。
- TS/rust `approve` 均为无存在性检查的 no-op 200（index.map 不命中照样 ok）→「approve 200 后 reject 404」不矛盾。
- 双侧 `compose` 同形：只产 intent/context 治理工件，**不落章**（TS `createGovernedArtifacts` / rust `run_compose` 返回 ComposeChapterResult 三路径）。
- rust 终态残留 `chapters/0001-镜中醒来.md`（**连字符**，mtime=夹具播种时刻），index.json 条目 createdAt=该文件 mtime、status 默认 ready-for-review、wordCount 由文件算出——这是 `rebuild_chapter_index_from_files` 的签名，不是任何写链新写。
- rust writer 命名为 `{:04}_{title}.md`（下划线）；夹具（683/698/700 号）**故意**为 rust 腿播种 legacy 连字符形态作读侧容错探针，node 腿播种正典下划线。

## 二、根因：同文件两正则口径不一

| 站点 | 正则 | 认连字符? |
|---|---|---|
| rust `chapter_file_re`（rollback 删章 `leading_number` / `get_persisted_chapter_count`） | `^(\d+)_.*\.md$` | 否 |
| rust `rebuild_file_re`（空/损坏索引重建） | `^(\d+)[_-]?(.*?)\.md$` | 是 |
| TS rollback `collectNumberedDeletes` chapters/drafts | `/^(\d+)_.*\.md$/` | 否 |
| TS rebuild（manager.ts:495） | `/^(\d+)[_-]?(.*?)\.md$/` | 是 |

链：主表 reject(1) → `rollback_to_chapter(0)` 删掉写链自产的 `0001_/0002_/0003_`，**连字符旧章逃过** → `save_chapter_index([])` → 下次 `load_chapter_index` 见空索引走宽容重建 → 旧章被捞回成 chapter 1（复活）→ 之后 eval 有章可算（/avgParagraphWarnings 差）、detect/1 有章可查（200 vs 404）、尾批 reject(1) 有章可拒（200 vs 404）。**三条分歧同根**。node 腿无连字符文件故干净。

（顺带：旧章复活使 rust 把「连字符旧章+下划线新章」都计章，夹具「章节≥2」稳定轮询被提前满足，02:37:31 的 write-next 落在尾批期间——同根的时序副作用。）

## 三、修复：读侧认什么为章，删侧与计数侧必须认同一集合

- rust `engine-rs/src/state/manager.rs`：`leading_number` 与 `get_persisted_chapter_count` 改用 `rebuild_file_re`（`NNNN_` / `NNNN-` / `NNNN.md` 同列），删除 `chapter_file_re`；rollback 注释更新。
- TS `packages/core/src/state/manager.ts`：`rollbackToChapter` 的 chapters/drafts 删除改为 `numberedMarkdown`（与 rebuild 同正则）。TS 迁移不正典化章节文件名，works 树可含连字符旧章，同病同修。
- 测试：rust `rollback_to_chapter_deletes_legacy_hyphen_chapter_files`（三命名入索引→rollback 全删→索引空且不再重建捞回）+ `persisted_chapter_count_includes_legacy_hyphen_files_deduped_by_number`（同章双形态计 1）；TS `rollback-legacy-chapter-files.test.ts` 同场景。**双侧反向验证**：恢复严格正则 → 连字符/裸章号文件幸存 → 测试失败；修复后通过。

## 四、活体

diff 报告模式 **71 对比 / 2 分歧**（806 时代 71/5）：eval ✓、detect/1 ✓（双 404）、尾批 reject(1) ✓（双 404）三条同根收敛；余 quality-trend/currentChapter、roster-candidates（695 移植族）。双腿终态完全对称：chapters 仅 index.json=[]、snapshots 0、state 四件、manifest lastApplied 0。

## 五、验证

cargo:testgate engine-rs 47 目标 **1970**（806 为 1968，+2）+ src-tauri 25 目标 584 + RustSec 双 0；clippy 双 0；gate:ts 八步全绿（含 engine-contract-diff + legacy 11/11）；权威 pnpm -r test core **178/983**（+1 文件 +1 测）+ studio 134/717 + cli 23/81。

## 六、登记（695 工项余项）

- TS 仍有三站严格口径：`getPersistedChapterCount`、`saveChapterIndex` 空索引守卫、durable 进度扫描 `^\d+_`（698/700 号播种设计明确「TS 扫描不认连字符」）——与本轮 rollback 宽容化并存无害（多删不少删；守卫更保守），但计数语义仍 TS 1 vs rust 2（夹具以「互斥播种」规避），待独立定口径。
- rust `rebuild_chapter_index_from_files_at` 不按章号去重（同章双形态会产两条目）；TS 重建同形待核。
- quality-trend / roster / mock 撤回支 / UI superseded 徽标维持登记。

## 七、教训

1. **同文件两正则=两个"什么算章"的定义**：读侧宽容、删侧严格 → 删不掉的东西会被读回来。枚举同一实体的所有识别站点，统一到一个谓词（804 原子写"同文件双轨=漏网信号"方法论的又一实例）。
2. **复活类 bug 的取证锚点是 mtime + 重建签名**（createdAt=文件 mtime、默认 status、由文件算出的 wordCount）——一眼区分"重建捞回"与"新写"。
3. **夹具的故意非规范播种是产品级容错契约**（683 号）：它要求引擎在**所有**路径上把 legacy 形态当作一等章节，不只是读路径。
4. 一条分歧修掉三条：先问"这几条分歧是否共享同一前置状态"，再逐条分析。
