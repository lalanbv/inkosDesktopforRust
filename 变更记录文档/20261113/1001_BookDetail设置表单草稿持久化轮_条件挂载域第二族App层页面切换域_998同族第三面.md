# 1001 号：BookDetail 设置表单草稿持久化轮——条件挂载域第二族（App 层页面切换域）+ 998 同族第三面

日期：2026-11-13。开工三查净（HEAD=39905e68、工作区空、无并行会话）。

## ① 域扩展：条件挂载族从 dock 域到 App 层页面切换域

999 普查收口了 dock 域（visible 条件渲染）。本轮扩展到 App 层页面切换域：`view.page === "book-settings"` 条件渲染下的 **BookDetail**（1086 行，20+ useState）——页面切换卸载组件，逐 state 分级定性：

- **settings 表单三件**（`settingsWordCount/TargetChapters/Status`，覆盖层语义 `settings* ?? book.*`）：页面切换重置 null 回落真源旧值 = **用户未保存的表单修改丢失**——998 同族第三面（数据丢失级）。
- `reviewMode/autoBeats`：**非缺陷**——有服务端真源（fetch 初始化 + API 持久化 + 失败回滚乐观更新），重挂载重新拉服务端无损。
- `exportFormat/exportApprovedOnly`：纯前端导出选项（偏好级丢失）——低频动作重选成本低，备案接受（无触发不预做，与 996 形同源但收益边际低）。
- `writeRequestPending/deleting/confirmDeleteOpen` 等：操作进行时标志/瞬态对话框——unmount 重置可接受（进行中任务由全局 store/SSE 面 648 号机制覆盖）。

## ② 修复：sessionStorage 草稿（998 惯例形，App 层域第二面）

新模块 `lib/book-settings-draft.ts`（键 = `inkos.bookSettingsDraft:{bookId}`，多书隔离）：load（损坏 JSON / 字段类型错 / 非有限数逐字段过滤，空对象回落 null）/ save / clear 三件套 + SSR typeof 卫兵 + 隐私模式 try/catch（996/998 同形）。ArtifactView 模块不合并复用（键形与草稿形状不同——按域分模块与 996/998 分形一致）。

BookDetail 接线：三 state 惰性初始化读草稿（无草稿回落 null=原语义）；`persistSettingsDraft`（onChange 组装当前全量修改态写入——部分字段草稿语义：未修改字段不写入）；handleSaveSettings 成功清草稿（失败保留——不丢未保存修改）。

## ③ 红绿闭环

- helper 单测 3 例（node 内存 stub）：key 按 bookId 隔离 / roundtrip+clear / 字段类型错与非有限数逐字段过滤+损坏 JSON。
- 两波编译期勘误（先于活体）：load 内组装对象撞接口 readonly（局部可变类型修）；BookDetail 缺 useCallback import。
- NEUTER1001 必红活体：`saveBookSettingsDraft` 早退摘写入 → roundtrip 与 key 隔离两例红（均依赖写入路径）、损坏回落例恒绿——分形自洽 → 互逆恢复 3/3 复绿。
- **活体强度备案**：本轮无 BookDetail 整渲染集成例（1086 行组件依赖重，无既有基建）——接线为 998 已三次验证的同形模式，行为由 typecheck 编译期形状 + helper 层必红 + 全套件回归覆盖；真 IAB 走查若复现表单丢失再立集成专项。

## ④ 门禁

typecheck 双 tsconfig 过；studio 全套件 **156 文件 859 例全绿**（+3）；U+FFFD 净；Rust 树零改动（引用 1000）。提交路径限定四文件。

## 备案

1. exportFormat/exportApprovedOnly 偏好级丢失维持备案（低频低成本）。
2. 条件挂载族普查域已覆盖 dock 域（999）+ App 层页面切换域（本轮 BookDetail）；其余页面组件（Dashboard/Analytics 等）如有交互态随查随修——族判据已沉淀（交互态持久化或派生化）。

下一号自 1002 起；出口回归常备；用户侧待办 = Fork 推送（含 955-1001）。
