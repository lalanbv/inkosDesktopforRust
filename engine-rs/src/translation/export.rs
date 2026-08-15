//! 翻译导出（export.ts）：txt / md / epub（最小合法 EPUB 2 打包）。

use std::io::Write as _;
use std::path::{Path, PathBuf};

use crate::translation::run_store::*;
use crate::translation::types::*;

pub async fn write_translation_export(
    project_root: &Path,
    project_id: &str,
    format: TranslationExportFormat,
    output_path: Option<&str>,
) -> Result<TranslationExportResult, String> {
    let manifest = load_translation_manifest(project_root, project_id)
        .await
        .map_err(|e| match e {
            LoadManifestError::NotFound => format!("translation project not found for {project_id}"),
            LoadManifestError::BadPayload(message) => message,
        })?;
    let output_path: PathBuf = match output_path {
        Some(path) => project_root.join(path),
        None => translation_project_dir(project_root, project_id)
            .join("exports")
            .join(format!("{}.{}", safe_filename(&manifest.title), format_str(format))),
    };
    if let Some(parent) = output_path.parent() {
        tokio::fs::create_dir_all(parent).await.map_err(|e| e.to_string())?;
    }

    let chapters = load_chapters(project_root, &manifest).await?;
    match format {
        TranslationExportFormat::Epub => {
            let bytes = build_epub(&manifest, &chapters)?;
            tokio::fs::write(&output_path, bytes).await.map_err(|e| e.to_string())?;
        }
        format => {
            let rendered = render_text_export(&manifest, &chapters, format);
            tokio::fs::write(&output_path, rendered).await.map_err(|e| e.to_string())?;
        }
    }

    Ok(TranslationExportResult {
        output_path: to_posix_path(project_root, &output_path),
        format,
        chapters_exported: manifest.chapters.len(),
    })
}

async fn load_chapters(
    project_root: &Path,
    manifest: &TranslationProjectManifest,
) -> Result<Vec<TranslationChapterFile>, String> {
    let mut chapters = Vec::new();
    for chapter_info in &manifest.chapters {
        chapters.push(load_translation_chapter(project_root, &chapter_info.translated_path).await?);
    }
    Ok(chapters)
}

fn format_str(format: TranslationExportFormat) -> &'static str {
    match format {
        TranslationExportFormat::Txt => "txt",
        TranslationExportFormat::Md => "md",
        TranslationExportFormat::Epub => "epub",
    }
}

/// `renderTextExport`：md（# 标题 + 引言行）/ txt（纯标题行）+ 逐章逐段。
fn render_text_export(
    manifest: &TranslationProjectManifest,
    chapters: &[TranslationChapterFile],
    format: TranslationExportFormat,
) -> String {
    let mut lines: Vec<String> = Vec::new();
    let is_md = format == TranslationExportFormat::Md;
    if is_md {
        lines.push(format!("# {}", manifest.title));
        lines.push(String::new());
        lines.push(format!("> {} -> {}", manifest.source_language, manifest.target_language));
        lines.push(String::new());
    } else {
        lines.push(manifest.title.clone());
        lines.push(format!("{} -> {}", manifest.source_language, manifest.target_language));
        lines.push(String::new());
    }
    for chapter in chapters {
        lines.push(if is_md {
            format!("## {}", chapter.title)
        } else {
            chapter.title.clone()
        });
        lines.push(String::new());
        for segment in &chapter.segments {
            if let Some(target) = segment.target.as_deref().map(str::trim) {
                if !target.is_empty() {
                    lines.push(target.to_string());
                    lines.push(String::new());
                }
            }
        }
    }
    format!("{}\n", lines.join("\n").trim_end())
}

fn escape_html(text: &str) -> String {
    text.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;")
}

fn safe_filename(value: &str) -> String {
    let replaced: String = value
        .trim()
        .chars()
        .map(|c| {
            if matches!(c, '\\' | '/' | ':' | '*' | '?' | '"' | '<' | '>' | '|') {
                '-'
            } else if c.is_whitespace() {
                ' '
            } else {
                c
            }
        })
        .collect();
    let mut out = String::new();
    let mut units = 0usize;
    for ch in replaced.trim().chars() {
        let ch_units = ch.len_utf16();
        if units + ch_units > 80 {
            break;
        }
        out.push(ch);
        units += ch_units;
    }
    if out.is_empty() {
        "translation".to_string()
    } else {
        out
    }
}

/// 测试暴露面。
pub mod tests_support {
    use super::*;

    pub fn build_epub_for_test(
        manifest: &TranslationProjectManifest,
        chapters: &[TranslationChapterFile],
    ) -> Result<Vec<u8>, String> {
        build_epub(manifest, chapters)
    }
}

/// 最小合法 EPUB（mimetype STORED + container + OPF + NCX + 章节 XHTML）。
fn build_epub(
    manifest: &TranslationProjectManifest,
    chapters: &[TranslationChapterFile],
) -> Result<Vec<u8>, String> {
    let book_id = format!("urn:uuid:{}", uuid::Uuid::new_v4());
    let mut xhtml_files: Vec<(String, String)> = Vec::new();
    for (index, chapter) in chapters.iter().enumerate() {
        let paragraphs: Vec<String> = chapter
            .segments
            .iter()
            .filter_map(|segment| {
                segment
                    .target
                    .as_deref()
                    .map(str::trim)
                    .filter(|t| !t.is_empty())
                    .map(|text| format!("<p>{}</p>", escape_html(text)))
            })
            .collect();
        let filename = format!("chapter-{:04}.xhtml", index + 1);
        let body = format!(
            "<?xml version=\"1.0\" encoding=\"utf-8\"?>\n<!DOCTYPE html>\n<html xmlns=\"http://www.w3.org/1999/xhtml\"><head><title>{}</title></head><body><h1>{}</h1>\n{}\n</body></html>",
            escape_html(&chapter.title),
            escape_html(&chapter.title),
            paragraphs.join("\n"),
        );
        xhtml_files.push((filename, body));
    }

    let manifest_items: Vec<String> = std::iter::once(
        "    <item id=\"ncx\" href=\"toc.ncx\" media-type=\"application/x-dtbncx+xml\"/>".to_string(),
    )
    .chain(xhtml_files.iter().enumerate().map(|(index, (filename, _))| {
        format!(
            "    <item id=\"ch{num}\" href=\"{filename}\" media-type=\"application/xhtml+xml\"/>",
            num = index + 1
        )
    }))
    .collect();
    let spine_items: Vec<String> = (1..=xhtml_files.len())
        .map(|num| format!("    <itemref idref=\"ch{num}\"/>"))
        .collect();
    let opf = format!(
        "<?xml version=\"1.0\" encoding=\"utf-8\"?>\n<package xmlns=\"http://www.idpf.org/2007/opf\" version=\"2.0\" unique-identifier=\"book-id\">\n  <metadata xmlns:dc=\"http://purl.org/dc/elements/1.1/\">\n    <dc:identifier id=\"book-id\">{book_id}</dc:identifier>\n    <dc:title>{title}</dc:title>\n    <dc:language>{lang}</dc:language>\n  </metadata>\n  <manifest>\n{items}\n  </manifest>\n  <spine toc=\"ncx\">\n{spine}\n  </spine>\n</package>\n",
        title = escape_html(&manifest.title),
        lang = manifest.target_language,
        items = manifest_items.join("\n"),
        spine = spine_items.join("\n"),
    );

    let nav_points: Vec<String> = xhtml_files
        .iter()
        .enumerate()
        .map(|(index, (filename, _))| {
            format!(
                "    <navPoint id=\"nav{num}\" playOrder=\"{num}\"><navLabel><text>Chapter {num}</text></navLabel><content src=\"{filename}\"/></navPoint>",
                num = index + 1
            )
        })
        .collect();
    let ncx = format!(
        "<?xml version=\"1.0\" encoding=\"utf-8\"?>\n<ncx xmlns=\"http://www.daisy.org/z3986/2005/ncx/\" version=\"2005-1\">\n  <head><meta name=\"dtb:uid\" content=\"{book_id}\"/></head>\n  <docTitle><text>{title}</text></docTitle>\n  <navMap>\n{nav}\n  </navMap>\n</ncx>\n",
        title = escape_html(&manifest.title),
        nav = nav_points.join("\n"),
    );
    let container = "<?xml version=\"1.0\" encoding=\"utf-8\"?>\n<container version=\"1.0\" xmlns=\"urn:oasis:names:tc:opendocument:xmlns:container\">\n  <rootfiles><rootfile full-path=\"OEBPS/content.opf\" media-type=\"application/oebps-package+xml\"/></rootfiles>\n</container>\n";

    let mut buffer = std::io::Cursor::new(Vec::new());
    {
        let mut writer = zip::ZipWriter::new(&mut buffer);
        let stored: zip::write::FileOptions<'_, ()> =
            zip::write::FileOptions::default().compression_method(zip::CompressionMethod::Stored);
        let deflated: zip::write::FileOptions<'_, ()> =
            zip::write::FileOptions::default().compression_method(zip::CompressionMethod::Deflated);
        let mut push = |name: &str, options: zip::write::FileOptions<'_, ()>, body: &[u8]| -> Result<(), String> {
            writer
                .start_file(name, options)
                .map_err(|e| e.to_string())?;
            writer.write_all(body).map_err(|e| e.to_string())
        };
        push("mimetype", stored, b"application/epub+zip")?;
        push("META-INF/container.xml", deflated, container.as_bytes())?;
        push("OEBPS/content.opf", deflated, opf.as_bytes())?;
        push("OEBPS/toc.ncx", deflated, ncx.as_bytes())?;
        for (filename, body) in &xhtml_files {
            push(&format!("OEBPS/{filename}"), deflated, body.as_bytes())?;
        }
        writer.finish().map_err(|e| e.to_string())?;
    }
    Ok(buffer.into_inner())
}
