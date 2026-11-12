# 997 号：Mimosa medium 备案定性结案轮——writing-stats 端点「疑似跨文件污点」证伪（零用户输入+惯例形态+纯统计汇）

日期：2026-11-13。开工三查净（HEAD=b96b6cc1、工作区空、无并行会话）。996 备案直系清偿。

## ① 备案面与定性结论

996 轮 commit 前 Mimosa 全项目扫描标记 `server.ts:4370/4371`（`/api/v1/writing-stats` handler 内 `await import("../lib/writing-stats.js")` 及其后的聚合调用两行）「疑似跨文件污点 [medium]」。本轮逐证据定性：**误报，备案结案**。

## ② 三重证据

1. **零用户输入流入聚合链**：`bookId` 来自 `state.listBooks()`（本地 books 目录枚举产物），**非** `c.req.param`——无路径拼接用户面；`rows` 全为磁盘 chapter index 元数据的结构化字段（updatedAt/wordCount/status/totalTokens）；handler 全程无 param/query/body 参与。端点连参数都没有（GET 无参聚合）。
2. **动态 import 是 server.ts 既定惯例**：全文件 **41 处** `await import(`（`@actalk/inkos-core` 惰性加载 ×多、`node:fs/promises`、本端点 writing-stats）——懒加载模块拆分既定形态，4369 行非异常点。收敛为静态 import = 单点破坏 41 处惯例一致性，且无安全收益（模块本地、内容纯函数）。
3. **汇面为纯统计**：`aggregateWritingStats`/`passRateByBook`（lib/writing-stats.ts）对 rows 做 length/计数/取整后经 `c.json` 返回——无危险汇（无 fs/exec/eval/再持久化）。

## ③ 处置：零代码改动，备案结案

启发式触发面 = 「动态 import 跨文件 + 循环累积聚合数据流」形态噪声；按 995 判据纪律（判据收窄、负结果亦是结果），不做收敛性改写（避免惯例破坏与横生枝节），本号文档即定性记录。后续若 Mimosa 对同形态再报，引用本号即可。

## ④ 同族快查（判据内不扩面）

server.ts 其余 40 处动态 import 逐类归群：inkos-core 惰性加载（约 30+）/ node 内建模块惰性 / writing-stats 单点——均为惯例群，无「动态加载后拼路径/执行」形态。style-profiles 写入面（同文件更早区段）已有 957 号 `isSafeBookId` 穿越防线（本轮顺带核实在位）。不展开 41 处逐一深挖（无触发面）。

## ⑤ 门禁

本轮零代码改动（单文档提交）；全门禁引用 996（同树同态）。U+FFFD 净。

下一号自 998 起；出口回归常备；用户侧待办 = Fork 推送（含 955-997）。
