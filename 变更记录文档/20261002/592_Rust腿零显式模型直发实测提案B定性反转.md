# 592 号：Rust 腿零显式模型直发实测——589 提案 B 定性反转（静默兜底=主路径既有行为）

- 日期：2026-10-02
- 类型：audit(提案增补)（实测+提案文档增补，零代码改动）
- 路线：589 号提案 B 的关键前置输入补全
- 状态：✅ 实测收口

## 实测

同款 fixture 环境（inkos.json custom:Mock + secrets sk-mock）对 **Rust 引擎**直发零显式模型请求（POST /agent 仅 instruction/sessionId/sessionKind）：**直接成功**——mock 回复完整返回，transcript 事件族完整五件（session_created/request_started/user/assistant/request_committed）。

## 定性反转（提案 B）

Rust 端（默认后端）的 secrets 静默兜底**已是既有生产行为**（97 号四层解析第三层）。因此 589 提案 B（Node 解析链补 secrets 兜底）的定性从「需先裁决静默兜底是否符合产品意图的新行为」**反转为「把 Node 回退端对齐主路径既有能力」**——消除的是双端不一致（回退路径弱于主路径），裁决难度大降，**B 升格为推荐实施**（挂 R45 seam 载体；实施注意 Node 解析链 secrets 消费点与 pi-ai provider 映射）。

提案文档已增补本段（592 号增补节+结论更新）。

## 下一号自 593 起
