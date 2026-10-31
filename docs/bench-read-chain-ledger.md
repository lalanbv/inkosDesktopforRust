# 读链性能基线台账（read-chain baseline ledger）

> 784 号：757 号演进路线 #5「对读链（chapters/7 类热端点）建固定静默窗基线台账，
> 防 0GC 收益被后续无感回吐」的清偿交付。机器可读基线 = `scripts/bench-baseline.json`
>（`read_chain/*` 七条）；本文件是人工可读台账：协议、场景映射、归因发现、候选登记。

## 一、协议（铁律）

1. **静默窗判定**：基准只认静默窗数值。`pnpm bench:gate`（bench-gate.mjs）内置负载前置闸
   ——1 分钟 loadavg ÷ 核数 > 0.20 时拒绝执行（257 号实测：满载漂移 +190%~+520% 且每轮
   漂移的基准各不相同）。手动跑 `cargo bench` 也须先核 `uptime`。
2. **漂移裁决**：门禁红 ≠ 回归。先隔离复跑（关负载源后单跑该 bench）——回到基线即噪声；
   隔离复跑仍超阈值才是真回退，此时定位本次 diff 中的分配/正则/IO 变化。**严禁 `--update`
   洗基线**（那是「优化落地后显式刷新」专用，拿来抹平回退=台账作废）。
3. **基线只增不改**：新增场景只追加；既有条目除「优化落地后显式 --update」外不得改写。
   合并脚本须断言零重叠。
4. **机器相关性**：基线仅同机可比（当前 darwin/arm64 18 核）；换机后 `--update` 重建并在
   本文件记一行。
5. **自检前置**：bench 场景首轮必须断言 HTTP 200（712 号教训：不许把 404/5xx 链静默测进
   基线——`read_chain` 组已内置）。

## 二、场景映射（read_chain 组，7 条）

生产形态全量路由 `router_books` + tower `oneshot` 端到端（axum 路由+提取器+handler+响应
序列化，非裸内核）。播种：12 章 × ~1600 字 + index.json + 真相文件 + 真仓 cozy.md 流派画像。

| 场景 | 端点 | UI 频率依据 | 首测（2026-10-31，load 0.23，双轮复现 ±1.5%） |
|---|---|---|---|
| `read_chain/list_books` | `GET /api/v1/books` | Dashboard 首屏 | **8.93 ms** |
| `read_chain/book_detail` | `GET /api/v1/books/:id` | 写作页载入 | **9.96 ms** |
| `read_chain/read_chapter` | `GET /api/v1/books/:id/chapters/:num` | 开章阅读（最重单章读） | 50.0 µs |
| `read_chain/chapter_workspace` | `GET .../chapters/:num/workspace` | 编辑器聚合视图 | 494.6 µs |
| `read_chain/truth_list` | `GET /api/v1/books/:id/truth` | 真相文件面板 | 147.9 µs |
| `read_chain/list_genres` | `GET /api/v1/genres` | 流派面板/建书流程 | 135.6 µs |
| `read_chain/analytics` | `GET /api/v1/books/:id/analytics` | 分析面板（Dashboard 四面板之一） | 472.6 µs |

## 三、首测发现（台账的第一笔归因）

**list_books / book_detail ≈ 9ms，比 read_chapter（50µs）重 180×**——跨轮复现稳定，
非负载漂移（负载比 0.23 略超 0.20 闸，但两轮全场景 ±1.5% 一致=真重量）。

归因链：两 handler 均经 `StateManager::get_next_chapter_number`
（engine-rs/src/state/manager.rs:164），其每次调用执行：

1. `resolve_durable_story_progress`（durable 进度链解析）
2. `bootstrap_structured_state_from_markdown`（state/state_bootstrap.rs:718）——
   **ensure 型引导**：mkdir_p + 装载校验 manifest/current_state/hooks/summaries 四件，
   缺失即从 markdown 重建。

**行为面注记**：「读端点内嵌 ensure 引导」可能系 legacy 书可用性所依（读列表顺带把
结构化状态补齐）——优化属语义变更（skip-fast-path/mtime 记忆化），**不在台账轮热修**，
登记为候选（见下）。9ms 量级本身非当前用户可感瓶颈（本地引擎、UI 一次性加载），
无紧迫性；此发现的价值=基线在位后，任何让它变更差的改动会被门禁抓住。

### 候选登记（按需开工，无日期承诺）

| 候选 | 归因 | 预期收益 | 风险 |
|---|---|---|---|
| get_next_chapter_number 快路径（状态四件齐且 manifest 进度≥durable 时跳过重建） | bootstrap 每读必跑 | list_books/book_detail 9ms→µs 级（200×） | ensure 语义时序变化——须 legacy 书兼容探针 |

## 四、0GC 资产关联（既有 21 条基线，非本轮产物，台账收录备查）

711 号 title_dedup（-19.8%）、719 号 render_hook_snapshot（-99.97%)+resolve_timing、
720 号正则清偿+settle 投影族、724 号 export_artifact（1.14ms）、725 号 backup_export
（3.57ms）——五轮 0GC 选点成果全部在 `scripts/bench-baseline.json` 回归闸内；
本台账将其与读链七条并档，形成「写链内核 + 读链端到端」完整防护面。
