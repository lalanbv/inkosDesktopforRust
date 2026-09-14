# 455 号：BackupPanel 交互测试——预览解包修复回归锁定（443 修复无测试覆盖收口）

- **日期**：2026-09-15
- **类型**：test(infra 扩展) —— 预览解包修复的回归锁定
- **关联**：443 号（备份 Rust 移植 + BackupPanel 预览解包修复）、451/452 号（交互测试基建）
- **提交**：`packages/studio/src/components/BackupPanel.interaction.test.tsx`（新）+ 本记录

## 1. 测试内容（+1 文件 +1 用例）

`BackupPanel.interaction.test.tsx`（jsdom 环境，vi.mock fetchJson 按路径分流预览/确认）：

1. **预览数字可读断言**：`userEvent.upload` 直接向隐藏文件输入注入 File（绕开 IAB/浏览器文件选择器不可自动化限制，jsdom 内合法）→ 断言渲染「包内 3 个文件：新增 2，将覆盖 1」+「将覆盖：books/b1/book.json」——**443 号修复前该数字为 undefined**（平铺读取嵌套 `{preview:{...}}` 线格式），此断言即回归闸。
2. **确认恢复断言**：点击「确认恢复」→ 二次调用带 `confirm=1` → 成功通知「已恢复 3 个文件（原文件快照在 backups/）」；调用序列断言恰好 2 次 import 调用且含 confirm=1。

## 2. 门禁

- studio：vitest **101 文件 836 用例全绿**（+1 文件 +1 测）+ `tsc --noEmit` 净。
- 交互基建三连扩展完成：TimelineEditDialog（451）→ Codex/SeriesCanon（452）→ Backup（本号）。

## 3. 遗留

- **待推送：433–455 共 23 笔**，请用户在 Fork 图形端推送后核验 origin/develop。
- 下一个编号自 **456** 起。
