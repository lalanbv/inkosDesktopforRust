//! 时间记忆数据库（temporal memory）。
//!
//! 移植自 `packages/core/src/state/memory-db.ts`（359 行）。Node.js 内置 `node:sqlite` →
//! [`rusqlite`]（`bundled`，零系统依赖）。存储带时间有效期的 facts（valid_from/valid_until 章节），
//! 支持诸如「角色 X 在第 5 章时知道什么？」的精确查询。
//!
//! ## 与 TS 源的契约对齐
//! - 三张表（facts / chapter_summaries / hooks）的 DDL、列名、索引与 TS `migrate()` 完全一致——
//!   确保 Rust 实现可直接读写 TS 版产生的 `story/memory.db` 文件（向前兼容现有 markdown 真值文件）。
//! - 所有方法语义忠实对应 TS 同名方法；返回顺序、过滤条件、状态黑名单均逐一对齐。
//! - API 为同步（对齐 `node:sqlite` 的 `DatabaseSync`），由上层编排（runtime-state-store）在
//!   `spawn_blocking` 中调用。
//!
//! ## 待移植
//! `runtime-state-store`（fs + JSON schema 编排）依赖本模块 + reducer + validator，属下一阶段。

use std::path::Path;

use crate::Result;
use rusqlite::{params, params_from_iter, Connection, Row};

/// 一条带时间有效期的 fact（对应 `facts` 表一行）。
///
/// `id` 在新建时由数据库自增分配，故输入用 [`NewFact`]，读出时才有 `id`。
/// 字段与 TS `Fact` 接口一一对应（驼峰 ↔ snake_case 列名）。
#[derive(Debug, Clone, PartialEq)]
pub struct Fact {
    pub id: i64,
    pub subject: String,
    pub predicate: String,
    pub object: String,
    pub valid_from_chapter: i64,
    pub valid_until_chapter: Option<i64>,
    pub source_chapter: i64,
}

/// 创建 fact 的输入（不含自增 `id`）。
///
/// 对齐 TS `addFact(fact: Omit<Fact, "id">)`：`valid_until_chapter` 可空（表示当前有效）。
#[derive(Debug, Clone, PartialEq)]
pub struct NewFact {
    pub subject: String,
    pub predicate: String,
    pub object: String,
    pub valid_from_chapter: i64,
    pub valid_until_chapter: Option<i64>,
    pub source_chapter: i64,
}

/// G3/337 号：质量债务账本一行（对齐 TS StoredQualityDebt）。
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StoredQualityDebt {
    pub debt_id: String,
    pub book_id: String,
    pub chapter: i64,
    pub issue_category: String,
    pub severity: String,
    pub status: String,
    pub created_at: String,
    #[serde(default)]
    pub follow_up_note: String,
}

/// G1/349 号：语义检索 chunk 向量一行（vector 为 JSON 数组文本）。
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StoredChunkVector {
    pub chunk_id: String,
    pub source: String,
    pub fingerprint: String,
    pub dim: u32,
    pub vector: Vec<f64>,
    pub created_at: String,
}

/// 章节摘要（对应 `chapter_summaries` 表一行）。字段与 TS `StoredSummary` 一一对应。
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StoredSummary {
    pub chapter: i64,
    pub title: String,
    pub characters: String,
    pub events: String,
    pub state_changes: String,
    pub hook_activity: String,
    pub mood: String,
    pub chapter_type: String,
}

/// hook 记录（对应 `hooks` 表一行）。字段与 TS `StoredHook` 的持久化子集一一对应。
///
/// 注意：TS `StoredHook` 还携带 Phase 7 的可选元数据（dependsOn/paysOffInArc/coreHook 等），
/// 但这些**不入库**——TS 版的 `upsertHook` SQL 只写 8 列，元数据来自内存对象。本结构同样只建模
/// 持久化的 8 列，与 SQL 完全对齐。
#[derive(Debug, Clone, PartialEq)]
pub struct StoredHook {
    pub hook_id: String,
    pub start_chapter: i64,
    pub r#type: String,
    pub status: String,
    pub last_advanced_chapter: i64,
    pub expected_payoff: String,
    pub payoff_timing: String,
    pub notes: String,
}

const FACT_SELECT_COLUMNS: &str = "\
    id, \
    subject, \
    predicate, \
    object, \
    valid_from_chapter, \
    valid_until_chapter, \
    source_chapter";

/// 时间记忆数据库。
pub struct MemoryDb {
    conn: Connection,
}

impl MemoryDb {
    /// 打开/创建 `bookDir/story/memory.db` 并执行迁移。
    ///
    /// 对齐 TS 构造函数：`new DatabaseSync(join(bookDir, "story", "memory.db"))` +
    /// `PRAGMA journal_mode = WAL` + `migrate()`。
    pub fn open<P: AsRef<Path>>(book_dir: P) -> Result<Self> {
        let db_path = book_dir.as_ref().join("story").join("memory.db");
        let conn = Connection::open(&db_path)?;
        // 内存库（测试）会静默降级为 memory 模式，不报错；磁盘库启用 WAL。
        let _ = conn.pragma_update(None, "journal_mode", "WAL");
        let db = Self { conn };
        db.migrate()?;
        Ok(db)
    }

    /// 打开内存数据库（测试入口）。TS 版无此入口（node:sqlite 的 `:memory:` 需特殊处理），
    /// 这里作为纯 Rust 增益，使单测无需临时文件。
    pub fn open_in_memory() -> Result<Self> {
        let conn = Connection::open_in_memory()?;
        let db = Self { conn };
        db.migrate()?;
        Ok(db)
    }

    /// 执行 schema 迁移。DDL 与 TS `migrate()` 逐字对齐（表/列/索引/默认值）。
    fn migrate(&self) -> Result<()> {
        self.conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS facts (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                subject TEXT NOT NULL,
                predicate TEXT NOT NULL,
                object TEXT NOT NULL,
                valid_from_chapter INTEGER NOT NULL,
                valid_until_chapter INTEGER,
                source_chapter INTEGER NOT NULL,
                created_at TEXT NOT NULL DEFAULT (datetime('now'))
            );

            CREATE TABLE IF NOT EXISTS chapter_summaries (
                chapter INTEGER PRIMARY KEY,
                title TEXT NOT NULL,
                characters TEXT NOT NULL DEFAULT '',
                events TEXT NOT NULL DEFAULT '',
                state_changes TEXT NOT NULL DEFAULT '',
                hook_activity TEXT NOT NULL DEFAULT '',
                mood TEXT NOT NULL DEFAULT '',
                chapter_type TEXT NOT NULL DEFAULT ''
            );

            CREATE TABLE IF NOT EXISTS hooks (
                hook_id TEXT PRIMARY KEY,
                start_chapter INTEGER NOT NULL DEFAULT 0,
                type TEXT NOT NULL DEFAULT '',
                status TEXT NOT NULL DEFAULT 'open',
                last_advanced_chapter INTEGER NOT NULL DEFAULT 0,
                expected_payoff TEXT NOT NULL DEFAULT '',
                payoff_timing TEXT NOT NULL DEFAULT '',
                notes TEXT NOT NULL DEFAULT ''
            );

            CREATE INDEX IF NOT EXISTS idx_facts_subject ON facts(subject);
            CREATE INDEX IF NOT EXISTS idx_facts_valid ON facts(valid_from_chapter, valid_until_chapter);
            CREATE INDEX IF NOT EXISTS idx_facts_source ON facts(source_chapter);
            CREATE INDEX IF NOT EXISTS idx_hooks_status ON hooks(status);
            CREATE INDEX IF NOT EXISTS idx_hooks_last_advanced ON hooks(last_advanced_chapter);
            CREATE TABLE IF NOT EXISTS quality_debts (
                debt_id TEXT PRIMARY KEY,
                book_id TEXT NOT NULL DEFAULT '',
                chapter INTEGER NOT NULL DEFAULT 0,
                issue_category TEXT NOT NULL DEFAULT '',
                severity TEXT NOT NULL DEFAULT 'warning',
                status TEXT NOT NULL DEFAULT 'open',
                created_at TEXT NOT NULL DEFAULT '',
                follow_up_note TEXT NOT NULL DEFAULT ''
            );
            CREATE INDEX IF NOT EXISTS idx_debts_status ON quality_debts(status);
            CREATE INDEX IF NOT EXISTS idx_debts_book ON quality_debts(book_id, chapter);
            CREATE TABLE IF NOT EXISTS retrieval_chunks (
                chunk_id TEXT PRIMARY KEY,
                source TEXT NOT NULL DEFAULT '',
                fingerprint TEXT NOT NULL DEFAULT '',
                dim INTEGER NOT NULL DEFAULT 0,
                vector TEXT NOT NULL DEFAULT '[]',
                created_at TEXT NOT NULL DEFAULT ''
            );
            CREATE INDEX IF NOT EXISTS idx_chunks_source ON retrieval_chunks(source);",
        )?;

        // 与 TS ensureColumn("hooks", "payoff_timing", ...) 对齐：新表已含该列，
        // ALTER 会失败（"duplicate column"），静默忽略——保证旧库迁移路径等价。
        self.ensure_column("hooks", "payoff_timing", "TEXT NOT NULL DEFAULT ''")?;
        Ok(())
    }

    /// 等价 TS `ensureColumn`：尝试 `ALTER TABLE ... ADD COLUMN`，列已存在时静默忽略。
    fn ensure_column(&self, table: &str, column: &str, definition: &str) -> Result<()> {
        let sql = format!("ALTER TABLE {table} ADD COLUMN {column} {definition}");
        match self.conn.execute_batch(&sql) {
            Ok(()) => Ok(()),
            // SQLite 错误码 1 (SQLITE_ERROR) + "duplicate column name" 表示列已存在，
            // 与 TS 版的 try/catch 静默吞错等价。
            Err(rusqlite::Error::SqliteFailure(err, msg))
                if err.extended_code == rusqlite::ffi::SQLITE_ERROR
                    && msg
                        .as_deref()
                        .is_some_and(|m| m.contains("duplicate column name")) =>
            {
                Ok(())
            }
            Err(e) => Err(e.into()),
        }
    }

    // ---------------------------------------------------------------------------
    // Facts（temporal）
    // ---------------------------------------------------------------------------

    /// 新增 fact，返回自增 id。对齐 TS `addFact`。
    pub fn add_fact(&self, fact: &NewFact) -> Result<i64> {
        self.conn.execute(
            "INSERT INTO facts (subject, predicate, object, valid_from_chapter, valid_until_chapter, source_chapter)
             VALUES (?, ?, ?, ?, ?, ?)",
            params![
                fact.subject,
                fact.predicate,
                fact.object,
                fact.valid_from_chapter,
                fact.valid_until_chapter,
                fact.source_chapter,
            ],
        )?;
        Ok(self.conn.last_insert_rowid())
    }

    /// 失效一条 fact（设置 valid_until）。对齐 TS `invalidateFact`。
    pub fn invalidate_fact(&self, id: i64, until_chapter: i64) -> Result<()> {
        self.conn.execute(
            "UPDATE facts SET valid_until_chapter = ? WHERE id = ?",
            params![until_chapter, id],
        )?;
        Ok(())
    }

    /// 当前有效 facts（valid_until IS NULL），按 subject, predicate 排序。对齐 `getCurrentFacts`。
    pub fn get_current_facts(&self) -> Result<Vec<Fact>> {
        let sql = format!(
            "SELECT {FACT_SELECT_COLUMNS} FROM facts
             WHERE valid_until_chapter IS NULL
             ORDER BY subject, predicate"
        );
        let rows = self
            .conn
            .prepare(&sql)?
            .query_map([], row_to_fact)?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(rows)
    }

    /// 某角色在指定章节有效的 facts。对齐 `getFactsAt`。
    pub fn get_facts_at(&self, subject: &str, chapter: i64) -> Result<Vec<Fact>> {
        let sql = format!(
            "SELECT {FACT_SELECT_COLUMNS} FROM facts
             WHERE subject = ? AND valid_from_chapter <= ?
             AND (valid_until_chapter IS NULL OR valid_until_chapter > ?)
             ORDER BY predicate"
        );
        let rows = self
            .conn
            .prepare(&sql)?
            .query_map(params![subject, chapter, chapter], row_to_fact)?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(rows)
    }

    /// 某角色的全部 fact 历史。对齐 `getFactHistory`。
    pub fn get_fact_history(&self, subject: &str) -> Result<Vec<Fact>> {
        let sql = format!(
            "SELECT {FACT_SELECT_COLUMNS} FROM facts
             WHERE subject = ?
             ORDER BY valid_from_chapter"
        );
        let rows = self
            .conn
            .prepare(&sql)?
            .query_map(params![subject], row_to_fact)?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(rows)
    }

    /// 按 predicate 查询当前有效 facts。对齐 `getFactsByPredicate`。
    pub fn get_facts_by_predicate(&self, predicate: &str) -> Result<Vec<Fact>> {
        let sql = format!(
            "SELECT {FACT_SELECT_COLUMNS} FROM facts
             WHERE predicate = ? AND valid_until_chapter IS NULL
             ORDER BY subject"
        );
        let rows = self
            .conn
            .prepare(&sql)?
            .query_map(params![predicate], row_to_fact)?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(rows)
    }

    /// 与一组角色名相关的当前有效 facts。对齐 `getFactsForCharacters`。
    ///
    /// 空数组返回空（与 TS 一致，避免 `IN ()` 语法错误）。
    pub fn get_facts_for_characters(&self, names: &[String]) -> Result<Vec<Fact>> {
        if names.is_empty() {
            return Ok(Vec::new());
        }
        let placeholders = std::iter::repeat_n("?", names.len())
            .collect::<Vec<_>>()
            .join(",");
        let sql = format!(
            "SELECT {FACT_SELECT_COLUMNS} FROM facts
             WHERE subject IN ({placeholders}) AND valid_until_chapter IS NULL
             ORDER BY subject, predicate"
        );
        let rows = self
            .conn
            .prepare(&sql)?
            .query_map(params_from_iter(names.iter()), row_to_fact)?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(rows)
    }

    /// 清空当前有效 facts 后重新插入。对齐 `replaceCurrentFacts`。
    ///
    /// 注意：只删 `valid_until_chapter IS NULL` 的行（当前有效），历史 facts 保留——与 TS 一致。
    pub fn replace_current_facts(&self, facts: &[NewFact]) -> Result<()> {
        self.conn.execute("DELETE FROM facts WHERE valid_until_chapter IS NULL", [])?;
        for fact in facts {
            self.add_fact(fact)?;
        }
        Ok(())
    }

    /// 清空所有 facts。对齐 `resetFacts`。
    pub fn reset_facts(&self) -> Result<()> {
        self.conn.execute("DELETE FROM facts", [])?;
        Ok(())
    }

    // ---------------------------------------------------------------------------
    // Chapter summaries
    // ---------------------------------------------------------------------------

    /// G1/349 号：upsert chunk 向量（幂等）。
    pub fn upsert_chunk_vector(&self, chunk: &StoredChunkVector) -> Result<()> {
        let vector_json = serde_json::to_string(&chunk.vector)?;
        self.conn.execute(
            "INSERT OR REPLACE INTO retrieval_chunks (chunk_id, source, fingerprint, dim, vector, created_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            rusqlite::params![
                chunk.chunk_id,
                chunk.source,
                chunk.fingerprint,
                chunk.dim,
                vector_json,
                chunk.created_at,
            ],
        )?;
        Ok(())
    }

    /// chunk 向量全量（检索侧载入后内存余弦）。坏 JSON 行跳过。
    pub fn list_chunk_vectors(&self) -> Result<Vec<StoredChunkVector>> {
        let mut stmt = self
            .conn
            .prepare("SELECT chunk_id, source, fingerprint, dim, vector, created_at FROM retrieval_chunks")?;
        let rows = stmt.query_map([], |row| {
            Ok(StoredChunkVector {
                chunk_id: row.get(0)?,
                source: row.get(1)?,
                fingerprint: row.get(2)?,
                dim: row.get(3)?,
                vector: serde_json::from_str::<Vec<f64>>(row.get::<_, String>(4)?.as_str())
                    .unwrap_or_default(),
                created_at: row.get(5)?,
            })
        })?;
        let mut out = Vec::new();
        for row in rows {
            if let Ok(chunk) = row {
                out.push(chunk);
            }
        }
        Ok(out)
    }

    pub fn chunk_vector_count(&self) -> Result<u32> {
        let count: u32 = self
            .conn
            .query_row("SELECT COUNT(*) FROM retrieval_chunks", [], |row| row.get(0))?;
        Ok(count)
    }

    /// G3/337 号：记入一条质量债务（INSERT OR REPLACE，幂等）。
    pub fn record_debt(&self, debt: &StoredQualityDebt) -> Result<()> {
        self.conn.execute(
            "INSERT OR REPLACE INTO quality_debts (debt_id, book_id, chapter, issue_category, severity, status, created_at, follow_up_note)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
            rusqlite::params![
                debt.debt_id,
                debt.book_id,
                debt.chapter,
                debt.issue_category,
                debt.severity,
                debt.status,
                debt.created_at,
                debt.follow_up_note,
            ],
        )?;
        Ok(())
    }

    /// 债务清单：按书/状态过滤（None = 不过滤），新章在前。
    pub fn list_debts(
        &self,
        book_id: Option<&str>,
        status: Option<&str>,
    ) -> Result<Vec<StoredQualityDebt>> {
        let mut sql = String::from(
            "SELECT debt_id, book_id, chapter, issue_category, severity, status, created_at, follow_up_note \
             FROM quality_debts",
        );
        let mut clauses: Vec<String> = Vec::new();
        if book_id.is_some() {
            clauses.push("book_id = ?".into());
        }
        if status.is_some() {
            clauses.push("status = ?".into());
        }
        if !clauses.is_empty() {
            sql.push_str(" WHERE ");
            sql.push_str(&clauses.join(" AND "));
        }
        sql.push_str(" ORDER BY chapter DESC, created_at DESC");
        let mut stmt = self.conn.prepare(&sql)?;
        let rows = stmt.query_map(
            rusqlite::params_from_iter(
                book_id.iter().copied().chain(status.iter().copied()),
            ),
            |row| {
                Ok(StoredQualityDebt {
                    debt_id: row.get(0)?,
                    book_id: row.get(1)?,
                    chapter: row.get(2)?,
                    issue_category: row.get(3)?,
                    severity: row.get(4)?,
                    status: row.get(5)?,
                    created_at: row.get(6)?,
                    follow_up_note: row.get(7)?,
                })
            },
        )?;
        let mut debts = Vec::new();
        for row in rows {
            debts.push(row?);
        }
        Ok(debts)
    }

    /// 债务状态流转落库（附跟进备注）；返回是否更新到行。
    pub fn update_debt_status(
        &self,
        debt_id: &str,
        status: &str,
        follow_up_note: Option<&str>,
    ) -> Result<bool> {
        let changed = self.conn.execute(
            "UPDATE quality_debts SET status = ?1, follow_up_note = CASE WHEN ?2 = '' THEN follow_up_note ELSE ?2 END
             WHERE debt_id = ?3",
            rusqlite::params![status, follow_up_note.unwrap_or(""), debt_id],
        )?;
        Ok(changed > 0)
    }

    /// upsert 章节摘要。对齐 `upsertSummary`。
    pub fn upsert_summary(&self, summary: &StoredSummary) -> Result<()> {
        self.conn.execute(
            "INSERT OR REPLACE INTO chapter_summaries (chapter, title, characters, events, state_changes, hook_activity, mood, chapter_type)
             VALUES (?, ?, ?, ?, ?, ?, ?, ?)",
            params![
                summary.chapter,
                summary.title,
                summary.characters,
                summary.events,
                summary.state_changes,
                summary.hook_activity,
                summary.mood,
                summary.chapter_type,
            ],
        )?;
        Ok(())
    }

    /// 清空后批量插入摘要。对齐 `replaceSummaries`。
    pub fn replace_summaries(&self, summaries: &[StoredSummary]) -> Result<()> {
        self.conn.execute("DELETE FROM chapter_summaries", [])?;
        for summary in summaries {
            self.upsert_summary(summary)?;
        }
        Ok(())
    }

    /// 区间章节摘要，按 chapter 升序。对齐 `getSummaries`（区间含端点）。
    pub fn get_summaries(&self, from_chapter: i64, to_chapter: i64) -> Result<Vec<StoredSummary>> {
        let rows = self
            .conn
            .prepare(
                "SELECT chapter, title, characters, events, state_changes, hook_activity, mood, chapter_type
                 FROM chapter_summaries
                 WHERE chapter >= ? AND chapter <= ?
                 ORDER BY chapter",
            )?
            .query_map(params![from_chapter, to_chapter], row_to_summary)?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(rows)
    }

    /// 命中任一角色名的摘要（characters LIKE），按 chapter 升序。对齐 `getSummariesByCharacters`。
    ///
    /// 空数组返回空（与 TS 一致，避免空 WHERE 子句）。
    pub fn get_summaries_by_characters(&self, names: &[String]) -> Result<Vec<StoredSummary>> {
        if names.is_empty() {
            return Ok(Vec::new());
        }
        let conditions = names
            .iter()
            .map(|_| "characters LIKE ?")
            .collect::<Vec<_>>()
            .join(" OR ");
        let patterns: Vec<String> = names.iter().map(|n| format!("%{n}%")).collect();
        let sql = format!(
            "SELECT chapter, title, characters, events, state_changes, hook_activity, mood, chapter_type
             FROM chapter_summaries
             WHERE {conditions}
             ORDER BY chapter"
        );
        let rows = self
            .conn
            .prepare(&sql)?
            .query_map(params_from_iter(patterns.iter()), row_to_summary)?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(rows)
    }

    /// 章节总数。对齐 `getChapterCount`。
    pub fn get_chapter_count(&self) -> Result<i64> {
        let count: i64 = self.conn.query_row(
            "SELECT COUNT(*) as count FROM chapter_summaries",
            [],
            |row| row.get(0),
        )?;
        Ok(count)
    }

    /// 最近 N 条摘要，按 chapter 降序。对齐 `getRecentSummaries`。
    pub fn get_recent_summaries(&self, count: i64) -> Result<Vec<StoredSummary>> {
        let rows = self
            .conn
            .prepare(
                "SELECT chapter, title, characters, events, state_changes, hook_activity, mood, chapter_type
                 FROM chapter_summaries
                 ORDER BY chapter DESC
                 LIMIT ?",
            )?
            .query_map(params![count], row_to_summary)?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(rows)
    }

    // ---------------------------------------------------------------------------
    // Hooks
    // ---------------------------------------------------------------------------

    /// upsert hook。对齐 `upsertHook`（仅写持久化 8 列）。
    pub fn upsert_hook(&self, hook: &StoredHook) -> Result<()> {
        self.conn.execute(
            "INSERT OR REPLACE INTO hooks (hook_id, start_chapter, type, status, last_advanced_chapter, expected_payoff, payoff_timing, notes)
             VALUES (?, ?, ?, ?, ?, ?, ?, ?)",
            params![
                hook.hook_id,
                hook.start_chapter,
                hook.r#type,
                hook.status,
                hook.last_advanced_chapter,
                hook.expected_payoff,
                hook.payoff_timing,
                hook.notes,
            ],
        )?;
        Ok(())
    }

    /// 清空后批量插入 hooks。对齐 `replaceHooks`。
    pub fn replace_hooks(&self, hooks: &[StoredHook]) -> Result<()> {
        self.conn.execute("DELETE FROM hooks", [])?;
        for hook in hooks {
            self.upsert_hook(hook)?;
        }
        Ok(())
    }

    /// 活跃 hooks（排除 resolved/closed/已回收/已解决，大小写不敏感），按
    /// `last_advanced_chapter DESC, start_chapter DESC, hook_id ASC` 排序。对齐 `getActiveHooks`。
    /// G10/338 号：全量 hook（含已兑付）——承诺时间线数据源。
    pub fn get_all_hooks(&self) -> Result<Vec<StoredHook>> {
        let rows = self
            .conn
            .prepare(
                "SELECT hook_id, start_chapter, type, status, last_advanced_chapter, expected_payoff, payoff_timing, notes
                 FROM hooks
                 ORDER BY start_chapter ASC, hook_id ASC",
            )?
            .query_map([], row_to_hook)?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(rows)
    }

    pub fn get_active_hooks(&self) -> Result<Vec<StoredHook>> {
        let rows = self
            .conn
            .prepare(
                "SELECT hook_id, start_chapter, type, status, last_advanced_chapter, expected_payoff, payoff_timing, notes
                 FROM hooks
                 WHERE lower(status) NOT IN ('resolved', 'closed', '已回收', '已解决')
                 ORDER BY last_advanced_chapter DESC, start_chapter DESC, hook_id ASC",
            )?
            .query_map([], row_to_hook)?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(rows)
    }

    // ---------------------------------------------------------------------------
    // Lifecycle
    // ---------------------------------------------------------------------------

    /// 关闭连接。对齐 TS `close()`；Rust 的 Connection 在 Drop 时自动关闭，此方法消费 self 供显式提前关闭。
    pub fn close(self) -> Result<()> {
        // Connection 的 Drop 会自动关闭数据库句柄；这里显式消费即表明意图，
        // 无需手动调用底层 close（rusqlite 未公开等价 API）。
        drop(self.conn);
        Ok(())
    }
}

/// 从一行解析出 [`Fact`]。集中列→字段映射，避免每个查询重复。
fn row_to_fact(row: &Row) -> rusqlite::Result<Fact> {
    Ok(Fact {
        id: row.get("id")?,
        subject: row.get("subject")?,
        predicate: row.get("predicate")?,
        object: row.get("object")?,
        valid_from_chapter: row.get("valid_from_chapter")?,
        valid_until_chapter: row.get("valid_until_chapter")?,
        source_chapter: row.get("source_chapter")?,
    })
}

/// 从一行解析出 [`StoredSummary`]。
fn row_to_summary(row: &Row) -> rusqlite::Result<StoredSummary> {
    Ok(StoredSummary {
        chapter: row.get("chapter")?,
        title: row.get("title")?,
        characters: row.get("characters")?,
        events: row.get("events")?,
        state_changes: row.get("state_changes")?,
        hook_activity: row.get("hook_activity")?,
        mood: row.get("mood")?,
        chapter_type: row.get("chapter_type")?,
    })
}

/// 从一行解析出 [`StoredHook`]。
fn row_to_hook(row: &Row) -> rusqlite::Result<StoredHook> {
    Ok(StoredHook {
        hook_id: row.get("hook_id")?,
        start_chapter: row.get("start_chapter")?,
        r#type: row.get("type")?,
        status: row.get("status")?,
        last_advanced_chapter: row.get("last_advanced_chapter")?,
        expected_payoff: row.get("expected_payoff")?,
        payoff_timing: row.get("payoff_timing")?,
        notes: row.get("notes")?,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn db() -> MemoryDb {
        MemoryDb::open_in_memory().expect("open in-memory db")
    }

    fn sample_debt(id: &str, chapter: i64, status: &str) -> StoredQualityDebt {
        StoredQualityDebt {
            debt_id: id.to_string(),
            book_id: "book-1".to_string(),
            chapter,
            issue_category: "continuity".to_string(),
            severity: "critical".to_string(),
            status: status.to_string(),
            created_at: "2026-09-12T00:00:00.000Z".to_string(),
            follow_up_note: String::new(),
        }
    }

    #[test]
    fn quality_debts_record_list_update_roundtrip() {
        let db = db();
        db.record_debt(&sample_debt("d1", 3, "deferred")).unwrap();
        db.record_debt(&sample_debt("d2", 5, "open")).unwrap();
        db.record_debt(&sample_debt("d3", 1, "resolved")).unwrap();

        // 全量：新章在前。
        let all = db.list_debts(None, None).unwrap();
        assert_eq!(all.len(), 3);
        assert_eq!(all[0].debt_id, "d2");

        // 状态过滤。
        let open: Vec<_> = db.list_debts(None, Some("open")).unwrap();
        assert_eq!(open.len(), 1);
        assert_eq!(open[0].debt_id, "d2");

        // 书过滤 + 状态更新。
        assert!(db.update_debt_status("d2", "resolved", Some("patched")).unwrap());
        let resolved: Vec<_> = db.list_debts(Some("book-1"), Some("resolved")).unwrap();
        assert_eq!(resolved.len(), 2);
        let d2 = resolved.iter().find(|d| d.debt_id == "d2").unwrap();
        assert_eq!(d2.follow_up_note, "patched");

        // 不存在的 id → false。
        assert!(!db.update_debt_status("nope", "open", None).unwrap());
    }

    // --- schema / 迁移 ----------------------------------------------------------

    #[test]
    fn open_in_memory_creates_all_three_tables() {
        let db = db();
        let count: i64 = db
            .conn
            .query_row(
                "SELECT COUNT(*) FROM sqlite_master WHERE type='table' AND name IN ('facts','chapter_summaries','hooks')",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(count, 3, "三张业务表应被 migrate 创建");
    }

    #[test]
    fn facts_table_columns_match_ts_ddl() {
        let db = db();
        let cols: Vec<String> = db
            .conn
            .prepare("PRAGMA table_info(facts)")
            .unwrap()
            .query_map([], |row| row.get::<_, String>(1))
            .unwrap()
            .map(|r| r.unwrap())
            .collect();
        assert_eq!(
            cols,
            vec![
                "id",
                "subject",
                "predicate",
                "object",
                "valid_from_chapter",
                "valid_until_chapter",
                "source_chapter",
                "created_at",
            ]
        );
    }

    #[test]
    fn hooks_default_status_is_open() {
        // 迁移时 hooks.status 默认 'open'：插入仅含主键的行应得到 'open'。
        let db = db();
        db.conn
            .execute(
                "INSERT INTO hooks (hook_id) VALUES ('h-default')",
                [],
            )
            .unwrap();
        let status: String = db
            .conn
            .query_row("SELECT status FROM hooks WHERE hook_id='h-default'", [], |row| {
                row.get(0)
            })
            .unwrap();
        assert_eq!(status, "open");
    }

    #[test]
    fn migrate_is_idempotent() {
        // 重复 open（同库）不应因 ensure_column 的 payoff_timing ALTER 报错。
        // 用内存库无法跨连接共享，改为在同一连接上重跑 migrate（公开行为等价）。
        let db = db();
        db.migrate().expect("二次 migrate 不应报错");
    }

    #[test]
    fn ensure_column_swallows_duplicate_column_error() {
        let db = db();
        // facts.subject 已存在 → 应静默 Ok，而非向上抛错。
        db.ensure_column("facts", "subject", "TEXT")
            .expect("列已存在时应静默忽略");
    }

    // --- facts（temporal）------------------------------------------------------

    fn fact(subject: &str, predicate: &str, object: &str, from: i64, source: i64) -> NewFact {
        NewFact {
            subject: subject.to_string(),
            predicate: predicate.to_string(),
            object: object.to_string(),
            valid_from_chapter: from,
            valid_until_chapter: None,
            source_chapter: source,
        }
    }

    #[test]
    fn add_fact_returns_autoincrement_id() {
        let db = db();
        let id1 = db.add_fact(&fact("alice", "location", "forest", 1, 1)).unwrap();
        let id2 = db.add_fact(&fact("bob", "location", "town", 1, 1)).unwrap();
        assert!(id2 > id1, "自增 id 应递增（got {id1} → {id2}）");
    }

    #[test]
    fn get_current_facts_excludes_invalidated_and_sorts_by_subject_then_predicate() {
        let db = db();
        db.add_fact(&fact("alice", "weapon", "sword", 1, 1)).unwrap();
        db.add_fact(&fact("alice", "location", "forest", 1, 1)).unwrap();
        let old = db.add_fact(&fact("alice", "location", "village", 1, 1)).unwrap();
        db.add_fact(&fact("bob", "mood", "happy", 2, 2)).unwrap();
        db.invalidate_fact(old, 3).unwrap();

        let current = db.get_current_facts().unwrap();
        // 排除已失效的 village；按 subject 升序、predicate 升序。
        let pairs: Vec<(&str, &str)> = current
            .iter()
            .map(|f| (f.subject.as_str(), f.predicate.as_str()))
            .collect();
        assert_eq!(
            pairs,
            vec![
                ("alice", "location"), // forest（village 已失效）
                ("alice", "weapon"),
                ("bob", "mood"),
            ]
        );
    }

    #[test]
    fn get_facts_at_respects_temporal_window() {
        let db = db();
        // alice.location: 第 1-5 章（valid_until=6 表示 >6 有效即 [1,6)）
        let loc_id = db.add_fact(&fact("alice", "location", "forest", 1, 1)).unwrap();
        db.invalidate_fact(loc_id, 6).unwrap();
        // alice.weapon: 第 3 章起，持续有效
        db.add_fact(&fact("alice", "weapon", "sword", 3, 3)).unwrap();

        // 第 2 章：location 有效（1<=2 且 6>2），weapon 无效（3>2）
        let at_2 = db.get_facts_at("alice", 2).unwrap();
        assert_eq!(at_2.len(), 1);
        assert_eq!(at_2[0].predicate, "location");

        // 第 5 章：location 有效（6>5），weapon 有效
        let at_5 = db.get_facts_at("alice", 5).unwrap();
        assert_eq!(at_5.len(), 2);

        // 第 6 章：location 已失效（valid_until=6 不 >6），weapon 有效
        let at_6 = db.get_facts_at("alice", 6).unwrap();
        assert_eq!(at_6.len(), 1);
        assert_eq!(at_6[0].predicate, "weapon");
    }

    #[test]
    fn get_fact_history_includes_invalidated_sorted_by_valid_from() {
        let db = db();
        let old = db.add_fact(&fact("alice", "location", "village", 1, 1)).unwrap();
        db.invalidate_fact(old, 5).unwrap();
        db.add_fact(&fact("alice", "location", "forest", 5, 5)).unwrap();

        let history = db.get_fact_history("alice").unwrap();
        assert_eq!(history.len(), 2);
        // 按 valid_from_chapter 升序。
        assert_eq!(history[0].object, "village");
        assert_eq!(history[1].object, "forest");
    }

    #[test]
    fn get_facts_by_predicate_filters_current_only_and_sorts_by_subject() {
        let db = db();
        db.add_fact(&fact("alice", "location", "forest", 1, 1)).unwrap();
        db.add_fact(&fact("carol", "location", "lake", 1, 1)).unwrap();
        let bob_old = db.add_fact(&fact("bob", "location", "inn", 1, 1)).unwrap();
        db.invalidate_fact(bob_old, 2).unwrap();
        db.add_fact(&fact("alice", "weapon", "sword", 1, 1)).unwrap();

        let locs = db.get_facts_by_predicate("location").unwrap();
        let subjects: Vec<&str> = locs.iter().map(|f| f.subject.as_str()).collect();
        // bob 的 location 已失效被排除；按 subject 升序。
        assert_eq!(subjects, vec!["alice", "carol"]);
    }

    #[test]
    fn get_facts_for_characters_empty_names_returns_empty() {
        let db = db();
        db.add_fact(&fact("alice", "location", "forest", 1, 1)).unwrap();
        assert!(db.get_facts_for_characters(&[]).unwrap().is_empty());
    }

    #[test]
    fn get_facts_for_characters_matches_any_current_only() {
        let db = db();
        db.add_fact(&fact("alice", "location", "forest", 1, 1)).unwrap();
        db.add_fact(&fact("bob", "mood", "happy", 1, 1)).unwrap();
        db.add_fact(&fact("carol", "weapon", "bow", 1, 1)).unwrap();
        let names = vec!["alice".to_string(), "carol".to_string()];
        let got = db.get_facts_for_characters(&names).unwrap();
        let subjects: Vec<&str> = got.iter().map(|f| f.subject.as_str()).collect();
        assert_eq!(subjects, vec!["alice", "carol"]);
    }

    #[test]
    fn invalidate_fact_sets_valid_until_chapter() {
        let db = db();
        let id = db.add_fact(&fact("alice", "location", "forest", 1, 1)).unwrap();
        db.invalidate_fact(id, 9).unwrap();
        let until: Option<i64> = db
            .conn
            .query_row("SELECT valid_until_chapter FROM facts WHERE id=?", params![id], |row| {
                row.get(0)
            })
            .unwrap();
        assert_eq!(until, Some(9));
    }

    #[test]
    fn replace_current_facts_clears_current_but_keeps_history() {
        let db = db();
        let old = db.add_fact(&fact("alice", "location", "village", 1, 1)).unwrap();
        db.invalidate_fact(old, 3).unwrap(); // 历史保留
        db.add_fact(&fact("alice", "location", "forest", 3, 3)).unwrap(); // 当前

        db.replace_current_facts(&[fact("bob", "location", "town", 5, 5)]).unwrap();

        let current = db.get_current_facts().unwrap();
        assert_eq!(current.len(), 1);
        assert_eq!(current[0].subject, "bob");
        // 历史的 village 未被清（forest 是当前有效，被 replace 清掉）。
        let history = db.get_fact_history("alice").unwrap();
        assert_eq!(history.len(), 1);
        assert_eq!(history[0].object, "village");
    }

    #[test]
    fn reset_facts_clears_everything() {
        let db = db();
        let old = db.add_fact(&fact("alice", "location", "village", 1, 1)).unwrap();
        db.invalidate_fact(old, 3).unwrap();
        db.add_fact(&fact("alice", "location", "forest", 3, 3)).unwrap();

        db.reset_facts().unwrap();
        assert_eq!(db.get_current_facts().unwrap().len(), 0);
        assert_eq!(db.get_fact_history("alice").unwrap().len(), 0);
    }

    // --- chapter summaries -----------------------------------------------------

    fn summary(chapter: i64, title: &str, characters: &str) -> StoredSummary {
        StoredSummary {
            chapter,
            title: title.to_string(),
            characters: characters.to_string(),
            events: String::new(),
            state_changes: String::new(),
            hook_activity: String::new(),
            mood: String::new(),
            chapter_type: String::new(),
        }
    }

    #[test]
    fn upsert_summary_replaces_existing_chapter() {
        let db = db();
        db.upsert_summary(&summary(1, "初稿", "alice")).unwrap();
        db.upsert_summary(&summary(1, "定稿", "alice,bob")).unwrap();
        let got = db.get_summaries(1, 1).unwrap();
        assert_eq!(got.len(), 1);
        assert_eq!(got[0].title, "定稿");
        assert_eq!(got[0].characters, "alice,bob");
    }

    #[test]
    fn get_summaries_returns_inclusive_range_sorted_ascending() {
        let db = db();
        for ch in [1, 2, 3, 4, 5] {
            db.upsert_summary(&summary(ch, &format!("t{ch}"), "")).unwrap();
        }
        let got = db.get_summaries(2, 4).unwrap();
        let chapters: Vec<i64> = got.iter().map(|s| s.chapter).collect();
        assert_eq!(chapters, vec![2, 3, 4]);
    }

    #[test]
    fn get_summaries_by_characters_empty_returns_empty() {
        let db = db();
        db.upsert_summary(&summary(1, "t1", "alice")).unwrap();
        assert!(db.get_summaries_by_characters(&[]).unwrap().is_empty());
    }

    #[test]
    fn get_summaries_by_characters_matches_any_via_like() {
        let db = db();
        db.upsert_summary(&summary(1, "t1", "alice")).unwrap();
        db.upsert_summary(&summary(2, "t2", "bob")).unwrap();
        db.upsert_summary(&summary(3, "t3", "carol")).unwrap();
        // LIKE 子串匹配：alice 应命中第 1 章；无 alice 的 bob/carol 不命中。
        let names = vec!["alice".to_string()];
        let got = db.get_summaries_by_characters(&names).unwrap();
        let chapters: Vec<i64> = got.iter().map(|s| s.chapter).collect();
        assert_eq!(chapters, vec![1]);
    }

    #[test]
    fn get_summaries_by_characters_matches_multiple_names_or() {
        let db = db();
        db.upsert_summary(&summary(1, "t1", "alice")).unwrap();
        db.upsert_summary(&summary(2, "t2", "bob")).unwrap();
        db.upsert_summary(&summary(3, "t3", "carol")).unwrap();
        let names = vec!["bob".to_string(), "carol".to_string()];
        let got = db.get_summaries_by_characters(&names).unwrap();
        let chapters: Vec<i64> = got.iter().map(|s| s.chapter).collect();
        assert_eq!(chapters, vec![2, 3]);
    }

    #[test]
    fn get_chapter_count_returns_total_rows() {
        let db = db();
        assert_eq!(db.get_chapter_count().unwrap(), 0);
        db.upsert_summary(&summary(1, "t1", "")).unwrap();
        db.upsert_summary(&summary(2, "t2", "")).unwrap();
        db.upsert_summary(&summary(1, "t1-overwrite", "")).unwrap(); // upsert 不增计数
        assert_eq!(db.get_chapter_count().unwrap(), 2);
    }

    #[test]
    fn get_recent_summaries_returns_n_sorted_desc() {
        let db = db();
        for ch in [1, 2, 3] {
            db.upsert_summary(&summary(ch, &format!("t{ch}"), "")).unwrap();
        }
        let got = db.get_recent_summaries(2).unwrap();
        let chapters: Vec<i64> = got.iter().map(|s| s.chapter).collect();
        assert_eq!(chapters, vec![3, 2]);
    }

    #[test]
    fn replace_summaries_clears_then_inserts() {
        let db = db();
        db.upsert_summary(&summary(1, "old", "alice")).unwrap();
        db.replace_summaries(&[summary(10, "new", "bob")]).unwrap();
        let got = db.get_summaries(0, 100).unwrap();
        assert_eq!(got.len(), 1);
        assert_eq!(got[0].chapter, 10);
        assert_eq!(got[0].title, "new");
    }

    // --- hooks -----------------------------------------------------------------

    fn hook(id: &str, start: i64, status: &str, last_advanced: i64) -> StoredHook {
        StoredHook {
            hook_id: id.to_string(),
            start_chapter: start,
            r#type: "mystery".to_string(),
            status: status.to_string(),
            last_advanced_chapter: last_advanced,
            expected_payoff: String::new(),
            payoff_timing: String::new(),
            notes: String::new(),
        }
    }

    #[test]
    fn upsert_hook_replaces_existing_id() {
        let db = db();
        db.upsert_hook(&hook("h1", 1, "open", 5)).unwrap();
        db.upsert_hook(&hook("h1", 1, "resolved", 9)).unwrap();
        let active = db.get_active_hooks().unwrap();
        assert!(active.is_empty(), "覆写为 resolved 后应不在活跃集");
    }

    #[test]
    fn upsert_hook_normalizes_missing_payoff_timing_to_empty() {
        // TS 版 upsert 写 8 列，payoff_timing 直接落库；这里仅验证默认列存在且可写。
        let db = db();
        db.upsert_hook(&hook("h1", 1, "open", 1)).unwrap();
        let timing: String = db
            .conn
            .query_row("SELECT payoff_timing FROM hooks WHERE hook_id='h1'", [], |row| {
                row.get(0)
            })
            .unwrap();
        assert_eq!(timing, "");
    }

    #[test]
    fn get_active_hooks_excludes_resolved_closed_and_chinese_statuses() {
        let db = db();
        db.upsert_hook(&hook("h-open", 1, "open", 1)).unwrap();
        db.upsert_hook(&hook("h-pending", 2, "pending", 2)).unwrap();
        // 大小写 + 中英文状态黑名单。
        db.upsert_hook(&hook("h-resolved", 3, "resolved", 3)).unwrap();
        db.upsert_hook(&hook("h-closed", 4, "closed", 4)).unwrap();
        db.upsert_hook(&hook("h-resolved-cn", 5, "已解决", 5)).unwrap();
        db.upsert_hook(&hook("h-recycled-cn", 6, "已回收", 6)).unwrap();

        let active = db.get_active_hooks().unwrap();
        let ids: Vec<&str> = active.iter().map(|h| h.hook_id.as_str()).collect();
        // h-pending 的 last_advanced(2) > h-open(1)，DESC → pending 在前。
        assert_eq!(ids, vec!["h-pending", "h-open"]);
    }

    #[test]
    fn get_active_hooks_sorts_by_last_advanced_then_start_then_id() {
        let db = db();
        // 同 last_advanced(5)：start 降序（b 先于 a）；start 也同则 hook_id 升序。
        db.upsert_hook(&hook("a", 1, "open", 5)).unwrap();
        db.upsert_hook(&hook("b", 3, "open", 5)).unwrap();
        db.upsert_hook(&hook("c", 2, "open", 9)).unwrap(); // last_advanced 最大，排第一

        let active = db.get_active_hooks().unwrap();
        let ids: Vec<&str> = active.iter().map(|h| h.hook_id.as_str()).collect();
        // c(9) → b(start=3) → a(start=1)
        assert_eq!(ids, vec!["c", "b", "a"]);
    }

    #[test]
    fn get_active_hooks_id_ascending_as_tiebreaker() {
        let db = db();
        // last_advanced 与 start 都相同 → hook_id 升序。
        db.upsert_hook(&hook("zeta", 2, "open", 5)).unwrap();
        db.upsert_hook(&hook("alpha", 2, "open", 5)).unwrap();
        db.upsert_hook(&hook("mid", 2, "open", 5)).unwrap();

        let active = db.get_active_hooks().unwrap();
        let ids: Vec<&str> = active.iter().map(|h| h.hook_id.as_str()).collect();
        assert_eq!(ids, vec!["alpha", "mid", "zeta"]);
    }

    #[test]
    fn replace_hooks_clears_then_inserts() {
        let db = db();
        db.upsert_hook(&hook("old", 1, "open", 1)).unwrap();
        db.replace_hooks(&[hook("new1", 2, "open", 2), hook("new2", 3, "open", 3)]).unwrap();
        let active = db.get_active_hooks().unwrap();
        let ids: Vec<&str> = active.iter().map(|h| h.hook_id.as_str()).collect();
        assert_eq!(ids, vec!["new2", "new1"]); // 按 last_advanced 降序
    }

    #[test]
    fn close_consumes_connection_without_panic() {
        let db = db();
        db.upsert_hook(&hook("h1", 1, "open", 1)).unwrap();
        // close 消费 self 并释放连接；不 panic 即视为成功。
        db.close().expect("显式关闭不应报错");
    }
}
