# 1454 号：1453 改动树 testgate 全量首谱与 RAII 收割全量负载首验轮

## 目标与范围

/goal 自主持续开发续轮。1453 备案待办「下次全量轮走 1433 清单重验 Rust 面」
兑现：1453 改动了 engine-rs 代码树（tests/strangler_duel.rs 测试文件 +
Cargo.toml dev-dep libc + Cargo.lock），1452 谱（engine-rs 2115 /
src-tauri 593）是改动**前**代码树的证据；且 1453 的 RAII 进程组收割修复
只在 duel 单文件 10 用例验证过孤儿=0，**全量 testgate 负载下（52 目标
并行、duel 真跑）的孤儿零泄漏从未验证**——这正是当初每轮泄 4 双层孤儿
的场景重放。本轮=纯验证轮，零代码改动。

前置收尾：1453 遗留未提交的密封扫描回执（钩子 ENOBUFS 补救），开工时
发现并行会话已提交为 `e55c508c`（docs(1453补录)，内容一致零冲突），
幂等跳过。

## 结果

①**testgate 全量首谱一次全绿 exit 0**（e55c508c 树 = 1453 改动树）：

- engine-rs：52 个目标全绿，累计 **2115 passed**
- src-tauri：25 个目标全绿，累计 **593 passed**

与 1452 谱（1444 谱系）**精确同数零漂移**——1453「产码零触碰、改动面
= 测试文件 + dev-dep」的预期兑现：测试树改动（TsSidecar 重构 + libc
dev-dep）在全目标编译与运行面零回归。1444 备案的 pending_abort flake
（agent_route cap 逐出竞态）本轮全量首跑未复现（1444/1452 之后第三次
未复现，稀疏性旁证累积）。

②**RAII 收割全量负载首验：孤儿终检 0**。duel 10 用例随全量 testgate
真跑（脚本 ：87 固定注入 INKOS_DUEL=1），跑前基线进程表零孤儿 →
跑后终检进程表**零 tsx/inkos 残留**。对照 1452 轮同一全量场景每轮泄
4 个双层孤儿（tsx 包装 PPID=1 → node server 高位口），修复验收从
1453 的单文件级（10/10 后孤儿 0）升格为**全量负载级**：52 目标并行
调度下 10 个 sidecar 全部被 Drop 的 killpg 整树收割。

③**audit:rust 腿顺带新鲜**（testgate 前置哨兵）：双 crate 0 漏洞 +
必红双夹具活（7/1 RUSTSEC 检出=检测器活着）；yanked 联网核验超时
336 次=网络抖动非漏洞（advisory-db 1 天前新鲜）。

④**泄漏普查第七证**：testgate cargo 测试窗后 TMPDIR inkos-**恒 5**
设计内（产品运行时 + 固定名 mock log×3 + m1-demo）——1440/1442 修复
第七次独立实证（1450 sweep 第五证 / 1452 testgate 第六证 / 本轮
testgate+duel 双面第七证），且首次覆盖「duel 真跑 + RAII 收割」组合
窗口。

## 清单对照（1433 清单制）

testgate 全量 ✓（本轮 e55c508c 树首谱）/ gate:ts 十腿 ✓（1450 首谱
971c9a54——1453/1454 零 TS 面改动，代码树同一引证在位）/ audit 族 ✓
（audit:rust 本轮新鲜 + semantic∩改动面=0 + bindings 186 + manifests
三包 = 1447 同树引证）/ clippy ✓（1453 --all-targets 零告警，1454 零
代码改动沿用）/ bench = 1448 定性制会话内不重试 / ignored-sweep ✓
（1450 当前树首跑 12 枚全绿，1453 后挂起路径零触碰）/ 泄漏普查 ✓
（第七证本轮）/ 巡检周期未到（1452 当日全清，快照/watchdog/依赖备案
延续）。

零新问题零修复需求零代码改动，U+FFFD 净。

出口=下一巡检周期/earendil-mz 单包合并；用户侧待办=Fork 推送(含
955-1454)+正式打包三脚本重烤+bench 静默补跑（可选）+TS 腿 no-store
统一层（备案项可选）+依赖 semver 兼容刷新窗口（7+25 包备案项）。
下一号自 1455 起。
