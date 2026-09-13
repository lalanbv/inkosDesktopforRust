# 407 · settle 落盘台账丢分类列修复——promises kind 断言转硬通过

日期：2026-09-14　前置：406 号备案的 407 候选缺陷

## 根因定位（406 号遗留）

「settle 链重写 pending_hooks.md 后 13 列（kind 丢失）」的真凶不是
`render_hook_snapshot`（写法绑定/晋级/consolidator 面，14 列正确），而是
**`state/projections.rs` 的 `render_hooks_projection`**——settle 落盘走
**结构化 state 投影**（state.hooks → markdown）这条独立渲染面，其表头与
行 cells 均止于第 13 列 notes，未输出 R23 新增的第 14 列 kind。

定位方法（对照实验 + 单元解析实验）：
1. 端到端 fixture 环境中 settle 后台账 14 列 fixture 被重写为 13 列；
2. 一次性 Rust 测试对同文件 `parse_pending_hooks_markdown` → 4 hooks
   kind 全 None，而 `parse_markdown_table_rows` 行 len=13——排除解析器，
   锁定上游投影渲染；
3. `grep 起始章节` 全引擎表头渲染点对照，唯一 13 列落盘面 =
   `render_hooks_projection`。

## 修复

- `render_hooks_projection`：表头（zh「分类」/ en「kind」）与行 cells 补
  第 14 列 `hook.kind → hook_kind_id`（无 kind 输出空单元格，列数恒 14）；
- 新增单元测试 `hooks_projection_emits_kind_column`（带 kind 渲染悬念行、
  无 kind 行列数保持 14）；projections 9/9 绿；
- `walkthrough-fixture.mjs` kinds 断言由「警告级」转回哨兵语义（kinds 再
  缺失即投影/落盘链回归）。

## 端到端验证（fresh root，重建引擎后）

`walkthrough-fixture.mjs` exit=0 且**无警告**：

```
promises timeline = 4 条，kinds = worldview,suspense,emotion,artifact
run-log total = 11，kept = 11
```

promises 的 kind 补齐链路（settle 产 kind → 结构化 → 落盘投影 14 列 →
promises 端点读台账补齐）全绿。

## 门禁

INKOS_DUEL=1 **40 运行单元 1788 用例** exit 0（projections 修复 + 新测试
零回归）；bindings 无变更（本批无 ts-rs 面）。

## 附带澄清

405/406 号走查时「promises kind 有值」的记录来自 sqlite3 手工注入的
DB 行 + 手写 14 列台账；本批后链路自身即可产出——fixture 环境不再需要
任何手工注入。
