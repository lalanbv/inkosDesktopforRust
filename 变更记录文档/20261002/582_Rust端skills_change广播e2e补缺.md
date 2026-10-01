# 582 号：Rust 端 skills:change 广播 e2e 补缺——import/delete 双步 payload 断言（566 发射点测试闭环）

- 日期：2026-10-02
- 类型：test(engine-rs)
- 路线：566 号发射点 Rust 侧测试补缺（此前仅 574 Node 真机+实现审查）
- 状态：✅ 门禁全绿收口

## 交付

`skills60_e2e::import_and_delete_broadcast_skills_change`：hub.subscribe() 先订阅再驱动，POST /skills/import → receiver 收 `skills:change` + `reason="imported"`；DELETE → `reason="deleted"`。payload 形态锁定（与 TS server 同构，574 真机已验 Node 腿）。

## 三次红→绿的工程记录（诚实归档）

1. **首红 Empty**：receiver 绑定在测试手建的 rt60 hub，而 `app60(&root)` 每次调用都**重建 runtime/hub**——订阅与广播落在不同总线。教训：e2e 里「每 call 重建 app」的惯用法与 hub 订阅类断言互斥，须同 runtime 贯穿。
2. **复用 app 仍红**：app60 内部 rt60 与测试手建 runtime 是两个实例——修正为**内联路由构建**（import/delete 两路由 + with_state(同一 runtime)），订阅落在注入 app 的同一 hub。
3. **借用/所有权三连**：Arc 包装多余（move 进 with_state 即可，subscribe 先于 move 且 `hub.clone()` 克隆 Arc 保接收器存活）；scope 内 sed 全局替换误伤 books47 三处 `.with_state(runtime.clone())`（runtime move 后续用）——按行区间精准恢复；`&manifest` needless_borrow 被 clippy -D 拦截改裸值。

## 门禁

clippy:gate 双 crate 0 告警；cargo:testgate engine 45 目标 **1894 passed**（+1 广播测试）+ src-tauri 24 目标 583；books47 5/5 复验恢复无恙。TS 零改动。

## 下一号自 583 起
