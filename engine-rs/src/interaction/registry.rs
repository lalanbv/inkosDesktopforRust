//! 工具注册表（R38a，547 号）——dsh `packages/core/tools` 工具面的 InkOS
//! 收拢形态（学模式不搬框架，v7 路线 R38 批一）。
//!
//! 批一范围：文件三件双作用域族（项目作用域 / 书会话作用域）的
//! 名称→(schema, 执行器, 变更面定性) 同址注册。此前 schema（interaction_tools /
//! book_file_tool_schemas）与执行（execute_tool / execute_book_file_tool）分居
//! 两处，同名 read/ls/grep 靠 ChatToolRouter 尾部分发顺序隐式消解——dsh 三重
//! 经验对应：
//! - ToolDefinition 的 name/description/parameters/execute 元组 → [`ToolDef`]；
//! - scope layers「最近层胜出」→ [`ToolScope`] 分层遮蔽（[`execute_shadowed`]）；
//! - restrictions 变更面定性 → [`MutationKind`]（R38b 全族注册后接
//!   suppressProductionTools，替代 PRODUCTION_MUTATION_TOOL_NAMES 字符串集）。
//!
//! 装配为 [`ToolRegistry::global`] OnceLock 静态单例（ZST 单元，构建一次，
//! 无 Box::leak / 无静态堆引用），查找按 &str 线性扫（层内 ≤3 项，零分配；
//! 声明序稳定 = schema 投影序稳定）。

use std::path::Path;
use std::sync::OnceLock;

use serde_json::{json, Value};

use super::project_tools::{self, ToolResult};

/// 变更面定性（dsh restrictions 收拢；R38b 全族注册后由 suppressProductionTools
/// 面消费）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MutationKind {
    /// 只读：无落盘、无项目外副作用。
    ReadOnly,
    /// 项目内落盘写入（素材入库等；生产变更面判定 R38b 接线）。
    #[allow(dead_code)] // 批一全族只读，无构造点；R38b 编辑族注册后即消费
    ProjectWrite,
}

/// 作用域层：同名工具最近层胜出（dsh scope layers 语义）——书会话层遮蔽
/// 项目层的 read/ls/grep，material 等项目层工具在书会话继续可达。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ToolScope {
    /// 项目根作用域（回环尾面，全会话可达）。
    Project,
    /// 书会话作用域（book/edit 会话，books/ 相对解析）。
    BookSession,
}

/// 工具执行上下文（借用不克隆；后续批次扩会话/路由句柄时在此加字段）。
pub struct ToolCtx<'a> {
    pub root: &'a Path,
}

/// 工具定义：schema 三元组 + 变更面定性 + 执行器（dsh ToolDefinition 同址
/// 形态）。实现体为 ZST，注册表静态装配。
#[async_trait::async_trait]
pub trait ToolDef: Send + Sync {
    /// 工具名（查找键；两作用域族允许同名，分发由作用域层消解）。
    fn name(&self) -> &'static str;
    /// 描述（schema 面；书 read 随 INKOS_AGENT_ALLOW_SYSTEM_READ 分流）。
    fn description(&self) -> &'static str;
    /// 参数 schema（json! 每次构建——与原 interaction_tools 行为一致；
    /// 装配面每请求一次，非热路径）。
    fn parameters(&self) -> Value;
    fn mutation_kind(&self) -> MutationKind;
    async fn execute(&self, ctx: &ToolCtx<'_>, args: &Value) -> ToolResult;
}

// ── 项目作用域族（read/ls/grep，TS project-tools 移植面） ──────────────────

struct ProjectRead;
struct ProjectLs;
struct ProjectGrep;

#[async_trait::async_trait]
impl ToolDef for ProjectRead {
    fn name(&self) -> &'static str {
        "read"
    }
    fn description(&self) -> &'static str {
        "读取项目内文本文件内容"
    }
    fn parameters(&self) -> Value {
        json!({
            "type": "object",
            "properties": { "path": { "type": "string", "description": "项目相对路径" } },
            "required": ["path"],
        })
    }
    fn mutation_kind(&self) -> MutationKind {
        MutationKind::ReadOnly
    }
    async fn execute(&self, ctx: &ToolCtx<'_>, args: &Value) -> ToolResult {
        project_tools::tool_read(ctx.root, args).await
    }
}

#[async_trait::async_trait]
impl ToolDef for ProjectLs {
    fn name(&self) -> &'static str {
        "ls"
    }
    fn description(&self) -> &'static str {
        "列出项目目录内容"
    }
    fn parameters(&self) -> Value {
        json!({
            "type": "object",
            "properties": { "path": { "type": "string", "description": "项目相对路径（默认 .）" } },
        })
    }
    fn mutation_kind(&self) -> MutationKind {
        MutationKind::ReadOnly
    }
    async fn execute(&self, ctx: &ToolCtx<'_>, args: &Value) -> ToolResult {
        project_tools::tool_ls(ctx.root, args).await
    }
}

#[async_trait::async_trait]
impl ToolDef for ProjectGrep {
    fn name(&self) -> &'static str {
        "grep"
    }
    fn description(&self) -> &'static str {
        "在项目文本文件中搜索"
    }
    fn parameters(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "query": { "type": "string" },
                "path": { "type": "string", "description": "搜索根（默认 .）" },
            },
            "required": ["query"],
        })
    }
    fn mutation_kind(&self) -> MutationKind {
        MutationKind::ReadOnly
    }
    async fn execute(&self, ctx: &ToolCtx<'_>, args: &Value) -> ToolResult {
        project_tools::tool_grep(ctx.root, args).await
    }
}

// ── 书会话作用域族（105 号，TS createReadTool/createLsTool/createGrepTool 逐字） ──

struct BookRead;
struct BookLs;
struct BookGrep;

#[async_trait::async_trait]
impl ToolDef for BookRead {
    fn name(&self) -> &'static str {
        "read"
    }
    fn description(&self) -> &'static str {
        if project_tools::allow_system_read() {
            "Read a file. Relative paths resolve under books/; absolute paths read from the system filesystem."
        } else {
            "Read a file from the book directory. Path is relative to books/."
        }
    }
    fn parameters(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "path": { "type": "string", "description": "File path relative to books/, or an absolute path when system path reading is enabled." },
            },
            "required": ["path"],
        })
    }
    fn mutation_kind(&self) -> MutationKind {
        MutationKind::ReadOnly
    }
    async fn execute(&self, ctx: &ToolCtx<'_>, args: &Value) -> ToolResult {
        project_tools::tool_read_book(ctx.root, args).await
    }
}

#[async_trait::async_trait]
impl ToolDef for BookLs {
    fn name(&self) -> &'static str {
        "ls"
    }
    fn description(&self) -> &'static str {
        "List files in a book directory. Optionally specify a subdirectory like 'story' or 'chapters'."
    }
    fn parameters(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "bookId": { "type": "string", "description": "Book ID" },
                "subdir": { "type": "string", "description": "Subdirectory within the book, e.g. 'story', 'chapters', 'story/runtime'" },
            },
            "required": ["bookId"],
        })
    }
    fn mutation_kind(&self) -> MutationKind {
        MutationKind::ReadOnly
    }
    async fn execute(&self, ctx: &ToolCtx<'_>, args: &Value) -> ToolResult {
        project_tools::tool_ls_book(ctx.root, args).await
    }
}

#[async_trait::async_trait]
impl ToolDef for BookGrep {
    fn name(&self) -> &'static str {
        "grep"
    }
    fn description(&self) -> &'static str {
        "Search for a text pattern across a book's story/ and chapters/ directories. Returns matching lines."
    }
    fn parameters(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "bookId": { "type": "string", "description": "Book ID to search within" },
                "pattern": { "type": "string", "description": "Search pattern (plain text or regex)" },
            },
            "required": ["bookId", "pattern"],
        })
    }
    fn mutation_kind(&self) -> MutationKind {
        MutationKind::ReadOnly
    }
    async fn execute(&self, ctx: &ToolCtx<'_>, args: &Value) -> ToolResult {
        project_tools::tool_grep_book(ctx.root, args).await
    }
}

// ── 注册表与分发面 ─────────────────────────────────────────────────────────

/// 单条 schema 投影（名称/描述/参数——schema 面消费的中间形态，
/// name/description 保持 'static 免复制）。
pub struct ToolEntry {
    pub name: &'static str,
    pub description: &'static str,
    pub parameters: Value,
}

struct ToolLayer {
    scope: ToolScope,
    defs: Vec<Box<dyn ToolDef>>,
}

/// 双作用域工具注册表（静态装配，查找零分配，声明序 = 投影序）。
pub struct ToolRegistry {
    layers: Vec<ToolLayer>,
}

impl ToolRegistry {
    /// 全局注册表（OnceLock 装配一次；ZST 单元无堆泄漏面）。
    pub fn global() -> &'static Self {
        static REGISTRY: OnceLock<ToolRegistry> = OnceLock::new();
        REGISTRY.get_or_init(|| Self {
            layers: vec![
                ToolLayer {
                    scope: ToolScope::Project,
                    defs: vec![Box::new(ProjectRead), Box::new(ProjectLs), Box::new(ProjectGrep)],
                },
                ToolLayer {
                    scope: ToolScope::BookSession,
                    defs: vec![Box::new(BookRead), Box::new(BookLs), Box::new(BookGrep)],
                },
            ],
        })
    }

    fn defs(&self, scope: ToolScope) -> impl Iterator<Item = &dyn ToolDef> {
        self.layers
            .iter()
            .filter(move |layer| layer.scope == scope)
            .flat_map(|layer| layer.defs.iter())
            .map(|def| def.as_ref())
    }

    /// 单层查找（&str 线性扫）。
    pub fn get(&self, scope: ToolScope, name: &str) -> Option<&dyn ToolDef> {
        self.defs(scope).find(|def| def.name() == name)
    }

    /// 单层 schema 投影（声明序稳定）。
    pub fn entries(&self, scope: ToolScope) -> Vec<ToolEntry> {
        self.defs(scope)
            .map(|def| ToolEntry { name: def.name(), description: def.description(), parameters: def.parameters() })
            .collect()
    }

    /// 单层 OpenAI function schema 投影。
    pub fn schemas(&self, scope: ToolScope) -> Vec<Value> {
        self.entries(scope)
            .into_iter()
            .map(|entry| {
                json!({
                    "type": "function",
                    "function": {
                        "name": entry.name,
                        "description": entry.description,
                        "parameters": entry.parameters,
                    },
                })
            })
            .collect()
    }
}

/// 书会话作用域分发（原 execute_book_file_tool 语义；未命中返回 None）。
pub async fn execute_book_file(root: &Path, name: &str, args: &Value) -> Option<ToolResult> {
    let def = ToolRegistry::global().get(ToolScope::BookSession, name)?;
    Some(def.execute(&ToolCtx { root }, args).await)
}

/// 项目作用域分发（原 execute_tool 文件三件分支语义；未命中返回 None）。
pub async fn execute_project_file(root: &Path, name: &str, args: &Value) -> Option<ToolResult> {
    let def = ToolRegistry::global().get(ToolScope::Project, name)?;
    Some(def.execute(&ToolCtx { root }, args).await)
}

/// 分层遮蔽分发（ChatToolRouter 回环尾面）：`book_session=true` 时书层先查
/// （最近层胜出，同名 read/ls/grep 书会话语义生效），未命中落项目层；两层皆
/// 未命中 → None（调用方继续 material 双件 / 未知工具原链）。
pub async fn execute_shadowed(root: &Path, name: &str, book_session: bool, args: &Value) -> Option<ToolResult> {
    let registry = ToolRegistry::global();
    if book_session {
        if let Some(def) = registry.get(ToolScope::BookSession, name) {
            return Some(def.execute(&ToolCtx { root }, args).await);
        }
    }
    let def = registry.get(ToolScope::Project, name)?;
    Some(def.execute(&ToolCtx { root }, args).await)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn project_layer_projection_order_and_shape() {
        let schemas = ToolRegistry::global().schemas(ToolScope::Project);
        let names: Vec<&str> = schemas
            .iter()
            .map(|s| s["function"]["name"].as_str().unwrap())
            .collect();
        assert_eq!(names, vec!["read", "ls", "grep"], "声明序 = 投影序");
        assert_eq!(schemas[0]["function"]["description"], "读取项目内文本文件内容");
        assert_eq!(schemas[0]["function"]["parameters"]["required"][0], "path");
        assert_eq!(schemas[2]["function"]["parameters"]["required"][0], "query");
    }

    #[test]
    fn book_layer_projection_matches_ts_verbatim() {
        let schemas = ToolRegistry::global().schemas(ToolScope::BookSession);
        let names: Vec<&str> = schemas
            .iter()
            .map(|s| s["function"]["name"].as_str().unwrap())
            .collect();
        assert_eq!(names, vec!["read", "ls", "grep"]);
        // 缺省（系统读关闭）→ 相对路径描述（TS 逐字）。
        assert_eq!(
            schemas[0]["function"]["description"],
            "Read a file from the book directory. Path is relative to books/."
        );
        assert_eq!(schemas[1]["function"]["parameters"]["required"][0], "bookId");
        assert_eq!(schemas[2]["function"]["parameters"]["required"], json!(["bookId", "pattern"]));
    }

    #[test]
    fn mutation_kinds_read_only() {
        let registry = ToolRegistry::global();
        for scope in [ToolScope::Project, ToolScope::BookSession] {
            for name in ["read", "ls", "grep"] {
                assert_eq!(
                    registry.get(scope, name).unwrap().mutation_kind(),
                    MutationKind::ReadOnly,
                    "{scope:?}/{name} 只读"
                );
            }
        }
    }

    #[tokio::test]
    async fn shadowing_book_layer_wins_in_book_sessions() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        std::fs::write(root.join("note.md"), "project-scope").unwrap();
        std::fs::create_dir_all(root.join("books").join("b1")).unwrap();
        std::fs::write(root.join("books").join("b1").join("book.json"), "{\"id\":\"b1\"}").unwrap();

        // 书会话：read 命中书层（books/ 相对解析）。
        let book = execute_shadowed(root, "read", true, &json!({ "path": "b1/book.json" }))
            .await
            .unwrap();
        assert!(book.text.contains("b1"), "书层遮蔽生效：{}", book.text);
        // 书层失败为非错误文本（TS textResult 逐字），项目根文件按书层语义不可达。
        let miss = execute_shadowed(root, "read", true, &json!({ "path": "note.md" }))
            .await
            .unwrap();
        assert!(!miss.is_error, "书 read 失败为非错误文本（TS textResult 形态）");
        // 非书会话：项目层。
        let project = execute_shadowed(root, "read", false, &json!({ "path": "note.md" }))
            .await
            .unwrap();
        assert!(project.text.contains("project-scope"));
        assert!(!project.is_error);
    }

    #[tokio::test]
    async fn unknown_tool_falls_through_both_layers() {
        let dir = tempfile::tempdir().unwrap();
        assert!(
            execute_shadowed(dir.path(), "propose_action", true, &json!({}))
                .await
                .is_none(),
            "未收族（propose_action）不劫持，留给原链"
        );
        assert!(execute_book_file(dir.path(), "nope", &json!({})).await.is_none());
        assert!(execute_project_file(dir.path(), "nope", &json!({})).await.is_none());
    }
}
