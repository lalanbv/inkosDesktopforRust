# 834 号：孤儿文件清理——governed-context.ts 与 tool-spill.ts（全仓零消费者实证）

日期：2026-11-02。前置：823 号（巡检零新工项——「还有谁在读」跨全部五包实证闭合）发现两个孤儿文件。

## 一、发现

`packages/core/src/utils/` 下两个文件经全格式搜索（import/再导出/子路径/函数名）确认为**孤儿**：
- **`governed-context.ts`**：`buildGovernedMemoryEvidenceBlocks` 全仓零消费者（唯一命中=dist 声明文件）。
- **`tool-spill.ts`**：全部导出（`shouldSpill`/`sessionDirName`/`sliceUtf8Bytes`/4 常量）全仓零消费者。Rust 对偶 `interaction/spill.rs` 为活跃生产实现（交互编辑工具链），TS 副本从未被消费。

删除前安全核查：测试引用仅为数据字符串（pi-stream.test.ts 的 `sourceId:'governed-context'` 非模块 import）；core index.ts 无再导出；无专用测试文件。

## 二、删除

`rm packages/core/src/utils/governed-context.ts packages/core/src/utils/tool-spill.ts`——typecheck 0、pi-stream 29/29、golden 5/5、core 全量 180/988 全绿。

## 三、教训

1. **孤儿文件删前三查**：精确 import（不同路径格式）、再导出（index.ts）、测试引用（数据字符串 vs 模块 import）——三查全零才可删。
2. core build=全量 tsc 自动发射 dist（816 号结论），删除源文件后 dist 中的旧 .js/.d.ts 由 clean-package-dist.mjs 在下次 build 时清理。
