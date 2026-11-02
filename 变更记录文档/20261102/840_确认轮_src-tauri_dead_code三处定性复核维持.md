# 840 号：确认轮——src-tauri dead_code 三处定性复核维持

日期：2026-11-02。前置：823 号（src-tauri 零消费实锤）。本轮复核 src-tauri 仅有的三处 `#[allow(dead_code)]` 是否仍需保留。

复核结果：三处均有明确文档化理由——①migration.rs `last_selected` 字段 serde 兼容保留 ②windows.rs `win_firewall_add_args` M1/M2 不再调用但 M3 WFP 实现复用 ③windows.rs 同族——全部维持。src-tauri 零消费结论（823 实证）复核维持。

上游=0 维持。serde_json=1.0.151 钉定维持（crates.io 最新仍 1.0.154 回退线）。全指标稳定。
