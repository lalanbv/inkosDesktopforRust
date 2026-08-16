# 83 号变更记录：材料工具面（ingest_material / retrieve_material）

- 日期：2026-08-14
- 阶段：Phase3 strangler 迁移（engine-rs）
- 契约源：`packages/core/src/materials/ingest.ts`（288 行，ingestMaterial 全链）、`packages/core/src/materials/retrieve.ts`（137 行，retrieveMaterials 全链）、`packages/core/src/agent/agent-tools.ts` L1048-1208（IngestMaterialParams / createIngestMaterialTool / RetrieveMaterialParams / createRetrieveMaterialTool）、`packages/core/src/agent/agent-session.ts` L832-940（注册面：material 双工具注册于 chat/short/script/storyboard/film/play/book-create/edit 全部分支）、`packages/core/src/utils/path-safety.ts`（safeChildPath）

## 一、背景

agent 聊天面的工具注册矩阵中，material 双工具（归档 URL/上传文件为可追溯 Markdown 卡 + 语义召回片段）是覆盖全部会话类型的最大缺口——TS 每个会话分支都带它们，Rust 聊天面此前只有 read/ls/grep（+play 三件）。`engine-rs/src/materials.rs` 自 P2 起是 2 行占位。本轮交付域本体 + 工具壳 + 全会话注册，聊天面的"资料归档 → 按需召回"参考链闭环。

## 二、交付

1. **`engine-rs/src/materials.rs`**（占位 2 行 → ~640 行，5 单测）：
   - `ingest_material`：url（reqwest 抓取：UA `InkOS/1.6 material-ingestion`、Accept 头、20s 超时、`.no_proxy()`、http/https 白名单、18MB 上限）与 file（`safe_child_path` 项目内词法校验）两源 → `extract_buffer_material`（html → webpage：script/style 剥离 + 标签剥离 + title 提取；text-like → text；pdf → 固定错误文案）→ 归一（实体解码 + CRLF + 行尾空白 + 3+ 空行折叠）→ `.inkos/materials/{utc-iso 冒点改横杠}-{slug}.md`（Markdown 卡：模板空行全滤）+ `.json`（manifest camelCase、pretty 无尾换行、excerpt 1600 码元、title 120 码元）
   - `retrieve_materials`：manifest 列表（损坏/缺字段静默跳过）→ purpose 过滤 → 词项打分（title +8 / source +4 / 正文首现 +2+max(0, 2−pos/4000)）→ 非零分保留 → UTF-16 半径 700 片段（无命中以前 500 为中心）→ score desc + title 升序 → limit 1-12（默认 5，NaN → 5）
   - `extract_terms`：NFKC（unicode-normalization）+ lower + `\p{L}\p{N}{2,}` 去重 ≤24
   - `slug`：`\p{L}\p{N}` 外折叠 `-`、首尾剥、80 码元、空回退 "material"
2. **`engine-rs/src/interaction/material_tools.rs`**（新建，3 单测）：
   - `tool_ingest_material`：sourceKind/purpose 枚举校验 → 域本体；成功文本逐字（"Material ingested: …/Kind: …; chars: …; source: …/Excerpt:"，PDF pages 行仅有；filter(Boolean) 空行滤除）+ details `{kind:"material_ingested", asset}`；错误透传 error_result
   - `tool_retrieve_material`：query 必需 + purpose 枚举；无结果固定文案；成功文本（"Retrieved N material snippet(s)." + 每条 `## i. title` 四行元数据 + 片段，空行保留）+ details `{kind:"material_retrieval", query, purpose?, results}`（purpose 未提供时键不出现，对齐 JSON.stringify 省略 undefined）
   - `material_tool_schemas`：两工具 OpenAI schema（描述/枚举/required 逐字）
3. **注册面**：`project_tools::execute_tool` 分发加两 case（全部聊天会话可用）；`agent_route` tools payload 在文件工具后恒追加 material 双 schema（play 条件追加保持其后）
4. **E2E `material83_e2e`**（2 测试）：
   - `chat_ingests_file_then_retrieves_snippet`：聊天指令 → mock LLM 首轮 ingest_material(file)、次轮 retrieve_material、终文；两卡 completed + 文本断言；注册面（ingest/retrieve/read 同现）；磁盘 `.inkos/materials` 一 md 一 json、manifest camelCase 断言
   - `chat_ingests_url_as_webpage`：mock HTTP 源（text/html + title）→ 聊天 ingest_material(url, purpose=research) → kind webpage + title 提取 + 正文入卡；manifest source/purpose 断言

## 三、parity 要点

- **会话内普适注册**：TS agent-session 每个分支都带 material 双工具——Rust 经 execute_tool 恒分发 + payload 恒注册，对齐覆盖面
- **归档不污染正典**：工具描述逐字强调 "must not mutate canon, chapters, scripts, or play state"——只写 `.inkos/materials`
- **Markdown 卡空行语义**：TS `renderMaterialMarkdown` 的 `filter(line => line !== "")` 滤的是**模板**空行（标题直连 Metadata）；正文自身的段落 `\n\n` 保留（normalizeText 只折叠 3+）
- **manifest 无尾换行**：TS `JSON.stringify(asset, null, 2)` 直写；md 卡同样 join("\n") 无尾换行
- **snippet 位置 = UTF-16 码元**：charStart/charEnd 与切片半径按 JS `String.slice` 语义（BMP 内与 TS 完全一致）
- **损坏 manifest 静默跳过**：召回不允许炸掉聊天轮（TS 注释语义逐字）

## 四、偏差备案

1. **PDF 抽取暂缓**：TS 走 unpdf（getDocumentProxy + extractText）；Rust 无对应依赖，pdf 源返回固定错误文案（"PDF text extraction is not supported by the Rust engine yet; …"），totalPages 恒缺失——待引入 PDF 文本抽取库后补
2. **score.toFixed(2) 舍入**：Rust `{:.2}` 与 JS toFixed 在极端半数位舍入上存在已知差异（如 2.675）——展示层无实际影响
3. **title 排序**：TS `localeCompare` vs Rust 字节序——同分时非 ASCII 标题排序可能不同（极罕见）
4. **工具层枚举校验文案**：TS 由 zod 产生（英文默认文案）；Rust error_result 自拟双语括注文案，语义等价
5. **url 协议错误文案**：TS `new URL` 抛 Invalid URL；Rust 译为 "Unsupported URL protocol: {url}"（解析失败与协议非法合并同一文案）

## 五、暂缓件

- propose_action / research_web / import_chapters 聊天工具（agent-session 注册面其余成员）
- PDF 文本抽取（引入依赖后补 totalPages/kind=pdf 全链）
- 聊天面空文本 + 工具成功 → `response: ""` 的 TS 分支形态（66 号范围）
- 散件：单章写作中途截断、/agent model 校验、resumeFrom、fetchWithProxy、attachments 归一化、模型四层解析

## 六、验证基线（2026-08-14）

- `cargo test --lib`：**1094**（+8：materials 5 + material_tools 3）
- `cargo test --test golden_leaf`：76
- `cargo test --test e2e_write_next_contract`：**140**（+2：material83_e2e）
- `cargo test --features export-bindings --lib`：**1253**（+8）
- `cargo clippy --lib --tests --bins`：零警告
- TS：`packages/core` vitest 185 文件 / **1798** 测试全过

## 七、影响面与下一步

- 影响面：`materials.rs` 从占位转正式域（lib 注册不变）；聊天工具面从 3（read/ls/grep）扩到 5（+material 双件），play 会话叠加至 8；E2E 80/82 号注册面断言不受影响（contains 语义）
- 下一步（84 号候选）：**首选 propose_action 确认卡聊天工具**（agent-session 注册面最后一大块——确认卡生成 + session 切换源）；其次 research_web / import_chapters；或散件收尾（单章截断/model 校验/resumeFrom/attachments/模型四层解析）
