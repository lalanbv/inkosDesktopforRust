# 40 号｜Phase 3 pipeline 域：chapter-analyzer 与持久化产物装配（41 号最后依赖扫清）

> 里程碑：write-next 链的**最后一枚 agent 依赖**移植完毕。chapter-analyzer
> （634 行——修订/重写后的结算重分析）+ runner 的 `buildPersistenceOutput`
> （~40 行编排：正文未变原样返回 / 已变重分析装配）。41 号
> `_writeNextChapterLocked` 主体至此**零未移植依赖**。

## 背景

39 号后 runner 主体前只剩两件：chapter-analyzer（`buildPersistenceOutput` 的
核心依赖，全部底层依赖已就绪）与该方法本身。本号一并交付，41 号进入纯装配。

## 改动

### 新建 `engine-rs/src/agents/chapter_analyzer.rs`（~840 行，含测试）

**编排入口 `analyze_chapter`**
- genre/语言解析 → 八路真相文件读（current_state 回退链 / ledger / hooks /
  subplot / emotional / 角色矩阵 / story_frame / volume_map——占位文案按
  language 双语 `(文件尚未创建)`/`(file not created yet)`）
- book_rules 读取 + governed 三全判定（intent+package+ruleStack）
- 记忆选集（goal = 标题+正文空行连接；outline_node 用 analyzer 简化版）
- governed 工作集：hook（keep_recent 默认）/ subplot / emotional / 矩阵 +
  记忆证据块（hooks/summaries/volumeSummaries 的 `??` 回退——非空串保留）
- LLM(0.3) → `parse_writer_output` → **canonical 正文/字数回填**（分析器
  不重写正文）→ 标题回退（解析标题等于默认形态或 `第N章` → 用入参标题）

**纯函数（golden 守门）**
- `build_system_prompt`（zh/en 逐字——分析维度/输出格式十二 tag/关键规则）
- `build_user_prompt`（zh/en 逐字——标题行/正文/状态卡/账本/七块拼接）
- `build_reduced_control_block`（analyzer 版：已选上下文带 excerpt 的 bullet
  形态，en 用 `, ` 连接 zh 用 `、`）
- `build_memory_goal`、`find_outline_node`（简化版：标题行正则 + 下一非标题
  行；`\b` 语义防 3 误配 34）、`render_summary_snapshot`（analyzer 私有版：
  空表 → 占位、`\n` → `<br>`）

### 新建 `engine-rs/src/pipeline/build_persistence_output.rs`（~250 行，含测试）

`build_persistence_output`：
- `finalContent === output.content` → 原样返回 writer 产物（零 LLM 调用）
- 已变 → `analyze_chapter` 重跑 → 回填 canonical 正文/字数、清空 post-write
  校验结果（针对旧正文）、**保留** writer 的 hook 健康与 token 用量、delta/
  snapshot/updatedSummaries 清空（重分析产物走 markdown 真相面）

### golden 双侧

- TS dump +1 域 14 例：`chapter_analyzer_suite`（系统提示词 zh 数值/en 无规则、
  用户提示词 zh 全块/en 极简、缩减控制块双语、记忆目标双态、大纲节点四态、
  摘要快照转义/占位）

## parity 要点（移植难点）

1. **模板空行微观结构 ×2**：①系统提示词的 `- 平台：${platform}\n${numericalBlock}`
   ——行断 + 前导 `\n` 合成**空行**；②用户提示词的 `${currentState}\n${ledgerBlock}`
   同构。两处首版都漏行断，golden 差分逐字抓出修正（本会话第三、四例模板
   换行 bug——模板逐字纪律已固化为检查项）
2. **analyzer 的 `findOutlineNode` 与 planner 不同源**：简化版（标题行正则 +
   下一行），**勿合并**——两者的语义差异是 load-bearing（planner 需范围/节拍
   四级匹配，analyzer 只需锚点）
3. **`renderSummarySnapshot` 三个版本**：story_markdown 版（空表 `- none`）/
   analyzer 版（空表占位文案 + `<br>` 转义）/ projections 版（带诊断）——
   各服务不同消费面，保留三份
4. **`defaultChapterTitle` 按 language 而非 countingMode**：与 writer-parser 的
   版本不同源；标题回退双条件（默认形态或 `第N章`）
5. **记忆证据块的 `??` 回退**：块为**空串**时保留（仅 None 回退）——与 reviser
   同语义，`and_then` 不加 `filter(!is_empty)`
6. **buildPersistenceOutput 的保留/清除矩阵**：保留 title 传入供回退判定、
   hook 健康、token 用量；清除 post-write 校验（旧正文产物）、delta/snapshot
   （重分析产物无结构化 delta）

## 验证

- **lib 单测**：845 passed / 0 failed（+10 新单测：analyzer 纯函数六组 +
  buildPersistenceOutput 三态[未变直通/重分析装配/analyzer 失败传播——含
  genre 档案落盘的全链路 mock 集成]）
- **golden 差分**：76 域全绿（+1 域 14 例；差分抓出两处模板空行真 bug）
- **export-bindings**：1004 passed
- **TS 全量**：185 文件 / 1798 测试全绿（leaf dump 重生成）
- **clippy**：`cargo clippy --lib --tests` 零警告

## 下一步

- ⬜ **41 号**（write-next 收官，零未移植依赖）：`_writeNextChapterLocked`
  主体装配——prepareWriteInput（35 号 plan 持久化复用 + composer）→
  writeChapter（32 号）→ manual 模式写完即停 / review-cycle（37 号）→
  promotion pass（hook-promotion，已移植）→ 标题去重（post-write-validator，
  已移植）→ buildPersistenceOutput（本号）→ 长跨度疲劳（long-span-fatigue，
  已移植）→ truth-validation（38 号）→ 段落形态检查 → persistChapterArtifacts
  （37 号）→ 通知/webhook + state-store book 层薄封装 +
  `/api/v1/books/:id/write-next` 路由（39 号 task-store + SSE 广播面）。
