# 497 号：死代码清扫——BookCreate 孤儿页面移除（1412 行）与 354 号死接线发现

日期：2026-09-16　分支：develop　基线：ade1edbe（490 号后系）　本轮实际基线：c53f407b（493 号）——注意 495 号提交 dadd7576 也在本基线之前。

## 清扫与发现

402 号备案的清理候选 `pages/BookCreate.tsx`（1069 行）经活体引用面复查坐实为**孤儿模块**：全仓零静态/零动态 import（`isBookCreateChatRoute`/`openBookCreate`/`nav.toBookCreate` 等同名概念均为独立实现，非本模块引用）。连带发现其唯一"消费者"是 `page-state.test.ts`（343 行）——只测该文件自己的 13 个纯函数助手，无运行时消费方。两者构成自封闭死岛，一并移除（净 -1412 行）。

删除后 vite 构建、tsc、全量测试通过——树摇与引用面双重确认无遗漏。

## 重要发现（立案）

**G6/354 号"建书成功 → 导演灵感卡"客户端接线位于死代码中，从未在 UI 生效**：该 PUT 逻辑写在 BookCreate.tsx 页面组件内（402 号起 ChatPage 对话流取代建书 UI，该页面即死）。核查现役建书链：ChatPage/agent 流与服务端建书链（双引擎）均不写导演灵感卡 → 现状=新建书的导演驾驶舱为空态（DirectorPanel 空态优雅降级，无崩溃），用户手动编辑灵感卡保存（PUT）功能正常。

**正确落点专项**：灵感卡应由双引擎建书链服务端写入（Rust：book_create_routes 创建完成点；TS：server.ts 建书成功路径）——语义对齐 354 号原意（stage=directions、inspiration{premise=brief}），失败不阻断。Rust 侧需 cargo；TS 侧需先确认现役建书入口（agent confirm 流 vs REST 建书）后补齐。

## 门禁

- vite 构建 ✓；studio tsc 0 错；
- 全量 `pnpm -r test`：core 2113 + studio 884（-22=死岛自测）+ cli 218 = 3215 全绿；
- cargo 链接仍被 Xcode 许可阻断（481 号欠账 + 489 号 2 新单测持续排队）。

## 后续

- 导演灵感卡服务端写入专项（双引擎）；
- Xcode 许可解除后：481 号全量 cargo test、489 号 2 新单测、duel、bench:gate、skills/genres 移植评估；
- 三库种子 canonical 化待产品拍板；npm 接受风险余 2 条上游钉死项滚动跟踪。
