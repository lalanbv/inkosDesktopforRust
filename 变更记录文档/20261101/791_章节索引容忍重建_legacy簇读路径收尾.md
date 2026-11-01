# 791 号：章节索引容忍重建——legacy 簇读路径收尾（Rust rebuild 镜像）

日期：2026-11-01。前置：788/789/790 legacy 发散簇（snapshot 加载/编号解析）已闭环；本轮补齐同簇最后一面——**章节索引加载**。

## 一、发散发现

TS `StateManager.loadChapterIndex` 四种情形 throw：index 缺失+章节文件存在 / index 损坏（SyntaxError）/ 非数组 / 条目 schema 不符（ZodError 传播）。Rust `load_chapter_index` 同场景 = `rebuild_chapter_index_from_files_at` 从章节文件**内存重建**返回。后果：index-less legacy 书在 Node 回退腿 `GET /api/v1/books/:id`（bookDetail 调 loadChapterIndex）**500**，Rust 腿重建后 **200**——读路径双栈发散，legacy 簇（788 snapshot/789 编号/791 索引）最后一块。

## 二、修复（packages/core/src/state/manager.ts）

- `loadChapterIndex` 容忍化：index 缺失/损坏/非数组/条目 schema 不符 → 不再 throw，降级重建；**valid 非空 index 原样返回**（不变）；重建非空 → 用重建行；重建空（真 fresh 书）→ []。语义与 Rust `load_chapter_index`（valid → 返回；corrupt/missing/empty → rebuild；rebuilt 空 → []）逐分支对齐。
- 新增 `rebuildChapterIndexFromFilesAt`（Rust rebuild 镜像）：文件名 `^(\d+)[_-]?(.*?)\.md$` 提取章号+标题（0 号跳过）、标题规则镜像 `title_from_filename`（前导 `_` 剥离、`_`→空格、空 → 第N章）、词数=去全部空白后的码元数（Rust 注释引 TS 原版口径）、mtime→ISO 时间戳、按章号排序、**仅内存返回不写回**（写回走显式 saveChapterIndex——Rust 同款）。
- TS `ChapterMetaSchema` required `provenance`（Rust 无此字段）→ 重建行定 "generated"（与管线产出行一致）；status 定 "ready-for-review"（Rust ReadyForReview 镜像）。

## 三、测试（durable-progress.test.ts 791 块，6 项）

index-less 重建（排序/标题/词数口径/字段/provenance/**index.json 不被写回断言**）/ 损坏重建 / 非数组重建 / **valid 原样返回（不变量）** / 标题规则矩阵（夜_探→夜 探、无标题→第4章、前导 __ 剥离）/ 空书 → [] 不变。

## 四、验证

- core vitest **979/979**（973+6，全 flow 套件零回归）+ tsc 归零；gate:ts 八步全绿含契约差分 0 分歧（valid-index 书行为不变）。
- Rust 零触碰。

## 五、教训

1. **发散簇按「消费面」穷尽后才算收尾**：788（写链 snapshot）/789（编号）/791（读链索引）同属 legacy 簇但分散在三处——簇的边界由「哪些函数读 legacy 态」决定，不由轮次计划决定；789 收官时「簇已闭环」的宣告早了一轮。
2. **重建口径逐字段对照**：词数=去空白码元（Rust 注释反向引用 TS 原版）、标题规则、0 号跳过、不写回——每一条都是行为面，漏一条就是新的发散。
3. TS `ChapterMetaSchema` required `provenance` vs Rust 无此字段——schema 差异在镜像移植时要显式决策（本轮回退行定 "generated"），不能留给隐式 undefined。
