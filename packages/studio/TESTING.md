# studio 测试与走查惯例

> 汇总 441–465 号循环沉淀的测试基建与惯例（2026-09-15 收口）。新增组件/页面/链路时按本档随实现随测。

## 1. 测试分层

| 层 | 位置/命名 | 环境 | 用途 |
| --- | --- | --- | --- |
| 纯函数单测 | `*.test.ts` | node | 解析/聚合/派生逻辑 |
| API 行为测试 | `src/api/server.test.ts` | node | 端点契约（mock 白名单须显式登记新导出） |
| 组件静态冒烟 | `*.test.tsx`（renderToString） | node | 渲染结构快照断言 |
| **交互测试** | `*.interaction.test.tsx` | **jsdom**（文件级 docblock） | 用户操作时序/回调载荷 |

- 交互测试文件首行必须 `// @vitest-environment jsdom`；全包其余测试保持 node 缺省（106+ 文件零扰动）。
- 字段定位用组件惯例 **`data-slot`**（`document.querySelector('[data-slot="…"]')`），测试专有挂钩用 `data-testid`。

## 2. 交互测试惯例（踩坑沉淀）

1. **`vi.fn` 实现必须带参**才能断言调用参数——零参 mock 触发 TS2554（458 号）。
2. 渲染后推 store/状态变更须包 **`act(...)`**——否则 zustand 订阅的 DOM 刷新不落地（459 号）。
3. 恢复/删除类 confirm 守卫：jsdom 无原生 `window.confirm`，须 `vi.spyOn(window, "confirm")` 打桩（462 号）。
4. `useApi` 与 `fetchJson` **分双 mock**：前者返回固定快照，后者按路径路由并捕获载荷（457 号模式）。
5. `userEvent.upload` 可直注隐藏 `input[type=file]`，绕开文件选择器（455 号 BackupPanel）。
6. 每用例 `cleanup()` + mock 计数清零；异步拉取用 `vi.waitFor` / `findBy*`。
7. **radix/cmdk 系组件两桩**（jsdom 缺失即渲染崩）：`ResizeObserver`（空 `observe/unobserve/disconnect`）与 `Element.prototype.scrollIntoView`（空函数）——参考 QuickOpenPalette.interaction.test.tsx 头部（471 号）。
8. 多处出现的文案用 `getAllByText(...).length` 断言；文本被子元素拆分时用 `findByText` 函数匹配器或直接查 `textContent`。

## 3. 路由：PAGE_SPEC 单一事实表（461 号）

- 新增页面的 hash 写入/解析/深链全部收敛在 `src/hooks/use-hash-route.ts` 的 **`PAGE_SPEC`**（mapped type 键穷举 `HashRoute["page"]`）——漏配 = TS2322 编译错。
- `writable: false` = state-only 页（不写 URL，但 parseHash 深链分支仍需手写）。
- round-trip 穷举测试（`use-hash-route.test.ts`）锁定 `parseHash(toHash(sample)) === sample`；新增页面同步补 sample 即可被覆盖。

## 4. 走查脚手架（scripts/，头注即文档）

```bash
node scripts/walkthrough-env.mjs --root /tmp/inkos-walkXXX   # mock+引擎+fixture 一键
WALKTHROUGH_MOCK_FAIL_ARCHITECT=1 node scripts/walkthrough-mock.mjs  # 架构师链失败注入
```

- fixture 幂等：复用根（write-next 章已落盘）自动走只读断言路径（440/442 号）；
- fixture 书已补 `story/story_bible.md`——同名建书走 409 不再清空数据；story_bible 在场会进 write-next 装配（13 条基线），勿用旧基线断言；
- 引擎内存缓存启动时 dist/index.html——**重建 dist 后须重启引擎**（449 号已修为 mtime 热加载，重启仅为保险）。

## 5. 门禁清单（提交前）

```bash
pnpm gate:ts                        # TS 全门禁一条命令（509 号：typecheck/build/test/audit+三活体套件；--fast 跳过 build 与套件）
pnpm -r test && pnpm -r typecheck   # TS/Studio（gate:ts 的组成项）
pnpm clippy:gate                    # Rust 双 crate -D warnings
pnpm audit:rust                     # RustSec（含 --strict 可选）
pnpm verify:engine-bindings         # ts-rs 导出锁
INKOS_DUEL=1 cargo test --test strangler_duel   # 契约面真跑（engine-rs/ 下）
node scripts/bench-gate.mjs         # 性能基准（静默窗；严禁 --update 洗基线）
node scripts/node-fallback-smoke.mjs            # 回退端+双引擎一致性（485/486 号）
node scripts/engine-contract-diff.mjs           # 双引擎 GET 契约活体差分（487/492 号）
```

## 6. 已知约束

- npmmirror registry 无 audit 端点：`pnpm audit --registry=https://registry.npmjs.org/`。
- API 响应统一 `Cache-Control: no-store`（445 号中间件）——跨会话陈旧遥测已根治，新端点自动继承。
- 引擎侧 `INKOS_LLM_CONTEXT_WINDOW` env 可显式覆盖上下文窗口（缺省按模型卡两层查，miss 128k）。
