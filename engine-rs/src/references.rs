//! 书籍引用绑定子系统（216 号）。
//!
//! 移植自 `packages/core/src/references/book-references.ts`（持久化）与
//! `packages/core/src/references/reference-context.ts`（写路径选段注入）。
//!
//! - 持久化：`books/{bookId}/story/reference_bindings.json`（version 1 envelope +
//!   bindings，原子替换写）；bind 校验书存在 + materialId 安全 + 素材 manifest
//!   完整性（markdown 路径必须等于素材 id 对应路径——防路径漂移）；uses
//!   1..=12 条、单条 ≤120、去重；note ≤2000。
//! - 选段：绑定素材 markdown 抽正文（`## Extracted content` 标记）→ 按标题
//!   分节 + slug anchor 去重 → LLM 选段器只回 source id → 命中节全文作为
//!   ContextSource 条目注入 composer selectedContext；素材缺失/选段失败只记
//!   notes，不阻断写作链。
//!
//! 双端纯函数域（分节/anchor/正文抽取）由共享 golden 向量守门
//! （`packages/core/src/__tests__/golden/references-vectors.json`）。

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::models::input_governance::ContextSource;
use crate::utils::path::safe_child_path;

// ---- 持久化类型 ----

/// 单条绑定。对齐 TS `BookReferenceBinding`。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BookReferenceBinding {
    pub material_id: String,
    pub uses: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub note: Option<String>,
    pub created_at: String,
    pub updated_at: String,
}

/// 绑定清单 envelope。对齐 TS `BookReferenceManifest`。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BookReferenceManifest {
    pub version: u8,
    pub book_id: String,
    pub bindings: Vec<BookReferenceBinding>,
}

/// bind 入参。对齐 TS `BindBookReferenceInput`。
pub struct BindBookReferenceInput<'a> {
    pub material_id: &'a str,
    pub uses: &'a [String],
    pub note: Option<&'a str>,
}

/// 解析后的绑定（含素材可用性）。对齐 TS `ResolvedBookReference`。
#[derive(Debug, Clone)]
pub struct ResolvedBookReference {
    pub material_id: String,
    pub uses: Vec<String>,
    pub note: Option<String>,
    pub created_at: String,
    pub updated_at: String,
    pub available: bool,
    pub title: Option<String>,
    pub asset: Option<MaterialAssetRef>,
    pub error: Option<String>,
}

/// 素材 manifest 视图（TS 复用 `MaterialAsset`——本侧只暴露选段/工具所需字段）。
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MaterialAssetRef {
    pub id: String,
    pub title: String,
    pub markdown_path: String,
    pub manifest_path: String,
}

/// 清单 + 全量解析结果。对齐 TS `BookReferenceList`。
#[derive(Debug, Clone)]
pub struct BookReferenceList {
    pub manifest: BookReferenceManifest,
    pub references: Vec<ResolvedBookReference>,
}

const MANIFEST_FILE: &str = "reference_bindings.json";
const MAX_USES: usize = 12;
const MAX_USE_LENGTH: usize = 120;
const MAX_NOTE_LENGTH: usize = 2_000;

// ---- 路径与安全 ----

fn reference_manifest_path(project_root: &Path, book_id: &str) -> PathBuf {
    project_root.join("books").join(book_id).join("story").join(MANIFEST_FILE)
}

fn assert_book_exists(project_root: &Path, book_id: &str) -> Result<(), String> {
    let book_dir = project_root.join("books").join(book_id);
    match std::fs::metadata(&book_dir) {
        Ok(meta) if meta.is_dir() => Ok(()),
        Ok(_) => Err(format!("Book not found: {book_id}")),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            Err(format!("Book not found: {book_id}"))
        }
        Err(error) => Err(error.to_string()),
    }
}

/// `assertMaterialId`：trim 非空、≤240、非 . / .. / 无 `..` / 无路径分隔与空字节。
pub fn assert_material_id(value: &str) -> Result<String, String> {
    let material_id = value.trim();
    if material_id.is_empty()
        || material_id.len() > 240
        || material_id == "."
        || material_id == ".."
        || material_id.contains("..")
        || material_id.contains(['/', '\\', '\0'])
    {
        return Err(format!(
            "Invalid materialId: {}",
            serde_json::to_string(value).unwrap_or_default()
        ));
    }
    Ok(material_id.to_string())
}

/// `loadMaterialAsset`：读 `{materialsDir}/{id}.json` 并校验（id 一致 +
/// title/markdownPath/manifestPath 为字符串 + markdownPath 必须等于
/// `{materialsDir}/{id}.md`——经 safeChildPath 双向防穿越）。
pub fn load_material_asset(project_root: &Path, material_id_input: &str) -> Result<MaterialAssetRef, String> {
    let material_id = assert_material_id(material_id_input)?;
    let materials_dir = project_root.join(".inkos").join("materials");
    let manifest_path = safe_child_path(&materials_dir.to_string_lossy(), &format!("{material_id}.json"))?;
    let raw = std::fs::read_to_string(&manifest_path)
        .map_err(|error| format!("Invalid material manifest: {}: {error}", manifest_path.display()))?;
    let parsed: Value = serde_json::from_str(&raw)
        .map_err(|error| format!("Invalid material manifest: {}: {error}", manifest_path.display()))?;
    // TS Partial 校验面：id 一致 + title/markdownPath/manifestPath 为字符串。
    let valid = parsed.get("id").and_then(Value::as_str) == Some(material_id.as_str())
        && parsed.get("title").map(Value::is_string).unwrap_or(false)
        && parsed.get("markdownPath").map(Value::is_string).unwrap_or(false)
        && parsed.get("manifestPath").map(Value::is_string).unwrap_or(false);
    if !valid {
        return Err(format!("Invalid material manifest: {}", manifest_path.display()));
    }
    let markdown_path = parsed["markdownPath"].as_str().expect("checked string").to_string();
    let expected_markdown_path =
        safe_child_path(&materials_dir.to_string_lossy(), &format!("{material_id}.md"))?;
    let resolved_markdown_path = safe_child_path(&project_root.to_string_lossy(), &markdown_path)?;
    if resolved_markdown_path != expected_markdown_path {
        return Err(format!(
            "Material markdown path does not match its asset id: {}",
            manifest_path.display()
        ));
    }
    Ok(MaterialAssetRef {
        id: parsed["id"].as_str().expect("checked string").to_string(),
        title: parsed["title"].as_str().expect("checked string").to_string(),
        markdown_path,
        manifest_path: parsed["manifestPath"].as_str().expect("checked string").to_string(),
    })
}

// ---- 校验 ----

fn normalize_uses(values: &[String]) -> Result<Vec<String>, String> {
    let mut uses: Vec<String> = Vec::new();
    let mut seen = std::collections::HashSet::new();
    for value in values {
        let candidate = value.trim();
        if candidate.is_empty() || !seen.insert(candidate.to_string()) {
            continue;
        }
        if candidate.chars().count() > MAX_USE_LENGTH {
            return Err(format!(
                "Reference use is too long ({}/{}).",
                candidate.chars().count(),
                MAX_USE_LENGTH
            ));
        }
        uses.push(candidate.to_string());
    }
    if uses.is_empty() {
        return Err("At least one reference use is required.".to_string());
    }
    if uses.len() > MAX_USES {
        return Err(format!("Too many reference uses ({}/{}).", uses.len(), MAX_USES));
    }
    Ok(uses)
}

fn normalize_note(value: Option<&str>) -> Result<Option<String>, String> {
    let Some(value) = value else {
        return Ok(None);
    };
    let note = value.trim();
    if note.is_empty() {
        return Ok(None);
    }
    if note.chars().count() > MAX_NOTE_LENGTH {
        return Err(format!(
            "Reference note is too long ({}/{}).",
            note.chars().count(),
            MAX_NOTE_LENGTH
        ));
    }
    Ok(Some(note.to_string()))
}

// ---- 清单读写 ----

/// 读清单：缺文件 → 空 envelope；结构非法 → 逐字错误。bindings 逐条手工
/// 校验（对齐 TS parseBinding 的错误文案面）。
pub fn load_book_reference_manifest(project_root: &Path, book_id: &str) -> Result<BookReferenceManifest, String> {
    let path = reference_manifest_path(project_root, book_id);
    let raw = match std::fs::read_to_string(&path) {
        Ok(raw) => raw,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            return Ok(BookReferenceManifest { version: 1, book_id: book_id.to_string(), bindings: Vec::new() });
        }
        Err(error) => return Err(error.to_string()),
    };
    let parsed: Value = serde_json::from_str(&raw)
        .map_err(|error| format!("Invalid book reference manifest: {}: {error}", path.display()))?;
    if parsed.get("version").and_then(Value::as_u64) != Some(1)
        || parsed.get("bookId").and_then(Value::as_str) != Some(book_id)
        || !parsed.get("bindings").map(Value::is_array).unwrap_or(false)
    {
        return Err(format!("Invalid book reference manifest: {}", path.display()));
    }
    let mut bindings = Vec::new();
    for value in parsed["bindings"].as_array().expect("checked array") {
        bindings.push(parse_binding(value)?);
    }
    Ok(BookReferenceManifest { version: 1, book_id: book_id.to_string(), bindings })
}

fn parse_binding(value: &Value) -> Result<BookReferenceBinding, String> {
    let Some(obj) = value.as_object() else {
        return Err("Invalid book reference binding.".to_string());
    };
    let material_id = assert_material_id(obj.get("materialId").and_then(Value::as_str).unwrap_or(""))?;
    // TS `binding.uses ?? []`：缺键 → 空数组（落入 uses 为空的错误面）。
    let uses_value = obj.get("uses").cloned().unwrap_or_else(|| Value::Array(Vec::new()));
    let Some(uses_raw) = uses_value.as_array() else {
        return Err("Reference uses must be an array.".to_string());
    };
    let mut uses = Vec::new();
    for item in uses_raw {
        let Some(text) = item.as_str() else {
            return Err("Reference uses must contain only text.".to_string());
        };
        uses.push(text.to_string());
    }
    let uses = normalize_uses(&uses)?;
    let created_at = obj.get("createdAt").and_then(Value::as_str).unwrap_or_default();
    let updated_at = obj.get("updatedAt").and_then(Value::as_str).unwrap_or_default();
    if created_at.is_empty() || updated_at.is_empty() {
        return Err(format!("Invalid timestamps for book reference binding {material_id}."));
    }
    let note = normalize_note(obj.get("note").and_then(Value::as_str))?;
    Ok(BookReferenceBinding { material_id, uses, note, created_at: created_at.to_string(), updated_at: updated_at.to_string() })
}

fn write_manifest_atomic(project_root: &Path, manifest: &BookReferenceManifest) -> Result<(), String> {
    let path = reference_manifest_path(project_root, &manifest.book_id);
    let story_dir = project_root.join("books").join(&manifest.book_id).join("story");
    std::fs::create_dir_all(&story_dir).map_err(|e| e.to_string())?;
    let temp_path = story_dir.join(format!(
        "{MANIFEST_FILE}.tmp-{}",
        uuid::Uuid::new_v4()
    ));
    let write = || -> Result<(), String> {
        let body = format!("{}\n", serde_json::to_string_pretty(manifest).map_err(|e| e.to_string())?);
        std::fs::write(&temp_path, body).map_err(|e| e.to_string())
    };
    if let Err(error) = write() {
        let _ = std::fs::remove_file(&temp_path);
        return Err(error);
    }
    std::fs::rename(&temp_path, &path).map_err(|e| e.to_string())
}

// ---- bind / unbind / list ----

/// 绑定素材到书（upsert 语义：同 materialId 保留 createdAt 刷新其余）。
pub fn bind_book_reference(
    project_root: &Path,
    book_id: &str,
    input: &BindBookReferenceInput<'_>,
) -> Result<BookReferenceManifest, String> {
    assert_book_exists(project_root, book_id)?;
    let material_id = assert_material_id(input.material_id)?;
    load_material_asset(project_root, &material_id)?;
    let uses = normalize_uses(input.uses)?;
    let note = normalize_note(input.note)?;
    let current = load_book_reference_manifest(project_root, book_id)?;
    let now = crate::utils::utc_time::utc_now_iso();
    let created_at = current
        .bindings
        .iter()
        .find(|binding| binding.material_id == material_id)
        .map(|binding| binding.created_at.clone())
        .unwrap_or_else(|| now.clone());
    let binding = BookReferenceBinding {
        material_id,
        uses,
        note,
        created_at,
        updated_at: now,
    };
    let mut bindings: Vec<BookReferenceBinding> = current
        .bindings
        .into_iter()
        .filter(|entry| entry.material_id != binding.material_id)
        .collect();
    bindings.push(binding);
    let next = BookReferenceManifest { version: 1, book_id: book_id.to_string(), bindings };
    write_manifest_atomic(project_root, &next)?;
    Ok(next)
}

/// 解绑（素材本体保留在 .inkos/materials）；未绑定时不写盘。
pub fn unbind_book_reference(
    project_root: &Path,
    book_id: &str,
    material_id_input: &str,
) -> Result<(bool, BookReferenceManifest), String> {
    assert_book_exists(project_root, book_id)?;
    let material_id = assert_material_id(material_id_input)?;
    let current = load_book_reference_manifest(project_root, book_id)?;
    let bindings: Vec<BookReferenceBinding> = current
        .bindings
        .iter()
        .filter(|binding| binding.material_id != material_id)
        .cloned()
        .collect();
    let removed = bindings.len() != current.bindings.len();
    let manifest = BookReferenceManifest { bindings, ..current };
    if removed {
        write_manifest_atomic(project_root, &manifest)?;
    }
    Ok((removed, manifest))
}

/// 清单 + 素材可用性解析（素材缺失 → available:false + error 文本，不抛）。
pub fn list_book_references(project_root: &Path, book_id: &str) -> Result<BookReferenceList, String> {
    assert_book_exists(project_root, book_id)?;
    let manifest = load_book_reference_manifest(project_root, book_id)?;
    let mut references = Vec::new();
    for binding in &manifest.bindings {
        match load_material_asset(project_root, &binding.material_id) {
            Ok(asset) => references.push(ResolvedBookReference {
                material_id: binding.material_id.clone(),
                uses: binding.uses.clone(),
                note: binding.note.clone(),
                created_at: binding.created_at.clone(),
                updated_at: binding.updated_at.clone(),
                available: true,
                title: Some(asset.title.clone()),
                asset: Some(asset),
                error: None,
            }),
            Err(error) => references.push(ResolvedBookReference {
                material_id: binding.material_id.clone(),
                uses: binding.uses.clone(),
                note: binding.note.clone(),
                created_at: binding.created_at.clone(),
                updated_at: binding.updated_at.clone(),
                available: false,
                title: None,
                asset: None,
                error: Some(error),
            }),
        }
    }
    Ok(BookReferenceList { manifest, references })
}

// ---- 写路径选段（reference-context.ts 对应面） ----

/// 选段任务。对齐 TS `BookReferenceSelectionTask`。
pub struct ReferenceSelectionTask<'a> {
    pub chapter_number: u32,
    pub goal: &'a str,
    pub outline_node: &'a str,
    pub must_keep: &'a [String],
    pub language: &'a str,
}

/// 选段候选（不含正文）。对齐 TS `ReferenceSectionCandidate`。
#[derive(Debug, Clone, PartialEq, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ReferenceSectionCandidate {
    pub source: String,
    pub material_id: String,
    pub title: String,
    pub heading: String,
    pub uses: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub note: Option<String>,
}

/// 选段请求。对齐 TS `ReferenceSectionSelectionRequest`。
pub struct ReferenceSectionSelectionRequest<'a> {
    pub chapter_number: u32,
    pub goal: &'a str,
    pub outline_node: &'a str,
    pub must_keep: &'a [String],
    pub language: &'a str,
    pub candidates: &'a [ReferenceSectionCandidate],
}

/// LLM 选段器端口（返回命中候选的 source id 列表）。
#[async_trait::async_trait]
pub trait ReferenceSectionSelector: Send + Sync {
    async fn select(&self, request: ReferenceSectionSelectionRequest<'_>) -> Result<Vec<String>, String>;
}

/// compose 注入端口（TS `BookReferenceContextProvider` 对应面：runner 把
/// `selectBookReferenceContext` 以闭包注入 composer 输入）。
#[async_trait::async_trait]
pub trait BookReferenceContextProvider: Send + Sync {
    async fn select_context(&self, task: &ReferenceSelectionTask<'_>) -> BookReferenceContextSelection;
}

/// 生产实现：绑定清单 + LLM 选段；清单/文件面失败落
/// `book-reference-context-unavailable`（TS loadReferenceContext 的 catch 语义）。
pub struct ProductionReferenceContextProvider<'a> {
    pub project_root: &'a Path,
    pub book_id: &'a str,
    pub selector: &'a dyn ReferenceSectionSelector,
}

#[async_trait::async_trait]
impl BookReferenceContextProvider for ProductionReferenceContextProvider<'_> {
    async fn select_context(&self, task: &ReferenceSelectionTask<'_>) -> BookReferenceContextSelection {
        match select_book_reference_context(self.project_root, self.book_id, task, self.selector).await {
            Ok(selection) => selection,
            Err(_) => BookReferenceContextSelection {
                entries: Vec::new(),
                notes: vec!["book-reference-context-unavailable".to_string()],
            },
        }
    }
}

/// 选段结果：composer 注入条目 + 观测 notes。
#[derive(Debug, Clone, Default)]
pub struct BookReferenceContextSelection {
    pub entries: Vec<ContextSource>,
    pub notes: Vec<String>,
}

/// 写路径引用上下文选取：绑定素材 → 分节候选 → 选段器 → 命中节全文条目。
/// 素材缺失记 `book-reference-unavailable:{id}`；选段失败记
/// `book-reference-selection-failed`；无候选直接返回。
pub async fn select_book_reference_context(
    project_root: &Path,
    book_id: &str,
    task: &ReferenceSelectionTask<'_>,
    selector: &dyn ReferenceSectionSelector,
) -> Result<BookReferenceContextSelection, String> {
    let listed = list_book_references(project_root, book_id)?;
    let mut notes: Vec<String> = Vec::new();
    let mut sections: Vec<SplitSection> = Vec::new();
    for reference in &listed.references {
        let Some(asset) = reference.asset.as_ref() else {
            notes.push(format!("book-reference-unavailable:{}", reference.material_id));
            continue;
        };
        let markdown_path = safe_child_path(&project_root.to_string_lossy(), &asset.markdown_path)?;
        let markdown = std::fs::read_to_string(&markdown_path).map_err(|e| e.to_string())?;
        let content = extract_material_content(&markdown);
        sections.extend(split_reference_sections(&SplitSectionsInput {
            material_id: &reference.material_id,
            title: &asset.title,
            uses: &reference.uses,
            note: reference.note.as_deref(),
            content: &content,
        }));
    }
    if sections.is_empty() {
        return Ok(BookReferenceContextSelection { entries: Vec::new(), notes });
    }

    // TS：候选（去掉正文，只给标题面）交给选段器；命中后按 source 载回原文。
    let candidates: Vec<ReferenceSectionCandidate> = sections
        .iter()
        .map(|section| ReferenceSectionCandidate {
            source: section.source.clone(),
            material_id: section.material_id.clone(),
            title: section.title.clone(),
            heading: section.heading.clone(),
            uses: section.uses.clone(),
            note: section.note.clone(),
        })
        .collect();
    let request = ReferenceSectionSelectionRequest {
        chapter_number: task.chapter_number,
        goal: task.goal,
        outline_node: task.outline_node,
        must_keep: task.must_keep,
        language: task.language,
        candidates: &candidates,
    };
    let selected_sources = match selector.select(request).await {
        Ok(sources) => sources,
        Err(_) => {
            notes.push("book-reference-selection-failed".to_string());
            return Ok(BookReferenceContextSelection { entries: Vec::new(), notes });
        }
    };

    let selected: std::collections::HashSet<String> = selected_sources.into_iter().collect();
    let entries = sections
        .iter()
        .filter(|section| selected.contains(&section.source))
        .map(|section| ContextSource {
            source: section.source.clone(),
            reason: render_reason(&section.title, &section.uses, section.note.as_deref()),
            excerpt: Some(section.content.clone()),
        })
        .collect();
    Ok(BookReferenceContextSelection { entries, notes })
}

/// 分节结果（含节正文；候选即去掉 content 的投影）。可序列化——golden 向量面。
#[derive(Debug, Clone, PartialEq, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SplitSection {
    pub source: String,
    pub material_id: String,
    pub title: String,
    pub heading: String,
    pub uses: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub note: Option<String>,
    pub content: String,
}

pub struct SplitSectionsInput<'a> {
    pub material_id: &'a str,
    pub title: &'a str,
    pub uses: &'a [String],
    pub note: Option<&'a str>,
    pub content: &'a str,
}

/// `extractMaterialContent`：`## Extracted content` 标记之后为正文；无标记全文 trim。
pub fn extract_material_content(markdown: &str) -> String {
    let marker = regex::Regex::new(r"(?m)^## Extracted content\s*$").expect("extract marker regex");
    match marker.find(markdown) {
        Some(found) => markdown[found.end()..].trim().to_string(),
        None => markdown.trim().to_string(),
    }
}

struct ParsedSection {
    heading: String,
    lines: Vec<String>,
}

/// `splitReferenceSections`（纯函数，golden 向量守门）：标题行分节 +
/// 前言（非空）以素材标题作节 + 空节过滤 + slug anchor 去重。
pub fn split_reference_sections(input: &SplitSectionsInput<'_>) -> Vec<SplitSection> {
    let heading_re = heading_line_re();
    let mut parsed: Vec<ParsedSection> = Vec::new();
    let mut current: Option<ParsedSection> = None;
    let mut preamble: Vec<String> = Vec::new();
    for line in input.content.split('\n') {
        let line = line.strip_suffix('\r').unwrap_or(line);
        if let Some(caps) = heading_re.captures(line) {
            if let Some(existing) = current.take() {
                parsed.push(existing);
            }
            current = Some(ParsedSection {
                heading: caps[2].trim().to_string(),
                lines: vec![line.to_string()],
            });
            continue;
        }
        match current.as_mut() {
            Some(section) => section.lines.push(line.to_string()),
            None => preamble.push(line.to_string()),
        }
    }
    if let Some(existing) = current.take() {
        parsed.push(existing);
    }
    if preamble.iter().any(|line| !line.trim().is_empty()) {
        parsed.insert(
            0,
            ParsedSection { heading: input.title.to_string(), lines: preamble },
        );
    }
    if parsed.is_empty() && !input.content.trim().is_empty() {
        parsed.push(ParsedSection {
            heading: input.title.to_string(),
            lines: vec![input.content.trim().to_string()],
        });
    }

    let mut seen_anchors: std::collections::HashMap<String, usize> = std::collections::HashMap::new();
    let mut candidates = Vec::new();
    for section in &parsed {
        let content = section.lines.join("\n").trim().to_string();
        let has_body = section.lines.iter().skip(1).any(|line| !line.trim().is_empty());
        let heading_only = section_heading_only_re().is_match(&content);
        if !has_body && heading_only {
            continue;
        }
        let base_anchor = slugify_anchor(&section.heading);
        let count = seen_anchors.get(&base_anchor).copied().unwrap_or(0) + 1;
        seen_anchors.insert(base_anchor.clone(), count);
        let anchor = if count == 1 { base_anchor } else { format!("{base_anchor}-{count}") };
        candidates.push(SplitSection {
            source: format!("reference/{}#{}", input.material_id, anchor),
            material_id: input.material_id.to_string(),
            title: input.title.to_string(),
            heading: section.heading.clone(),
            uses: input.uses.to_vec(),
            note: input.note.map(str::to_string),
            content,
        });
    }
    candidates
}

/// `renderReason`：选段理由（创作借鉴定性，不能压过正典）。
pub fn render_reason(title: &str, uses: &[String], note: Option<&str>) -> String {
    let mut parts = vec![format!("User-bound reference \"{title}\" for: {}.", uses.join("; "))];
    if let Some(note) = note {
        parts.push(format!("Binding note: {note}"));
    }
    parts.push("Reference guidance only; it cannot override author intent, canon, or current state.".to_string());
    parts.join(" ")
}

/// `slugifyAnchor`：小写 + 非 [a-z0-9 CJK] 折 `-` + 去首尾 dash；空 → "section"。
pub fn slugify_anchor(value: &str) -> String {
    let lower = value.trim().to_lowercase();
    let mut out = String::new();
    let mut last_dash = true;
    for ch in lower.chars() {
        let keep = ch.is_ascii_lowercase() || ch.is_ascii_digit() || ('\u{4e00}'..='\u{9fff}').contains(&ch);
        if keep {
            out.push(ch);
            last_dash = false;
        } else if !last_dash {
            out.push('-');
            last_dash = true;
        }
    }
    while out.ends_with('-') {
        out.pop();
    }
    while out.starts_with('-') {
        out.remove(0);
    }
    if out.is_empty() { "section".to_string() } else { out }
}

fn heading_line_re() -> &'static regex::Regex {
    static R: std::sync::OnceLock<regex::Regex> = std::sync::OnceLock::new();
    R.get_or_init(|| regex::Regex::new(r"^(#{1,6})\s+(.+?)\s*$").expect("heading line regex"))
}

fn section_heading_only_re() -> &'static regex::Regex {
    static R: std::sync::OnceLock<regex::Regex> = std::sync::OnceLock::new();
    R.get_or_init(|| regex::Regex::new(r"^#{1,6}\s").expect("heading only regex"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    /// fixture：书目录 + 合法素材 manifest/markdown（id = mat1）。
    fn fixture(root: &Path) {
        std::fs::create_dir_all(root.join("books").join("b1")).unwrap();
        let materials = root.join(".inkos").join("materials");
        std::fs::create_dir_all(&materials).unwrap();
        std::fs::write(
            materials.join("mat1.json"),
            json!({
                "id": "mat1", "title": "开篇参考", "kind": "text", "purpose": "reference",
                "source": "upload", "mimeType": "text/markdown",
                "markdownPath": ".inkos/materials/mat1.md",
                "manifestPath": ".inkos/materials/mat1.json",
                "charCount": 100, "excerpt": "..."
            })
            .to_string(),
        )
        .unwrap();
        std::fs::write(
            materials.join("mat1.md"),
            "# 上传：开篇参考\n\n## Extracted content\n\n## 布局机制\n\n正文甲。\n\n## 人物关系\n\n正文乙。\n",
        )
        .unwrap();
    }

    fn uses(v: &[&str]) -> Vec<String> {
        v.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn bind_list_unbind_roundtrip() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        fixture(root);
        let manifest = bind_book_reference(
            root,
            "b1",
            &BindBookReferenceInput { material_id: "mat1", uses: &uses(&["开篇机制", "人物关系"]), note: Some("慢节奏") },
        )
        .unwrap();
        assert_eq!(manifest.version, 1);
        assert_eq!(manifest.book_id, "b1");
        assert_eq!(manifest.bindings.len(), 1);
        // 原子写落盘。
        assert!(reference_manifest_path(root, "b1").exists());
        // list 解析出可用素材与标题。
        let listed = list_book_references(root, "b1").unwrap();
        assert_eq!(listed.references.len(), 1);
        assert!(listed.references[0].available);
        assert_eq!(listed.references[0].title.as_deref(), Some("开篇参考"));
        // upsert：同素材再 bind 保留 createdAt。
        let first_created = manifest.bindings[0].created_at.clone();
        let updated = bind_book_reference(
            root,
            "b1",
            &BindBookReferenceInput { material_id: "mat1", uses: &uses(&["调查节奏"]), note: None },
        )
        .unwrap();
        assert_eq!(updated.bindings[0].created_at, first_created);
        assert_eq!(updated.bindings[0].uses, uses(&["调查节奏"]));
        // unbind：移除后 manifest 不再含绑定；再次 unbind removed=false。
        let (removed, _) = unbind_book_reference(root, "b1", "mat1").unwrap();
        assert!(removed);
        let (removed_again, empty_manifest) = unbind_book_reference(root, "b1", "mat1").unwrap();
        assert!(!removed_again);
        assert!(empty_manifest.bindings.is_empty());
    }

    #[test]
    fn bind_validation_errors_match_ts_surface() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        fixture(root);
        let err = |input: BindBookReferenceInput<'_>| {
            bind_book_reference(root, "b1", &input).unwrap_err()
        };
        // 书不存在。
        assert_eq!(
            bind_book_reference(root, "b2", &BindBookReferenceInput { material_id: "mat1", uses: &uses(&["x"]), note: None })
                .unwrap_err(),
            "Book not found: b2"
        );
        // 素材 id 非法。
        assert_eq!(
            err(BindBookReferenceInput { material_id: "../escape", uses: &uses(&["x"]), note: None }),
            "Invalid materialId: \"../escape\""
        );
        // 素材缺失。
        assert!(err(BindBookReferenceInput { material_id: "mat9", uses: &uses(&["x"]), note: None })
            .starts_with("Invalid material manifest:"));
        // uses 空 / 超限。
        assert_eq!(
            err(BindBookReferenceInput { material_id: "mat1", uses: &uses(&[]), note: None }),
            "At least one reference use is required."
        );
        let too_many: Vec<String> = (1..=13).map(|i| format!("用途{i}")).collect();
        assert_eq!(
            err(BindBookReferenceInput { material_id: "mat1", uses: &too_many, note: None }),
            "Too many reference uses (13/12)."
        );
        let too_long = "长".repeat(121);
        assert_eq!(
            err(BindBookReferenceInput { material_id: "mat1", uses: &uses(&[&too_long]), note: None }),
            "Reference use is too long (121/120)."
        );
        // note 超限。
        let long_note = "注".repeat(2001);
        assert_eq!(
            err(BindBookReferenceInput { material_id: "mat1", uses: &uses(&["x"]), note: Some(&long_note) }),
            "Reference note is too long (2001/2000)."
        );
        // 空 note/uses trim 规整。
        let manifest = bind_book_reference(
            root,
            "b1",
            &BindBookReferenceInput { material_id: "mat1", uses: &uses(&[" 开篇机制 ", "开篇机制", ""]), note: Some("  ") },
        )
        .unwrap();
        assert_eq!(manifest.bindings[0].uses, uses(&["开篇机制"]));
        assert_eq!(manifest.bindings[0].note, None);
    }

    #[test]
    fn load_manifest_missing_or_invalid() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        fixture(root);
        // 缺文件 → 空 envelope。
        let empty = load_book_reference_manifest(root, "b1").unwrap();
        assert!(empty.bindings.is_empty());
        // 非法 envelope。
        std::fs::create_dir_all(root.join("books").join("b1").join("story")).unwrap();
        std::fs::write(
            reference_manifest_path(root, "b1"),
            json!({"version": 2, "bookId": "b1", "bindings": []}).to_string(),
        )
        .unwrap();
        let error = load_book_reference_manifest(root, "b1").unwrap_err();
        assert!(error.starts_with("Invalid book reference manifest:"));
        // 非法 binding（uses 缺失）。
        std::fs::write(
            reference_manifest_path(root, "b1"),
            json!({"version": 1, "bookId": "b1", "bindings": [{"materialId": "mat1", "createdAt": "t", "updatedAt": "t"}]})
                .to_string(),
        )
        .unwrap();
        assert_eq!(
            load_book_reference_manifest(root, "b1").unwrap_err(),
            "At least one reference use is required."
        );
    }

    #[test]
    fn markdown_path_mismatch_rejected() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        fixture(root);
        let materials = root.join(".inkos").join("materials");
        let mut asset: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(materials.join("mat1.json")).unwrap()).unwrap();
        asset["markdownPath"] = json!(".inkos/materials/other.md");
        std::fs::write(materials.join("mat1.json"), asset.to_string()).unwrap();
        let error = load_material_asset(root, "mat1").unwrap_err();
        assert!(error.starts_with("Material markdown path does not match its asset id"), "{error}");
    }

    #[test]
    fn extract_content_with_and_without_marker() {
        let with = "# 上传：x\n\n## Extracted content\n\n正文A\n\n正文B\n";
        assert_eq!(extract_material_content(with), "正文A\n\n正文B");
        let without = "# 纯正文\n\n段落。\n";
        assert_eq!(extract_material_content(without), "# 纯正文\n\n段落。");
    }

    #[test]
    fn split_sections_headings_preamble_and_anchor_dedup() {
        let input = SplitSectionsInput {
            material_id: "mat1",
            title: "开篇参考",
            uses: &uses(&["布局"]),
            note: None,
            content: "前言行。\n\n## 布局机制\n\n正文甲。\n\n## 人物关系\n\n正文乙。\n\n## 布局机制\n\n重复标题。\n",
        };
        let sections = split_reference_sections(&input);
        let sources: Vec<&str> = sections.iter().map(|s| s.source.as_str()).collect();
        assert_eq!(
            sources,
            vec![
                "reference/mat1#开篇参考",
                "reference/mat1#布局机制",
                "reference/mat1#人物关系",
                "reference/mat1#布局机制-2",
            ]
        );
        assert_eq!(sections[0].content, "前言行。");
        assert_eq!(sections[1].content, "## 布局机制\n\n正文甲。");
        assert_eq!(sections[3].content, "## 布局机制\n\n重复标题。");
    }

    #[test]
    fn split_sections_heading_only_filtered_and_empty_fallback() {
        // 纯标题无正文的节被过滤。
        let input = SplitSectionsInput {
            material_id: "m",
            title: "T",
            uses: &uses(&["u"]),
            note: None,
            content: "## 空节\n\n## 有正文\n\n内容。",
        };
        let sections = split_reference_sections(&input);
        assert_eq!(sections.len(), 1);
        assert_eq!(sections[0].heading, "有正文");
        // 空内容 → 无候选（不回退：TS 仅在 parsed 为空且 content.trim() 时回退）。
        let input2 = SplitSectionsInput { material_id: "m", title: "T", uses: &uses(&["u"]), note: None, content: "  " };
        assert!(split_reference_sections(&input2).is_empty());
        // 无标题正文 → 单节回退以标题命名。
        let input3 = SplitSectionsInput { material_id: "m", title: "T", uses: &uses(&["u"]), note: None, content: "裸正文。" };
        let sections3 = split_reference_sections(&input3);
        assert_eq!(sections3.len(), 1);
        assert_eq!(sections3[0].heading, "T");
        assert_eq!(sections3[0].source, "reference/m#t");
    }

    #[test]
    fn slugify_anchor_cases() {
        assert_eq!(slugify_anchor("布局机制"), "布局机制");
        assert_eq!(slugify_anchor("The Opening Hook!"), "the-opening-hook");
        assert_eq!(slugify_anchor("  混合 ABC 机制  "), "混合-abc-机制");
        assert_eq!(slugify_anchor("!!!"), "section");
        assert_eq!(slugify_anchor("a--b"), "a-b");
    }

    // -- select_book_reference_context 的 mock 选段器 --

    struct MockSelector {
        result: Result<Vec<String>, String>,
    }

    #[async_trait::async_trait]
    impl ReferenceSectionSelector for MockSelector {
        async fn select(&self, _request: ReferenceSectionSelectionRequest<'_>) -> Result<Vec<String>, String> {
            self.result.clone()
        }
    }

    #[tokio::test]
    async fn select_context_end_to_end() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        fixture(root);
        bind_book_reference(
            root,
            "b1",
            &BindBookReferenceInput { material_id: "mat1", uses: &uses(&["布局", "人物"]), note: Some("慢节奏") },
        )
        .unwrap();
        let task = ReferenceSelectionTask {
            chapter_number: 3,
            goal: "开局布局",
            outline_node: "第一幕",
            must_keep: &uses(&["悬念"]),
            language: "zh",
        };
        // 命中一节 → entries 带原文与理由。
        let selector = MockSelector { result: Ok(vec!["reference/mat1#人物关系".to_string()]) };
        let selection = select_book_reference_context(root, "b1", &task, &selector).await.unwrap();
        assert!(selection.notes.is_empty());
        assert_eq!(selection.entries.len(), 1);
        assert_eq!(selection.entries[0].source, "reference/mat1#人物关系");
        assert_eq!(selection.entries[0].excerpt.as_deref(), Some("## 人物关系\n\n正文乙。"));
        assert!(selection.entries[0].reason.contains("User-bound reference \"开篇参考\" for: 布局; 人物."));
        assert!(selection.entries[0].reason.contains("Binding note: 慢节奏"));
        assert!(selection.entries[0].reason.contains("Reference guidance only"));
        // 选段失败 → 空条目 + note。
        let failing = MockSelector { result: Err("llm down".to_string()) };
        let selection = select_book_reference_context(root, "b1", &task, &failing).await.unwrap();
        assert!(selection.entries.is_empty());
        assert_eq!(selection.notes, vec!["book-reference-selection-failed".to_string()]);
    }

    #[tokio::test]
    async fn select_context_reports_unavailable_material() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        fixture(root);
        bind_book_reference(
            root,
            "b1",
            &BindBookReferenceInput { material_id: "mat1", uses: &uses(&["布局"]), note: None },
        )
        .unwrap();
        // 素材 manifest 丢失 → available:false → note（TS loadMaterialAsset
        // 只读 manifest；markdown 丢失属于选段中读文件失败，向上抛由
        // provider 层落 book-reference-context-unavailable）。
        std::fs::remove_file(root.join(".inkos/materials/mat1.json")).unwrap();
        let task = ReferenceSelectionTask {
            chapter_number: 1,
            goal: "g",
            outline_node: "o",
            must_keep: &[],
            language: "zh",
        };
        let selector = MockSelector { result: Ok(vec![]) };
        let selection = select_book_reference_context(root, "b1", &task, &selector).await.unwrap();
        assert_eq!(selection.notes, vec!["book-reference-unavailable:mat1".to_string()]);
        assert!(selection.entries.is_empty());
    }
}
