# 594 号：CHANGELOG 双语 Unreleased 段——本会话用户可见面入账（594 对齐线收尾）

- 日期：2026-10-02
- 类型：docs（入库 CHANGELOG 双语，零代码）
- 路线：578/580/585 文档对齐线的最后一块（CHANGELOG 为 USER_GUIDE 引用的入库文件，停在 v1.8.0/330–425 时代）
- 状态：✅ 收口

## 补齐

CHANGELOG.md / CHANGELOG.en.md 头部新增 **Unreleased** 段（Keep-a-Changelog 惯例），记载本会话（562–593）用户可见变化六条：

- **Added**：Agent Skills 导入（AgentSkills/OpenClaw 兼容+即时热刷新+模型专用技能不进用户列表）；上下文计量徽章（双来源/超窗告警/悬停明细）；模型配置自定义服务置顶+测试连接自动发现；
- **Changed**：无模型发送错误指认已配置服务（590）；会话事件日志写前落盘（565，中断不丢输入）；零配置直发双端一致（592/593 实证）。

内部专项（审计哨兵/死代码扫描/gate 步骤序/差分器扩容等）不入用户 changelog（口径：用户可见面 only）。

## 下一号自 595 起
