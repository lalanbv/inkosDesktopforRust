# 824 号：依赖白名单复审——streamdown 族升级与 katex 条目理由更新

日期：2026-11-02。前置：819 号登记（dev minor/patch 批量择机）；823 号巡检（零新工项）。

## 一、白名单复审（逐条对照 registry 现状）

audit-npm 白名单 8 模块 10 条 advisory 逐条复核：
- `vitest`/`@vitest/mocker`（critical+moderate×3）：解除条件=上游升 vitest 4——上游 master 零增量（817/818/819/本条轮四查），**维持**。
- `braces`/`deepmerge`：无补丁线（patched <0.0.0）——**维持**（终态）。
- `esbuild`：vite 6.4.1 配对——vite 6.4.5 仍 6 线，**维持**。
- `zod`（high）：MCP/Anthropic SDK peer 契约——上游 SDK 未动，**维持**。
- `@hono/node-server`（moderate）：1.x 终态 1.19.17 在漏洞区间、补丁线 2.1.3 跨 major——**维持**（819 已升 hono 本体 patch，node-server 1.x 线最新不变）。
- `minimatch`（moderate）：filelist 锁 ^5 / @ts-morph/common 锁 ^10——**维持**。
- **`katex`（low）：条目疑似过期**——树内已有 0.18.2（=登记补丁线）却仍被标记。`pnpm why`/lock 实锤：**双 katex 线**——studio 直依 0.18.2（已修复 ✓）+ **streamdown 2.5.0 拉入的 0.16.45/0.16.47**（漏洞线，advisory 实际来源）。

## 二、streamdown 族升级（dev minor 择机项清偿）

`pnpm -r update streamdown @streamdown/cjk @streamdown/math @streamdown/mermaid`：streamdown **2.5.0→2.7.1**（**2.7 已移除自身 katex 依赖**）+ cjk 1.0.4 / math 1.0.3 / mermaid 1.0.3。
- 结果：streamdown 本体的 katex 拉入消失；**0.16 线改由 `@streamdown/math` 的 `katex ^0.16.27` 契约拉入**（math 插件渲染面）→ katex advisory 仍在（10 条维持），白名单条目**维持**、**理由更新**（「streamdown 依赖 0.16 线」→「@streamdown/math 钉 katex ^0.16.27，streamdown 2.7 已移除自身 katex」）。
- 解除条件不变且更明确：@streamdown/math 升 katex 0.18 线（或 studio 移除 math 插件）时移除条目。

## 三、验证

studio typecheck 0；侧栏组件测试 7/7（SummarySection=streamdown 消费面）；gate:ts 八步全绿（含 export-epub-smoke）；权威 pnpm -r test core 180/988 + studio 137/725 + cli 23/81；audit-npm 10/10 白名单放行 0 拦截。

## 四、登记

- `@streamdown/math` katex ^0.16.27：katex 条目维持的真实来源（解除随 math 插件升线）。
- 其余白名单条目解除条件逐条复核维持（上游 governed）。
- 数值分协议扩展（上游面）维持唯一实质余项。

## 五、教训

1. **白名单复审要追到 lock 实例而非条目文字**：树内 0.18.2（补丁线）与 0.16.x（漏洞线）并存时，`pnpm list` 的单行展示会掩盖双线事实——`pnpm why` + lock grep 才见全貌。
2. 上游 lib 移除传递依赖（streamdown 2.7 去 katex）是白名单清偿的常见路径，但**插件的直接依赖**会补位——清偿判断要看解析后的最终实例集合。
