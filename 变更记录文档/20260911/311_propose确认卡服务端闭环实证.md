# 311 号：propose→确认卡服务端闭环实证 + 卡片渲染确认（310 号 mock 加固实施）

- 日期：2026-09-11
- 分支：develop
- 关联：310 号（探针——本批实施其备案的 mock 加固并取得卡片成功态）、231 号（propose→confirm 协议）
- 编号衔接：查 20260911 目录最大号 310，顺延 311
- 推送核验：origin/develop = 498253fa（300 号）未动；本地领先 301–311 共 11 提交待推

## 一、mock 加固实施（310 号备案清偿）

按 231 号 propose→confirm 协议实现**两轮状态机保真 mock**：

- 轮 1（末条 user / 自由文本）→ `propose_action` 工具调用，args 含 **action 必填字段** + instruction + `createBook` 结构化载荷（310 号失败根因即缺 action，301 号 schema 侧要求 action+instruction 必填）；
- 生产链轮（system 含 总架构师/网络小说架构师 → 5 段地基 ARCHITECT_OUTPUT；资深小说编辑 → REVIEW_PASS）；
- 工具结果轮 → 确认卡应答文本。

310 号失败根因定位确认：mock ARGS 缺 `action` 必填字段（schema 正确拒绝）——加固后 propose 工具执行**成功**。

## 二、成功态实证（生产 bin + 真实浏览器）

| 层 | 结果 |
|---|---|
| 服务端 | ✓ `POST /agent` 自由文本 → propose_action **status: completed**（session 持久化，assistant 应答「已生成确认卡，请在下方点击确认。」） |
| UI 卡片 | ✓ 「propose_action 0s 已完成」+ 确认卡（创建长篇书籍 + instruction 摘要 + 「已执行」标记）渲染 |

## 三、边界（如实记录）

确认后的**点击自动化**未走通：卡片渲染态中确认按钮的可见性/选择器未能在本探针会话定位（卡片生命周期状态机与服务端 pending 提案的联动属前端交互细节）。服务端 propose→pending 链已闭环；确认后的执行链路由 291 号按钮路径（requestedIntent 直达生产）与 e2e（6818 号用例）双覆盖。

## 四、验证性质与遗留

零代码改动批（mock+探针）。遗留：301–311 共 11 提交待推送；并行会话三件第三十九轮在途未落库；两项默认值、ja A/B、历史瘦身待用户决策。
