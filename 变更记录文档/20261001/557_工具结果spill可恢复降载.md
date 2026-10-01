# 557 号：R42 工具结果 spill——头尾保留+全文落盘+可恢复通知（post 钩子首消费，双端同水位）

日期：2026-10-01
类型：feat(engine)+feat(core)，544 号 dsh 施工图 R42（R39 管线 post 段空集骨架的首消费者）

## 选题来路

R39 落了三段管线与 post 钩子空集骨架，施工图明确「post（PostHook 派链，首消费
R42）」。本专项把超阈值工具结果的降载从「破坏性截断」升级为「可恢复引用」：
agent_loop 既有 8000 字符截断保留为兜底，spill 在管线 post 段先行接管（阈值
6000 字节 < 8000，保证任何场景 spill 先于截断）。TS 侧（Node 回退引擎）此前
**无任何结果截断面**——工具结果全量进上下文，比 Rust 更差，双端同水位补齐。

## 考古（dsh 上游 sparse-clone 源码级 + 本仓实证）

- dsh `spill-policy/src/index.ts`（156 行）：post-execute 钩子挂 token 保留策略；
  `bound()` 超预算 → spillStore.saveText 全文 → 头尾保留+省略通知；**失败降级
  保留 inline**；`exec.name === 'read'` 豁免防回环。
- `retention.ts`：head/tail 对半预算 + fitText 二分贴预算 + surrogate pair
  完整性（textSlice cut ± 1）。
- `notice.ts`：`(Omitted N bytes. Full formatted result stored at: <locator>.
  <hint>)` 通知形态；**通知预留进预算**（worstNotice 超预算即抛）。
- `spill-local/store.ts`：OS tmpdir per-process root、sessionDir=sha256 前
  12 hex、encodeSegment 注入安全、**wx+0600/目录 0700**；retrievalHint
  =「Use read with offset/limit, or grep」——**依赖 dsh read 的 offset/limit
  分页参数，本仓 read 无此参数**（备案后续专项）。
- 本仓实证：read/ls/grep 三件均 books_root（root.join("books")）限定——
  `.inkos/spills/` 默认不可达 → 读回通道需显式放行；ChatToolRouter 构造点
  作用域有 session_id（穿参面单点）；TS ChatToolSetParams.sessionId 在场；
  TS 工具结果无统一截断/降载面。

## 设计定案

- **落盘位置 = `<root>/.inkos/spills/session-<sha256[0..12]>/`**：项目内工件
  树（.inkos/ 已 gitignore；research/ 同族惯例；GUI 可达性）；session-adhoc
  兜底目录（无会话上下文）。dsh 用 OS tmpdir（项目外）——差分器「工件存在性
  对照」要求与 GUI 发现性均指向项目内。
- **阈值与保留**：SPILL_THRESHOLD_BYTES=6000、HEAD=2800、TAIL=1600（UTF-8
  字节；dsh 用 token 预算，本仓无 token meter——字节为双端同构量纲）；通知
  预留进预算（head+tail+GAP+notice ≤ 阈值）。
- **豁免**：read/ls/grep（读回通道工具，spill 其结果会破坏读回语义并可能
  回环）；**错误结果不 spill**（排障信息保持内联，错误少有超阈）。
- **读回通道 = read 工具 `.inkos/spills/` 前缀放行**：tool_read_book（books/
  限定）对该前缀改按项目根 safe_child_path 解析；schema 面不变（参数哈希
  不漂移，差分器 tool-catalog 硬对照安全）；`..` 段仍有 R39 PathTraversalGuard
  前置 + safe_child_path 权威边界（真逃逸拒绝语义不变）。dsh 的 read
  offset/limit 分页本仓未立（读回在 8000 字符截断内有效+grep 可搜索——备案）。
- **PostHook 签名扩展**：`post(ctx, name, res)`——落盘类钩子需项目根/会话
  标识；R39 骨架空集零消费者，签名变更零破坏。default_post_hooks() 注册
  SpillHook（OnceLock 静态）。
- **ToolCtx 加 session_id: Option<&str>**（唯一构造点 agent_route 穿参；
  root_only 兜底链 None → session-adhoc）。
- **TS 同水位**：utils/tool-spill.ts（shouldSpill/sessionDirName/sliceUtf8Bytes/
  spillToolText 四件逐项镜像；dsh fitText 二分+surrogate 修复同构移植）；
  chat-tool-set.ts **buildChatToolSet 统一出口包装**（工厂零改动条款遵守——
  包装不改工厂；豁免件直接透传不包装；只处理单一 text 块结果）；agent-tools
  resolveReadPath 同款前缀放行（projectRoot 穿参）。
- **差分器/duel 不受影响验证**：套件请求均为小结果（不触发 spill）；read
  schema 未变（哈希不漂移）；备案——若未来套件引入大结果用例，差分器工件
  对照需加 spills/ 目录豁免（随机名文件双端自然分叉）。

## 实施面

- 新建 `engine-rs/src/interaction/spill.rs`：should_spill/session_dir_name/
  split_head_tail（char 边界回退）/save_spill（wx+0600、目录 0700、随机
  8hex-工具名）/spill_result/SpillHook + 测试 6 件（判定三维/会话目录确定性/
  边界切分/端到端落盘逐字/落盘失败降级/钩子端到端）。
- pipeline.rs：PostHook trait 加 ctx 参数 + default_post_hooks 注册 SpillHook；
  显式派链测试适配新签名。
- registry.rs：ToolCtx.session_id 字段；agent_route.rs 构造点传参。
- project_tools.rs：tool_read_book 放行分支 + 测试（spill 前缀可达/普通路径
  仍拒/真逃逸仍拒——`..` 消解未越界时「No such file」为正确语义，测试向量
  用 4 层 `..` 真逃逸）。
- 新建 `packages/core/src/utils/tool-spill.ts` + `__tests__/tool-spill.test.ts`
  （7 件含 read 放行端到端）；chat-tool-set.ts 出口包装；agent-tools.ts
  resolveReadPath 放行。

## 门禁（八项全绿）

engine-rs 1442；clippy:gate 双 crate 0 告警；core tsc 0+7 测试；gate:ts 七步
（typecheck/test/audit 0/build/node-fallback-smoke/engine-contract-diff 0 分歧/
export-epub-smoke）；duel 真跑 INKOS_DUEL=1 10/10。

## 教训与备案

- **vitest 5 的 toBe 不收 message 参数**（TS2554）——断言文案改行内注释。
- **spill 后结果尾部是通知不是原文**——endsWith 断言要对准通知收尾；
  写断言先写清结果的完整形态再逐段断言。
- **`..` 消解未越界 = 合法路径**：safe_child_path 拒的是真逃逸，测试向量
  `.inkos/spills/../../x`（消解后在 root 内）返回 No such file 是正确行为，
  别把「拒绝所有 ..」当期望（R39 守卫面拦 `..` 段、工具面拦真逃逸，两层
  语义不同）。
- **read 有两件**（tool_read 项目级 root 相对 / tool_read_book 书会话 books/
  限定）——放行分支只加 books 限定件；测试先确认被测函数的解析根。
- 备案：spill 清理 sweep（dsh cleanupPeriodDays=30 启动扫）未移植（累积慢，
  R43+ 评估）；read offset/limit 分页未立；TS 侧 session id 未传 spill 时不
  落 session-adhoc（buildChatToolSet 恒有 sessionId，实际不走）。
