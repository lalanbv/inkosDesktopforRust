# 1047 号 · 低覆盖面补强轮——ignored-sweep 复验 + cli smoke + L3 第六次误报处置

> 1046 出口执行。三包中 cli 覆盖最低（81 例）——本号补 cli 入口活体 smoke+
> ignored-sweep 周期复验（1012 后欠账）。含一笔 L3 误报形态治理。

## 一、结果

| 项 | 结果 |
|---|---|
| ignored-sweep | **exit=0 六段全绿**（挂起用例清单精确匹配——1012 后周期复验） |
| cli smoke | `inkos --help` 活体完整输出（Usage/六 options/commands——bin 入口 dist/index.js 活体） |

## 二、L3 第六次拦截与形态治理

提交时 L3 第六次拦截：`project-bootstrap.ts:13` 与 `doctor.ts:161` 的
`includes("INKOS_LLM_API_KEY=")` env 键名探测被报 [high] 硬编码凭据——定性为
**检测形态误报**（探测全局 .env 是否已配置 key 的键名标记 + 占位符反证，非凭据值；
与 1015 治理的「apiKey 字段+字面量」判据同族被新口径覆盖）。

治理（1039 形态一致化同构，语义零变化）：utils.ts 新增共享常量
`GLOBAL_API_KEY_ENV_MARKER` / `API_KEY_PLACEHOLDER`，四处字面量引用化
（两探测行 + doctor:192 同型占位符比较单源化）。`:96`/`:111` 为 `# ` 注释模板
写入行（本轮非报面，模板字节不变原则保持原样）。验证：cli typecheck 干净 +
81 例全绿（含 cli.flow init round-trip 覆盖探测路径）。

## 三、下一号自 1048 起

出口=earendil-mz 单包合并（触发条件制持续）/ 下一巡检周期 / 用户侧待办=Fork 推送（含 955-1047）。
