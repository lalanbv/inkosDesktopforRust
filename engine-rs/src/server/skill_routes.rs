//! skills / prompt-packs 轻域端点（server.ts L4209-L4268）。
//!
//! - `GET /skills`：五目录扫描 + 注册表归并 → `{skills, diagnostics}`
//! - `GET /prompt-packs`：3 pack + 12 prompt（project 覆盖优先）
//! - `PUT /prompt-packs/:promptId`：写 project 覆盖文件（404 未知 id /
//!   400 INVALID_PROMPT_PACK_PAYLOAD 两段校验）
//! - `DELETE /prompt-packs/:promptId`：删 project 覆盖（回 builtin）
//! - `POST /skills/import`：dataUrl 文件组 → staging 原子导入 `.agents/skills/{id}`
//! - `DELETE /skills/:skillId`：删项目技能目录（404 SKILL_NOT_FOUND）
//!
//! 错误形状：`{"error":{"code":..,"message":..}}`（ApiError onError 形状逐字）。

use std::collections::HashSet;
use std::path::{Path, PathBuf};

use axum::body::Bytes;
use axum::extract::{Path as AxumPath, State};
use axum::http::StatusCode;
use axum::response::IntoResponse;
use axum::Json;
use serde::Serialize;
use serde_json::{json, Value};

use crate::prompts::{builtin_prompt_packs, builtin_prompts, get_builtin_prompt, BuiltinPrompt};
use crate::prompts::prompt_pack::prompt_override_path;
use crate::server::books_routes::BooksRuntime;
use crate::skills::external_loader::{
    list_project_skill_ids, load_available_agent_skills, parse_agent_skill_document,
};
use crate::skills::{
    create_skill_registry, normalize_skill_id_strict, AgentSkill, SkillRegistry, SkillSource,
};

const MAX_SKILL_IMPORT_FILES: usize = 128;
const MAX_SKILL_IMPORT_FILE_BYTES: usize = 2 * 1024 * 1024;
const MAX_SKILL_IMPORT_TOTAL_BYTES: usize = 8 * 1024 * 1024;

type ApiErrorResponse = (StatusCode, Json<Value>);

/// ApiError 响应（onError 形状 `{"error":{"code","message"}}`）。
fn api_error(status: StatusCode, code: &str, message: impl Into<String>) -> ApiErrorResponse {
    (
        status,
        Json(json!({ "error": { "code": code, "message": message.into() } })),
    )
}

fn bad_request(code: &str, message: impl Into<String>) -> ApiErrorResponse {
    api_error(StatusCode::BAD_REQUEST, code, message)
}

fn internal_error(error: &std::io::Error) -> ApiErrorResponse {
    api_error(
        StatusCode::INTERNAL_SERVER_ERROR,
        "INTERNAL_ERROR",
        error.to_string(),
    )
}

// ── 路径件 ─────────────────────────────────────────────────────

fn project_skills_dir(root: &Path) -> PathBuf {
    root.join(".agents").join("skills")
}

fn project_skill_dir(root: &Path, id: &str) -> PathBuf {
    project_skills_dir(root).join(id)
}

fn project_skill_path(root: &Path, id: &str) -> PathBuf {
    project_skill_dir(root, id).join("SKILL.md")
}

/// `toPosixPath(relative(root, path))`：root 内相对路径的 posix 形态。
fn to_posix_relative(root: &Path, path: &Path) -> String {
    path.strip_prefix(root)
        .map(|rel| rel.to_string_lossy().replace(std::path::MAIN_SEPARATOR, "/"))
        .unwrap_or_else(|_| path.to_string_lossy().replace(std::path::MAIN_SEPARATOR, "/"))
}

// ── 响应形状 ───────────────────────────────────────────────────

/// `toStudioSkill` 形状（undefined 字段不序列化）。
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct StudioSkill {
    id: String,
    name: String,
    description: String,
    body: String,
    source: String,
    editable: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    path: Option<String>,
}

fn skill_source_str(source: SkillSource) -> &'static str {
    match source {
        SkillSource::Builtin => "builtin",
        SkillSource::Project => "project",
        SkillSource::User => "user",
        SkillSource::External => "external",
    }
}

fn to_studio_skill(skill: &AgentSkill, root: &Path, project_skill_ids: &HashSet<String>) -> StudioSkill {
    let is_project = project_skill_ids.contains(&skill.id);
    let path = is_project.then(|| to_posix_relative(root, &project_skill_path(root, &skill.id)));
    StudioSkill {
        id: skill.id.clone(),
        name: skill.name.clone(),
        description: skill.description.clone(),
        body: skill.body.clone(),
        source: if is_project {
            "project".to_string()
        } else {
            skill_source_str(skill.source).to_string()
        },
        editable: is_project,
        path,
    }
}

/// `toStudioPromptPackPrompt` 形状。
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct StudioPromptPackPrompt {
    id: String,
    pack_id: String,
    title: String,
    default_content: String,
    content: String,
    source: String,
    overridden: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    path: Option<String>,
}

/// `toStudioPromptPackPrompt`：读 project 覆盖（存在 → project，否则 builtin）。
///
/// 对齐 `loadPromptPackPrompt({projectRoot})` 只传 project 层的行为（user 层不注入）；
/// 读取任何失败均视为无覆盖（TS readTextIfExists catch 全部）。
async fn to_studio_prompt_pack_prompt(root: &Path, prompt: &BuiltinPrompt) -> StudioPromptPackPrompt {
    let override_path = PathBuf::from(prompt_override_path(
        &root.to_string_lossy(),
        prompt.id,
    ));
    if let Ok(content) = tokio::fs::read_to_string(&override_path).await {
        return StudioPromptPackPrompt {
            id: prompt.id.to_string(),
            pack_id: prompt.pack_id.to_string(),
            title: prompt.title.to_string(),
            default_content: prompt.content.to_string(),
            content,
            source: "project".to_string(),
            overridden: true,
            path: Some(to_posix_relative(root, &override_path)),
        };
    }
    StudioPromptPackPrompt {
        id: prompt.id.to_string(),
        pack_id: prompt.pack_id.to_string(),
        title: prompt.title.to_string(),
        default_content: prompt.content.to_string(),
        content: prompt.content.to_string(),
        source: "builtin".to_string(),
        overridden: false,
        path: None,
    }
}

// ── GET /api/v1/skills ─────────────────────────────────────────

pub async fn list_skills(State(runtime): State<BooksRuntime>) -> impl IntoResponse {
    let root = runtime.state.project_root();
    let env_dirs = env_skill_dirs();
    let home = home_dir();
    // 130 号合并同步：TS loadStudioSkills 从 configured-only 升级为
    // loadAvailableAgentSkills（builtin + configured，同名后者覆盖）。
    let available = load_available_agent_skills(root, &env_dirs, home.as_deref()).await;
    let Ok(project_skill_ids) = list_project_skill_ids(root).await else {
        return internal_error_message("failed to list project skills");
    };
    let registry = create_skill_registry(available.skills);
    let skills: Vec<StudioSkill> = registry
        .list_skills()
        .iter()
        .map(|skill| to_studio_skill(skill, root, &project_skill_ids))
        .collect();
    (
        StatusCode::OK,
        Json(json!({ "skills": skills, "diagnostics": available.diagnostics })),
    )
}

fn internal_error_message(message: &str) -> ApiErrorResponse {
    api_error(StatusCode::INTERNAL_SERVER_ERROR, "INTERNAL_ERROR", message)
}

/// `INKOS_SKILL_DIRS`（路径分隔符分割 → trim → 去空）。
fn env_skill_dirs() -> Vec<String> {
    std::env::var("INKOS_SKILL_DIRS")
        .map(|value| {
            std::env::split_paths(&value)
                .map(|p| p.to_string_lossy().trim().to_string())
                .filter(|s| !s.is_empty())
                .collect()
        })
        .unwrap_or_default()
}

fn home_dir() -> Option<PathBuf> {
    std::env::var_os("HOME")
        .or_else(|| std::env::var_os("USERPROFILE"))
        .map(PathBuf::from)
}

// ── GET /api/v1/prompt-packs ───────────────────────────────────

pub async fn list_prompt_packs(State(runtime): State<BooksRuntime>) -> impl IntoResponse {
    let root = runtime.state.project_root();
    let mut prompts = Vec::with_capacity(builtin_prompts().len());
    for prompt in builtin_prompts() {
        prompts.push(to_studio_prompt_pack_prompt(root, prompt).await);
    }
    (
        StatusCode::OK,
        Json(json!({ "packs": builtin_prompt_packs(), "prompts": prompts })),
    )
}

// ── PUT /api/v1/prompt-packs/:promptId ─────────────────────────

/// `normalizeStudioPromptId`：trim + lower → 必须命中 builtin（消息带原值）。
fn normalize_studio_prompt_id(value: &str) -> Result<String, ApiErrorResponse> {
    let prompt_id = value.trim().to_lowercase();
    if prompt_id.is_empty() || get_builtin_prompt(&prompt_id).is_none() {
        return Err(api_error(
            StatusCode::NOT_FOUND,
            "PROMPT_PACK_PROMPT_NOT_FOUND",
            format!("Prompt pack prompt not found: {value}"),
        ));
    }
    Ok(prompt_id)
}

pub async fn put_prompt_pack(
    State(runtime): State<BooksRuntime>,
    AxumPath(prompt_id): AxumPath<String>,
    body: Bytes,
) -> impl IntoResponse {
    let root = runtime.state.project_root();
    let prompt_id = match normalize_studio_prompt_id(&prompt_id) {
        Ok(id) => id,
        Err(response) => return response,
    };
    let Ok(payload) = serde_json::from_slice::<Value>(&body) else {
        return bad_request("INVALID_PROMPT_PACK_PAYLOAD", "Prompt pack payload must be JSON");
    };
    // TS：`payload && typeof payload === "object" && "content" in payload`（数组无 content 键）
    let Some(content) = payload
        .as_object()
        .and_then(|record| record.get("content"))
        .and_then(Value::as_str)
    else {
        return bad_request("INVALID_PROMPT_PACK_PAYLOAD", "content must be a string");
    };

    let file = PathBuf::from(prompt_override_path(&root.to_string_lossy(), &prompt_id));
    if let Some(parent) = file.parent() {
        if let Err(e) = tokio::fs::create_dir_all(parent).await {
            return internal_error(&e);
        }
    }
    if let Err(e) = tokio::fs::write(&file, content).await {
        return internal_error(&e);
    }
    let prompt = builtin_prompts()
        .iter()
        .find(|item| item.id == prompt_id)
        .expect("normalize_studio_prompt_id 已确认存在");
    (
        StatusCode::OK,
        Json(json!({ "prompt": to_studio_prompt_pack_prompt(root, prompt).await })),
    )
}

// ── DELETE /api/v1/prompt-packs/:promptId ──────────────────────

pub async fn delete_prompt_pack(
    State(runtime): State<BooksRuntime>,
    AxumPath(prompt_id): AxumPath<String>,
) -> impl IntoResponse {
    let root = runtime.state.project_root();
    let prompt_id = match normalize_studio_prompt_id(&prompt_id) {
        Ok(id) => id,
        Err(response) => return response,
    };
    let file = PathBuf::from(prompt_override_path(&root.to_string_lossy(), &prompt_id));
    // TS rm force：ENOENT 静默，其余上抛
    if let Err(e) = tokio::fs::remove_file(&file).await {
        if e.kind() != std::io::ErrorKind::NotFound {
            return internal_error(&e);
        }
    }
    let prompt = builtin_prompts()
        .iter()
        .find(|item| item.id == prompt_id)
        .expect("normalize_studio_prompt_id 已确认存在");
    (
        StatusCode::OK,
        Json(json!({ "prompt": to_studio_prompt_pack_prompt(root, prompt).await })),
    )
}

// ── POST /api/v1/skills/import ─────────────────────────────────

/// dataUrl base64 解码（Node `Buffer.from(s, "base64")` 宽松语义）。
///
/// 逐 6bit 累积、忽略一切非 base64 字符（含 `=`/空白/非法字符），
/// 每 8bit 产出一字节——与 Node 的宽松解码完全一致。
fn decode_base64_lenient(input: &str) -> Vec<u8> {
    let mut out = Vec::new();
    let mut acc: u32 = 0;
    let mut bits: u32 = 0;
    for c in input.chars() {
        let v = match c {
            'A'..='Z' => c as u32 - 'A' as u32,
            'a'..='z' => c as u32 - 'a' as u32 + 26,
            '0'..='9' => c as u32 - '0' as u32 + 52,
            '+' => 62,
            '/' => 63,
            _ => continue,
        };
        acc = (acc << 6) | v;
        bits += 6;
        if bits >= 8 {
            bits -= 8;
            out.push(((acc >> bits) & 0xFF) as u8);
        }
    }
    out
}

/// `parseDataUrl`：`^data:([^;,]+)?(?:;[^,]*)?;base64,(.*)$`（dotall）。
///
/// mime 段与参数段都禁止逗号，故 `;base64,` 的匹配点之前不得出现任何逗号；
/// payload（`(.*)` dotall）可含逗号与换行。
fn parse_data_url(data_url: &str) -> Result<Vec<u8>, ApiErrorResponse> {
    let invalid = || {
        bad_request("INVALID_ATTACHMENT_DATA_URL", "Attachment must be a base64 data URL")
    };
    let Some(rest) = data_url.strip_prefix("data:") else {
        return Err(invalid());
    };
    let Some(marker) = rest.find(";base64,") else {
        return Err(invalid());
    };
    if rest[..marker].contains(',') {
        return Err(invalid());
    }
    Ok(decode_base64_lenient(&rest[marker + ";base64,".len()..]))
}

struct SkillImportFile {
    path: String,
    buffer: Vec<u8>,
}

/// `normalizeSkillImportPath`：反斜杠归一、`./+` 前缀剥离、段校验。
fn normalize_skill_import_path(value: Option<&Value>) -> Result<String, ApiErrorResponse> {
    let invalid = |raw: &str| {
        bad_request(
            "INVALID_SKILL_IMPORT_PATH",
            format!("Unsafe skill import path: {raw}"),
        )
    };
    let Some(raw) = value.and_then(Value::as_str) else {
        return Err(bad_request(
            "INVALID_SKILL_IMPORT_PATH",
            "Skill import file path must be a string",
        ));
    };
    let mut normalized = raw.trim().replace('\\', "/");
    // TS：replace(/^\.\/+/, "")（一个 '.' + 至少一个 '/' 及后续连续 '/'）
    if let Some(rest) = normalized.strip_prefix('.') {
        let slashes = rest.len() - rest.trim_start_matches('/').len();
        if slashes > 0 {
            normalized = rest[slashes..].to_string();
        }
    }
    let looks_like_drive = {
        let b = normalized.as_bytes();
        b.len() >= 3 && b[0].is_ascii_alphabetic() && b[1] == b':' && b[2] == b'/'
    };
    if normalized.is_empty()
        || normalized.starts_with('/')
        || looks_like_drive
        || normalized.contains('\0')
    {
        return Err(invalid(raw));
    }
    let parts: Vec<&str> = normalized.split('/').collect();
    if parts.iter().any(|part| part.is_empty() || *part == "." || *part == "..") {
        return Err(invalid(raw));
    }
    Ok(parts.join("/"))
}

/// `normalizeSkillImportFiles`：数量/大小/重复/manifest 唯一性/前缀约束（顺序逐字）。
fn normalize_skill_import_files(
    value: Option<&Value>,
) -> Result<(Vec<SkillImportFile>, String), ApiErrorResponse> {
    let arr = value
        .and_then(Value::as_array)
        .ok_or_else(|| bad_request("INVALID_SKILL_IMPORT", "Skill import requires at least one file"))?;
    if arr.is_empty() {
        return Err(bad_request(
            "INVALID_SKILL_IMPORT",
            "Skill import requires at least one file",
        ));
    }
    if arr.len() > MAX_SKILL_IMPORT_FILES {
        return Err(api_error(
            StatusCode::PAYLOAD_TOO_LARGE,
            "SKILL_IMPORT_TOO_MANY_FILES",
            format!("A skill may contain at most {MAX_SKILL_IMPORT_FILES} files"),
        ));
    }

    let mut files: Vec<SkillImportFile> = Vec::with_capacity(arr.len());
    let mut seen_path_keys: HashSet<String> = HashSet::new();
    let mut total_bytes = 0usize;
    for item in arr {
        let record = item
            .as_object()
            .ok_or_else(|| bad_request("INVALID_SKILL_IMPORT", "Each skill import entry must be an object"))?;
        let path = normalize_skill_import_path(record.get("path"))?;
        let path_key = path.to_lowercase();
        if !seen_path_keys.insert(path_key) {
            return Err(bad_request(
                "INVALID_SKILL_IMPORT",
                format!("Duplicate skill import path: {path}"),
            ));
        }
        let data_url = record
            .get("dataUrl")
            .and_then(Value::as_str)
            .ok_or_else(|| bad_request("INVALID_SKILL_IMPORT", format!("Missing dataUrl for {path}")))?;
        let buffer = parse_data_url(data_url)?;
        if buffer.len() > MAX_SKILL_IMPORT_FILE_BYTES {
            return Err(api_error(
                StatusCode::PAYLOAD_TOO_LARGE,
                "SKILL_IMPORT_FILE_TOO_LARGE",
                format!("{path} exceeds {MAX_SKILL_IMPORT_FILE_BYTES} bytes"),
            ));
        }
        total_bytes += buffer.len();
        if total_bytes > MAX_SKILL_IMPORT_TOTAL_BYTES {
            return Err(api_error(
                StatusCode::PAYLOAD_TOO_LARGE,
                "SKILL_IMPORT_TOO_LARGE",
                format!("Skill folder exceeds {MAX_SKILL_IMPORT_TOTAL_BYTES} bytes"),
            ));
        }
        files.push(SkillImportFile { path, buffer });
    }

    let manifests: Vec<&SkillImportFile> = files
        .iter()
        .filter(|file| file.path == "SKILL.md" || file.path.ends_with("/SKILL.md"))
        .collect();
    if manifests.len() != 1 {
        return Err(bad_request(
            "INVALID_SKILL_IMPORT",
            "Skill import must contain exactly one SKILL.md",
        ));
    }
    let manifest_path = manifests[0].path.clone();
    let folder = if manifest_path == "SKILL.md" {
        String::new()
    } else {
        manifest_path[..manifest_path.len() - "/SKILL.md".len()].to_string()
    };
    if !folder.is_empty() {
        let prefix = format!("{folder}/");
        for file in &files {
            if !file.path.starts_with(&prefix) {
                return Err(bad_request(
                    "INVALID_SKILL_IMPORT_PATH",
                    "All imported files must be inside the SKILL.md folder",
                ));
            }
        }
    }
    Ok((files, manifest_path))
}

/// `importStudioSkillFolder`：manifest 解析 → 冲突检查 → staging 原子落盘。
async fn import_studio_skill_folder(
    root: &Path,
    payload: &Value,
) -> Result<AgentSkill, ApiErrorResponse> {
    let record = payload
        .as_object()
        .ok_or_else(|| bad_request("INVALID_SKILL_IMPORT", "Skill import payload must be an object"))?;
    let (files, manifest_path) = normalize_skill_import_files(record.get("files"))?;
    let manifest = files
        .iter()
        .find(|file| file.path == manifest_path)
        .expect("manifest_path 来自 files 内唯一的 SKILL.md");
    let parsed = parse_agent_skill_document(
        &String::from_utf8_lossy(&manifest.buffer),
        &root.join(&manifest_path),
        SkillSource::Project,
    )
    .map_err(|message| bad_request("INVALID_SKILL_MANIFEST", message))?;

    let target_dir = project_skill_dir(root, &parsed.id);
    match tokio::fs::try_exists(&target_dir).await {
        Ok(true) => {
            return Err(api_error(
                StatusCode::CONFLICT,
                "SKILL_EXISTS",
                format!("Project skill already exists: {}", parsed.id),
            ));
        }
        Ok(false) => {}
        Err(e) => return Err(internal_error(&e)),
    }

    tokio::fs::create_dir_all(project_skills_dir(root))
        .await
        .map_err(|e| internal_error(&e))?;
    let staging_dir = project_skills_dir(root).join(format!(".import-{}", uuid::Uuid::new_v4()));
    let folder = if manifest_path == "SKILL.md" {
        String::new()
    } else {
        manifest_path[..manifest_path.len() - "/SKILL.md".len()].to_string()
    };
    let write_all = async {
        tokio::fs::create_dir_all(&staging_dir).await?;
        for file in &files {
            let relative = if folder.is_empty() {
                file.path.as_str()
            } else {
                &file.path[folder.len() + 1..]
            };
            let destination = staging_dir.join(relative);
            if let Some(parent) = destination.parent() {
                tokio::fs::create_dir_all(parent).await?;
            }
            tokio::fs::write(&destination, &file.buffer).await?;
        }
        tokio::fs::rename(&staging_dir, &target_dir).await
    };
    if let Err(e) = write_all.await {
        let _ = tokio::fs::remove_dir_all(&staging_dir).await;
        return Err(internal_error(&e));
    }
    Ok(AgentSkill {
        base_dir: Some(target_dir.to_string_lossy().to_string()),
        ..parsed
    })
}

pub async fn import_skill(
    State(runtime): State<BooksRuntime>,
    body: Bytes,
) -> impl IntoResponse {
    let root = runtime.state.project_root();
    let Ok(payload) = serde_json::from_slice::<Value>(&body) else {
        return bad_request("INVALID_SKILL_IMPORT", "Skill import payload must be JSON");
    };
    let skill = match import_studio_skill_folder(root, &payload).await {
        Ok(skill) => skill,
        Err(response) => return response,
    };
    let project_skill_ids: HashSet<String> = HashSet::from([skill.id.clone()]);
    (
        StatusCode::OK,
        Json(json!({ "skill": to_studio_skill(&skill, root, &project_skill_ids) })),
    )
}

// ── DELETE /api/v1/skills/:skillId ─────────────────────────────

/// `normalizeStudioSkillId`：SkillIdSchema 校验（消息带 zod 默认文本）。
fn normalize_studio_skill_id(value: &str, field: &str) -> Result<String, ApiErrorResponse> {
    normalize_skill_id_strict(value).map_err(|e| {
        bad_request("INVALID_SKILL_ID", format!("Invalid {field}: {}", e.message()))
    })
}

pub async fn delete_skill(
    State(runtime): State<BooksRuntime>,
    AxumPath(skill_id): AxumPath<String>,
) -> impl IntoResponse {
    let root = runtime.state.project_root();
    let id = match normalize_studio_skill_id(&skill_id, "skillId") {
        Ok(id) => id,
        Err(response) => return response,
    };
    let path = project_skill_path(root, &id);
    // TS access 失败（含权限错）一律 404
    if !matches!(tokio::fs::try_exists(&path).await, Ok(true)) {
        return api_error(
            StatusCode::NOT_FOUND,
            "SKILL_NOT_FOUND",
            format!("Project skill not found: {id}"),
        );
    }
    if let Err(e) = tokio::fs::remove_dir_all(project_skill_dir(root, &id)).await {
        return internal_error(&e);
    }
    (StatusCode::OK, Json(json!({ "ok": true })))
}
