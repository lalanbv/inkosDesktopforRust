# 104 号变更记录：P3 一行接线批（play 语言判定 + daemon 真实章号 + secrets 保序）

## 一、背景

103 号审计"下一步"首选：P3 一行接线批三件——#13 play_language 接 `utils::infer_language`、#7 最小件 daemon:chapter 真实章号、#4 层 3 secrets 保序。三件均已在审计中核实现状（默认 zh / 定值 0 / 按名排序），本轮闭合。

## 二、交付

### 1. play 语言内容判定（73 号备案 #4 闭合）

- `execute_play_start`：`infer_language([title, premise, worldContract, visualContract, initialScene].filter(Boolean).join("\n"))` TS 逐字——五段非空拼接后按 CJK/Latin 比例判 zh/en（`utils::infer_language`）。
- 缺省开场正文按世界语言分流：en → `You enter "{title}".\n{premise || "The scene is set. Make your first move."}`（TS en 分支逐字）；zh 维持原文案。
- E2E `play_start_english_premise_infers_en_world_and_scene`：英文 title/premise → `world.json` `language=="en"` + `sceneText` 英文逐字（seed 开场对 en 世界走 fail-open——mock renderer 仅配 zh 关键词，行为面与 73 号一致）。

### 2. daemon:chapter 真实章号/状态（72 号备案 #1 最小件闭合）

- `write_one_chapter` 返回 `(成功位, 章号, 状态)` 三元组（原 `bool`）。
- `process_book` 对齐 TS `onChapterComplete(bookId, result.chapterNumber, result.status)`：**成功与审计未过两路都广播真实值**（原为成功路定值 `chapter:0, status:"ready-for-review"`）；异常路径仍只 `daemon:error`（TS catch 同）。重试路径的成功/未过同样广播真实值。
- E2E `daemon_write_cycle_broadcasts_real_chapter_and_status`：daemon start + 全链 mock LLM → 首轮写循环（启动即跑）→ 断言 `daemon:chapter` 含 `bookId b1` + `chapter:1` + `status ready-for-review` + 章节文件真实落盘。

### 3. 层 3 secrets 磁盘插入序（97 号备案 #1 闭合）

- `llm/secrets.rs` 增 `service_key_order(root)`：从 secrets.json **原始文本**扫描 `services` 对象一层键序（`json_object_keys` 迷你扫描器——serde BTreeMap 解析必丢序；键串/值串转义、嵌套对象数组深度跳过；任何异常返回空序）。
- 层 3 迭代：磁盘键序优先（sidecar 写入的 secrets.json 保序——strangler 共存场景的关键），键序外的余量按名排序回退（Rust 自写文件本就字母序，两径一致）。
- 单测 4：文本序保留（字母逆排样本）、嵌套值/含逗号括号字符串跳过、缺失/畸形/截断回退、磁盘读序 + 无文件空序。

## 三、parity 要点

- play 五源拼接判定与 en 开场文案 TS 逐字；daemon 广播值语义与 `onChapterComplete` 回调参数逐位对齐；层 3 迭代序与 `Object.entries` 插入序对齐。

## 四、偏差备案

1. **`json_object_keys` 转义键不解码**（`\uXXXX` 原样保留）——服务 id 为 ASCII（a-z0-9-），无实义转义；异常键不命中 HashMap 自动跳过，余量回退排序。
2. **daemon 重试路径的广播为行为收窄补充**：TS 重试环细节（handleAuditFailure 内部温度递进）仍属 72 号精简面备案；本轮只对齐广播值。

## 五、暂缓件（滚动）

103 号审计开放清单 #4/#7（最小件）/#13 已闭合；余：prompt-pack 体系（#1）、env 请求级合并（#2）、responses 传输 + provider 特判族（#3/#5）、pi-ai 模型卡（#6，维持）、Scheduler 其余面（#7 余量）、schema 中文简述（#8）、聊天卡 details/SSE 结构化（#9）、同步钩子族（#10）、回放治理输入（#11）、评审轮数配置位（#12）、authoring 边角（#14）。

## 六、验证基线

| 项 | 结果 |
|---|---|
| `cargo test --lib` | **1145** 过（+4：secrets 键序） |
| `cargo test --test golden_leaf` | 76 过 |
| `cargo test --test e2e_write_next_contract` | **169** 过（+2：daemon 真实值 / play en） |
| `cargo test --features export-bindings --lib` | **1304** 过（+4） |
| `cargo clippy --lib --tests --bins` | 零警告 |
| TS `packages/core` vitest | 185 文件 / 1798 测试全过 |

## 七、影响面与下一步（105 号候选）

1. **首选：strangler 实切演练**——按 98 号 runbook 只读面起跑（真实流量分桶对跑：books/世界/材料/日志读面切 Rust，写面留 sidecar），103 号就绪度结论的实战验证。
2. 其次：P3 余量精修（responses 传输 + provider 特判族同轮 / 聊天卡 details 外露——后者需前端契约确认）。
3. 或：schema 中文简述英文化（#8，提示词行为面小件）。
