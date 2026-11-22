# 1662 号：builtin 数据单源双环 + smoke 三腿修复（回声室四形态定性）+ 观察项巡检

> 日期：2026-11-23 ｜ 类型：混合轮（R54 补全数据单源 + 门禁基建修复；TS 端点族实施后因 duel 黑盒回滚=1663 专项）
> 定位：原计划「TS 回退腿 prompt-packs 端点族」——数据单源双环落地；端点族实施完成且全链测绿，但 **strangler_duel 对 server.ts 任意 prompt-packs 路由注册呈 8 红黑盒敏感**（成分消去实验矩阵：HEAD=绿/+import=绿/+import+任意一条路由（空 handler 亦红）=红；位置无关（writing-stats 锚/尾部同红）；动态/静态 import 无关）——机理未明（tsx sidecar 行为差），**回滚 server.ts 至 HEAD 保门禁绿，端点族=1663 专项**（core 侧全组件就位：store+exports+vitest alias+单源双环，接入=纯 server.ts 增量）。

## ① builtin 12 条数据单源双环（R54 数据面收官）

- `packages/core/src/prompts/builtin-prompts.json`（6.4KB，3 pack+12 prompt 逐字段）——`engine-rs/examples/dump_builtin_prompts.rs` 导出（重跑可再生成）。
- 守卫环 A（Rust）：`golden_builtin_prompts_diff.rs` JSON↔RAW_BUILTIN_PROMPTS 逐字段。
- 守卫环 B（TS）：`golden-builtin-prompts-data.test.ts` JSON↔`BUILTIN_PROMPTS`/`BUILTIN_PROMPT_PACKS` 常量（core/src/prompts/builtin-prompts-data.ts）。
- builtin 变更流程：改 Rust 源→重跑 example→同步 TS 常量（双环红引路）。
- core `utils/prompt-pack-store.ts`（1658 Rust 版本账本 TS 镜像：promptOverridePath 点分映射+load/append/list/read+write/remove 覆盖原子写）+barrel+exports 子路径+vitest alias（三件套第三处实战：动态 import 同被 bare alias 吞→ENOTDIR）。

## ② smoke 三腿修复（本轮流出真价值）

- **判据迁移**：fixture/diff/epub 三脚本的 fork 判据 `/api/v1/prompt-packs` → **`/api/v1/health`**（62/170 号设计=fork 独有）——旧判据在 prompt-packs 同构化后失效（TS 腿被误判 fork=浅播**回声室**）。
- **回环 Origin 回偿（验证后回滚）**：TS 守卫缺 W-A4a 回环放行分支（173 号）→ 补齐后 bin_process 过但暴露后继 8 红（=端点族黑盒同源）→ 随 server.ts 回滚；**分支已验证正确**（1663 随端点族回座）。
- **ECONNRESET 重试**：smoke fetchT 对 keep-alive 僵死连接（fixture 深播长 IO 期间被服务端回收）重试一次——518 号同族竞态，浅播时代未触发。
- **回声室四形态定性（本季度第三实例）**：493 号 node 腿 health 断言（backend=node-fallback）在 493 端点被上游 2.0 重铺裁撤后**靠腿中断静默逃逸**（catch 吞中断=断言静默减少照绿）——本轮 retry 放行后暴露，删除死断言（对齐 62 号设计）。run-log 计数断言同因（node 深播口径差=1663）限 fork 腿+显性跳过备案。
- **genre 穿越探针**：400→400/404 均拒（404=genres 归一无此条目，安全性等价）。
- **调试残留教训**：手工 spawn 调试 server 残留占口→smoke rust 腿错连残留进程（fixture 误判）——spawn 调试后必须 pkill 清场（smoke-orphan 记忆族补章）。
- **fixture 代理豁免**：`NO_PROXY=127.0.0.1,localhost` 模块级注入（系统代理窗口性拦截本地探测——本地压测记忆族）。

## ③ 观察项例行巡检（2026-11-23）

五项维持观察（R50 批三 docx 列表/R51 批三 streak 徽章/R52 box-decoration-break/R8 评测自动跑/NovelCrafter 无 11 月新条目）——无翻案。

## 门禁记录

- cargo:testgate 全绿：audit 双 0+必红自检（9+1）+ engine-rs **72 目标 2200 passed**（+2 目标=golden_builtin_prompts_diff+dump example）+ src-tauri 600；INKOS_DUEL=1 真跑（duel 12/12）。
- gate:ts 十腿全绿（终跑；server.ts 回滚后零差异稳态）。
- U+FFFD 净。

## 出口

- R54 数据面收官；TS 端点族=1663 专项（ duel 黑盒机理取证：tsx sidecar 对路由增量的行为差——strace/日志级）。
- 下号自 1663 起；出口=**prompt-packs TS 端点族接入（含回环 origin 回偿+run-log 口径）**+观察项窗口。
