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
//! - chaptersExported = 过滤后索引长度（含无文件的章，对齐 TS）
//! - approvedOnly 过滤 status === "approved"
//! - 无可导出章 → Err("No chapters to export.")（HTTP 层转 500 Export failed）

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::state::manager::StateManager;

/// 导出格式。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExportFormat {
    Txt,
    Md,
    Epub,
}

impl ExportFormat {
    pub fn parse(value: Option<&str>) -> Self {
        match value {
            Some("md") => ExportFormat::Md,
            Some("epub") => ExportFormat::Epub,
            _ => ExportFormat::Txt,
        }
    }

    fn extension(self) -> &'static str {
        match self {
            ExportFormat::Txt => "txt",
            ExportFormat::Md => "md",
            ExportFormat::Epub => "epub",
        }
    }

    fn content_type(self) -> &'static str {
        match self {
            ExportFormat::Epub => "application/epub+zip",
            ExportFormat::Md => "text/markdown; charset=utf-8",
            ExportFormat::Txt => "text/plain; charset=utf-8",
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

/// 章节文件查找表：`^\d{4}` 前缀 → 文件名（首个命中，对齐 TS buildChapterFileLookup）。
async fn build_chapter_file_lookup(chapters_dir: &Path) -> std::collections::HashMap<u32, String> {
    let mut lookup = std::collections::HashMap::new();
    let Ok(mut entries) = tokio::fs::read_dir(chapters_dir).await else {
        return lookup;
    };
    while let Ok(Some(entry)) = entries.next_entry().await {
        let name = entry.file_name().to_string_lossy().into_owned();
        if !name.ends_with(".md") {
            continue;
        }
        let digits: String = name.chars().take_while(|c| c.is_ascii_digit()).collect();
        if digits.len() != 4 {
            continue;
        }
        if let Ok(number) = digits.parse::<u32>() {
            lookup.entry(number).or_insert(name);
        }
    }
    lookup
}

fn escape_html(text: &str) -> String {
    text.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;")
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
pub async fn build_export_artifact(
    state: &StateManager,
    book_id: &str,
    format: ExportFormat,
    approved_only: bool,
    output_path: Option<&Path>,
) -> Result<ExportArtifact, String> {
    let index = state.load_chapter_index(book_id).await.map_err(|e| e.to_string())?;
    let book = state.load_book_config(book_id).await.map_err(|e| e.to_string())?;
    let chapters: Vec<&crate::models::chapter::ChapterMeta> = if approved_only {
        index.iter().filter(|ch| ch.status == crate::models::chapter::ChapterStatus::Approved).collect()
    } else {
        index.iter().collect()
    };
    if chapters.is_empty() {
        return Err("No chapters to export.".to_string());
    }

    let book_dir = state.book_dir(book_id);
    let chapters_dir = book_dir.join("chapters");
    let default_output = state
        .project_root()
        .join(format!("{book_id}_export.{}", format.extension()));
    let output_path: PathBuf = output_path.map(Path::to_path_buf).unwrap_or(default_output);
    let chapter_files = build_chapter_file_lookup(&chapters_dir).await;
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
            let lang = if book.language.as_deref() == Some("en") { "en" } else { "zh-CN" };
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
            let joiner = if format == ExportFormat::Md { "\n---\n\n" } else { "\n" };
            (format!("{book_id}.{}", format.extension()), parts.join(joiner).into_bytes())
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

// ── 零依赖 stored-zip EPUB ──────────────────────────────────────

/// IEEE CRC-32（zip 标准，反射多项式 0xEDB88320）。
fn crc32(data: &[u8]) -> u32 {
    static TABLE: std::sync::OnceLock<[u32; 256]> = std::sync::OnceLock::new();
    let table = TABLE.get_or_init(|| {
        let mut table = [0u32; 256];
        for (i, slot) in table.iter_mut().enumerate() {
            let mut crc = i as u32;
            for _ in 0..8 {
                crc = if crc & 1 != 0 { (crc >> 1) ^ 0xEDB8_8320 } else { crc >> 1 };
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
        StoredZipWriter { out: Vec::new(), entries: Vec::new() }
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
        self.out.extend_from_slice(&(data.len() as u32).to_le_bytes());
        self.out.extend_from_slice(&(data.len() as u32).to_le_bytes());
        self.out.extend_from_slice(&(name_bytes.len() as u16).to_le_bytes());
        self.out.extend_from_slice(&0u16.to_le_bytes());
        self.out.extend_from_slice(name_bytes);
        self.out.extend_from_slice(data);
        self.entries.push(ZipEntry { name: name.to_string(), crc, size: data.len() as u32, offset });
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
            self.out.extend_from_slice(&(name_bytes.len() as u16).to_le_bytes());
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
        self.out.extend_from_slice(&(self.entries.len() as u16).to_le_bytes());
        self.out.extend_from_slice(&(self.entries.len() as u16).to_le_bytes());
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

    let modified = format!("{}Z", &crate::utils::utc_time::utc_now_iso().get(..19).unwrap_or("1970-01-01T00:00:00"));
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
        zip.add(&format!("OEBPS/chapter{}.xhtml", idx + 1), document.as_bytes());
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
        tokio::fs::create_dir_all(book_dir.join("chapters")).await.unwrap();
        tokio::fs::write(book_dir.join("chapters").join("0001_风.md"), "# 第1章\n\n正文一。").await.unwrap();
        tokio::fs::write(book_dir.join("chapters").join("0002_云.md"), "# 第2章\n\n正文二。").await.unwrap();
        state
    }

    #[tokio::test]
    async fn txt_export_joins_title_and_chapters() {
        let dir = tempfile::tempdir().unwrap();
        let state = fixture(dir.path()).await;
        let artifact = build_export_artifact(&state, "b1", ExportFormat::Txt, false, None).await.unwrap();
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

    #[tokio::test]
    async fn md_export_uses_heading_and_divider() {
        let dir = tempfile::tempdir().unwrap();
        let state = fixture(dir.path()).await;
        let artifact = build_export_artifact(&state, "b1", ExportFormat::Md, false, None).await.unwrap();
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
        let artifact = build_export_artifact(&state, "b1", ExportFormat::Txt, true, None).await.unwrap();
        assert_eq!(artifact.chapters_exported, 1);
        let text = String::from_utf8(artifact.payload).unwrap();
        assert!(text.contains("正文一。"));
        assert!(!text.contains("正文二。"));
    }

    #[tokio::test]
    async fn empty_index_errors() {
        let dir = tempfile::tempdir().unwrap();
        let state = StateManager::new(dir.path());
        // 缺失书：loadBookConfig 在空章判断之前失败（TS 同序）。
        let error = build_export_artifact(&state, "ghost", ExportFormat::Txt, false, None)
            .await
            .unwrap_err();
        assert!(!error.is_empty());
    }

    #[tokio::test]
    async fn epub_is_valid_stored_zip_with_mimetype_first() {
        let dir = tempfile::tempdir().unwrap();
        let state = fixture(dir.path()).await;
        let artifact = build_export_artifact(&state, "b1", ExportFormat::Epub, false, None).await.unwrap();
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
        assert_eq!(crc32(b"The quick brown fox jumps over the lazy dog"), 0x414F_A339);
    }

    #[test]
    fn markdown_to_simple_html_extracts_title() {
        let (title, html) = markdown_to_simple_html("# 风起\n\n他走了。<b>加粗</b>\n\n");
        assert_eq!(title, "风起");
        assert_eq!(html, "<p>他走了。&lt;b&gt;加粗&lt;/b&gt;</p>");
        let (title, _) = markdown_to_simple_html("无标题正文");
        assert_eq!(title, "Untitled Chapter");
    }
}
