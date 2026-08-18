# 119 号变更记录：会话域跨端对跑（runbook 第三步——聊天面会话域）

## 一、背景

118 号候选首选：对跑扩面到会话域——strangler 切换第三步（聊天面）的会话存储/改名/聊天回合跨端兼容性。会话磁盘格式（store + transcript）是 67/68 号移植面，跨端读写兼容此前只有单侧 E2E。

## 二、交付

### 1. `strangler_session_domain_duel`（`strangler_duel.rs` 增 1 测试）

- **A 方向**：Rust `POST /sessions`（绑书 b1）→ **双端 GET 会话详情全等价**（normalize 后逐字段——session store 磁盘格式跨端互认）。
- **B 方向**：TS `POST /sessions` + `PUT /sessions/:id` 改名 → 双端可见新名 `跨端改名`。
- **C 方向**：TS **真实聊天回合**（`POST /agent`，mock LLM——双形态：TS 客户端实际走流式请求，mock 补 SSE 分支）→ 回合后双端 GET 会话仍全等价（transcript 追加不改会话读面契约）。

### 2. 排障收获（runbook 勘误）

- sidecar 聊天面经 `buildPipelineConfig` 解析模型时，`stream` 偏好实际仍走流式请求（客户端层默认）——对跑 mock 需双形态（SSE + JSON）；105/108 号的 stream 配置位在 sidecar 聊天面的生效路径与 Rust 不同步，记入切换注意项。

### 3. 实跑结果

`INKOS_DUEL=1 cargo test --test strangler_duel`：**3/3 通过**（只读 10 端点 + 跨端写读 + 会话域）。

## 三、验证基线（119 号时点）

| 项 | 结果 |
|---|---|
| `cargo test --lib` | 1157 过 |
| `cargo test --test golden_leaf` | 76 过 |
| `cargo test --test e2e_write_next_contract` | 182 过 |
| `cargo test --features export-bindings --lib` | 1316 过 |
| `cargo clippy --lib --tests --bins` | 零警告 |
| TS `packages/core` vitest | 185 文件 / 1798 测试全过 |
| `INKOS_DUEL=1 cargo test --test strangler_duel` | **3/3**（只读/写读/会话域） |

## 四、对跑资产终态（117-119 三步）

runbook 前三步（只读面/创作链写读/聊天面会话域）均有了可重复的双进程验收门——**全部不依赖用户资源**。118 号已捕获并修复一个真实契约分歧（review-mode 误 404），证明该资产的有效性。

## 五、下一步（120 号候选）

1. **首选：对跑第四步（模型配置域）**——services/secrets/test 端点双端行为对跑（探测计划对 mock 上游的一致性——深链/回退路径双方已逐字对齐，对跑可验证配置面互操作）。
2. 或：实切条件到位后全量执行（用户三项条件）。
3. 或：按需轮（用户指定）。
