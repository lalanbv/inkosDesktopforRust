# 1025 号 · 升级态后首 IAB 全链活体走查轮——write-next 端到端

> 1024 出口执行。1014-1022 十轮升级对**产品链路**的活体验证（1021 只做了代码
> 审查维度）——walkthrough 环境（mock LLM+Rust 引擎+fixture+dist 08:16 新鲜
> 构建）+ IAB 浏览器真实旅程。**零代码改动轮**。

## 一、走查旅程与结果

1. **write-next 全链真实写章**：首页书卡「写下一章」→ mock LLM 全链（mz 0.73.1 agent loop + typebox 1.4.7 schema 面 + 1019 适配层 + 1020 投影）→ **第 3 章落盘**（`0003_风起：林动.md` 文件系统实锤）——升级态产品核心循环端到端 ✓。
2. **UI 活体三章显示**：书页 ChaptersSection 显示 001 镜中醒来/002 风起/003——SSE bookDataVersion bump → 竞态守卫面（1018 修复）活体 ✓。
3. **SSE 实时**：全程 sse-ok ✓。

## 二、备案

- IAB tab 为 1020 走查遗留会话（跨轮持久）——走查前 goto 显式导航+断言当前页（1020 遗留的 translation 页导航残留无碍，显式路由恢复）。
- fixture 书的 chaptersWritten 字段在 books 端点响应中形态变化（undefined）——文件系统为落章真源，非缺陷。

## 三、下一号自 1026 起

出口=earendil-mz 最终单包合并（远期）/ 下一巡检周期 / 用户侧待办=Fork 推送（含 955-1025）。
