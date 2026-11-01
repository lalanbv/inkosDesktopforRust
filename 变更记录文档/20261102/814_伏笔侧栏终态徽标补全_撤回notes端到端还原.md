# 814 号：伏笔侧栏终态徽标补全——superseded/resolved 可见化 + 撤回 notes 端到端还原

日期：2026-11-02。前置：805 号登记（UI PendingHooksView 无 superseded 徽标）；810-812 号（投影转义/还原契约定案）。

## 一、现状

- `PendingHooksView` 徽标只覆盖 open/progressing（活跃）与 deferred（种子）；**resolved 与 superseded 无标**——已撤回/已兑现的钩子外观与 open 无差，作者看不出前提已被撤回。
- `truth-display.ts` 的 `splitTableRow` 朴素按 `|` 拆分且不还原：810/812 号起渲染器把换行转 `<br>`、竖线转 `\|`，含竖线的撤回 notes 行会被拆出多一列 → `parsePendingHooks` 的「列数==表头」过滤**整行丢弃**——伏笔直接从 UI 消失；无竖线的多行 notes 则以 `<br>` 字面展示。核与 UI 两侧的转义契约在 UI 读面断链。

## 二、修复（studio）

- `truth-display.ts splitTableRow`：感知转义拆分 `(?<!\\)\|` + 单元格还原（`<br>` 三形态忽略大小写→换行、`\|`→`|`），与 core `parseMarkdownTableRows`（811/812）同集。
- `PendingHooksView.tsx`：新增 `resolved`→「已回收 / Resolved」（sky 色）、`superseded`→「已撤回 / Withdrawn」（弱化 + 删除线装饰）徽标；正文 `whitespace-pre-line`（撤回 notes「既有 + 换行 + 依据」按行展示），已撤回条目正文弱化。双语沿用 `tr()` 惯例（728/731 号 i18n 口径）。

测试：`truth-display.test.ts` +1（含 `\|`/`<br>` 的 superseded 行不丢、还原、status/kind 透出）；新建 `PendingHooksView.test.tsx`（renderToString）两场景：三态徽标齐全 + 撤回依据行可见（无 `&lt;br&gt;` 泄漏、含 `H01|H04` 还原）+ 英文本地化。typecheck 0。

## 三、验证

cargo:testgate engine-rs 47 目标 1975 + src-tauri 25 目标 584 + RustSec 双 0（rust 本轮零触碰，门禁照跑）；clippy 双 0；gate:ts 八步全绿；权威 pnpm -r test core 179/987 + studio **137/725**（+1 文件 +3 测）+ cli 23/81。diff 71/1 维持。

## 四、805→814 撤回生命周期端到端闭合

settle 撤回（805 reducer 三守卫）→ 状态落盘 → 投影渲染转义（810/812）→ 台账回读还原（811/812，rust resync / TS bootstrap / kind enrich）→ **UI 读面还原 + 终态徽标（814）**。差分器活体（810 mock 撤回支）覆盖引擎段；本轮补齐展示段。

## 五、登记（695 工项余项）

- 渲染副本合并重构（5 份物理拷贝，语义已对齐，择机）；mock 记录工具支（需状态机）；数值分协议扩展（上游面）；quality-trend 残差三件（808）。
- studio 侧其他读台账的视图（如有）沿用 `parsePendingHooks` 即自动受益；其他表格解析器（摘要/名册视图）若存在独立拆分器，沿同口径核一遍——本轮 grep `splitTableRow` 仅此一处。

## 六、教训

1. **契约要走到最后一个读者**：核侧转义/还原 811/812 闭合后，UI 的独立解析器仍是旧口径——「同实体多解析器」不只在 core 与 rust 之间，也在 core 与 studio 之间。
2. 终态可见化是生命周期落地的一部分：没有徽标的 superseded 对作者等于不存在撤回这件事。
