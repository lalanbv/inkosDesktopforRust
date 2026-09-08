//! 词法检索内核（LocalSearchIndex，241 号）。
//!
//! 移植自 `packages/core/src/retrieval/local-search.ts`（284 行）：
//! SQLite FTS5 投影（external-content 表 + 触发器同步）+ BM25 排序
//! （title 权重 5.0 / body 1.0）+ 分词器（NFKC + word 分段 + 相邻汉字
//! bigram + 连字符复合词）。
//!
//! ## 移植差异备案
//! TS 用 `Intl.Segmenter`（ICU）做词边界——Rust 以
//! `unicode-segmentation::split_word_bounds` 近似；个别语种的分词粒度
//! 可能不同（分数不逐字一致），检索意图（top-k 相关排序）等价。
//! 消费面：use_skill 的 query 检索分支（240 号备案项）。

use std::path::Path;

use regex::Regex;
use rusqlite::{params, Connection};
use unicode_normalization::UnicodeNormalization;

/// 单条检索文档。对齐 TS `SearchDocument`。
#[derive(Debug, Clone)]
pub struct SearchDocument {
    pub id: String,
    pub scope: String,
    pub kind: String,
    pub source: String,
    pub title: String,
    pub body: String,
}

/// 单条命中。对齐 TS `SearchHit`（score = -bm25，越大越相关）。
#[derive(Debug, Clone)]
pub struct SearchHit {
    pub id: String,
    pub kind: String,
    pub source: String,
    pub title: String,
    pub body: String,
    pub score: f64,
}

/// 检索选项（scope 恒必填；kinds 可选过滤；limit 1..=200 默认 24）。
pub struct SearchOptions<'a> {
    pub scope: &'a str,
    pub kinds: &'a [String],
    pub limit: usize,
}

pub struct LocalSearchIndex {
    conn: Connection,
}

impl LocalSearchIndex {
    /// `:memory:` 内存索引或文件索引（父目录自动创建）。
    pub fn new(path: &str) -> rusqlite::Result<Self> {
        if path != ":memory:" {
            if let Some(parent) = Path::new(path).parent() {
                let _ = std::fs::create_dir_all(parent);
            }
        }
        let conn = Connection::open(path)?;
        let index = Self { conn };
        index.migrate()?;
        Ok(index)
    }

    fn migrate(&self) -> rusqlite::Result<()> {
        // SQL 逐字对齐 TS migrate()。
        self.conn.execute_batch(
            r#"
            CREATE TABLE IF NOT EXISTS retrieval_documents (
                rowid INTEGER PRIMARY KEY AUTOINCREMENT,
                document_id TEXT NOT NULL,
                scope TEXT NOT NULL,
                kind TEXT NOT NULL,
                source TEXT NOT NULL,
                title TEXT NOT NULL,
                body TEXT NOT NULL,
                title_tokens TEXT NOT NULL,
                body_tokens TEXT NOT NULL,
                metadata_json TEXT NOT NULL DEFAULT '{}',
                content_hash TEXT NOT NULL,
                updated_at TEXT NOT NULL DEFAULT (datetime('now')),
                UNIQUE(scope, document_id)
            );

            CREATE INDEX IF NOT EXISTS idx_retrieval_documents_scope_kind
              ON retrieval_documents(scope, kind);

            CREATE VIRTUAL TABLE IF NOT EXISTS retrieval_documents_fts USING fts5(
                title_tokens,
                body_tokens,
                content='retrieval_documents',
                content_rowid='rowid'
            );

            CREATE TRIGGER IF NOT EXISTS retrieval_documents_ai AFTER INSERT ON retrieval_documents BEGIN
                INSERT INTO retrieval_documents_fts(rowid, title_tokens, body_tokens)
                VALUES (new.rowid, new.title_tokens, new.body_tokens);
            END;

            CREATE TRIGGER IF NOT EXISTS retrieval_documents_ad AFTER DELETE ON retrieval_documents BEGIN
                INSERT INTO retrieval_documents_fts(retrieval_documents_fts, rowid, title_tokens, body_tokens)
                VALUES ('delete', old.rowid, old.title_tokens, old.body_tokens);
            END;

            CREATE TRIGGER IF NOT EXISTS retrieval_documents_au AFTER UPDATE ON retrieval_documents BEGIN
                INSERT INTO retrieval_documents_fts(retrieval_documents_fts, rowid, title_tokens, body_tokens)
                VALUES ('delete', old.rowid, old.title_tokens, old.body_tokens);
                INSERT INTO retrieval_documents_fts(rowid, title_tokens, body_tokens)
                VALUES (new.rowid, new.title_tokens, new.body_tokens);
            END;
            "#,
        )?;
        Ok(())
    }

    /// scope 内全量替换（hash 去重增改 + 消失 id 删除，事务）。
    pub fn replace_scope(&self, scope: &str, documents: &[SearchDocument]) -> rusqlite::Result<()> {
        let normalized: Vec<NormalizedDocument> = documents
            .iter()
            .map(|document| normalize_document(document, scope))
            .collect();
        let keep_ids: std::collections::HashSet<&str> =
            normalized.iter().map(|document| document.id.as_str()).collect();

        self.conn.execute_batch("BEGIN IMMEDIATE")?;
        let upsert_result = (|| -> rusqlite::Result<()> {
            let existing_rows: Vec<(String, String)> = {
                let mut existing = self.conn.prepare(
                    "SELECT document_id, content_hash FROM retrieval_documents WHERE scope = ?",
                )?;
                let rows = existing
                    .query_map(params![scope], |row| {
                        Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
                    })?
                    .collect::<Result<Vec<_>, _>>()?;
                rows
            };
            let existing_hashes: std::collections::HashMap<String, String> =
                existing_rows.iter().cloned().collect();

            let mut upsert = self.conn.prepare(
                r#"
                INSERT INTO retrieval_documents (
                    document_id, scope, kind, source, title, body,
                    title_tokens, body_tokens, metadata_json, content_hash
                ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
                ON CONFLICT(scope, document_id) DO UPDATE SET
                    kind = excluded.kind,
                    source = excluded.source,
                    title = excluded.title,
                    body = excluded.body,
                    title_tokens = excluded.title_tokens,
                    body_tokens = excluded.body_tokens,
                    metadata_json = excluded.metadata_json,
                    content_hash = excluded.content_hash,
                    updated_at = datetime('now')
                "#,
            )?;
            let mut remove = self.conn.prepare(
                "DELETE FROM retrieval_documents WHERE scope = ? AND document_id = ?",
            )?;

            for document in &normalized {
                if existing_hashes.get(&document.id).map(String::as_str)
                    == Some(document.content_hash.as_str())
                {
                    continue;
                }
                upsert.execute(params![
                    document.id,
                    scope,
                    document.kind,
                    document.source,
                    document.title,
                    document.body,
                    document.title_tokens,
                    document.body_tokens,
                    document.metadata_json,
                    document.content_hash,
                ])?;
            }
            for row in &existing_rows {
                if !keep_ids.contains(row.0.as_str()) {
                    remove.execute(params![scope, row.0])?;
                }
            }
            Ok(())
        })();

        match upsert_result {
            Ok(()) => self.conn.execute_batch("COMMIT")?,
            Err(error) => {
                let _ = self.conn.execute_batch("ROLLBACK");
                return Err(error);
            }
        }
        Ok(())
    }

    /// BM25 检索（score = -bm25；越大越相关）。
    pub fn search(&self, query: &str, options: &SearchOptions<'_>) -> Vec<SearchHit> {
        let Some(match_query) = build_match_query(query) else {
            return Vec::new();
        };
        let kinds: Vec<&str> = options.kinds.iter().map(String::as_str).collect();
        let kind_filter = if kinds.is_empty() {
            String::new()
        } else {
            format!(
                " AND d.kind IN ({})",
                kinds.iter().map(|_| "?").collect::<Vec<_>>().join(", ")
            )
        };
        let limit = options.limit.clamp(1, 200);
        let sql = format!(
            r#"
            SELECT
                d.document_id AS id,
                d.kind,
                d.source,
                d.title,
                d.body,
                bm25(retrieval_documents_fts, 5.0, 1.0) AS rank
            FROM retrieval_documents_fts
            JOIN retrieval_documents d ON d.rowid = retrieval_documents_fts.rowid
            WHERE retrieval_documents_fts MATCH ?
              AND d.scope = ?{kind_filter}
            ORDER BY rank ASC, d.document_id ASC
            LIMIT ?
            "#
        );
        let mut statement = match self.conn.prepare(&sql) {
            Ok(statement) => statement,
            Err(_) => return Vec::new(),
        };
        let mut bind_params: Vec<String> = vec![match_query, options.scope.to_string()];
        for kind in &kinds {
            bind_params.push((*kind).to_string());
        }
        let mut bind_params: Vec<rusqlite::types::Value> = bind_params
            .iter()
            .map(|value| rusqlite::types::Value::Text(value.clone()))
            .collect();
        bind_params.push(rusqlite::types::Value::Integer(limit as i64));
        let rows = statement.query_map(rusqlite::params_from_iter(bind_params.iter()), |row| {
            Ok(SearchHit {
                id: row.get("id")?,
                kind: row.get("kind")?,
                source: row.get("source")?,
                title: row.get("title")?,
                body: row.get("body")?,
                // SQLite FTS5 bm25() 越小越相关 → 取负为分数。
                score: -row.get::<_, f64>("rank")?,
            })
        });
        match rows {
            Ok(rows) => rows.collect::<Result<Vec<_>, _>>().unwrap_or_default(),
            Err(_) => Vec::new(),
        }
    }

    pub fn close(self) {}
}

struct NormalizedDocument {
    id: String,
    kind: String,
    source: String,
    title: String,
    body: String,
    title_tokens: String,
    body_tokens: String,
    metadata_json: String,
    content_hash: String,
}

fn normalize_document(document: &SearchDocument, _scope: &str) -> NormalizedDocument {
    use sha2::{Digest, Sha256};
    let metadata_json = "{}".to_string();
    let title_tokens = tokenize_search_text(&document.title).join(" ");
    let body_tokens = tokenize_search_text(&document.body).join(" ");
    let content_hash = {
        let mut hasher = Sha256::new();
        for part in [
            &document.kind,
            &document.source,
            &document.title,
            &document.body,
            &metadata_json,
        ] {
            hasher.update(part.as_bytes());
            hasher.update([0u8]);
        }
        format!("{:x}", hasher.finalize())
    };
    NormalizedDocument {
        id: document.id.clone(),
        kind: document.kind.clone(),
        source: document.source.clone(),
        title: document.title.clone(),
        body: document.body.clone(),
        title_tokens,
        body_tokens,
        metadata_json,
        content_hash,
    }
}

/// `tokenizeSearchText`：NFKC + lower → word 分段（word-like 过滤 +
/// Latin/数字单字符丢弃）→ 相邻汉字 bigram 追加 → 连字符复合词追加。
pub fn tokenize_search_text(text: &str) -> Vec<String> {
    use unicode_segmentation::UnicodeSegmentation;
    let normalized: String = text.nfkc().collect::<String>().to_lowercase();
    let mut tokens: Vec<String> = Vec::new();
    for part in normalized.split_word_bounds() {
        let token: &str = part.trim();
        if !is_word_like(token) {
            continue;
        }
        if is_ascii_word(token) && token.chars().count() < 2 {
            continue;
        }
        tokens.push(token.to_string());
    }
    let segmented_count = tokens.len();
    for index in 0..segmented_count.saturating_sub(1) {
        let left = &tokens[index];
        let right = &tokens[index + 1];
        if is_single_han(left) && is_single_han(right) {
            tokens.push(format!("{left}{right}"));
        }
    }
    for m in hyphen_compound_re().find_iter(&normalized) {
        tokens.push(m.as_str().to_string());
    }
    tokens
}

fn is_word_like(token: &str) -> bool {
    token
        .chars()
        .any(|c| c.is_alphanumeric() || c == '_' || c == '-')
}

fn is_ascii_word(token: &str) -> bool {
    token
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-')
}

fn is_single_han(token: &str) -> bool {
    let mut chars = token.chars();
    matches!(chars.next(), Some(c) if ('\u{4e00}'..='\u{9fff}').contains(&c))
        && chars.next().is_none()
}

fn hyphen_compound_re() -> &'static Regex {
    static R: std::sync::OnceLock<Regex> = std::sync::OnceLock::new();
    R.get_or_init(|| Regex::new(r"[\p{L}\p{N}]+(?:[-_][\p{L}\p{N}]+)+").expect("compound regex"))
}

/// `buildMatchQuery`：去重取 64 词 → `"tok"` OR 连接；空 → None。
pub fn build_match_query(query: &str) -> Option<String> {
    let mut seen = std::collections::HashSet::new();
    let tokens: Vec<String> = tokenize_search_text(query)
        .into_iter()
        .filter(|token| seen.insert(token.clone()))
        .take(64)
        .collect();
    if tokens.is_empty() {
        return None;
    }
    Some(
        tokens
            .iter()
            .map(|token| format!("\"{}\"", token.replace('"', "\"\"")))
            .collect::<Vec<_>>()
            .join(" OR "),
    )
}

/// 分段结果。对齐 TS `MarkdownSearchSegment`（偏移为字符数）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MarkdownSearchSegment {
    pub heading: String,
    pub body: String,
    pub char_start: usize,
    pub char_end: usize,
}

/// `splitMarkdownForSearch`：空行分块（块内无空行）、标题块只继承 heading
/// 不产段；偏移为字符数。
pub fn split_markdown_for_search(markdown: &str) -> Vec<MarkdownSearchSegment> {
    let heading_re = heading_line_re();
    let mut segments: Vec<MarkdownSearchSegment> = Vec::new();
    let mut heading = String::new();

    // 按空行边界切块（TS 惰性正则 `\n\s*\n` 的等价语义：段内无空行），
    // 并记录块首字符偏移。
    let mut char_offset = 0usize;
    let mut current: Option<(usize, String)> = None;
    for line in markdown.split_inclusive('\n') {
        let is_blank = line.trim().is_empty();
        let line_text = line.strip_suffix('\n').unwrap_or(line);
        if is_blank {
            if let Some((start, raw)) = current.take() {
                push_segment(&mut segments, &mut heading, heading_re, start, &raw);
            }
        } else {
            match &mut current {
                Some((_, raw)) => raw.push_str(line_text),
                None => current = Some((char_offset, line_text.to_string())),
            }
        }
        char_offset += line.chars().count();
    }
    if let Some((start, raw)) = current.take() {
        push_segment(&mut segments, &mut heading, heading_re, start, &raw);
    }
    segments
}

fn push_segment(
    segments: &mut Vec<MarkdownSearchSegment>,
    heading: &mut String,
    heading_re: &Regex,
    start: usize,
    raw: &str,
) {
    let body = raw.trim();
    if body.is_empty() {
        return;
    }
    if let Some(caps) = heading_re.captures(body) {
        if let Some(m) = caps.get(1) {
            *heading = m.as_str().trim().to_string();
        }
        return;
    }
    segments.push(MarkdownSearchSegment {
        heading: heading.clone(),
        body: body.to_string(),
        char_start: start,
        char_end: start + raw.chars().count(),
    });
}

fn heading_line_re() -> &'static Regex {
    static R: std::sync::OnceLock<Regex> = std::sync::OnceLock::new();
    R.get_or_init(|| Regex::new(r"^#{1,6}\s+(.+)$").expect("heading regex"))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn doc(id: &str, title: &str, body: &str) -> SearchDocument {
        SearchDocument {
            id: id.to_string(),
            scope: "skill:combat".to_string(),
            kind: "skill-reference".to_string(),
            source: format!("{id}.md"),
            title: title.to_string(),
            body: body.to_string(),
        }
    }

    #[test]
    fn fts5_available_and_search_roundtrip() {
        // bundled SQLite 启用 FTS5 的前提验证 + 基本检索往返。
        let index = LocalSearchIndex::new(":memory:").expect("index");
        index
            .replace_scope(
                "skill:combat",
                &[
                    doc("d1", "布局", "开篇要建立核心冲突与人物压力。"),
                    doc("d2", "对白", "对白要短，避免书面语。"),
                ],
            )
            .unwrap();

        let hits = index.search(
            "开篇 核心冲突",
            &SearchOptions { scope: "skill:combat", kinds: &[], limit: 4 },
        );
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].id, "d1");
        assert!(hits[0].score > 0.0, "score = -bm25 应为正");

        // 空查询 / 无命中。
        assert!(index.search("", &SearchOptions { scope: "skill:combat", kinds: &[], limit: 4 }).is_empty());
        assert!(index.search("完全不相关词汇组合", &SearchOptions { scope: "skill:combat", kinds: &[], limit: 4 }).is_empty());
    }

    #[test]
    fn replace_scope_removes_missing_ids() {
        let index = LocalSearchIndex::new(":memory:").unwrap();
        index
            .replace_scope("s", &[doc("a", "A", "内容甲"), doc("b", "B", "内容乙")])
            .unwrap();
        // 二次替换只剩 a → b 被删除。
        index.replace_scope("s", &[doc("a", "A2", "内容甲更新")]).unwrap();
        let hits = index.search("内容", &SearchOptions { scope: "s", kinds: &[], limit: 10 });
        assert!(hits.iter().all(|hit| hit.id == "a"), "{hits:?}");
    }

    #[test]
    fn split_markdown_segments_and_heading_inheritance() {
        let markdown = "# 手册\n\n## 布局机制\n\n正文甲。\n\n延续段。\n\n## 对白\n\n正文乙。\n";
        let segments = split_markdown_for_search(markdown);
        let headings: Vec<&str> = segments.iter().map(|s| s.heading.as_str()).collect();
        assert_eq!(headings, vec!["布局机制", "布局机制", "对白"]);
        assert_eq!(segments[0].body, "正文甲。");
        // 标题块自身不产段，正文块继承标题。
        assert_eq!(segments[1].body, "延续段。");
    }

    #[test]
    fn tokenize_han_bigrams_and_hyphen_compounds() {
        let tokens = tokenize_search_text("核心冲突 core-hook");
        assert!(tokens.contains(&"核心".to_string()), "{tokens:?}");
        assert!(tokens.contains(&"心冲".to_string()), "{tokens:?}");
        assert!(tokens.contains(&"core-hook".to_string()), "{tokens:?}");
    }
}
