# 759 号：@hono/node-server 公告白名单（serveStatic 零暴露面定性）

> 目标线：自主持续开发（安全可靠优先）。继承 758 号方法论：audit 红先归因
> （合并引入 vs 公告库环境漂移）再逐条定性，白名单零静默放行。

## 一、归因：公告库环境漂移（非合并引入）

`git log --oneline -2 -- pnpm-lock.yaml package.json` 复核：758 号提交后
依赖零触碰；本公告（GHSA-rmxm-3fg6-px4f）为 npm 公告库新收录——**环境
漂移**，与 688→758 号 undici/zod 序列同型。

## 二、四项实证（白名单前置核查，全部实锤）

1. **公告本体**：`@hono/node-server` serveStatic 对请求路径二次解码，
   致静态路径上的中间件被绕过（非目录穿越）；moderate；
   vulnerable `>=1.19.10 <2.1.3`，patched `>=2.1.3`（官方 registry
   `pnpm audit --json` 实取）。
2. **实际解析线**：pnpm-lock 唯一解析 `1.19.17`（spec `^1.19.15`）——
   1.x 线 latest，**仍在漏洞区间**，且 patched 线从 2.1.3 起步 = 1.x
   终态无补丁，升级必跨 major。
3. **调用面零命中**：`grep -rn serveStatic` 全仓（packages/ src-tauri/
   engine-rs/）零引用；唯一 import 为 `packages/studio/src/api/server.ts:5`
   的 `serve`（HTTP server 桥接，与静态文件中间件无关）——fork 静态
   服务为自实现路由，公告影响的「挂中间件的 serveStatic 使用方」形态
   不存在。
4. **处置**：白名单放行 + `since: "759"` + 移除条件注明「上游升 2.x
   时移除本条即恢复拦截」（688 号 vitest / 758 号 zod 同型）。

## 三、变更内容

- `scripts/audit-npm.mjs` ACCEPTED 新增 1 条（+8 行）：
  `@hono/node-server` → moderate；补丁线 2.1.3 跨 major；fork 零调用面。

## 四、验证

- `node scripts/audit-npm.mjs` → 「advisory 合计 9：白名单放行 9，拦截 0」✓
- HEAD 版本（无本条）复跑 → 「拦截 1」且红在且仅在本公告（白名单
  恰好覆盖、无静默扩面）✓
- `pnpm gate:ts` 全绿（本变更仅触 scripts/，build/typecheck/test/audit
  四步与 758 号终态一致）✓
