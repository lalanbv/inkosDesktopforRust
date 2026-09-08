//! materials 域（83 号）：材料归档（ingest）与召回（retrieve）。
//!
//! 移植自 `packages/core/src/materials/ingest.ts` + `retrieve.ts`：
//! - `ingestMaterial`：url（HTTP 抓取）/ file（项目内路径）→ HTML/text 归一 →
//!   `.inkos/materials/{id}.md`（Markdown 卡）+ `{id}.json`（manifest）
//! - `retrieveMaterials`：manifest 列表 → 词项打分（title/source/body 权重 +
//!   首现位置）→ 半径 700 片段 → 限幅排序
//!
//! PDF 文本抽取经 pdf-extract（100 号闭合 83 号备案；空文本判定对齐
//! TS 的"扫描件需 OCR"文案）。

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

const MAX_SOURCE_BYTES: usize = 18 * 1024 * 1024;
const EXCERPT_CHARS: usize = 1600;

pub const PURPOSES: &[&str] = &["reference", "worldbuilding", "script", "storyboard", "research", "general"];

#[derive(Debug, Clone)]
pub struct IngestMaterialInput<'a> {
    pub source_kind: &'a str,
    pub url: Option<&'a str>,
    pub file_path: Option<&'a str>,
    pub filename: Option<&'a str>,
    pub mime_type: Option<&'a str>,
    pub title: Option<&'a str>,
    pub purpose: Option<&'a str>,
}

/// `MaterialAsset`（manifest JSON 形态，camelCase）。
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MaterialAsset {
    pub id: String,
    pub title: String,
    pub kind: String,
    pub purpose: String,
    pub source: String,
    pub mime_type: String,
    pub markdown_path: String,
    pub manifest_path: String,
    pub char_count: usize,
    pub excerpt: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub total_pages: Option<u32>,
}

#[derive(Debug, Clone)]
pub struct RetrieveMaterialsInput {
    pub query: String,
    pub purpose: Option<String>,
    pub limit: Option<f64>,
}

/// `RetrievedMaterial`。
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RetrievedMaterial {
    pub id: String,
    pub title: String,
    pub kind: String,
    pub purpose: String,
    pub source: String,
    pub markdown_path: String,
    pub score: f64,
    pub excerpt: String,
    pub char_start: usize,
    pub char_end: usize,
}

struct MaterialSource {
    kind: &'static str,
    source: String,
    title: Option<String>,
    mime_type: String,
    text: String,
    total_pages: Option<u32>,
}

/// `ingestMaterial`：抽取 → Markdown 卡 + manifest 落盘。
pub async fn ingest_material(
    project_root: &Path,
    input: &IngestMaterialInput<'_>,
) -> Result<MaterialAsset, String> {
    let purpose = input.purpose.filter(|p| PURPOSES.contains(p)).unwrap_or("reference");
    let source = read_material_source(project_root, input).await?;
    let title: String = input
        .title
        .map(str::trim)
        .filter(|t| !t.is_empty())
        .map(str::to_string)
        .or_else(|| source.title.clone())
        .or_else(|| title_from_source(input))
        .unwrap_or_else(|| "material".to_string());
    let title = truncate_utf16(&title, 120);
    let id = format!("{}-{}", crate::utils::utc_time::utc_now_iso().replace([':', '.'], "-"), slug(&title));
    let materials_dir = project_root.join(".inkos").join("materials");
    tokio::fs::create_dir_all(&materials_dir)
        .await
        .map_err(|e| e.to_string())?;

    let markdown = render_material_markdown(
        &title,
        source.kind,
        purpose,
        &source.source,
        &source.mime_type,
        &source.text,
        source.total_pages,
    );
    let markdown_path_abs = materials_dir.join(format!("{id}.md"));
    let manifest_path_abs = materials_dir.join(format!("{id}.json"));
    tokio::fs::write(&markdown_path_abs, &markdown)
        .await
        .map_err(|e| e.to_string())?;
    let asset = MaterialAsset {
        id,
        title: title.clone(),
        kind: source.kind.to_string(),
        purpose: purpose.to_string(),
        source: source.source.clone(),
        mime_type: source.mime_type.clone(),
        markdown_path: to_posix_relative(project_root, &markdown_path_abs),
        manifest_path: to_posix_relative(project_root, &manifest_path_abs),
        char_count: source.text.chars().count(),
        excerpt: truncate_utf16(&source.text, EXCERPT_CHARS),
        total_pages: source.total_pages,
    };
    // TS `JSON.stringify(asset, null, 2)`：无尾换行。
    let manifest = serde_json::to_string_pretty(&asset).unwrap_or_default();
    tokio::fs::write(&manifest_path_abs, manifest)
        .await
        .map_err(|e| e.to_string())?;
    Ok(asset)
}

async fn read_material_source(project_root: &Path, input: &IngestMaterialInput<'_>) -> Result<MaterialSource, String> {
    if input.source_kind == "url" {
        let Some(url) = input.url.filter(|u| !u.is_empty()) else {
            return Err("ingest_material.url is required for URL sources.".to_string());
        };
        return read_url_material(url).await;
    }
    let Some(file_path) = input.file_path.filter(|p| !p.is_empty()) else {
        return Err("ingest_material.filePath is required for file sources.".to_string());
    };
    let safe_path = safe_child_path(project_root, file_path)?;
    let buffer = tokio::fs::read(&safe_path)
        .await
        .map_err(|e| format!("read failed: {e}"))?;
    if buffer.len() > MAX_SOURCE_BYTES {
        return Err(format!("Material file is too large ({} bytes).", buffer.len()));
    }
    let filename = input
        .filename
        .filter(|f| !f.is_empty())
        .map(String::from)
        .unwrap_or_else(|| {
            safe_path
                .file_name()
                .map(|n| n.to_string_lossy().to_string())
                .unwrap_or_default()
        });
    let mime_type = input
        .mime_type
        .filter(|m| !m.is_empty())
        .map(String::from)
        .unwrap_or_else(|| mime_from_filename(&filename));
    extract_buffer_material(
        &buffer,
        &to_posix_relative(project_root, &safe_path),
        &filename,
        &mime_type,
    )
}

async fn read_url_material(url: &str) -> Result<MaterialSource, String> {
    let parsed = url::Url::parse(url).map_err(|_| format!("Unsupported URL protocol: {url}"))?;
    if !matches!(parsed.scheme(), "http" | "https") {
        return Err(format!("Unsupported URL protocol: {}", parsed.scheme()));
    }
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(20))
        .no_proxy()
        .build()
        .map_err(|e| e.to_string())?;
    let response = client
        .get(url)
        .header("User-Agent", "InkOS/1.6 material-ingestion")
        .header("Accept", "text/html, text/plain, application/json, application/pdf, */*")
        .send()
        .await
        .map_err(|e| format!("Fetch failed: {e}"))?;
    if !response.status().is_success() {
        return Err(format!(
            "Fetch failed: {} {}",
            response.status().as_u16(),
            response.status().canonical_reason().unwrap_or("")
        ));
    }
    let mime_type = response
        .headers()
        .get("content-type")
        .and_then(|v| v.to_str().ok())
        .map(|v| v.split(';').next().unwrap_or("").trim().to_string())
        .filter(|v| !v.is_empty())
        .unwrap_or_else(|| mime_from_filename(parsed.path()));
    let buffer = response.bytes().await.map_err(|e| format!("Fetch failed: {e}"))?;
    if buffer.len() > MAX_SOURCE_BYTES {
        return Err(format!("Fetched material is too large ({} bytes).", buffer.len()));
    }
    let filename = parsed
        .path()
        .rsplit('/')
        .next()
        .filter(|s| !s.is_empty())
        .map(String::from)
        .unwrap_or_else(|| parsed.host_str().unwrap_or("material").to_string());
    extract_buffer_material(&buffer, url, &filename, &mime_type)
}

fn extract_buffer_material(
    buffer: &[u8],
    source: &str,
    filename: &str,
    mime_type: &str,
) -> Result<MaterialSource, String> {
    let mime_type = if mime_type.is_empty() {
        mime_from_filename(filename)
    } else {
        mime_type.to_string()
    };
    if is_pdf(filename, &mime_type) {
        // pdf-extract：常规文本 PDF（含 ToUnicode CMap）；页数经首层解析。
        // 空文本 → TS 逐字错误（扫描件需 OCR）。
        let text = pdf_extract::extract_text_from_mem(buffer)
            .map_err(|e| format!("PDF text extraction failed: {e}"))?;
        let text = normalize_text(&text);
        if text.is_empty() {
            return Err(
                "PDF text extraction returned no text. Scanned PDFs require OCR and are not supported yet."
                    .to_string(),
            );
        }
        let total_pages = pdf_total_pages(buffer);
        return Ok(MaterialSource {
            kind: "pdf",
            source: source.to_string(),
            title: Some(strip_extension(filename)),
            mime_type: "application/pdf".to_string(),
            text,
            total_pages,
        });
    }
    let Ok(raw) = String::from_utf8(buffer.to_vec()) else {
        return Err(format!("Unsupported material type: {mime_type}"));
    };
    if is_html(filename, &mime_type) {
        return Ok(MaterialSource {
            kind: "webpage",
            source: source.to_string(),
            title: extract_html_title(&raw).or_else(|| Some(strip_extension(filename))),
            mime_type,
            text: normalize_text(&html_to_text(&raw)),
            total_pages: None,
        });
    }
    if is_text_like(filename, &mime_type) {
        return Ok(MaterialSource {
            kind: "text",
            source: source.to_string(),
            title: Some(strip_extension(filename)),
            mime_type,
            text: normalize_text(&raw),
            total_pages: None,
        });
    }
    Err(format!(
        "Unsupported material type: {}",
        if mime_type.is_empty() { filename.to_string() } else { mime_type }
    ))
}

/// `renderMaterialMarkdown`（空行全滤 + join("\n")，TS 逐字）。
fn render_material_markdown(
    title: &str,
    kind: &str,
    purpose: &str,
    source: &str,
    mime_type: &str,
    text: &str,
    total_pages: Option<u32>,
) -> String {
    let mut lines: Vec<String> = vec![
        format!("# {title}"),
        "## Metadata".to_string(),
        format!("- kind: {kind}"),
        format!("- purpose: {purpose}"),
        format!("- source: {source}"),
        format!("- mime_type: {mime_type}"),
    ];
    if let Some(pages) = total_pages {
        lines.push(format!("- total_pages: {pages}"));
    }
    lines.push(format!("- char_count: {}", text.chars().count()));
    lines.push("## Extracted content".to_string());
    lines.push(text.to_string());
    lines.join("\n")
}

fn mime_from_filename(filename: &str) -> String {
    match extension_of(filename).as_str() {
        "pdf" => "application/pdf",
        "html" | "htm" => "text/html",
        "json" => "application/json",
        "md" | "markdown" => "text/markdown",
        "csv" => "text/csv",
        _ => "text/plain",
    }
    .to_string()
}

/// PDF 页数（lopf 文档层解析；失败省略 manifest 键）。
fn pdf_total_pages(bytes: &[u8]) -> Option<u32> {
    lopdf::Document::load_mem(bytes)
        .ok()
        .map(|doc| doc.get_pages().len() as u32)
}

fn is_pdf(filename: &str, mime_type: &str) -> bool {
    mime_type.contains("pdf") || extension_of(filename) == "pdf"
}

fn is_html(filename: &str, mime_type: &str) -> bool {
    mime_type.contains("html") || matches!(extension_of(filename).as_str(), "html" | "htm")
}

fn is_text_like(filename: &str, mime_type: &str) -> bool {
    if mime_type.starts_with("text/") {
        return true;
    }
    if mime_type.contains("json") || mime_type.contains("xml") || mime_type.contains("yaml") {
        return true;
    }
    matches!(
        extension_of(filename).as_str(),
        "txt" | "md" | "markdown" | "json" | "csv" | "tsv" | "yaml" | "yml" | "log"
    )
}

fn html_to_text(html: &str) -> String {
    static SCRIPT_STYLE: std::sync::OnceLock<regex::Regex> = std::sync::OnceLock::new();
    static TAG: std::sync::OnceLock<regex::Regex> = std::sync::OnceLock::new();
    let script_style = SCRIPT_STYLE.get_or_init(|| {
        regex::Regex::new(r"(?i)<script[\s\S]*?</script>|<style[\s\S]*?</style>").unwrap()
    });
    let tag = TAG.get_or_init(|| regex::Regex::new(r"<[^>]+>").unwrap());
    let text = script_style.replace_all(html, " ").to_string();
    tag.replace_all(&text, " ").to_string()
}

fn extract_html_title(html: &str) -> Option<String> {
    static TITLE_RE: std::sync::OnceLock<regex::Regex> = std::sync::OnceLock::new();
    let re = TITLE_RE.get_or_init(|| regex::Regex::new(r"(?is)<title[^>]*>([\s\S]*?)</title>").unwrap());
    re.captures(html)
        .and_then(|captures| captures.get(1).map(|m| decode_html(m.as_str())))
        .map(|title| title.trim().to_string())
        .filter(|title| !title.is_empty())
        .map(|title| truncate_utf16(&title, 120))
}

fn decode_html(value: &str) -> String {
    value
        .replace("&nbsp;", " ")
        .replace("&amp;", "&")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&#39;", "'")
}

fn normalize_text(value: &str) -> String {
    let decoded = decode_html(value).replace("\r\n", "\n");
    static TRAILING: std::sync::OnceLock<regex::Regex> = std::sync::OnceLock::new();
    let trailing = TRAILING.get_or_init(|| regex::Regex::new(r"[ \t]+\n").unwrap());
    let collapsed = trailing.replace_all(&decoded, "\n").to_string();
    static BLANKS: std::sync::OnceLock<regex::Regex> = std::sync::OnceLock::new();
    let blanks = BLANKS.get_or_init(|| regex::Regex::new(r"\n{3,}").unwrap());
    blanks.replace_all(&collapsed, "\n\n").trim().to_string()
}

fn strip_extension(filename: &str) -> String {
    match filename.rfind('.') {
        Some(index) if index > 0 => filename[..index].to_string(),
        _ => filename.to_string(),
    }
}

fn extension_of(filename: &str) -> String {
    match filename.rfind('.') {
        Some(index) if index > 0 => filename[index + 1..].to_lowercase(),
        _ => String::new(),
    }
}

fn title_from_source(input: &IngestMaterialInput<'_>) -> Option<String> {
    if let Some(filename) = input.filename.filter(|f| !f.is_empty()) {
        return Some(strip_extension(filename));
    }
    if let Some(file_path) = input.file_path.filter(|p| !p.is_empty()) {
        let base = file_path.rsplit(['/', '\\']).next().unwrap_or(file_path);
        return Some(strip_extension(base));
    }
    let url = input.url?;
    let Ok(parsed) = url::Url::parse(url) else {
        return None;
    };
    let base = parsed.path().rsplit('/').next().unwrap_or("");
    if !base.is_empty() {
        return Some(strip_extension(base));
    }
    parsed.host_str().map(String::from)
}

fn slug(value: &str) -> String {
    static NON_ALNUM: std::sync::OnceLock<regex::Regex> = std::sync::OnceLock::new();
    let re = NON_ALNUM.get_or_init(|| regex::Regex::new(r"[^\p{L}\p{N}]+").unwrap());
    let lowered = value.trim().to_lowercase();
    let slugified = re.replace_all(lowered.as_str(), "-");
    let trimmed = slugified.trim_matches('-').to_string();
    let truncated = truncate_utf16(&trimmed, 80);
    if truncated.is_empty() {
        "material".to_string()
    } else {
        truncated
    }
}

fn truncate_utf16(value: &str, max: usize) -> String {
    let mut units = 0usize;
    let mut out = String::new();
    for ch in value.chars() {
        let ch_units = ch.len_utf16();
        if units + ch_units > max {
            break;
        }
        units += ch_units;
        out.push(ch);
    }
    out
}

fn to_posix_relative(root: &Path, target: &Path) -> String {
    let rel = target.strip_prefix(root).unwrap_or(target);
    rel.to_string_lossy().replace('\\', "/")
}

/// `safeChildPath`：resolve(root, requested) 词法归一后必须仍在根内
/// （含等于根本身）；逃逸 → Err（TS `relative` 前缀判定语义）。
fn safe_child_path(root: &Path, requested: &str) -> Result<PathBuf, String> {
    let trimmed = requested.trim();
    if trimmed.is_empty() || trimmed.contains('\0') {
        return Err(format!("Path traversal blocked: {requested}"));
    }
    let mut lexical = root.to_path_buf();
    for part in trimmed.trim_start_matches('/').split(['/', '\\']) {
        match part {
            "" | "." => {}
            ".." => {
                lexical.pop();
            }
            other => lexical.push(other),
        }
    }
    if lexical.starts_with(root) {
        Ok(lexical)
    } else {
        Err(format!("Path traversal blocked: {requested}"))
    }
}

// ── retrieve ─────────────────────────────────────────────────────

const DEFAULT_LIMIT: usize = 5;
const MAX_LIMIT: usize = 12;

/// `retrieveMaterials`：词项打分召回。
pub async fn retrieve_materials(
    project_root: &Path,
    input: &RetrieveMaterialsInput,
) -> Vec<RetrievedMaterial> {
    // 246 号：对齐 TS retrieveMaterials——markdown 分段 + FTS5 BM25 检索
    //（.inkos/retrieval.db 持久投影）替代整文件词法评分；purpose 走 kind 过滤，
    // limit×4 候选去重后截断。
    const MATERIAL_SCOPE: &str = "archived-materials";

    let assets = list_material_assets(project_root).await;
    let mut documents: Vec<crate::utils::local_search::SearchDocument> = Vec::new();
    for asset in &assets {
        let markdown_path = match safe_child_path(project_root, &asset.markdown_path) {
            Ok(path) => path,
            Err(_) => continue,
        };
        let Ok(markdown) = tokio::fs::read_to_string(&markdown_path).await else {
            continue;
        };
        let normalized_path = asset.markdown_path.replace('\\', "/");
        let segments = crate::utils::local_search::split_markdown_for_search(&markdown);
        #[cfg(test)]
        if std::env::var("INKOS_LS_DEBUG").is_ok() {
            println!("SEG markdown chars: {}, segments: {}", markdown.chars().count(), segments.len());
        }
        for (index, segment) in segments.into_iter().enumerate()
        {
            documents.push(crate::utils::local_search::SearchDocument {
                id: format!("material:{}:{}", asset.id, index),
                scope: MATERIAL_SCOPE.to_string(),
                kind: format!("material:{}", asset.purpose),
                source: format!("{}:{}-{}", normalized_path, segment.char_start, segment.char_end),
                title: [asset.title.as_str(), segment.heading.as_str()]
                    .iter()
                    .filter(|part| !part.is_empty())
                    .cloned()
                    .collect::<Vec<_>>()
                    .join(" \u{b7} "),
                body: segment.body,
                metadata: Some(serde_json::json!({
                    "assetId": asset.id,
                    "assetTitle": asset.title,
                    "assetKind": asset.kind,
                    "purpose": asset.purpose,
                    "source": asset.source,
                    "markdownPath": normalized_path,
                    "charStart": segment.char_start,
                    "charEnd": segment.char_end,
                })),
            });
        }
    }

    let index_path = project_root.join(".inkos").join("retrieval.db");
    let Ok(index) = crate::utils::local_search::LocalSearchIndex::new(
        &index_path.to_string_lossy(),
    ) else {
        return Vec::new();
    };
    let retrieval = (|| -> rusqlite::Result<Vec<RetrievedMaterial>> {
        #[cfg(test)]
        std::env::var("INKOS_LS_DEBUG").is_ok().then(|| {
            println!(
                "RETRIEVE assets: {}, docs: {}, query: {:?}",
                assets.len(),
                documents.len(),
                input.query
            )
        });
        index.replace_scope(MATERIAL_SCOPE, &documents)?;
        let limit = normalize_limit(input.limit);
        let kinds: Vec<String> = input
            .purpose
            .as_ref()
            .map(|purpose| vec![format!("material:{purpose}")])
            .unwrap_or_default();
        let hits = index.search(
            &input.query,
            &crate::utils::local_search::SearchOptions {
                scope: MATERIAL_SCOPE,
                kinds: &kinds,
                limit: (MAX_LIMIT * 4).min(limit * 4),
            },
        );
        let mut out: Vec<RetrievedMaterial> = Vec::new();
        let mut seen: std::collections::HashSet<String> = std::collections::HashSet::new();
        for hit in &hits {
            let meta = hit
                .metadata
                .as_ref()
                .cloned()
                .unwrap_or(serde_json::Value::Null);
            let field = |name: &str| {
                meta.get(name)
                    .and_then(serde_json::Value::as_str)
                    .unwrap_or_default()
                    .to_string()
            };
            let asset_id = field("assetId");
            if asset_id.is_empty() || !seen.insert(asset_id.clone()) {
                continue;
            }
            let char_start = meta
                .get("charStart")
                .and_then(serde_json::Value::as_u64)
                .unwrap_or(0) as usize;
            let char_end = meta
                .get("charEnd")
                .and_then(serde_json::Value::as_u64)
                .unwrap_or(hit.body.chars().count() as u64)
                as usize;
            out.push(RetrievedMaterial {
                id: asset_id,
                title: field("assetTitle"),
                kind: field("assetKind"),
                purpose: field("purpose"),
                source: field("source"),
                markdown_path: field("markdownPath"),
                score: hit.score,
                excerpt: hit.body.clone(),
                char_start,
                char_end,
            });
            if out.len() >= limit {
                break;
            }
        }
        Ok(out)
    })();
    let out = match retrieval {
        Ok(out) => out,
        Err(_) => {
            index.close();
            return Vec::new();
        }
    };
    index.close();
    out
}

async fn list_material_assets(project_root: &Path) -> Vec<MaterialAsset> {
    let materials_dir = project_root.join(".inkos").join("materials");
    let Ok(mut entries) = tokio::fs::read_dir(&materials_dir).await else {
        return Vec::new();
    };
    let mut assets = Vec::new();
    while let Ok(Some(entry)) = entries.next_entry().await {
        let name = entry.file_name().to_string_lossy().to_string();
        if !name.ends_with(".json") {
            continue;
        }
        // 损坏/过期 manifest 静默跳过：召回不允许炸掉聊天轮。
        if let Ok(raw) = tokio::fs::read_to_string(entry.path()).await {
            if let Ok(asset) = serde_json::from_str::<MaterialAsset>(&raw) {
                if !asset.id.is_empty() && !asset.markdown_path.is_empty() && !asset.title.is_empty() {
                    assets.push(asset);
                }
            }
        }
    }
    assets
}

fn normalize_limit(limit: Option<f64>) -> usize {
    let Some(limit) = limit.filter(|v| v.is_finite()) else {
        return DEFAULT_LIMIT;
    };
    (limit.floor() as i64).clamp(1, MAX_LIMIT as i64) as usize
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn fixture_pdf() -> PathBuf {
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/sample.pdf")
    }

    #[tokio::test]
    async fn pdf_ingestion_extracts_text_and_pages() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().to_path_buf();
        std::fs::copy(fixture_pdf(), root.join("sample.pdf")).unwrap();
        let asset = ingest_material(
            &root,
            &IngestMaterialInput {
                source_kind: "file",
                url: None,
                file_path: Some("sample.pdf"),
                filename: Some("sample.pdf"),
                mime_type: None,
                title: None,
                purpose: Some("reference"),
            },
        )
        .await
        .unwrap();
        assert_eq!(asset.kind, "pdf");
        assert_eq!(asset.title, "sample");
        assert_eq!(asset.mime_type, "application/pdf");
        assert!(asset.excerpt.contains("Chapter one reference material."), "excerpt: {asset:?}");
        assert!(asset.char_count > 0);
        assert_eq!(asset.total_pages, Some(1));
        // 空文本（扫描件语义）：无文本流的 PDF fixture → TS 逐字错误。
        std::fs::copy(
            PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/scan.pdf"),
            root.join("scan.pdf"),
        )
        .unwrap();
        let error = ingest_material(
            &root,
            &IngestMaterialInput {
                source_kind: "file",
                url: None,
                file_path: Some("scan.pdf"),
                filename: Some("scanned.pdf"),
                mime_type: None,
                title: None,
                purpose: Some("reference"),
            },
        )
        .await
        .unwrap_err();
        assert_eq!(
            error,
            "PDF text extraction returned no text. Scanned PDFs require OCR and are not supported yet."
        );
    }

    #[test]
    fn helpers_mirror_ts() {
        assert_eq!(slug("  雪夜 古宅 Notes!! "), "雪夜-古宅-notes");
        assert_eq!(slug("!!!"), "material");
        assert_eq!(slug(&"x".repeat(100)).chars().count(), 80);
        assert_eq!(mime_from_filename("a.PDF"), "application/pdf");
        assert_eq!(mime_from_filename("a.weird"), "text/plain");
        assert!(is_text_like("a.md", "application/octet-stream"));
        assert!(!is_text_like("a.bin", "application/octet-stream"));
        assert_eq!(strip_extension("notes.md"), "notes");
        assert_eq!(extension_of("archive.tar.gz"), "gz");
        // html 抽取：script/style 剥离 + 标签剥离 + 实体解码。
        let html = r#"<html><head><title>冷库 &amp; 账页</title><style>x{}</style></head><body><script>bad()</script><p>第&nbsp;一行</p><div>第二行</div></body></html>"#;
        assert_eq!(extract_html_title(html).as_deref(), Some("冷库 & 账页"));
        // htmlToText 只剥 script/style 与标签（title 文本保留、实体不解码）；
        // 实体解码在 normalizeText。
        let text = html_to_text(html);
        assert!(text.contains("冷库 &amp; 账页"), "{text}");
        assert!(text.contains("第&nbsp;一行"), "{text}");
        assert!(!text.contains("<p>") && !text.contains("bad()") && !text.contains("x{}"), "{text}");
        let normalized = normalize_text(&text);
        assert!(normalized.contains("冷库 & 账页"), "{normalized}");
        assert!(normalized.contains("第 一行"), "{normalized}");
        assert_eq!(normalize_text("a\r\nb\n\n\n\nc  \n"), "a\nb\n\nc");
    }


    #[tokio::test]
    async fn ingest_file_then_retrieve_roundtrip() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        std::fs::write(
            root.join("source.md"),
            "# 雪夜古宅\n\n大门后是厅堂，厅堂里有一只灯笼。\n\n账页记载着冷库赔偿款的去向。",
        )
        .unwrap();

        let asset = ingest_material(
            root,
            &IngestMaterialInput {
                source_kind: "file",
                url: None,
                file_path: Some("source.md"),
                filename: Some("雪夜古宅.md"),
                mime_type: None,
                title: Some("  雪夜古宅资料  "),
                purpose: Some("worldbuilding"),
            },
        )
        .await
        .unwrap();
        assert_eq!(asset.kind, "text");
        assert_eq!(asset.title, "雪夜古宅资料");
        assert_eq!(asset.purpose, "worldbuilding");
        assert!(asset.id.starts_with("20") && asset.id.contains("-雪夜古宅资料"), "{}", asset.id);
        assert_eq!(asset.markdown_path, format!(".inkos/materials/{}.md", asset.id));
        // markdown 卡：模板空行全滤（标题直连 Metadata）；正文段落 \n\n 保留。
        let markdown = std::fs::read_to_string(root.join(&asset.markdown_path)).unwrap();
        assert!(markdown.starts_with("# 雪夜古宅资料\n## Metadata\n- kind: text"), "{markdown}");
        assert!(!markdown.contains("\n\n\n"), "{markdown}");
        assert!(markdown.contains("## Extracted content\n# 雪夜古宅"), "{markdown}");
        assert!(markdown.contains("- purpose: worldbuilding"));
        assert!(markdown.contains("## Extracted content"));
        // manifest：无尾换行 + camelCase。
        let manifest_raw = std::fs::read_to_string(root.join(&asset.manifest_path)).unwrap();
        assert!(!manifest_raw.ends_with('\n'));
        let manifest: serde_json::Value = serde_json::from_str(&manifest_raw).unwrap();
        assert_eq!(manifest["charCount"], asset.char_count as u64);
        assert_eq!(manifest["markdownPath"], asset.markdown_path);

        // 召回：标题命中（+8）排最前；purpose 过滤。
        let results = retrieve_materials(
            root,
            &RetrieveMaterialsInput {
                query: "雪夜古宅资料 账页".to_string(),
                purpose: Some("worldbuilding".to_string()),
                limit: None,
            },
        )
        .await;
        assert_eq!(results.len(), 1);
        // 246 号 BM25 语义：score = -bm25（正值、量级由语料决定）。
        assert!(results[0].score > 0.0, "{:?}", results[0]);
        let filtered = retrieve_materials(
            root,
            &RetrieveMaterialsInput {
                query: "账页".to_string(),
                purpose: Some("reference".to_string()),
                limit: None,
            },
        )
        .await;
        assert!(filtered.is_empty());
        // 无词项命中 → 空结果。
        let miss = retrieve_materials(
            root,
            &RetrieveMaterialsInput { query: "完全不存在词组".to_string(), purpose: None, limit: None },
        )
        .await;
        assert!(miss.is_empty());
    }

    #[tokio::test]
    async fn ingest_html_and_error_paths() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        std::fs::write(root.join("page.html"), "<html><head><title>官网首页</title></head><body><p>第一段正文。</p></body></html>").unwrap();
        let asset = ingest_material(
            root,
            &IngestMaterialInput {
                source_kind: "file",
                url: None,
                file_path: Some("page.html"),
                filename: None,
                mime_type: None,
                title: None,
                purpose: None,
            },
        )
        .await
        .unwrap();
        assert_eq!(asset.kind, "webpage");
        assert_eq!(asset.title, "官网首页");
        assert_eq!(asset.purpose, "reference");
        assert!(asset.excerpt.contains("第一段正文"));

        // 逃逸路径拒绝。
        let escape = ingest_material(
            root,
            &IngestMaterialInput {
                source_kind: "file",
                url: None,
                file_path: Some("../../etc/passwd"),
                filename: None,
                mime_type: None,
                title: None,
                purpose: None,
            },
        )
        .await
        .unwrap_err();
        assert!(escape.contains("Path traversal blocked"), "{escape}");
        // 缺 url / filePath 的固定错误文案。
        let no_url = ingest_material(
            root,
            &IngestMaterialInput {
                source_kind: "url",
                url: None,
                file_path: None,
                filename: None,
                mime_type: None,
                title: None,
                purpose: None,
            },
        )
        .await
        .unwrap_err();
        assert_eq!(no_url, "ingest_material.url is required for URL sources.");
        let no_file = ingest_material(
            root,
            &IngestMaterialInput {
                source_kind: "file",
                url: None,
                file_path: None,
                filename: None,
                mime_type: None,
                title: None,
                purpose: None,
            },
        )
        .await
        .unwrap_err();
        assert_eq!(no_file, "ingest_material.filePath is required for file sources.");
        // pdf（Rust 侧暂缓）。
        std::fs::write(root.join("doc.pdf"), b"%PDF-1.4 fake").unwrap();
        let pdf = ingest_material(
            root,
            &IngestMaterialInput {
                source_kind: "file",
                url: None,
                file_path: Some("doc.pdf"),
                filename: None,
                mime_type: None,
                title: None,
                purpose: None,
            },
        )
        .await
        .unwrap_err();
        assert!(pdf.starts_with("PDF text extraction failed:"), "{pdf}");
    }

    #[tokio::test]
    async fn retrieve_ignores_corrupt_manifests_and_scores_by_body() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        let materials = root.join(".inkos").join("materials");
        std::fs::create_dir_all(&materials).unwrap();
        // 手工造两张卡 + 一份损坏 manifest。
        for (id, title, body) in [
            ("t1", "A 卡", "关键字出现得很早。后面是很长的正文。"),
            ("t2", "B 卡", "开头无关，中间才出现关键字，位置靠后一点。"),
        ] {
            let md_path = format!(".inkos/materials/{id}.md");
            std::fs::write(root.join(&md_path), body).unwrap();
            std::fs::write(
                materials.join(format!("{id}.json")),
                serde_json::to_string_pretty(&serde_json::json!({
                    "id": id, "title": title, "kind": "text", "purpose": "reference",
                    "source": "local", "mimeType": "text/plain",
                    "markdownPath": md_path, "manifestPath": format!("{id}.json"),
                    "charCount": body.chars().count(), "excerpt": ""
                }))
                .unwrap(),
            )
            .unwrap();
        }
        std::fs::write(materials.join("broken.json"), "{not json").unwrap();
        let results = retrieve_materials(
            root,
            &RetrieveMaterialsInput { query: "关键字".to_string(), purpose: None, limit: None },
        )
        .await;
        assert_eq!(results.len(), 2, "损坏 manifest 不参与且不炸");
        // 246 号 BM25：短文档关键词密度更高者排序靠前（t2 更短 → 分更高）。
        assert_eq!(results[0].id, "t2", "{results:?}");
        assert!(results[0].score > results[1].score);
    }
}
