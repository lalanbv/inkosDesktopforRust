# 940 号 · includeSessions 参数真化轮：sessions/ 打包双端落地——备份参数语义患清偿

> 939 备份域续察的收尾患：`includeSessions=1` **参数骗人**——双端实现
> 均「参数仅写进 manifest scope、打包范围静默忽略」（Rust 144 行
> `let _ = include_sessions;` 注释自证「与 TS 一致不含 sessions/」）。
> sessions/ 为会话 JSONL 用户创作上下文（543 号九事件），参数存在即
> 用户意图表达——静默忽略=API 契约患。本轮双端参数真化：=1 时真正打
> 包 sessions/；默认仍排除。
> 验证：TS backup-routes 6/6（+2：includeSessions=1 sessions 条目在
> 包+默认排除语义保持）；Rust backup_routes 7/7 回归绿（fixture 无
> sessions 种子，行为变化不影响既有断言）；testgate 2054/584+gate:ts
> 十步全绿+U+FFFD 净。

## 一、修复（双端同构）

- Rust run_export：BACKUP_ROOT_ENTRIES 循环后加 `if include_sessions`
  条件包含（root/sessions 存在性判断+copy_recursive 至 staging/sessions）。
- TS backup/export：同位置 `if (includeSessions && access(sessions)
  形态)` cp recursive（access 为同文件既有 import，existsSync 未导入）。

## 二、测试（TS +2 用例）

1. includeSessions=1：sessions/sess-1/session.jsonl 种子→export?
   includeSessions=1→`inkos-backup/sessions/sess-1/session.jsonl` 条目
   在包+manifest scope `{includeSecrets:false, includeSessions:true}`。
2. 默认排除语义保持：未设参数→sessions 条目不入包。

## 三、验证汇总

- TS backup-routes 6/6；Rust backup_routes 7/7 回归（fixture 无
  sessions 种子=行为变化不影响既有断言）；testgate engine-rs
  **2054**/src-tauri 584；typecheck 净；gate:ts 十步全绿；U+FFFD 净。

## 四、后续出口

- 备份参数语义患清偿（includeSessions 真化——参数与行为一致）。
- 出口清空回归常备。

**下一号自 941 起；出口清空回归常备；用户侧待办=Fork 推送 65 个提交。**
