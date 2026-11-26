# 1695 号：路由 parity 机械守卫 + 幽灵路由处置（对方 1693 立案 (a)(b) 清偿）+ R63 基线入库

日期：2026-11-27。基线：dabdb024（fix(1694)）。主树直开。

## ① 路由 parity 机械守卫（对方 1693 立案 (a)）

`scripts/check-route-parity.mjs`：
- 解析 TS `app.<method>("字面")`（1693 实测 .on/.route/反引号零别形）+ Rust mod.rs `.route("字面", <handler>)`——**method 位不参与 parity**（Rust 大量路由的 method 位是 handler 变量名非字面，路径集合才是稳定可比面——首版含方法比对误报 71 条 TS-only，改路径集合后差集与 1693 定性完全吻合）。
- 通配符归一：`:x{.+}` ≡ `*x` **整段收成统一占位符** `*WILDCARD`（段名只是占位语义，两腿通配段名不同名也是等价路由——首版按名配对被自检绿例咬出）。
- 不对称基线表 `golden/route-parity-baseline.json`：rustOnly=10（health 单腿设计+9 惰性路由，与 1693 定性逐条吻合）、tsOnly=5（fallback 通配 `*`/assets、sessions branch/branches 单腿双打、writing-stats 主打）——**TS 侧含已删幽灵路由前的形态基线在幽灵路由删除后重生成**（删除使 tsOnly 少一条）。
- 模式：比对（新不对称 exit 1）/`--update`（有意变更重写基线）/`--self-check` 四夹具（847 号精神：绿例一致零差/必红 TS 单腿假路由检出/通配归一活体/解析死链哨空集 exit 3）。
- **接入 gate:ts 第十二腿**（route-parity）——审计腿常驻（1671 号「检测器不在门禁里=红了几十轮没人看」先例）。

## ② 幽灵路由处置（对方 1693 立案 (b)）：删

`GET /api/v1/projects/:id/preview/html`（自家 `/export/html` 的转发壳；1693 考古=2.0 上游 7e60e13f 带入、studio/CLI/Rust 三面零消费）——删除路由块（本域 grep 复核零消费）。删除后 tsOnly 基线少一条、parity 比对绿。

## ③ R63 基线快照入库

`scripts/mutation-baseline.json`：四文件（chapter_filename 14/0、usage_ledger 42/0、chapter_workspace 99/0、audit_route 3/0）missed 全零快照+使用说明（`mutation-gate --files <新触及> --baseline scripts/mutation-baseline.json`——missed 只许下降）。首版范围=施工图点名的解析与判定类纯函数；扩充新文件重跑并入。

## ④ 测试与回归

- 守卫自检四夹具全过；比对模式 exit 0（删除幽灵路由后的树上无新不对称）。
- 删路由回归：core 结构套件 30 绿、studio 结构路由+覆盖守卫 6 绿、`pnpm -r typecheck` 净。
- 门禁终跑：**gate:ts 十二腿全绿（含新 route-parity）**／testgate duel 真跑 22/豁免 2 地板 22、engine-rs 81 目标 2370、src-tauri 600／clippy 0／fmt 0。

## ⑤ 出口

下号自 **1696** 起。R63 收尾仅剩 TS StrykerJS spike（vitest 5 兼容性实测——不兼容则 TS 腿变异挂起不阻塞）。立案：undo 产物逆迁移；对方 1693 新立案 (b) 已清、(a) 已机制化。壳内实机走查与推送（等用户）。
