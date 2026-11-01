# 802 号：hooks 落盘插桩定位——saveChapter 写者确认与 sc1 注入窗口定界

日期：2026-11-01。前置：801 号登记（openedAt/lastAdvanced 覆写疑云——state hooks startChapter 全被覆写为 sync settle 的 delta.chapter，静态分析无法定界）。

## 一、插桩落地（env 门控永久观测件）

`commitAtomicFileSet`（atomic-file-set.ts）加 WALKTHROUGH_MOCK_DEBUG=1 门控日志：hooks.json 落盘时打印 **sc 值 + 调用栈片段**——覆盖一切写入者（此前只插桩 saveRuntimeStateSnapshot 证实其零调用=写入者另有其人）。

## 二、定位结果（/tmp/wtestC 活体）

- **写入者=WriterAgent.saveChapter（writer.ts:492）**，两条调用路径：`_resyncChapterArtifactsLocked`（runner.ts:1929）与 `persistChapterArtifacts`（chapter-persistence.ts:42）。
- 落盘值：sc=H04:1,H01:1,H02:1,H03:1——**全部=sync settle 的 delta.chapter（1）**。
- **核心管线无罪实锤**（决定性单元实验）：空基线 + delta upsert（H01 sc1/H04 sc3）→ buildRuntimeStateArtifacts（arbitrate+reducer）→ resolvedDelta/snapshot **startChapter 完整保留**（H01 sc1、H04 sc3）——sc1 注入发生在 saveChapter 之前的 **settle-decode→arbitrate 窗口**（pi-ai 工具 decode 或 arbitrate 输入侧）。

## 三、sc1 注入窗口定界（剩余一步）

注入窗口=「settle 工具 TypeBox decode 结果 → arbitrate 输入」之间。下一轮插桩点：writer.ts settle 的 decode 结果日志（result.hookOps.upsert 的 sc 值）——若 decode 即 sc1=prompt-ai 工具参数规范化；若 decode 后 3 而 arbitrate 后 1=arbitrate 输入侧改写。单点插桩即可闭合。

## 四、mock 对齐固化（800 号 ledgerUpserts 保留）

settle 工具支 upsert=4 条 ledger 终态对齐记录（H01-H03 lastAdvanced 3、H04 台账原始）——与 rust ledger 终态一致的正确形态保留；sc1 注入与 mock 无关（args 携带 3/2 被中途改写）。

## 五、验证

- 插桩活体：/tmp/wtestC 探针捕获 saveChapter 两条调用路径 + sc 落盘值。
- core 979/979 + 权威 pnpm -r test 三包全绿 + tsc 归零 + clippy 双 0 + testgate 1956+584 零漂移 + gate:ts 八步全绿。

## 六、教训

1. **插桩要放在公共汇点**：saveRuntimeStateSnapshot 零调用实锤后，把插桩移到 commitAtomicFileSet（一切状态落盘的必经汇点）+ 调用栈片段，一次定位写入者——逐路径猜测试远慢于汇点捕获。
2. **env 门控插桩是合法永久观测件**：WALKTHROUGH_MOCK_DEBUG 家族（mock 兜底/state-save/atomic-commit）三件套构成 diff 环境的可观测面，后续轮次复用。
