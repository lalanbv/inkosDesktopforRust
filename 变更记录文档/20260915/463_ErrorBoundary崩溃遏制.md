# 463 号：ErrorBoundary 顶层崩溃遏制——组件渲染错误不再白屏整个应用

- **日期**：2026-09-15
- **类型**：feat(studio) —— 桌面应用健壮性改进
- **关联**：449 号（走查实测 root 空挂载白屏形态，本号动机来源）
- **提交**：`packages/studio/src/components/ErrorBoundary.tsx`（新）+ `main.tsx` + 交互测试 + 本记录

## 1. 缺陷与修复

449 号走查实测：任意组件渲染错误会让 React 卸载整棵树 → **root 空挂载白屏**，用户只能手动刷新（刷新还依赖引擎可用）。App 无顶层错误边界。

修复：新增 `ErrorBoundary`（class 组件，`getDerivedStateFromError` + `componentDidCatch` console 留痕），挂载于 `main.tsx` 根部包裹 `<App />`。渲染错误降级为受限错误卡：「界面遇到了问题」+ 错误消息 + 「重试」按钮（清零错误态重新渲染子树）。**刻意不依赖 i18n/app-language 等模块**——崩溃源可能在任意 import 链上。

## 2. 测试（+1 文件 +2 用例，全绿）

1. **降级断言**：抛错子组件 → `error-boundary` 测试位出现 + 「界面遇到了问题」+ 错误消息可见（console.error spy 验证留痕）；
2. **重试恢复**：错误态清零后子组件恢复正常内容渲染。

## 3. 门禁

- studio：vitest **107 文件 863 用例全绿**（+1 文件 +2 测）+ `tsc --noEmit` 净 + dist 重建（边界已进产物）。
- Rust 零改动。

## 4. 遗留

- **待推送：433–463 共 31 笔**，请用户在 Fork 图形端推送后核验 origin/develop。
- 下一个编号自 **464** 起。
