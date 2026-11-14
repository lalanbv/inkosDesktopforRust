# 1088 号 · walkthrough-env rust 路径活体与 fast 复验轮

> 1087 出口执行（1087 scripts 改动后复验组合：rust 默认路径活体+fast 例行）。
> 零代码改动轮。

## 一、walkthrough-env rust 默认路径活体（1087 改造后功能确认）

- mock 就绪（18812/v1/models）→ **引擎就绪**（inkos-engine-server listening 18811）
  → fixture 播种完成（work 播种/run-log 11 条/承诺账本 4 条/固定 id 会话就绪）
  → **走查环境就绪** ✓
- SIGTERM 主进程 → shutdown 触发 → **post-term pgrep 零残留**——SIGKILL 进程组
  兜底对 engineBin 子进程同样生效（与 1087 node 泄漏路径活体互为补充，两路径
  清理行为全实证）
- 首次活体崩（"Node.js v26.13.1" 栈尾）且证据被误删无法归因——**同参第二次
  全绿**定性环境瞬态；「先留证后清理」纪律再实证（1061 tail 截断/400 号 head
  截断同族）

## 二、fast 四腿复验

build 19.1s / typecheck 19.0s / test 108.9s（三包全量）/ audit:npm 4.0s（5 advisory
全白名单拦截 0）——**全绿**。

## 三、触发条件周期复查

上游 0 behind；earendil 事件翼近期轮内持续未命中——触发条件制持续。

## 四、下一号自 1089 起

出口=下一巡检周期 / earendil-mz 单包合并（触发条件制已更新）/ 用户侧待办=Fork 推送（含 955-1088）。
