# 1008 号：异步响应竞态族扫清偿轮——useApi 中心杠杆 + stale-response 防护 + status 枚举防御

日期：2026-11-14
范围：`packages/studio`（11 修复站）+ `packages/core/package.json`（exports 加 subpath，additive）+ `packages/studio/vitest.config.ts`（subpath alias 登记）
前置：1007 号备案双项（ArtifactView stale-response 防护 + book-settings-draft status 枚举防御轻缺）+ 族扫扩面
门禁：gate:ts 十腿全绿（build/typecheck/test/audit:npm/rust-bin/node-fallback-smoke/engine-contract-diff/export-epub-smoke/work-alternation/check-routes）；studio 套件 893/893（+9）；U+FFFD 净；Rust 树（engine-rs/src-tauri）零改动引用 1007 定桩

---

## 一、为什么做（1007 备案转正）

1007 号备案两项转正：

1. **ArtifactView fetch 无 stale-response 防护**（预存类）：effect 内 `fetchJson` 无取消/过期守卫，目标切换时旧响应晚到覆盖新内容。
2. **book-settings-draft status 枚举防御轻缺**：`loadBookSettingsDraft` 对 status 只做 `typeof === "string"`，不校验枚举合法性——sessionStorage 被篡改/旧协议残留的非法值直接预填设置表单，保存即污染真源。

族扫扩面发现：竞态不是孤立缺陷而是**全站族**（无守卫候选面 24 文件），且存在一个**中心杠杆**（useApi）修复一次即护全部消费方。

## 二、族扫台账（判据：effect/load 内 fetch + deps 挂载中 in-place 变化或多调用点交错 + 无过期守卫 → setState）

### 修复面（11 站，全部红绿/既有套件覆盖）

| # | 站点 | 触发形态 | 守卫形态 |
|---|------|----------|----------|
| 1 | `hooks/use-api.ts` useApi.refetch | 中心漏斗：挂载/path 变化/失效事件/手动调用（护 Dashboard、DaemonControl、BookDetail SSE refetch 环等全部 useApi 消费方） | seqRef latest-wins，data/error/loading 三分支卫兵 |
| 2 | `BookSidebar.tsx` ArtifactView | artifactFile/章节号 in-place 切换（1007 备案主缺陷） | effect cancelled 旗标（then/catch/finally 三卫） |
| 3 | `ChaptersSection.tsx` | [bookId, bookDataVersion]——Sidebar 换书不卸载 ContextDock + SSE bump | effect cancelled 旗标 |
| 4 | `FoundationSection.tsx` | 同上 | effect cancelled 旗标 |
| 5 | `AssetLibraryPanel.tsx` | [kind] 用户切 tab + 操作后重取共用 load | loadSeqRef（load 内 seq latest-wins） |
| 6 | `TranslationManager.tsx` | [selected?.projectId] 项目行 in-place 切换 | effect cancelled 旗标 |
| 7 | `SeriesBackfillPanel.tsx` | [target] 目标书 in-place 切换 | effect cancelled 旗标 |
| 8 | `PlayHud.tsx` load | 四调用点（挂载/开面板/回合收尾/操作后）+ base 随回合推进 | loadSeqRef |
| 9 | `BackupPanel.tsx` loadSnapshots | 挂载+操作后重取多入口 | loadSeqRef |
| 10 | `lib/book-settings-draft.ts` | status 枚举防御（备案第二项） | BookStatusSchema.safeParse 逐字段过滤 |
| 11 | `pages/BookDetail.tsx` | 本地手打 BookStatus union **漂移丢 "incubating"**（core 六值）+ :194 盲 `as` | 删本地 union → core subpath type import；:194 转型撤销（草稿库已强类型返回）；persistSettingsDraft 参数收紧 BookStatus |

### 豁免面（定性依据）

- **已有守卫（合规，15 文件）**：ContextLensPanel / ContextMeterBadge / DirectorPanel / PromiseTimelineCard / QualityDebtsCard / QuickOpenPalette / RunLogPanel / StyleBindingCard / TaskRoutingPanel / prompt-input / ProjectArtifactDrawer / ToolExecutionSteps / CharacterSection / SummarySection / ChatPage / ServiceDetailPage / ServiceListPage（cancelled 惯用形已普及，998 新增面 ArtifactView 是漏网）。
- **一次性 mount fetch（页级，页切即卸载）**：WritingStatsCard[]、ServiceConfigSourceCard[]、RadarView、LogViewer、DoctorView、StoryGraphTree、WorkInspector、TruthFiles、StyleManager、GenreManager、CreativeMethodsEditor、OnboardingWizard、ChapterReader、Dashboard、FlowView、ImportManager[initialTab]（路由 prop，页级重挂载）。
- **数据同步效应非 fetch 触发**：ProjectSettings[xData 族]（useApi→表单 state 拷贝）、ChapterWorkspacePanel[data]、FlowView[graph 族]（派生同步）。useApi 修复后其数据面自动受护。
- **Dashboard[:82][open]**：点击外监听效应，无 fetch（粗普查 grep 命中为文件级噪声）。

### 备案面（潜在不可达/行为面变更需产品判断）

- **App.tsx:695 BookDetail 无 `key={view.bookId}`**：跨书 in-place（book-settings(A)→book-settings(B) 不卸载）当前 UI 不可达（Sidebar.toBook 走 "book" 页；Dashboard 菜单入口从 Dashboard 发起，BookDetail 未挂载）——但惰性 useState 初始化器（`loadBookSettingsDraft(bookId)` 覆盖层恢复）在 in-place 换 bookId 时**不会重跑**=跨书覆盖层串扰潜在面；BookDetail 内 5 个 [bookId] 效应 + 七个 [bookId] 卡片（RosterCandidates/TensionCurve/QualityTrend/AntiAiAndExperience/CodexPanel/SeriesCanonPanel）同潜在域。`key={view.bookId}` 一行可永久封口，但引入 remount 行为变更，留待可达路径出现（如 quick-open 加书跳转）时一并落。
- **BookDetail:630 `book.status as BookStatus`**：服务端 string 类型盲转型保留（服务端 core-validated 保证枚举合法；BookData 类型未收紧避免面扩大）。
- **status select 五选项不含 incubating**：core 枚举有六值，UI 字典无 statusIncubating 键、全仓无赋值点（声明未用）。草稿层六值全收（枚举真源校验）；select 面维持现状备案。未触碰 status 的保存不携带 status 字段（:471 只在有覆盖层时写入），incubating 书籍状态保存安全。
- **测试样例盲区自证**：1001 号既有 roundtrip 例样例 `status: "ongoing"` **不在枚举内**——测试自身认证过非法值可往返（枚举盲区在测试面同时存在），本号改为 "active" 并新增全六值合法例。

## 三、修复细节与红绿

### 中心杠杆形态（useApi seq 守卫）

```ts
const seqRef = useRef(0);
const refetch = useCallback(async () => {
  const seq = ++seqRef.current;
  // ...
  try {
    const json = await fetchJson<T>(url);
    if (seq !== seqRef.current) return;   // 旧响应晚到：不覆盖 data
  } catch (e) {
    if (seq !== seqRef.current) return;   // 旧失败晚到：不误报 error
  } finally {
    if (seq === seqRef.current) setLoading(false); // 旧请求不掉新请求的 loading
  }
}, [path]);
```

早退不掉 loading 是关键面：新请求在途时由新请求自己的 finally 收口，旧请求早退不干扰。

### effect cancelled 形态（ArtifactView/Chapters/Foundation/Translation/SeriesBackfill）

effect 内 `let cancelled = false`，cleanup `cancelled = true`，then/catch/finally 三分支逐卫——**catch 分支必卫**是本族最重形态（旧失败晚到 `.catch(() => setX([]))` 会把已到手好数据清成空态）。

### load 内 seq 形态（AssetLibrary/PlayHud/BackupPanel）

load 为多调用点共享（effect + 事件 handler）时，effect cleanup 旗标覆盖不到 handler 触发——守卫须在 load 体内 seqRef 比较。

### 红绿闭环

1. **红测试先行**：use-api.test.ts 三例（手动 refetch 竞态/过期失败误报/path 变更竞态）+ book-settings-draft 两新例（非法 status 过滤/core 六值全合法）+ ChaptersSection 两例（版本 bump 交错/过期失败清空）+ BookSidebar.interaction 两例（切目标旧覆盖新/旧失败清空态）。
2. 首轮红矩阵 8 例全为**竞态/防御语义红**（无基建假红；ChaptersSection t2 曾撞 store 跨例持久同值不触发 effect——改相对 bump 后归净）。
3. 修复后 15/15 绿；studio 全套件 893/893（+9）；typecheck 双 tsconfig 过。
4. **NEUTER1008**：摘 useApi 守卫（`if (false) return`）→ 恰红 3/3（测试锚定修复本体非恒绿）→ 互逆恢复。
5. **首跑门禁抓真红（修复引入面自证门禁有效）**：`import { BookStatusSchema } from "@actalk/inkos-core"` 把 core 主入口拖进浏览器 bundle → rollup 拒绝 `node:async_hooks`（agent-trajectory）→ build 腿红。正解=core exports 加 `./models/book` subpath（additive，dist/models/book.js 纯 zod 浏览器安全）+ studio vitest alias 登记（先例：399 号三 subpath 同法）。教训入账：**studio 引 core 运行时值必须走 subpath，主入口只可 type-only**。

## 四、测试清单（+9）

- `src/hooks/use-api.test.ts`（新，3 例）：手动 refetch 竞态 / 过期失败不清成功态 / path 变更竞态。
- `src/components/sidebar/ChaptersSection.test.tsx`（新，2 例）：bookDataVersion 交错 / 过期失败不清好数据。
- `src/components/chat/BookSidebar.interaction.test.tsx`（+2 例）：切目标竞态 / 过期失败保正文。
- `src/lib/book-settings-draft.test.ts`（+2 例，改 1 例）：非法 status 过滤 / core 六值全合法；roundtrip 样例 "ongoing"→"active"（盲区自证）。

## 五、教训

1. **竞态守卫的 catch 分支与 then 分支同等必卫**——旧失败晚到清空好数据比旧数据覆盖更常见（failure-wipe 形态）。
2. **多调用点 load 的守卫必须在 load 体内（seq），不能只在 effect cleanup**——handler 触发路径不受 effect cleanup 管。
3. **中心杠杆优先于逐站修复**——普查先找漏斗（useApi），一次修复护全族；逐站修复只处理绕过漏斗的裸 fetchJson。
4. **枚举防御的真源必须是 schema 而非本地字面**——本地 union 与测试样例会同漂（"ongoing" 实证：生产代码与测试同时认证非法值）。
5. **studio 引 core 运行时值必须 subpath**——主入口拖 node:async_hooks 链，rollup 浏览器构建拒绝；新 subpath 须同步 core exports + studio vitest alias 双登记。
6. **测试基建自身的状态泄漏**——zustand store 跨测试例持久，同值 setState 不触发 useSyncExternalStore 重渲染/effect 重跑；依赖 bump 的测试用相对 bump。

## 六、下一号自 1009 起

出口常备：回归审查轮（新增面全量走查）/ src-tauri 桌面壳 CJK i18n 专项（1006 备案，INKOS_LOCALE 先例）/ BookDetail key 防御行（备案，可达路径出现时落）/ 用户侧待办=Fork 推送（含 955-1008）。
