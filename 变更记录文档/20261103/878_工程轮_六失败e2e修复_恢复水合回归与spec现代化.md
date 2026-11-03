# 878 号：工程轮——6 个既有 e2e 失败修复（真回归 2 + spec 过期 3 + 主题时段默认 1）

日期：2026-11-03。性质：**实质工程轮（真回归修复 + spec 现代化）**。

## 一、判别结果（877 的「seed 互扰」假设被推翻）

单跑（隔离）与组跑同样红 → 非 spec 间互扰。逐一取证后的真实根因分三类：

| spec | 根因 | 类别 | 修复 |
|---|---|---|---|
| entry-session-recovery ×2 | **2.0 合并丢失 fork 恢复特性（真回归）**：`loadSessionDetail` 的 chatRequest 水合（2e839afb 10/10 引入）在 2.0 合并时被上游重写吞掉——现版只读 session/messages/task，`data.chatRequest` 无人消费 → 失败重试按钮（lastFailedSend）从不恢复；且 ChatPage 入口效果的 `session.bookId === null` 旧条件被合并带回——绑定 work 的入口会话（bookId=id）被跳过激活、落到新建草稿 | 真 app 回归 | ①恢复 chatRequest 水合（running=脱管 isChatStreaming/detachedChatRequestId、toolExecutions 合并、failed=错误消息+`lastFailedSend: failedRequest.retry ?? runtime`、completed/cancelled=清除；failedRequest 强制走磁盘投影）；②恢复会话身份字段（profileId/workId/modelOverride/serviceOverride——use-session-events 重定向的读取前提）；③删 `bookId === null` 条件恢复 2e839afb 语义 |
| analysis | 上游 2.0 harness v2 迁移（7aed0d82）把 AnalysisPanel 的 **EmotionArcChart 整体删除**（-127 行）——spec 断言已不存在的 `emotion-arc` testid | spec 过期 | 删 emotion-arc 断言（validation-panel/validation-issue/path-distribution 三个 testid 在 2.0 面板均在位，覆盖意图保留） |
| film-image | seed assetRef 用旧路径 `interactive-films/<id>/assets/nodes/s.png`——2.0 `/project/files` 端点**只服务 `works/` 狱内**（resolveProjectImageFile 前缀白名单）→ 404 → img 高 0 → hidden | spec/seed 过期 | assetRef 迁 `works/<id>/assets/...`（2.0 图生成同形态） |
| film-list | `/interactive-films` 列表走 **works 清单**（profile=interactive-film）——seed 只写 story-graph.json 不写 work.json → 清单恒空 | spec/seed 过期 | beforeAll 补 `createWorkManifest({profileId:"interactive-film"})` + saveWorkManifest |
| flow-editor | FlowView `onAddNode` 用 **`as never` 类型逃逸**传了缺 `sceneDesc`/`dialogue`/`act` 的残缺节点——2.0 delta 端点对 upsert 节点跑 `assertDisplayText`（`sceneDesc.trim`/`dialogue.forEach`）→ `.trim` on undefined → 500 → 节点不增 | 真 app 缺陷（类型逃逸掩盖） | 节点补全字段 + **移除 `as never`**（让编译器守卫）；core probe 实证：残缺节点两个不同 500、全字段节点 OK |
| （贯穿）a11y spec 的 chapter/service-detail/doctor 在夜间全量跑新红 | **默认主题按时段**（6-18 点浅、其余深，151 号 P2-3 既定）——877 全量在白天=浅色基线，878 首次夜间全量=整站 dark，暴露三处 dark 色卡（其中 doctor amber-800 是 877 修浅色时漏写 dark: 分支导致 dark 反而劣化） | 测试环境假设缺失 | a11y spec 固定 `inkos:studio:theme=light`（恢复 877 基线语义；dark 色卡专项另行）+ 补齐漏写的 dark: 分支（DoctorView amber、BookTimeline 五色卡） |

## 二、修复的核心价值：入口失败恢复（用户可见回归）

entry-session-recovery 对应的真实用户场景：创作对话发送失败（网络中断/后端重启）→ 重开应用从入口（新建书/项目对话）进入——**修复前**：失败会话与重试按钮全部消失，只剩全新空草稿（用户丢上下文）；**修复后**：入口书签恢复绑定会话 + 失败错误消息 + 「重试上一条消息」按钮 + 自动重定向到已绑定的书。水合同时恢复：running 脱管检测（isChatStreaming/detachedChatRequestId）、toolExecutions 回放合并、work 绑定身份字段（profileId/workId/modelOverride/serviceOverride）。

## 三、变异/活性证明

- Rust 侧同金标准（876 已证）；本轮 TS：修复前快照显示全新草稿（重试钮不存在）——修复后 spec 两态（入口页按钮 + 重定向后 book 页按钮+reload 持久）全过，且 `agentPosts===0`（不自动重发）断言保证「恢复≠自动执行」语义。
- a11y 必红自检在主题固定后照常工作（58 例含必红用例）。

## 四、验证

| 面 | 结果 |
|---|---|
| 6 个目标 spec（隔离） | ✓ 7/7（含 flow-editor 拖拽伴生用例） |
| **e2e 全套件** | ✓ **95/95（a11y 58 + 既有 37）零失败零回归** |
| studio vitest | ✓ 755 |
| `gate:ts` 八步 | ✓ 全绿 |
| typecheck | ✓（`as never` 移除后编译器即守卫） |

## 五、备案

- dark 模式色卡（chapter 错误横幅 destructive 深底 3.46 / service-detail 3.76 / doctor amber 深底 2.54）为**真实 dark 主题缺陷**——本轮只固定审计主题不修 dark 色卡（专题另行，涉及 dark: 分支全面普查）；DoctorView/BookTimeline 漏写的 dark: 分支已补（877 引入的劣化即时修正）。
- 主题时段默认（151 号 P2-3）+ 夜间跑套件 = 基线漂移的机制性组合，a11y spec 固定主题后消除；既有 37 spec 无主题断言不受影响。

**下一号自 879 起；出口不变；用户侧待办=Fork 推送 10 个提交（至本提交）。**
