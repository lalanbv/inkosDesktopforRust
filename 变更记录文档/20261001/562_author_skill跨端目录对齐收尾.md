# 562 号：author_skill 跨端目录对齐收尾——560 号遗留计数同步

日期：2026-10-01
类型：fix(catalog)，560 号（R37 Rust 侧 author_skill 清偿）的目录面收尾

## 选题来路

560 号把 author_skill 装配进 Rust 注册表（34→35 注册、31→32 唯一名），
registry.rs 的总数测试已同步 35，但 `server/mod.rs` 的 debug/tools 测试
（31 唯一名）与 TS 目录面三处计数未随批入库——HEAD 上 engine lib 的
`debug_tools_projects_full_registry` 处于红态。本号收尾：author_skill 自
TS 目录 node-only 组移入跨端组（Rust 已清偿，"Rust 走 propose→confirm +
端点"的设计内不对称理由消失），计数全量同步。

## 实施

- `packages/core/src/agent/chat-tool-set.ts`：createAuthorSkillTool 移入
  跨端组（声明序末位=material 后，对齐 Rust registry 装配序）；node-only
  14→13；注释计数 31→32 两处。
- `packages/core/src/__tests__/chat-tool-set.test.ts`：跨端名单锁
  31→32（+author_skill）、node-only 14→13（-author_skill）。
- `engine-rs/src/server/mod.rs`：debug/tools 测试 31→32 唯一名（35 注册
  去重 32）+ 端点文档注释同步（修复 HEAD 红态的主体）。
- `packages/studio/src/api/server.ts`：debug/tools 端点注释
  "31 跨端名+node-only 13 名"→"32+13"（并行会话漏改的第 4 处，全仓
  grep 旧计数清出）。
- 差分器无需改：560 号已删 author_skill 豁免（scripts/engine-contract-diff.mjs
  -2 行），活体 tool-catalog 维度双端 32 件天然绿。

## 门禁

- engine-rs `cargo test --lib server::tests` 10/10 绿（含修复目标）；
- core vitest chat-tool-set 9 + agent-session 51 + author-skill 7 = 67 全绿；
- core/studio `tsc --noEmit` 双 0。

## 教训

- 提交切片要带全同族文件：注册表计数测试（registry.rs）与投影消费测试
  （server/mod.rs）分属两文件但同锁一个事实，只带前者入库= HEAD 红态
  躲过提交时点的定向测试。
- 目录计数是散布面：总数锁（registry.rs 35）、去重锁（server/mod.rs 32）、
  名单锁（chat-tool-set.test.ts 32+13）、注释计数（chat-tool-set.ts×2、
  server.ts、server/mod.rs 文档注）共 7 处，新工具六处同步清单应入
  TESTING/移植纪律。
