# 173 · 第三批：CORS 回环反射收紧 + 172 号停机超时回归修复 + duel 真跑门禁 + serde_yaml_ng 迁移

- 日期：2026-09-04
- 模块：engine-rs（server/mod.rs / loopback_guard.rs / bin / strangler_duel.rs / Cargo.toml + 4 文件 serde_yaml 面）、packages/studio（server.ts / server.test.ts）、总体方案文档、根 README
- 类型：feat + fix + chore + docs
- 关联：170 号（守卫首批）、172 号（W-B2 超时语义缺陷来源）、[重构优化总体方案_v1](../../开发时SpecCoding'sPlan/inkosDesktop/06_全局重构优化/重构优化总体方案_v1.md)（W-A4a / W-C6 → 已落地标注同步）

## 一、背景

第三批拣选总体方案剩余 backlog：**W-A4a CORS 收紧**（`*` → 回环 Origin 反射，需双端同步 + duel 语义评审）。实施期取证发现两处被掩盖的欠账，一并处置：

1. **duel 门禁自 170 号起 latent 红**：`bin_process_static_face_duel` 的 CORS 断言仍锁 125 号 `ACAO:*` 语义，而 170 号守卫已先行 403 非回环 Origin（ACAO=None）。因 duel 用例需 `INKOS_DUEL=1` 才真跑（未设时早退 return 也计入 pass），170/172 两次「集成 8 全绿」实为跳过。
2. **172 号 W-B2 生产级回归**：`timeout(5s)` 包在 serve **整体生命周期**上——引擎启动 5 秒后必被砍掉自退。短于 5s 的手工冒烟（先 kill）与静态面 duel（1.3s 跑完）从未触及；`INKOS_DUEL=1` 真跑 sse_event_face_duel（20s+）一跑即中（connection refused，bin 日志有 "drain timed out" 而无 "shutdown signal received"——超时自然到期而非信号到达）。

## 二、改动

### W-A4a CORS 回环 Origin 反射（双端同步）

- **Rust** `engine-rs/src/server/mod.rs`：`sidecar_cors_layer()` 由 `AllowOrigin::Any`（ACAO `*` 恒设，含无 Origin 请求）改为 `AllowOrigin::predicate` **反射**——回环主机 Origin（localhost/127.0.0.1/[::1]/[::]，端口不限）、`tauri://localhost` 或 `INKOS_ENGINE_ALLOWED_ORIGINS` 白名单命中 → ACAO=请求 Origin；其余（远端/`null`/无 Origin）→ 不发 ACAO。判定复用 `loopback_guard::origin_is_allowed`（本批转 `pub`）+ 新抽 `loopback_guard::env_extra_origins()` 共享 env 解析，**守卫与 CORS 两层面判定同源**——守卫被 `INKOS_ENGINE_LOOPBACK_GUARD=0` 关闭时 `*` 放大面也不复活。新增显式白名单形态 `sidecar_cors_layer_with_extras(Vec<String>)` 供测试/嵌入装配。preflight 语义：tower-http 0.5 对不允许的 preflight 仍回 200 仅缺 ACAO（浏览器以无 ACAO 拒绝，与 Hono 204 无 ACAO 同效）。
- **Node** `packages/studio/src/api/server.ts`：`cors()` 默认参 → `cors({ origin: (o) => originIsAllowed(o, env 白名单) ? o : null })`，复用 `loopback-guard.ts` 已导出的 `originIsAllowed`，env 白名单与守卫中间件同源（`guardOptionsFromEnv`）。方法族/头镜像保持 Hono 默认参（与 Rust 侧逐项对齐）；非 `*` 后 Hono 自动补 `Vary: Origin`。
- 零影响面论证：桌壳 webview 直接导航 `http://127.0.0.1:{port}/`（main.rs:843）**同源**，不触发 CORS；跨源消费方仅 vite dev / 浏览器直连，Origin 恒回环 → 反射后行为不变。
- 单测：Rust `cors_layer_adds_headers_only_for_origin_requests` 重写为 `cors_layer_reflects_loopback_origins_only`（回环反射×3 / 远端+null 无 ACAO / 无 Origin 无 ACAO / 回环 preflight 反射+方法+镜像 / 远端 preflight 无 ACAO）+ 新增 `cors_layer_extra_origins_reflected`（白名单精确命中反射、前缀相近不命中）；studio 新增 describe「CORS loopback origin reflection」两用例（对称矩阵，Hono 204 形态）。

### 172 号 W-B2 超时语义修正

- `engine-rs/src/bin/inkos-engine-server.rs`：`main` 移除 `timeout(5s, serve)` 整体包裹（改为直接 `serve.await`，io 错误显式回传）；`shutdown_signal` 在 `hub.shutdown()` 后 **spawn 5s drain 看门狗**（`std::process::exit(0)` 强退砍连接）——drain 正常完成时进程先行退出、看门狗任务随之消亡；在途请求 hang 死时才由看门狗兜底。原「5s 兜底」本意（锁排空段）保留，错误作用域（整个服务生命周期）纠正。

### duel CORS 块语义更新 + 真跑门禁恢复

- `engine-rs/tests/strangler_duel.rs` `bin_process_static_face_duel` CORS 块重写为新语义四段：回环 Origin GET（`/api/v1/books`，**双端同位路径**——TS 侧无 `/api/v1/health`，旧断言只查头不查状态故从未暴露 404）→ 反射断言；远端 Origin GET → 双端 403（守卫）+ 无 ACAO；回环 preflight → 双端 2xx + 反射 + 方法/头；远端 preflight → 双端 403 + 无 ACAO。
- `INKOS_DUEL=1` 真跑 8/8 全绿（含修复后的 sse_event_face_duel 与 static_face_duel）。

### W-C6 serde_yaml → serde_yaml_ng 迁移

- 评估结论：serde_yaml 0.9 停维护（RUSTSEC-2024-0320）；`serde_yaml_ng 0.10` 为同 API 维持续作（drop-in），使用面仅 4 文件（`runtime_writer.rs` rule-stack 序列化、`genre_profile.rs`/`book_rules.rs`/`skills/external_loader.rs` frontmatter 解析），全部受控输入。备选 `serde_yml` 弃选（API 漂移与社区质量争议）。
- 落地：`Cargo.toml` 换依赖（注释同步）+ 4 文件 `serde_yaml::` → `serde_yaml_ng::` 机械改名。**输出等价性验证**：全量 1227 lib（含 golden 向量、frontmatter-empty 语义断言）+ 194+70 集成 + duel（对齐 TS js-yaml）全绿——序列化/解析字节面行为无漂移。

### W-A4c capabilities 复核（登记，无动作）

- `src-tauri/capabilities/main.json` 复核：仅 notification:default / dialog:default / core:window:allow-start-dragging / window-state:default 四项，逐项有成文理由（ACL 拒绝防误报、拖拽通道、窗口状态）。已是细粒度收紧态，总体方案该子项关闭。

### W-A4b secrets 掩码（维持 backlog）

- 复核：`GET /api/v1/services/:service/secret` 回明文，但 service-detail 页直接消费做编辑展示；远端读取面已由回环守卫关闭。掩码默认需改 UI 交互（掩码占位 + 重输入替换），属产品决策，维持「需 UI 需求确认」backlog，不擅改。

## 三、验证

| 项 | 结果 |
| --- | --- |
| `engine-rs cargo test` 全量 | **全绿**：lib 1227（基线 1226 + CORS 矩阵净增 1）+ 集成 194 + 70 + 8，0 失败 |
| `engine-rs cargo clippy --all-targets -- -D warnings` | 零告警 |
| `src-tauri cargo test` | 全绿（466 + 集成套件，0 失败） |
| `packages/studio` vitest（server.test.ts + loopback-guard.test.ts） | **175/175 绿**（含新增 CORS 两用例） |
| `INKOS_DUEL=1 cargo test --test strangler_duel` | **8/8 真跑全绿**（修复前：static_face latent 红 + sse 红于 172 号回归） |
| 停机冒烟（修正后 bin） | 健康就绪 → 存活 6s+（旧代码 5s 即自退）→ `kill -TERM` → "shutdown signal received" 优雅退出，看门狗未误触发 |

## 四、过程教训与登记

1. **「跳过计入 pass」的门禁盲区**：duel 系 8 用例未设 `INKOS_DUEL` 时早退 return，cargo 计 ok——「集成 8 全绿」不能作为 duel 面验证证据。后续涉及契约面的验证须显式 `INKOS_DUEL=1 cargo test --test strangler_duel` 真跑（本批已恢复为绿基线，成本约 40s）。
2. **超时兜底的作用域**：给「排空段」设超时不能包在整体 serve future 上；axum `with_graceful_shutdown` 的正确形态是信号链内后置看门狗。
3. **双端同位断言**：duel 对双端共路径断言时须选两实态都存在的端点（`/api/v1/health` 仅 Rust 有；`/api/v1/books` 双端有），断言状态码 + 头而不仅头。

## 五、遗留

- 总体方案剩余 backlog：W-A4b（secrets 掩码，需 UI 确认）、W-B4/B5（插件路由 / WIT batch，待插件生态起量）、W-C3/C4/C5（系列书导入 / 时间线 / 检查点显式化）、W-D4（bench 常态化门禁——benches 尚未建立，需先建 plugin_execute/scanner 两 bench）。
- TROUBLESHOOTING.md 全面复审、en/ja README 未译（既有遗留）。
- `sse_event_face_duel` 对 172 号及更早基线不可复跑为绿（依赖本批 bin 修正），基线可比性自 173 号起。
- 推送须在 Fork 图形端执行（本机 CLI 无 GitHub 凭据，既有约定）。
