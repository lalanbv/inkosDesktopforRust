# 615 号：DESKTOP_WALKTHROUGH.md doc↔code 护栏（604 入库文档随批护栏，568 先例）

- 日期：2026-10-02
- 类型：test(src-tauri)+docs（护栏新目标+文档增补）
- 路线：585 入库文档随批护栏（plugin_doc_contract 先例延续）
- 状态：✅ 门禁全绿收口

## 交付

- **`desktop_walkthrough_doc_contract.rs`**（新护栏目标，src-tauri 24→25 目标）：DESKTOP_WALKTHROUGH.md 引用的 31 个代码锚点逐一核验（命令名/实现符号/能力枚举/streamPreference），命令族锚点双源核验（doc ⊇ 名单 ∧ main.rs ⊇ 名单）；
- **文档增补**（护栏驱动，三条均为真实遗漏）：3.0 权限模型行（五能力枚举+fail-closed）、3.7 事件广播行（cmd_broadcast_event）、3.8 插件注册表行（五命令族）、4.4 应用壳更新行（cmd_apply_shell_update，桌壳通道独立）、5.0 命令族明细行、3·五 节聊天流式偏好说明（607 层 3 兜底尊重配置）。

## 门禁

clippy 双 0；testgate src-tauri 25 目标 584（+1 护栏）。

## 下一号自 616 起
