# 1070 号 · failure 路径双语 hint 补齐轮（1069 缺口清偿）

> 1069 出口执行。agent 链 failure 存状态路径的操作建议补齐。

## 一、实施（判据族复用 + 两处命令面接线）

- `localization.ts` 新增 **`buildAgentFailureHints(failure)`**：按特征追加双语
  hint——判据族与 doctor.ts catch 分支**同源复用非新造**：
  Connection error/ECONNREFUSED/fetch failed → baseUrl 建议 / 400 → stream+model
  建议 / 401 → Key 建议 / 403·429·quota 族 → 配额建议
- 两处命令面接线：`agent.ts` failure 分支 hint 循环 logError；`interact.ts`
  非 json 输出 hint 行（json=机器消费面不加行）

## 二、TUI 面不接线备案（设计决策）

`agent-input` 的 responseText 进 session 持久化（appendInteractionMessage）——
hint 追加会污染对话历史。TUI 失败展示已有 dashboard lastError（1057 接线）+
交互补问能力，无需 hint 行。

## 三、红绿闭环与验证

- 测试补 3 例：**Connection error.（SDK 最高频形态——1069 活体实证）→ baseUrl
  建议** / 401 → Key 建议 / 无关文案空数组不误伤
- **NEUTER1070 摘判据恰红 2/2**（空数组例语义正确仍绿）→互逆恢复绿
- 验证：cli typecheck 0 错误 + **90 例全绿**（87+3）+ U+FFFD 净

## 四、下一号自 1071 起

出口=下一巡检周期 / earendil-mz 单包合并（触发条件制已更新）/ 用户侧待办=Fork 推送（含 955-1070）。
