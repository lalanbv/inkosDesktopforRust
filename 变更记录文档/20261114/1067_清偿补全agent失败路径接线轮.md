# 1067 号 · 清偿补全轮——agent 链失败路径 zh 承接三处接线

> 1066 出口执行。1057 清偿完整性自查发现的遗漏面补全。

## 一、遗漏面实锤

errorMessage 生产链（provider.ts:211 / pi-stream.ts:177/:272——**捕获存状态而非
抛出**）携带 provider EN 诊断块；消费三处均未接线：

| 消费点 | 形态 |
|---|---|
| tui/agent-input.ts:161 | `result.errorMessage ?? (completion blocked message)` |
| commands/interact.ts:107 | 同构 |
| commands/agent.ts:70 | 同构（logError 直显） |

——1057 只接了 dashboard.tsx 抛异常路径，捕获存状态路径（agent 链正常返回但
stopReason=error/blocked）为遗漏面。

## 二、实施（单源 helper + 三处接线）

- `localization.ts` 新增 **`localizeAgentFailure(failure)`**：undefined 透传 /
  字符串走 localizeTuiRuntimeMessage 单源映射
- 三处接线：import + failure 计算包装（一行/处）

## 三、红绿闭环与验证

- 测试补 2 例（undefined 透传 / 429 生产派生样例映射）
- **NEUTER1067 摘映射恰红 1/1**（undefined 例语义正确仍绿）→互逆恢复绿
- 验证：cli typecheck 干净 + **87 例全绿**（85+2）+ U+FFFD 净

## 四、下一号自 1068 起

出口=下一巡检周期 / earendil-mz 单包合并（触发条件制已更新）/ 用户侧待办=Fork 推送（含 955-1067）。
