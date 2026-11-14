# 1079 号 · studio 消费点锚定补全轮（1057 链路 use-api 面测试锚）

> 1078 出口执行。zh 承接链的 studio hooks 面消费点测试锚补全。

## 一、缺口与实施

- **缺口**：映射函数有 error-copy.test.ts 9 例锚，但 use-api 消费点
  （readErrorMessage → localizeKnownRuntimeMessage）**无直接锚**——消费点回归
  （误删调用改直通）会静默漏网
- **实施**：新增 `use-api-error-copy.test.ts` 3 例——经 `fetchJson` 公开导出 +
  `deps.fetchImpl` 注入（use-api 既有测试友好设计）：
  1. error:string 形态（409 生产字面 1004 号）→ ApiResponseError.message = zh 映射
  2. error:{message} 对象形态（provider not-set 块 1006 号）→ 同款映射
  3. 非 json 错误 → status/statusText 直通（映射不命中不误伤）

## 二、红绿闭环与验证

- **NEUTER1079**：摘 use-api.ts:69 消费点映射（直通）→**恰红 1/1**（string 例红；
  :78 对象例与直通例语义正确仍绿）→互逆恢复绿
- 验证：studio typecheck 0 错误 + **898 例全绿**（895+3）+ U+FFFD 净

## 三、zh 承接链锚定终态（五路径全锚）

studio server 映射（映射函数 9 例+消费点 3 例）/ TUI 抛异常 / 失败存状态 /
SDK 原文族 hint——单测+NEUTER+活体三重，消费点级回归防护闭合。

## 四、下一号自 1080 起

出口=下一巡检周期 / earendil-mz 单包合并（触发条件制已更新）/ 用户侧待办=Fork 推送（含 955-1079）。
