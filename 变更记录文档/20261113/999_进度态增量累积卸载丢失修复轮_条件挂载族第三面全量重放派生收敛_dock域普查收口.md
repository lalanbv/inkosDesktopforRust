# 999 号：进度态增量累积卸载丢失修复轮——条件挂载族第三面（全量重放派生收敛）+ dock 域普查收口

日期：2026-11-13。开工三查净（HEAD=f8880e7c、工作区空、无并行会话）。996/998 同族族扫收口（修洞按族扫纪律：985/958 口径——两轮「发现一处修一处」后本轮系统性扫净）。

## ① dock 域系统性普查（条件挂载族收口）

ContextDock `visible` 条件渲染卸载域（996/998 已修两处）全量盘点：
- 面板注册表四区组件 useState 逐个定性：`ChaptersSection.chapters` / `FoundationSection.files` / `SummarySection.worldOverview` / `CharacterSection.roles` 全为**数据加载态**（重挂载自动重拉，语义正确）——非易失交互态。
- **唯一真面：`ProgressSection.completedSteps`（Set 增量累积）**——effect 只处理 `sse.messages` 最后一条（增量形），dock 开合卸载期间 store 继续累积的中间 log 事件在重挂载后被跳过 → **中间步骤完成态丢失，进度条回退显示不完整**（写作进行中 dock 开合即触发）。
- 普查结论：族面收敛完成——996（筛选态/偏好级 localStorage）+ 998（编辑草稿/数据级 sessionStorage）+ 999（进度态/派生级全量重放）三面全清，dock 域无剩余易失交互态。

## ② 修复：全量重放派生（消除增量形态，比持久化更正的修法）

进度态本就是消息流的**纯派生**——`deriveProgressState(messages)` 纯函数（export 供测试）全量遍历重放（book:creating/write:start 清集 → book:created/write:complete 收敛全集 → log 推进 active/completed），幂等且卸载期事件不丢。组件内三段增量 useState+useEffect 全部删除，改一行派生调用（useState/useEffect import 随之移除）。O(n) 重放每事件（单次写作 log 数十条）——小数组重放远优于易丢增量态（0GC/正确性同向）。

## ③ 红绿闭环

- 纯函数测试 5 例（node 环境零 mock——deriveProgressState 纯导出）：①**卸载期累积流重放**（999 主场景：[write:start, log A, log B, log C] → A/B 进 completed、C 为 active——增量旧形只处理 C 即红）②init 序列收敛 INIT 全集 ③write:complete 收敛 WRITE 全集且 active 清空 ④idle 下 log 不累积+空流全 idle ⑤全量重放幂等（重挂载重放安全）。
- NEUTER999 必红活体：函数入口 `messages.slice(-1)` 截断（增量旧形语义）→ **恰红 3 例**（主场景+init+write——全部依赖多消息流；idle/幂等两例恒真不受影响——分形语义自洽）→ 互逆恢复 5/5 复绿。

## ④ 门禁

typecheck 双 tsconfig 过；studio 全套件 **155 文件 856 例全绿**（+5）；U+FFFD 净；Rust 树零改动（引用 998）。提交路径限定三文件：组件 + 测试 + 本文档。

## 备案

1. deriveProgressState 导出供测试——纯函数导出面最小（无额外运行时 API）。
2. 重放 O(n) 每事件：n=单次写作 log 条数（数十），无 perf 触发；若未来 SSE 消息量级增至千条/写作轮再议窗口化（截断到最近 start 事件即可，语义无损）。
3. 条件挂载族三轮收官（996/998/999）——「ContextDock visible 条件渲染域内组件的易失态」普查全净；后续新增 dock 面板组件时按本族判据自查（交互态持久化或派生化）。

下一号自 1000 起（编号进三位→四位）；出口回归常备；用户侧待办 = Fork 推送（含 955-999）。
