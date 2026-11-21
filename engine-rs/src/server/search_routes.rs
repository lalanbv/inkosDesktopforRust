//! 全局搜索面（R47，1640 号）：`GET /api/v1/books/:id/search?q=&scope=&limit=`
//!
//! 只读端点，四过滤轴：
//! - `chapters`（默认）：正文 BM25——`.inkos/chapter-search.db` 持久 FTS5 投影
//!   （materials `.inkos/retrieval.db` 同惯用形：content_hash 去重增量，首次
//!   全量分词、热查零重 tokenize），分词走 G1 单源
//!   [`crate::utils::local_search::tokenize_search_text`]。
//! - `entity` / `hook` / `truth`：直读名册/承诺账本/真相文件（规格「走名册/
//!   承诺账本索引不重新分词」），加权字段扫描不建持久索引。
//!
//! TS 镜像 = `packages/studio/src/api/server.ts`（Node 回退端对称），双端同构
//! 由 strangler_duel 只读清单 + engine-contract-diff 登记；golden 向量
//! `search-highlight-vectors.json` / `search-ranking-vectors.json` 锁纯函数。
//!
//! 写面备案：GET 请求会落 `.inkos/chapter-search.db` 投影（hidden 目录，
//! retrieval.db 先例同类）——故不入 e2e_read_chain_zero_write 零写扫描清单。

use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::Json;
use serde_json::{json, Value};

use crate::server::books_routes::{book_id_is_unsafe, BooksRuntime};
use crate::utils::global_search::{
    build_highlight_snippet, find_weighted_field_match, query_tokens,
};
use crate::utils::local_search::{LocalSearchIndex, SearchDocument, SearchOptions};

const DEFAULT_LIMIT: usize = 20;
const MAX_LIMIT: usize = 50;
/// 查询长度上限（超长 query 负路径：裁剪而非拒绝——分词单源已 dedupe+64 上限）。
const MAX_QUERY_CHARS: usize = 200;
/// 章节投影 scope 键（单库多书，键按书隔离）。
const CHAPTER_SCOPE_PREFIX: &str = "chapters:";

#[derive(Debug, serde::Deserialize)]
pub struct SearchQueryParams {
    #[serde(default)]
    pub q: Option<String>,
    #[serde(default)]
    pub scope: Option<String>,
    /// 契约：须为正整数，否则回落默认（`limit=-1`/非数字 → 20）——双端同规则。
    #[serde(default)]
    pub limit: Option<String>,
}

/// 单条命中（camelCase 序列化，TS 镜像逐键对齐）。
#[derive(Debug, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GlobalSearchHit {
    pub kind: &'static str,
    pub id: String,
    pub title: String,
    pub source: String,
    pub score: f64,
    pub snippet: String,
    pub match_start: usize,
    pub match_length: usize,
    /// 仅 chapters 轴携带。
    #[serde(skip_serializing_if = "Option::is_none")]
    pub chapter_number: Option<u32>,
}

pub async fn get_search(
    State(runtime): State<BooksRuntime>,
    Path(book_id): Path<String>,
    Query(params): Query<SearchQueryParams>,
) -> Response {
    if book_id_is_unsafe(&book_id) {
        return bad_request("Invalid book id");
    }
    let Some(raw_query) = params.q.as_deref().map(str::trim).filter(|q| !q.is_empty()) else {
        return bad_request("query is required");
    };
    // 超长 query 负路径：按字符裁剪（双端同规则）。
    let query: String = raw_query.chars().take(MAX_QUERY_CHARS).collect();
    let scope = params.scope.as_deref().unwrap_or("chapters");
    let limit = limit_of(&params);
    let hits = match scope {
        "chapters" => search_chapters(&runtime, &book_id, &query, limit).await,
        "entity" => search_entity(&runtime, &book_id, &query, limit).await,
        "hook" => search_hooks(&runtime, &book_id, &query, limit).await,
        "truth" => search_truth(&runtime, &book_id, &query, limit).await,
        _ => return bad_request("invalid scope"),
    };
    (
        StatusCode::OK,
        Json(json!({ "query": query, "scope": scope, "hits": hits })),
    )
        .into_response()
}

/// 正整数解析回落默认 + 1..=50 收敛（`-1`/非数字 → 20；双端同规则）。
fn limit_of(params: &SearchQueryParams) -> usize {
    params
        .limit
        .as_deref()
        .and_then(|raw| raw.parse::<i64>().ok())
        .filter(|limit| *limit > 0)
        .map(|limit| limit as usize)
        .unwrap_or(DEFAULT_LIMIT)
        .clamp(1, MAX_LIMIT)
}

fn bad_request(message: &str) -> Response {
    (StatusCode::BAD_REQUEST, Json(json!({ "error": message }))).into_response()
}

/// 字段扫描命中 → GlobalSearchHit（名册/账本/真相三轴共用投影形）。
fn roster_hit(
    kind: &'static str,
    id: String,
    title: String,
    source: String,
    score: f64,
    matched_text: &str,
    query: &str,
) -> Option<GlobalSearchHit> {
    let snippet = build_highlight_snippet(matched_text, query)?;
    Some(GlobalSearchHit {
        kind,
        id,
        title,
        source,
        score,
        snippet: snippet.text,
        match_start: snippet.match_start,
        match_length: snippet.match_length,
        chapter_number: None,
    })
}

/// 加权命中排序（权重降序、id 升序）——名册/账本两轴共用（双端同规则）。
fn sort_roster_hits(hits: &mut [GlobalSearchHit]) {
    hits.sort_by(|a, b| {
        b.score
            .partial_cmp(&a.score)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then(a.id.cmp(&b.id))
    });
}

// ── chapters 轴 ──────────────────────────────────────────────────

async fn search_chapters(
    runtime: &BooksRuntime,
    book_id: &str,
    query: &str,
    limit: usize,
) -> Vec<GlobalSearchHit> {
    let book_dir = runtime.state.book_dir(book_id);
    let chapters_dir = book_dir.join("chapters");
    let Ok(metas) = runtime.state.load_chapter_index(book_id).await else {
        return Vec::new();
    };
    if metas.is_empty() {
        return Vec::new();
    }
    let files = list_chapter_files(&chapters_dir).await;
    let mut documents: Vec<SearchDocument> = Vec::with_capacity(metas.len());
    for meta in &metas {
        let Some(file_name) =
            crate::state::chapter_filename::find_chapter_file_by_number(&files, meta.number)
        else {
            continue;
        };
        let Ok(content) = tokio::fs::read_to_string(chapters_dir.join(&file_name)).await else {
            continue;
        };
        let title = if meta.title.trim().is_empty() {
            file_name.clone()
        } else {
            meta.title.clone()
        };
        documents.push(SearchDocument {
            id: format!("chapter:{}", meta.number),
            scope: format!("{CHAPTER_SCOPE_PREFIX}{book_id}"),
            kind: "chapter".to_string(),
            source: file_name,
            title,
            body: content,
            metadata: Some(json!({ "chapterNumber": meta.number })),
        });
    }
    if documents.is_empty() {
        return Vec::new();
    }
    let scope = format!("{CHAPTER_SCOPE_PREFIX}{book_id}");
    let index_path = runtime
        .state
        .project_root()
        .join(".inkos")
        .join("chapter-search.db");
    let query_owned = query.to_string();
    // 765 号惯例：索引重建+查询移出 runtime 线程（检索尽力而为面：打开/写失败降级空集）。
    let searched =
        tokio::task::spawn_blocking(move || -> Vec<crate::utils::local_search::SearchHit> {
            let Ok(index) = LocalSearchIndex::new(&index_path.to_string_lossy()) else {
                return Vec::new();
            };
            let _ = index.replace_scope(&scope, &documents);
            let hits = index.search(
                &query_owned,
                &SearchOptions {
                    scope: &scope,
                    kinds: &[],
                    limit,
                },
            );
            index.close();
            hits
        })
        .await
        .unwrap_or_default();

    searched
        .iter()
        .filter_map(|hit| {
            let snippet = build_highlight_snippet(&hit.body, query)?;
            Some(GlobalSearchHit {
                kind: "chapter",
                id: hit.id.clone(),
                title: hit.title.clone(),
                source: hit.source.clone(),
                score: hit.score,
                snippet: snippet.text,
                match_start: snippet.match_start,
                match_length: snippet.match_length,
                chapter_number: hit
                    .metadata
                    .as_ref()
                    .and_then(|meta| meta.get("chapterNumber"))
                    .and_then(Value::as_u64)
                    .map(|number| number as u32),
            })
        })
        .collect()
}

async fn list_chapter_files(chapters_dir: &std::path::Path) -> Vec<String> {
    let Ok(mut entries) = tokio::fs::read_dir(chapters_dir).await else {
        return Vec::new();
    };
    let mut files = Vec::new();
    while let Ok(Some(entry)) = entries.next_entry().await {
        files.push(entry.file_name().to_string_lossy().into_owned());
    }
    files
}

// ── entity 轴 ────────────────────────────────────────────────────

/// 字段权重：name 3 / aliases 2 / summary·facts 1（双端同表）。
const ENTITY_WEIGHT_NAME: i64 = 3;
const ENTITY_WEIGHT_ALIAS: i64 = 2;
const ENTITY_WEIGHT_BODY: i64 = 1;

async fn search_entity(
    runtime: &BooksRuntime,
    book_id: &str,
    query: &str,
    limit: usize,
) -> Vec<GlobalSearchHit> {
    // 名册源双面（规格「走名册索引」；get_codex 同源形态）：entity_codex.json
    // 文件卡优先 + entity_roster.md 派生卡（TS deriveCodexCards 同构，golden
    // entity-codex-vectors 已锁派生面）——同 name 派生卡被文件卡遮蔽。
    let story_dir = runtime.state.book_dir(book_id).join("story");
    let raw = tokio::fs::read_to_string(story_dir.join("entity_codex.json"))
        .await
        .unwrap_or_default();
    let parsed = serde_json::from_str::<Value>(&raw).unwrap_or(Value::Null);
    let cards = parsed
        .get("cards")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    let roster_markdown = tokio::fs::read_to_string(story_dir.join("entity_roster.md"))
        .await
        .unwrap_or_default();
    // RosterEntity → CodexRosterEntity 字段同构映射（本仓 derive 消费首站）。
    let roster_entities: Vec<crate::utils::entity_codex::CodexRosterEntity> =
        crate::utils::entity_roster::parse_entity_roster(&roster_markdown)
            .into_iter()
            .map(|entity| crate::utils::entity_codex::CodexRosterEntity {
                id: entity.id,
                name: entity.name,
                aliases: entity.aliases,
                kind: entity.kind,
                registered_at: entity.registered_at,
            })
            .collect();
    let roster_cards = crate::utils::entity_codex::derive_codex_cards(&roster_entities);
    let known: std::collections::HashSet<String> = cards
        .iter()
        .filter_map(|card| card.get("name").and_then(Value::as_str))
        .map(str::to_string)
        .collect();

    // 统一扫描形：name + 字段集（派生卡 summary/facts 置空，仅 name 可命中）。
    struct CardScan {
        name: String,
        fields: Vec<(String, i64)>,
    }
    let mut scans: Vec<CardScan> = Vec::new();
    for card in &cards {
        let Some(name) = card
            .get("name")
            .and_then(Value::as_str)
            .filter(|n| !n.is_empty())
        else {
            continue;
        };
        let field_str = |key: &str| -> String {
            card.get(key)
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_string()
        };
        let summary = field_str("summary");
        let aliases: Vec<String> = card
            .get("aliases")
            .and_then(Value::as_array)
            .map(|list| {
                list.iter()
                    .filter_map(Value::as_str)
                    .map(str::to_string)
                    .collect()
            })
            .unwrap_or_default();
        let facts: Vec<String> = card
            .get("facts")
            .and_then(Value::as_array)
            .map(|list| {
                list.iter()
                    .filter_map(Value::as_str)
                    .map(str::to_string)
                    .collect()
            })
            .unwrap_or_default();
        let mut fields: Vec<(String, i64)> = vec![(name.to_string(), ENTITY_WEIGHT_NAME)];
        for alias in &aliases {
            fields.push((alias.clone(), ENTITY_WEIGHT_ALIAS));
        }
        if !summary.is_empty() {
            fields.push((summary, ENTITY_WEIGHT_BODY));
        }
        for fact in &facts {
            fields.push((fact.clone(), ENTITY_WEIGHT_BODY));
        }
        scans.push(CardScan {
            name: name.to_string(),
            fields,
        });
    }
    for card in &roster_cards {
        if known.contains(&card.name) {
            continue;
        }
        scans.push(CardScan {
            name: card.name.clone(),
            fields: vec![(card.name.clone(), ENTITY_WEIGHT_NAME)],
        });
    }

    let source = "story/entity_codex.json".to_string();
    let mut hits: Vec<GlobalSearchHit> = Vec::new();
    for scan in &scans {
        let field_refs: Vec<(&str, i64)> = scan
            .fields
            .iter()
            .map(|(text, weight)| (text.as_str(), *weight))
            .collect();
        let Some(matched) = find_weighted_field_match(&field_refs, query) else {
            continue;
        };
        let matched_text = scan.fields[matched.field_index].0.clone();
        if let Some(hit) = roster_hit(
            "entity",
            format!("entity:{}", scan.name),
            scan.name.clone(),
            source.clone(),
            matched.weight as f64,
            &matched_text,
            query,
        ) {
            hits.push(hit);
        }
    }
    sort_roster_hits(&mut hits);
    hits.truncate(limit);
    hits
}

// ── hook 轴 ──────────────────────────────────────────────────────

/// 字段权重：summary 2 / expectedPayoff·hookId 1（双端同表）。
const HOOK_WEIGHT_SUMMARY: i64 = 2;
const HOOK_WEIGHT_BODY: i64 = 1;

async fn search_hooks(
    runtime: &BooksRuntime,
    book_id: &str,
    query: &str,
    limit: usize,
) -> Vec<GlobalSearchHit> {
    let book_dir = runtime.state.book_dir(book_id);
    let db_path = book_dir.join("story").join("memory.db");
    if !db_path.exists() {
        return Vec::new();
    }
    // 763/764 号惯例：台账 markdown 异步预读 + db 查询/投影入 spawn_blocking。
    let hooks_markdown = tokio::fs::read_to_string(book_dir.join("story").join("pending_hooks.md"))
        .await
        .unwrap_or_default();
    let timeline = tokio::task::spawn_blocking(
        move || -> Vec<crate::utils::promise_ledger::PromiseTimelineEntry> {
            let Ok(db) = crate::state::memory_db::MemoryDb::open(&book_dir) else {
                return Vec::new();
            };
            let Ok(hooks) = db.get_all_hooks() else {
                return Vec::new();
            };
            let mut kind_by_id: std::collections::HashMap<
                String,
                crate::models::runtime_state::HookKind,
            > = std::collections::HashMap::new();
            for hook in crate::utils::story_markdown::parse_pending_hooks_markdown(&hooks_markdown)
            {
                if let Some(kind) = hook.kind {
                    kind_by_id.insert(hook.hook_id.clone(), kind);
                }
            }
            let hook_inputs: Vec<crate::utils::promise_ledger::PromiseHookInput> = hooks
                .iter()
                .map(|hook| crate::utils::promise_ledger::PromiseHookInput {
                    hook_id: hook.hook_id.clone(),
                    start_chapter: hook.start_chapter,
                    status: hook.status.clone(),
                    last_advanced_chapter: hook.last_advanced_chapter,
                    expected_payoff: hook.expected_payoff.clone(),
                    notes: hook.notes.clone(),
                    core_hook: false,
                    kind: kind_by_id.get(&hook.hook_id).copied(),
                })
                .collect();
            let Ok(summaries) = db.get_summaries(1, 100_000) else {
                return Vec::new();
            };
            let current_chapter = summaries
                .iter()
                .map(|summary| summary.chapter)
                .max()
                .map(|last| last + 1)
                .unwrap_or(1);
            crate::utils::promise_ledger::build_promise_timeline(&hook_inputs, current_chapter)
        },
    )
    .await
    .unwrap_or_default();

    let source = "story/pending_hooks.md".to_string();
    let mut hits: Vec<GlobalSearchHit> = Vec::new();
    for entry in &timeline {
        let fields: Vec<(&str, i64)> = vec![
            (entry.summary.as_str(), HOOK_WEIGHT_SUMMARY),
            (entry.expected_payoff.as_str(), HOOK_WEIGHT_BODY),
            (entry.hook_id.as_str(), HOOK_WEIGHT_BODY),
        ];
        let Some(matched) = find_weighted_field_match(&fields, query) else {
            continue;
        };
        let matched_text = fields[matched.field_index].0.to_string();
        let title = if entry.summary.trim().is_empty() {
            entry.hook_id.clone()
        } else {
            entry.summary.clone()
        };
        if let Some(hit) = roster_hit(
            "hook",
            format!("hook:{}", entry.hook_id),
            title,
            source.clone(),
            matched.weight as f64,
            &matched_text,
            query,
        ) {
            hits.push(hit);
        }
    }
    sort_roster_hits(&mut hits);
    hits.truncate(limit);
    hits
}

// ── truth 轴 ─────────────────────────────────────────────────────

async fn search_truth(
    runtime: &BooksRuntime,
    book_id: &str,
    query: &str,
    limit: usize,
) -> Vec<GlobalSearchHit> {
    let story_dir = runtime.state.book_dir(book_id).join("story");
    let files = crate::server::books_state_routes::truth_search_files(&story_dir).await;
    if query_tokens(query).is_empty() {
        return Vec::new();
    }
    let mut hits: Vec<GlobalSearchHit> = Vec::new();
    for rel in files {
        let Ok(content) = tokio::fs::read_to_string(story_dir.join(&rel)).await else {
            continue;
        };
        // 行级扫描：首个命中行产一条命中（片段取行内窗，行已窄）。
        for line in content.lines() {
            let trimmed = line.trim();
            if trimmed.is_empty() {
                continue;
            }
            if let Some(hit) = roster_hit(
                "truth",
                format!("truth:{rel}"),
                rel.clone(),
                rel.clone(),
                1.0,
                trimmed,
                query,
            ) {
                hits.push(hit);
                break;
            }
        }
        if hits.len() >= limit {
            break;
        }
    }
    hits
}

#[cfg(test)]
mod tests {
    use super::*;

    fn hit(text: &str, query: &str) -> GlobalSearchHit {
        roster_hit(
            "entity",
            "entity:测试".to_string(),
            "测试".to_string(),
            "story/entity_codex.json".to_string(),
            1.0,
            text,
            query,
        )
        .expect("命中")
    }

    #[test]
    fn limit_clamps() {
        let make = |limit: Option<&str>| SearchQueryParams {
            q: None,
            scope: None,
            limit: limit.map(str::to_string),
        };
        assert_eq!(limit_of(&make(Some("999"))), MAX_LIMIT);
        assert_eq!(limit_of(&make(Some("0"))), DEFAULT_LIMIT, "非正数回落默认");
        assert_eq!(limit_of(&make(Some("-1"))), DEFAULT_LIMIT);
        assert_eq!(limit_of(&make(Some("abc"))), DEFAULT_LIMIT);
        assert_eq!(limit_of(&make(Some("3"))), 3);
        assert_eq!(limit_of(&make(None)), DEFAULT_LIMIT);
    }

    #[test]
    fn roster_hit_shape() {
        let hit = hit("背景提及碎镜之主", "碎镜之主");
        assert_eq!(hit.kind, "entity");
        assert_eq!(hit.id, "entity:测试");
        assert_eq!(hit.chapter_number, None);
        assert!(hit.snippet.contains("碎镜之主"));
        assert_eq!(hit.match_length, 4);
    }

    #[test]
    fn roster_hit_none_on_miss() {
        assert!(roster_hit(
            "entity",
            "e".to_string(),
            "t".to_string(),
            "s".to_string(),
            1.0,
            "无关内容",
            "碎镜"
        )
        .is_none());
    }
}
