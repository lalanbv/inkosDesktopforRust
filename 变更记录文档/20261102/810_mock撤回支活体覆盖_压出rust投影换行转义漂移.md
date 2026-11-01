# 810 号：mock 撤回支活体覆盖——压出 rust 投影换行转义漂移（promises 收敛维持 71/1）

日期：2026-11-02。前置：805 号登记（撤回路径仅单元级验证，mock 无撤回分支活体差分未覆盖）；808 号登记（mock 记录工具支）。

## 一、选型

- **mock 撤回支**（本轮落地）：`SETTLER_TEMPLATE` 的 H03 改为 `status: "superseded"` + notes=撤回依据——双协议同源（TS `submit_runtime_state_delta` 工具支 / Rust 文本 `RUNTIME_STATE_DELTA`）；`ANALYZER_HOOKS_TABLE` 的 H03 **保持 pressured**，故意压测「台账导入 merge 不得复活终态」（805 terminal 语义 + 804 incoming 优先的交互）。
- **mock 记录工具支**（登记不做）：808 已证收益有限——issueCount 可收敛但 `passed` 因 2.0 无数值分必散（TS 有 issue 即 false vs rust verdict true）；且 TS 腿发现→触发修订循环，有 794 号 182× 循环同型风险，需 mock 状态机，不是「最小有界」。

## 二、活体压出新分歧 → 真 bug

撤回支落地后 diff：promises `/timeline/3/kind` 分叉。借本轮新增的 `INKOS_DIFF_BODY_PREVIEW=4000` 全量预览对照双腿 timeline：

- **撤回生命周期双腿逐字一致**：H03 summary=「母亲留下的碎镜在镜界发光。\n作者修订撤回：…」（既有 notes + 换行 + 依据）、openedAt 2、lastAdvancedAt 3、state advancing——805 移植活体成立；H01/H02/H04 全同。
- 唯一差异：**rust 的 H03 丢 `kind`**。

根因：rust `projections.rs::escape_table_cell` 只转义 `|`；TS `escapeTableCell` 还把换行转 `<br>`（7e60e13f，2026-09-25）——rust 移植 e5adc8e1（08-11）早于该提交未跟进=**移植漂移**。撤回 notes 天然多行 → rust 渲染的台账 H03 行被截成两行 → `get_promises` 回读台账做 kind enrich（R23/396：sqlite 投影不落 kind，从台账第 14 列补）时 H03 行残缺 → kind 丢。node 的 kind enrich 读 books/ **静态原始台账**（801），故未炸。

修复：`escape_table_cell` 补 `\r\n`/`\n` → `<br>`（TS 逐字同款）；单测扩展 + 新增 `hooks_projection_keeps_multiline_notes_on_one_row_and_round_trips_kind`（多行 notes 单行渲染、`| superseded |`、回读 kind/status 不丢）。重跑 diff：**promises ✓**，共享面 **71 对比 / 1 分歧**（仅余 808 登记的 quality-trend 残差）。

## 三、差分器可观测面

`engine-contract-diff.mjs` 分歧响应体预览长度 env 可调 `INKOS_DIFF_BODY_PREVIEW`（默认 300 不变）——803/804/810 三轮都因 300 字截断看不到分叉字段而绕道 --keep 现场或二次脚本。

## 四、验证

cargo:testgate engine-rs 47 目标 **1971**（+1）+ src-tauri 25 目标 584 + RustSec 双 0；clippy 双 0；gate:ts 八步全绿（含撤回支下 node-fallback-smoke 夹具断言 + engine-contract-diff + legacy 11/11）；权威 pnpm -r test core 178/983 + studio 136/722 + cli 23/81。

## 五、登记（695 工项余项）

- `<br>` 回读不还原：双侧解析器均不把 `<br>` 还原为换行（TS 同形）。rust resync 回读台账进 state 时多行 notes 会以 `<br>` 字面落回（非终态钩子才会发生；superseded 受终态保护）——双侧对等的 wart，待定口径（还原 or 保持字面）。
- mock 记录工具支（审改循环风险，需 mock 状态机）；quality-trend 残差三件（808）；TS 计数口径定性；rust 重建去重；UI superseded 徽标；数值分协议扩展（上游面）。

## 六、教训

1. **活体压测是找漂移最便宜的方法**：805 单元级全绿，但只有把撤回真正跑进双腿写链，才暴露投影渲染层的漂移——单元测试覆盖的是「我想到的路径」，差分器覆盖的是「数据真走过的路径」。
2. **多行文本是 markdown 投影的边界用例**：任何追加式语义（撤回依据、修订记录）都会产多行；渲染器转义与解析器还原要成对设计。
3. 工具债顺手清：取证被同一截断绊倒三次后，把预览长度做成可调比每次写二次脚本便宜。
