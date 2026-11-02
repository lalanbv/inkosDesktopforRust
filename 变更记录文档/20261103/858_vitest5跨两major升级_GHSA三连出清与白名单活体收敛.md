# 858 号：vitest 3→5 跨两 major 升级——GHSA 三连出清与 audit:npm 白名单活体收敛

日期：2026-11-03。性质：**出口触发的实质工程轮**——backlog 登记出口「vitest 升级消除 GHSA-5xrq/82fw/qxg6 三连」真实开门（vitest 5.1.0 已发布、peer 前置满足），非例行巡检。

## 一、出口核查（本轮开工三查）

| 出口 | 状态 |
|---|---|
| 上游 | `fetch --prune` **0 新**——原白名单理由「随上游跳线」不会自行到来 |
| vitest | **5.1.0 已发布**（4.1.12=三连补丁线早已越过）；peer `vite ^6.4.0 || ^7 || ^8` 被树内 vite 6.4.5（override `vite@<7→^6.4.5`）**满足**；mocker peer `msw ^2.4.9 \|\| ^3` 被树内 2.12.13 满足 |
| serde_json（820 号钉定） | **仍关闭**：crates.io latest = 1.0.154（无 >1.0.154 发布），1.0.151 性能钉定维持 |

## 二、改判记录（688 号「随上游跳线」→ fork 侧独立升级）

原裁决语境：688 号上游 2.0 合并期，跨 major 跳线以「版本对齐上游发布契约」为由推迟。本轮改判依据：

1. **dev-only 依赖与发布契约解耦**——package.json 一行差异对上游合并成本可忽略（与 core/studio/cli 的 2.0.0 发布版本号无关）；
2. **持续再定性负债是真实的**——三条 advisory 每轮 audit 都要维持定性（854 号就为此补钉过 GHSA-qxg6），消除优于接受；
3. 上游 0 新意味着「随上游」=无限期搁置，而消除条件（peer 兼容）已全部满足。

## 三、变更面（5 文件）

1. **三包 vitest `^3.0.0`→`^5.1.0`**（core/studio/cli devDependencies；package.json 不写注释——JSON 严格语法，首踩即改，文档归位本档与 audit 脚本注释）。
2. **pnpm-lock.yaml 重生成**：vitest/mocker 5.1.0，`grep vitest@3.2.4` 零残留。
3. **audit-npm.mjs 白名单摘三钉定**（9 模块 11 id → 6 模块 6 id，恰等活体接受集）：
   - vitest（GHSA-5xrq/82fw）、@vitest/mocker（GHSA-82fw/qxg6）——三连随补丁线出清；
   - braces（GHSA-vfj7）——**休眠条目出清**：857 号实报 10 条即无 braces（树内 3.0.3=latest，公告面上游侧更新后不再命中），非本轮变更引起；
   - 自检 4 夹具全用合成模块（left-pad/katex/zod 伪 id），无需改动。
4. 三份 vitest.config.ts **零改动**（只用 include/maxWorkers/testTimeout/fileParallelism/alias，无废弃选项）。

## 四、验证

| 面 | 结果 |
|---|---|
| 暴露面预扫 | `.sequential` 0 / 已移除入口点（`vitest/mocker` 等）导入 0 / 空 `toThrow("")` 0 / 快照文件 0 / fake timers 仅 2 文件（无 Temporal） / Node v26.13.0 ≥22.12 ✓ |
| core 套件 | **998/998**（180 文件）全绿 |
| studio 套件 | **725/725**（137 文件）全绿 |
| cli 套件 | **81/81**（23 文件）全绿 |
| v5 破坏面（clearMocks 默认开/嵌套 vi.mock 硬抛/pretty-format 快照形变/未 await 断言即败） | **零现形**——零测试代码改动通过，既有测试卫生（per-test 断言、顶层 hoisted）本就合规 |
| audit:npm | advisory **6 条全白名单放行拦截 0** + 必红自检 4 夹具 ✓（放行 6=活体 6，无休眠残留） |
| gate:ts 八步 | **全绿**（build 17.8s / typecheck 16.5s / test 115.1s / audit:npm 3.9s / rust-bin 0.4s / 三冒烟腿 35.8+36.3+35.4s） |
| Rust 门禁 | 不重跑（853 事件驱动裁决：本轮零 Rust 面变更，lock 仅 pnpm；857 全电池仍新鲜） |
| bench | 不重跑（dev-only 依赖与审计脚本注释，零生产代码/热路径变更） |

## 五、教训

- **出口登记要配「解除条件实查」而非静待**——「随上游」类出口若不主动核查外部状态（vitest 已到 5.1.0、peer 已兼容），会无限期滞留；本轮开工三查（上游/vitest/serde_json）即出口巡检的最小动作集。
- package.json 是 JSON 不是 JSONC——行注释首踩 pnpm `ERR_PNPM_JSON_PARSE` 即改；变更叙述归档在变更记录与脚本注释，保持 package.json 纯净也贴上游合并面。
- 白名单收敛方向：**条目集应恒等于「当前报告面 ∩ 已定性集」**——休眠条目（公告不再命中）按「移除即恢复拦截」语义出清，再现时走红并重新定性，fail-closed 不变。

## 六、出口（更新）

GHSA-5xrq/82fw/qxg6 三连**已消除**；白名单余 6 条全部活体（esbuild/katex/zod/deepmerge/@hono/node-server/minimatch，消除条件各自登记在条目注释）。剩余出口：上游数值分协议扩展 / serde_json >1.0.154（本轮实查仍关闭）/ @streamdown/math katex 0.18 / pi1.0 key / 新真实需求 / 下一巡检周期。

**用户侧待办：Fork 推送 2 个提交（aa50b132=857 + 本轮）。**
