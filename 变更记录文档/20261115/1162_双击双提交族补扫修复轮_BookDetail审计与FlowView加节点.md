# 1162 号 · 双击双提交族补扫修复轮（BookDetail 审计 + FlowView 加节点）

- **类型**：fix（生产 2 文件净增约 30 行，零删除既有逻辑）
- **范围**：`packages/studio/src/pages/BookDetail.tsx`（审计按章守卫）+
  `packages/studio/src/pages/FlowView.tsx`（加节点在途互斥+双击吞没）
- **家族**：955 号双击双提交族补扫（1092 号竞态守卫轮明确备案的姊妹族）

## 一、家族扫定性（studio 全部 POST/DELETE 站逐站核验）

全 studio 变更型请求共 **50 站（26 文件）**，逐站以「守卫形态×危害等级」定性：

| 站点群 | 定性 |
|---|---|
| RecallTest/Deconstruct/Director/ServiceConfig/Roster/TranslationManager×4/ImportManager×3/RadarView×2/StoryGraphTree/OnboardingWizard/StyleManager/WorkInspector(OCC)/CreativeMethods/ChapterWorkspace/PlayHud/Codex（PUT 面） | **已有守卫**：busy/键控互斥（`runSave`/`runBookAction`/`runExclusive`/`perform+disabled`）或服务端 OCC（`expectedCurrentRevisionId`） |
| BackupPanel 全站 | 已有 `runExclusive` 互斥壳（955/956 号注解在位） |
| Dashboard/GenreManager 删除站 | 确认对话框先关闭门=有效单击化；服务端幂等 |
| BookDetail consolidate/foundation/plan/compose/repair-state | 已有 `runBookAction(key,…)` 键控互斥 |
| BookDetail rewrite/revise/sync | 已有按章数组守卫+disabled+spinner |
| approve/reject（BookDetail :1010/:1020） | **备案不扩面**：服务端 `approve_chapter` 幂等无条件置位恒 200（books_state_routes.rs:326），双击零害 |
| export-save（BookDetail :887） | **备案不扩面**：落盘固定路径 `books/{id}.{fmt}` 同内容重写（幂等） |
| AntiAi `removeEntry` / AssetLibrary `remove`（DELETE） | **备案不扩面**：双击第二发 404 假失败 notice，毫秒级窗口纯提示面 |
| **BookDetail 审计（:1032 内联 async）** | **真缺口一**：同排兄弟全守卫唯 audit 裸奔；`run_audit_flow` 重计算+hub 双广播+章节索引回写，双击=双审计流 |
| **FlowView `onAddNode`（:244）** | **真缺口二**：`genNodeId()` 每发新 id，双击两发都在响应前入队→两节点重复入图 |

## 二、缺陷与修复

- **修一（BookDetail.handleAudit）**：`auditingChapters` 按章数组守卫（handleRewrite
  同形：add / finally remove）+ `disabled` + spinner 换 ShieldCheck + `evt.detail>=2`
  双击吞没。
- **修二（FlowView）**：`posting` 在途互斥（CreativeMethodsEditor
  `perform+disabled={busy}` 同形）包 `post` 全出口 + `onAddNode` 的 `evt.detail>=2`
  双击吞没。
- **补强动因（走查揭示）**：delta/audit 本地飞行仅毫秒~秒级，而真实双击第二发落
  在 ~80-150ms 间隙——飞行互斥覆盖不了规范双击时序（本机 flight-window 守卫族的
  共同边界）；浏览器原生 `MouseEvent.detail` 计数精确识别双击手势第二发，两次有
  意单击 detail=1 各自放行（不误伤、零定时器、0GC）。飞行互斥与手势吞没互补成
  完整防御：前者管飞行重叠（慢后端/慢网），后者管规范双击（快后端）。

## 三、走查活体闭环（IAB + walkthrough-env rust 路径，1112 号同程序）

- **绿相四腿**：F1' 加节点 click1 后微任务边界读 `disabled=true`（React 离散事件
  flush 落位实证；落定后解除）+ 节点恰 +1；F2 detail=2→0 节点吞没 / detail=1→
  1 节点放行；B1 审计 click1 飞行中 `disabled=true` + 恰 1 条 "Audit passed"
  alert + 落定解除；B2 detail=2→alert 不增 / detail=1→+1。
- **NEUTER 红相**：摘两处 detail 守卫（`void evt` 占位防 unused）→重建→恰红双腿
  （FlowView detail=2→节点入图；BookDetail detail=2→alert 直发）=双缺陷形态复现。
- **互逆恢复**：守卫回填→重建→**bundle 同哈希**（`index-DPg9EBMG.js` 与首轮绿相
  完全一致，零残留的比 diff 更强证明）→复绿点验三腿全绿（detail=2 吞没恢复×2 面
  +detail=1 放行恢复）。
- **环境教训备案**：dist 重建后引擎重启仍不够——浏览器按 URL 缓存旧 index.html
  整包启动旧资产（指纹对比实锤 `DPg9EBMG`≠`4xMNEoTr`）；须**换 query 串缓存击穿**
  再走查。已知边界备案：同一任务内 `el.click()`×2 同步派发不经 React 重渲染（批处
  理到任务尾），不构成规范双击输入面，真实输入事件均为独立任务。

## 四、验证

- studio 全套件 **165 文件 900 例全绿**（改动零测试面破坏）+ typecheck 双
  tsconfig 过 + 两触文件 U+FFFD 净 + NEUTER 残留 grep 净；engine-rs / src-tauri /
  core / cli 零改动；走查环境收尾 pgrep 零残留 + tmp 清净（1087 治本形态）。

## 五、编号备案

- 动工时宣称 1154（其时并行已落 1153=8f9365be）；写档前双查发现并行会话已推进
  至 1154-1160 序列（其 1158-1160 号提交明示「M BookDetail.tsx+M FlowView.tsx
  未落库——让号隔离持续」——并行侧全程为我方在途物让号），让 1161（其 trail
  下一号），本轮取 **1162**。生产注释已同步改号（1154→1162，零 stale）。

## 下一号

自 1163 起；出口=下一巡检周期 / earendil-mz 单包合并（触发条件制持续）；用户侧
待办=Fork 推送（含 955-1162）。
