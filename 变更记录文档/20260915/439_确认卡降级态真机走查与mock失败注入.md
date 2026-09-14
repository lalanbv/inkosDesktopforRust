# 439 号：确认卡降级态真机走查 + mock 生产链失败注入

日期：2026-09-15
类型：test(walkthrough)——438 号 UI 变更的视觉验证闭环 + 走查脚手架失败注入能力
提交：scripts/walkthrough-mock.mjs

## 背景与动机

438 号给确认卡加了「已执行 · 生产链失败（错误详情见任务卡）」降级态，但当时验收只有
studio 827 单测 + tsc——**改的是可见 UI，却从未在真实浏览器里见过渲染效果**（红色/
图标/文案/布局全是推断）。本号补上这最后一环：真机驱动一条真实失败的生产链，眼见为实。

## 变更内容

### walkthrough-mock.mjs 新增失败注入（唯一代码变更）

- 环境变量 `WALKTHROUGH_MOCK_FAIL_ARCHITECT=1`：架构师链（同人架构师/网络小说架构师/
  总架构师三类 system 标记）的 LLM 调用一律返回 **HTTP 400** + JSON error body。
- **400 而非 500 的取舍**：205 号 `is_retryable_llm_error` 对 5xx/429/传输层词汇重试
  （2 次退避），400=非瞬态立即失败——注入路径快且不触发 R25 接管链的等待。
- fixture 数据面不受影响（write-next 走 planner/writer/settler/审稿，无架构师调用），
  实测带旗标拉起 walkthrough-env 全 fixture 断言照常通过。

## 真机走查记录（IAB + 生产引擎 debug 档 + 重建后的 studio dist）

环境：`WALKTHROUGH_MOCK_FAIL_ARCHITECT=1 node scripts/walkthrough-env.mjs
--port 8790 --mock-port 1235 --root /tmp/inkos-walk439`

全链七环：
1. 首页 → 长篇小说 → `#/book/new` 新建书会话 ✓
2. 填入建书指令 → 发送（**发送按钮是 textarea 后第一个无名按钮；Enter 不发送**）✓
3. mock 第 1 轮回 propose_action 工具调用 → 确认卡出现 ✓
4. 点「继续执行」→ 生产链启动：生成基础设定 ✓ → 保存书籍配置 ✓ → architect 首 LLM
   调用被 mock 400 拒绝 → 任务卡「建书 0s 失败」展开显示
   `Starting architect for book "镜花水月"...` ✓
5. **确认卡降级态渲染正确**（截图取证）：propose_action 徽标保持绿色「已完成」，
   卡内降级行 = 警示三角 svg 图标 + 红字「已执行 · 生产链失败（错误详情见任务卡）」，
   计算样式 `text-red-600`（oklch(0.577 0.245 27.325)）——与 438 号实现逐项吻合 ✓
6. 绿色「已执行」成功路径不回归：436 号已真机验证 + 438 号 4 条新单测锁配对逻辑，
   本次降级分支为纯新增 else 分支，不复验。
7. 收尾：SIGTERM 停环境 → mock/引擎零残留进程，临时 root 已清 ✓

## 结论

- **438 号降级态真机验证通过，未发现产品缺陷**——文案、颜色、图标、确认卡锁定语义
  （已确认不再出按钮）全部符合设计。
- 走查脚手架自此具备双向能力：默认全成功链 + `WALKTHROUGH_MOCK_FAIL_ARCHITECT=1`
  失败链，UI 失败态从此可真机复现。

## 走查操作备忘（IAB 伪象与控件定位）

- IAB 后台页动画冻结照旧（436 号教训复现）：所有点击走 DOM `.click()`。
- 输入框 `fill` + `press("Enter")` 不触发发送——必须点发送按钮（textarea 之后第一个
  无文本内容的 button）。
- 「添加 Skill」按钮与发送按钮相邻，按父链找按钮会误点开 Skill 面板（本次误开，
  不遮挡消息区，无碍走查）。
- `scrollIntoView` 定位文案要取**最内层**包含元素：首个 textContent 匹配常是祖先容器
  （空 className），滚不动也读不到目标样式。

## 门禁

- `node --check scripts/walkthrough-mock.mjs` ✓
- mock 旗标 curl 预验：architect 标记请求 400、planner 请求 200 ✓
- 纯脚本改动，不触 engine-rs/packages 源码；bench/clippy/TS 测试面零影响。

## 待办移交

- 用户照常经 Fork 图形端推送（433–439 共 7 笔本地待推）。
