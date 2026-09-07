//! interactive-films / projects 域端点（69 号）——故事图谱与互动影视。
//!
//! 契约来源 `packages/studio/src/api/server.ts` L6504-L6812：
//! - `GET /interactive-films`（L6504）：目录枚举 + safe id + 图谱标题 + zh 排序
//! - `POST /projects/:id/story-graph/delta`（L6668）：`applyGraphDelta`（rev 前进 +
//!   pre-rev 快照 + per-project 串行锁，authoring-store.ts）
//! - `GET /projects/:id/story-graph`（L6678）：原样回显（不 zod 校验）
//! - `GET /projects/:id/export`（L6695）：tar.gz 归档（ustar 头逐字段对齐）
//! - `GET .../validation` / `.../analysis`：review 报告 / 情感弧线 + 路径分布
//! - `GET .../export/json|ink|html`：三种导出（html 含资产 data URI 内嵌）
//! - `POST /projects/:id/nodes/:nodeId/image`（L6789）：生图 + setImageRef delta
//!   （生图执行链未移植——生产返回 503，见偏差备案）
//!
//! 错误形态：INVALID_ID 400 `{error:{code,message}}`；NOT_FOUND 404 同形态。

use std::collections::HashMap;
use std::path::Path;
use std::sync::{Arc, Mutex, OnceLock};

use axum::body::Bytes;
use axum::extract::{Path as AxumPath, State};
use axum::http::{header, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::Json;
use serde::Deserialize;
use serde_json::{json, Value};

use crate::interactive_film::{
    self as film, StoryGraph, StoryGraphDelta,
};
use crate::interaction::session::is_safe_book_id;
use crate::server::books_routes::BooksRuntime;

fn api_error(status: StatusCode, code: &str, message: impl Into<String>) -> Response {
    (
        status,
        Json(json!({ "error": { "code": code, "message": message.into() } })),
    )
        .into_response()
}

fn invalid_id(id: &str) -> Response {
    api_error(
        StatusCode::BAD_REQUEST,
        "INVALID_ID",
        format!("invalid project id: {id}"),
    )
}

fn graph_not_found(id: &str) -> Response {
    api_error(
        StatusCode::NOT_FOUND,
        "NOT_FOUND",
        format!("story graph not found for {id}"),
    )
}

// ── authoring-store（authoring-store.ts） ────────────────────────

const SNAPSHOT_KEEP: usize = 20;

#[derive(Debug, Clone, PartialEq)]
pub struct AuthoringState {
    pub phase: &'static str,
    pub rev: i64,
}

const DEFAULT_STATE: AuthoringState = AuthoringState { phase: "world", rev: 0 };
const VALID_PHASES: &[&str] = &["world", "scale", "structure", "workshop"];

fn project_dir(project_root: &Path, project_id: &str) -> std::path::PathBuf {
    film::story_graph_path(project_root, project_id)
        .parent()
        .map(Path::to_path_buf)
        .unwrap_or_else(|| project_root.join("interactive-films").join(project_id))
}

pub fn authoring_state_path(project_root: &Path, project_id: &str) -> std::path::PathBuf {
    project_dir(project_root, project_id).join("authoring-state.json")
}

/// `loadAuthoringState`：缺失/坏字段回退默认（phase 白名单 + rev 数字判定）。
pub async fn load_authoring_state(project_root: &Path, project_id: &str) -> AuthoringState {
    let Ok(raw) = tokio::fs::read_to_string(authoring_state_path(project_root, project_id)).await
    else {
        return DEFAULT_STATE;
    };
    let Ok(parsed) = serde_json::from_str::<Value>(&raw) else {
        return DEFAULT_STATE;
    };
    let phase = parsed
        .get("phase")
        .and_then(Value::as_str)
        .filter(|p| VALID_PHASES.contains(p))
        .and_then(|p| VALID_PHASES.iter().find(|valid| **valid == p))
        .copied()
        .unwrap_or(DEFAULT_STATE.phase);
    let rev = parsed
        .get("rev")
        .and_then(Value::as_i64)
        .unwrap_or(DEFAULT_STATE.rev);
    AuthoringState { phase, rev }
}

fn snapshot_dir(project_root: &Path, project_id: &str) -> std::path::PathBuf {
    project_dir(project_root, project_id).join("snapshots")
}

/// `writeSnapshot`：pre-rev 快照 + 只保留最近 20 个（按 rev 升序删旧）。
async fn write_snapshot(project_root: &Path, project_id: &str, rev: i64, graph: &StoryGraph) {
    let dir = snapshot_dir(project_root, project_id);
    if tokio::fs::create_dir_all(&dir).await.is_err() {
        return;
    }
    let payload = format!("{}\n", serde_json::to_string_pretty(graph).unwrap_or_default());
    let _ = tokio::fs::write(dir.join(format!("{rev}.json")), payload).await;
    let mut entries = match tokio::fs::read_dir(&dir).await {
        Ok(entries) => entries,
        Err(_) => return,
    };
    let mut revs: Vec<i64> = Vec::new();
    while let Ok(Some(entry)) = entries.next_entry().await {
        let name = entry.file_name().into_string().unwrap_or_default();
        if let Some(stem) = name.strip_suffix(".json") {
            if let Ok(rev) = stem.parse::<i64>() {
                revs.push(rev);
            }
        }
    }
    revs.sort_unstable();
    let excess = revs.len().saturating_sub(SNAPSHOT_KEEP);
    for old in revs.into_iter().take(excess) {
        let _ = tokio::fs::remove_file(dir.join(format!("{old}.json"))).await;
    }
}

/// per-project 异步互斥（TS withProjectLock 的 promise 链等价）。
fn project_locks() -> &'static Mutex<HashMap<String, Arc<tokio::sync::Mutex<()>>>> {
    static LOCKS: OnceLock<Mutex<HashMap<String, Arc<tokio::sync::Mutex<()>>>>> = OnceLock::new();
    LOCKS.get_or_init(|| Mutex::new(HashMap::new()))
}

async fn with_project_lock<T>(
    project_root: &Path,
    project_id: &str,
    f: impl std::future::Future<Output = T>,
) -> T {
    let key = format!("{}::{}", project_root.display(), project_id);
    let lock = {
        let mut locks = project_locks().lock().unwrap();
        locks.entry(key).or_default().clone()
    };
    let _guard = lock.lock().await;
    f.await
}

/// `applyGraphDelta`：加载（缺失回空图谱）→ pre-rev 快照 → 应用 → 保存 →
/// rev+1 落盘 authoring-state。
pub async fn apply_graph_delta(
    project_root: &Path,
    project_id: &str,
    delta: &StoryGraphDelta,
) -> Result<(StoryGraph, i64), String> {
    with_project_lock(project_root, project_id, async {
        let current = film::load_story_graph(project_root, project_id)
            .await?
            .unwrap_or_else(|| film::empty_graph(project_id));
        let state = load_authoring_state(project_root, project_id).await;
        write_snapshot(project_root, project_id, state.rev, &current).await;

        let graph = film::apply_story_graph_delta(&current, delta)?;
        film::save_story_graph(project_root, project_id, &graph).await?;

        let next_rev = state.rev + 1;
        let next_state = json!({ "phase": state.phase, "rev": next_rev });
        let dir = project_dir(project_root, project_id);
        if tokio::fs::create_dir_all(&dir).await.is_err() {
            return Err("failed to create project dir".to_string());
        }
        let payload = format!("{}\n", serde_json::to_string_pretty(&next_state).unwrap_or_default());
        tokio::fs::write(authoring_state_path(project_root, project_id), payload)
            .await
            .map_err(|e| e.to_string())?;
        Ok((graph, next_rev))
    })
    .await
}

// ── tar 归档（server.ts buildTarArchive / listArchiveFiles） ─────

fn normalize_archive_path(path: &str) -> String {
    path.replace('\\', "/")
        .trim_start_matches('/')
        .to_string()
}

fn list_archive_files<'a>(
    dir: &'a Path,
    prefix: &'a str,
) -> std::pin::Pin<Box<dyn std::future::Future<Output = Vec<String>> + Send + 'a>> {
    Box::pin(list_archive_files_inner(dir, prefix))
}

async fn list_archive_files_inner(dir: &Path, prefix: &str) -> Vec<String> {
    let mut entries = match tokio::fs::read_dir(dir).await {
        Ok(entries) => entries,
        Err(_) => return Vec::new(),
    };
    let mut files = Vec::new();
    while let Ok(Some(entry)) = entries.next_entry().await {
        let name = entry.file_name().into_string().unwrap_or_default();
        if name == ".DS_Store" {
            continue;
        }
        let relative = if prefix.is_empty() {
            name.clone()
        } else {
            format!("{prefix}/{name}")
        };
        let full = dir.join(&name);
        // Dirent 判定 + symlink 兜底（TS 第三分支 stat 跟随链接语义）。
        let meta = tokio::fs::metadata(&full).await;
        if meta.as_ref().is_ok_and(|m| m.is_dir()) {
            files.extend(list_archive_files(&full, &relative).await);
        } else if meta.as_ref().is_ok_and(|m| m.is_file()) {
            files.push(normalize_archive_path(&relative));
        }
    }
    files.sort();
    files
}

fn write_tar_string(header: &mut [u8; 512], offset: usize, length: usize, value: &str) -> Result<(), String> {
    let encoded = value.as_bytes();
    if encoded.len() > length {
        return Err(format!("Archive path is too long for tar header: {value}"));
    }
    header[offset..offset + encoded.len()].copy_from_slice(encoded);
    Ok(())
}

fn write_tar_octal(header: &mut [u8; 512], offset: usize, length: usize, value: u64) {
    let text = format!("{:o}", value);
    let width = length - 1;
    let padded = if text.len() >= width {
        text[text.len() - width..].to_string()
    } else {
        format!("{:0width$}", text.parse::<u64>().unwrap_or(0), width = width)
            .chars()
            .rev()
            .take(width)
            .collect::<Vec<_>>()
            .into_iter()
            .rev()
            .collect()
    };
    let bytes = padded.as_bytes();
    header[offset..offset + bytes.len()].copy_from_slice(bytes);
    header[offset + length - 1] = 0;
}

fn create_tar_header(name: &str, size: usize) -> Result<[u8; 512], String> {
    let mut header = [0u8; 512];
    write_tar_string(&mut header, 0, 100, name)?;
    write_tar_octal(&mut header, 100, 8, 0o644);
    write_tar_octal(&mut header, 108, 8, 0);
    write_tar_octal(&mut header, 116, 8, 0);
    write_tar_octal(&mut header, 124, 12, size as u64);
    let mtime = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    write_tar_octal(&mut header, 136, 12, mtime);
    // checksum 字段先置空格再算和（TS fill(0x20)）。
    for byte in header.iter_mut().skip(148).take(8) {
        *byte = 0x20;
    }
    header[156] = b'0';
    write_tar_string(&mut header, 257, 6, "ustar")?;
    write_tar_string(&mut header, 263, 2, "00")?;
    let checksum: u32 = header.iter().map(|b| *b as u32).sum();
    write_tar_octal(&mut header, 148, 8, checksum as u64);
    Ok(header)
}

/// `buildTarArchive`：512 头 + 内容 + padding + 1024 尾块。
async fn build_tar_archive(source_dir: &Path, package_root_name: &str) -> Result<Vec<u8>, String> {
    let files = list_archive_files(source_dir, "").await;
    let mut chunks: Vec<u8> = Vec::new();
    for file in files {
        let payload = tokio::fs::read(source_dir.join(&file))
            .await
            .map_err(|e| e.to_string())?;
        let archive_name = normalize_archive_path(&format!("{package_root_name}/{file}"));
        chunks.extend_from_slice(&create_tar_header(&archive_name, payload.len())?);
        chunks.extend_from_slice(&payload);
        let padding = (512 - (payload.len() % 512)) % 512;
        chunks.extend(std::iter::repeat_n(0u8, padding));
    }
    chunks.extend(std::iter::repeat_n(0u8, 1024));
    Ok(chunks)
}

// ── GET /interactive-films ──────────────────────────────────────

pub async fn list_interactive_films(
    State(runtime): State<BooksRuntime>,
) -> impl IntoResponse {
    let root = runtime.state.project_root();
    let films_dir = root.join("interactive-films");
    let mut entries: Vec<String> = Vec::new();
    if let Ok(mut dirents) = tokio::fs::read_dir(&films_dir).await {
        while let Ok(Some(entry)) = dirents.next_entry().await {
            if tokio::fs::metadata(films_dir.join(entry.file_name()))
                .await
                .map(|m| m.is_dir())
                .unwrap_or(false)
            {
                if let Ok(name) = entry.file_name().into_string() {
                    entries.push(name);
                }
            }
        }
    }
    let mut films: Vec<(String, String)> = Vec::new();
    for project_id in entries {
        if !is_safe_book_id(&project_id) {
            continue;
        }
        // 坏图谱（zod 失败）跳过——TS catch 同语义。
        if let Ok(Some(graph)) = film::load_story_graph(root, &project_id).await {
            let title = if graph.title.is_empty() {
                project_id.clone()
            } else {
                graph.title
            };
            films.push((project_id, title));
        }
    }
    // TS localeCompare(zh)：拼音序需 ICU；Rust 按码点序（前端按标题展示，
    // 排序差仅在同名首字不同拼音的场景，偏差备案）。
    films.sort_by(|a, b| a.1.cmp(&b.1));
    (
        StatusCode::OK,
        Json(json!({
            "films": films
                .into_iter()
                .map(|(project_id, title)| json!({ "projectId": project_id, "title": title }))
                .collect::<Vec<_>>()
        })),
    )
}

// ── POST /projects/:id/story-graph/delta ────────────────────────

pub async fn post_story_graph_delta(
    State(runtime): State<BooksRuntime>,
    AxumPath(id): AxumPath<String>,
    body: Bytes,
) -> impl IntoResponse {
    if !is_safe_book_id(&id) {
        return invalid_id(&id);
    }
    // TS 直接取 body.delta（undefined 时 zod parse 报错 → 500）。
    let parsed: Result<Value, _> = serde_json::from_slice(&body);
    let Ok(payload) = parsed else {
        return api_error(
            StatusCode::INTERNAL_SERVER_ERROR,
            "INTERNAL_ERROR",
            "Unexpected server error.",
        );
    };
    let delta_value = payload.get("delta").cloned().unwrap_or(Value::Null);
    let Ok(delta) = serde_json::from_value::<StoryGraphDelta>(delta_value) else {
        return api_error(
            StatusCode::INTERNAL_SERVER_ERROR,
            "INTERNAL_ERROR",
            "Unexpected server error.",
        );
    };
    let root = runtime.state.project_root();
    match apply_graph_delta(root, &id, &delta).await {
        Ok((graph, rev)) => {
            let graph_json = serde_json::to_value(&graph).unwrap_or(Value::Null);
            (StatusCode::OK, Json(json!({ "rev": rev, "graph": graph_json }))).into_response()
        }
        Err(message) => api_error(StatusCode::INTERNAL_SERVER_ERROR, "INTERNAL_ERROR", message),
    }
}

// ── GET /projects/:id/story-graph（原样回显） ────────────────────

pub async fn get_story_graph(
    State(runtime): State<BooksRuntime>,
    AxumPath(id): AxumPath<String>,
) -> impl IntoResponse {
    if !is_safe_book_id(&id) {
        return invalid_id(&id);
    }
    let path = film::story_graph_path(runtime.state.project_root(), &id);
    match tokio::fs::read_to_string(&path).await {
        Ok(raw) => {
            let value: Value = serde_json::from_str(&raw).unwrap_or(Value::Null);
            (StatusCode::OK, Json(value)).into_response()
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => graph_not_found(&id),
        Err(e) => api_error(StatusCode::INTERNAL_SERVER_ERROR, "INTERNAL_ERROR", e.to_string()),
    }
}

// ── GET /projects/:id/export（tar.gz） ───────────────────────────

pub async fn get_project_export(
    State(runtime): State<BooksRuntime>,
    AxumPath(id): AxumPath<String>,
) -> impl IntoResponse {
    if !is_safe_book_id(&id) {
        return invalid_id(&id);
    }
    let project_dir = runtime.state.project_root().join("interactive-films").join(&id);
    if tokio::fs::metadata(&project_dir).await.is_err() {
        return api_error(
            StatusCode::NOT_FOUND,
            "NOT_FOUND",
            format!("interactive film project not found for {id}"),
        );
    }
    let tar = match build_tar_archive(&project_dir, &id).await {
        Ok(tar) => tar,
        Err(message) => return api_error(StatusCode::INTERNAL_SERVER_ERROR, "INTERNAL_ERROR", message),
    };
    let mut encoder = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::default());
    use std::io::Write as _;
    if encoder.write_all(&tar).is_err() {
        return api_error(StatusCode::INTERNAL_SERVER_ERROR, "INTERNAL_ERROR", "gzip failed");
    }
    let Ok(gzip) = encoder.finish() else {
        return api_error(StatusCode::INTERNAL_SERVER_ERROR, "INTERNAL_ERROR", "gzip failed");
    };
    let filename = crate::server::task_store::js_encode_uri_component(&id);
    (
        StatusCode::OK,
        [
            (header::CONTENT_TYPE, "application/gzip".to_string()),
            (
                header::CONTENT_DISPOSITION,
                format!("attachment; filename=\"{filename}.tar.gz\""),
            ),
        ],
        gzip,
    )
        .into_response()
}

// ── 校验 / 分析 ─────────────────────────────────────────────────

async fn loaded_graph(runtime: &BooksRuntime, id: &str) -> Result<StoryGraph, Response> {
    match film::load_story_graph(runtime.state.project_root(), id).await {
        Ok(Some(graph)) => Ok(graph),
        Ok(None) => Err(graph_not_found(id)),
        Err(message) => Err(api_error(StatusCode::INTERNAL_SERVER_ERROR, "INTERNAL_ERROR", message)),
    }
}

pub async fn get_story_graph_validation(
    State(runtime): State<BooksRuntime>,
    AxumPath(id): AxumPath<String>,
) -> impl IntoResponse {
    if !is_safe_book_id(&id) {
        return invalid_id(&id);
    }
    match loaded_graph(&runtime, &id).await {
        Ok(graph) => (
            StatusCode::OK,
            Json(serde_json::to_value(film::review_story_graph(&graph)).unwrap_or(Value::Null)),
        )
            .into_response(),
        Err(response) => response,
    }
}

pub async fn get_story_graph_analysis(
    State(runtime): State<BooksRuntime>,
    AxumPath(id): AxumPath<String>,
) -> impl IntoResponse {
    if !is_safe_book_id(&id) {
        return invalid_id(&id);
    }
    match loaded_graph(&runtime, &id).await {
        Ok(graph) => {
            let report = serde_json::to_value(film::review_story_graph(&graph))
                .unwrap_or(Value::Null);
            let (arcs, _) = film::analyze_emotional_arcs(&graph);
            (
                StatusCode::OK,
                Json(json!({
                    "report": report,
                    "arcs": arcs,
                    "distribution": film::analyze_path_distribution(&graph),
                })),
            )
                .into_response()
        }
        Err(response) => response,
    }
}

// ── 三种导出 ────────────────────────────────────────────────────

fn attachment_disposition(filename: &str) -> String {
    let safe_ascii: String = filename
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-') {
                c
            } else {
                '_'
            }
        })
        .collect();
    let encoded = crate::server::task_store::js_encode_uri_component(filename);
    format!("attachment; filename=\"{safe_ascii}\"; filename*=UTF-8''{encoded}")
}

fn text_response(body: String, content_type: &str, disposition_filename: &str) -> Response {
    (
        StatusCode::OK,
        [
            (header::CONTENT_TYPE, content_type.to_string()),
            (
                header::CONTENT_DISPOSITION,
                attachment_disposition(disposition_filename),
            ),
        ],
        body,
    )
        .into_response()
}

pub async fn get_export_json(
    State(runtime): State<BooksRuntime>,
    AxumPath(id): AxumPath<String>,
) -> impl IntoResponse {
    if !is_safe_book_id(&id) {
        return invalid_id(&id);
    }
    match loaded_graph(&runtime, &id).await {
        Ok(graph) => text_response(
            format!("{}\n", serde_json::to_string_pretty(&graph).unwrap_or_default()),
            "application/json; charset=utf-8",
            &format!("{id}.story-graph.json"),
        ),
        Err(response) => response,
    }
}

pub async fn get_export_ink(
    State(runtime): State<BooksRuntime>,
    AxumPath(id): AxumPath<String>,
) -> impl IntoResponse {
    if !is_safe_book_id(&id) {
        return invalid_id(&id);
    }
    match loaded_graph(&runtime, &id).await {
        Ok(graph) => text_response(
            film::export_ink(&graph),
            "text/plain; charset=utf-8",
            &format!("{id}.ink"),
        ),
        Err(response) => response,
    }
}

pub async fn get_export_html(
    State(runtime): State<BooksRuntime>,
    AxumPath(id): AxumPath<String>,
) -> impl IntoResponse {
    if !is_safe_book_id(&id) {
        return invalid_id(&id);
    }
    match loaded_graph(&runtime, &id).await {
        Ok(graph) => {
            // 资产 data URI 内嵌（坏资产跳过——TS console.warn 同语义）。
            let root = runtime.state.project_root();
            let mut asset_data_uris = std::collections::BTreeMap::new();
            for node in &graph.nodes {
                let Some(reference) = node
                    .image_slot
                    .as_ref()
                    .and_then(|slot| slot.asset_ref.as_deref())
                else {
                    continue;
                };
                if asset_data_uris.contains_key(reference) {
                    continue;
                }
                let resolved = crate::server::project_files_routes::resolve_project_image_file(
                    root, reference,
                );
                if let Ok((path, content_type)) = resolved {
                    if let Ok(bytes) = tokio::fs::read(&path).await {
                        use base64::Engine as _;
                        asset_data_uris.insert(
                            reference.to_string(),
                            format!(
                                "data:{content_type};base64,{}",
                                base64::engine::general_purpose::STANDARD.encode(bytes)
                            ),
                        );
                        continue;
                    }
                }
            }
            text_response(
                film::build_playable_html(&graph, &asset_data_uris),
                "text/html; charset=utf-8",
                &format!("{id}.html"),
            )
        }
        Err(response) => response,
    }
}

// ── POST /projects/:id/nodes/:nodeId/image ──────────────────────

#[derive(Debug, Default, Deserialize)]
pub struct NodeImageBody {
    #[serde(default)]
    pub size: Option<String>,
}

pub async fn post_node_image(
    State(runtime): State<BooksRuntime>,
    AxumPath((id, node_id)): AxumPath<(String, String)>,
    body: Option<Json<NodeImageBody>>,
) -> impl IntoResponse {
    if !is_safe_book_id(&id) {
        return invalid_id(&id);
    }
    let graph = match film::load_story_graph(runtime.state.project_root(), &id).await {
        Ok(Some(graph)) => graph,
        Ok(None) => {
            return api_error(
                StatusCode::NOT_FOUND,
                "NODE_NOT_FOUND",
                format!("node {node_id} not found"),
            )
        }
        Err(message) => return api_error(StatusCode::INTERNAL_SERVER_ERROR, "INTERNAL_ERROR", message),
    };
    if !graph.nodes.iter().any(|n| n.id == node_id) {
        return api_error(
            StatusCode::NOT_FOUND,
            "NODE_NOT_FOUND",
            format!("node {node_id} not found"),
        );
    }
    // 生图（74 号接通 cover 基础设施）：prompt 取 imageSlot.prompt 或
    // sceneDesc；落盘 run/images 相对路径后经 setImageRef delta 写回图谱。
    let node = graph
        .nodes
        .iter()
        .find(|n| n.id == node_id)
        .cloned()
        .expect("上方已验证存在");
    let prompt = node
        .image_slot
        .as_ref()
        .map(|slot| slot.prompt.trim().to_string())
        .filter(|p| !p.is_empty())
        .unwrap_or_else(|| node.scene_desc.trim().to_string());
    if prompt.is_empty() {
        // TS generateNodeImage 直接 throw（端点 onError → 500）；消息逐字。
        return api_error(
            StatusCode::INTERNAL_SERVER_ERROR,
            "INTERNAL_ERROR",
            format!("node {node_id} has no imageSlot.prompt or sceneDesc to generate an image from"),
        );
    }
    let root = runtime.state.project_root();
    let request = match crate::llm::cover::resolve_cover_generation_request(root).await {
        Ok(request) => request,
        Err(message) => {
            return (
                StatusCode::BAD_REQUEST,
                Json(json!({ "error": message, "needsCoverConfig": true })),
            )
                .into_response()
        }
    };
    // 尺寸链（TS node-image 逐字）：body.size ?? env INKOS_FILM_IMAGE_SIZE ?? "1024x1536"。
    let size: String = body
        .as_ref()
        .and_then(|body| body.size.clone())
        .filter(|s| !s.is_empty())
        .or_else(|| {
            std::env::var("INKOS_FILM_IMAGE_SIZE")
                .ok()
                .filter(|s| !s.is_empty())
        })
        .unwrap_or_else(|| "1024x1536".to_string());
    let image = match crate::llm::cover::generate_image_from_prompt(&request, &prompt, &size).await
    {
        Ok(image) => image,
        Err(message) => {
            return api_error(StatusCode::INTERNAL_SERVER_ERROR, "IMAGE_GENERATION_FAILED", message)
        }
    };
    let asset_ref = film::node_image_rel_path(&id, &node_id, image.extension);
    let abs = root.join(&asset_ref);
    if let Some(parent) = abs.parent() {
        if let Err(message) = tokio::fs::create_dir_all(parent).await {
            return api_error(StatusCode::INTERNAL_SERVER_ERROR, "INTERNAL_ERROR", message.to_string());
        }
    }
    if let Err(message) = tokio::fs::write(&abs, &image.bytes).await {
        return api_error(StatusCode::INTERNAL_SERVER_ERROR, "INTERNAL_ERROR", message.to_string());
    }
    // buildSetImageRefDelta：upsert 节点 + imageSlot {prompt, assetRef}。
    let mut node_with_image = node.clone();
    node_with_image.image_slot = Some(film::ImageSlot {
        prompt: prompt.clone(),
        asset_ref: Some(asset_ref.clone()),
    });
    let delta = match serde_json::from_value::<film::StoryGraphDelta>(json!({
        "nodes": { "upsert": [serde_json::to_value(&node_with_image).unwrap_or(Value::Null)] }
    })) {
        Ok(delta) => delta,
        Err(e) => return api_error(StatusCode::INTERNAL_SERVER_ERROR, "INTERNAL_ERROR", e.to_string()),
    };
    match apply_graph_delta(root, &id, &delta).await {
        Ok((_, rev)) => (
            StatusCode::OK,
            Json(json!({ "assetRef": asset_ref, "rev": rev })),
        )
            .into_response(),
        Err(message) => api_error(StatusCode::INTERNAL_SERVER_ERROR, "INTERNAL_ERROR", message),
    }
}
