//! 导出工件构建（buildExportArtifact 移植，47 号）。
//!
//! 移植自 `packages/core/src/interaction/export-artifact.ts`：
//! - txt/md：标题头 + 章节正文按 TS join 语义拼接
//! - epub：EPUB 3 最小结构（mimetype → container.xml → OPF → nav → 章节 xhtml）。
//!   TS 用 epub-gen-memory（Node）；Rust 侧零依赖手写 stored-zip writer
//!   （zip 允许 method-0 存储，epub 规范只要求 mimetype 首位且不压缩——
//!   全部条目 stored 即满足）。
//!
//! ## 契约要点
//! - chaptersExported = 过滤后索引长度（对齐 TS；源一致性校验通过后无文件章不存在）
//! - approvedOnly 过滤 status === "approved"
//! - 源一致性校验（1681 号，TS 同口径拒绝）：缺章/同章双形态/索引重复/盘上孤儿
//!   → `ExportError::SourceMismatch`（HTTP 层 409 + CHAPTER_EXPORT_SOURCE_MISMATCH）
//! - 无可导出章 → `ExportError::NoChapters`（500，TS 同文案）

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::state::manager::StateManager;

/// 导出格式。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExportFormat {
    Txt,
    Md,
    Epub,
    /// R50/1645 号：整书 markdown zip 打包（按卷目录 + 元数据 manifest）。
    MdZip,
    /// R50/1652 号：Word 文档（最小 4-part OOXML，零新依赖）。
    Docx,
}

impl ExportFormat {
    pub fn parse(value: Option<&str>) -> Self {
        match value {
            Some("md") => ExportFormat::Md,
            Some("epub") => ExportFormat::Epub,
            Some("md-zip") => ExportFormat::MdZip,
            // 施工图 §1.4#1 的 mime 串仅用于 content_type 响应头；parse 面
            // 只收短名（TS GET 面无 mime 归一，加别名即双端漂移）。
            Some("docx") => ExportFormat::Docx,
            _ => ExportFormat::Txt,
        }
    }

    /// R50/1647 号：pub(crate)——export-save 落盘名须用格式正典扩展名
    /// （md-zip→zip；此前路由用原始 fmt 串拼出 `.md-zip` 文件名）。
    pub(crate) fn extension(self) -> &'static str {
        match self {
            ExportFormat::Txt => "txt",
            ExportFormat::Md => "md",
            ExportFormat::Epub => "epub",
            ExportFormat::MdZip => "zip",
            ExportFormat::Docx => "docx",
        }
    }

    fn content_type(self) -> &'static str {
        match self {
            ExportFormat::Epub => "application/epub+zip",
            ExportFormat::Md => "text/markdown; charset=utf-8",
            ExportFormat::Txt => "text/plain; charset=utf-8",
            ExportFormat::MdZip => "application/zip",
            ExportFormat::Docx => {
                "application/vnd.openxmlformats-officedocument.wordprocessingml.document"
            }
        }
    }
}

/// 导出工件。payload 已是最终字节（txt/md UTF-8 / epub zip）。
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ExportArtifact {
    pub output_path: String,
    pub file_name: String,
    pub chapters_exported: u32,
    pub total_words: u64,
    pub format: String,
    pub content_type: String,
    /// base64 之外的原始负载（serde 侧仅测试用）。
    #[serde(skip)]
    pub payload: Vec<u8>,
}

/// 章节文件查找表：章号 → 该章的章文件。认的章文件与索引重建同一集合（三种命名形态）；
/// 同章双形态取读侧的胜者（[`crate::state::manager::select_chapter_files`]，与
/// `find_chapter_file_by_number` 对该章的胜者恒等）。
///
/// 1678 号：此前是「目录序首个命中」——同章双形态的书（`0001-旧.md` 与 `0001_新.md`
/// 并存）导出哪一份取决于目录顺序，旧稿会被当成正文导出去。纯函数接收显式的文件名
/// 列表：择优不依赖目录顺序这件事得能直接测（目录读取顺序本身不可控）。
fn export_chapter_file_lookup(names: Vec<String>) -> std::collections::HashMap<u32, String> {
    crate::state::manager::select_chapter_files(names)
        .into_iter()
        .filter_map(|name| {
            crate::state::manager::leading_number(&name).map(|number| (number, name))
        })
        .collect()
}

/// 1681 号：导出源一致性——四类细节（对齐 TS `ChapterExportSourceError.details`；
/// 字段顺序即 [`Self::details_json`] 的键序，与 `JSON.stringify` 构造序逐字对齐）。
#[derive(Debug, Clone, PartialEq)]
pub struct ExportSourceMismatch {
    pub missing_chapter_numbers: Vec<u32>,
    pub duplicate_chapter_numbers: Vec<u32>,
    pub duplicate_index_numbers: Vec<u32>,
    pub unindexed_files: Vec<String>,
}

impl ExportSourceMismatch {
    /// 四组细节全空=校验通过（golden 集成测试的通过判据）。
    pub fn is_empty(&self) -> bool {
        self.missing_chapter_numbers.is_empty()
            && self.duplicate_chapter_numbers.is_empty()
            && self.duplicate_index_numbers.is_empty()
            && self.unindexed_files.is_empty()
    }

    /// details 的 JSON 串。手拼而非 `json!`/`Serialize`：serde_json 的 Map 是字母序
    ///（未开 preserve_order），键序会与 TS 构造序漂移；数组用 `serde_json::to_string`
    ///（紧凑无空格，同 `JSON.stringify`）。共享 golden 向量逐字锁定。
    pub fn details_json(&self) -> String {
        let numbers =
            |values: &[u32]| serde_json::to_string(values).unwrap_or_else(|_| "[]".to_string());
        let names =
            |values: &[String]| serde_json::to_string(values).unwrap_or_else(|_| "[]".to_string());
        format!(
            "{{\"missingChapterNumbers\":{},\"duplicateChapterNumbers\":{},\"duplicateIndexNumbers\":{},\"unindexedFiles\":{}}}",
            numbers(&self.missing_chapter_numbers),
            numbers(&self.duplicate_chapter_numbers),
            numbers(&self.duplicate_index_numbers),
            names(&self.unindexed_files),
        )
    }

    /// TS `ChapterExportSourceError.message` 逐字镜像（生产派生：TS 测试自证
    /// expectedMessage 出自同一模板，Rust 消费同一 golden）。
    pub fn message(&self) -> String {
        format!(
            "Chapter index and source files differ: {}. Inspect these files and reconcile the chapter index through chapter import before exporting.",
            self.details_json()
        )
    }

    /// HTTP 409 响应体的 details 对象（客户端按键取值，serde Map 字母序无语义影响）。
    pub fn details_value(&self) -> serde_json::Value {
        serde_json::json!({
            "missingChapterNumbers": self.missing_chapter_numbers,
            "duplicateChapterNumbers": self.duplicate_chapter_numbers,
            "duplicateIndexNumbers": self.duplicate_index_numbers,
            "unindexedFiles": self.unindexed_files,
        })
    }
}

/// 1681 号：导出错误结构化——调用方按变体分流（HTTP 层 409/500；agent 工具面
/// 直出 `Display`，对齐 TS 异常 message 冒泡）。
#[derive(Debug)]
pub enum ExportError {
    /// 无可导出章（TS 同文案；HTTP 层 500 裸串）。
    NoChapters,
    /// 源一致性校验拒绝（HTTP 层 409 + CHAPTER_EXPORT_SOURCE_MISMATCH）。
    SourceMismatch(ExportSourceMismatch),
    /// 其余（索引/配置加载失败、章目录缺失等；HTTP 层 500 裸串）。
    Other(String),
}

impl std::fmt::Display for ExportError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ExportError::NoChapters => write!(f, "No chapters to export."),
            ExportError::SourceMismatch(m) => write!(f, "{}", m.message()),
            ExportError::Other(e) => write!(f, "{e}"),
        }
    }
}

/// 1681 号：导出源一致性校验（TS `checkChapterExportSource` 同构）。
///
/// 1678 号出口的产品定线：两腿统一「拒绝」而非静默部分导出——缺章/错章的导出物
/// 是数据完整性事故（发布/打印的下游输入），同章双形态的择优是需要用户裁决的状态；
/// Node 腿（上游 2.0 语义）已是拒绝，本轮 Rust 腿对齐。
///
/// 校验面对**全量索引**——部分导出（chapters 勾选）/ approved-only 不豁免未导出章
/// 的对账（TS 同口径）。
pub(crate) fn check_export_source(
    files: &[String],
    indexed_numbers: &[u32],
) -> Result<(), ExportSourceMismatch> {
    let mut indexed: std::collections::BTreeSet<u32> = std::collections::BTreeSet::new();
    let mut duplicate_index_numbers: std::collections::BTreeSet<u32> =
        std::collections::BTreeSet::new();
    for &number in indexed_numbers {
        if !indexed.insert(number) {
            duplicate_index_numbers.insert(number);
        }
    }
    let mut covered: std::collections::BTreeSet<u32> = std::collections::BTreeSet::new();
    let mut duplicate_chapter_numbers: std::collections::BTreeSet<u32> =
        std::collections::BTreeSet::new();
    let mut unindexed_files: Vec<String> = Vec::new();
    // 排序=码点序（`&str` 字节序，与 TS compareByCodePoint 恒等——JS 裸 sort 的
    // UTF-16 码元序在增补平面字符上相反，双端错误消息会漂移；854 号同款尺子）。
    let mut sorted: Vec<&str> = files.iter().map(String::as_str).collect();
    sorted.sort_unstable();
    for file in sorted {
        if !file.ends_with(".md") {
            continue;
        }
        // 章文件的认定与索引重建同一集合（1678 号立的尺子：leading_number——
        // 正典 `NNNN_标题.md` / legacy `NNNN-标题.md` / 裸 `NNNN.md`；0 号与
        // 超 u32 同样落「未入索引」）。
        let Some(number) = crate::state::manager::leading_number(file) else {
            unindexed_files.push(file.to_string());
            continue;
        };
        if !indexed.contains(&number) {
            unindexed_files.push(file.to_string());
            continue;
        }
        if !covered.insert(number) {
            duplicate_chapter_numbers.insert(number);
        }
    }
    let mismatch = ExportSourceMismatch {
        // BTreeSet 迭代=升序（TS `.sort((a,b) => a-b)` 同序）。
        missing_chapter_numbers: indexed.difference(&covered).copied().collect(),
        duplicate_chapter_numbers: duplicate_chapter_numbers.into_iter().collect(),
        duplicate_index_numbers: duplicate_index_numbers.into_iter().collect(),
        unindexed_files,
    };
    if mismatch.is_empty() {
        Ok(())
    } else {
        Err(mismatch)
    }
}

/// 1681 号：`#[cfg(test)]` 之外的 pub 包装（集成测试无法触达 `pub(crate)` fn——
/// `markdown_to_simple_html_for_test` 同一惯例；tests/golden_export_source_diff.rs
/// 消费共享 golden 向量）。通过时四组细节全空。
pub fn check_export_source_for_test(
    files: &[String],
    indexed_numbers: &[u32],
) -> ExportSourceMismatch {
    match check_export_source(files, indexed_numbers) {
        Ok(()) => ExportSourceMismatch {
            missing_chapter_numbers: Vec::new(),
            duplicate_chapter_numbers: Vec::new(),
            duplicate_index_numbers: Vec::new(),
            unindexed_files: Vec::new(),
        },
        Err(m) => m,
    }
}

/// 列出章目录的全部条目名（校验与择优共用同一份目录快照）。目录缺失 →
/// Err（对齐 TS：readdir ENOENT 异常 → HTTP 500，而非静默当空目录）。
async fn read_chapter_dir_names(chapters_dir: &Path) -> std::io::Result<Vec<String>> {
    let mut entries = tokio::fs::read_dir(chapters_dir).await?;
    let mut names = Vec::new();
    while let Some(entry) = entries.next_entry().await? {
        names.push(entry.file_name().to_string_lossy().into_owned());
    }
    Ok(names)
}

fn escape_html(text: &str) -> String {
    text.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
}

/// markdown → 简单 xhtml：首个 `# ` 标题 + 非 # 行变 `<p>`。
fn markdown_to_simple_html(markdown: &str) -> (String, String) {
    // 214 号：对齐 TS `^#[ \t]+`——行首 # 后任意个空格/制表符（此前
    // strip_prefix("# ") 只接受恰一个空格，`#\t标题` 漂移）；标题内容
    // trim 后为空回退 Untitled（共享 golden 差分守门）。
    let title = markdown
        .lines()
        .find_map(|line| {
            let rest = line.strip_prefix('#')?.trim_start_matches([' ', '\t']);
            let trimmed = rest.trim();
            (!trimmed.is_empty()).then(|| trimmed.to_string())
        })
        .unwrap_or_else(|| "Untitled Chapter".to_string());
    let html = markdown
        .lines()
        .filter(|line| !line.starts_with('#'))
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .map(|line| format!("<p>{}</p>", escape_html(line)))
        .collect::<Vec<_>>()
        .join("\n");
    (title, html)
}

/// 214 号：共享 golden 差分入口（tests/golden_export_diff.rs）——
/// `#[cfg(test)]` 之外的 pub 包装（集成测试无法触达私有 fn）。
pub fn markdown_to_simple_html_for_test(markdown: &str) -> (String, String) {
    markdown_to_simple_html(markdown)
}

/// 构建导出工件。state 注入 StateManager；output_path 缺省 root/{bookId}_export.{format}。
///
/// 1681 号起错误结构化（[`ExportError`]）：源一致性校验拒绝 → 409（HTTP 层映射），
/// 无可导出章 → TS 同文案 500，其余 → 裸串 500。
pub async fn build_export_artifact(
    state: &StateManager,
    book_id: &str,
    format: ExportFormat,
    approved_only: bool,
    output_path: Option<&Path>,
    // R50/1645 号：章节勾选子集（Scrivener Compile 语义；None=全部）。
    chapter_filter: Option<&[u32]>,
) -> Result<ExportArtifact, ExportError> {
    let index = state
        .load_chapter_index(book_id)
        .await
        .map_err(|e| ExportError::Other(e.to_string()))?;
    let book = state
        .load_book_config(book_id)
        .await
        .map_err(|e| ExportError::Other(e.to_string()))?;
    let chapters: Vec<&crate::models::chapter::ChapterMeta> = if approved_only {
        index
            .iter()
            .filter(|ch| ch.status == crate::models::chapter::ChapterStatus::Approved)
            .collect()
    } else {
        index
            .iter()
            .filter(|ch| chapter_filter.is_none_or(|allowed| allowed.contains(&ch.number)))
            .collect()
    };
    if chapters.is_empty() {
        return Err(ExportError::NoChapters);
    }

    let book_dir = state.book_dir(book_id);
    let chapters_dir = book_dir.join("chapters");
    let default_output = state
        .project_root()
        .join(format!("{book_id}_export.{}", format.extension()));
    let output_path: PathBuf = output_path.map(Path::to_path_buf).unwrap_or(default_output);
    // 1681 号：源一致性校验（全量索引面；拒绝 → SourceMismatch）。校验与择优
    // 共用同一份目录快照（读一次）。校验通过 ⇒ 每个索引章号必有唯一文件。
    let dir_names = read_chapter_dir_names(&chapters_dir)
        .await
        .map_err(|e| ExportError::Other(e.to_string()))?;
    let indexed_numbers: Vec<u32> = index.iter().map(|ch| ch.number).collect();
    check_export_source(&dir_names, &indexed_numbers).map_err(ExportError::SourceMismatch)?;
    let chapter_files = export_chapter_file_lookup(dir_names);
    let total_words: u64 = chapters.iter().map(|ch| ch.word_count as u64).sum();

    let (file_name, payload) = match format {
        ExportFormat::Epub => {
            let mut epub_chapters: Vec<(String, String)> = Vec::new();
            for chapter in &chapters {
                let Some(file) = chapter_files.get(&chapter.number) else {
                    continue;
                };
                let markdown = tokio::fs::read_to_string(chapters_dir.join(file))
                    .await
                    .unwrap_or_default();
                let (title, html) = markdown_to_simple_html(&markdown);
                epub_chapters.push((title, html));
            }
            let lang = if book.language.as_deref() == Some("en") {
                "en"
            } else {
                "zh-CN"
            };
            (
                format!("{book_id}.epub"),
                build_epub(&book.title, lang, book_id, &epub_chapters),
            )
        }
        ExportFormat::Txt | ExportFormat::Md => {
            let mut parts: Vec<String> = Vec::new();
            parts.push(if format == ExportFormat::Md {
                format!("# {}\n\n---\n", book.title)
            } else {
                format!("{}\n\n", book.title)
            });
            for chapter in &chapters {
                let Some(file) = chapter_files.get(&chapter.number) else {
                    continue;
                };
                let content = tokio::fs::read_to_string(chapters_dir.join(file))
                    .await
                    .unwrap_or_default();
                parts.push(content);
                parts.push("\n\n".to_string());
            }
            let joiner = if format == ExportFormat::Md {
                "\n---\n\n"
            } else {
                "\n"
            };
            (
                format!("{book_id}.{}", format.extension()),
                parts.join(joiner).into_bytes(),
            )
        }
        ExportFormat::MdZip => {
            // R50/1645 号：整书打包——按卷目录（{order:02}_{title}，未分组
            // 00_ungrouped）+ 元数据 manifest.json。目录名路径分隔符 sanitize。
            let volumes = book
                .writing
                .as_ref()
                .and_then(|writing| writing.volumes.as_ref())
                .cloned()
                .unwrap_or_default();
            let sanitize_name = |raw: &str| -> String {
                raw.chars()
                    .map(|c| {
                        if matches!(c, '/' | '\\' | ':' | '*' | '?' | '"' | '<' | '>' | '|') {
                            '-'
                        } else {
                            c
                        }
                    })
                    .collect()
            };
            let mut writer = StoredZipWriter::new();
            let mut manifest_chapters: Vec<serde_json::Value> = Vec::new();
            for chapter in &chapters {
                let Some(file) = chapter_files.get(&chapter.number) else {
                    continue;
                };
                let content = tokio::fs::read_to_string(chapters_dir.join(file))
                    .await
                    .unwrap_or_default();
                let dir = chapter
                    .volume_id
                    .as_deref()
                    .and_then(|id| volumes.iter().find(|volume| volume.id == id))
                    .map(|volume| format!("{:02}_{}", volume.order, sanitize_name(&volume.title)))
                    .unwrap_or_else(|| "00_ungrouped".to_string());
                let path = format!(
                    "{dir}/{:04}_{}.md",
                    chapter.number,
                    sanitize_name(&chapter.title)
                );
                writer.add(&path, content.as_bytes());
                manifest_chapters.push(serde_json::json!({
                    "number": chapter.number,
                    "title": chapter.title,
                    "volumeId": chapter.volume_id,
                    "path": path,
                    "wordCount": chapter.word_count,
                }));
            }
            let manifest = serde_json::json!({
                "book": { "id": book_id, "title": book.title, "language": book.language },
                "volumes": volumes,
                "chapters": manifest_chapters,
            });
            writer.add(
                "manifest.json",
                serde_json::to_string_pretty(&manifest)
                    .unwrap_or_else(|_| "{}".to_string())
                    .as_bytes(),
            );
            (format!("{book_id}.zip"), writer.finish())
        }
        ExportFormat::Docx => {
            // R50/1652 号：最小 4-part OOXML（条目序固定，stored 零压缩=
            // method-0）。TS 镜像 = export-artifact.ts docx 分支（jszip
            // STORE）。
            let volumes = book
                .writing
                .as_ref()
                .and_then(|writing| writing.volumes.as_ref())
                .cloned()
                .unwrap_or_default();
            let mut docx_chapters: Vec<DocxChapterInput> = Vec::new();
            for chapter in &chapters {
                let Some(file) = chapter_files.get(&chapter.number) else {
                    continue;
                };
                let content = tokio::fs::read_to_string(chapters_dir.join(file))
                    .await
                    .unwrap_or_default();
                docx_chapters.push(DocxChapterInput {
                    number: chapter.number,
                    title: chapter.title.clone(),
                    volume_id: chapter.volume_id.clone(),
                    paragraphs: docx_body_paragraphs(&content),
                });
            }
            let document = docx_document_xml(&book.title, &volumes, &docx_chapters);
            let mut writer = StoredZipWriter::new();
            writer.add("[Content_Types].xml", DOCX_CONTENT_TYPES_XML.as_bytes());
            writer.add("_rels/.rels", DOCX_RELS_XML.as_bytes());
            writer.add("word/document.xml", document.as_bytes());
            writer.add("word/styles.xml", DOCX_STYLES_XML.as_bytes());
            (format!("{book_id}.docx"), writer.finish())
        }
    };

    Ok(ExportArtifact {
        output_path: output_path.to_string_lossy().into_owned(),
        file_name,
        chapters_exported: chapters.len() as u32,
        total_words,
        format: format.extension().to_string(),
        content_type: format.content_type().to_string(),
        payload,
    })
}

/// R50/1645 号：md-zip 条目路径构造（纯函数，双端 golden
/// chapter-mdzip-vectors.json 锁定）。卷目录 = `{order:02}_{title}`（路径
/// 分隔符 sanitize），未分组 = `00_ungrouped`；章文件 = `{NNNN}_{title}.md`
/// （正典 4 位补零形态）。TS 镜像 = export-artifact.ts `mdZipEntryPath`。
pub fn md_zip_entry_path(
    volume_id: Option<&str>,
    volumes: &[crate::models::book::BookVolume],
    chapter_number: u32,
    chapter_title: &str,
) -> String {
    let sanitize_name = |raw: &str| -> String {
        // R62/1690 号性质测试抓出：标题含连续点（".."）落进 zip 段=zip-slip
        // 家族——非法字符替换之外折叠连续点为 "-"，并剥离段尾点（与 TS 同修；
        // "标题." + ".md" 相连同样成 ".."）。
        let sanitized: String = raw
            .chars()
            .map(|c| {
                if matches!(c, '/' | '\\' | ':' | '*' | '?' | '"' | '<' | '>' | '|') {
                    '-'
                } else {
                    c
                }
            })
            .collect();
        sanitized
            .replace("..", "-")
            .trim_end_matches('.')
            .to_string()
    };
    let dir = volume_id
        .and_then(|id| volumes.iter().find(|volume| volume.id == id))
        .map(|volume| format!("{:02}_{}", volume.order, sanitize_name(&volume.title)))
        .unwrap_or_else(|| "00_ungrouped".to_string());
    format!(
        "{dir}/{:04}_{}.md",
        chapter_number,
        sanitize_name(chapter_title)
    )
}

// ── R50/1652 号：docx 导出纯函数族（TS 镜像 = export-artifact.ts docx*
// 系列，golden chapter-docx-vectors.json 锁定）。最小 4-part OOXML
// （ECMA-376）：[Content_Types].xml / _rels/.rels / word/document.xml /
// word/styles.xml——属性序固定、无缩进无注释、UTF-8 无 BOM = duel 条目级
// 逐位可比。零新依赖（StoredZipWriter stored 容器，EPUB/MdZip 同款）。──

/// XML 五实体转义（`&` 最先——防二次转义）。TS 镜像 = `docxEscapeXml`。
pub fn docx_escape_xml(text: &str) -> String {
    text.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&apos;")
}

/// 章正文切段：非 `#` 行 trim 后非空各一段——与 EPUB `markdown_to_simple_html`
/// 行语义一致（同书双格式分段一致；章标题由索引 title 单独成 Heading2 段，
/// 正文中的标题行滤除）。TS 镜像 = `docxBodyParagraphs`。
pub fn docx_body_paragraphs(markdown: &str) -> Vec<String> {
    markdown
        .lines()
        .filter(|line| !line.starts_with('#'))
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .map(str::to_string)
        .collect()
}

const DOCX_EAST_ASIA_FONT: &str = "SimSun";

/// (style_id, text) → `<w:p>`：pPr 样式引用 + run 级 rPr eastAsia（中文字族
/// 声明不依赖 styles 继承链）+ xml:space="preserve"。空文本 = 无 run 空段。
/// TS 镜像 = `docxParagraphXml`。
pub fn docx_paragraph_xml(style_id: &str, text: &str) -> String {
    if text.is_empty() {
        return format!(r#"<w:p><w:pPr><w:pStyle w:val="{style_id}"/></w:pPr></w:p>"#);
    }
    format!(
        concat!(
            r#"<w:p><w:pPr><w:pStyle w:val="{style_id}"/></w:pPr>"#,
            r#"<w:r><w:rPr><w:rFonts w:eastAsia="{font}"/></w:rPr>"#,
            r#"<w:t xml:space="preserve">{text}</w:t></w:r></w:p>"#,
        ),
        style_id = style_id,
        font = DOCX_EAST_ASIA_FONT,
        text = docx_escape_xml(text),
    )
}

/// docx 文档章节输入（章号 + 标题 + 卷归属 + 正文段）。TS 镜像 =
/// `DocxChapterInput`。
#[derive(Debug, Clone)]
pub struct DocxChapterInput {
    pub number: u32,
    pub title: String,
    pub volume_id: Option<String>,
    pub paragraphs: Vec<String>,
}

/// 章序 = (卷 order（未分组/未知卷 = 0 前置，与 md-zip `00_ungrouped` 桶序
/// 一致）, 章号) 稳定排序；卷桶首章前发 Heading1（卷名）。未分组章直排不造
/// 卷标题——docx 无目录树语义，直排更符 Word 阅读习惯（与 md-zip 的
/// `00_ungrouped` 目录形态有意不同）。sectPr = A4 + 1 英寸页边距（Word
/// 惯例 twips 值）。TS 镜像 = `docxDocumentXml`。
pub fn docx_document_xml(
    book_title: &str,
    volumes: &[crate::models::book::BookVolume],
    chapters: &[DocxChapterInput],
) -> String {
    let volume_of = |id: Option<&str>| -> Option<&crate::models::book::BookVolume> {
        id.and_then(|id| volumes.iter().find(|volume| volume.id == id))
    };
    let mut ordered: Vec<(&DocxChapterInput, Option<&crate::models::book::BookVolume>)> = chapters
        .iter()
        .map(|chapter| (chapter, volume_of(chapter.volume_id.as_deref())))
        .collect();
    ordered.sort_by_key(|(chapter, volume)| (volume.map_or(0, |v| v.order), chapter.number));
    let mut body = String::new();
    body.push_str(&docx_paragraph_xml("Title", book_title));
    let mut prev_bucket: Option<Option<&str>> = None;
    for (chapter, volume) in ordered {
        let bucket = volume.map(|volume| volume.id.as_str());
        if prev_bucket != Some(bucket) {
            if let Some(volume) = volume {
                body.push_str(&docx_paragraph_xml("Heading1", &volume.title));
            }
        }
        prev_bucket = Some(bucket);
        body.push_str(&docx_paragraph_xml("Heading2", &chapter.title));
        for paragraph in &chapter.paragraphs {
            body.push_str(&docx_paragraph_xml("Normal", paragraph));
        }
    }
    body.push_str(r#"<w:sectPr><w:pgSz w:w="11906" w:h="16838"/>"#);
    body.push_str(
        r#"<w:pgMar w:top="1440" w:right="1440" w:bottom="1440" w:left="1440" w:header="720" w:footer="720" w:gutter="0"/></w:sectPr>"#,
    );
    format!(
        concat!(
            r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>"#,
            r#"<w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main">"#,
            r#"<w:body>{body}</w:body></w:document>"#,
        ),
        body = body,
    )
}

/// 三常量 part（TS 字面镜像 = export-artifact.ts `DOCX_*_XML`，golden
/// chapter-docx-vectors.json staticParts sha256 锁定）。document.xml 由
/// docx_document_xml 产出；条目序 = [Content_Types].xml → _rels/.rels →
/// word/document.xml → word/styles.xml。
pub const DOCX_CONTENT_TYPES_XML: &str = r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?><Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types"><Default Extension="rels" ContentType="application/vnd.openxmlformats-package.relationships+xml"/><Default Extension="xml" ContentType="application/xml"/><Override PartName="/word/document.xml" ContentType="application/vnd.openxmlformats-officedocument.wordprocessingml.document.main+xml"/><Override PartName="/word/styles.xml" ContentType="application/vnd.openxmlformats-officedocument.wordprocessingml.styles+xml"/></Types>"#;

pub const DOCX_RELS_XML: &str = r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?><Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/officeDocument" Target="word/document.xml"/></Relationships>"#;

pub const DOCX_STYLES_XML: &str = r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?><w:styles xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"><w:docDefaults><w:rPrDefault><w:rPr><w:rFonts w:ascii="Calibri" w:hAnsi="Calibri" w:eastAsia="SimSun"/><w:sz w:val="24"/></w:rPr></w:rPrDefault></w:docDefaults><w:style w:type="paragraph" w:default="1" w:styleId="Normal"><w:name w:val="Normal"/><w:qFormat/></w:style><w:style w:type="paragraph" w:styleId="Title"><w:name w:val="Title"/><w:basedOn w:val="Normal"/><w:qFormat/><w:pPr><w:jc w:val="center"/></w:pPr><w:rPr><w:b/><w:sz w:val="56"/></w:rPr></w:style><w:style w:type="paragraph" w:styleId="Heading1"><w:name w:val="heading 1"/><w:basedOn w:val="Normal"/><w:next w:val="Normal"/><w:qFormat/><w:pPr><w:outlineLvl w:val="0"/></w:pPr><w:rPr><w:b/><w:sz w:val="36"/></w:rPr></w:style><w:style w:type="paragraph" w:styleId="Heading2"><w:name w:val="heading 2"/><w:basedOn w:val="Normal"/><w:next w:val="Normal"/><w:qFormat/><w:pPr><w:outlineLvl w:val="1"/></w:pPr><w:rPr><w:b/><w:sz w:val="30"/></w:rPr></w:style></w:styles>"#;

// ── 零依赖 stored-zip EPUB ──────────────────────────────────────

/// IEEE CRC-32（zip 标准，反射多项式 0xEDB88320）。
fn crc32(data: &[u8]) -> u32 {
    static TABLE: std::sync::OnceLock<[u32; 256]> = std::sync::OnceLock::new();
    let table = TABLE.get_or_init(|| {
        let mut table = [0u32; 256];
        for (i, slot) in table.iter_mut().enumerate() {
            let mut crc = i as u32;
            for _ in 0..8 {
                crc = if crc & 1 != 0 {
                    (crc >> 1) ^ 0xEDB8_8320
                } else {
                    crc >> 1
                };
            }
            *slot = crc;
        }
        table
    });
    let mut crc = 0xFFFF_FFFFu32;
    for &byte in data {
        crc = (crc >> 8) ^ table[((crc ^ byte as u32) & 0xFF) as usize];
    }
    !crc
}

struct ZipEntry {
    name: String,
    crc: u32,
    size: u32,
    offset: u32,
}

/// stored（method 0）zip 写入器：本地头 + 中央目录 + EOCD。
struct StoredZipWriter {
    out: Vec<u8>,
    entries: Vec<ZipEntry>,
}

impl StoredZipWriter {
    fn new() -> Self {
        StoredZipWriter {
            out: Vec::new(),
            entries: Vec::new(),
        }
    }

    fn add(&mut self, name: &str, data: &[u8]) {
        let offset = self.out.len() as u32;
        let crc = crc32(data);
        let name_bytes = name.as_bytes();
        // Local file header（version 2.0，flags 0，method 0=store，
        // DOS 时间 0 / 日期 1980-01-01=0x0021）。
        self.out.extend_from_slice(&0x0403_4B50u32.to_le_bytes());
        self.out.extend_from_slice(&20u16.to_le_bytes());
        self.out.extend_from_slice(&0u16.to_le_bytes());
        self.out.extend_from_slice(&0u16.to_le_bytes());
        self.out.extend_from_slice(&0u16.to_le_bytes());
        self.out.extend_from_slice(&0x0021u16.to_le_bytes());
        self.out.extend_from_slice(&crc.to_le_bytes());
        self.out
            .extend_from_slice(&(data.len() as u32).to_le_bytes());
        self.out
            .extend_from_slice(&(data.len() as u32).to_le_bytes());
        self.out
            .extend_from_slice(&(name_bytes.len() as u16).to_le_bytes());
        self.out.extend_from_slice(&0u16.to_le_bytes());
        self.out.extend_from_slice(name_bytes);
        self.out.extend_from_slice(data);
        self.entries.push(ZipEntry {
            name: name.to_string(),
            crc,
            size: data.len() as u32,
            offset,
        });
    }

    fn finish(mut self) -> Vec<u8> {
        let central_start = self.out.len() as u32;
        for entry in &self.entries {
            let name_bytes = entry.name.as_bytes();
            self.out.extend_from_slice(&0x0201_4B50u32.to_le_bytes());
            self.out.extend_from_slice(&20u16.to_le_bytes()); // version made by
            self.out.extend_from_slice(&20u16.to_le_bytes()); // version needed
            self.out.extend_from_slice(&0u16.to_le_bytes()); // flags
            self.out.extend_from_slice(&0u16.to_le_bytes()); // method store
            self.out.extend_from_slice(&0u16.to_le_bytes()); // mod time
            self.out.extend_from_slice(&0x0021u16.to_le_bytes()); // mod date
            self.out.extend_from_slice(&entry.crc.to_le_bytes());
            self.out.extend_from_slice(&entry.size.to_le_bytes());
            self.out.extend_from_slice(&entry.size.to_le_bytes());
            self.out
                .extend_from_slice(&(name_bytes.len() as u16).to_le_bytes());
            self.out.extend_from_slice(&0u16.to_le_bytes()); // extra len
            self.out.extend_from_slice(&0u16.to_le_bytes()); // comment len
            self.out.extend_from_slice(&0u16.to_le_bytes()); // disk number
            self.out.extend_from_slice(&0u16.to_le_bytes()); // internal attrs
            self.out.extend_from_slice(&0u32.to_le_bytes()); // external attrs
            self.out.extend_from_slice(&entry.offset.to_le_bytes());
            self.out.extend_from_slice(name_bytes);
        }
        let central_size = self.out.len() as u32 - central_start;
        // EOCD。
        self.out.extend_from_slice(&0x0605_4B50u32.to_le_bytes());
        self.out.extend_from_slice(&0u16.to_le_bytes()); // disk number
        self.out.extend_from_slice(&0u16.to_le_bytes()); // central dir disk
        self.out
            .extend_from_slice(&(self.entries.len() as u16).to_le_bytes());
        self.out
            .extend_from_slice(&(self.entries.len() as u16).to_le_bytes());
        self.out.extend_from_slice(&central_size.to_le_bytes());
        self.out.extend_from_slice(&central_start.to_le_bytes());
        self.out.extend_from_slice(&0u16.to_le_bytes()); // comment len
        self.out
    }
}

/// EPUB 3 最小结构：mimetype（首位 stored）→ container.xml → OEBPS/content.opf
/// → OEBPS/nav.xhtml → 章节 xhtml。
fn build_epub(title: &str, lang: &str, book_id: &str, chapters: &[(String, String)]) -> Vec<u8> {
    let mut zip = StoredZipWriter::new();
    zip.add("mimetype", b"application/epub+zip");

    let container = concat!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n",
        "<container version=\"1.0\" xmlns=\"urn:oasis:names:tc:opendocument:xmlns:container\">\n",
        "  <rootfiles>\n",
        "    <rootfile full-path=\"OEBPS/content.opf\" media-type=\"application/oebps-package+xml\"/>\n",
        "  </rootfiles>\n",
        "</container>\n",
    );
    zip.add("META-INF/container.xml", container.as_bytes());

    let mut chapter_items = String::new();
    let mut spine_items = String::new();
    let mut nav_items = String::new();
    for (idx, (chapter_title, _)) in chapters.iter().enumerate() {
        let id = format!("chapter{}", idx + 1);
        chapter_items.push_str(&format!(
            "    <item id=\"{id}\" href=\"{id}.xhtml\" media-type=\"application/xhtml+xml\"/>\n"
        ));
        spine_items.push_str(&format!("    <itemref idref=\"{id}\"/>\n"));
        nav_items.push_str(&format!(
            "        <li><a href=\"{id}.xhtml\">{}</a></li>\n",
            escape_html(chapter_title)
        ));
    }

    let modified = format!(
        "{}Z",
        crate::utils::utc_time::utc_now_iso()
            .get(..19)
            .unwrap_or("1970-01-01T00:00:00")
    );
    let opf = format!(
        concat!(
            "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n",
            "<package xmlns=\"http://www.idpf.org/2007/opf\" version=\"3.0\" unique-identifier=\"pub-id\">\n",
            "  <metadata xmlns:dc=\"http://purl.org/dc/elements/1.1/\">\n",
            "    <dc:identifier id=\"pub-id\">urn:inkos:{book_id}</dc:identifier>\n",
            "    <dc:title>{title}</dc:title>\n",
            "    <dc:language>{lang}</dc:language>\n",
            "    <meta property=\"dcterms:modified\">{modified}</meta>\n",
            "  </metadata>\n",
            "  <manifest>\n",
            "    <item id=\"nav\" href=\"nav.xhtml\" media-type=\"application/xhtml+xml\" properties=\"nav\"/>\n",
            "{chapter_items}  </manifest>\n",
            "  <spine>\n{spine_items}  </spine>\n",
            "</package>\n",
        ),
        book_id = escape_html(book_id),
        title = escape_html(title),
        lang = lang,
        modified = modified,
        chapter_items = chapter_items,
        spine_items = spine_items,
    );
    zip.add("OEBPS/content.opf", opf.as_bytes());

    let nav = format!(
        concat!(
            "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n",
            "<html xmlns=\"http://www.w3.org/1999/xhtml\" xmlns:epub=\"http://www.idpf.org/2007/ops\">\n",
            "  <head><title>{title}</title></head>\n",
            "  <body>\n    <nav epub:type=\"toc\">\n      <ol>\n{nav_items}      </ol>\n    </nav>\n  </body>\n",
            "</html>\n",
        ),
        title = escape_html(title),
        nav_items = nav_items,
    );
    zip.add("OEBPS/nav.xhtml", nav.as_bytes());

    for (idx, (chapter_title, html)) in chapters.iter().enumerate() {
        let document = format!(
            concat!(
                "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n",
                "<html xmlns=\"http://www.w3.org/1999/xhtml\">\n",
                "  <head><title>{title}</title></head>\n",
                "  <body>\n{body}\n  </body>\n",
                "</html>\n",
            ),
            title = escape_html(chapter_title),
            body = html,
        );
        zip.add(
            &format!("OEBPS/chapter{}.xhtml", idx + 1),
            document.as_bytes(),
        );
    }

    zip.finish()
}

#[cfg(test)]
mod tests {
    use super::*;

    async fn fixture(root: &Path) -> StateManager {
        let state = StateManager::new(root);
        state
            .save_book_config(
                "b1",
                &serde_json::from_str::<crate::models::book::BookConfig>(
                    r#"{"id":"b1","title":"风起","platform":"other","genre":"other","status":"active","targetChapters":10,"chapterWordCount":3000,"language":"zh","createdAt":"","updatedAt":""}"#,
                )
                .unwrap(),
            )
            .await
            .unwrap();
        let now = "2026-01-01T00:00:00.000Z";
        let meta = |number: u32, status: &str, words: u32| crate::models::chapter::ChapterMeta {
            number,
            title: format!("第{number}章"),
            status: serde_json::from_value::<crate::models::chapter::ChapterStatus>(
                serde_json::json!(status),
            )
            .unwrap(),
            word_count: words,
            created_at: now.into(),
            updated_at: now.into(),
            audit_issues: Vec::new(),
            volume_id: None,
            length_warnings: Vec::new(),
            review_note: None,
            detection_score: None,
            detection_provider: None,
            detected_at: None,
            length_telemetry: None,
            token_usage: None,
        };
        state
            .save_chapter_index(
                "b1",
                &[meta(1, "approved", 100), meta(2, "ready-for-review", 200)],
            )
            .await
            .unwrap();
        let book_dir = state.book_dir("b1");
        tokio::fs::create_dir_all(book_dir.join("chapters"))
            .await
            .unwrap();
        tokio::fs::write(
            book_dir.join("chapters").join("0001_风.md"),
            "# 第1章\n\n正文一。",
        )
        .await
        .unwrap();
        tokio::fs::write(
            book_dir.join("chapters").join("0002_云.md"),
            "# 第2章\n\n正文二。",
        )
        .await
        .unwrap();
        state
    }

    #[tokio::test]
    async fn txt_export_joins_title_and_chapters() {
        let dir = tempfile::tempdir().unwrap();
        let state = fixture(dir.path()).await;
        let artifact = build_export_artifact(&state, "b1", ExportFormat::Txt, false, None, None)
            .await
            .unwrap();
        assert_eq!(artifact.file_name, "b1.txt");
        assert_eq!(artifact.content_type, "text/plain; charset=utf-8");
        assert_eq!(artifact.chapters_exported, 2);
        assert_eq!(artifact.total_words, 300);
        let text = String::from_utf8(artifact.payload).unwrap();
        // parts = ["风起\n\n", c1, "\n\n", c2, "\n\n"] join "\n"：
        // 元素间各插一个 "\n"。
        assert!(text.starts_with("风起\n\n\n# 第1章\n\n正文一。"));
        assert!(text.ends_with("正文二。\n\n\n"));
    }

    /// 1678 号：同章双形态（`0001-旧.md` 与 `0001_新.md` 并存，845 号的读侧裁决是正典
    /// 优先）此前导出取正典、不随目录顺序变。1681 号定线改「拒绝」——双形态的择优是
    /// 需要用户裁决的状态（旧稿 vs 新稿），静默择优会把选择藏起来；两腿同口径（TS 校验
    /// 双形态即 duplicateChapterNumbers 拒绝）。择优纯函数本身仍由
    /// `export_lookup_prefers_the_canonical_file_whatever_the_listing_order` 锁定（读侧共用）。
    #[tokio::test]
    async fn dual_form_chapter_is_rejected_whatever_the_directory_order() {
        for (first, second) in [("0001-旧.md", "0001_新.md"), ("0001_新.md", "0001-旧.md")] {
            let dir = tempfile::tempdir().unwrap();
            let state = fixture(dir.path()).await;
            let chapters = state.book_dir("b1").join("chapters");
            tokio::fs::remove_file(chapters.join("0001_风.md"))
                .await
                .unwrap();
            // 内容跟着文件名走：正典 `0001_新.md` 是新稿，`0001-旧.md` 是旧稿；只变创建顺序。
            for name in [first, second] {
                let draft = if name == "0001_新.md" {
                    "新稿。"
                } else {
                    "旧稿。"
                };
                tokio::fs::write(chapters.join(name), format!("# 第1章\n\n{draft}"))
                    .await
                    .unwrap();
            }
            let error = build_export_artifact(&state, "b1", ExportFormat::Txt, false, None, None)
                .await
                .unwrap_err();
            let ExportError::SourceMismatch(m) = error else {
                panic!("expected SourceMismatch for dual-form chapter, got {error:?}");
            };
            assert_eq!(m.duplicate_chapter_numbers, vec![1]);
            assert!(m.missing_chapter_numbers.is_empty());
            assert!(m.unindexed_files.is_empty());
        }
    }

    /// 1681 号：源一致性拒绝——索引在、盘上缺（先红基础：1678 前的产品引擎会
    /// 静默跳过缺文件章导出其余）。
    #[tokio::test]
    async fn missing_chapter_is_rejected() {
        let dir = tempfile::tempdir().unwrap();
        let state = fixture(dir.path()).await;
        tokio::fs::remove_file(state.book_dir("b1").join("chapters").join("0002_云.md"))
            .await
            .unwrap();
        let error = build_export_artifact(&state, "b1", ExportFormat::Txt, false, None, None)
            .await
            .unwrap_err();
        let ExportError::SourceMismatch(m) = error else {
            panic!("expected SourceMismatch for missing chapter, got {error:?}");
        };
        assert_eq!(m.missing_chapter_numbers, vec![2]);
        assert!(m.duplicate_chapter_numbers.is_empty());
        assert!(m.unindexed_files.is_empty());
    }

    /// 1681 号：盘上孤儿（未入索引的 .md）拒绝；非 .md 文件不进细节。
    #[tokio::test]
    async fn orphan_and_non_md_files() {
        let dir = tempfile::tempdir().unwrap();
        let state = fixture(dir.path()).await;
        let chapters = state.book_dir("b1").join("chapters");
        tokio::fs::write(chapters.join("0003_未入索引.md"), "# 第3章\n\n孤儿。")
            .await
            .unwrap();
        tokio::fs::write(chapters.join("0001_notes.txt"), "非章文件")
            .await
            .unwrap();
        let error = build_export_artifact(&state, "b1", ExportFormat::Txt, false, None, None)
            .await
            .unwrap_err();
        let ExportError::SourceMismatch(m) = error else {
            panic!("expected SourceMismatch for orphan file, got {error:?}");
        };
        assert_eq!(m.unindexed_files, vec!["0003_未入索引.md"]);
    }

    /// 1681 号：校验面=全量索引——部分导出（chapters 勾选子集）不豁免未勾选章的
    /// 对账（TS 同口径：校验在前、过滤在后）。
    #[tokio::test]
    async fn partial_export_does_not_waive_reconciliation() {
        let dir = tempfile::tempdir().unwrap();
        let state = fixture(dir.path()).await;
        tokio::fs::remove_file(state.book_dir("b1").join("chapters").join("0002_云.md"))
            .await
            .unwrap();
        let error = build_export_artifact(&state, "b1", ExportFormat::Txt, false, None, Some(&[1]))
            .await
            .unwrap_err();
        let ExportError::SourceMismatch(m) = error else {
            panic!("expected SourceMismatch even for subset export, got {error:?}");
        };
        assert_eq!(m.missing_chapter_numbers, vec![2]);
    }

    /// 1678 号：同章双形态的择优不依赖目录顺序——显式列表正着给、倒着给，胜者都是正典。
    #[test]
    fn export_lookup_prefers_the_canonical_file_whatever_the_listing_order() {
        let mut forward = vec![
            "0001-旧.md".to_string(),
            "0001_新.md".to_string(),
            "0002.md".to_string(),
            "0003_收束.md".to_string(),
            "notes.md".to_string(),
        ];
        let lookup = export_chapter_file_lookup(forward.clone());
        forward.reverse();
        assert_eq!(
            export_chapter_file_lookup(forward),
            lookup,
            "倒序的结果应与正序相同"
        );
        assert_eq!(
            lookup.get(&1).map(String::as_str),
            Some("0001_新.md"),
            "同章双形态取正典"
        );
        assert_eq!(lookup.get(&2).map(String::as_str), Some("0002.md"));
        assert_eq!(lookup.get(&3).map(String::as_str), Some("0003_收束.md"));
        assert_eq!(lookup.len(), 3, "不是章文件的不进表");
    }

    /// 1678 号：旧命名的章（`NNNN-标题.md` / 裸 `NNNN.md`）照样导得出——查找表认的章文件
    /// 与索引重建同一集合。
    #[tokio::test]
    async fn legacy_named_chapters_are_exported() {
        let dir = tempfile::tempdir().unwrap();
        let state = fixture(dir.path()).await;
        let chapters = state.book_dir("b1").join("chapters");
        tokio::fs::remove_file(chapters.join("0001_风.md"))
            .await
            .unwrap();
        tokio::fs::remove_file(chapters.join("0002_云.md"))
            .await
            .unwrap();
        tokio::fs::write(chapters.join("0001-风.md"), "# 第1章\n\n正文一。")
            .await
            .unwrap();
        tokio::fs::write(chapters.join("0002.md"), "# 第2章\n\n正文二。")
            .await
            .unwrap();
        let artifact = build_export_artifact(&state, "b1", ExportFormat::Txt, false, None, None)
            .await
            .unwrap();
        assert_eq!(artifact.chapters_exported, 2);
        let text = String::from_utf8(artifact.payload).unwrap();
        assert!(
            text.contains("正文一。") && text.contains("正文二。"),
            "{text}"
        );
    }

    #[tokio::test]
    async fn md_export_uses_heading_and_divider() {
        let dir = tempfile::tempdir().unwrap();
        let state = fixture(dir.path()).await;
        let artifact = build_export_artifact(&state, "b1", ExportFormat::Md, false, None, None)
            .await
            .unwrap();
        assert_eq!(artifact.content_type, "text/markdown; charset=utf-8");
        let text = String::from_utf8(artifact.payload).unwrap();
        // parts[0] 以 "---\n" 结尾 + joiner "\n---\n\n" → 头部出现双分隔线（TS 同款）。
        assert!(text.starts_with("# 风起\n\n---\n\n---\n\n# 第1章"));
        assert!(text.contains("\n---\n\n# 第2章"));
    }

    #[tokio::test]
    async fn approved_only_filters_index() {
        let dir = tempfile::tempdir().unwrap();
        let state = fixture(dir.path()).await;
        let artifact = build_export_artifact(&state, "b1", ExportFormat::Txt, true, None, None)
            .await
            .unwrap();
        assert_eq!(artifact.chapters_exported, 1);
        let text = String::from_utf8(artifact.payload).unwrap();
        assert!(text.contains("正文一。"));
        assert!(!text.contains("正文二。"));
    }

    #[tokio::test]
    async fn empty_index_errors() {
        let dir = tempfile::tempdir().unwrap();
        let state = StateManager::new(dir.path());
        // 缺失书：loadBookConfig 在空章判断之前失败（TS 同序）；1681 号起
        // 结构化为 Other 裸串（HTTP 500 / 工具面 Display 直出）。
        let error = build_export_artifact(&state, "ghost", ExportFormat::Txt, false, None, None)
            .await
            .unwrap_err();
        let ExportError::Other(message) = error else {
            panic!("expected Other for missing book, got {error:?}");
        };
        assert!(!message.is_empty());
    }

    #[tokio::test]
    async fn epub_is_valid_stored_zip_with_mimetype_first() {
        let dir = tempfile::tempdir().unwrap();
        let state = fixture(dir.path()).await;
        let artifact = build_export_artifact(&state, "b1", ExportFormat::Epub, false, None, None)
            .await
            .unwrap();
        assert_eq!(artifact.content_type, "application/epub+zip");
        assert_eq!(artifact.file_name, "b1.epub");
        // zip 魔数 + 首文件名 mimetype（stored）。
        assert_eq!(&artifact.payload[..4], &[0x50, 0x4B, 0x03, 0x04]);
        let head = &artifact.payload[..30];
        assert_eq!(u16::from_le_bytes([head[8], head[9]]), 0); // method store
        let name_len = u16::from_le_bytes([head[26], head[27]]) as usize;
        assert_eq!(&artifact.payload[30..30 + name_len], b"mimetype");
        // 中央目录包含全部条目。
        let tail = &artifact.payload[artifact.payload.len() - 512..];
        assert!(tail.windows(4).any(|w| w == 0x0605_4B50u32.to_le_bytes()));
        let body = String::from_utf8_lossy(&artifact.payload).into_owned();
        assert!(body.contains("META-INF/container.xml"));
        assert!(body.contains("OEBPS/content.opf"));
        assert!(body.contains("urn:inkos:b1"));
        assert!(body.contains("<p>正文一。</p>"));
    }

    #[test]
    fn crc32_known_vectors() {
        assert_eq!(crc32(b""), 0);
        assert_eq!(crc32(b"123456789"), 0xCBF4_3926);
        assert_eq!(
            crc32(b"The quick brown fox jumps over the lazy dog"),
            0x414F_A339
        );
    }

    #[test]
    fn markdown_to_simple_html_extracts_title() {
        let (title, html) = markdown_to_simple_html("# 风起\n\n他走了。<b>加粗</b>\n\n");
        assert_eq!(title, "风起");
        assert_eq!(html, "<p>他走了。&lt;b&gt;加粗&lt;/b&gt;</p>");
        let (title, _) = markdown_to_simple_html("无标题正文");
        assert_eq!(title, "Untitled Chapter");
    }

    // ── R50/1652 号：docx 全链 ─────────────────────────────────────

    /// 解 stored zip local headers → (条目名, 内容) 序列（StoredZipWriter
    /// 产物无 extra 字段；duel 断言与 JSZip 解包同视角）。
    fn unzip_stored(payload: &[u8]) -> Vec<(String, Vec<u8>)> {
        let mut entries = Vec::new();
        let mut offset = 0usize;
        while payload[offset..].starts_with(&[0x50, 0x4B, 0x03, 0x04]) {
            let name_len =
                u16::from_le_bytes([payload[offset + 26], payload[offset + 27]]) as usize;
            let extra_len =
                u16::from_le_bytes([payload[offset + 28], payload[offset + 29]]) as usize;
            let size = u32::from_le_bytes([
                payload[offset + 18],
                payload[offset + 19],
                payload[offset + 20],
                payload[offset + 21],
            ]) as usize;
            let name_start = offset + 30;
            let data_start = name_start + name_len + extra_len;
            let name = String::from_utf8(payload[name_start..name_start + name_len].to_vec())
                .expect("条目名 UTF-8");
            entries.push((name, payload[data_start..data_start + size].to_vec()));
            offset = data_start + size;
        }
        entries
    }

    async fn docx_fixture(root: &Path) -> StateManager {
        let state = StateManager::new(root);
        let config: crate::models::book::BookConfig = serde_json::from_str(
            r#"{"id":"b1","title":"风起云涌","platform":"other","genre":"other","status":"active","targetChapters":4,"chapterWordCount":1000,"language":"zh","createdAt":"","updatedAt":"","writing":{"volumes":[{"id":"v1","title":"第一卷 风起","order":1},{"id":"v2","title":"第二卷 云涌","order":2}]}}"#,
        )
        .unwrap();
        state.save_book_config("b1", &config).await.unwrap();
        let now = "2026-01-01T00:00:00.000Z";
        let meta = |number: u32, volume_id: Option<&str>| crate::models::chapter::ChapterMeta {
            number,
            title: format!("第{number}章"),
            status: serde_json::from_value::<crate::models::chapter::ChapterStatus>(
                serde_json::json!("approved"),
            )
            .unwrap(),
            word_count: 5,
            created_at: now.into(),
            updated_at: now.into(),
            audit_issues: Vec::new(),
            volume_id: volume_id.map(str::to_string),
            length_warnings: Vec::new(),
            review_note: None,
            detection_score: None,
            detection_provider: None,
            detected_at: None,
            length_telemetry: None,
            token_usage: None,
        };
        state
            .save_chapter_index(
                "b1",
                &[
                    meta(1, Some("v1")),
                    meta(2, Some("v1")),
                    meta(3, Some("v2")),
                    meta(4, None),
                ],
            )
            .await
            .unwrap();
        let chapters_dir = state.book_dir("b1").join("chapters");
        tokio::fs::create_dir_all(&chapters_dir).await.unwrap();
        // 与 TS golden-chapter-docx.test.ts docxFixture 同数据（双端活体
        // sha256 对拍前提；正文段两端口径一致——renderChapterDocument 只
        // 重写标题行，标题行两侧都被 docx_body_paragraphs 滤除）。
        let bodies = [
            (1u32, "风起青萍。"),
            (2, "青萍之末。"),
            (3, "云涌微澜。"),
            (4, "未分组直排。"),
        ];
        for (number, body) in bodies {
            tokio::fs::write(
                chapters_dir.join(format!("{number:04}_第{number}章.md")),
                format!("# 第{number}章\n\n{body}"),
            )
            .await
            .unwrap();
        }
        state
    }

    #[tokio::test]
    async fn docx_export_builds_four_part_ooxml_with_volume_headings() {
        let dir = tempfile::tempdir().unwrap();
        let state = docx_fixture(dir.path()).await;
        let artifact = build_export_artifact(&state, "b1", ExportFormat::Docx, false, None, None)
            .await
            .unwrap();
        assert_eq!(artifact.file_name, "b1.docx");
        assert_eq!(
            artifact.content_type,
            "application/vnd.openxmlformats-officedocument.wordprocessingml.document"
        );
        assert_eq!(artifact.chapters_exported, 4);
        let entries = unzip_stored(&artifact.payload);
        assert_eq!(
            entries
                .iter()
                .map(|(name, _)| name.as_str())
                .collect::<Vec<_>>(),
            [
                "[Content_Types].xml",
                "_rels/.rels",
                "word/document.xml",
                "word/styles.xml"
            ],
            "条目序固定（duel 逐位可比前提）"
        );
        let document = String::from_utf8(entries[2].1.clone()).unwrap();
        // 卷 Heading1 序（未分组(0) 前置 → 卷1 → 卷2；未分组无卷标题）。
        let positions = [
            document.find("第4章</w:t>").unwrap(),
            document.find("第一卷 风起</w:t>").unwrap(),
            document.find("第1章</w:t>").unwrap(),
            document.find("风起青萍。</w:t>").unwrap(),
            document.find("第二卷 云涌</w:t>").unwrap(),
            document.find("云涌微澜。</w:t>").unwrap(),
        ];
        assert!(positions.windows(2).all(|pair| pair[0] < pair[1]));
        assert!(document.contains(r#"w:val="Title""#));
        assert!(document.ends_with("</w:sectPr></w:body></w:document>"));
        // 双端活体逐位对拍锚：word/document.xml sha256 = TS 同 fixture 生产
        // 值（golden chapter-docx-vectors.json fullChainDocumentDigest）。
        use sha2::{Digest, Sha256};
        let digest = format!("{:x}", Sha256::digest(entries[2].1.as_slice()));
        assert_eq!(
            digest, "dfa752c347d4d2a426bc3d6623abe421840f70f6821c07b6f017688a0fc6a8cc",
            "word/document.xml 与 TS 双端漂移"
        );
        // 常量 part 逐字节 = 生产常量。
        assert_eq!(entries[0].1, DOCX_CONTENT_TYPES_XML.as_bytes());
        assert_eq!(entries[1].1, DOCX_RELS_XML.as_bytes());
        assert_eq!(entries[3].1, DOCX_STYLES_XML.as_bytes());
    }

    #[tokio::test]
    async fn docx_export_honors_chapter_filter_subset() {
        let dir = tempfile::tempdir().unwrap();
        let state = docx_fixture(dir.path()).await;
        let artifact =
            build_export_artifact(&state, "b1", ExportFormat::Docx, false, None, Some(&[2]))
                .await
                .unwrap();
        assert_eq!(artifact.chapters_exported, 1);
        let entries = unzip_stored(&artifact.payload);
        let document = String::from_utf8(entries[2].1.clone()).unwrap();
        assert!(document.contains("第2章</w:t>"));
        assert!(document.contains("青萍之末。</w:t>"));
        assert!(!document.contains("第1章</w:t>"));
        assert!(!document.contains("云涌微澜。</w:t>"));
    }

    // textutil 活体开档（macOS 本机自动可读性用例——施工图 §1.5；Word/WPS/
    // Pages 三端人工走查备案变更记录）。
    #[cfg(target_os = "macos")]
    #[tokio::test]
    async fn docx_export_opens_with_textutil() {
        let dir = tempfile::tempdir().unwrap();
        let state = docx_fixture(dir.path()).await;
        let artifact = build_export_artifact(&state, "b1", ExportFormat::Docx, false, None, None)
            .await
            .unwrap();
        let path = dir.path().join("b1.docx");
        tokio::fs::write(&path, &artifact.payload).await.unwrap();
        let output = std::process::Command::new("textutil")
            .arg("-convert")
            .arg("txt")
            .arg("-stdout")
            .arg(&path)
            .output()
            .expect("textutil 可执行");
        assert!(output.status.success(), "textutil 开档失败");
        let text = String::from_utf8_lossy(&output.stdout);
        assert!(text.contains("风起云涌"));
        assert!(text.contains("风起青萍。"));
        assert!(text.contains("第一卷 风起"));
    }
}
