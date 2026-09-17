# 518 号：resync 端点活体差分对照——抓获 TS 消费面崩溃缺陷 + smoke 僵尸连接根治

日期：2026-09-17　分支：develop　基线：93f3e23d（517 号）

## 背景

resync 双端架构分歧（483 号备案：Rust 直写 vs TS governed 管线+结构化 delta 机）悬置 35 号无决策依据。本轮把 `POST /books/:id/resync/:chapter` 纳入契约差分器写入面（端点 37→38），用活体对照替代"豁免了事"，为产品决策补实证。

## 差分器扩展（engine-contract-diff.mjs）

1. **resync POST 活体对照**：双端各 POST（fixture 已跑过一次，二次调用兼职幂等性验证）；状态码对照 + 响应体归一化深比（剥 `auditResult.summary` 文案 / `tokenUsage`、`lengthTelemetry` 保存在性剥 LLM 计数 / `contextTrace` TS 独有；`prune` 排键序——双端序列化键序不同，裸 stringify 会假分叉）。
2. **豁免面无豁免复核**：resync 关联三豁免端点（promises / context-lens / roster-candidates）去豁免重照，信息性输出。

## 抓获并修复的真缺陷：TS 实体卡消费面崩溃

差分器首跑即坐实：`PUT /codex` 接受缺 `aliases` 字段的卡（双端 200 原样落盘），随后 resync 在 TS 端 500——`matchCodexCards` 展开 `...card.aliases`（undefined）抛 `TypeError: card.aliases is not iterable`；Rust 端 200（`EntityCodexCard` 全字段 `#[serde(default)]`）。**写入契约与读取容差不一致，UI 建卡 aliases 为空即踩中**。

修复（`packages/core`）：
- `entity-codex.ts` 新增 `normalizeCodexCards`：**消费面**归一化 = Rust serde(default) 的 TS 对应物——补缺省数组/字符串、kind 缺省归 other（不校验取值，Rust 侧为 String）、未知字段（如 Rust 回显的 id）spread 保留、坏卡（无名）跳过。
- `composer.ts` `loadCodexEntries` 读磁盘卡后接线归一化。
- **读写端点保持 Value 原样透传**（双端一致；中途曾试 PUT 落盘归一化，GET 面即与 Rust 分叉——对齐原则：容差只发生在进管线前，不改变落盘形态）。
- 新增 `entity-codex-normalize.test.ts` 3 断言（含修复前必崩的 matchCodexCards 场景）。

## 豁免复核结论（483 备案的实证定性）

三豁免端点无豁免重照**均不一致**，分歧点精确落位：
- `promises/currentChapter`、`roster-candidates/currentChapter`：TS resync 推进 currentChapter，Rust 纯工件重建不推进；
- `context-lens/chapters` 长度 3 vs 2：TS resync 管线产生额外章装配留痕，Rust 不产生。

**定性：真实语义分歧（非环境伪象）**。豁免保留；产品决策三选项（TS 换直写 / Rust 引入状态机 / 维持现状豁免）中"维持现状"的证据面已完备——外部契约主面（resync 响应形状、books/codex 等 38 端点）双端一致，分歧限于 resync 副作用的推进度语义。

## 附带抓获并修复：smoke 僵尸 keep-alive 连接（515 号引入的时序炸弹）

完整门禁中 node-fallback-smoke rust 腿稳定 `ECONNRESET`（cause 诊断定位）。根因：515 号 SIGKILL 后端口秒释放，rust 腿绑**同一端口** 8899，但 smoke 主进程 undici keep-alive 池仍持有 node 腿时代的 TCP 连接——PUT 复用僵尸连接即被 RST（fixture 子进程池独立故独活；ECONNREFUSED=进程死/ECONNRESET=僵尸连接的区分由本轮新增的 cause 输出坐实）。

修复：两顺序切腿套件（node-fallback-smoke / export-epub-smoke）**腿间端口位移** `Number(port) + legs.indexOf(engine) * 10`（node=基口、rust=+10），连接池按 origin 天然隔离。契约差分器为并发双腿（异端口）天然免疫，不动。

## 验证

- 差分器：38 端点 **0 分歧**（含 resync POST 新面）；豁免复核三处不一致为**预期输出**（已定性）。
- node-fallback-smoke 双引擎 13/13 绿（连续 3 次复跑稳定）；export-epub-smoke 双引擎 8 断言绿。
- `pnpm gate:ts` 完整七步全绿（typecheck 16.6s / test 67.9s / audit 2.0s / build 17.4s / 三活体套件）。
- core golden-entity-codex 测试不受影响（7/7）。

## 教训

1. **文本替换型脚本改动须防类型坑**：`argOf` 返回字符串，`"8899" + 0 = "88990"`——端口位移首跑全腿起不来（ERR_SOCKET_BAD_PORT），`Number()` 强转后恢复；改完必须立即跑目标套件而非只 `node --check`。
2. **对齐双端行为先看双端架构**：TS 修复一度写成 PUT 落盘归一化（制造 GET 分叉），重读 Rust 端点确认"读写透传+消费容差"分层后才落到正确位置。

## 关联

- 483/484（resync 备案与基线修复——本轮实证定性）、490（写入面差分方法）、495（错误面对齐先例）、515（SIGKILL——僵尸连接根因引入点）、509（gate:ts——本轮完整档复验入口）。
