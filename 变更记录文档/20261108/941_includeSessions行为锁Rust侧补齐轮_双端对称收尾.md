# 941 号（补档） · includeSessions 行为锁 Rust 侧补齐：双用例——双端测试对称收尾

> 940 参数真化后 Rust 侧仅有回归绿无行为断言（双端测试不对称）——本
> 轮（73987571）补两用例，双端 includeSessions 行为锁对称收尾。补档说
> 明：本档随 942 号提交入库（941 轮收口时文档遗漏，942 轮纪律补齐）。

## 一、两用例（engine-rs backup_routes tests）

1. `export_include_sessions_true_packs_sessions`：sessions/sess-1/
   session.jsonl 种子 → run_export(root, false, **true**) →
   `inkos-backup/sessions/sess-1/session.jsonl` 条目在包 + manifest
   scope includeSessions=true 断言（参数真化行为锁）。
2. `export_default_excludes_sessions`：同种子 → run_export(root,
   false, **false**) → sessions 条目不入包（默认排除语义保持）。

测试自坑：ArchiveEntry 字段笔误 payload→bytes（E0609）。

## 二、验证

- Rust backup_routes 9/9（+2）；cargo:testgate engine-rs 52 目标
  **2056**（+2）/src-tauri 584；clippy 双 0。

**下一号自 942 起；用户侧待办=Fork 推送 66 个提交（至 73987571）。**
