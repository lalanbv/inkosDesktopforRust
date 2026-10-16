# 741 号：ChatPage 挂载冒烟（大页另案转正第一批）+ 上游第十三巡

> 目标线：自主持续开发。挂载冒烟矩阵「大页另案」的最大项转正：
> ChatPage（1423 行核心页）空会话态挂载不崩 + 输入区在位。

## 一、上游第十三巡

fetch upstream 双 master 判据复跑：落后 upstream/master = 0，全部分支零
落后——第十三巡维持。

## 二、挂载冒烟（chat-page-mount.test.tsx，1 测）

ChatPage 空会话态挂载（mode="project-chat"）：
- **不崩**：1423 行组件全树 renderToString 成功；
- **输入区在位**：textarea placeholder「Enter command...」+ 发送钮
  `aria-label="Send"`（728 号 aria 修复在挂载面同步实证）；
- **附加技能钮 aria 在位**：Add skill / Attach files（729 号 file input
  aria 同挂载面实证）；
- **附件 file input aria**：Upload attachment files 在位（729 号）。

依赖 mock：use-api 三函数 + zustand 三 store setState 预填
（chat/service/preferences）。isZh 判定经 t mock 直返键名 → 英文分支。

## 三、验收

- studio **710/710**（+1）；typecheck 双 tsconfig 过（mode 合法值
  "project-chat"、sse connected 字段）；gate:ts 八步全绿。

## 四、教训

1. 大页挂载冒烟并非不可做——zustand setState 预填 + useApi mock 即可；
   「另案」的真实门槛是**props 类型对齐**（mode 合法值/sse connected）而非
   mock 面。
2. t mock 直返键名导致 isZh 判定走英文分支——断言对准实际分支文案
   （与 735 TruthFiles「断言对准列表条目名」同法：对准实际渲染分支）。

## 涉及文件

- packages/studio/src/pages/chat-page-mount.test.tsx（新增 1 测）
