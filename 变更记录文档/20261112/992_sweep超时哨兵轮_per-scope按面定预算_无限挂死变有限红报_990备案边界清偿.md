# 992 号：sweep 超时哨兵轮——per-scope 按面定预算 + 无限挂死变有限红报（990 备案①边界清偿）

日期：2026-11-12。开工三查净（HEAD=e3afe1fb、工作区空、无并行会话）。

## ① 立项：990 备案①已知边界清偿

990 轮备案：keychain 锁定（dark wake）时 e2e_secrets 的 securityd 调用阻塞无上限，sweep 无内层哨兵——无人值守场景（CI 半夜/桌面自动休眠边界）一次锁屏即让整个 sweep 挂死，且无诊断输出。898「无界等待」教训同族（signal-less 等待永久挂死）。本轮给 `cargo-ignored-sweep.mjs` 六 scope 全部加上 per-scope 超时哨兵。

## ② 改造：预算按面定（982 口径）

| scope | 预算 | 依据 |
|---|---|---|
| lib | 300s | 网络两枚 + keychain 四枚 + 冷编译余量 |
| sync_watcher_io | 180s | 三枚 IO 测 ~6s + 冷编译 |
| e2e_updater | 120s | 纯逻辑 0.00s 级（990 勘误），预算=编译余量 |
| supervisor_integration | 240s | sidecar 60s 探测上限 + 端口重试 5s + 组装 |
| e2e_startup（release） | 1200s | 最宽：release 缓存陈旧时数分钟级增量编译（989 备案②） |
| e2e_secrets | 240s | **哨兵主目标**：9 枚 ~19s，keychain 锁定阻塞无上限 |

实现：`spawnSync(..., { timeout, killSignal: "SIGKILL" })` + 超时分支先行（`run.signal === "SIGKILL" || run.error?.code === "ETIMEDOUT"`）——红报含预算值、高概率根因（keychain 锁定/securityd 异常/编译僵死）、解锁重跑指引、孤儿清理特征 pkill 命令。已知边界备案：spawnSync kill 只达 cargo 直接子进程，测试二进制可能成孤儿（903 族）——指引命令即清理路径，且 supervisor 端口预检与 e2e_startup 独占启动会暴露残留。

实现过程一处自纠：超时分支初稿引用了其后才声明的 `output`（TDZ ReferenceError）——声明顺序修正先于活体验证。

## ③ 红绿闭环

- 改造后全量活体：**6/6 绿 exit 0**（哨兵正常路径零误伤——正常完成时 `run.signal` 为 null，解析路径不变）。
- NEUTER992 必红活体：secrets 预算临时 1ms → 全量跑恰红 `✗ [--test e2e_secrets] 超时哨兵触发（预算 1ms 被 SIGKILL）`，报文四要素（预算/根因/指引/清理）完整，exit 1 → 精确互逆恢复（1ms→240_000）→ 复绿终跑 6/6 exit 0 + 预算六值净度核（无 NEUTER 残留）。

## ④ 门禁与提交

本轮仅改 `scripts/cargo-ignored-sweep.mjs`（Node 脚本，无 Rust/TS 源码改动）：clippy 双 0 / testgate 584 / audit 双 0 引用 990（同树同态）；脚本 `node --check` 过；全量活体 23 枚六 scope 绿为本轮核心证据；U+FFFD 净。提交路径限定两文件：本脚本 + 本文档。

## 备案

1. 预算值是「按面定」的经验上限，非 SLA——机器显著变慢（CI 老化 runner）时按红报调整，禁为绕红放宽洗哨兵。
2. 超时孤儿风险（spawnSync 进程组限制）维持备案：指引 pkill 特征清理已足够，进程组级 kill（detached+kill(-pid)）待真实误伤案例再评估。

下一号自 993 起；出口回归常备；用户侧待办 = Fork 推送（含 955-992）。
