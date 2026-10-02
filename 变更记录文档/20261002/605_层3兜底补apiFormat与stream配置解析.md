# 605 号：Rust 层 3 对 custom 服务补 apiFormat/stream 配置解析——兜底路径协议不再硬编码 Chat

- 日期：2026-10-02
- 类型：fix(engine-rs)（行为修复：层 3 兜底尊重服务配置协议/流式）
- 路线：596 号层 3 custom 感知的收尾（发现于 605 实施时对齐检查）
- 状态：✅ 门禁全绿收口

## 缺口

596 层 3 实施时返回体 `api_format` 硬编码 `TransportApiFormat::Chat`、`stream: None`——配置了 `apiFormat: "responses"`（或 stream 偏好）的 custom 服务在**零显式兜底路径**上会用错协议失败。层 1（显式请求）已有 `resolve_configured_service_api_format/stream` 解析（106 号），层 3 漏配。

## 修复

层 3 返回前经 `resolve_configured_service_api_format`（缺省回退 Chat）与 `resolve_configured_service_stream` 解析——与层 1 同一解析器，无新逻辑。

## 验证

- 新单测 `layer3_respects_configured_api_format_and_stream`：`apiFormat: "responses"` + `stream: false` 的 custom 服务 → 层 3 解析结果 `api_format == Responses` 且 `stream == Some(false)`（本地 stub /v1/models）；
- 既有 layer3 两测（chat case+:9 不可达下落）不变；clippy 双 0；testgate engine 45 目标 1897（+1）+ src-tauri 583。

## 下一号自 606 起
