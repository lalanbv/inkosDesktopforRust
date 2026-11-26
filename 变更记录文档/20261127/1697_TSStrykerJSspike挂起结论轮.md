# 1697 号：TS StrykerJS spike 结论——工具链不兼容，TS 腿变异挂起（不阻塞）

日期：2026-11-27。基线：c92ff046（fix(1695)）。R63 施工图 TS 侧验收项（「StrykerJS vitest-runner 半天 spike，vitest ^5.1.0 已知问题=按测试名过滤可能匹配为空」）的兼容性实测结论轮。docs-only（源码零改动；树上并行会话 1696 的 parity 解析器补强三文件在途，零重叠不碰）。

## ① spike 过程

- 装 `@stryker-mutator/core`+`vitest-runner`+`instrumenter` ^10.1.1（core devDeps）+ `stryker.conf.json`（testRunner vitest、mutate 单文件、coverageAnalysis perTest、concurrency 2、buildCommand pnpm build——fc 语料与测试映射面同 R62 基建）。
- 试跑即崩：**instrumenter 阶段**（未及 vitest runner/按测试名过滤——施工图点名的 vitest5 匹配风险更靠后）`@babel/generator` ts-printer `TypeError: Cannot read properties of undefined (reading 'length')` at `Printer._parameters`（TS 函数参数头打印）。
- 对照实验：换 mutate 文件（usage-budget.ts → writing-goal.ts，两文件分别对应既有 golden-usage-budget 与 properties-native 测试面）**同崩**——工具级不兼容，非单文件语法孤例。

## ② 结论与处置

- **@stryker-mutator 10.1.1 instrumenter 对本仓 TS 源普遍不兼容**（TS 5.x 产出的 AST 与其内置 babel 打印器的函数参数打印路径冲突）。spike 出口按施工图：**TS 腿变异挂起、不阻塞**（Rust 腿 cargo-mutants 已收口 missed 全零——TS 侧变异覆盖缺口由 Rust 镜像函数回放+fc 原生断言+既有单测承担）。
- 处置：Stryker 三件套卸载（依赖恢复原状）、stryker.conf.json 与报告目录删除——零残留。重试条件：@stryker-mutator 升级修复 ts-printer 后重建配置重跑（配置 20 行成本）；届时再实测 vitest5 按名过滤风险。
- R63 状态收官：Rust 腿 missed 全零（基线 scripts/mutation-baseline.json 入库）+TS 腿挂起结论——**变异门 R63 整项完成**（TS 侧按施工图允许的挂起口径）。

## ③ 门禁

docs-only 零源码改动——零门禁（1693 docs-only 先例同例）。依赖卸载后 `git status` 恢复原三文件在途形态（并行会话 1696 不受影响）。

## ④ 出口

下号自 **1698** 起。立案两条：undo 产物逆迁移；对方新立案（路由 parity 机械守卫已机制化 1695、幽灵路由已删 1695——两案均已清）。出口=undo 产物逆迁移、R59a 真壳冒烟（等用户选走查方式）、壳内实机走查与推送（等用户）。
