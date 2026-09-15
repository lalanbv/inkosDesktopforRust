# 483 号：Node 回退端生产形态真机冒烟——三处分歧修复与一处架构分歧立案

日期：2026-09-16　分支：develop　基线：c6e67f61（482 号）

## 选题与方法

Xcode 许可仍未接受（本轮开头实测 cargo 链接仍 exit 69——**481 号 cargo test 欠账继续挂起，请用户执行 `sudo xcodebuild -license accept`**），Rust 侧验证全阻，故转向不依赖 cargo 的**Node 回退端生产形态真机冒烟**：walkthrough 三件套（mock LLM 1234 + fixture 数据 + 真实 TS 服务器 tsx src/api/index.ts，dist 静态面）全部指向 **TS 服务器端口**——fixture 脚本本身的逐步断言即成为回退端一致性套件。冒烟历经多轮红→改→绿，坐实四处回退端分歧。

## 坐实并修复（三处产品分歧 + 一处走查基建缺口）

1. **resync 基线快照缺失即崩（真缺陷）**：Rust `run_resync_chain` 直接读当前真相文件（`unwrap_or_default` 容错），TS `resyncChapterArtifacts` 强依赖 `story/snapshots/{n-1}/` 基线、缺失即抛——裸 fixture 根（不经建书链直写文件，405/406 号设计）在回退端必挂。修复：基线读取失败回退当前真相文件（runner.ts，注明 483 号）。全仓无测试断言旧行为，红绿无忧。
2. **`loadRuntimeStateSnapshotAtChapter` 违背 IfPresent 契约（真缺陷）**：markdown 回退读取无容错，快照目录整体缺失时抛裸 ENOENT（fs 钩子抓栈定位：`buildRuntimeStateArtifactsIfPresent` ← settle 链）。修复：markdown 基线文件缺失 → 返回 null；调用方 writer 补 null 分支；返回类型 `| null`。新增单测：快照目录缺失 → null（runtime-state-store.test.ts，8 用例全绿）。
3. **SPA 入口与 API 面缺 no-store（469/445 号回退端复发）**：Rust 侧 469（SPA 入口）/445（api_no_store）都有，TS 服务器两处皆无——Node 模式下浏览器缓存旧入口跑旧 bundle、API 读面间歇陈旧。修复：SPA fallback `Cache-Control: no-store`；`app.use("/api/v1/*")` 中间件统一 no-store。活体验证三处头全部到位。
4. **走查 mock 缺 observer 锚点（基建）**：双端 observer 提示词同为"事实提取专家"（逐字移植），mock 分派表无此路由→未匹配兜底分支回 propose_action 工具块→TS 管线判空流致命。补 `事实提取专家` 路由返回无害文本。

## 立案（架构性分歧，非本号可平替）

**resync 双端架构不同**：Rust = 直接真相文件改写（51 号移植期形态，无结构化状态机）；TS = 走 governed 管线 + runtime-state 结构化 delta 机（state-reducer 有防重放守卫：`duplicate summary row` / `goes backwards`）。fixture 裸根场景下，TS resync 即使过了基线关也会在 state-reducer 处拒绝（fixture 预置的 chapter-1 摘要行与 delta 重放冲突）。**对齐方向需专项决策**（TS resync 换直写语义 vs Rust 引入结构化状态机 vs fixture 按引擎分路径），属独立循环工作量。其余端点冒烟全过：director PUT{patch}/GET（478 修复活体验证 ✓，savedChapters+resumeAdvice 注入 ✓）、task-routing PUT/GET ✓、run-log（链路调用计数 ✓）、books ✓、health 为 Rust 单端端点（TS 404，前端不消费，豁免）。

## 门禁

- 全量 `pnpm -r test`：**core 2113（+1 新单测）+ studio 906 + cli 218 = 3237 全绿**；
- studio `tsc --noEmit` 0 错；core build 0 错；`pnpm audit:npm` 8 条全白名单 exit 0；
- 临时 fs 钩子与 instrumentation 已移除，冒烟进程已清理。

## 改动

- `packages/core/src/pipeline/runner.ts`：resync 基线回退；
- `packages/core/src/state/runtime-state-store.ts`：`loadRuntimeStateSnapshotAtChapter` null 语义；
- `packages/core/src/agents/writer.ts`：调用方 null 分支；
- `packages/studio/src/api/server.ts`：SPA/API no-store（+临时打点已移除）；
- `packages/core/src/__tests__/runtime-state-store.test.ts`：+1 单测与既有用例 null 断言收紧；
- `scripts/walkthrough-mock.mjs`：observer 锚点路由。

## 后续

- **用户动作两项**：`sudo xcodebuild -license accept`（解 cargo 门禁）+ Fork 推送 433–483；
- resync 架构分歧专项（上述备案）；npm 接受风险余 8 条滚动跟踪。
