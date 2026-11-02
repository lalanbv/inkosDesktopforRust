# 822 号：PromiseTimelineEntry 类型单一真源——studio 本地接口副本收敛

日期：2026-11-02。前置：821 号（时间线 withdrawn 双栈延伸——本地接口需手工双写同步 state，即 816 教训的类型版实例）；816 号（markdown 转义单一真源）。

## 一、盘点

`PromiseTimelineCard` 本地 `PromiseTimelineEntry` 接口与 core `promise-ledger.ts` 导出的同名类型**逐字段重复**（hookId/summary/openedAt/lastAdvancedAt?/expectedPayoff/state/kind?）。821 加 `withdrawn` 时需要**手工双写同步**（漏写即类型漂移+STATE_STYLE/LABEL Record 缺键编译报错兜底）——正是 816 单一真源教训的类型版。

## 二、修复

- core `package.json` exports 增 `"./utils/promise-ledger"`（types+import → dist；build=全量 `tsc` 自动发射，无需枚举入口）。exports 子路径为 fork 既有惯例（688 号「fork 三子路径 exports 回补」、400 号 author-report），加法键合并无摩擦。
- studio `PromiseTimelineCard` 删本地接口，改 `import type { PromiseTimelineEntry } from "@actalk/inkos-core/utils/promise-ledger"`。
- forecast/render 的「换行→空格」另语义副本维持（818 定性）；studio truth-display 的拆分器（814）消费形态不同维持。

## 三、验证

core build 0（dist/utils/promise-ledger.d.ts 在位）；studio typecheck 0（dist 类型解析）；gate:ts 八步全绿（build 含新 exports 子路径解析）；权威 pnpm -r test core 180/988 + studio 137/725 + cli 23/81；cargo:testgate 1980+584 + RustSec 双 0 + clippy 双 0（rust 零触碰 uniformity 照跑）。diff 71/1 维持。

## 四、登记

- 数值分协议扩展（上游面）维持唯一实质余项。
- 类型真源核对：marathon 触及的类型族中，`StoredHook`/`HookKind`（studio 经 `@actalk/inkos-core/utils/hook-kind` 与 promise-ledger 导入）、`PromiseTimelineEntry`（本轮收敛）、`ChapterMeta`（core 内部）均单一真源；truth-display 的 `PendingHook` 为显示专用形（14 列列名映射），不同实体不收敛。

## 五、教训

1. **类型拷贝的漂移成本在"下一次枚举扩展"时显现**：821 的 withdrawn 若本地接口漏同步，STATE_STYLE 的 Record 缺键会在 studio typecheck 兜住——但那是**消费端编译**兜底而非源头设计；类型单一真源让扩展只发生一次。
2. exports 子路径 additions 是 fork 既有惯例（688「回补」/400），加法键合并安全。
