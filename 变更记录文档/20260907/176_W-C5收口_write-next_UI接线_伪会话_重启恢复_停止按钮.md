# 176 · W-C5 收口：write-next UI 接线（伪会话锚点 + 重启恢复 + 停止按钮）

- 日期：2026-09-07
- 模块：packages/studio（hooks/use-book-activity.ts / use-i18n.ts、pages/BookDetail.tsx / Dashboard.tsx、api/server.test.ts 类型修正）、总体方案文档
- 类型：feat（UI；纯前端，引擎零改动）
- 关联：174 号（检查点）、175 号（可停止）、总体方案 W-C5「UI 接线随产品批次」→ 提前收口

## 一、背景

174/175 号把 write-next 的检查点（重启对账）与真实可停止做进了引擎，但 UI 从不传 `sessionId`——两项能力在生产链路休眠。本批接线激活：这是 W-C5 的最后一公里，做完即整条链路（发起 → 留痕 → 恢复 → 停止 → 对账）闭环。

## 二、改动

1. **伪会话锚点 `writeTaskSessionId(bookId)`**（use-book-activity.ts 导出）：`book:{bookId}:write`。书籍页没有聊天会话上下文,用稳定伪会话做 task-store 文件名键与 abort 端点参数——不对应会话目录（task-store 只按键落盘;abort 端点对无会话条目自然无害）。BookDetail 与 Dashboard 共用同一 helper 保证锚点一致。
2. **发起透传**：BookDetail `handleWriteNext` / Dashboard 书卡按钮 POST body 带 `sessionId`——引擎 174 号检查点、175 号可停止在此激活。draft 暂不接（两端 draft 管线均无信号参数,保持诚实）。
3. **重启恢复**：BookDetail 挂载（bookId 变化）时按伪会话建临时 `EventSource(?sessionId=)`——收到 `task:snapshot`：running/processing → 恢复 writing 态;终态 → 静默 refetch。收帧即关;5s 无快照超时关闭（无任务常态零残留）。重启后 write:start/complete 事件已丢,检查点快照是唯一真相源（对账读取在服务端,174 号）。
4. **停止按钮**：writing 中主按钮从 disabled-spinner 翻转为 destructive「停止写作」（Square 图标）→ `POST /sessions/{伪会话}/abort`。Rust 引擎实停（175）;Node 回退端诚实 `aborted:false` 任务跑完——两端都以 `write:error|write:complete` 事件收敛 UI（write:error 本就在 BOOK_REFRESH_EVENTS,停止后自动 refetch 清态）。停止请求失败不打断（SSE 终态仍兜底）。
5. i18n 新键 `book.stopWriting`（zh 停止写作 / en Stop Writing）;server.test.ts 的 readTaskSnapshotEvent helper 参数类型改结构化（原 `ReturnType<typeof createStudioServer>` 引用动态导入符号,typecheck 不过）。

## 三、验证

| 项 | 结果 |
| --- | --- |
| `packages/studio` `npm run typecheck`（双 tsconfig） | 干净 |
| `packages/studio` vitest 全量 | **741/741 绿**（含 writeTaskSessionId 2 用例） |
| 真 bin 三请求序列冒烟（UI 将发出的完整序列） | ①write-next 带伪会话 → 契约响应;②`SSE ?sessionId=` 下发 running 快照;③abort → `{"aborted":true}` → 管线在阶段边界停止 → error 终态快照含「Operation aborted: the user requested to stop this task.」 |
| **浏览器级 GUI 验证**（vite dev → Rust 引擎直连） | ①首页书卡发起 → 按钮转「写作中...」+ Foundry 面板显示阶段、引擎落 `book:b1:write` running 快照;②任务在途进 settings 页（BookDetail 挂载恢复）→ 直接显示「停止写作」+「后台正在写作」提示;③点击停止 → 按钮翻回「写下一章」+ 失败横幅「Operation aborted…」→ 引擎终态快照 error/aborted/completedAt 齐备;④另观察到自然失败路径（mock 内容违规）→ write:error 收敛同样正确 |

## 四、过程教训

1. **单线程 mock LLM 会串行化并发端口调用**：管线九路端口并发打 LLM,python `HTTPServer`（单线程）把每个挂起请求排队——每路 3s 也能拖出 60s+ 总时长,abort 检查点迟迟不到,冒烟假阴性。并发场景的 mock 必须用 `ThreadingHTTPServer`。
2. **set -e 下 `grep -q … && break` 的假绿**：grep 失败被 `&&` 列表豁免,轮询循环空转到超时也不会走 FAIL 分支（`[ … ] && { FAIL; exit 1; }` 同理被豁免）——冒烟脚本的失败路径必须显式验证过一次。
3. 中止语义是「阶段边界」不是「即时硬杀」:在途 LLM 调用完成后才停——冒烟断言要按「在途时长 + 下一边界」设定窗口,别按即时生效预期。

## 五、遗留

- Dashboard 书卡不做恢复/停止（卡片场景,详情页承担）;draft 接线待两端管线支持信号后同批做。
- 书工作台（`#/book/:id`，ChatPage）的「写下一章」chip 走聊天 agent 生产任务路径（确认式任务,检查点/可停止 174 号前已具备）——与本次 REST 面接线并存,语义各归其位。
- 总体方案剩余 backlog：W-A4b（secrets 掩码,需 UI 确认）、W-B4/B5、W-C3/C4（产品级）、W-D4（bench 对象需重选型）、en/ja README、TROUBLESHOOTING 复审。
- 推送须在 Fork 图形端执行（既有约定）。
