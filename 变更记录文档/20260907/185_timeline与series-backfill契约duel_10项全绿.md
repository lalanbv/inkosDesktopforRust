# 185 · timeline 与 series-backfill 契约 duel：单测验证提升到契约层

- 日期：2026-09-07
- 模块：engine-rs（tests/strangler_duel.rs +2 用例、server/series_backfill_routes.rs apply 响应 path 相对化）、packages/studio/api/server.ts（extract 补目标书校验 + apply path 相对化）、需求分解文档
- 类型：test + fix（契约层补强）
- 关联：181/184 号（被测端点）、174 号教训（测试路由 ≠ 生产路由）、179 号分解文档

## 一、改动

1. **`strangler_timeline_duel`**：双端真进程对跑——GET 缺文件 `{"timeline":null}`、PUT 合法落盘、PUT 非法（version≠1）400、GET roundtrip normalize 后 DTO 等价。
2. **`strangler_series_backfill_duel`**：新增 `spawn_backfill_mock_llm`（返回**围栏 JSON** content——顺带把「宽松提取」的 LLM 侧输入搬进契约层）。对跑：apply 无草稿 400、extract 缺参 400、extract 合法（真实 mock LLM HTTP 链路）→ 草稿 DTO normalize 等价 + items 数量校验、apply 勾选子集 → 200 applied:1。
3. **契约修正（duel 抓到的双端差异）**：TS extract 缺「目标书必须存在」校验（Rust 有）→ 对齐补上；apply 响应 `path` 由绝对路径改相对 `story/series_backfill.md`（服务器侧细节不入契约，双端天然等价）。

## 二、验证

| 项 | 结果 |
| --- | --- |
| `INKOS_DUEL=1 cargo test --test strangler_duel` | **10/10 全绿**（+timeline/series-backfill 两新用例，总时长 ~41s） |
| studio vitest 全量 | 765/765（三次复跑稳定） |
| studio typecheck | 干净 |
| engine-rs cargo test 全量 + clippy | 全绿 + 零告警 |

## 三、过程教训

1. **duel 第一跑就抓到真差异**（TS 缺目标书校验）——「双端 DTO 等价」在单测层各自为政永远发现不了，契约层对跑一上来就命中。185 号的目的（把验证从单测提升到契约层）当即自证价值。
2. **带响应语义的路径字段不入契约**：绝对路径泄漏服务器细节且双端天然不等价——契约响应只用相对/语义标识。
3. 大测试套件的偶发红要复跑定位（本次 1 次失败 × 3 次复跑全绿 → 判定偶发非回归；连续定位失败再升级）。

## 四、遗留

- 可选增强按价值排序（待真实写作流反馈再动）：回填内容 diff 预览 > 节拍与章节状态联动着色 > 节拍拖拽排序 > 抽取 prompt 类别扩展。
- 生成端点默认关闭待产品决策。
- 推送须在 Fork 图形端执行（既有约定）。
