# 70 号：translations 域 —— 翻译工作流六端点

**日期**：2026-08-15
**阶段**：Phase3 strangler 迁移（62 号清单清缺口：translations 域 6 条）
**契约源**：`packages/studio/src/api/server.ts` L6521-L6665（六端点 + storeTranslationUpload/safeUploadFileName）+ `packages/core/src/translation/`（9 模块 911 行：types / project / source / epub / run-store / runner / llm-model / export / text（已移植））

---

## 一、背景

62 号清单 translations 域 6 条：上传 → 建项 → 详情 → 执行（LLM 逐段翻译 + 术语表合并 + 章节评审）→ 导出（txt/md/epub）的完整翻译工作流。Rust 侧 61 号前已移植纯函数 text.rs（归一/剥 HTML/分章/分段），本号补齐其余七模块与全部端点。

## 二、交付

### 1. 域本体 `engine-rs/src/translation/`（+6 模块 ~1400 行）

- **types.rs**：全类型 serde（**camelCase**——吸取 69 号教训，磁盘形态直接对齐 TS）；`TranslationModelPort` trait（translate_segments + 默认 review_chapter 直通）。
- **source.rs**：`extract_translation_source`（text/markdown/epub；**safeChildPath 越界拒绝**；80MB 上限；UTF-8 lossy）。**epub 提取**（epub.ts）：zip 解包 → container.xml → OPF（dc:title/manifest/spine）→ 章节 xhtml（h1-h6 标题 + strip_html 复用 61 号面 + XML 实体解码）。pdf → 明确错误（偏差备案）。
- **project.rs**：`create_translation_project_from_file`——id = `{ISO 时间戳连字符化}-{slug(title)}`（Unicode 字母数字保留 + 折叠 + 80 码元）；source/translated 双目录逐章 JSON（translated 空段起步）；manifest（charCount 按 JS `.length` 的字符数语义）+ glossary.json + review-report.md。
- **run_store.rs**：manifest/chapter/glossary 读写 + `merge_glossary_terms`（source.trim().lower() 去重后写覆盖）+ LoadManifestError（NotFound/BadPayload 分叉——端点 404/500 语义）。
- **runner.rs**：`run_translation_project`——逐章：pending 段（无 target）批翻译（batchSize 1-32 clamp）→ 术语合并落盘 → 章节有序落盘 → reviewChapter（有 target 时）→ 状态推进（reviewed/translated）→ manifest 落盘；终态 review-report.md（`## 标题 / - passed / - summary / - issue` 行格式）。
- **llm_model.rs**：`LlmTranslationModel`（AgentRouter "translator" 端口；翻译 0.2/8192、评审 0.1/4096——提示词逐字）；**JSON 容错解析**（整串 → ```fence 剥离 → 首 `{` 到末 `}` 子串）；segments 解析（index 必须命中源段 + target 非空；全空 → Err）；glossary 解析（source/target 非空）。
- **export.rs**：txt/md 渲染（md `# 标题` + `> src -> dst` 引言行；txt 纯行）+ **epub 生成**（最小合法 EPUB 2：mimetype **STORED** + container.xml + content.opf + toc.ncx + 章节 xhtml 转义；zip crate）。

### 2. 端点 `engine-rs/src/server/translation_routes.rs`（6 条）

- `GET /translations`：目录枚举 + safe id + 坏 manifest skip + **projectId 降序**（新项目在前）。
- `POST /translations/upload`：`safe_upload_filename`（斜杠/NUL 折叠 + 空白压缩 + 保留集过滤 + 120 码元）+ dataUrl 解析（base64/裸）+ 80MB 413 + `.inkos/uploads/translation/{毫秒}-{name}` → `{storedPath,size,mimeType}`。
- `POST /translations/create`：MISSING_FILE_PATH / MISSING_LANGUAGES 400 → 创建 → 响应展开 + projectId/title。
- `GET /translations/:id`：manifest + report 全文 + **章节段级合并**（source 段序 + translated 按 index 配对，target/notes 缺省 ""）。
- `POST /translations/:id/run`：LlmTranslationModel（batchSize/maxTokens 透传）→ `{projectId,translatedSegments,reviewedChapters,reportPath}`；错误分类（上游正则 → 502 TRANSLATION_RUN_FAILED，**补 reqwest "error sending request"**（TS Node "fetch failed" 的等价形态）/ 其余 500 同 code）。
- `POST /translations/:id/export`：format（md 默认）/outputPath → `{outputPath,format,chaptersExported}`。

### 3. 依赖与接线

Cargo.toml + zip = "2"；mod.rs 注册 translation_routes + router_books 挂 6 条路由。

### 测试

- **lib 单测（8 个）**：创建链目录结构（source/translated/manifest/glossary/report + 分段形态）、拒绝链（越界/缺失/pdf/不支持类型）、**epub roundtrip**（手造 epub zip → 提取标题/章节/实体 → 导出 epub zip 读回 mimetype STORED + XHTML 转义）、LLM JSON 容错（fence/子串/坏串）、slug 安全、glossary 合并去重、**注入模型 runner 全链**（翻译+评审+状态+报告+术语表）、md/txt 导出形态。
- **E2E `translations70_e2e`（3 个）**：**mock LLM 全链**（upload → create → detail（pending/空 target）→ run（按 system 关键词分流的翻译/评审 mock）→ detail（reviewed + `[译]` 回填 + passed 报告 + 术语表）→ export md（`# 山河之书` + 译文）+ epub（zip 读回）→ 列表摘要）；校验面（缺 dataUrl 400/MISSING_FILE_PATH/MISSING_LANGUAGES/编码 INVALID_ID/404/非法 format 400）；上游失败 502 TRANSLATION_RUN_FAILED。

## 三、parity 要点

1. **id 派生**：`{时间戳连字符化}-{slug}`——前端按 id 前缀时间戳降序即最新在前，slug Unicode 折叠与 TS `\p{L}\p{N}` 一致。
2. **runner 的有序回填**：translated 章节按 source 段序重排（translatedByIndex map → source 顺序映射），断点续译（已译段跳过）。
3. **review 条件**：仅当章节存在非空 target 才评审；`passed` 决定 reviewed/translated。
4. **上游错误分类**：TS 正则逐词 + reqwest 连接错误形态补齐（fetch failed ↔ error sending request）。
5. **epub mimetype STORED**：EPUB 规范要求第一项不压缩——E2E 读回验证。

## 四、偏差备案

1. **pdf 源不支持**：TS 走 unpdf 提取；纯 Rust 无质量可用的轻量 PDF 文本提取（扫描件本就需 OCR），返回明确错误消息。前端上传 pdf 的项目在 Rust 端创建会失败——**切流前需确认前端约束或后续号接 pdf crate**。
2. **非法 export format 400**：TS 任意字符串会走 txt 渲染分支（扩展名原样）；Rust 显式 `INVALID_FORMAT` 400（前端只传三种合法值，边缘差异）。
3. **非 base64 dataUrl 的百分号解码**：TS queryUnescape 语义；Rust 按原始字节透传（前端上传均为 base64 形态）。
4. **charCount 计数**：TS `.length`（UTF-16 码元）；Rust chars().count()（Unicode 标量）——BMP 外字符（emoji 等）有 1 vs 2 差，展示用途无碍。
5. **mergeGlossaryTerms 输出序**：BTreeMap 键序 vs TS Map 插入序（按 source 语义等价）。

## 五、暂缓件（沿 69 号清单滚动）

- 62 号清单剩余：play 域 4 条、daemon/doctor/logs/radar 7 条、foundation/revise 1 条（缺口 18 → 12 条）
- pdf 源提取（pdf crate 选型）、生图链、film-authoring LLM 链、单章写作中途截断、actionPayload strict、/agent model 校验、resumeFrom、fetchWithProxy、attachments、模型四层解析

## 六、验证基线

| 套件 | 结果 |
| --- | --- |
| `cargo test --lib` | **1009** 通过（+8） |
| `cargo test --test golden_leaf` | 76 通过 |
| `cargo test --test e2e_write_next_contract` | **107** 通过（+3） |
| `cargo test --features export-bindings --lib` | 1168 通过 |
| `cargo clippy --lib --tests --bins` | 零警告 |
| `packages/core vitest run` | 185 文件 / 1798 测试全绿 |

## 七、影响面与下一步

- **影响面**：翻译工作流前端（上传/项目列表/双语对照详情/执行/导出）全部可切流；62 号清单缺口 18 → 12 条。
- **下一步（71 号候选）**：play 域 4 条（互动世界，64 号会话域已有 play 会话形态基础）；随后 daemon/doctor/logs/radar 7 条运维面 + foundation/revise 1 条收官，62 号清单清零。
