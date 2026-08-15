//! 翻译源提取（source.ts + epub.ts）：text / markdown / epub；pdf 需 PDF 文本
//! 提取引擎（纯 Rust 侧无质量可用的轻量实现——偏差备案，明确错误）。

use std::path::Path;

use crate::translation::text::{
    normalize_translation_text, split_translation_chapters, strip_html,
    TranslationTextChapter,
};
use crate::translation::types::*;

const MAX_INPUT_BYTES: usize = 80 * 1024 * 1024;

pub async fn extract_translation_source(
    project_root: &Path,
    input: &CreateTranslationProjectInput<'_>,
) -> Result<ExtractedTranslationSource, String> {
    // safeChildPath：必须落在项目根内。
    let safe_path = safe_child_path(project_root, input.file_path)?;
    let buffer = tokio::fs::read(&safe_path)
        .await
        .map_err(|e| e.to_string())?;
    if buffer.len() > MAX_INPUT_BYTES {
        return Err(format!(
            "Translation input is too large ({} bytes).",
            buffer.len()
        ));
    }
    let source_path = to_posix_relative(project_root, &safe_path);
    let filename = safe_path
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or_default()
        .to_string();
    let ext = safe_path
        .extension()
        .and_then(|e| e.to_str())
        .map(|e| format!(".{}", e.to_lowercase()))
        .unwrap_or_default();

    let trimmed_title = input.title.map(str::trim).filter(|t| !t.is_empty());

    if ext == ".pdf" {
        return Err(
            "PDF translation sources are not supported in this engine build yet (requires a PDF text extraction engine)."
                .to_string(),
        );
    }

    if ext == ".epub" {
        let epub = extract_epub(&buffer)?;
        let chapters: Vec<TranslationTextChapter> = epub
            .chapters
            .iter()
            .map(|chapter| TranslationTextChapter {
                title: chapter.title.clone(),
                content: chapter.content.clone(),
            })
            .collect();
        let char_count: u64 = chapters.iter().map(|c| c.content.chars().count() as u64).sum();
        return Ok(ExtractedTranslationSource {
            title: trimmed_title
                .map(String::from)
                .or(epub.title)
                .unwrap_or_else(|| title_from_filename(&filename)),
            kind: TranslationSourceKind::Epub,
            source_path,
            char_count,
            total_pages: None,
            chapters,
        });
    }

    if matches!(ext.as_str(), ".txt" | ".md" | ".markdown") {
        let text = normalize_translation_text(&String::from_utf8_lossy(&buffer));
        let kind = if ext == ".txt" {
            TranslationSourceKind::Text
        } else {
            TranslationSourceKind::Markdown
        };
        let char_count = text.chars().count() as u64;
        return Ok(ExtractedTranslationSource {
            title: trimmed_title
                .map(String::from)
                .unwrap_or_else(|| title_from_filename(&filename)),
            kind,
            source_path,
            char_count,
            total_pages: None,
            chapters: split_translation_chapters(&text),
        });
    }

    Err(format!(
        "Unsupported translation input type: {}",
        if ext.is_empty() { &filename } else { &ext }
    ))
}

fn title_from_filename(filename: &str) -> String {
    match filename.rfind('.') {
        Some(idx) if idx > 0 => filename[..idx].to_string(),
        _ => filename.to_string(),
    }
}

fn to_posix_relative(project_root: &Path, path: &Path) -> String {
    path.strip_prefix(project_root)
        .unwrap_or(path)
        .to_string_lossy()
        .replace('\\', "/")
}

fn safe_child_path(project_root: &Path, raw: &str) -> Result<std::path::PathBuf, String> {
    let root = project_root
        .canonicalize()
        .unwrap_or_else(|_| project_root.to_path_buf());
    let candidate = root.join(raw);
    let normalized = candidate
        .canonicalize()
        .unwrap_or_else(|_| candidate.clone());
    if normalized.starts_with(&root) && normalized != root.parent().unwrap_or(&root) {
        Ok(normalized)
    } else {
        Err(format!("Translation input path escapes project root: {raw}"))
    }
}

// ── epub 提取（epub.ts） ─────────────────────────────────────────

pub struct ExtractedEpubChapter {
    pub title: String,
    pub content: String,
}

pub struct ExtractedEpub {
    pub title: Option<String>,
    pub chapters: Vec<ExtractedEpubChapter>,
}

pub fn extract_epub(buffer: &[u8]) -> Result<ExtractedEpub, String> {
    let reader =
        zip::ZipArchive::new(std::io::Cursor::new(buffer)).map_err(|e| e.to_string())?;
    let read_entry = |name: &str| -> Option<String> {
        let mut reader = reader.clone();
        let mut file = reader.by_name(name).ok()?;
        let mut text = String::new();
        std::io::Read::read_to_string(&mut file, &mut text).ok()?;
        Some(text)
    };

    let container_xml = read_entry("META-INF/container.xml")
        .ok_or("EPUB container.xml does not point to an OPF package.")?;
    let opf_path = extract_opf_path(&container_xml)
        .ok_or("EPUB container.xml does not point to an OPF package.")?;
    let opf = read_entry(&opf_path)
        .ok_or_else(|| format!("EPUB OPF package not found: {opf_path}"))?;

    let title = extract_first(&opf, r"(?i)<dc:title[^>]*>([\s\S]*?)</dc:title>")
        .map(|raw| decode_xml(&raw).trim().to_string())
        .filter(|t| !t.is_empty());
    let manifest = extract_manifest_items(&opf);
    let spine = extract_spine(&opf);
    let base_dir = opf_path
        .rsplit_once('/')
        .map(|(dir, _)| dir.to_string())
        .unwrap_or_else(|| ".".to_string());

    let mut chapters: Vec<ExtractedEpubChapter> = Vec::new();
    for idref in spine {
        let Some(href) = manifest.get(&idref) else {
            continue;
        };
        let chapter_path = if base_dir == "." {
            normalize_zip_path(href)
        } else {
            normalize_zip_path(&format!("{base_dir}/{href}"))
        };
        let Some(html) = read_entry(&chapter_path) else {
            continue;
        };
        let title_from_html = extract_first(&html, r"(?i)<h[1-6][^>]*>([\s\S]*?)</h[1-6]>")
            .map(|raw| {
                decode_xml(&raw)
                    .replace(['<', '>'], "")
                    .trim()
                    .to_string()
            })
            .filter(|t| !t.is_empty())
            .unwrap_or_else(|| format!("Chapter {}", chapters.len() + 1));
        let content = normalize_translation_text(&strip_html(&html));
        if content.is_empty() {
            continue;
        }
        chapters.push(ExtractedEpubChapter {
            title: title_from_html,
            content,
        });
    }
    if chapters.is_empty() {
        return Err("EPUB contains no readable spine chapters.".to_string());
    }
    Ok(ExtractedEpub { title, chapters })
}

fn extract_opf_path(container_xml: &str) -> Option<String> {
    extract_first(
        container_xml,
        r#"(?i)<rootfile[^>]+full-path=["']([^"']+)["']"#,
    )
}

fn extract_manifest_items(opf: &str) -> std::collections::HashMap<String, String> {
    let mut map = std::collections::HashMap::new();
    let re = regex::Regex::new(r"(?i)<item\b([^>]+)>").unwrap();
    let attr_re = |attrs: &str, name: &str| -> Option<String> {
        let pattern = format!(r#"(?i){name}\s*=\s*["']([^"']*)["']"#);
        let re = regex::Regex::new(&pattern).unwrap();
        re.captures(attrs)
            .and_then(|c| c.get(1))
            .map(|m| m.as_str().to_string())
    };
    for caps in re.captures_iter(opf) {
        let attrs = caps.get(1).map(|m| m.as_str()).unwrap_or_default();
        let (Some(id), Some(href)) = (attr_re(attrs, "id"), attr_re(attrs, "href")) else {
            continue;
        };
        map.insert(id, href);
    }
    map
}

fn extract_spine(opf: &str) -> Vec<String> {
    let spine_start = opf.find("<spine");
    let spine_end = opf.find("</spine>");
    let Some((start, end)) = spine_start.zip(spine_end) else {
        return Vec::new();
    };
    let spine = &opf[start..end];
    let re = regex::Regex::new(r#"(?i)<itemref\b[^>]*idref\s*=\s*["']([^"']+)["']"#).unwrap();
    re.captures_iter(spine)
        .filter_map(|caps| caps.get(1).map(|m| m.as_str().to_string()))
        .collect()
}

fn extract_first(text: &str, pattern: &str) -> Option<String> {
    let re = regex::Regex::new(pattern).ok()?;
    re.captures(text)
        .and_then(|caps| caps.get(1))
        .map(|m| m.as_str().to_string())
}

fn normalize_zip_path(path: &str) -> String {
    path.replace('\\', "/")
        .trim_start_matches("./")
        .to_string()
}

fn decode_xml(text: &str) -> String {
    text.replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&apos;", "'")
        .replace("&amp;", "&")
}
