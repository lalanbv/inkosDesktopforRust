# 811 号：markdown 单元格 `<br>` 回读还原口径定案——双侧解析器补还原，渲染/解析成对

日期：2026-11-02。前置：810 号登记（双侧解析器均不把 `<br>` 还原为换行；rust resync 回读台账时非终态多行 notes 以 `<br>` 字面落回 state）。

## 一、考古定性

- TS 渲染侧 `state-projections.ts escapeTableCell` 于 7e60e13f（2026-09-25，InkOS 2.0）加换行→`<br>` 转义；同提交**未触碰** `story-markdown.ts` 解析器，且全史无任何提交给解析器加 `<br>` 还原 → **上游自身的有损往返缺口**，不是 rust 漂移。
- rust 渲染侧 810 号刚补齐 `<br>` 转义；解析侧与 TS 同形（朴素按 `|` 拆分、原样回传）。
- 暴露面不对称：TS 2.0 以 state JSON 为真相，md 只在 bootstrap 恢复/kind enrich 时回读（损失面小）；rust resync 把台账 md 回读进 state（主路径）——非终态钩子的多行 notes 会以 `<br>` 字面写回 state，再经 promises/UI 以原样 `<br>` 露出。

## 二、口径定案与修复

**渲染产什么，解析就还原什么**：双侧 `parse_markdown_table_rows` / `parseMarkdownTableRows` 的单元格管线统一加 `restore_table_cell`——`<br>` / `<br/>` / `<br />`（忽略大小写）→ 换行。普通单元格不受影响；该函数是三张表（hooks/summaries/roster 等）的公共拆分器，还原随之全表生效（与渲染侧转义同为全局）。

- `engine-rs/src/utils/story_markdown.rs`：`restore_table_cell`（OnceLock Regex `(?i)<br\s*/?>`）。
- `packages/core/src/utils/story-markdown.ts`：`restoreTableCell`（`/<br\s*\/?>/gi`）。

测试：rust `parse_markdown_table_rows_restores_br_to_newline`（三形态+普通单元格）+ 810 号投影测试扩断言（notes 经 `<br>` 往返无损）；TS `keeps multi-line withdrawal notes intact across the markdown round trip`（单行渲染 + 含 `<br>` + 解析回原 notes + `parseMarkdownTableRows` 三形态）。golden leaf.json 唯一含 `<br` 的用例（chapter_analyzer_suite[12]）为**渲染**期望，不受解析还原影响（即便先解析再渲染也会重新转义回 `<br>`）。

## 三、验证

cargo:testgate engine-rs 47 目标 **1972**（+1）+ src-tauri 25 目标 584 + RustSec 双 0（golden 套件在列全绿）；clippy 双 0；gate:ts 八步全绿（含 engine-contract-diff + legacy 11/11）；权威 pnpm -r test core **178/984**（+1）+ studio 136/722 + cli 23/81。diff 71/1 维持（仅余 808 quality-trend 残差）。

## 四、登记（695 工项余项）

- **竖线转义 `\|` 的还原**：渲染器把 `|` 转 `\|`，但双侧拆分器朴素按每个 `|` 拆（拆分先于还原）——notes 含竖线仍会把行拆出多一列。需感知转义的拆分器（`\|` 不拆 + 还原）双侧同修；对称缺陷、diff 不可见，独立轮。
- 渲染器副本不一：TS `story-markdown.ts:375 escapeTableCell`（仅竖线）与 rust `story_markdown.rs:478`（仅竖线）为 fork 期 `render_hook_snapshot` 族所用，均未转义换行——golden 锁定、对称，待与竖线拆分器一并定口径。
- TS 计数口径定性 / rust 重建去重 / UI superseded 徽标 / mock 记录工具支（需状态机）/ 数值分协议扩展（上游面）/ quality-trend 残差三件。

## 五、教训

1. **「对等豁免」要先问暴露面**：双侧同形的缺口在 TS 可以是 wart、在 rust 却在主路径（resync 回读）——对等≠无害，定口径看谁会真的走到。
2. **转义与还原成对设计**：补了一侧（810 转义）就要补另一侧（811 还原），否则只是把损失从「行截断」换成「字面泄漏」。
3. 公共拆分器是最省的还原落点：一处改、三张表受益，且与渲染侧「转义全局」对称。
