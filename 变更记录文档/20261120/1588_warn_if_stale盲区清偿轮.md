# 1588 warn_if_stale dev 分支盲区清偿轮（2026-11-20）

## 备案清偿：dev 分支陈旧告警缺口（1587 号发现）

1587 实证 dev release bin 停数日（wasmtime 36 旧代码）被零信号消费——`warn_if_stale`（1480 号机制化）此前只盖 resource_dir 分支，dev 分支无告警=盲区。本轮清偿：dev 分支 candidate 命中处同款 `warn_if_stale` 调用（1 处扩盖）。

语义面：debug bin 常新于 src（每次 cargo test 重编）天然不告警；**release bin 迭代中常旧=本告警主目标**（testgate/集成测/直跑形态消费的正是 dev release bin）。app_data 分支维持不比对（updater 自管新鲜度，1480 注释口径）。

## 红路活体三拍（行为面实证）

构造口径勘误先行：`engine_bin_is_stale` 判定=engine-rs/src **第一层条目 mtime**（浅层目录汇总，1480 可测化口径）——touch 文件不动父目录 mtime，首版正例（touch language.rs）零告警=构造方式错（顺带实证判定对文件级 touch 不敏感=免抖动设计）；正确构造=touch 子目录本身。

1. 负例（bin 新于 src）：0 告警；
2. 正例（touch src/utils 目录→bin 陈旧）：**告警出现 ×2**（两测试各调一次 resolve_server_bin，--nocapture 可见——eprintln 走测试捕获流，不带 --nocapture 不可见=首跑零输出第二原因）；
3. 复原（touch bin）：0 告警。

## 验证

- rustbin 既有单测 9/9 绿（engine_bin_is_stale 纯函数锁定面零变化）；rust_backend_launch 2/2。
- testgate 双 crate **2731 passed**（2131+600）；clippy/fmt 双 0；gate:ts 十腿全绿；U+FFFD 净。

## 后续

- 下号自 1589 起；用户侧待办不变（Fork 推送 955-1588 + 正式打包三脚本重烤 + e2e_startup 插件面复核）。
