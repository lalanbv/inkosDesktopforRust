# 400 号 · 全量回归测试与三处 HEAD 真实缺陷修复

> 背景：09-10 上轮全量验证（271 号）之后，R10–R23 四轮 backlog（376–394 号）与 v5 立项（396–399 号）共 19+ 个批次落地，代码面实质变化，按「全量测试+代码审查+模拟真人测试」目标在新 HEAD（7268f92d）重跑全套门禁。全程与并行会话（R25 接管链）共用工作区，其 WIP 文件一律未触碰。

## 一、门禁总表

| 门禁 | 结果 | 备注 |
|---|---|---|
| engine `cargo test`（INKOS_DUEL=1） | **1766/0 全绿** | 较上轮 1618 新增 148 测试；strangler duel 10/10 真跑 42s |
| engine `cargo clippy --all-targets -D warnings` | **红：38 个 items_after_test_module** | HEAD 真实回归，见 §三.3（交接） |
| src-tauri `cargo test` | 全绿 | 含 wasm 执行 3/3（wasmtime 36） |
| core vitest | **2087/2087 ✓** | |
| studio vitest | **813/813 ✓** | 并行会话新增 2 测试亦过 |
| cli vitest | 初测 **37 红 → 修复后 232/232 ✓** | 根因见 §三.1 |
| studio `tsc tsconfig.server` | 初测 **7 红 → 修复后 0 ✓** | §三.2 |
| studio `vite build`（浏览器端） | 初测 **红 → 修复后 ✓**（5.1s，5438 模块） | §三.2 |
| studio Playwright e2e | **36/36 ✓**（1.8m，worktree 干净 HEAD+修复） | |
| 真实引擎冒烟 | **全绿** | 当前 HEAD 构建，14 端点探测：health/books/详情/章节/context-lens/promises/anti-ai-rules/quality-trend/analytics 全 200 或优雅 404；优雅停机 ✓ |
| bench:gate | **本轮跳过** | 负载 8.0（并行会话连跑构建中），高负载窗口跑基准即噪声（见 bench-gate-load-noise 备忘），其自带负载门禁亦会拒绝 |
| 模拟真人 GUI 走查 | **通过** | 浏览器实操：首页（书卡/R18 备份 UI/R13 数据面板/SSE 实时）→ 进书工作台（面包屑/聊天/上下文面板）→ 无 vite 错误浮层、root 挂载正常 |

## 二、验证方法要点

并行会话实时在改 promise-ledger→HookKind→R25 接管链（WIP 一度 18 文件），live 树门禁被其中间态污染（E0063/TS2724 均系其 WIP）。故在 `git worktree`（/tmp）检出干净 HEAD 分两轮验证（9b033186 与 7268f92d），修复先在 worktree 证实再回 live，最终以 live 树全量门禁收口。

## 三、三处 HEAD 真实回归（均已定位引入批次）

### 1. P0：Node≥22 上 CLI 任何命令启动即崩（368 号 R7 引入）
- `packages/core/src/utils/author-error-catalog.ts:19` 的 `import catalogData from "../data/author-errors.json"` 缺 import attribute；Node≥22 ESM 强制要求 → `ERR_IMPORT_ATTRIBUTE_MISSING`，`inkos --version` 都崩，发布面全灭。
- **修复**：补 `with { type: "json" }`；因根 tsconfig `module: Node16` 不支持 attributes（TS2823），core 的 tsconfig 局部覆写 `module/moduleResolution: NodeNext`（core 源码本就全 `.js` 后缀相对导入，兼容；爆炸半径仅 core）。
- 全仓扫描确认生产代码仅此一处 JSON 相对导入。

### 2. P1：studio 浏览器端构建红 + 服务端 typecheck 红
- **浏览器构建红**：`App.tsx`（`buildTaskReport`，334 号引入）与 `DoctorView.tsx`（`nextActionFor`）从 core **barrel 主入口做值导入**，barrel 把整棵 core 树（writer→atomic-file-set→node:fs/path）拖进浏览器 bundle，rollup 对 `__vite-browser-external` 的命名导入直接报错。core `package.json` 补 `./utils/author-report`、`./utils/author-error-catalog`（顺手补 `./utils/hook-kind` 供下游）、`./utils/hook-kind` 两处深路径导入后构建转绿。`sideEffects:false` 实测对 vite build 无效，未采用。
- **服务端 typecheck 红**：server.ts 三处 import 缺 `.js` 扩展名（asset-library-store/writing-stats/tar-read，R13/R18 引入）、`BodyInit` 裸类型（改用文件内既有 `new Uint8Array(archive)` 惯用法）、3 个 TS7006 为扩展名缺失的级联。修复后 0 错。
- 修复时注意：R23 尾链 UI（396 号）提交的 `PromiseTimelineCard/PendingHooksView/truth-display` 三文件同样存在 barrel 值导入（`HOOK_KIND_IDS/hookKindLabel/normalizeHookKind`），已一并改 `./utils/hook-kind` 深路径。

### 3. P1（交接，未修）：engine clippy --all-targets 38 个 `items_after_test_module`
- 既有各轮 clippy 门禁未带 `--all-targets`，测试目标从未被 lint 覆盖；R 系列批量落库后累积 38 处（`mod tests` 之后定义代码项）。分布：ops_routes.rs×14、agent_loop.rs×5、style_feature_engine.rs×2，其余 14 文件各 1。
- 与并行会话 R25 正在改的文件高度重叠（ops_routes 等），为避免混提本轮只记录不修。修法机械：把 `mod tests` 之后的项移到其前。

## 四、遗留与建议

1. **clippy 门禁升级**：引擎标准建议固化为 `cargo clippy --all-targets -- -D warnings`，否则 test 代码永远在覆盖外。
2. **客户端禁 barrel 值导入**：studio 客户端代码从 `@actalk/inkos-core` 主入口做值导入会拖入 Node 依赖链（已两次踩中），建议 lint 约束或改用深路径。
3. **发布链路验证缺位**：R7/R13/R18/R23 各批落地时 cli vitest / studio build / server typecheck 未跑——这三面恰好是 `pnpm release` 链路，建议并行会话每批至少跑 `pnpm -r test && pnpm -r typecheck`。
4. bench:gate 待负载窗口复跑；R16/R19 仍待真实长跑数据（既有备忘不变）。

## 五、本轮改动文件（路径限定提交）

- `packages/core/src/utils/author-error-catalog.ts`（with type json）
- `packages/core/tsconfig.json`（NodeNext 覆写）
- `packages/core/package.json`（exports 3 子路径）
- `packages/studio/src/App.tsx`、`pages/DoctorView.tsx`（深路径导入）
- `packages/studio/src/api/server.ts`（4 处 typecheck 修复）
- `packages/studio/src/components/PromiseTimelineCard.tsx`、`components/sidebar/PendingHooksView.tsx`、`lib/truth-display.ts`（深路径导入）
