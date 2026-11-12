# 993 号：文档对齐轮——tests/README 与三语主 README 对齐 988-992 后真实形态（旧 CI 段移除+keyring 误诊勘误+sweep 一键收编）

日期：2026-11-12。开工三查净（HEAD=2a54e918、工作区空、无并行会话）。readme-docs-alignment 口径（docs 与 README 已跟踪入库须照常提交）。

## ① 触发面：挂起清偿三部曲后的文档滞后

988-992 三轮大改挂起测试面（探针自同步/unique_service/超时哨兵/sweep 全量收编），而 `src-tauri/tests/README.md` 与三语主 README 仍描述改造前形态——测试文档与被测物漂移，按双栈收敛同款纪律对齐。

## ② src-tauri/tests/README.md 五处对齐

1. **CI 段（原 82-113 行）整段移除替换**：原「GitHub Actions Desktop E2E Tests workflow + artifacts 下载」描述在本仓 2026-08-23 已全量移除 workflows（no-github-actions-workflows 纪律）后为纯虚构指引——替换为「失败时调试」（本地复现 + sweep 哨兵红报诊断路径）。
2. **「已知问题」段勘误**：旧记载「keyring 3.6.3 set_password 只写内存不落盘，跨实例读不到」与 988/990 实测矛盾（lib 四枚 roundtrip 跨实例读回 + 并发 20 线程全绿、e2e_secrets 9/9 ×2 活体）——改写为历史误诊备案：真实机制 = 固定 service 名跨构建 ACL 授权框被误读为「没写进去」，990 unique_service 修复后不复存在；当前唯一真实门控 = keychain 锁定（由 sweep 哨兵兜底）。
3. **跑法重构**：一键跑法（`pnpm cargo:ignored-sweep`，含地板卫兵/超时哨兵/自动组装/单跑约束）列为推荐；手动单跑勘误——release binary 仅 e2e_startup 需要（原全文 `--release` 一刀切过时）；新增「新测试须登记 sweep INVENTORY（地板卫兵强制）」指引。
4. **覆盖范围表扩全**：三文件 → 六 scope 23 枚（补 --lib/sync_watcher_io/supervisor/e2e_updater），逐 scope 场景与枚数。
5. **性能基准换实测**：旧「startup ~5s/updater ~3s/secrets ~2s/总计 ~10s」→ 988-990 实测（lib 33s/watcher 6s/updater 0.00s/supervisor 1s/startup 10s/secrets 19s/全量 60-80s）。维护清单去 CI 项（workflow_dispatch 已不存在），改 sweep 登记与哨兵纪律项。

## ③ 三语主 README 各一行对齐

`README.md`（中）/ `README.en.md` / `README.ja.md` 的 Testing 段：裸跑 `cargo test -- --ignored`（会把需 release 二进制的 e2e_startup 拉出假红）→ `pnpm cargo:ignored-sweep`（注明 23 枚与卫兵语义）。

## ④ 备案

1. Windows taskkill 与 Linux Secret Service 常见问题两段保留（与 workflows 无关仍有效）；Linux apt 依赖段精简保留（构建依赖，非测试运行依赖）。
2. `docs/QUICK_START.md` 的 `desktop-build-inkos.sh` 引用为现行发版脚本（非过时），不动。
3. keyring 3.6.3 维持现状（误诊已勘误、实测全绿、无升级触发条件）。

本轮零代码改动（四文档 + 本文档）。门禁引用 992（同树同态，脚本仅文档改动不影响）；tests/README 描述与 sweep 实际行为逐条对照过（清单/预算/哨兵/组装行为均 992 终态）。U+FFFD 净。提交路径限定五文件。

下一号自 994 起；出口回归常备；用户侧待办 = Fork 推送（含 955-993）。
