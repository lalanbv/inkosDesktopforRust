# 989 号：src-tauri 挂起 e2e 面二批清偿轮——桌面可清偿面全量收编 sweep + is_newer 假挂起勘误 + 988 计数勘误 + TS 盲区族负结果

日期：2026-11-12。988 号直系续轮（挂起用例清偿第二批）。开工查：987 并行会话已落地（1f42cefd，rcm 分序列潜伏红清偿+UI 写作链审计端口错配修复），本轮工作区独占。

## ① A 面：is_newer 假挂起勘误 + 即时清偿

`tests/e2e_updater.rs::test_is_newer_version_check` 旧 ignore 理由「需要网络访问」**已过时**——用例本体是纯版本比较逻辑（`is_newer("0.1.0", "v0.2.0")`，注释自证「不需要网络」），实跑 0.00s 绿。假挂起 = 另一形态的盲区（可用覆盖被错误封存）。已入 sweep 清单。

## ② B 面：supervisor 真实 sidecar 实跑清偿

`supervisor_integration::real_inkos_sidecar_serves_spa`：`./scripts/desktop-package-engine.sh` 组装 `src-tauri/engine`（v2.0.0，TS dist 已存在走跳过分支，组装秒级）→ 实跑绿（**0.97s**：真实 node sidecar 拉起 → :4567 SPA 60s 内可达 → kill_tree → 端口重绑成功零孤儿）。测试自带端口预检（被占即 panic 指认残留孤儿）。

## ③ C 面：e2e_startup 桌面启动冒烟清偿（release 构建）

`e2e_startup` 两枚（`test_startup_smoke` + `test_startup_logs_version`）：`cargo test --release` 增量重建 release 二进制（9/17 陈年旧物换新）→ 2/2 绿（10.02s）。孤儿防池：预检/后检进程表快照比对（`inkos-engine-server|inkos-desktop|tsx` 特征）+ :4567 复查——**前后均 0，零残留**（903 号孤儿池教训前置应用）。GUI 形态备案：应用窗口闪现各 ~5s（超时击杀即用例预期语义）。

## ④ sweep 扩面收编：14 枚五 scope

`cargo:ignored-sweep` INVENTORY 10→14 枚、3→5 scope：新增 `--test e2e_updater`（1）/ `--test supervisor_integration`（1，新增 **pre 前置钩子**自动跑 desktop-package-engine.sh——幂等组装，缺失时用例自带 panic 指引）/ `--release --test e2e_startup`（2）。头注释同步：覆盖面逐 scope 说明（含 GUI 闪现与 :4567 占用假红预警）、豁免族收缩为仅 e2e_secrets 9 枚（授权框阻塞无人值守禁跑）。**活体：14/14 全绿 exit 0**（扩面后首跑即全通，含自动打包与 release 增量编译路径）。

## ⑤ 988 号计数勘误

`grep -c "#\[ignore\]"` 把 supervisor_integration.rs 头注释与 doc 注释内两处 `` `#[ignore]` `` 字样误计为用例（3 实为 1）——真实总数 **23 枚非 25**（lib 7 + tests/ 16）。教训：`#[ignore]` 盘点须以「属性行紧邻 `fn`/`async fn`」判据逐枚核对，`grep -c` 计数会把注释/文档里的字样算进去。988 文档 ①/备案① 已就地勘误标注。此前 988 提交内的 sweep 头注释「release 二进制族 6 枚」同源误数，本轮一并修正。

## ⑥ TS 盲区族负结果记档（988 镜像面）

TS 树 `.skip/.todo/xit` 全域清点：**仅 1 处**条件跳过（`packages/core/src/__tests__/play-db.test.ts` 的 `sqliteIt = hasNodeSqlite ? it : it.skip`）——本机 `node:sqlite` 可用（Node 26）→ 门控激活常跑，非盲区；三包 vitest 配置零 `exclude` 面（无整目录排除盲区）。TS 腿无 988 同族债，负结果亦是结果（980 先例）。

## ⑦ 门禁与提交

本轮零 Rust/TS 源码改动（sweep 脚本 + 两文档）：clippy:gate 双 0 与 testgate 584 引用 988（同树同态）；engine-contract/gate:ts 引用 986/987（TS 树零改动，987 刚全量验收）。sweep 14/14 活体为本轮核心证据。U+FFFD 净。提交路径限定三文件：`scripts/cargo-ignored-sweep.mjs` / 988 文档（勘误标注）/ 本文档。

## 备案

1. e2e_secrets 9 枚维持豁免（授权框阻塞）；其用例均为进程内 keychain 断言，价值与 lib keyring 四枚高度重叠——维持豁免损失极小。
2. sweep 的 release scope 在 release 缓存陈旧时会触发数分钟级增量编译——头注释已预警，属清偿成本非缺陷。
3. supervisor scope 需 :4567 空闲——用户桌面应用运行期间 sweep 会假红（测试自带 panic 指认，非静默）。

下一号自 990 起；出口回归常备；用户侧待办 = Fork 推送（含 955-989）。
