# 608 号：guardedCompleteStream 双分支单测——成功/失败事件流形态锁定（607 配套）

- 日期：2026-10-02
- 类型：test(core)
- 路线：607 号非流式适配器的回归锁定
- 状态：✅ 门禁全绿收口

## 交付

`guarded-complete-stream.test.ts` 两 case（vi.mock 打桩 piCompleteSimple）：

1. **成功分支**：resolve → 事件流推 `done`（message 透传）+ end；
2. **失败分支**：reject → 推 `error` 事件（流以 error 终结，无 done 收尾——pi-ai 事件流惯例实测确认）。

## 过程

事件收集从 setHandler/result 改为 `for await` 异步迭代器（AssistantMessageEventStream 无 setHandler 接口——首次尝试失败后按实际接口修正）。

## 门禁

gate:ts 七步全绿（core 测试含新 2/2）。

## 下一号自 609 起
