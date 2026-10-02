# 596 号：Rust 层 3 补 custom 服务 baseUrl 感知模型发现——589/595 提案 B 落地（双端零显式直发对齐）

- 日期：2026-10-02
- 类型：fix(engine-rs)（解析行为变更：custom 服务零显式直发从失败变成功）
- 路线：589 提案 B（595 方向精确化）实施；挂 R45 提供方 seam 载体
- 状态：✅ 门禁全绿+真机复验通过

## 实施（两处，最小对齐 Node 第 3 层）

1. **`llm/probe.rs` `list_models_for_service`**：custom:* 服务（无 bank 卡/预设）在调用方传入 `live_base_url` 时不再直接空返回——仍做 live `/models` 探测（仅 live 层生效；bank/legacy fallback 对 custom 自然为空）。
2. **`agent_production.rs` 层 3 循环**：`resolve_configured_service_base_url` 提前到 list 之前并传入（每服务一次；命中后复用同一 base_url 返回）——此前 list 传 None 使 custom 服务（不在 bank）模型发现为空整体下落（595 边界实锤）。

## 验证

- **新单测** `layer3_custom_probe_tests::layer3_discovers_models_for_custom_service_via_configured_base_url`（本地 stub /v1/models）：inkos.json llm.services 带 baseUrl+secrets 有 key → 零显式解析命中 custom:probe/base/api_key/首文本模型。
- 既有层 3 测试（custom baseUrl=:9 不可达）行为不变：probe 失败=空=下落，与旧空返回等价（17→18 passed）。
- **真机复验**（595 探针环境重跑：去 INKOS_LLM_BASE_URL+标准 llm.services 布局+secrets）：零显式直发**成功**——mock 回复完整+transcript 五事件族（对比 595 修复前打到缺省 :9 失败）。双端零显式直发对齐达成（Node 593 实测=Rust 596 实测）。

## 决策状态

「静默兜底」至此成为**双端既有一致行为**（Rust 层 3 custom 感知+Node 第 3 层），589 提案的 B 分支闭合；590 错误注解保留（无任何可用服务场景仍有价值）。

## 门禁

clippy:gate 双 crate 0 告警；cargo:testgate engine 45 目标 **1896**（+1 层 3 custom probe）+ src-tauri 24 目标 583。TS 零改动。

## 教训

lib 内联测试模块的导入用 `crate::`（`inkos_engine::` 仅 tests/ 独立目标可用）；`spawn_models_stub` 参数收 `String` 避免 `&str` 跨 await 逃逸（E0521）。

## 下一号自 597 起
