# 606 号：层 3 stream 偏好双端分歧精确归因与备案（提案文档 606 增补节，零代码）

- 日期：2026-10-02
- 类型：docs(提案增补)（零代码）
- 路线：605 的层 3 apiFormat/stream 解析实施后的双端复检
- 状态：✅ 收口（分歧精确归因，备案不立项）

## 分歧精确归因

605 落地后双端复检发现层 3 偏好的两半分歧：

- **apiFormat**：双端一致支持（Rust 605 补齐 `resolve_configured_service_api_format`；Node 第 3 层经 `resolveServiceModel` 的 `customApiFormat` 参数支持）；
- **stream 偏好**：**Rust 层 3 尊重服务配置**（`resolve_configured_service_stream` → 非流式直发），**Node 第 3 层恒流式**（`ResolvedModel` 无 stream 字段；`/agent` 的 `streamFn` 架构恒走 `guardedAgentStream` 流式）——配置 `stream: false` 的 custom 服务用户：Rust 引擎下非流式（按偏好），Node 回退端下流式（忽略偏好）。

**归因**：Node `streamFn` 架构恒流式，层 3 传 stream 偏好需要 agent-session 增加非流式路径（架构重构，非小改）。

## 备案裁决

Rust 保留尊重配置（正确语义）；Node 层 3 stream 支持缺口**备案**——实际影响为展示层差异（流式 vs 等待完整回复），功能双端均可用；若后续 Node `streamFn` 架构支持非流式，随批补齐。提案文档 606 增补节已同步。

## 下一号自 607 起
