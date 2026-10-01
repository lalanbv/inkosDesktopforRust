# 583 号：TS 侧 skills:change 广播断言（582 对称）——真实 SSE 流 import/delete 双向帧断言

- 日期：2026-10-02
- 类型：test(studio)
- 路线：566 号发射点 TS 侧测试补缺（与 582 Rust 端对称；此前仅 574 Node 真机）
- 状态：✅ 门禁全绿收口

## 交付

`skills-endpoint.test.ts` 新增广播断言测试：经 `app.request("/api/v1/events")` 打开**真实 SSE 流**（连接即 ping=流开启信号），驱动 import/delete 端点，逐帧解析断言 `skills:change` 事件帧各含 `"reason":"imported"`/`"reason":"deleted"`。带 3s 超时护栏（未广播即红，防挂死）；帧解析器处理 chunk 合并（buffer 按 `\n\n` 切帧循环读流）。

## 实现注记

- server 的 subscribers Set 为模块私有——测试**不加导出钩子**（572 出口卫生纪律），走真实 SSE 流断言（更强的端到端性：含 streamSSE 转发层）。
- Hono streamSSE 响应 body 为 ReadableStream，vitest node 环境直接 getReader 可读。

## 门禁

gate:ts 七步全绿（skills-endpoint 8/8 含新广播测试）。Rust 零改动。

## 下一号自 584 起
