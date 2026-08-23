# P2~P4 任务分解与总 WBS(08-31 ~ 09-18)

- 日期:2026-08-24
- 上游:《功能规划与日程安排.md》;P1 已有实现级分解(见《P1任务实现分解.md》)
- 本文:P2~P4 每任务给到**依赖 / 文件清单 / 实现要点 / 验收**四要素(中等粒度,开工前一天再细化到 P1 级)
- 文末附四期总 WBS(估时汇总、关键路径、并行性)

---

## P2 桌面融合层(08-31 ~ 09-04)

### P2-1 macOS 融合标题栏(周一上午)

- **依赖**:P1-7 顶栏三段结构。
- **文件**:M `src-tauri/tauri.conf.json`(main 窗口加 `"titleBarStyle": "Overlay"`、`"hiddenTitle": true`);M `packages/studio/src/App.tsx`(header 根加 `data-tauri-drag-region`;左 padding-left 预留 78px 交通灯,仅 macOS 判定 `navigator.userAgent` 或 tauri os api);M `index.css`(可选 `.drag-region button, .drag-region input { -webkit-app-region: no-drag }` 防拖拽劫持点击)。
- **要点**:所有交互子元素必须阻止拖拽冒泡(按钮/搜索框/分段控件逐个核对);双击标题栏最大化语义由系统提供,验证不被前端拦截;Windows/Linux 分支保持系统标题栏(配置按平台条件,用 tauri.conf 的平台覆写文件或运行时创建窗口参数)。
- **验收**:macOS 真机——顶栏可拖动窗口、双击最大化、交通灯不遮挡面包屑首段、所有顶栏控件可点击;Windows/Linux 冒烟无回归。
- **回滚**:删除配置项 + 移除 padding 即回到系统标题栏。

### P2-2 窗口状态记忆(周一下午)

- **依赖**:无(可与 P2-1 并行)。
- **方案二选一**:① 引入 `tauri-plugin-window-state`(优先,成熟);② 自实现(main.rs 保存/恢复 bounds+maximized 到 app config 目录 JSON,约 80 行)。
- **文件**:M `src-tauri/Cargo.toml`/`src/main.rs`(或 `lib.rs` 注册插件);M `src-tauri/capabilities/main.json`(插件权限,对齐既有 ACL 修复经验——见 146 号变更,注入脚本勿误报)。
- **验收**:调整尺寸/位置/最大化 → 重启还原;多显示器拔插后不跑到屏外(越界矫正:恢复前检查 bounds 与当前 displays 相交)。

### P2-3 主题 auto 档(周二上午)

- **依赖**:无。
- **文件**:M `hooks/use-theme.ts`:① `Theme = "light" | "dark"` → `ThemeMode = "light" | "dark" | "auto"`(存储三态,resolve 后仍输出 light/dark 供现有 `isDark` 消费,不改 App.tsx 判定);② auto = `matchMedia("(prefers-color-scheme: dark)")` + change 监听(替换现有每分钟时间轮询——**注意**:现有无存储默认是"按时段",auto 未显式选择时保留该行为作为默认值,避免老用户体感突变);③ 顶栏主题按钮三态循环 + 命令面板三条命令(扩 P1-6 注册表);M 对应 `use-theme.test.ts`(现有 getTimeBasedThemeForHour 等测试全保,新增 matchMedia mock 用例)。
- **验收**:系统切换深浅 → 应用即时跟随(auto 档);显式选择持久化优先于系统。

### P2-4 全局快捷键分发器(周二下午~周三上午)

- **依赖**:P1-5 命令面板(快捷键同时是命令)。
- **文件**:N `hooks/use-global-hotkeys.ts`:
  ```ts
  interface HotkeyDef { combo: string; commandId: string; }        // "mod+k" → 单一事实源
  export function parseCombo(e: KeyboardEvent): string | null       // 纯函数可测:mod=meta/ctrl 归一
  export function shouldIgnore(e: KeyboardEvent): boolean           // 输入框/textarea/contenteditable 放行(除 mod 组合)
  ```
  App 挂载一次,keydown 单点分发 → 查 `lib/commands.ts` 注册表 run;M `lib/commands.ts`(hotkey 字段反查);N `use-global-hotkeys.test.ts`(parseCombo 全组合矩阵 + shouldIgnore 表)。
- **首批绑定**:Cmd/Ctrl+K(面板)、Cmd+P(快速打开,P2-5)、Cmd+B(侧栏,P3 前先空占位 toast)、Cmd+E(最近)、Esc 链(面板内建,全局仅补充停止生成场景)、Cmd+/ 或 ?(速查,P2-6)。
- **验收**:输入框打字不触发;全键位与速查页 100% 一致(一致性由注册表生成速查页保证)。

### P2-5 Cmd+P 快速打开(周三下午~周四上午)

- **依赖**:P2-4;P1-5 面板。
- **文件**:M `lib/commands.ts` — 增内容层:构建期/打开时聚合 useApi(`/books`)、(`/interactive-films`)、chat store 会话列表、章节标题(书详情 API 顶层缓存,避免面板打开时全量扫章节——大书 500 章 fuzz 过滤 <50ms 即可);N `lib/quick-open.ts` — 过滤纯函数(标题 includes + 拼音首字母可选);面板复用 CommandDialog 另一实例(mode="quick-open",标题「跳转到…」,参照 VSCode Cmd+P 与 Cmd+Shift+P 分离)。
- **验收**:打开→输入书名/会话名/章节号 均可直达;500+ 章书过滤无可感卡顿(>100ms 记录优化项)。

### P2-6 快捷键速查弹层(周四下午)

- **依赖**:P2-4。
- **文件**:N `components/HotkeyCheatSheet.tsx`(Dialog,表格按组:全局/导航/写作;**数据源 = hotkeys 注册表 + commands 表自动生成**,不做第二份手写表);M App 挂载 + Cmd+/ 绑定。
- **验收**:注册表加键 → 速查页自动出现;中英双语。

### P2-7 macOS 菜单栏对齐(周五上午)

- **文件**:M `src-tauri/src/lib.rs`(tauri Menu api:新建/打开最近/视图(切主题/语言/侧栏)/窗口/编辑(copy/paste/selectAll 由系统 webview 补齐));菜单项触发经 event 转发到前端(webview 监听 `menu://` 事件调 commands run)。
- **验收**:菜单快捷键与 Web 内一致;点菜单项功能等价面板命令。

### P2 收尾(周五下午)

`pnpm -r test` + e2e + `cargo test`(src-tauri 侧 P2 首次动到 Rust,必须跑)+ macOS/Windows(至少一虚拟机或同事机)/Linux 三平台冒烟;打包 v0.2.0-alpha.2;归档变更记录。

---

## P3 工作台深化(09-07 ~ 09-11)

### P3-1 活动栏与四区重组(周一)

- **文件**:N `components/ActivityBar.tsx`(48px,四区图标 + 底部设置;选中态与折叠态);N `lib/nav-sections.ts` — **纯映射表**(23 路由 → 创作/工具/管理/互动影视 四区,含每区图标/排序/i18n key)+ `nav-sections.test.ts` 全路由覆盖断言无孤儿;M `Sidebar.tsx` → 改造为 `SidePanel.tsx`(按当前区渲染,内部复用现有 Section 组件);M `App.tsx`(布局插列,nav 统一——**消除 Nav 接口重复声明**,单一 `lib/nav.ts` 导出类型与工厂)。
- **要点**:feature flag(preferences `navLayoutV2`)双轨一周,旧 Sidebar 保留到 P4 走查后删;首次启动导览气泡(4 条,一次性)。
- **验收**:全部 23 路由可达且归属正确(测试断言);导航点击次数:高频页(书/会话/章节)≤2 次。

### P3-2 侧边栏增强(周二)

- **文件**:M `SidePanel.tsx` + N `hooks/use-panel-width.ts`(pointer 拖拽 180~400 clamp,localStorage 记忆,双击分隔条复位) + N `lib/tree-filter.ts`(节点树标题 includes 过滤纯函数,保留命中祖先)+ 两个测试文件;Cmd+B 折叠为图标条(宽度 0 ↔ 48 ↔ 记忆宽 三态)。
- **验收**:50 书×每书 10 会话的树,过滤输入 <16ms 一帧;折叠态 hover 有 tooltip(图标定位)。

### P3-3 标签页多任务(周三)

- **文件**:N `store/tabs/`(zustand:标签数组 { id; route; title; pinned; preview },操作 open/close/closeOthers/pin/activate/replacePreview,持久化恢复;reducer 式纯函数 `tabs-reducer.ts` 全量单测——**P3 最重的测试点**);N `components/TabStrip.tsx`(溢出横向滚动、中键关、右键菜单、拖拽重排可后置);M `App.tsx` 主区由 route 直渲染改为 activeTab.route 渲染(不写 URL 页面照常,hash 仅同步 activeTab 以保深链兼容:外部 hashchange → openOrActivate(preview))。
- **验收**:深链(刷新/分享 hash)进入对应标签;Cmd+1..9 切换;预览语义(单击替换 preview 标签,双击驻留)可用;20 标签不破版。

### P3-4 右侧 dock 与底部面板(周四)

- **文件**:M `components/chat/BookSidebar.tsx` → 泛化 `ContextDock.tsx`(面板注册表:设定/大纲/进度/工具执行,宽度记忆,`Cmd+Shift+D` 开合);N `components/BottomPanel.tsx`(Cmd+J;两 tab:生成任务流(SSE 事件流渲染,复用 ToolExecutionSteps 数据源)/日志(LogViewer 轻量内嵌);可最大化高度、暂停滚动跟随)。
- **验收**:Chat 页 dock 与底部面板并存不挤压编辑区(<40% 高);面板开合状态按页记忆。

### P3-5 状态栏(周五上午)

- **文件**:N `components/StatusBar.tsx`(h-6,左:当前书·章节+字数(章节 API/前端统计);中:SSE 连接点(绿/黄/红)+ daemon 运行点(复用 Sidebar 现有 /daemon 源);右:生成中 spinner+「查看」点击开底部面板);M `App.tsx` 布局收口。
- **验收**:状态全部真实数据源(无占位);SSE 断连→黄点+点击重连;深浅主题可辨。

### P3 收尾(周五下午)

全量回归(前端+e2e+cargo)+「建书→写作→查设定→看日志」全流程走查;打包 v0.2.0-beta.1;归档。

---

## P4 精致化收口(09-14 ~ 09-18)

### P4-1 Focus Mode 与打字机模式(周一)

- **文件**:`index.css` 增 `.focus-mode` 变量覆盖(--background/--muted/--border 三变量派生低对比)+ `.typewriter-focus`(当前段 opacity 1、其余 0.55,JS 判定光标所在段);N `hooks/use-focus-mode.ts`(进/出:切类+隐藏活动栏/状态栏保留+通知静默标志+Esc 退出);M ChapterReader 与 ChatPage 写作区接入;preferences 持久化开关。
- **验收**:进入/退出 <300ms 无闪烁;reduced-motion 用户默认关闭平滑滚动。

### P4-2 密度档与动效规范(周二)

- **文件**:M `store/preferences/types.ts`(+`density: "comfortable" | "compact"` 与 setter,照抄现有字段模式);`index.css` 增 `.density-compact`(列表行高/内边距 ×0.85 的变量化:--row-h、--pad-y 两个变量驱动,不逐类覆盖);M 动效 token(--motion-fast/base/slow)落地 + `@media (prefers-reduced-motion: reduce)` 全局关闭 fadeIn/stagger/chat 系列(保留 opacity 终态);设置入口(顶栏主题钮旁或命令面板)。
- **验收**:紧凑档列表可视行数 +25% 以上;系统开启减弱动态效果 → 所有装饰动效静止、无 opacity=0 卡死(stagger 的 forwards 终态必须保住)。

### P4-3 通知中心与任务可取消(周三)

- **文件**:N `store/notifications/`(分级 INFO/WARN/ERROR,上限 50,铃铛未读数);N `components/NotificationCenter.tsx`(右下角 Popover);生成任务取消:排查现有 SSE/生成 API 是否已有 cancel 端点,**无则后端补(唯一可能动 Rust 的点,提前周四缓冲)**;底部面板任务行加停止钮。
- **验收**:通知不阻塞、可清空、未读准确;取消生成后状态栏与面板一致收敛。

### P4-4 无障碍审计与 E2E 补齐(周四)

- **文件**:对照 `docs/i18n-a11y.md` 清单:全 ui/* focus-visible ring 核对、IconButton aria-label(重点:折叠/主题/语言/铃铛)、骨架 aria-busy、面板 Esc 焦点返还、tab 顺序走查(键盘-only 完成建书全流程);N e2e:`command-palette.spec.ts`、`tabs.spec.ts`、`focus-mode.spec.ts`、`hotkeys.spec.ts`(键盘事件驱动,沿用现有 playwright 设施与 fixtures/seed 模式)。
- **验收**:axe(如引入)或手动清单无 P0/P1;新 e2e 全绿且覆盖 P1~P4 关键路径。

### P4-5 十维复评与 v0.2.0 发布(周五)

- 走查:《参考案例对比分析.md》矩阵十维复评(目标均值 ≥4.0,逐维写证据);视觉走查(浅/深×舒适/紧凑×三平台截图对比);性能复测(启动/面板/过滤三指标);打包 v0.2.0 正式 + 归档变更记录 + 更新 CHANGELOG。

---

## 四期总 WBS

### 估时汇总(单位:0.5d)

| 期 | 任务串 | 合计(工作日) |
| --- | --- | --- |
| P1 | 1+1+0.5+0.75+1+0.5+0.5+0.5+收尾0.5 | 5.25 → 预留弹性后 ≈5(见下) |
| P2 | 1+0.5+0.5+1+1+0.5+0.5+收尾0.5 | 5.5(超支点:P2-1 跨平台) |
| P3 | 1+1+1+1+0.5+收尾0.5 | 5 |
| P4 | 1+1+1+1+1 | 5 |

弹性说明:P1-3(0.25d)与 P1-8 可压缩合并;P2-2 若用插件仅 0.25d 省出缓冲给 P2-1;P3-3 是全局最大风险项,tab 纯函数 reducer 单测先行(TDD 顺序),UI 后接。

### 关键路径

```
骨架(P1-1/2/4) → 顶栏(P1-7) → 标题栏融合(P2-1) → 活动栏(P3-1) → 标签页(P3-3) → dock/面板(P3-4) → Focus/密度(P4-1/2) → 发布(P4-5)
```
面板系列(P1-5/6 → P2-4/5/6)为独立支线,与关键路径仅在 P1-7 汇合;P2-2/P2-3 可插空并行。

### 里程碑对齐

M1(08-28):无白屏启动 + 面板 MVP 演示;M2(09-04):macOS 融合窗口 + 快捷键演示;M3(09-11):四区导航 + 标签页全流程走查;M4(09-18):十维复评 ≥4.0 + v0.2.0 发布。

### 全局风险 Top3(滚动跟踪)

1. **P3-3 标签页 × hash 路由兼容**(历史行为:非 hash 页不写 URL、同 hash 重复赋值不触发 change——已在 use-hash-route 注释中记录陷阱):tab reducer 必须以 store 为真相、hash 仅做入站同步;测试覆盖「同 hash 重复打开」用例。
2. **P2-1 平台差异**:Overlay 仅 macOS 生效需运行时判定;三平台冒烟依赖设备可用性,提前在 P2 周一预约 Windows/Linux 环境。
3. **P4-3 取消生成可能动后端**:08-31 前预查 SSE cancel 契约,若需 Rust 侧改动则提前到 P3 周末缓冲执行,避免发布周动引擎。
