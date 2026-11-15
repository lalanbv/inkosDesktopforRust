# 1435 号：巡检周期全量重验 + sweep 孤儿自愈修复轮

## 目标与范围

/goal 全量测试第七轮。出口条件「下一巡检周期」到点触发；earendil-mz 触发未点火
（见巡检面）。增量面=进程卫生巡检抓到真发现（孤儿池两族）+ sweep 脚本孤儿自愈
修复（本轮唯一代码改动：scripts/cargo-ignored-sweep.mjs）+ gate:ts 当前树首谱 +
上游/三面烤入快照巡检。

## 进程卫生巡检（真发现两族）

ps 全量快照定案（pgrep 旗标后置+命令自匹配伪影教训：一次干跑曾虚报 54 命中，
ps 权威复核全数证伪——归因前逐流落盘证伪纪律再+1）：

1. **TS sidecar 孤儿 18 枚**：启动时间全为 11-16 01:21:08 = 1431 轮 pre-fix
   挂死 sweep 的 e2e_startup SIGKILL 时刻（Fix C 落地前），tsx cli+loader 成
   对、PPID=1，特征 `.tmpXXXX` 短哈希 tmp 根（Rust tempfile TempDir 形态），
   存活 12h。1431 清场 pkill 特征（inkos-smoke-*/tsx watch 等）盖不住该形态。
2. **tsx watch 残留 1 枚**（PID 2737）：1429 轮 dev:server 代——当时 pkill 模式
   `tsx watch src/api/index.ts` 被 `--clear-screen=false` 旗标隔断未命中（漏杀
   备案的第三形态）。

处置：显式 PID SIGKILL 全清，验证 0 残留 0 监听。

## 修复面（scripts/cargo-ignored-sweep.mjs）

903 纪律「spawn 前清场=确定面」补齐三族，全部活体验证：

1. **TS sidecar 族 spawn 前清场**：pkill 特征=本仓绝对路径 + tmp 根参数
   （/var/folders|/tmp 下 .tmp* 目录）——dev:server（相对路径无参）与用户真项
   目根运行不匹配。**三诱饵双向自检**：必中形态被清 / dev 形态豁免 / 真项目根
   形态豁免（匹配器非恒绿实证）；干净态无匹配走非致命路径活体过。
2. **引擎孤儿族自愈（头尾双清）**：双轮复现实证=e2e_startup 按设计「超时击杀
   即预期」（assert_cmd SIGKILL app，cleanup 不可达）→ release 引擎每轮必孤儿
   化占默认 4567，上一轮残留毒下一轮 supervisor_integration（要求 :4567 空闲）
   假红=**自毒化循环**。修复=ps 扫描 PPID=1 + 本仓引擎路径特征（用户 shell/桌
   面应用拉起的引擎 PPID≠1 不误伤；无差别 pkill 引擎/app 仍不做）。活体=第三
   跑 pre-clean 精确回收 39779/39780 + post-clean 自收本轮 40682/40683，终态
   零孤儿。
3. **sweep tmp 日志目录泄漏**：988 号创建以来每轮残留一个（实测 149 个/588K）
   ——绿档清盘（成功路径+哨兵超时路径删目录，红档路径保留日志供事后取证）。
   活体=149→0 且两次全量复跑后仍 0。
4. 哨兵指引扩展（TS sidecar 特征入红报）+ 引用改名同步。

不引入新问题论证：单文件脚本改动；node --check + 三次全量真流实跑；特征双向
自检；PPID 过滤不误伤用户进程面；红档取证面无损。1431 备案的 parent-watchdog
env 形态留工项不变（自愈闭环后优先级更低）。

## 全量重验

- **gate:ts 十腿全绿**（当前树首谱）：build 21.3s / typecheck 21.9s / test
  113.4s / audit:npm 3.3s / rust-bin 0.3s / node-fallback-smoke 35.9s /
  engine-contract-diff 36.6s / export-epub-smoke 35.1s / work-alternation 5.2s
  / check-routes 0.1s——1433 谱噪声带一致零回归。
- **cargo:ignored-sweep 三跑 23/23 全绿**清单精确匹配（7+3+1+1+2+9），第三跑=
  自愈闭环终审活体（pre-clean 回收在先，supervisor_integration 不再被毒化）。
- **semantic-patterns candidates=41** 恰等 1425/1430/1433 稳态——脚本改动零新
  候选（exit 1=有候选的工具本义非门禁红）。
- audit:rust / bench:gate 引证 1434（同代码树当日实证）；clippy 引证 1431（本
  轮零 Rust 改动）。

## 巡检面

- **上游零落后**：`HEAD..upstream/master` = 0（tip=merge-base=53adac4e，上游自
  上次同步零推进）；`ls-remote` 上游分支无 earendil\*——earendil-mz 单包合并
  触发条件未点火。
- **三面烤入快照健康**：缺失方向 0（fresh dist 独有文件=0）/ 同名内容漂移 0 /
  面 B≡面 C 全等 / 三面 index.html 同 md5=活体 SPA 链完好。56 个陈旧 hashed
  chunk + {api,lib,shared} = 备案死重（rsync 无 --delete 累积，Rust 不执行，
  下次打包 rm -rf 自愈，489 号惯例），不清理（防误删面）。
- src-tauri/engine 组装新鲜（sweep 内 desktop-package-engine.sh 当轮重装）。

## 结论

一处真缺陷闭环修复（孤儿自愈两族 + tmp 泄漏，sweep 自毒化循环破除）+ 全谱重
验零回归。清场终态：孤儿 0 / tmp 0 / 4567 空 / 引擎进程 0。

出口=下一巡检周期/earendil-mz 单包合并（触发条件制已更新）；用户侧待办=Fork
推送(含 955-1435)+正式打包三脚本重烤。
