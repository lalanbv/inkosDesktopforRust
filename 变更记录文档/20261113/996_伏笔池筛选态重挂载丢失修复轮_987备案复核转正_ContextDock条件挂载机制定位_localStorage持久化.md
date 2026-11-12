# 996 号：伏笔池筛选态重挂载丢失修复轮——987 走查备案面复核转正（ContextDock visible 条件挂载机制定位）+ localStorage 持久化（仓库惯例形）

日期：2026-11-13 凌晨。开工三查净（HEAD=d214a74a、工作区空、无并行会话）。跨会话接力：987（并行会话）走查备案的复核转正。

## ① 987 备案面复核：定性修正「非产品缺陷」→「真实 UX 缺陷」

987 走查备案：伏笔池筛选「悬念」后首轮点击 ~800ms 内列表还原全量，定性「面板内容重载重挂载竞态，非产品缺陷」。本轮逐层定位机制并**修正定性**：

- 机制链（逐层排除后坐实）：`PendingHooksView`（筛选=本地 useState 纯客户端过滤，无 refetch）← `renderTruthBody` ← `ArtifactView`（`{loading ? Spinner : renderTruthBody}`，loading 仅 effect 依赖变化时置 true；artifactFile/sidebarView 写入面仅用户动作，无 SSE 自动重置）← **`ContextDock:96 if (!visible) return null`**——dock 开合（visible 切换）即整体卸载重建 → ArtifactView 重拉（~800ms）→ PendingHooksView remount → `kindFilter` 重置 "all" → 列表还原全量。「数据重载一次后稳定」= visible 稳定后不再卸载。
- **定性修正依据**：写作运行期间 dock 开合是高频动作（用户切面板看进度再回伏笔池），筛选态每次被静默重置——机制上无逻辑错误，但对用户是真实可复现的 UX 缺陷（筛选意图丢失）。987 的「非产品缺陷」定性的是「数据正确性」（数据无丢失错乱），本号定性的是「交互状态保持」——两者不矛盾，后者是缺陷。

## ② 修复：localStorage 持久化（对齐仓库 UI 偏好惯例）

- `loadKindFilter` / `saveKindFilter`：`inkos.hooksKindFilter` 键；脏值（非词表成员）回落 "all"；SSR（`typeof localStorage === "undefined"`）与隐私模式 try/catch 双卫兵。惯例依据：同面板 `usePanelWidth` 的 `DOCK_WIDTH_STORAGE_KEY` 即 localStorage 持久化形——仓库既有 UI 偏好惯例，非新造。
- 组件内 `setKindFilter` 包装（setState+save），两调用点签名不变。

## ③ 红绿闭环

- 回归测试 3 例（node 环境 SSR 形，内存 localStorage stub 走真实浏览器路径；实现侧 typeof 卫兵由「defaults to all」例隐式覆盖）：
  1. **remount 恢复**（987 场景直证）：预置 `suspense` → render → 仅悬念核心文本可见、情感线核心被筛掉；
  2. 脏值回落：`bogus-kind` → 全量；
  3. 无存储默认：全量。
- 首轮 3 红两波勘误（各自定位先于改正）：①测试文件是 node 环境（无 jsdom localStorage）→ 装内存 stub；②断言锚点用台账列 `H01/H02`（卡片不渲染 hook_id）→ 改伏笔核心文本锚点。**修复本身首轮即生效**（渲染输出悬念 chip violet 激活+单卡，两波红均为测试自身形态）。
- NEUTER996 必红活体：`loadKindFilter` 恒 "all" 摘持久化 → **恰红 remount 恢复例独红**（其余 4 例恒真不受影响）→ 精确互逆恢复 → 5/5 复绿。

## ④ 门禁

typecheck 过；studio 全套件 **152 文件 845 例全绿**（+6：本号 3 例与 987 轮 3 例新增面）；U+FFFD 净；Rust 树零改动（clippy/testgate/audit 引用 995）。提交路径限定三文件：组件 + 测试 + 本文档。

## 备案

1. 筛选态跨会话保留（localStorage 不清则持久）——预期行为（对齐 dock 宽度先例）；如需「仅会话内」可改 sessionStorage，无触发不预做。
2. `ArtifactView` 的 editing 分支在重拉时 `editContent` 不受影响（state 独立），暂无同族缺陷；若未来 content refetch 与编辑并发出现覆盖，再立专项。
3. 987 备案原文已在走查档（987 文档不回改）——本号文档即为定性修正的正式记录。

下一号自 997 起；出口回归常备；用户侧待办 = Fork 推送（含 955-996）。
