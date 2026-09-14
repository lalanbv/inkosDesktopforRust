# 452 号：CodexPanel 回填缺陷修复 + 交互测试扩展至 Codex/SeriesCanon 表单面板

- **日期**：2026-09-15
- **类型**:fix(studio) + test(infra 扩展) —— 442 缺陷类第二次实例修复与回归锁定
- **关联**：442 号（SeriesCanon 同款修复）、447 号（实体卡走查）、451 号（交互测试基建）
- **提交**：`packages/studio/src/components/CodexPanel.tsx` + 两个 interaction 测试文件 + package.json/lockfile + 本记录

## 1. 坐实并修复：CodexPanel 同款「自动选中不回填」缺陷（442 类第二次实例）

交互测试扩展读码时发现：`CodexPanel.load()` 自动选中首卡（`setSelected(cards[0].name)`）但 **不回填 summary/facts 编辑字段**——与 442 号 SeriesCanonPanel 完全同构。危害链：卡片选中态 + 字段空 → 用户输入新 facts 后「保存卡片」→ `save()` 以空 summary + 新 facts 覆盖整卡 → **原 summary/ facts 静默清空**。

修复（442 同款镜像）：`load()` 选定 `next` 后同步回填 `setSummary/setFacts`；`selected` 已存在时保持用户当前选择。

## 2. 交互测试扩展（基建首扩展，+2 文件 +4 用例）

- **CodexPanel.interaction.test.tsx**：①挂载即自动选中首卡并回填摘要/事实（修复前必挂——红绿验证：还原 HEAD 修复前版两测全红）；②编辑 facts → 保存卡片 → PUT 载荷含新增行且保留原事实。
- **SeriesCanonPanel.interaction.test.tsx**：①442 号自动回填修复的回归锁定（绑定加载后字段回填）；②编辑摘要 → 保存条目 → PUT 载荷含新摘要且保留 facts/aliases。
- mock 模式：`vi.mock("../hooks/use-api")` 按路径路由 fetchJson（GET/PUT 分流、PUT 捕获载荷断言）。

## 3. 门禁

- studio：vitest **100 文件 835 用例全绿**（+2 文件 +4 测）+ `tsc --noEmit` 净。
- 红绿验证：CodexPanel 修复前版两测全红 → 修复版全绿。
- Rust 零改动。

## 4. 开放备案：intent Goal 疑似逗号截断（复核未结）

450 号真机观察：external_context 含「，」时 intent.md Goal 在首逗号处截断。静态复核：`render_intent_markdown` 逐字输出 `intent.goal` 无截断；`derive_goal → extract_first_directive` 返回整行无逗号切分；未定位到截断点（工件已随走查根清理，无法复检）。开放备案，后续循环以新根复现（填含逗号备注 → 写章 → 检查 intent.md 与 planner prompt 全文）后再定性——若为解析缺陷则修，若为设计则文档化。

## 5. 遗留

- **待推送：433–452 共 20 笔**，请用户在 Fork 图形端推送后核验 origin/develop。
- 下一个编号自 **453** 起。
