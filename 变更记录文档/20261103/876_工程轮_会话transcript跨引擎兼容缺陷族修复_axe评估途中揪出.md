# 876 号：工程轮——会话 transcript 跨引擎兼容缺陷族修复（axe 门禁评估途中揪出）

日期：2026-11-03。性质：**实质工程轮（真缺陷修复）**。原定议题为「axe headless 真浏览器自动门禁评估」，摸底时跑既有 Playwright e2e 套件，WebServer 日志冒出 `Invalid transcript event … Unrecognized key(s) in object: 'legacyDisplay'`——顺藤摸出一个双栈契约缺陷族，优先修复；axe 评估结论附于第六节。

## 一、根因与收敛方向（[[dual-stack-convergence-direction]] 判定法三步齐）

- 2026-04-27 TS 引入 transcript codec，消息事件展示补充字段名 `legacyDisplay`
- 2026-08-15 Rust 移植会话域（64 号 b940a8f5）——`git show b940a8f5:…schema.ts` 实证当时 TS 确为 `legacyDisplay`，**忠实移植**
- 2026-08-30 上游 2.0（b399a8d3 "replace legacy creative control paths"）**改名为 `display`**（扩为 completion/userInput/thinking/toolExecutions），新增 `visibility` 与 `request_delivery` 事件——**无迁移**
- Rust 从未跟进 → **漂移 rust→TS**（Rust 须跟上游后改）

桌面默认 Rust 引擎、故障回退 Node（164 号），同一项目的 `.inkos/sessions/*.jsonl` 由两个引擎交替读写；engine-contract-diff 各腿只读自己写的文件，**跨引擎读同一文件从未被覆盖**——缺陷由此潜伏两个月。

## 二、缺陷矩阵（全 9 种事件类型族扫）

| 写 → 读 | 症状 | 严重度 |
|---|---|---|
| Rust → Node | `legacyDisplay` 被 TS 严格 schema 拒——**整份会话不可读**，`listBookSessions` 的 Promise.all 连坐**整个会话列表 500** | 高（回退即丢会话） |
| 2.0 前 TS → 2.0 TS | 同上（上游改名无迁移，2.0 读不了自己的旧会话） | 高 |
| Node → Rust | `display` 被 Rust 静默丢弃（工具卡/思考过程回放丢失）；`visibility:"model"` 被忽略（仅模型可见消息漏到 UI） | 中 |
| Node → Rust | Rust 枚举**缺 `request_delivery` 变体**，读取器当坏行跳过 → `next_seq`/`transcript_head` 漏算 → TS 会话以 request_delivery 收尾（每轮必然）时 Rust 追加**复用其 seq 并从其前驱分叉父链**（R36 树化会话损坏） | 高（数据完整性） |
| 族扫其余 | SessionCreated/RequestStarted 的 2.0 可选字段（profileId/workId 等）Rust 读时忽略——append-only 无重写路径、work 域属 695 未迁，**备案** | 低 |

## 三、修复

**Rust（engine-rs）**：
- `Message.display`：`#[serde(rename="display", alias="legacyDisplay")]`——写 TS 认的新名、读兼容旧名；`MessageDisplay` 补齐 completion/userInput（Value 原样保真，读写无损）；**装箱**（clippy large_enum_variant——稀有字段不撑大每个事件，Vec 内存友好，未用 allow 压制）
- `Message.visibility` + `as_message_ref`（仅服务 UI 投影）剔除 `"model"`——TS visibleMessageEvents 同语义，模型恢复路径不受影响
- 新增 `RequestDelivery` 变体（state 原样保真）
- **defense-in-depth**：读取器重构为 `parse_transcript` 同时返回「全部行原始 seq 最大值」，追加 seq 取之——上游再新增事件类型也不会复用 seq
- 10 处构造点 + 1 处解构 + 1 处穷举 match 随编译器逐站更新

**TS（core）**：`normalizeLegacyTranscriptLine` 在 `readTranscriptEvents` 严格解析前把旧名搬到新名（新名优先、空旧名丢弃）——旧名形状恰为 `display` 严格子集；同时救活 Rust 旧文件与 2.0 前本端旧文件。

**前端**：`extractSessionToolExecutions` 回退读 `display` 优先、`legacyDisplay` 兼容。

## 四、双端恒等锁 + 变异证明

三条金标准行（旧名消息 / 2.0 消息含 visibility / request_delivery）在 Rust `mod tests` 与 TS `session-transcript-cross-engine.test.ts` **逐字共用**（程序化核验 IDENTICAL×3）。

| 变异 | 结果 |
|---|---|
| Rust 还原修复前语义（request_delivery 整行跳过 + 按已解析事件算 seq） | 追加回归测试精确红：`left: 4, right: 5`——**活体复现 seq 复用** |
| TS 去掉读边界归一 | 混合文件测试精确红：`Unrecognized key(s) in object: 'legacyDisplay'`——**与 e2e 日志原始报错同一错误** |

恢复后标记清零、全绿。

## 五、验证

| 面 | 结果 |
|---|---|
| Rust 新增单测 | ✓ 6 passed（旧名读+新名写 / 2.0 行无损往返 / request_delivery 往返 / 未知类型 seq 计入 / 追加越过 TS 末行不复用不分叉 / visibility UI 剔除） |
| `cargo:testgate` | ✓ **1990+584**（1984→1990） |
| `clippy:gate` | ✓ 双 crate 0 告警 |
| core transcript 相关 | ✓ 4 文件 42 passed |
| `gate:ts` 八步 | ✓ 全绿（新引擎二进制下契约差分照常） |
| **活体闭环** | ✓ 重跑 e2e command-palette spec（同一份真实 Rust 写出的 test-project 会话）：服务端 transcript 报错 **修复前整段 zod 堆栈 → 0** |

## 六、原定议题：axe headless 真浏览器自动门禁评估（结论）

- **依赖面增量为零**：studio 已有 `@playwright/test@1.61.0` devDep + 本机 Playwright Chromium headless shell（1228）缓存在位 + axe-core（874 已入 devDep）
- **最佳落点**：既有 Playwright e2e 套件（21 spec，`pnpm test:e2e`，Vite dev 4580 + tsx API 4581，健康度实测 2/2 绿 16s）——新增 `e2e/a11y.spec.ts`，复用 webServer 编排；Playwright 真实点击/键盘绕开 IAB 桥退化（[[iab-walkthrough-idioms]] 前三坑在此不存在）
- **不进 gate:ts 默认**：e2e 套件历来 opt-in（启动开销 + 数据播种），gate:ts 已 ~5 分钟
- **待办**：spec 需页面数据（test-project 仅影游、无书无 work）——沿用 e2e/fixtures/seed-*.ts 惯例播种或 page.route 桩；页面集与允许清单（region×2 备案项）照 872/873/875 定

## 七、备案

- TS 读取严格失败语义（一行坏整份拒）与 Rust 跳过语义不对称属上游设计，本轮只修已知根因不改语义
- 2.0 恢复语义（display.completion 覆盖正文 / userInput / blocked 状态）在 Rust 恢复层未实现——695 未迁域，Rust 现保真存储不丢数据
- test-project/.inkos/sessions 未入库（本地产物），无需更新提交夹具

**下一号自 877 起；出口不变；用户侧待办=Fork 推送 8 个提交。**
