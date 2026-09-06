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
category 只能取 worldview / character / plot / style / faction / item / location 之一\
（faction=势力组织，item=关键物品法宝，location=重要地点场景）；抽取 8-14 条最有迁移价值的设定。\n\n源书设定文件：\n"
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
    /// 190 号：写入粒度——`overwrite`（默认，整体覆盖）| `merge`（保留既有
    /// 条目，勾选项按 (category, title) 去重后追加）。
    #[serde(default)]
    pub mode: Option<String>,
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

/// 解析 render_backfill_markdown 产物中的条目（190 号 merge 模式数据源）。
///
/// 只承诺解析**本端点机器生成**的格式：`## [category] title` 行开新条目，
/// 其后到下一标题/EOF 之间的原始行为 content（首尾空白裁掉）；首个条目前的
/// 头部行忽略。条目 id 不在文件中——合成 `existing-N` 占位（渲染不消费 id）。
pub fn parse_backfill_items(content: &str) -> Vec<SeriesBackfillItem> {
    let mut items: Vec<SeriesBackfillItem> = Vec::new();
    let mut current: Option<(String, String, Vec<&str>)> = None;
    for line in content.lines() {
        if let Some(rest) = line.strip_prefix("## [") {
            if let Some(close) = rest.find("] ") {
                if let Some((category, title, lines)) = current.take() {
                    items.push(SeriesBackfillItem {
                        id: format!("existing-{}", items.len() + 1),
                        category,
                        title,
                        content: lines.join("\n").trim().to_string(),
                    });
                }
                current = Some((
                    rest[..close].to_string(),
                    rest[close + 2..].to_string(),
                    Vec::new(),
                ));
                continue;
            }
        }
        if let Some((_, _, lines)) = current.as_mut() {
            lines.push(line);
        }
    }
    if let Some((category, title, lines)) = current.take() {
        items.push(SeriesBackfillItem {
            id: format!("existing-{}", items.len() + 1),
            category,
            title,
            content: lines.join("\n").trim().to_string(),
        });
    }
    items
}

/// merge 写入集合：既有条目在前，勾选项按 (category, title) 去重后追加。
pub fn merge_backfill_items(
    existing: &[SeriesBackfillItem],
    selected: &[SeriesBackfillItem],
) -> Vec<SeriesBackfillItem> {
    let mut merged: Vec<SeriesBackfillItem> = existing.to_vec();
    for item in selected {
        let duplicate = merged
            .iter()
            .any(|e| e.category == item.category && e.title == item.title);
        if !duplicate {
            merged.push(item.clone());
        }
    }
    merged
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
    let mode = parsed.mode.as_deref().unwrap_or("overwrite");
    let merged: Vec<SeriesBackfillItem> = if mode == "merge" {
        let existing_path = runtime
            .state
            .book_dir(&book_id)
            .join("story")
            .join("series_backfill.md");
        let existing = match tokio::fs::read_to_string(&existing_path).await {
            Ok(content) => parse_backfill_items(&content),
            Err(_) => Vec::new(),
        };
        merge_backfill_items(&existing, &items)
    } else {
        items.clone()
    };
    let markdown = render_backfill_markdown(&draft, &merged);
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
            // 190 号：merge 模式回传合并后的总条数（= 文件内条目数）。
            "total": merged.len(),
            "mode": mode,
        })),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::llm::agent_router::{AgentRouter, LlmEndpointConfig};
    use crate::models::series_backfill::SeriesBackfillDraft;
    use crate::pipeline::merged_audit::RevisionGate;
    use crate::server::books_routes::BooksRuntime;
    use crate::server::sse::BroadcastHub;
    use crate::state::manager::StateManager;
    use axum::routing::post;
    use std::sync::Arc;
    use tower::util::ServiceExt;

    fn runtime_for(root: &std::path::Path) -> BooksRuntime {
        BooksRuntime {
            hub: Arc::new(BroadcastHub::new()),
            state: Arc::new(StateManager::new(root)),
            router: Arc::new(AgentRouter::new(
                LlmEndpointConfig {
                    base_url: "http://127.0.0.1:9".into(),
                    api_key: "k".into(),
                    model: "m".into(),
                    max_tokens: 1024,
                    extra_headers: Default::default(),
                },
                Default::default(),
            )),
            builtin_genres_dir: root.to_path_buf(),
            revision_gate: RevisionGate::default(),
        }
    }

    fn app(runtime: BooksRuntime) -> axum::Router {
        axum::Router::new()
            .route(
                "/api/v1/books/:id/series-backfill/apply",
                post(apply),
            )
            .route(
                "/api/v1/books/:id/series-backfill/existing",
                axum::routing::get(existing),
            )
            .with_state(runtime)
    }

    fn item(id: &str, category: &str, title: &str, content: &str) -> SeriesBackfillItem {
        SeriesBackfillItem {
            id: id.into(),
            category: category.into(),
            title: title.into(),
            content: content.into(),
        }
    }

    fn draft_with(items: &[SeriesBackfillItem]) -> SeriesBackfillDraft {
        serde_json::from_value(json!({
            "version": 1,
            "bookId": "b1",
            "sourceBookId": "src",
            "updatedAt": "2026-09-07T00:00:00.000Z",
            "items": items,
        }))
        .unwrap()
    }

    #[test]
    fn render_parse_roundtrip_preserves_items() {
        let draft = draft_with(&[
            item("it-1", "worldview", "灵气体系", "灵气分五行。"),
            item("it-2", "character", "林动", "主角，坚韧。"),
        ]);
        let markdown = render_backfill_markdown(&draft, &draft.items);
        let parsed = parse_backfill_items(&markdown);
        assert_eq!(parsed.len(), 2);
        assert_eq!(parsed[0].category, "worldview");
        assert_eq!(parsed[0].title, "灵气体系");
        assert_eq!(parsed[0].content, "灵气分五行。");
        assert_eq!(parsed[1].title, "林动");
        // 渲染不消费 id；解析合成 existing-N。
        assert_eq!(parsed[0].id, "existing-1");
        // 再渲染一次与原文件逐字一致（roundtrip 收敛）。
        assert_eq!(render_backfill_markdown(&draft, &parsed), markdown);
    }

    #[test]
    fn parse_ignores_header_and_multiline_content() {
        let content = "# 系列设定回填\n\n来源：《src》（src） · 抽取于 T · 勾选 1 条\n\n## [plot] 夺符\n\n第一段。\n\n第二段。\n\n";
        let parsed = parse_backfill_items(content);
        assert_eq!(parsed.len(), 1);
        assert_eq!(parsed[0].title, "夺符");
        assert_eq!(parsed[0].content, "第一段。\n\n第二段。");
    }

    #[test]
    fn merge_dedupes_by_category_and_title() {
        let existing = vec![
            item("existing-1", "worldview", "灵气体系", "旧描述。"),
        ];
        let selected = vec![
            item("it-1", "worldview", "灵气体系", "新描述。"), // 同 (category,title) → 跳过
            item("it-2", "character", "林动", "主角。"),
        ];
        let merged = merge_backfill_items(&existing, &selected);
        assert_eq!(merged.len(), 2);
        assert_eq!(merged[0].content, "旧描述。"); // 既有在前且保留
        assert_eq!(merged[1].id, "it-2");
    }

    /// 194 号：类别扩到 7 类（faction/item/location 新增），条数指引 8-14。
    #[test]
    fn extraction_prompt_lists_all_categories_and_counts() {
        let prompt = build_extraction_prompt("源书", &[("story/story_bible.md".into(), "设定正文".into())]);
        for category in ["worldview", "character", "plot", "style", "faction", "item", "location"] {
            assert!(prompt.contains(category), "prompt 应包含类别 {category}");
        }
        assert!(prompt.contains("8-14 条"));
        assert!(prompt.contains("story_bible.md"));
        assert!(prompt.contains("设定正文"));
    }

    #[tokio::test]
    async fn apply_merge_appends_to_existing_file() {
        let dir = tempfile::tempdir().unwrap();
        let book = dir.path().join("books").join("b1");
        std::fs::create_dir_all(book.join("story")).unwrap();
        std::fs::write(
            book.join("book.json"),
            r#"{"id":"b1","title":"t","platform":"other","genre":"other","status":"active","targetChapters":10,"chapterWordCount":3000,"language":"zh","createdAt":"","updatedAt":""}"#,
        )
        .unwrap();
        // 既有回填文件：一条 worldview 条目。
        std::fs::write(
            book.join("story").join("series_backfill.md"),
            "# 系列设定回填\n\n来源：《old》（old） · 抽取于 T · 勾选 1 条\n\n## [worldview] 灵气体系\n\n旧描述。\n\n",
        )
        .unwrap();
        // 新草稿：一条同题（去重）+ 一条新题。
        std::fs::write(
            book.join("story").join("series_backfill_draft.json"),
            serde_json::to_string(&draft_with(&[
                item("it-1", "worldview", "灵气体系", "新描述。"),
                item("it-2", "character", "林动", "主角。"),
            ]))
            .unwrap(),
        )
        .unwrap();

        let response = app(runtime_for(dir.path()))
            .oneshot(
                axum::http::Request::builder()
                    .method("POST")
                    .uri("/api/v1/books/b1/series-backfill/apply")
                    .header("content-type", "application/json")
                    .body(axum::body::Body::from(r#"{"itemIds":["it-1","it-2"],"mode":"merge"}"#))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        let bytes = axum::body::to_bytes(response.into_body(), 1 << 20).await.unwrap();
        let parsed: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(parsed["mode"], "merge");
        assert_eq!(parsed["applied"], 2); // 勾选 2 条
        assert_eq!(parsed["total"], 2); // 合并后 2 条（同题去重）

        let content = std::fs::read_to_string(book.join("story").join("series_backfill.md")).unwrap();
        assert!(content.contains("旧描述。")); // 既有条目保留
        assert!(content.contains("## [character] 林动"));
        assert!(!content.contains("新描述。")); // 同题新内容不覆盖旧条目
    }

    #[tokio::test]
    async fn apply_default_mode_stays_overwrite() {
        let dir = tempfile::tempdir().unwrap();
        let book = dir.path().join("books").join("b1");
        std::fs::create_dir_all(book.join("story")).unwrap();
        std::fs::write(
            book.join("book.json"),
            r#"{"id":"b1","title":"t","platform":"other","genre":"other","status":"active","targetChapters":10,"chapterWordCount":3000,"language":"zh","createdAt":"","updatedAt":""}"#,
        )
        .unwrap();
        std::fs::write(
            book.join("story").join("series_backfill.md"),
            "## [worldview] 旧条目\n\n旧描述。\n\n",
        )
        .unwrap();
        std::fs::write(
            book.join("story").join("series_backfill_draft.json"),
            serde_json::to_string(&draft_with(&[item("it-1", "character", "林动", "主角。")])).unwrap(),
        )
        .unwrap();

        // 不带 mode 字段（旧客户端形态）→ 整体覆盖语义不变。
        let response = app(runtime_for(dir.path()))
            .oneshot(
                axum::http::Request::builder()
                    .method("POST")
                    .uri("/api/v1/books/b1/series-backfill/apply")
                    .header("content-type", "application/json")
                    .body(axum::body::Body::from(r#"{"itemIds":["it-1"]}"#))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        let content = std::fs::read_to_string(book.join("story").join("series_backfill.md")).unwrap();
        assert!(content.contains("## [character] 林动"));
        assert!(!content.contains("旧条目")); // 覆盖：既有条目移除
    }
}
