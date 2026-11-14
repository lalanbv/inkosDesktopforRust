# 1024 号 · 回归审查轮——earendil 1.5.2 归并增量面（双栈交界 + pi-dispatch）

> 1023 出口执行。1021 回归审查的增量面（1022 归并轮新增）——两审查面定性，
> **零代码改动轮**。

## 一、面 1：双栈交界同形性（1.5.2 AgentMessage 探针）

earendil 1.5.2 的 `UserMessage{role:"user",content:string|parts}` / `AssistantMessage{role:"assistant",content:parts}`——**核心字段与 0.87.1 同形** ✓。agent-session 的双栈边界 cast（1018/1019 落地的三点）所依赖的「sanitize 消费面只读 role/content 核心字段」假设在 1.5.2 下**成立**——cast 语义有效，无需处置。

## 二、面 2：pi-dispatch 消费面（1.5.2 API 签名核对）

1.5.2 的 `streamSimple` 签名 context 类型名变化（Context→TranscriptContext）——仓内 pi-dispatch 的 **per-API 直调形态**（openaiCompletionsStreamSimple 等子路径直接导入+normalizeContext）自然吸收（不依赖 compat 分发链聚合签名）——**typecheck 0 红 + pi-dispatch-env-keys 27 例 + forecast flow 全绿** = 类型/行为双面已有全绿证据。**净**。

## 三、下一号自 1025 起

出口=earendil-mz 最终单包合并（远期，视上游双名收敛）/ 下一巡检周期 / 用户侧待办=Fork 推送（含 955-1024）。
