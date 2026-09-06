//! 系列书回填端点（184 号 C3-b）。对标 Sudowrite Story Bible「系列书从既有
//! 正文回填设定」，交互按 179 号分解文档默认「预览 + 逐项勾选」。
//!
//! - `POST /api/v1/books/:id/series-backfill/extract`：body `{sourceBookId}`。
//!   读源书 canon（story_bible / outline/story_frame / roles/**），经 LLM 摘要
//!   为结构化 items，落盘目标书 `story/series_backfill_draft.json` 并返回。
//!   同步长请求（单次 LLM 调用）——结果草稿落盘即检查点，失败重试零损失
//!   （不引入异步任务架构的理由见变更记录）。
//! - `POST /api/v1/books/:id/series-backfill/apply`：body `{itemIds?}`（缺省
//!   全量）。把草稿勾选子集渲染为 `story/series_backfill.md`（独立文件，
//!   不改写既有真相文件；不确认则零写入）。
//!
//! TS 回退端同款端点见 server.ts（本批双端实现）。

use axum::body::Bytes;
use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::response::IntoResponse;
use axum::Json;
use serde::Deserialize;
use serde_json::json;

use crate::llm::provider::{LLMMessage, LLMRole};
use crate::models::series_backfill::{SeriesBackfillDraft, SeriesBackfillItem};
use crate::server::books_routes::BooksRuntime;

const MAX_CANON_CHARS_PER_FILE: usize = 6_000;

fn internal_error(message: &str) -> (StatusCode, Json<serde_json::Value>) {
    (StatusCode::INTERNAL_SERVER_ERROR, Json(json!({ "error": message })))
}

fn bad_request(message: &str) -> (StatusCode, Json<serde_json::Value>) {
    (StatusCode::BAD_REQUEST, Json(json!({ "error": message })))
}

/// 读源书 canon 文件（存在则读、截断到上限；缺失静默跳过）。
async fn read_source_canon(book_dir: &std::path::Path) -> Vec<(String, String)> {
    let mut files: Vec<(String, String)> = Vec::new();
    for rel in ["story/story_bible.md", "story/outline/story_frame.md"] {
        if let Ok(content) = tokio::fs::read_to_string(book_dir.join(rel)).await {
            files.push((rel.to_string(), content));
        }
    }
    // roles 目录（新旧两种布局）。
    for role_dir in ["story/roles/主要角色", "story/roles/次要角色", "story/roles/major", "story/roles/minor"] {
        let dir = book_dir.join(role_dir);
        let Ok(mut entries) = tokio::fs::read_dir(&dir).await else {
            continue;
        };
        while let Ok(Some(entry)) = entries.next_entry().await {
            let name = entry.file_name().to_string_lossy().into_owned();
            if !(name.ends_with(".md") || name.ends_with(".json")) {
                continue;
            }
            if let Ok(content) = tokio::fs::read_to_string(dir.join(&name)).await {
                files.push((format!("{role_dir}/{name}"), content));
            }
        }
    }
    files
        .into_iter()
        .map(|(name, content)| {
            let truncated: String = content.chars().take(MAX_CANON_CHARS_PER_FILE).collect();
            (name, truncated)
        })
        .collect()
}

fn build_extraction_prompt(source_title: &str, canon: &[(String, String)]) -> String {
    let mut user = format!(
        "以下是一部已完结系列作品《{source_title}》的设定文件（故事圣经 / 世界观框架 / 人物卡）。\
请为同系列的新书抽取可迁移的设定，输出**纯 JSON**（不要 markdown 围栏、不要解释文字），形如：\n\
{{\"items\": [{{\"id\": \"it-1\", \"category\": \"worldview\", \"title\": \"标题\", \"content\": \"50-200字的具体设定描述\"}}]}}\n\
category 只能取 worldview / character / plot / style 之一；抽取 6-12 条最有迁移价值的设定。\n\n源书设定文件：\n"
    );
    for (name, content) in canon {
        user.push_str(&format!("\n--- {name} ---\n{content}\n"));
    }
    user
}

/// POST /api/v1/books/:id/series-backfill/extract
pub async fn extract(
    State(runtime): State<BooksRuntime>,
    Path(book_id): Path<String>,
    body: Bytes,
) -> impl IntoResponse {
    let parsed: serde_json::Value = serde_json::from_slice(&body).unwrap_or_else(|_| json!({}));
    let Some(source_book_id) = parsed.get("sourceBookId").and_then(serde_json::Value::as_str) else {
        return bad_request("sourceBookId is required");
    };
    if source_book_id == book_id {
        return bad_request("Source and target books must differ");
    }
    // 源书与目标书都必须存在。
    if runtime.state.load_book_config(source_book_id).await.is_err() {
        return bad_request("Source book not found");
    }
    if runtime.state.load_book_config(&book_id).await.is_err() {
        return bad_request("Target book not found");
    }

    let source_title: String = runtime
        .state
        .load_book_config(source_book_id)
        .await
        .ok()
        .map(|b| b.title)
        .unwrap_or_else(|| source_book_id.to_string());
    let source_dir = runtime.state.book_dir(source_book_id);
    let canon = read_source_canon(&source_dir).await;
    if canon.is_empty() {
        return bad_request("Source book has no canon files (story_bible / story_frame / roles)");
    }

    let system: String = "你是网文系列的设定编辑。只输出纯 JSON，不要 markdown 围栏或任何解释文字。".to_string();
    let user: String = build_extraction_prompt(&source_title, &canon);
    let router = (*runtime.effective_router().await).clone();
    let outcome = router
        .chat(
            "inspiration",
            vec![
                LLMMessage { role: LLMRole::System, content: system, tool_calls: None, tool_call_id: None },
                LLMMessage { role: LLMRole::User, content: user, tool_calls: None, tool_call_id: None },
            ],
            0.3,
            Some(2_000),
        )
        .await;
    let content = match outcome {
        Ok(outcome) => outcome.content,
        Err(error) => return internal_error(&error.to_string()),
    };
    let draft = match SeriesBackfillDraft::parse_items_text(&book_id, source_book_id, &content) {
        Ok(draft) => draft,
        Err(message) => return internal_error(&message),
    };

    // 草稿落盘 = 抽取检查点（失败重试零损失）。
    let path = runtime
        .state
        .book_dir(&book_id)
        .join("story")
        .join("series_backfill_draft.json");
    if let Some(parent) = path.parent() {
        let _ = tokio::fs::create_dir_all(parent).await;
    }
    let mut serialized = match serde_json::to_string_pretty(&draft) {
        Ok(text) => text,
        Err(error) => return internal_error(&error.to_string()),
    };
    serialized.push('\n');
    if let Err(error) = tokio::fs::write(&path, serialized).await {
        return internal_error(&error.to_string());
    }
    (StatusCode::OK, Json(json!({ "draft": draft })))
}

#[derive(Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ApplyBody {
    /// 勾选的条目 id；缺省 = 全量（「预览 + 逐项勾选」的确认动作）。
    #[serde(default)]
    pub item_ids: Option<Vec<String>>,
}

/// 把 items 渲染为 markdown 分节（独立文件，不改写既有真相文件）。
pub fn render_backfill_markdown(draft: &SeriesBackfillDraft, items: &[SeriesBackfillItem]) -> String {
    let mut out = format!(
        "# 系列设定回填\n\n来源：《{}》（{}） · 抽取于 {} · 勾选 {} 条\n\n",
        draft.source_book_id, draft.source_book_id, draft.updated_at, items.len()
    );
    for item in items {
        out.push_str(&format!("## [{}] {}\n\n{}\n\n", item.category, item.title, item.content));
    }
    out
}

/// GET /api/v1/books/:id/series-backfill/existing
/// 读目标书现有 series_backfill.md（缺失 → `{"content":null}`）——
/// 186 号 diff 预览的数据源（前端把「现有内容」与「将写入内容」做行级 diff）。
pub async fn existing(
    State(runtime): State<BooksRuntime>,
    Path(book_id): Path<String>,
) -> impl IntoResponse {
    let path = runtime.state.book_dir(&book_id).join("story").join("series_backfill.md");
    match tokio::fs::read_to_string(&path).await {
        Ok(content) => (StatusCode::OK, Json(json!({ "content": content }))),
        Err(_) => (StatusCode::OK, Json(json!({ "content": null }))),
    }
}

/// POST /api/v1/books/:id/series-backfill/apply
pub async fn apply(
    State(runtime): State<BooksRuntime>,
    Path(book_id): Path<String>,
    body: Bytes,
) -> impl IntoResponse {
    let parsed: ApplyBody = serde_json::from_slice(&body).unwrap_or_default();
    let draft_path = runtime
        .state
        .book_dir(&book_id)
        .join("story")
        .join("series_backfill_draft.json");
    let Ok(raw) = tokio::fs::read_to_string(&draft_path).await else {
        return bad_request("No backfill draft found; run extract first");
    };
    let Ok(draft) = serde_json::from_str::<SeriesBackfillDraft>(&raw) else {
        return bad_request("Backfill draft is corrupt; run extract again");
    };
    let items: Vec<SeriesBackfillItem> = match &parsed.item_ids {
        Some(ids) => draft.items.iter().filter(|item| ids.contains(&item.id)).cloned().collect(),
        None => draft.items.clone(),
    };
    if items.is_empty() {
        return bad_request("No items selected");
    }
    let markdown = render_backfill_markdown(&draft, &items);
    let path = runtime.state.book_dir(&book_id).join("story").join("series_backfill.md");
    if let Err(error) = tokio::fs::write(&path, &markdown).await {
        return internal_error(&error.to_string());
    }
    (
        StatusCode::OK,
        Json(json!({
            "ok": true,
            // 相对路径（185 号 duel：绝对路径属服务器侧细节，不入契约）。
            "path": "story/series_backfill.md",
            "applied": items.len(),
        })),
    )
}
