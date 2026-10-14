# 690 号：skills:change 失效事件重施（R44/566 号随上游 2.0 合并丢失面清偿）+ loopback 守卫等价性裁决

日期：2026-10-15。类型：fix(studio)。前置：689 号（ba0e3ed6）。

## 一、缺口与裁决

688 号把 server.ts 切到上游 2.0 后，fork 的 `skills:change` SSE 失效事件（R44/566 号：author_skill 写链/skill import/delete 后广播，UI 打开中的技能面板热刷新）从 TS 侧消失——合并保全下来的 UI 消费方（ChatPage/ProjectSettings 的 `useSkillsInvalidation`，R44/566 接线）在 Node 引擎下永远等不到事件，技能面板热刷新静默失效（Rust 引擎路径完好，其广播独立实现）。

**loopback 守卫等价性裁决（本号最重要的「不做」决定）**：立项时怀疑 2.0 server 缺回环守卫（fork loopback-guard.ts 模块存活但未接线），勘察推翻——上游 2.0 自带等价信任边界（`STUDIO_HOST_FORBIDDEN`/`STUDIO_ORIGIN_FORBIDDEN` 中间件 + CORS 只反射已准入 Origin + bind 感知：非回环绑定时按配置网络域放行），fork 守卫的差异化面（`INKOS_ENGINE_LOOPBACK_GUARD` 杀开关、`INKOS_ENGINE_ALLOWED_ORIGINS` env 白名单）在产品壳（src-tauri/桌面 webview 回环自洽）中零使用。**重复接线=双信任边界冲突面**，裁决不接线；fork 模块留树（backup 可复活）。

## 二、修复（最小面）

- `server.ts` skills import/delete 两路由补 `broadcast("skills:change", { reason: "imported" | "deleted" })`（fork 5204/5217 逐字镜像；2.0 路由体与 fork 完全一致，仅差广播行；author_skill 写链广播随 fork 会话聊天流退役，归 2.0 迁移项目）。
- 新测 `skills-change-broadcast.test.ts`：SSE 帧读取器（fork 582/583 号机制移植）——流开启 ping 后 import/delete 各自 3s 窗口内收到 `event: skills:change` 且 reason 匹配。

## 三、红绿与门禁

- **红绿实证**：stash 往返（撤广播行）→ `import 未广播 skills:change` 红；恢复 → 绿。
- studio **642/642**（641+1）、typecheck 0。
- 消费链核验：`useSkillsInvalidation`（sse）→ `STUDIO_SSE_EVENTS` 含 `skills:change`（688 号并集已保）→ refetchSkills 热刷新恢复。

## 四、教训

1. **「模块在树里」≠「防线在工作」**：688 号保全了 fork 的 UI 消费方与工具模块，但 server 切 2.0 后中段（广播/路由接线）断了——跨切面功能的移植清单要按「事件源→管道→消费方」全链核对，不能只看两端文件存在。
2. **守卫缺失要先勘察上游是否已有等价物**：2.0 的信任边界与 fork 异构同效（机制不同、语义相交），盲目重接线制造双边界冲突；「不做」与「做」一样需要活体证据（env 使用面为零的 grep 实证）。
