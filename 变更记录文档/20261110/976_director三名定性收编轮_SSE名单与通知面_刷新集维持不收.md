# 976 号 · director 三名定性收编轮：SSE 名单 + 通知面，刷新集维持不收

> 965 备案清偿（C 类 11 名余下的 director:start/complete/error 三名逐事件定性）。
> 定性复核推翻「不收」结论为**部分收编**：与 radar 三名完全同构（无 bookId 的
> LLM 长任务），radar 已收名单+通知表，director 却名单外=发射面盲区+长任务完成/
> 失败无人知晓。验证：use-sse 6/6+use-book-activity 12/12+author-report 10/10
> （+1 例）+gate:ts 十步全绿（含 pnpm -r test 全量）+U+FFFD 净；本轮零 Rust 改动
> （双端发射面已在位，Rust 门禁面引用 975 轮 testgate 全绿——树同则面不变）。

## 一、逐事件定性复核（director 三名）

| 维度 | 事实 |
|---|---|
| 发射点 | 双端各一：POST /director/directions（Rust ops_routes 1070/1130/1136；TS server.ts 5358/5368/5371）——R4/364 号端点、508 号 UI 接线 |
| 负载 | start={} / complete={count} / error={error}（**无 bookId，双端同形**） |
| UI 视图 | DirectorPanel 自带 generating 态按钮（发起方自洽，不依赖 SSE）；无其他视图消费方向候选；agent 注册表无 direction 工具=代理触发通道不存在 |
| 频度 | 低频（人工按钮，LLM 长任务 30s+） |
| 缺口 | ①用户点生成后切页再回，组件卸载 generating/directions 态丢失，不知前次完成与否；②发射面在 STUDIO_SSE_EVENTS 名单外=前端合法接收面盲区 |
| 定性 | **收编名单+通知表（complete/error 两名），start 不入通知表（零开始噪音，radar:start 同例）；刷新集维持不收（无 bookId——965 断言保留）** |

## 二、修复

- `use-sse.ts`：STUDIO_SSE_EVENTS +3 名（radar:error 之后，976 注释点名同构定性）。
- golden `studio-sse-events.json`：同步 3 名（顺序一致——toEqual 精确快照含序）。
- `author-report.ts`（core）：resolveTaskLabel 表 +director:complete「方向候选生成
  完成 / Direction candidates ready」+director:error「方向候选生成失败 / Direction
  candidates failed」；buildTaskReport 完成类新增 director:complete 专门分支——
  负载 {count} 数字→「N 个方向候选已生成」（en 同形），缺 count 降级人话（禁裸
  JSON 面不变）；error 形态走既有三分类（classifyEngineIssue+建议确认）。

## 三、测试与必红两形

- author-report +1 例「direction-candidate 三形」：start 不出报告（null）/
  complete{count:3} count detail（zh/en）+缺 count 降级+error 三分类 warn。
- use-book-activity 965 director 断言扩为三名全 false+书目 false（注释更新：定性
  复核维持刷新不收、可见性走名单+通知面）。
- use-sse.test covers 例 +3 名。
- **NEUTER976**：中和名单 3 名 → 恰 2 红（golden toEqual 精确快照+covers
  arrayContaining）；撤除后 6/6。
- **stash 形**：stash use-sse.ts+golden+author-report.ts → 恰 2 红（golden 快照+
  author-report director 例）；pop 后 28/28。

## 四、真引擎活体（8977，mock 18976）

- SSE 流（GET /api/v1/events）后台连着，POST /director/directions 生产形态
  （inspiration{premise,keywords}+count=3）→ 200 两候选（mock「故事导演」真分派）
  +SSE 流真实收到 `director:start` 与 `director:complete` 负载 `{"count":2}`（与
  通知分支读键一致）。
- error 形态：杀 mock 再触发 → 500+SSE 流收 `director:error` 负载
  `{"error":"HTTP 错误: …"}`（与通知表 error 读键/三分类一致）。
- 环境清偿：mock/引擎杀净、临时根删除、stash 栈 0。

## 五、门禁

- gate:ts 十步全绿（第 3 步 pnpm -r test=core+studio+cli 全量；diff 78/0）；
  U+FFFD 净；Rust 树零改动（git status 实证）——clippy/testgate/audit 面引用
  975 轮全绿（965 先例：树同则面不变）。

## 六、备案

- director:start 双端负载 {}——后续若需要「生成中」跨页提示，走名单已有面
  （use-book-activity 的 activity 环形缓冲），无需引擎改动。
- radar:start 同样在名单不在通知表——start 噪音面纪律先例成立，director 遵从。

**下一号自 977 起；出口=EPUB 字节级细节（如需）/下一巡检周期；用户侧待办=Fork
推送（含 955-976）。**
