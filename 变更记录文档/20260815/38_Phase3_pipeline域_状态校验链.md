# 38 号｜Phase 3 pipeline 域：状态校验链（validator + truth-validation + 重试结算）

> 里程碑：write-next 落盘前的真相一致性链全量合龙。state-validator（310 行）+
> chapter-truth-validation（145 行）+ chapter-state-recovery 的重试结算链
> （37 号预留的 ~160 行）——settler 输出经 LLM 校验，失败重试结算，再失败
> 降级保旧真相。39 号 runner 主体的最后一块编排件。

## 背景

37 号交付了审核环与落盘编排，但 `persistChapterArtifacts` 之前的
`validateChapterTruthPersistence`（4.1 步）依赖本号三件：校验器本体、
编排入口、以及 37 号有意留白的重试链（依赖 StateValidatorAgent）。
chapter-analyzer + `buildPersistenceOutput` 随 39 号与 runner 主体同批交付
（强耦合，单独移植无消费方）。

## 改动

### 新建 `engine-rs/src/agents/state_validator.rs`（~560 行，含测试）

- `validate`：双 diff（状态卡/伏笔池）→ 空 diff 直接通过 → LLM(0.1) → 解析
- **最小裁决协议**：首行 `PASS`/`FAIL`；警告行三分支（`[cat] desc` /
  `- desc`（`* ` 同）/ 长度 >5 裸行）
- **JSON 回退**：精确解析 → 平衡对象提取（字符串感知配对 + 闭合后仅允许
  空白/`,]}` 拒收尾随文本）；`passed` 非布尔拒收；warning 字段缺省
  `unknown`/空串
- `compute_diff`：行集合差（重排序无 diff；空白行不参与）
- `build_authority_context_block`：三级权威优先级声明 + story_frame/
  book_rules/章节摘要节选（空 → `(empty)`）
- 系统提示词逐字（六类矛盾 + FAIL 仅限硬矛盾的 IMPORTANT 段）
- `StateValidatorChat` trait 注入

### `chapter_state_recovery.rs` 扩展重试结算链（+~330 行）

- `SettlePort` / `ValidatePort` 端口（writer settle 与 validator 的链内形态）
- `retry_settlement_after_validation_failure`：settle(allowReapply + 反馈) →
  复验 → `Recovered{产物, 验证}` / `Degraded{问题清单}`
- `build_state_validation_feedback`（空警告的通用文案 / 警告逐条列出，zh/en）
- `build_state_degraded_issues`（空警告兜底单条 / 逐条映射，severity=warning）
- `build_state_degraded_persistence_output`：结算字段回退旧真相
  （delta/snapshot/updatedSummaries 清空，标题正文保留）

### 新建 `engine-rs/src/pipeline/chapter_truth_validation.rs`（~370 行，含测试）

`validate_chapter_truth_persistence` 编排：
- validator **抛错**（网络/解析）→ 可用性降级：`passed: true` + 空警告 +
  state-degraded + 旧真相回退 + 可用性注记注入审计
- 校验失败 → 重试结算：recovered 换产物；degraded 回退旧真相 + 降级问题
  合入审计
- 校验通过 → 原样放行

## parity 要点（移植难点）

1. **空 diff 短路**：文本相等**或**增删行集均为空（如纯重排/空白行差异）
   → 跳过 LLM 直接 `passed: true`（省调用且避免误报）
2. **闭合括号后的字符白名单**：空格属结构终结符——`{...} more text` 以空格
   开头会被**接受**（TS 同语义，首版测试预期写反被单测抓出）；紧邻非结构
   字符（`{...}more`）才拒收
3. **validator 抛错 ≠ 内容失败**：可用性降级返回 `passed: true`（不让网络
   问题伪装成内容矛盾），但状态标记 state-degraded 强制人工复核
4. **`ValidationWarning` camelCase**：JSON 路径字段 `category`/`description`
   天然一致；`ValidationResult` 序列化对齐
5. **降级产物的字段选择**：仅结算六字段动（三旧真相回填 + delta/snapshot/
   summaries 清空），标题/正文/字数/post-write 全保留

## 验证

- **lib 单测**：831 passed / 0 failed（+15 新单测：validator 七态[空 diff 短路/
  PASS 带类目/无效裁决/diff 四态/JSON 三态/平衡提取字符串感知/权威块矩阵]、
  重试链三态[恢复/降级/降级产物字段]、truth-validation 四态[直通/抛错降级/
  重试恢复/重试降级回退]）
- **golden 差分**：75 域全绿（+1 域 13 例：diff 四态/权威块三态/解析六态；
  错误路径断言错误存在性——V8 `Error.toString` 前缀差异不参与字节级对比）
- **export-bindings**：990 passed
- **TS 全量**：185 文件 / 1798 测试全绿（leaf dump 重生成）
- **clippy**：`cargo clippy --lib --tests` 零警告

## 下一步

- ⬜ **39 号**（write-next 收官）：chapter-analyzer.ts（634 行）+
  `buildPersistenceOutput` + `_writeNextChapterLocked` 主体
  （prepareWriteInput[35 号 plan 持久化复用] → writeChapter → review-cycle
  [37 号] → promotion pass → buildPersistenceOutput → truth-validation
  [本号] → persistChapterArtifacts[37 号] → 通知/webhook）+ task-store 异步
  任务模型 + `/api/v1/books/:id/write-next` 路由挂载（strangler 首个核心端点）。
