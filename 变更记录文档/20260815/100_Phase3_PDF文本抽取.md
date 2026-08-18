# 100 号变更记录：PDF 文本抽取（83 号备案闭合，P2-2 完成）

## 一、背景

99 号变更记录"下一步"首选：PDF 抽取选型轮。TS 侧 `ingest.ts` 的 pdf 分支走 unpdf（pdf.js 封装）：`extractText(mergePages)` → normalizeText → 空文本抛 `"PDF text extraction returned no text. Scanned PDFs require OCR and are not supported yet."`；产物 kind "pdf" + totalPages。Rust 侧（83 号）为固定错误文案。

## 二、选型结论

| 候选 | 评估 | 结论 |
|---|---|---|
| `pdf` crate v0.10 | 无 `text_extraction` feature（已移除） | 弃 |
| **`pdf-extract` v0.12** | 纯 Rust（lopdf 上层），`extract_text_from_mem` 单 API，支持 ToUnicode CMap 的常规文本 PDF；对手工最小 PDF 解析脆弱但对规范 PDF 稳定 | **采纳** |
| lopdf 自实现文本层 | 需自行遍历 content stream（Tj/TJ + 字体编码 + CMap），工作量大且易错 | 弃（作页数辅助依赖保留） |

页数（TS totalPages）：`lopdf::Document::load_mem().get_pages().len()`（pdf-extract 同树依赖，轻量直用；失败省略 manifest 键）。

## 三、交付

### 1. `materials.rs` pdf 分支真抽取

- `pdf_extract::extract_text_from_mem` → `normalize_text`（既有）→ **空文本 → TS 逐字错误**（扫描件需 OCR）；解析失败 → `PDF text extraction failed: {e}`（非 PDF 字节走此面）。
- 产物：`kind "pdf"` + `title Some(去扩展名)` + `mime application/pdf` + `total_pages`（lopdf 解析，Option 省略键语义与 manifest 既有序列化一致）。
- 头注释与 83 号备案闭合。

### 2. Fixture（`tests/fixtures/`，cupsfilter 生成规范 PDF）

- `sample.pdf`：单页两段英文文本（930B 规范 PDF——手工最小 PDF 会被 pdf-extract 拒，规范字节流稳定）。
- `scan.pdf`：空白文本内容流（扫描件语义——无可抽取文本）。

### 3. 测试

- 单测 +1：`pdf_ingestion_extracts_text_and_pages`——file 模式全链（kind/title/mime/excerpt 含两段文本/totalPages Some(1)）+ scan fixture → OCR 逐字错误；既有 83 号假 pdf 错误路径断言更新为新错误面前缀。
- E2E +1：`sub100_e2e::chat_ingests_pdf_material_with_extracted_text`——聊天面 `ingest_material(file)` → 卡片 `Material ingested:` + `PDF pages: 1` + manifest kind/totalPages/excerpt 断言。

## 四、parity 要点

- 空文本判定与逐字错误文案、kind/title/mime/totalPages 产物面、normalizeText 复用一致。

## 五、偏差备案

1. **抽取质量差分未做深对齐**：unpdf（pdf.js）与 pdf-extract 在复杂排版（多栏/表格/嵌入字体子集）上的输出细节有差异——材料归档是"召回语料"用途（打分/摘录消费），非逐字节契约面；以 fixture 行为对齐为准。
2. **非 PDF 字节错误面**：TS 在 getDocumentProxy 层抛 pdf.js 解析错误；Rust 归一为 `PDF text extraction failed: {e}` 前缀。
3. totalPages 解析失败静默省略（TS 恒有值——unpdf 解析失败时整个分支已抛错；Rust 文本抽取与页数解析独立，页数失败不阻断归档）。

## 六、暂缓件（滚动）

- P2 仅剩：**单章写作中途截断**（写作链检查点信号——writer 编排本体改造，规模最大）。
- P3：层 3 secrets 保序、provider 特判族、responses 传输、模型卡元数据。

## 七、验证基线

| 项 | 结果 |
|---|---|
| `cargo test --lib` | **1139** 过（+1） |
| `cargo test --test golden_leaf` | 76 过 |
| `cargo test --test e2e_write_next_contract` | **165** 过（+1：sub100） |
| `cargo test --features export-bindings --lib` | **1298** 过（+1） |
| `cargo clippy --lib --tests --bins` | 零警告 |
| TS `packages/core` vitest | 185 文件 / 1798 测试全过 |

## 八、影响面与下一步（101 号候选）

P2 清单仅剩单章截断一件。下一轮候选：

1. **首选：单章写作中途截断**（writer 编排检查点信号——abort 在章节内安全点生效；涉及 write_next 链本体与 settler 原子落盘边界，为收尾最大件）。
2. 其次：P3 精修批（层 3 secrets 保序 / responses 传输 / provider 特判族）。
3. 或：迁移收官审计轮（全备案清单复核 + 94/98 基线终版 + 迁移总结文档）。
