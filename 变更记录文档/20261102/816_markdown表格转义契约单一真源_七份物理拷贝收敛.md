# 816 号：markdown 表格转义契约单一真源——7 份物理拷贝收敛为每侧一模块

日期：2026-11-02。前置：812 号登记（渲染副本合并重构，择机）。

## 一、盘点（grep 转义字面全清单）

| 位置 | 语义 | 处置 |
|---|---|---|
| rust `state/projections.rs` | 竖线+换行+trim | 并入共享模块 |
| rust `utils/story_markdown.rs`（escape/restore/split） | 同上 | 移入共享模块 |
| rust `agents/chapter_analyzer.rs` | 竖线+换行、无 trim | 并入（加 trim；golden 输入无前后空白，testgate golden 全绿实证中性） |
| rust `agents/writer.rs`（摘要行） | 仅竖线+trim | 并入（**补换行→`<br>`**；golden render_delta_summary_row 用例无换行输入，实证中性） |
| rust `forecast/render.rs` | 换行→**空格**（双侧一致） | 不动——另一语义 |
| TS `state-projections.ts` | 竖线+换行+trim | 并入共享模块 |
| TS `utils/story-markdown.ts`（escape/restore/拆分） | 同上 | 移入共享模块 |

## 二、合并

- rust 新建 `utils/markdown_table.rs`：`escape_table_cell` / `restore_table_cell` / `split_table_row_escaped` + 单测 3（含 `\|` 感知拆分、`<br>`/CRLF/trim、往返）；`utils/mod.rs` 注册。
- 四消费点接线：projections / story_markdown / chapter_analyzer / writer（前两者删本地实现，后两者改为委托——analyzer 加 trim 与 TS 对齐，writer 补 `<br>` 与摘要渲染族对齐）。
- TS 新建 `utils/markdown-table.ts`（`escapeTableCell` / `restoreTableCell` / `splitTableRowEscaped`）；`state-projections` / `story-markdown` 接线（story-markdown 删本地两 fn、拆分改共享）。
- 行为零变更：golden（渲染期望）70/70 与 TS 侧 golden 反向断言全绿实证；810-812 的语义测试套件（多行/含竖线往返）全绿实证。

## 三、验证

cargo:testgate engine-rs 47 目标 **1979**（+3=共享模块单测）+ src-tauri 25 目标 584 + RustSec 双 0；clippy 双 0（无死代码告警=旧副本删净）；gate:ts 八步全绿；权威 pnpm -r test core 179/987 + studio 137/725 + cli 23/81（TS 共享模块导入重排零语义变化，三包全绿）。diff 71/1 维持。

## 四、810-816 转义契约族总账

- 810：projections 转义补 `<br>`（kind enrich 丢失实锤）→ 811：双侧解析器补还原（上游缺口）→ 812：双侧拆分器感知 `\|` + fork 副本补转义 → 814：studio 读面还原 + 终态徽标 → 816：全部副本收敛单一真源。
- 契约（双侧逐字同集）：渲染 `|`→`\|`、`\r?\n`→`<br>`、trim；解析未转义 `|` 拆分、`<br>` 三形态→换行、`\|`→`|`。唯一例外 `forecast/render`（换行→空格）双侧一致有意保留。

## 五、登记（695 工项余项）

- mock 记录工具支：定性**不做**（815 已证 issueCount 收敛必致 passed 发散——rust verdict true(88) vs TS 有 issue 即 false；净零收益+794 循环风险）——残余并入「数值分协议扩展（上游面）」。
- forecast/render 的换行→空格语义维持。
- UI superseded 徽标 ✅（814）；渲染副本合并 ✅（816）；TS 计数口径 ✅（813 对齐+813 durable 定性保留）；rust 重建去重 ✅（813）。

## 六、教训

1. **重构择机项的再评估要看"未清偿残差"**：808 认为收敛 issueCount 需要 mock 记录工具支，815 深挖后发现它必然换来 passed 发散——残差的清偿路径是协议扩展，mock 支不做才是最优解。
2. **单一真源的收益在"下一个改动"时兑现**：本轮 writer/analyzer 两处获得 `<br>`/trim 补齐是零成本的（改一行导入），若无共享模块则又是两轮"发现→修一处"。
3. 盘点用 grep 转义字面（812 教训）一次枚举 7 处，合并一次完成。
