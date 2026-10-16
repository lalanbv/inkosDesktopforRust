# 734 号：TS 续查第六面——同族弹层焦点归还扩展（QuickOpen/HotkeyCheatSheet）

> 目标线：自主持续开发。732 焦点归还模式的同族实例扩展：QuickOpenPalette
> （Cmd+P）与 HotkeyCheatSheet（快捷键速查）——与命令面板同病的两处
> CommandDialog/Dialog 实例。

## 一、同族审计

| 弹层 | 渲染 | 焦点归还 | 处置 |
| --- | --- | --- | --- |
| 命令面板（⌘K） | CommandDialog | 732 已修 | — |
| QuickOpen（Cmd+P） | CommandDialog | ✗ 同病 | **本批修** |
| HotkeyCheatSheet | Dialog | ✗ 同病 | **本批修** |
| LanguageSelector | Dialog（强制模态） | 设计内（不可 Esc/外点，必须选语言） | 豁免 |

## 二、修复（732 模式同码复用）

两组件各插 lastFocusedRef + handleOpenChange（打开快照/关闭 restore，
onOpenChange 回调时机防 autofocus 污染）+ `onOpenChange={handleOpenChange}`
接线。useRef 导入适配（QuickOpenPalette 并入既有 react import 行；
HotkeyCheatSheet 单独 import 行）。

## 三、验收

- typecheck 双 tsconfig 过；studio 689/689 全绿（QuickOpen 3 测）；
  gate:ts 八步全绿。
- 机制为 732 活体证模式（快照时机/restore 路径）的同码复用——不再重复
  走查（同码即同证）；上游巡检附带复核零落后（第六→七巡间复跑）。

## 教训

1. 同族 bug 的实例扩展批是「模式复用」而非「重新审计」——首例活体证后
   同码实例的验收收窄到 typecheck+测试+gate。
2. 强制模态（LanguageSelector）的焦点不归还属**设计内**——豁免判定要引
   设计依据（P1-3 强制语言选择）。

## 涉及文件

- packages/studio/src/components/QuickOpenPalette.tsx
- packages/studio/src/components/HotkeyCheatSheet.tsx
