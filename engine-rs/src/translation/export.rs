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
            crate::utils::atomic_file_set::write_file_atomic_bytes(&output_path, &bytes).await.map_err(|e| e.to_string())?;
        }
        format => {
            let rendered = render_text_export(&manifest, &chapters, format);
            crate::utils::atomic_file_set::write_file_atomic(&output_path, &rendered).await.map_err(|e| e.to_string())?;
        }
    }

    Ok(TranslationExportResult {
        output_path: to_posix_path(project_root, &output_path),
        format,
        chapters_exported: manifest.chapters.len(),
        export_source_paths: manifest.chapters.iter().map(|chapter| chapter.translated_path.clone()).collect(),
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
    // 945 号：补引号转义（防御纵深——未来模板改动将用户内容置入属性值
    // 时引号已在转义集内）。标题/navLabel/OPF 元数据面沿用（TS 参考产物
    // 中 CJK 原样保留）。
    text.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&apos;")
}

/// 974 号：epub-gen-memory 内容面引用策略——非 ASCII 数值字符引用
/// （`&#xNNNN;` 大写十六进制，ASCII 原样）；仅段落 `<p>` 内容使用
///（参考产物 0_.xhtml 实证；标题/navLabel 保留原样走 escape_html）。
fn escape_numeric(text: &str) -> String {
    let mut numeric = String::with_capacity(text.len() * 2);
    for ch in text.chars() {
        if ch.is_ascii() {
            numeric.push(ch);
        } else {
            numeric.push_str(&format!("&#x{:X};", ch as u32));
        }
    }
    numeric
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

/// 974 号：EPUB 生成对齐 TS `epub-gen-memory@1.1.2` 产物结构（参考产物解包逐字节
/// 勘察后移植）：EPUB 3（OPF version="3.0"）+ `toc.xhtml` nav + NCX（含目录
/// navPoint）+ `style.css` + 章节文件 `0_.xhtml`/`1_.xhtml`（basename 序）+
/// `META-INF/container.xml`（TS 单行紧凑形）+ `mimetype` STORED。
/// 不变量：uuid/时间戳非确定（TS 同为运行时生成）；其余面（模板串、转义策略、
/// navLabel "N. " 前缀、目录 navPoint、style.css 全文）与参考产物逐字节一致——
/// 由 `tests::epub_matches_ts_reference_structure` golden 解包对比锁。
fn build_epub(
    manifest: &TranslationProjectManifest,
    chapters: &[TranslationChapterFile],
) -> Result<Vec<u8>, String> {
    let book_id = uuid::Uuid::new_v4().to_string();
    // TS `<%= (new Date()).toISOString().split(".")[0]+"Z" %>` / `<%= date %>`：
    // modified=秒级 ISO，date=毫秒级 ISO（均运行时生成）。
    let now_millis = crate::utils::utc_time::utc_now_iso();
    let modified = now_millis.split('.').next().map(|s| format!("{s}Z")).unwrap_or_default();

    // 章节文件名（TS `content.filename` = `${index}_.xhtml`）。
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
                    .map(|text| format!("<p>{}</p>", escape_numeric(&escape_html(text))))
            })
            .collect();
        // TS chapter.xhtml 模板逐字节：head（charset/title/style.css link）+
        // body（prependChapterTitles 默认 true → h1；author/url 空 → 不产 p）+
        // `<%- content %>`（原样注入，escape_html 已在段构造时做）。
        let body = format!(
            "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<!DOCTYPE html>\n<html xmlns=\"http://www.w3.org/1999/xhtml\" xmlns:epub=\"http://www.idpf.org/2007/ops\" xml:lang=\"{lang}\" lang=\"{lang}\">\n<head>\n  <meta charset=\"UTF-8\" />\n  <title>{title}</title>\n  <link rel=\"stylesheet\" type=\"text/css\" href=\"style.css\" />\n</head>\n<body>\n  \n    <h1>{title}</h1>\n    \n    \n  \n{paragraphs}\n</body>\n</html>",
            lang = escape_html(&manifest.target_language),
            title = escape_html(&chapter.title),
            paragraphs = paragraphs.join("\n"),
        );
        let filename = format!("{index}_.xhtml");
        xhtml_files.push((filename, body));
    }

    // TS style.css 全文（template.css 逐字）。
    let style_css = ".epub-author {\n    color: #555;\n}\n\n.epub-link {\n    margin-bottom: 30px;\n}\n\n.epub-link a {\n    color: #666;\n    font-size: 90%;\n}\n\n.toc-author {\n    font-size: 90%;\n    color: #555;\n}\n\n.toc-link {\n    color: #999;\n    font-size: 85%;\n    display: block;\n}\n\nhr {\n    border: 0;\n    border-bottom: 1px solid #dedede;\n    margin: 60px 10%;\n}";

    // TS content.opf 模板逐字节（metadata 块含 identifier-type 22/dcterms 反射面/
    // anonymous creator·publisher/rights/generator/ibooks；manifest/spine 的
    // ejs 循环空行与 toc 前置 itemref 按参考产物形态固化）。
    let mut content_items = String::new();
    let mut content_spine = String::new();
    for (index, (filename, _)) in xhtml_files.iter().enumerate() {
        content_items.push_str(&format!(
            "        <item id=\"content_{index}_item_{index}\" href=\"{filename}\" media-type=\"application/xhtml+xml\" />\n        \n"
        ));
        content_spine.push_str(&format!(
            "        \n            \n                <itemref idref=\"content_{index}_item_{index}\"/>\n            \n        \n"
        ));
    }
    let opf = format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<package xmlns=\"http://www.idpf.org/2007/opf\"\n         version=\"3.0\"\n         unique-identifier=\"BookId\"\n         xmlns:dc=\"http://purl.org/dc/elements/1.1/\"\n         xmlns:dcterms=\"http://purl.org/dc/terms/\"\n         xml:lang=\"en\"\n         xmlns:media=\"http://www.idpf.org/epub/vocab/overlays/#\"\n         prefix=\"ibooks: http://vocabulary.itunes.apple.com/rdf/ibooks/vocabulary-extensions-1.0/\">\n\n    <metadata xmlns:dc=\"http://purl.org/dc/elements/1.1/\"\n              xmlns:opf=\"http://www.idpf.org/2007/opf\">\n\n        <dc:identifier id=\"BookId\">{book_id}</dc:identifier>\n        <meta refines=\"#BookId\" property=\"identifier-type\" scheme=\"onix:codelist5\">22</meta>\n        <meta property=\"dcterms:identifier\" id=\"meta-identifier\">BookId</meta>\n        <dc:title>{title}</dc:title>\n        <meta property=\"dcterms:title\" id=\"meta-title\">{title}</meta>\n        <dc:description></dc:description>\n        <dc:language>{lang}</dc:language>\n        <meta property=\"dcterms:language\" id=\"meta-language\">{lang}</meta>\n        <meta property=\"dcterms:modified\">{modified}</meta>\n        <dc:creator id=\"creator\">anonymous</dc:creator>\n        <meta refines=\"#creator\" property=\"file-as\">anonymous</meta>\n        <meta property=\"dcterms:publisher\">anonymous</meta>\n        <dc:publisher>anonymous</dc:publisher>\n        <meta property=\"dcterms:date\">{now_millis}</meta>\n        <dc:date>{now_millis}</dc:date>\n        <meta property=\"dcterms:rights\">All rights reserved</meta>\n        <dc:rights>Copyright &#x00A9; {year} by anonymous</dc:rights>\n        \n        <meta name=\"generator\" content=\"epub-gen\" />\n        <meta property=\"ibooks:specified-fonts\">true</meta>\n\n    </metadata>\n\n    <manifest>\n        <item id=\"ncx\" href=\"toc.ncx\" media-type=\"application/x-dtbncx+xml\" />\n        <item id=\"toc\" href=\"toc.xhtml\" media-type=\"application/xhtml+xml\" properties=\"nav\" />\n        <item id=\"css\" href=\"style.css\" media-type=\"text/css\" />\n\n        \n        {content_items}        \n\n        \n    </manifest>\n\n    <spine toc=\"ncx\">\n        \n            \n        \n            \n        \n        <itemref idref=\"toc\" />\n        {content_spine}    </spine>\n    <guide>\n        <reference type=\"text\" title=\"Table of Content\" href=\"toc.xhtml\"/>\n    </guide>\n</package>",
        title = escape_html(&manifest.title),
        lang = escape_html(&manifest.target_language),
        modified = modified,
        now_millis = now_millis,
        year = now_millis.get(..4).unwrap_or_default(),
        content_items = content_items,
        content_spine = content_spine,
    );

    // TS toc.xhtml 模板逐字节（nav id=toc epub:type=toc；li class=table-of-content；
    // a 文本=章节标题原文 + 空白行；ol 嵌套按参考产物形态固化）。
    let mut toc_items = String::new();
    for (filename, body) in &xhtml_files {
        let title = chapter_title_of(body);
        toc_items.push_str(&format!(
            "        \n            \n                \n                    <li class=\"table-of-content\">\n                        <a href=\"{filename}\">{title}\n                            \n                        </a>\n                        \n                    </li>\n                \n"
        ));
    }
    let toc_xhtml = format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<!DOCTYPE html>\n<html xmlns=\"http://www.w3.org/1999/xhtml\" xmlns:epub=\"http://www.idpf.org/2007/ops\" xml:lang=\"{lang}\" lang=\"{lang}\">\n<head>\n    <title>{book_title}</title>\n    <meta charset=\"UTF-8\" />\n    <link rel=\"stylesheet\" type=\"text/css\" href=\"style.css\" />\n</head>\n<body>\n    <h1 class=\"h1\">Table of Contents</h1>\n    <nav id=\"toc\" epub:type=\"toc\">\n        \n            <ol>\n{toc_items}        \n        </ol>\n    </nav>\n</body>\n</html>",
        lang = escape_html(&manifest.target_language),
        book_title = escape_html(&manifest.title),
        toc_items = toc_items,
    );

    // TS toc.ncx 模板逐字节（head 五 meta + docTitle/docAuthor=anonymous +
    // 目录 navPoint playOrder=0 + 章节 navPoint "N. 标题"）。
    let mut nav_points = String::new();
    nav_points.push_str("        \n        \n            \n        \n            \n        \n        \n        \n            <navPoint id=\"toc\" playOrder=\"0\" class=\"chapter\">\n                <navLabel>\n                    <text>Table of Contents</text>\n                </navLabel>\n                <content src=\"toc.xhtml\"/>\n            </navPoint>\n");
    for (index, (filename, body)) in xhtml_files.iter().enumerate() {
        let title = chapter_title_of(body);
        let num = index + 1;
        nav_points.push_str(&format!(
            "        \n        \n            \n                <navPoint id=\"content_{index}_item_{index}\" playOrder=\"{num}\" class=\"chapter\">\n                    <navLabel>\n                        <text>{num}. {title}</text>\n                    </navLabel>\n                    <content src=\"{filename}\"/>\n                </navPoint>\n            \n"
        ));
    }
    let ncx = format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<ncx xmlns=\"http://www.daisy.org/z3986/2005/ncx/\" version=\"2005-1\">\n    <head>\n        <meta name=\"dtb:uid\" content=\"{book_id}\" />\n        <meta name=\"dtb:generator\" content=\"epub-gen\"/>\n        <meta name=\"dtb:depth\" content=\"1\"/>\n        <meta name=\"dtb:totalPageCount\" content=\"0\"/>\n        <meta name=\"dtb:maxPageNumber\" content=\"0\"/>\n    </head>\n    <docTitle>\n        <text>{title}</text>\n    </docTitle>\n    <docAuthor>\n        <text>anonymous</text>\n    </docAuthor>\n    <navMap>\n{nav_points}    </navMap>\n</ncx>",
        book_id = book_id,
        title = escape_html(&manifest.title),
        nav_points = nav_points,
    );

    // TS container.xml 单行紧凑形（无尾换行）。
    let container = "<?xml version=\"1.0\" encoding=\"UTF-8\" ?><container version=\"1.0\" xmlns=\"urn:oasis:names:tc:opendocument:xmlns:container\"><rootfiles><rootfile full-path=\"OEBPS/content.opf\" media-type=\"application/oebps-package+xml\"/></rootfiles></container>";

    let mut buffer = std::io::Cursor::new(Vec::new());
    {
        let mut writer = zip::ZipWriter::new(&mut buffer);
        // 846 号：zip 9 的 FileOptions 多一个生命周期参数——改用跨版本稳定的 SimpleFileOptions 别名。
        let stored: zip::write::SimpleFileOptions =
            zip::write::SimpleFileOptions::default().compression_method(zip::CompressionMethod::Stored);
        let deflated: zip::write::SimpleFileOptions =
            zip::write::SimpleFileOptions::default().compression_method(zip::CompressionMethod::Deflated);
        let mut push = |name: &str, options: zip::write::SimpleFileOptions, body: &[u8]| -> Result<(), String> {
            writer
                .start_file(name, options)
                .map_err(|e| e.to_string())?;
            writer.write_all(body).map_err(|e| e.to_string())
        };
        push("mimetype", stored, b"application/epub+zip")?;
        push("OEBPS/style.css", deflated, style_css.as_bytes())?;
        for (filename, body) in &xhtml_files {
            push(&format!("OEBPS/{filename}"), deflated, body.as_bytes())?;
        }
        push("META-INF/container.xml", deflated, container.as_bytes())?;
        push("OEBPS/content.opf", deflated, opf.as_bytes())?;
        push("OEBPS/toc.ncx", deflated, ncx.as_bytes())?;
        push("OEBPS/toc.xhtml", deflated, toc_xhtml.as_bytes())?;
        writer.finish().map_err(|e| e.to_string())?;
    }
    Ok(buffer.into_inner())
}

/// 章节标题自产物 xhtml 的 `<title>` 行提取（navLabel/toc 链接共用）。
fn chapter_title_of(xhtml: &str) -> String {
    xhtml
        .split("<title>")
        .nth(1)
        .and_then(|rest| rest.split("</title>").next())
        .unwrap_or_default()
        .to_string()
}

#[cfg(test)]
mod golden_tests {
    use super::*;

    /// 974 号：TS 参考产物（`epub-gen-memory@1.1.2` 以同输入生成，fixture 固化）。
    const TS_REFERENCE: &[u8] = include_bytes!("../../tests/fixtures/ts-epub-reference.epub");

    struct Unpacked {
        names: Vec<String>,
        files: std::collections::BTreeMap<String, Vec<u8>>,
    }

    fn unpack(bytes: &[u8]) -> Unpacked {
        let mut reader = zip::ZipArchive::new(std::io::Cursor::new(bytes)).unwrap();
        let mut names = Vec::new();
        let mut files = std::collections::BTreeMap::new();
        for index in 0..reader.len() {
            let mut entry = reader.by_index(index).unwrap();
            let name = entry.name().unwrap().to_string();
            names.push(name.clone());
            let mut body = Vec::new();
            std::io::Read::read_to_end(&mut entry, &mut body).unwrap();
            files.insert(name, body);
        }
        Unpacked { names, files }
    }

    /// 时间戳/uuid/年份非确定面归一（TS modified=秒级 ISO；date=毫秒级 ISO；uuid v4）。
    fn normalize(text: &str) -> String {
        let uuid_re = regex::Regex::new(r"[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}").unwrap();
        let millis_re = regex::Regex::new(r"\d{4}-\d{2}-\d{2}T\d{2}:\d{2}:\d{2}\.\d{3}Z").unwrap();
        let secs_re = regex::Regex::new(r"\d{4}-\d{2}-\d{2}T\d{2}:\d{2}:\d{2}Z").unwrap();
        let year_re = regex::Regex::new(r"Copyright &#x00A9; \d{4} by").unwrap();
        let step1 = uuid_re.replace_all(text, "UUID").to_string();
        let step2 = millis_re.replace_all(&step1, "TIMESTAMP_MILLIS").to_string();
        let step3 = secs_re.replace_all(&step2, "TIMESTAMP_SECS").to_string();
        year_re.replace_all(&step3, "Copyright &#x00A9; YEAR by").to_string()
    }

    /// golden 结构对比：容器逐字节一致、OPF 骨架行与 TS 对齐、NCX navPoint 形、
    /// 章节 xhtml 头模板逐字节 + 段落 CJK 数字引用策略。
    #[test]
    fn epub_matches_ts_reference_structure() {
        let manifest = TranslationProjectManifest {
            id: "t".into(),
            title: "山河之书".into(),
            source_language: "en".into(),
            target_language: "zh".into(),
            created_at: "2026-01-01T00:00:00.000Z".into(),
            updated_at: "2026-01-01T00:00:00.000Z".into(),
            source: TranslationSourceManifest {
                kind: TranslationSourceKind::Text,
                path: "in.txt".into(),
                char_count: 10,
                total_pages: None,
            },
            // TS 参考产物 = 2 章输入（manifest 两章条目）。
            chapters: vec![
                TranslationChapterManifest {
                    number: 1,
                    title: "第一章 风起".into(),
                    source_path: "source/chapter-0001.json".into(),
                    translated_path: "translated/chapter-0001.json".into(),
                    segment_count: 2,
                    char_count: 12,
                    status: TranslationChapterStatus::Pending,
                    translated_segments: 0,
                    review_summary: None,
                    observations: None,
                },
                TranslationChapterManifest {
                    number: 2,
                    title: "第二章 云涌".into(),
                    source_path: "source/chapter-0002.json".into(),
                    translated_path: "translated/chapter-0002.json".into(),
                    segment_count: 1,
                    char_count: 6,
                    status: TranslationChapterStatus::Pending,
                    translated_segments: 0,
                    review_summary: None,
                    observations: None,
                },
            ],
        };
        let chapters = vec![
            TranslationChapterFile {
                number: 1,
                title: "第一章 风起".into(),
                source_language: "en".into(),
                target_language: "zh".into(),
                segments: vec![
                    TranslationSegment { index: 1, source: "a".into(), target: Some("林动握紧了拳。".into()), notes: None },
                    TranslationSegment { index: 2, source: "b".into(), target: Some("灵气涌动。".into()), notes: None },
                ],
            },
            TranslationChapterFile {
                number: 2,
                title: "第二章 云涌".into(),
                source_language: "en".into(),
                target_language: "zh".into(),
                segments: vec![TranslationSegment { index: 1, source: "c".into(), target: Some("天南大陆。".into()), notes: None }],
            },
        ];
        let rust = build_epub(&manifest, &chapters).unwrap();

        let ts = unpack(TS_REFERENCE);
        let rust_unpacked = unpack(&rust);
        // 文件名集：双章输入 → 两章节文件 + toc.xhtml（与 TS 参考 3 个 xhtml 同数）。
        assert_eq!(
            rust_unpacked.names.iter().filter(|n| n.ends_with(".xhtml")).count(),
            3,
            "双章输入 → 两章节文件 + toc.xhtml：{:?}",
            rust_unpacked.names
        );
        for name in ["mimetype", "META-INF/container.xml", "OEBPS/style.css", "OEBPS/content.opf", "OEBPS/toc.ncx", "OEBPS/toc.xhtml"] {
            assert!(rust_unpacked.files.contains_key(name), "{name} 在场");
            assert!(ts.files.contains_key(name), "TS 参考 {name} 在场");
        }
        // mimetype：STORED 原样（TS 同）；container.xml：逐字节一致（TS 单行紧凑形）；
        // style.css：逐字节一致（模板全文）。
        assert_eq!(rust_unpacked.files["mimetype"], ts.files["mimetype"]);
        assert_eq!(
            String::from_utf8(rust_unpacked.files["META-INF/container.xml"].clone()).unwrap(),
            String::from_utf8(ts.files["META-INF/container.xml"].clone()).unwrap()
        );
        assert_eq!(
            String::from_utf8(rust_unpacked.files["OEBPS/style.css"].clone()).unwrap(),
            String::from_utf8(ts.files["OEBPS/style.css"].clone()).unwrap()
        );
        // OPF：归一化（uuid/时间戳/年份）后骨架行与 TS 对齐；章节条目 1 条（单章输入）。
        let rust_opf = normalize(&String::from_utf8(rust_unpacked.files["OEBPS/content.opf"].clone()).unwrap());
        let ts_opf = normalize(&String::from_utf8(ts.files["OEBPS/content.opf"].clone()).unwrap());
        assert!(rust_opf.contains("version=\"3.0\""), "EPUB 3 OPF：{rust_opf}");
        assert!(rust_opf.contains("unique-identifier=\"BookId\""));
        assert!(rust_opf.contains("properties=\"nav\""));
        assert!(rust_opf.contains("id=\"content_0_item_0\""));
        assert!(rust_opf.contains("content_1_item_1"), "双章输入两条目");
        let skeleton = |opf: &str| -> Vec<String> {
            opf.lines()
                .map(|line| {
                    line.trim()
                        .replace("content_0_item_0", "CONTENT_ITEM_A")
                        .replace("content_1_item_1", "CONTENT_ITEM_B")
                        .replace("0_.xhtml", "CHAPTER_FILE_A")
                        .replace("1_.xhtml", "CHAPTER_FILE_B")
                })
                .filter(|line| !line.is_empty())
                .collect()
        };
        assert_eq!(skeleton(&rust_opf), skeleton(&ts_opf), "OPF 骨架行与 TS 对齐");

        // NCX：归一化后 navPoint 序（目录 + 章节）与 head 五 meta/docAuthor 形一致。
        let rust_ncx = normalize(&String::from_utf8(rust_unpacked.files["OEBPS/toc.ncx"].clone()).unwrap());
        let ts_ncx = normalize(&String::from_utf8(ts.files["OEBPS/toc.ncx"].clone()).unwrap());
        assert!(rust_ncx.contains("dtb:uid"), "ncx head 五 meta");
        assert!(rust_ncx.contains("dtb:generator\" content=\"epub-gen\""));
        assert!(rust_ncx.contains("playOrder=\"0\""), "目录 navPoint");
        assert!(rust_ncx.contains("<text>1. "), "章节 navLabel N. 前缀");
        assert!(rust_ncx.contains("<docAuthor>"), "docAuthor anonymous 面");
        let ts_nav_count = ts_ncx.matches("<navPoint").count();
        let rust_nav_count = rust_ncx.matches("<navPoint").count();
        assert_eq!(rust_nav_count, ts_nav_count, "双章输入 navPoint 数与 TS 参考一致（目录+2）");

        // 章节 xhtml：头模板逐字节（title 内容不同）+ 段落 CJK 数字引用 + h1 原样。
        let rust_chapter = String::from_utf8(rust_unpacked.files["OEBPS/0_.xhtml"].clone()).unwrap();
        let ts_chapter = String::from_utf8(ts.files["OEBPS/0_.xhtml"].clone()).unwrap();
        let chapter_head = |xhtml: &str| -> String {
            xhtml.split("<body>").next().unwrap().to_string().replace("第一章 风起", "TITLE")
        };
        assert_eq!(chapter_head(&rust_chapter), chapter_head(&ts_chapter), "章节头模板逐字节");
        assert!(rust_chapter.contains("&#x6797;"), "段落 CJK 数字引用：{rust_chapter}");
        assert!(rust_chapter.contains("<h1>第一章 风起</h1>"), "h1 保留原样");
    }
}
