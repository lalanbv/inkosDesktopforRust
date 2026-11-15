# 1428 号 · 四站守卫逐功能补测轮——ChatPage/Sidebar IME 守卫单测兜底（1169 修复面闭环）

> 1425 出口执行（「逐功能测试」目标驱动：全量套件绿≠每功能有专属回归锚）。零生产代码改动，新增 2 份交互测试。编号备案：动工期间并行会话连落 1426（use-api 启动门分流，已提交 4f2fddca）与 1427（裸网关 5xx 并入分类，在途）——按让号纪律取 1428。

## 一、逐功能缺口审计（全量绿下的专属锚点盘点）

1169 修复面五功能点逐一对照「专属自动化测试 / 活体走查」双证据面：

| 功能点 | 专属单测（本轮前） | 活体走查 |
|---|---|---|
| 1 ChatPage 聊天主输入 IME 守卫 | **缺**（仅 741 mount 冒烟） | 1424 ✓ |
| 2 Sidebar 改名 IME 守卫 | **缺**（仅 renderToString 骨架测） | 1424 ✓ |
| 3 RecallTestCard IME 守卫 | 1169 ✓ | 1169/1424 ✓ |
| 4 AntiAi 经验沉淀 IME 守卫 | 1169 ✓ | 1169/1424 ✓ |
| 5 RecallTestCard busy 飞行互斥 | 1169 ✓ | —（单测覆盖） |

缺口=站 1/站 2 的 IME 守卫回归只能靠人肉 IAB 走查兜底——本轮流补齐。

## 二、新增测试（两文件三/双腿）

- `ChatPage.interaction.test.tsx`（页级交互首例，mount 冒烟同款 mock 形）：三腿覆盖守卫面 `!e.shiftKey && !e.nativeEvent.isComposing` 全貌——①组合回车吞没（sendMessage 0 调用+输入保留）②普通回车恰 1 发送（sessionId+文本逐参断言）③shift+Enter 换行 0 发送。可观测面=useChatStore.sendMessage 注入 spy（:333 selector 读取，setState 先于 render 换装）。
- `Sidebar.interaction.test.tsx`（radix DropdownMenu jsdom 交互**首例**）：双腿——①组合回车 0 重命名+Dialog 不误关 ②普通回车恰 1 重命名（sessionId+新标题逐参）+Dialog 关闭。radix jsdom 桩=scrollIntoView/hasPointerCapture/setPointerCapture/releasePointerCapture（首例惯例沉淀）；驱动=userEvent.click（开菜单）+fireEvent.keyDown（守卫注入）。

## 三、过程破案（防复发备案）

- **腿②假红**：普通回车 spy 恒 0 调用——四步二分（Send 按钮同走 onSend 亦 0→源码插桩 guard1-pass→余下路径值全对）终定案：`await serializeChatAttachments([])` 把 sendMessage 推迟到**微任务**，同步断言跑早了。修=断言前 `await act(async () => {})` flush（RecallTestCard 同族 `vi.waitFor` 同理）。**代码零缺陷，测试缺 flush**——async handler 的 spy 计数断言必须先 flush，教训入案。
- 类型面：`vi.fn(async () => {})` 零参签名使 `mock.calls[0]?.[0]` 报 TS2493（零长元组）——spy 形参显式声明后逐参断言可查；`getByPlaceholderText` 返回 HTMLElement 读 `.value` 须显式转 HTMLInputElement。

## 四、验证与并行对账

- 两新文件各自单跑绿（1+1）+ 四份守卫测试族逐文件单独实跑（1/1/4/5）+ studio 全量 167 文件 **909 例全绿**（当前树含并行在途 use-api 改动：1426 已入库 +3、1427 在途——计数随并行漂移属正常，非动态用例；双跑用例全名集 diff 逐名一致证稳态）。
- studio typecheck 0 告警（两新文件初稿 TS2493/TS2393 清零后复验）；ChatPage.tsx 诊断插桩零残留（grep DEBUG-1426=0）；两新文件 U+FFFD 净；`.vitest/` json 报告临时目录清除。

## 五、勘误微提交备案

动工时按预估号 1426 编写测试文件头注释，落库前让号 1428——两文件头/描述符 stale「1426」恰与并行已落库 1426（4f2fddca）撞名，按 1169 先例「注释同步改号零 stale」sed 改号（1426→1428×4 处）+复跑绿，勘误随本文件一并微提交。Mimosa 前置扫描在 server.ts:4372-4373 两处 medium「疑似跨文件污点」（并行 1426/1427 use-api 面/既有代码，非本轮改动面）——经用户确认放行本轮路径限定提交备案在案。

## 六、下一号自 1429 起

出口=下一巡检周期 / earendil-mz 单包合并（触发条件制已更新）；用户侧待办=Fork 推送（含 955-1428，与并行 1426/1427 一并推送）。
