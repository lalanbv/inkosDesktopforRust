# 1587 release 链新鲜度恢复轮（2026-11-20）

## 瞬态竞态定案（既有基建形态）

testgate 首跑 engine-rs 1/55 红：`strangler_readonly_face_duel` 60s 未就绪（工作树净、上轮 2731 全绿后零代码改动）。685 号三步法定性：①引擎日志/泄漏检查（端口零泄漏）→ ②隔离复跑 0.66s 绿 → ③全量复跑 2731 全绿——**负载性启动超时瞬态竞态非回归**（685/627 号同款备案形态：duel 并行端口竞态=已知基建形态）。

## 新鲜度缺口发现与恢复（本轮主靶面）

审计 rust_backend_launch 文档时浮出：`resolve_server_bin` dev 分支 **release 优先解析**——本机 release bin 停在 11-18（38MB，wasmtime 36 时代旧代码），而 1583-1586 的引擎改动（代理修复/ai_tells 0GC/detect_shape 等）只在 debug bin——**rust_backend_launch/e2e_startup/sweep 组装产物消费的都是陈旧引擎**（956 号「活体前核验引擎二进制新鲜度」纪律缺口；warn_if_stale 只盖 resource_dir 分支，dev 分支无新鲜度告警=盲区备案）。

恢复：`cargo build --release` 重编（11-20 03:53，40MB 含 wasmtime 50+全部 1583-1586 改动）→ rust_backend_launch 复跑 2/2 绿（0.37s，消费新 bin）→ sweep 六腿 23/23 全绿（desktop-package-engine.sh 组装重烤新 bin，built_at=2026-11-19T19:54Z；e2e_startup release 冒烟消费新 bin 全绿；post-clean 设计内孤儿回收 1 枚）。

**用户侧待办部分兑现**：打包三脚本重烤的前置（engine-rs release 重编）已在本会话完成；正式打包时脚本会重走组装（幂等）。

## 顺带定性（备案不动）

- style_analyzer 段落形态已是 `Vec<&str>` 借用（API 端点路径，非每章链）——无需优化；
- rust_backend_launch 文档「前置 cargo build --release」与 release 优先解析链一致，无漂移（误判撤销）。

## 验证

- testgate 双 crate **2731 passed**（三步法第 3 步全量）；sweep 6 腿 23/23；rust_backend_launch 2/2。
- 本轮零生产代码变更（release bin 为 gitignore 产物），纯验证+新鲜度恢复。

## 后续

- 下号自 1588 起；用户侧待办更新：Fork 推送 955-1587 + 正式打包三脚本重烤（release 重编已完成，脚本幂等重走即可）+ e2e_startup 插件面复核。
