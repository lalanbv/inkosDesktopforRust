# 421 · 孤儿组件 BookCreate.tsx 复核——确认保留（备案修正）

日期：2026-09-14　性质：轻量质量面复核（402/406 号「清理候选」备案修正）

## 复核内容

402/406 号曾将 `pages/BookCreate.tsx`（1069 行表单式建书向导）备案为
「孤儿组件清理候选」。本批复核后**修正定性、确认保留**：

- 组件完整可编译、可测（`page-state.test.ts` 343 行纯函数测试全绿：
  pickValidValue / defaultBookCreateForm / isBookCreateFormReady /
  buildBookCreatePayload / platformOptionsForLanguage 等表单面纯函数）；
- 定位为**表单式建书备用 UI**——与对话式建书（ChatPage book-create 流）
  互补；R23 伏笔分类、418 面对照等后续批次均以其纯函数为测试锚点，
  无腐化迹象。

删除 1069 行可用备用 UI 的收益（仓容）远小于风险（丢失表单建书形态、
连带 343 行测试锚点）——不符合清理性价比。

## 结论

- 备案修正：`pages/BookCreate.tsx` 由「清理候选」改为「确认保留（备用
  表单建书 UI）」；「孤儿」仅指生产链路未挂载，其纯函数与测试继续作为
  建书表单面的行为锚点。
- 零代码变更，纯复核记录。

## 推送提醒

origin/develop=e555d6f3；剩余积压（402–421，含 7+ 真实缺陷修复）请继续
在 Fork 图形端推送，核验 origin/develop 追平本地 develop（421 提交）。
