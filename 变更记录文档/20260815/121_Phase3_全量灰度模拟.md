# 121 号变更记录：全量灰度模拟（runbook 第五步终验收）——sidecar 共存语义一站式验收 + 会话摘要分歧修复

## 一、背景

120 号候选首选：对跑第五步——同根双进程**共存**下按 runbook 顺序串行执行前四步场景，每步后双端只读桶复跑等价——"sidecar 保温"语义（98 号：切换后 sidecar 留存共读同盘）的自动化验收。

## 二、交付

### 1. `strangler_full_grey_simulation`（终验收）

同根下四步串行：① 只读基线对跑 → ② Rust 写（approve）→ ③ TS 写（review-mode + 建会话 + 聊天回合）→ ④ 双端模型探测——**每步后 10 只读端点双端复跑全等价**。

### 2. 会话摘要 playMode 分歧修复（对跑捕获）

列表摘要 `BookSessionSummary::to_json` 恒带 `"playMode": null`；TS `listBookSessions` 摘要**未设时省略键**（`...(session.playMode ? {...} : {})`）。修复为条件插入。

### 3. duel 基建加固（过程排障）

- **OS 分配端口**（bind :0 取空闲口）：根治两类污染——进程内多测试端口撞车、**跨 cargo 进程滞留 sidecar 占位导致跨根串读**（曾制造 playMode 假分歧，勘测后以 TS 源码真相回滚误改——"以代码为准"纪律的又一实证）。
- 滞留进程清理（`pkill tsx`）与日志重定向排障惯例。

### 4. 实跑结果

`INKOS_DUEL=1 cargo test --test strangler_duel`：**5/5 通过**（只读 / 跨端写读 / 会话域 / 模型配置域 / 全量灰度模拟）。**runbook 灰度五步的自动化验收体系完备且当前全绿**。

## 三、验证基线（121 号时点）

| 项 | 结果 |
|---|---|
| `cargo test --lib` | 1157 过 |
| `cargo test --test golden_leaf` | 76 过 |
| `cargo test --test e2e_write_next_contract` | 182 过 |
| `cargo test --features export-bindings --lib` | 1316 过 |
| `cargo clippy --lib --tests --bins` | 零警告 |
| TS `packages/core` vitest | 185 文件 / 1798 测试全过 |
| `INKOS_DUEL=1 cargo test --test strangler_duel` | **5/5** |

## 四、strangler 切换演练状态总结（117-121 五轮）

| runbook 步 | 验收资产 | 状态 |
|---|---|---|
| ① 只读面 | `readonly_face_duel`（10 端点） | ✅ 全一致 |
| ② 创作链写读 | `cross_write_read_duel`（双向） | ✅（捕获 review-mode 分歧并修复） |
| ③ 聊天面会话域 | `session_domain_duel`（含真实聊天回合） | ✅ |
| ④ 模型配置域 | `services_domain_duel`（探测逐字段） | ✅（捕获回退清单/候选构造双分歧并修复） |
| ⑤ 全量灰度 | `full_grey_simulation`（四步串行+复跑） | ✅（捕获会话摘要 playMode 分歧并修复） |

**五步全部具备不依赖用户资源的自动化双进程验收门**；三轮实战共捕获并修复 **4 个真实契约分歧**。真实流量切换（最终步）仍需用户三项环境条件。

## 五、下一步（122 号候选）

1. **首选：迁移收官文档终版 v4**（117-121 对跑资产与四处分歧修复并入总结 + runbook 补自动化验收章——切换 SOP 的最终形态）。
2. 或：实切条件到位后真实流量执行（用户三项条件）。
3. 或：按需轮（用户指定）。
