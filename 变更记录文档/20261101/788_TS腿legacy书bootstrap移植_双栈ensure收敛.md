# 788 号：TS 腿 legacy 书 bootstrap 移植——双栈 ensure 语义收敛

日期：2026-11-01。前置：785 号确认 Rust ensure 为 Rust 侧加固、TS `getNextChapterNumber` 无 ensure（当时判「无镜像债务」——**本轮证伪该结论的适用范围**：读链无债务，写链 snapshot 加载有真发散）。

## 一、发散发现（兼容性普查）

TS 腿 `loadRuntimeStateSnapshot`（runtime-state-store.ts）直读状态四件，`readJson` 对 ENOENT/损坏 JSON **直接 throw**。消费面：写链主流程 runner.ts `_executeNextChapterLocked` 无条件装载（L1655）→ `preserveCanonicalRuntimeState`（chapter-truth-validation 校验降级/和解失败路径）→ forecast context-builder。**后果**：legacy 书（状态四件缺失——上游 1.x 迁移项目/手工书）在 Node 回退腿上 write-next 直接 ENOENT 失败；同书在 Rust 腿被 ensure 自愈（785 快路径前置 bootstrap）——双栈发散，违反 780 号「双栈债务是镜像的」纪律。

## 二、移植实现（packages/core）

1. **`bootstrapStructuredStateFromMarkdown`**（state-bootstrap.ts，Rust `bootstrap_structured_state_from_markdown` 对齐移植）：
   - 容忍读四件（ENOENT/SyntaxError/ZodError → null，Rust `load_json_if_valid` 同语义；其余 IO 错误传播）；
   - durable 容忍版解析（index.json 缺失/损坏 → 空表，文件名 `N_` 前缀合并取连续前缀——Rust `load_durable_artifact_chapter_numbers` 镜像）；
   - language：existing manifest 合法值优先，否则 book.json（仅 "zh" 认定）；
   - 三件派生：summaries=markdown 表（剥 tension 列——2.0 strict schema 外字段；chapter≥1 且标题非空）、hooks=markdown 表+bullet fallback（type 空白→"unspecified"、status 词表外→"open"）、currentState=「当前章节」行||durable + parseCurrentStateFacts；
   - manifest fresh（lastApplied==durable）→ 保留，否则按 durable 重算（projectionVersion 保留）；
   - **只物化缺失/损坏件 + 重算过的 manifest；全 fresh → 零写**（785 快路径镜像）。
2. **与 `saveRuntimeStateSnapshot` 的关键差异（安全设计）**：bootstrap **只写四个 JSON、不重写 markdown 投影**——后者会 parse→render 往返洗掉解析器未覆盖的 markdown 内容（Rust bootstrap 同款：markdown 是真相源）。
3. **`loadRuntimeStateSnapshot` 接管**：四件全有效 → 原路径；任一缺失/损坏 → bootstrap（=Rust `load_runtime_state_snapshot` 先 ensure 再读同语义）。
4. **既有测试契约翻转**：`fails loudly when corrupt` → 损坏件重建为合法 JSON（markdown 真相源语义，Rust 镜像），更新断言并注记。

## 三、登记不热修（784 号先例）

`resolveDurableStoryProgress`（TS 写链编号权威）**index.json 缺失即返回 0**——index-less legacy 书上 `getNextChapterNumber`=1，write-next 会**覆盖既有第 1 章**（Rust 同场景=文件前缀容忍合并）。属写链编号语义变更（2.0 fail-closed 设计 vs Rust 容忍合并），须独立轮次带写流测试处置，本轮仅在 bootstrap 内部使用容忍版（manifest 进度正确），登记台账。

## 四、验证

- vitest：runtime-state-store 6/6（4 新增：markdown 物化+markdown 零触碰断言 / steady-state 零写 mtime 锁 / 损坏件重建+合法件字节保留 / 纯章节派生）+ **core 全量 175 文件 964 passed**（+4，全 flow 套件零回归）。
- tsc --noEmit 归零；gate:ts 八步全绿含**契约差分 0 分歧**（有状态书行为不变——bootstrap 仅 legacy 半态触发）。
- Rust 零触碰（本轮为 TS 腿专属轮）。

## 五、教训

1. **「无镜像债务」结论要限定适用面**：785 号判 TS 无 ensure=无债务，只对读链成立；写链 snapshot 加载是同簇债务的另一面——镜像巡检要按「消费面」枚举而非按「函数同名」枚举。
2. **镜像移植要防投影往返**：saveRuntimeStateSnapshot 顺手重写 markdown 投影，若 bootstrap 复用它会把 legacy 真相面洗成解析器投影的并集——「只写 JSON 不触真相源」是 Rust bootstrap 的载荷语义之一，移植时逐行对照副作用面。
3. **发散簇要一次定界、分轮处置**：snapshot 加载（本轮修）与编号解析器（登记）同属 legacy 簇，但后者动写链编号语义——按线性可控性拆轮，台账登记不热修。
