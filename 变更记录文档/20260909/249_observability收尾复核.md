# 249 号补：observability/logging 与发版工具轻量复核——零缺口

- 日期：2026-09-09
- 分支：develop
- 关联：222 号（Tauri 壳审计——本批收尾其 observability/bin 角落）、166 号（updater 签名链）
- 推送核验：origin/develop 仍停 6bc65564——**201–248 共 48 提交待推送**；本批随 249 后本地领先 50。两项默认值无新答复。

## 一、observability/logging.rs（120 行）

| 审计点 | 结论 |
|---|---|
| 滚动与保留 | ✓ Rotation::DAILY + max_log_files(7)——按天滚动且 7 天上限，不无限堆积 |
| 异步写入 | ✓ tracing_appender non_blocking + WorkerGuard（持有到进程退出保证 flush；drop 阻塞写完尾部） |
| 双层输出 | ✓ 文件 JSON 层 + stdout 人类可读层 |
| 环境过滤 | ✓ EnvFilter：RUST_LOG 可覆盖，默认 info |
| 全局/测试分离 | ✓ build_subscriber 与 init_logging 拆分——进程内单次注册约束不 panic，测试走线程局部 subscriber 可并行 |

## 二、发版工具 bin（sign-bundle 114 行 / publish-plugin 201 行）

| 审计点 | 结论 |
|---|---|
| 签名链闭环 | ✓ sign-bundle（Ed25519 私钥签 bundle → .sig hex）↔ updater sig::verify（241→228 号已审的 Reject fail-closed 接入）两端对称 |
| 公钥回显 | ✓ 签名后输出 pubkey hex（部署侧配置 INKOS_ENGINE_PUBKEY 的一致性保障） |
| 错误面 | ✓ 用法提示 + ExitCode 非 panic 退出 |

## 三、验证

纯复核批次零代码改动；全部门禁于 249 号已绿。
