# 72 号：运维面收官 —— daemon/logs/doctor/radar 七端点 + 架构稿修订

**日期**：2026-08-15
**阶段**：Phase3 strangler 迁移收官轮（62 号清单清零：8 → 0 条）
**契约源**：`packages/studio/src/api/server.ts` L4463-L4536（daemon 三条 + logs）、L6434-L6456（radar 两条）、L6459-L6500（doctor）、L3615-L3630（foundation/revise）+ `packages/core/src/pipeline/scheduler.ts`（cronToMs + 写循环策略）、`agents/radar.ts` + `agents/radar-source.ts`（雷达域本体）、`pipeline/runner.ts` reviseFoundation（L818-L908 四文件装配）

---

## 一、背景

62 号下线核对清单最后 8 条：daemon 常驻进程管理（自动写作 + 雷达调度生命周期）、日志尾读、环境体检、素材雷达扫描/历史、架构稿修订。本号交付后 Rust 端点与 Node sidecar 的**契约面清零**——全部 130 个业务端点可切流。

## 二、交付

### 1. 雷达域本体 `engine-rs/src/agents/radar.rs`（~290 行）

- **双内置源**：番茄小说（JSON 榜单 API，热门/黑马双榜 ×15 条）+ 起点中文网（榜页 HTML 正则 `book.qidian.com/info/{id}` 链接文本，去重 ≤20 条，长度 2-30 过滤）——**单源容错**（失败跳过）+ **5s 预算客户端**（禁系统代理探测——WPAD/PAC 可挂数十秒且不受请求超时覆盖，开发中实测捕获）。
- **榜单文本化**：按平台分节 `- 标题 (作者) [分类] extra`；全空回退"（未能获取到实时排行数据，请基于你的知识分析）"。
- **市场分析师提示词逐字**（四分析维度 + JSON 输出契约 + 3-5 条 confidence 降序）→ LLM（温度 0.6）→ **JSON 贪婪提取**（首个 `{` 到末个 `}`，TS `/\{[\s\S]*\}/` 等价）+ recommendations/marketSummary 缺省回填 + ISO 时间戳。

### 2. 架构稿修订（architect.rs 扩展 + book_create_routes.rs 端点）

- `build_revise_prompt`（architect.ts 逐字）："既有架构稿修订模式"段——旧四文全文嵌入（缺失"（无）"）+ 六条任务规则（保留伏笔/不退回条目式/不动运行时）+ 用户额外要求。
- `generate_foundation_inner`（原函数拆出 + revise_prompt 注入 system 尾部；既有 3 调用点签名不变）。
- **`POST /books/:id/foundation/revise`**：feedback 必填 400 平铺 → phase 判定（story/outline/story_frame.md 存在即 phase5）→ **备份**（legacy 四文件 + phase5 的 outline 浅拷/roles 深拷到 `.backup-{phase}-{ts}`）→ 旧四文读取（phase5 走 outline 新布局三读 + book_rules；phase4 全 legacy）→ architect 修订重写 → **审核环容错**（未通过/失败仅 warn，accept rewrite 语义）→ `write_foundation_files(Revise)` → `foundation:revised` 广播 + `{ok:true}`；失败 `foundation:error` + 500。

### 3. 运维端点 `engine-rs/src/server/ops_routes.rs`（8 条）

- **daemon 三条**：`GET /daemon`（running）+ `POST /daemon/start`（重复 400 "Daemon already running"；Scheduler 生命周期：**cronToMs 逐字**（`*/N` 分 → N 分钟、`0 */N` → N 小时、其余每日——TS 同为间隔近似而非真 cron）+ 写循环/radar 循环两个 spawn 任务 + 500ms 分片睡眠可中断）+ `POST /daemon/stop`（未运行 400 "Daemon not running"；置位 + abort + `daemon:stopped`）。
- **写循环（Scheduler 精简）**：日上限（UTC 日期计数，`maxChaptersPerDay` 默认 50）→ active/outlining 书前 `maxConcurrentBooks` 本并发 → 每书 `chaptersPerCycle` 章（章间 `cooldownAfterChapterMs` 冷却 + 失败 `retryDelayMs` 重试 + **温度步进**（0.7 + failures×0.1 上限 1.2）+ 连续失败 3 次暂停该书）→ 成功 `daemon:chapter` 广播 / 失败 `daemon:error`。
- **radar tick**：间隔到 → run_radar → 落盘（与 POST /radar/scan 同链）。
- **`GET /logs`**：inkos.log 尾 100 行，逐行 JSON.parse 失败回 `{message: line}`。
- **`GET /doctor`**：六项——四文件存在（inkos.json/.env/`~/.inkos/.env`/books）+ bookCount（list_books）+ **llmConnected**（router models 端点探测，3s 总预算 + 禁代理——慢/限流上游按未连接上报，TS doctor 预算语义）。
- **`POST /radar/scan`**：`radar:start` → 扫描 → 落盘 → `radar:complete` + 结果 / 失败 `radar:error` + 500。
- **`GET /radar/history`**：radar/scan-*.json 枚举 + {file,timestamp,marketSummary,summaryPreview(100),result} + file 降序。
- **雷达存储**：`scan-{ISO 时间戳 [:.]→-}.json` pretty 落盘。

### 测试

- **lib 单测（7 个）**：cron_to_ms 四形态、today_key UTC 形状、DaemonConfig 默认与覆盖（嵌套 schedule 段）、雷达 JSON 解析（嵌噪音/缺省/无 JSON）、榜单格式化与空回退、雷达存储 roundtrip（时间戳文件名 + 历史降序 + 干扰文件过滤）、revise 提示词（四文嵌入/（无）占位/任务规则）。
- **E2E `ops72_e2e`（5 个）**：daemon 生命周期全环（idle → start + started 广播 → running → 重复 400 → stop + stopped → 重复 400）；logs 尾读（JSON/裸行混合 + 缺文件空表）；**radar 全链**（mock LLM 按系统提示词分流 → 扫描 200 + recommendations + 落盘 → 历史两条降序 + summaryPreview + radar:start/complete 广播）；doctor（文件面 + bookCount + 不可达 LLM 3s 内判 false 不挂起 + 耗时上界断言）；**revise 全链**（缺 feedback 400 → phase4 备份目录 + outline 落盘 + foundation:revised → 幽灵书 500）。
- **开发中实测捕获**：reqwest 默认系统代理探测（WPAD）导致外网源请求挂数十分钟且请求级 timeout 不覆盖——`no_proxy()` + 5s 预算修复（生产同受益）。

## 三、parity 要点

1. **cronToMs 是间隔近似**（TS 原样）：`*/15` = 15 分钟间隔而非 cron 精确对齐——两端行为一致。
2. **重复 start/stop 的 400 文案逐字**（"Daemon already running" / "Daemon not running" 平铺 error）。
3. **审核不阻断修订**：reviseFoundation 的 review 未通过仅记日志并接受重写（TS 同语义）。
4. **doctor 预算**：LLM 探测 3s 上界——确认不了连通性按 false 上报而非挂起诊断页。
5. **雷达 JSON 贪婪提取**：首个 `{` 到末个 `}`（模型输出包裹噪音时仍可解析）。

## 四、偏差备案

1. **Scheduler 精简面**：暂缓——detection 自动改写环（52 号域可后接）、webhook 通知（notify/dispatcher）、失败维度聚类暂停、写循环 tick 与周期边界的 cron 精确对齐、`daemon:chapter` 广播的 chapter 号（当前为定值 0——write_next 返回值带章节号，接线随后续号）。
2. **radar 外网源质量**：无外网环境下两源全空 → 提示词回退"基于知识分析"（TS 同语义）；reqwest 直连不走系统代理（TS Node fetch 会走环境代理——内网代理环境行为差异）。
3. **doctor llmConnected 探测面**：models 端点 200 判真（TS probeServiceCapabilities 更全——含 apiFormat/stream 协商）；`~/.inkos/.env` 路径经 HOME 环境变量（TS homedir 同源）。
4. **revise 的 phase5 深备份**：roles 目录按 UTF-8 文本复制（二进制文件跳过——角色卡全文本，无实害）。

## 五、62 号清单清零核账

| 域 | 端点数 | 交付号 |
| --- | --- | --- |
| interactive-films / projects | 10 | 69 号 |
| translations | 6 | 70 号 |
| play | 4 | 71 号 |
| daemon / logs / doctor / radar | 7 | 本号 |
| books 补充（foundation/revise） | 1 | 本号 |

62 号清单 28 条缺口 → **0 条**。Rust 端点总数 116 → **130**，与 Node sidecar 契约面全量对齐（62 号清单"80 可切换 + 52 缺口"的缺口侧清零）。

## 六、验证基线

| 套件 | 结果 |
| --- | --- |
| `cargo test --lib` | **1022** 通过（+7） |
| `cargo test --test golden_leaf` | 76 通过 |
| `cargo test --test e2e_write_next_contract` | **116** 通过（+5） |
| `cargo test --features export-bindings --lib` | 1181 通过 |
| `cargo clippy --lib --tests --bins` | 零警告 |
| `packages/core vitest run` | 185 文件 / 1798 测试全绿 |

## 七、影响面与下一步

- **影响面**：桌面端运维面（常驻进程控制台/日志查看/环境体检/市场雷达/架构稿修订面板）全部可切流；**62 号对照清单清零**——strangler 迁移的端点契约阶段完成。
- **下一步（73 号候选）**：转入**深度收尾**——各号偏差备案的暂缓件按价值排序：① play 写入面与 runner（play_start 意图执行器 + 回合 LLM 执行流，解锁互动世界完整切流）；② 生图链（cover 基础设施——一次接线解锁 69 号 node-image 与 71 号 play generate-image 两处 503）；③ draft_structure/connect_choice/remove_node 三执行器（域本体已就绪，纯接线）；④ 单章写作中途截断 + actionPayload strict + model 校验等契约收紧件。
