# 1112 号 · BookDetail 入口考古与 SeriesCanonPanel UI 活体轮（1102 备案清偿）

> 1111 出口执行。1102 备案「入口知识补全」清偿+1092 改动后 UI 端到端活体收官。
> 零代码改动轮（本会话侧）。

## 一、入口考古（1102 备案清偿）

App.tsx 渲染分支实锤：
- `view.page === "book"` 渲染 **ChatPage mode="book"**（对话创作工作区+ContextDock）——
  BookDetail **不在此分支**
- `<BookDetail>` 仅在 **`view.page === "book-settings"`**（:699）——**SeriesCanonPanel
  的 UI 入口 = `#/book/<bookId>/settings`**（BookDetail.tsx 虽名为 Detail 实为书
  设定页——1010 号「草稿覆盖层」语义，名实偏差考古结论）
- 次入口：Dashboard 书籍列表书卡片（→page:"book" chat 视图）与「写作」工作区
  （recents 含 book 记录时→page:"book"——recents 空 null 回落首页）

## 二、UI 端到端活体（1092 改动后收官）

IAB + walkthrough-env rust 路径（1088 判例）：
1. **渲染活体 ✓**：settings 路由「系列正典（跨书共享）」heading [level=3] 正常渲染
   （无错误/绑定 UI 完整：系列 ID textbox+绑定按钮）
2. **绑定交互活体 ✓**：fill `wu_dong_qian_kun` → 点绑定 → 按钮变「改绑」[active]
   （绑定态生效）→ 空条目态提示「该系列暂无共享条目——新增后在同系列其他书中
   自动生效。」+「新增共享条目」按钮——**1092 守卫覆盖的 loadBinding→setSeriesId
   →重取路径在真实浏览器交互下工作正常**

## 三、环境收尾

walkthrough-env SIGTERM 收场（1087 治本形态：pgrep tsx/engine 零残留）+tmp 清理。

## 四、下一号自 1113 起

出口=下一巡检周期 / earendil-mz 单包合并（触发条件制已更新）/ 用户侧待办=Fork 推送（含 955-1112）。
