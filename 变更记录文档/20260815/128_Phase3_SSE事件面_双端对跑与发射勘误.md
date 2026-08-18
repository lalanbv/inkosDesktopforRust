# 128 号变更记录：SSE 事件面双端对跑（duel 第 8 测试）+ llm:progress 聊天轮过度发射勘误

## 一、交付

### 1. `sse_event_face_duel`（duel 第 8 测试）

同根双进程（TS sidecar + 真实 bin）各自订阅 `/api/v1/events`（reqwest 字节流 → SSE 块解析采集器，20s 截止 + 终态事件轮询收束）并发起同构聊天回合（同 instruction、各自会话），比对**共享词汇事件的序列（按名稳定排序）与负载形态**：

- 词汇：agent:start / draft:delta / agent:complete / session:title（llm:progress 见勘误）。
- 归一：① `null` 值键递归剥除（TS `undefined`→省略 vs Rust `None`→null 的表示层差异）；② sessionId 剥除（各自会话）；③ 事件间相对序不锁定（Rust monitor 终态在流尾 vs TS finally——单类型内形态等价即契约）。
- draft:delta 的 text 与 session:title 的 title **逐值相等**（同 instruction → 同 mock 响应/同首条消息标题 derive）。

### 2. 勘误（对跑实战价值又一实证）：llm:progress 聊天轮过度发射

126 号把 `llm:progress` 钩子挂到了 `RouterLoopChat`（agent 聊天循环）——**对跑捕获 TS 侧普通聊天轮并不发该事件**：TS `onStreamProgress` 仅随 **pipeline 面**（写作链/确认式生产）构造的 client 上报，直通聊天轮的 `createLLMClient(config.llm)` 无进度回调。

修复：回滚 RouterLoopChat 钩子（聊天轮零 llm:progress，与 TS 逐字对齐）；`llm:progress` 保留于 pipeline 面（effective_router 钩子——books 写链/确认式生产经 `build_write_next_agents` 同源生效）。`sub126_e2e::chat_broadcasts_*` 测试同步改写（普通聊天轮断言**不发** llm:progress——负向守门）。

### 3. duel 基建增量

- fixture `inkos.json` stream:false → **true**（此前 false 使 Rust 聊天走非流式——对跑语境下与 TS 恒流式偏好不对齐；该键对测试 1-7 无影响）。
- 采集器/归一器两辅助函数（SSE 面对跑可复用于后续词汇扩充）。

## 二、验证基线（128 号时点）

| 项 | 结果 |
|---|---|
| `cargo test --lib` | 1168 过 |
| `cargo test --test golden_leaf` | 76 过 |
| `cargo test --test e2e_write_next_contract` | 187 过 |
| `cargo test --features export-bindings --lib` | 1327 过 |
| `cargo clippy --lib --tests --bins` | 零警告 |
| TS `packages/core` vitest | 185 文件 / 1798 全过 |
| `INKOS_DUEL=1 cargo test --test strangler_duel` | **8/8**（runbook 五步 + bin 装配 + 静态/CORS + SSE 事件面） |

## 三、事件面迁移状态终态

普通聊天轮共享词汇 4/4 事件双端逐负载等价；词汇表 63/66（余 thinking:* 三事件——UI hub 未订阅，备案）。126/127 号三项补齐事件经真进程对跑验收（一处过度发射勘误回滚）。

## 四、下一步候选

1. thinking:* 三事件勘测（agent 响应流消费路径确认后再定）。
2. 实切条件到位后真实流量切换（duel 8/8 前置门）。
3. 按需轮（用户指定）。
