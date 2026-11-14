# 1087 号 · walkthrough-env 孤儿泄漏治本轮（1086 缺口清偿）

> 1086 出口执行。1086 周期清场后的治本方向：泄漏源头定位与修复。

## 一、泄漏源定位

1086 清场的 3 孤儿命令行（`tsx/dist/cli.mjs src/api/index.ts TMPDIR/.tmp*`）与
四脚本 spawn 形态吻合排查：

| 脚本 | kill 形态 | 定性 |
|---|---|---|
| export-epub-smoke / node-fallback-smoke / engine-contract-diff | 进程组 SIGKILL 兜底+exit 钩子+spawn 前清场（903 判例形态） | 已治本 |
| **walkthrough-env** | **旧 SIGTERM 形态**（无兜底无清场） | **泄漏源** |

## 二、改造（对齐 export-epub-smoke 判例形态）

- `killAllTrackedChildren()`：进程组 SIGKILL 兜底（`process.kill(-pid, "SIGKILL")`
  catch `child.kill("SIGKILL")`）
- `process.on("exit", killAllTrackedChildren)` 收尸钩子
- spawn 前清场：`pkill -f tsx/dist/cli.mjs` 特征限定（回收上轮孤儿）

## 三、活体修复实证（红绿对照）

`--engine node` 驱动 tsx 泄漏路径：环境起（tsx pid 10135 在跑）→ SIGTERM 主进程
→ shutdown 触发 → **post-term pgrep 零残留**。
红面=1086 实测 3 孤儿残留（旧 SIGTERM 形态），绿面=本活体零残留——修复闭环。
tmp 活体环境清理完成；FFFD 净。

## 四、下一号自 1088 起

出口=下一巡检周期 / earendil-mz 单包合并（触发条件制已更新）/ 用户侧待办=Fork 推送（含 955-1087）。
