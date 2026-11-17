# 1495 号：README 门禁清单补全轮——fmt:gate 可见性收尾

## 目标与范围

/goal 自主持续开发续轮。未覆盖角度扫描发现文档收尾缺口：**1487
接入的 fmt:gate 门禁未进 README 门禁命令清单**（「门禁脚本
（提交/发版前全量）」段落 193-201 原六条）——门禁可见性缺口，
新开发者/未来会话从 README 无法发现该卫兵。

## 实施

- README.md 门禁清单补 `pnpm fmt:gate` 行（clippy:gate 后语义
  相邻位——同为 Rust 静态检查面）：「双 crate rustfmt 格式门禁
  （必红夹具自检活体）」；
- en/ja README 无门禁命令段（grep 实证零命中），不涉及三语同步；
- **文档准确性活体验证**：README 所写命令 `pnpm fmt:gate` 实跑
  绿（双 crate 0 格式漂移）——文档命令可跑性核过。

零代码改动零新缺陷，U+FFFD 净。

出口=下一巡检周期（约 30 天后，2026-12 中旬前后到期）/earendil-mz
单包合并；用户侧待办=**Fork 推送（含 955-1495，Fork 图形端操作，
按 docs/RELEASE_RUNBOOK.md 待办一）+正式打包三脚本重烤（按
RUNBOOK 待办二，必须含 engine-rs release 重编）**。下一号自
1496 起。
