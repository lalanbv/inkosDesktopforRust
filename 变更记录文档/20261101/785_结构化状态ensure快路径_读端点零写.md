# 785 号：结构化状态 ensure 快路径——读端点零写（784 号台账候选清偿）

日期：2026-11-01。前置：784 号读链台账登记候选「get_next_chapter_number 快路径（9ms→µs 级 200×，须 legacy 兼容探针）」；757 号演进路线五条收官后的第一个台账候选轮。

## 一、交付

1. **快路径内嵌 bootstrap 本体**（engine-rs/src/state/state_bootstrap.rs）：新增 `structured_state_fresh_manifest` 私有判定 + 主入口 `bootstrap_structured_state_from_markdown` 提前返回——目标态已达成时零写零 markdown 装载返回既有 manifest。判定条件（全部通过才跳过，任一不确定回落慢路径）：
   - manifest.json 有效且结构定型：`schema_version == 2`、`language ∈ {zh, en}`、`migration_warnings` 已归一（`unique_strings` 不动点）；
   - 四件齐且合法免修：current_state.json 可解析；hooks.json 可解析**且 repair 零命中**（对齐 `load_hooks_state_if_valid` 的 repaired 分支）；chapter_summaries.json 可解析**且无重复行**（dedup 零命中）；
   - 进度与 durable 链一致：`manifest.last_applied_chapter == max(产物前缀, normalize(fallback))`（i64→u32 截断口径与 `load_markdown_bootstrap_state` 一致）。
2. **等价性论证**：慢路径「全有效分支」的唯一副作用是把逐字段相同的 manifest 重写回盘（`write_string` = `write_file_atomic` fsync 原子写）——快路径跳过该写，四件状态与进度字段观测恒等。derived 恒等证明：`get_next_chapter_number` 先算 `resolve_durable_story_progress(None)` = 纯产物前缀，bootstrap 内 derived = max(fallback, 产物前缀)，fallback ∈ {None→0, durable} → derived 恒等于产物前缀，manifest 目标态可零 markdown 装载判定。4 个调用点（manager 读链/write_next 持久化钩子/memory_retrieval/runtime_state_store）全部丢弃返回值——API 响应零变化，快路径全调用点受益。
3. **行为面注记（保守化口径）**：慢路径全有效分支还会把 markdown 装载 warnings 并入 `manifest.migration_warnings`——快路径跳过该诊断刷新（状态字段不受影响），warnings 在下一次非 fresh 的 ensure（写链）照常补记；migration_warnings 无逻辑消费方（grep 实证仅落盘诊断）。
4. **双栈核验**：TS `getNextChapterNumber`（packages/core/src/state/manager.ts:423）只解析 durable 链**无 bootstrap ensure**——ensure 是 Rust 产品引擎侧加固，无镜像债务；快路径让 Rust 读路径更贴近 TS 行为（读不落盘）。

## 二、测试（legacy 兼容探针 + 回归锁）

- state_bootstrap 54/54（+4 新）：spy store 写计数锁 `bootstrap_fast_path_second_call_writes_nothing`（legacy 首读建四件→二读**零写零 mkdir**，计数比内容比对更强）；进度落后回落探针；四件缺一补齐探针（784 台账行为面风险：读端点 ensure 系 legacy 书可用性所依）；显式 fallback 目标态零写探针。既有 5 个慢路径行为测试（absent/normalize 99→1/repair/corrupt/dedup）全过=回落面完好。
- manager 8/8（+1 新）：`get_next_chapter_number_ensures_legacy_state_then_stops_rewriting` 真 FS 端到端——legacy 书首读补齐四件+manifest 记 durable 进度，二读 **manifest mtime 不变**（fsync 原子写不再触发的端到端证明）。
- write_next 22/22（持久化钩子路径回归）。

## 三、bench（静默窗，基线显式 --update 刷新）

- **读链兑现**：list_books 8.93ms→214.7µs（**-97.6%，41.6×**）/ book_detail 9.96ms→683µs（**-93.1%，14.6×**），刷新基线 222.1µs/665.1µs；read_chapter 50.07µs 恒等（不受影响路径稳定）、chapter_workspace/truth/genres/analytics 噪声带内（analytics 前轮 +23.9% 复测回落 463µs=p0.18 噪声带确证）。
- **跨轮回退暴露与 A/B 归因**：全量复跑首次暴露 `backup_export/run_export_12ch` 3.48ms→12.9ms（+252% 超闸）。按台账铁律隔离复跑仍 11.9ms → **stash 本轮改动在 784 HEAD（481ce5fd）复跑同场景仍 12.2ms = 785 脱罪**——根因 779 号（10-30 落地）将备份清单写转 `write_file_atomic_sync`（backup_routes.rs:179，「撕裂清单=整包备份不可解读」的掉电韧性换成本）+782 号，均落在基线采集（10-29）之后，784 轮只做基线 JSON 合并未复跑写侧存量 21 条而漏检两轮。779/782 为刻意持久性决策（用户操作频率低），刷新基线=如实记录已发布现实，非洗回退（台账已注记）。

## 四、验证

- testgate：engine-rs 46 目标 **1955 passed**（784 为 1950，+5 恰为本轮新探针 4+1）、src-tauri 25 目标 583 零漂移、RustSec 双 0。
- clippy 双 crate 0 告警（--all-targets）；gate:ts 八步全绿含契约差分 0 分歧（audit:npm/rust-bin/node-fallback-smoke/export-epub-smoke 全过）。
- bench-gate --update 全量重测 28 场景刷新基线（collectedAt 2026-11-01），场景识别完好。

## 五、教训

1. **基线豁免面（--update）必须配 A/B 归因**——全量复跑暴露的跨轮回退先 stash 本轮改动在 HEAD~1 复跑定罪/脱罪，再刷新；否则 --update 会被误读为洗回退，台账公信力受损。
2. **JSON 合并式入闸 ≠ 存量基线健康**——784 新场景入闸时未复跑写侧 21 条，779/782 的 fsync 成本漏检两轮；全量复跑是暴露存量回退的唯一手段（本轮首次全量复跑即兑现台账价值第二笔）。
3. **ensure 幂等快路径要判定保守化**——所有不确定（读失败/需修复/需去重/进度不一致）一律回落慢路径；语义面（legacy 补齐）由端到端探针（spy 计数+真 FS mtime）锁住而非靠推理，等价性论证与回归锁缺一不可。
