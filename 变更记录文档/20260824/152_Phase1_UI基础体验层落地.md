# 152 号变更：UI 优化 P1 基础体验层落地（骨架加载 / 命令面板 / 顶栏三段化 / 空态）

- 日期：2026-08-24
- 模块：packages/studio（桌面 UI 主体）
- 类型：功能 + 体验优化
- 关联：150 号（四期规划总纲）、151 号（P1~P4 实施分解）；本文为 P1 八任务全量落地

## 一、P1-1 骨架组件三件套

- 新增 `components/ui/skeleton.tsx`（shadcn 标准基础件：`data-slot="skeleton"` + `animate-pulse rounded-md bg-muted`）
- 新增 `components/skeletons.tsx`：
  - `SkeletonRows`（列表行：图标圆 + 两行文本条，宽度错落静态序列）
  - `SkeletonCards`（书架大卡）/ `SkeletonParagraphs`（正文段落，末行宽度错落）
  - `useDelayedVisible(delayMs=200)` + 可测内核 `scheduleDelayedReveal`（快于 200ms 的请求不闪骨架）
  - 三型均带 `role="status" aria-busy` 无障碍标注
- 测试：`skeletons.test.tsx`（SSR 冒烟 + 假时钟）

## 二、P1-2 启动壳改造

- 新增 `components/AppShellSkeleton.tsx`：startupGate=loading 时渲染与就绪布局同构的整壳骨架（侧栏 w-[260px] + 顶栏 h-14 + 主区卡片），Logo 复用 `chat-icon-glow` 呼吸动画替代 spinner，根节点 `data-loading="shell" aria-busy`
- `App.tsx` loading 分支替换为 `<AppShellSkeleton />`（`deriveStartupGate` 逻辑不动，error 分支保持）

## 三、P1-3 语言选择器 Dialog 化

- `pages/LanguageSelector.tsx` 重构：整屏替换 → 主布局之上居中 Dialog（Base UI `disablePointerDismissal` + 吞掉 Esc 的 onOpenChange = 强制选择）；卡片内容抽 `LanguageOptions`（SSR 可测）
- `App.tsx`：删除 `showLanguageSelector` early-return，改为主布局内条件挂载。首启流程变为：壳骨架 → 就绪 UI → 语言 Dialog → 选完直接填充，全程无整屏切换

## 四、P1-4 三页接入骨架

- `components/Sidebar.tsx`：书列表 / 影游列表未就绪（`data===null && !error`，后台 refetch 不算未就绪）→ `SkeletonRows count=6/3`；空态判断加 `!booksPending` 守卫（未就绪不误报「还没有书」）
- `pages/Dashboard.tsx`：`loading && !data` → 标题条 + 按钮位 + `SkeletonCards count=3`（镜像就绪布局，消除 CLS）；顺带修复后台 refetch 闪 spinner
- `pages/ChapterReader.tsx`：正文未就绪 → 标题 + 元信息条 + `SkeletonParagraphs count=4`
- `components/ChapterWorkspacePanel.tsx`：`data?.versions?.length` 可选链防御工作台接口未就绪
- 三页测试（`Dashboard/ChapterReader/Sidebar.test.tsx`）：mock useApi 断言「未就绪渲染骨架 / 就绪渲染内容 / 空态互斥」

## 五、P1-5 命令面板 MVP

- 新增 `lib/commands.ts`（核心纯函数，全量单测）：
  - `CommandEntry`/`CommandContext` 模型；`buildNavigationCommands()` 18 条（13 无参页 + 5 个 import tab 直达）
  - `filterCommands`（title/keywords 不区分大小写，中英标题都参与匹配，空查询原样返回）、`groupCommands`（recent < navigation < action）、`commandTitle`
- 新增 `store/recents/`（照抄 preferences 模式：zustand + localStorage `inkos:studio:recents`）：`applyRecentPush` 去重置顶 + 上限 8，`recentIdentity` 六段判键（同书不同章是不同条目），坏数据静默降级
- 新增 `components/CommandPalette.tsx`：
  - `PaletteBody`（可 SSR 测试的主体）：空查询 = 最近访问 top5 + 推荐组（新建长篇 / 继续上次或对话 / 项目设置）；有查询 = 三组过滤；零匹配 = 「没有匹配的命令」+ 创建新书兜底（永不死胡同）；cmdk `shouldFilter={false}`（自过滤与预过滤冲突）
  - `CommandPalette`：CommandDialog 组合（Base UI portal 不可 SSR，故主体单独导出）
- `App.tsx`：⌘K/Ctrl+K 临时 keydown（P2 迁统一分发器）；`setRouteTracked` 包装（用户导航写 recents，SSE 系统跳转不写）；书名取自 App 侧 `/books` 同源数据

## 六、P1-6 动作层索引

- `buildActionCommands()` 19 条：创建类 12（长篇/短篇/剧本/分镜/互动影游/分支互动/开放世界/同人/续写/番外/仿写/翻译会话）+ 主题 3 + 语言 2 + 刷新项目 + 新建项目会话
- 总命令 18+19=37 ≥ 30（验收线）；创建类经 App 的 `commandCtx`（`openBookCreate`/`createProjectChatDraft`/`launchProjectMode`，与 Sidebar 等价实现，P3 收敛）
- 测试：命令计数 ≥30、run 全为函数、ctx 调用逐条断言

## 七、P1-7 顶栏三段化

- 新增 `lib/breadcrumb.ts`：`deriveBreadcrumb(route, {t, bookTitle})` 纯函数，23 种路由变体全覆盖（chapter 的 `{n}` 模板替换；书名缺失降级「书籍」），非末段携带回跳路由
- `App.tsx` header 重排：左面包屑（truncate 防溢出，点击逐级返回）/ 中伪搜索框（w-64，⌘K/Ctrl+K 按平台显示，点击开面板，md 以下隐藏）/ 右语言分段 + 主题按钮（不变）；P2 拖拽区注释锚点就位
- 测试：`breadcrumb.test.ts` 23 变体全断言

## 八、P1-8 EmptyState 与空态接入

- 新增 `components/EmptyState.tsx`（icon 泡 + 标题 + 描述 + 可选动作；`compact` 供侧栏）
- Sidebar 三处空态接入：书架（创建第一本书 → 书籍创建流）/ 影游（开始创作 → 互动影游草稿会话）/ 会话区（空时以空态替代裸「新建会话」按钮，动作直达同一 handler）
- Dashboard 书架空态原已达标（P1 前），保持不动

## 九、连带变更

- `hooks/use-i18n.ts`：新增 bread.*/cmd.*/empty.* 三组键（36→~60 键）
- `vitest.config.ts`：include 增加 `src/**/*.test.tsx`（SSR 冒烟通道，此前仅 .test.ts）
- `pnpm-workspace.yaml`：pnpm 11 构建脚本白名单迁移（esbuild/msw/protobufjs，原 package.json pnpm 字段已不被读取）；`pnpm-lock.yaml` 随修复安装重锁

## 十、验证链

| 门禁 | 结果 |
| --- | --- |
| studio vitest | 70 文件 / 640 测试全绿（新增 10 文件 / 78 测试） |
| typecheck（client + server） | tsc --noEmit 双双 0 错误 |
| build（vite + tsc server） | 通过（chunk 体积警告为存量） |
| e2e（playwright，chromium） | loading-skeletons 4/4 + command-palette 2/2（⌘K 打开→过滤「日志」→回车跳转→面包屑切换→重开见最近访问；伪搜索入口） |
| packages/cli 全量 | 232/232（中途一次 ERR_MODULE_NOT_FOUND 为并发 pnpm install 竞态，单独复跑通过） |

## 十一、验收口径对照（151 号 P1）

- ✅ 启动 loading 无整屏 spinner（壳骨架 + e2e 断言 body 无 animate-spin）
- ✅ 命令面板覆盖 100% 无参路由 + ≥30 命令（37 条）
- ✅ 顶栏三段化在 1280/960 不溢出（面包屑 truncate + 搜索框 md 断点）
- ✅ 空态与骨架互斥（undefined=骨架 / []=空态，双向测试）
- 打包 v0.2.0-alpha.1 与掐表基线：P1 代码完成日（08-24）不打包，按日程留到周五（08-28）验收打包节点

## 十二、备注

- gui-test-screenshots/（1 张目测截图）为过程产物，不入库
- P1 期间 App/Sidebar 双份 Nav 接口、双份创建类 handler 为已知债务，P3 活动栏重组收敛（151 号既定）
