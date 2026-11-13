# 1011 号：回归审查走查轮——key 同族五处清偿 + health 定桩 + IAB 四旅程

日期：2026-11-14
范围：`packages/studio/src/App.tsx`（五处 id 键页 key）+ `src-tauri/src/project/health.rs`（定桩注释）
前置：1010 出口清单（回归审查轮 / key 同族普查）双项合并执行
门禁：gate:ts 十腿全绿 + src-tauri lib 474/474 + studio App/BookSidebar/sidebar 26/26 + U+FFFD 净

---

## 一、代码审查段 A：id 键页 key 同族普查清偿（1010 备案转正）

普查 App.tsx 全部 id 键页挂载点（view.bookId/view.serviceId/view.chapterNumber props + 无 key）：

| 页面 | 键 | 处置 |
|---|---|---|
| BookDetail | bookId | 1010 已落 |
| BookTimeline | bookId | 1011 落 key |
| ChapterReader | bookId+chapterNumber | 1011 落复合 key（`${bookId}-${chapterNumber}`） |
| Analytics | bookId | 1011 落 key |
| TruthFiles | bookId | 1011 落 key |
| ServiceDetailPage | serviceId | 1011 落 key |

判据统一=「id 键页按 id 重挂载」（惰性 useState 初始化器/内部 state 与 id 绑定，in-place 换 id=串扰潜在面；跨 id in-place 当前 UI 不可达=防御基线，1010 定性沿用）。ChatPage（activeBookId）/ContextDock 不落 key——两者已内建 bookId 变化守卫（1008 修复面 + ChatPage guard=15），且换书不重挂载是聊天页产品语义（会话连续性）。

## 二、代码审查段 B：1008-1010 新增面双扫

1. **「谁解析 message」复核（1009 双语直出安全性）**：全仓扫 `.message.contains/.startsWith/===` 判定面——picker 前端 `event.error?.message` 为透传 details（JS 原生错误非 AppError）；Rust 命中均为测试断言（locale 锁已持）与 health.rs 内部 Issue（非 AppError）。**零内容解析判定面依赖 AppError.message** → 双语直出安全定性复核通过。
2. **useApi 中心杠杆复核**：失效事件/手动调用/path 变化全部漏斗经 refetch 单点 seq 比较 ✓；refresh_locale 与角标事件并发分析——badge 值每次 build_menu 从计数器现读，无陈旧文案窗口。
3. **cmd_set_locale 注入面**：tag 仅经 `Locale::parse`（startsWith("en") 布尔判定）无透传拼接 ✓。
4. **新线索 health.rs（普查漏网复核）**：Issue.message/suggestion 中文直显 ~58 处 + types.rs ~60 处——但 `cmd_check_project_health` **前端零 invoke 命中（picker/studio 均无）=预留命令不可达面**，零用户可见影响。按 1008 备案先例处置=**定桩不清偿**（health.rs 头注：未来接入时按 crate::locale::pick 双语化——机制锚点就地可见），台账落档。

## 三、IAB 走查（walkthrough-env 生产引擎 + fixture，dist 04:16 新鲜构建）

1. **ArtifactView/章节/伏笔池打开+返回链** ✓（1008 修复域 + 998 编辑草稿 textarea 活体）
2. **请求管道活体**：fetch 延迟钩子（首请求延迟 2s 假内容 STALE-A）——晚到正常显示；同挂载双文件切换入口在真实 UI 不存在（ArtifactView 打开时列表被替换=重挂载域），**确定性竞态由 1008 jsdom 测试锁死**（浏览器域无需重复）
3. **BookDetail key 行回归** ✓（设置区+status select+AssetLibraryPanel 三 tab）
4. **AssetLibraryPanel kind 切换往返** ✓（题材基底↔推进模式，空态渲染正常）
5. **深链三页** ✓：book-timeline/chapter(复合键)/translation（1008 修复站 TranslationManager 空态）全无错误

走查环境插曲备案：`localStorage "inkos:studio:dock-visibility"={"book":false}` 为历史会话存的按页开合偏好（996 功能正常工作），清键重载恢复——非缺陷。

## 四、教训

1. **「预留命令」类不可达直显面**（注册无消费）：清偿违背真实需求驱动（无用户可见缺陷），定桩+台账是正解——桩写明「接入时按 pick 双语化」使机制锚点就地可见。
2. IAB hash 深链形态以 `use-hash-route.ts` ROUTE_SPECS 为准（`#/book/<id>/settings` 非 `#/book-settings/<id>`）——路由形态先查表再 goto，不猜。

## 五、下一号自 1012 起

出口常备：health 面接入时双语清偿（定桩触发）/ BookSidebar 宽屏 dock 默认态复核（走查插曲）/ 用户侧待办=Fork 推送（含 955-1011）。
