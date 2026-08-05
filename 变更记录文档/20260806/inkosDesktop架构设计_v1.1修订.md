# 变更记录：inkosDesktop 架构设计 v1.0 → v1.1

| 项 | 值 |
|---|---|
| 日期 | 2026-08-06 |
| 类型 | 文档修订（设计文档二次审阅修正） |
| 文档 | `开发时SpecCoding'sPlan/inkosDesktop/01_架构设计/inkosDesktop架构设计.md` |
| 版本 | 1.0 → 1.1 |
| 触发 | 用户要求"二次审阅架构"后，对 inkos v1.6.3 上游源码逐条核查承重假设 |

## 核查方式

直读 inkos v1.6.3 上游源码：`packages/cli/src/commands/studio.ts`、`packages/studio/src/api/index.ts`、`packages/studio/src/api/server.ts`、`packages/studio/src/hooks/use-api.ts`、各 `package.json`、`@hono/node-server/src/server.ts`。

## 修正项（9 项）

| 编号 | 级别 | 修正 |
|---|---|---|
| H1 | HIGH | 生产端口：~~双端口 :4567/:4569~~ → **单端口统一服务**（默认 :4567，`INKOS_STUDIO_PORT`）；:4569 仅 dev |
| H2 | HIGH | 安全：inkos **默认绑 0.0.0.0**（非 localhost），壳层须用 OS 防火墙/沙箱锁回 loopback |
| M1 | MED | 数据目录：~~`HOME`/`INKOS_HOME`~~ → **`INKOS_PROJECT_ROOT`**（`argv[2]→env→cwd()`） |
| M2 | MED | 密钥：Studio 模式 `.inkos/secrets.json` **优先于 env**；给出两条注入路线取舍 |
| M3 | MED | SPA baseURL：同源相对 `/api/v1`，换端口自动跟随，~~initialScript 重写~~ 删除 |
| M4 | MED | SSE 事件：~~`audit:*/revise:*/import:*`~~ → 实际 ~25 个（`tool/book/write/draft/daemon/agent/...`） |
| M5 | MED | daemon：~~独立守护进程~~ → **Studio 进程内 Scheduler** |
| L1 | LOW | sidecar 必须随包**预构建 dist/**（禁止运行时 `npx vite build`） |
| L3 | LOW | 路径 `inkosforRust` → `inkosDesktopforRust`（元数据 + §16.1） |

## 涉及章节

元数据、§2.1、§2.2、§3.2（架构图）、§4（isolation 行）、§5.1、§5.2、§5.3、§6.1、§6.2、§6.4、§7、§8、§12（Phase 1）、§13、§14（开放问题→核查结论）、§16.5、§17、新增 §18 修订历史。

## 结论

γ 架构方向正确，且实际比 v1.0 设想更易实现（单端口 + 同源 baseURL 减负）。2 个 HIGH 地基性错误已修正，文档可进入 writing-plans 阶段。剩余 5 项为 writing-plans 实测确认项（见 §14.1），不阻塞架构审批。

## 关键证据（file:line）

- 生产单端口：`studio.ts`（CLI 默认 4567）、`api/index.ts`（`startStudioServer(root,port,{staticDir})`）、`server.ts:6192-6220`（SPA fallback）、`server.ts:3066`（`/api/v1/events`）
- 绑定 0.0.0.0：`server.ts:6231`（`serve({fetch,port})`）→ `@hono/node-server/src/server.ts`（`listen(port, undefined)`）→ Node 默认所有接口
- 数据目录：`api/index.ts`（`argv[2] ?? INKOS_PROJECT_ROOT ?? cwd()`）
- 密钥优先级：`server.ts` `loadSecrets(root)`、search_doc 密钥解析链
- baseURL：`use-api.ts`（`const BASE = "/api/v1"`）
- daemon：`server.ts:3849-3878`（进程内 Scheduler broadcast `daemon:*`）
