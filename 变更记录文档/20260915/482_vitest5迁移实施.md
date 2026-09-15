# 482 号：vitest 3.2.7→5.0.0 迁移实施——出清 2 条 moderate 接受风险

日期：2026-09-15　分支：develop　基线：9760a6d2（481 号）

## 选题与决策

473 号备案"vitest 4.x/5.x 双大版本迁移另评估"本号落地。评估结论：**直接升 ^5.0.0**——

- audit 补丁线：vitest/@vitest/mocker 路径穿越 advisory（moderate）补丁 `>=4.1.11`，3.x 线无补丁；
- 运行环境：Node 26.8.2（≥22.12 ✓）、studio vite 6.4.3 / core·cli vite 7.3.5（≥6.4 ✓，vitest 5 vite 为 peer，两拷贝并存但配置面互不相交）；
- 破坏面核查：三包 vitest 配置极简（include/testTimeout/alias，无 workspace/server.deps/coverage）；测试 API 面为 vi.mock/fn/spyOn/waitFor + testing-library，均稳定。

## 迁移破损与修复（44 处类型错 + 5 处运行时红）

1. **`vi.fn()`/`mockImplementation` 箭头实现不可被 `new`**（v4 起著名变更）：proxy-fetch 的 ProxyAgent 桩、pipeline-runner 的 MemoryDB spy、scheduler 的 MemoryDB 桩——改 `function` 形式（构造返回对象语义不变）；
2. **spyOn 泛型收紧**：历史惯用法 `vi.spyOn(X.prototype as never, "m" as never)`（绕 protected 可见性）把 spy 塌缩成 `never`（TS2339 ×44，集中于 writer/reviser/polisher/continuity 等 BaseAgent.chat 系）——新增统一出口 `src/__tests__/spy-loose.ts`（`spyOnLoose`：宽松 MockInstance），35 处单行 + 8 处多行链式机械改写；教训：块注释里写 `mock*/mock.calls` 会提前终止注释；
3. **write-next governed path 用例自递归**（包裹真实 planChapter 的写法在 v5 下 original===spy）：改用同文件 writeDraft 用例的独立合成实现模式；
4. **cli 收集面扩大**（42→84 文件）：vitest 5 默认不再排除 dist——cli vitest.config 显式 `include: ["src/__tests__/**/*.test.ts"]`，dist 编译副本不再重复执行（测试数 232→218 为正确口径）；
5. **`describe.sequential` 移除**：publish-package 测试改 `describe`（文件内默认即顺序执行）；
6. **类型旁路收尾**：server.test 三处 `createShortFictionRunToolMock.mockImplementationOnce` 变体形状与基 mock 不变式冲突（execute `as never` 旁路，运行时不变）；BookTimeline `ReturnType<typeof vi.fn>` 含构造签名→具体回调签名。

## 门禁与出清

- 三包 vitest 5.0.0：**core 2112 + studio 906 + cli 218 = 3236 全绿**（cli -14=dist 副本正确排除）；
- studio `tsc --noEmit` 0 错；core build（含测试类型检查）0 错；studio 生产构建通过；
- `pnpm audit:npm` exit 0：**advisory 10→8，vitest + @vitest/mocker 两条 moderate 出清**（白名单条目移除，恢复拦截）；esbuild low 补丁线 0.28.1 需 vite 7 线跟进，维持备案；
- **cargo test 仍被 Xcode 许可阻断**（本轮开头实测仍 exit 69）——待用户 `sudo xcodebuild -license accept` 后下一轮首件事补跑；clippy/check 不受影响。

## 改动

- `packages/{core,studio,cli}/package.json`：vitest ^3.0.0→^5.0.0；`pnpm-lock.yaml` 同步；
- 新增 `packages/core/src/__tests__/spy-loose.ts`；12 个测试文件破损修复；
- `packages/cli/vitest.config.ts`：include 收敛；`scripts/audit-npm.mjs`：白名单移除 vitest/@vitest/mocker 两项。

## 后续

- Xcode 许可解除后补跑全量 cargo test（481 号欠账）；
- npm 接受风险余 8 条（esbuild 等）滚动跟踪；451 号 TESTING.md 可补 vitest 5 惯例（spyOnLoose/构造桩）。
