# 118 号变更记录：跨端写读一致性对跑（runbook 第二步预演）——首战捕获真分歧（review-mode 404 勘误）

## 一、背景

117 号候选首选：对跑框架扩面到写路径——**单侧写、双端读**（A 方向 Rust 写 / B 方向 TS 写），再复跑只读桶（磁盘态演进下双端读面仍全等价）。双端并发写同根有互斥风险，故为串行跨端形态（双端同磁盘格式已由 114 号保证）。

## 二、交付

### 1. `strangler_cross_write_read_duel`（`strangler_duel.rs` 增 1 测试）

- **A 方向（Rust 写 → 双端读）**：Rust `POST /books/b1/chapters/1/approve` → 双端 `GET /books/b1` 章节摘要 `status=approved`（index.json 跨端可见）。
- **B 方向（TS 写 → 双端读）**：TS `PUT /books/b1/chapter-review-mode {"mode":"manual"}` → 双端 `GET` 同路径 `mode=manual`（book.json writing 跨端可见）。
- **C 复跑**：写后 10 只读端点双端再对跑（归一化后全等价）。

### 2. 首战捕获真分歧并修复（对跑框架价值的直接证明）

B 方向暴露：TS 写后 **Rust `GET /books/:id/chapter-review-mode` 404**。根因：48 号移植把 TS 的"inkos.json 缺失 → loadRawConfig 抛 → 404"怪癖**过度近似**为"`writing.reviewMode` **键**缺失也 404"——TS 真实语义（`readProjectChapterReviewMode` → `normalizeChapterReviewMode(undefined)`）是键缺失回退项目默认 **auto**，404 仅限文件缺失/不可解析。既有 48 号 E2E 因 fixture 恰含该键而从未暴露——**单侧测试结构性无法发现、双端对跑一跑即中**。

修复：`project_review_mode` 三态化（Loaded / Missing→auto / ConfigUnavailable→404 面），GET 与 PUT 回显同语义；补键缺失回归单测（`review_mode_missing_writing_key_falls_back_to_auto`）。

### 3. 实跑结果

`INKOS_DUEL=1 cargo test --test strangler_duel`：**2/2 通过**（10 只读端点全一致 + 跨端写读双向可见 + 写后复跑全一致）。

## 三、验证基线（118 号时点）

| 项 | 结果 |
|---|---|
| `cargo test --lib` | 1157 过 |
| `cargo test --test golden_leaf` | 76 过 |
| `cargo test --test e2e_write_next_contract` | **182** 过（+1：review-mode 键缺失回归） |
| `cargo test --features export-bindings --lib` | 1316 过 |
| `cargo clippy --lib --tests --bins` | 零警告 |
| TS `packages/core` vitest | 185 文件 / 1798 测试全过 |
| `INKOS_DUEL=1 cargo test --test strangler_duel` | **2/2**（只读 + 跨端写读） |

## 四、下一步（119 号候选）

1. **首选：对跑扩面第三步（聊天面/会话域）**——会话创建/重命名/列表跨端（Rust 写会话 → TS 读 transcript；TS 写 → Rust 回放）——历史回放（68 号 restore）是跨端兼容的关键面。
2. 或：实切条件到位后按 runbook 全量执行（用户三项条件）。
3. 或：按需轮（用户指定）。
