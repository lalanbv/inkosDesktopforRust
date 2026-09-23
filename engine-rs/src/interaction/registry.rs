//! 工具注册表（R38a 批一 547 号 / R38b 批二全族收拢）——dsh
//! `packages/core/tools` 工具面的 InkOS 收拢形态（学模式不搬框架，v7 路线 R38）。
//!
//! 批二范围：全族 33 件单点注册——schema、执行器、变更面定性、可用性门控
//! 四元同址。此前 film/play/propose/research/import/sub_agent/use_skill/
//! book_reference/book_edit/forecast/material 各族 schema 与执行分居两处，
//! 可用性靠 ChatToolRouter 里 `if let Some(deps)` 在场判断 + 分发顺序隐式
//! 消解，生产变更面靠 PRODUCTION_MUTATION_TOOL_NAMES 字符串集——dsh 三重
//! 经验对应：
//! - ToolDefinition 的 name/description/parameters/execute 元组 → [`ToolDef`]；
//! - scope layers「最近层胜出」→ [`ToolScope`] 分层遮蔽（read/ls/grep 双
//!   作用域族，声明序 = 遮蔽序）；
//! - restrictions 变更面定性 → [`MutationKind`]（suppressProductionTools 面
//!   消费 [`MutationKind::ProductionMutation`]，替代字符串名单）；
//! - 可用性门控 → [`ToolDef::available`]（deps 在场显式化）。
//!
//! 装配为 [`ToolRegistry::global`] OnceLock 静态单例（ZST 单元，构建一次，
//! 无 Box::leak / 无静态堆引用），查找按 &str 线性扫（全表 33 项，零分配；
//! 声明序 = 原 ChatToolRouter 分发链序 = schema 投影序）。

use std::path::Path;
use std::sync::OnceLock;

use serde_json::{json, Value};

use super::{
    book_edit_tools, book_reference_tool, film_authoring_tools, forecast_tools,
    import_chapters_tool, material_tools, play_tools, project_tools, propose_action_tool,
    research_tool, skill_tool, sub_agent_tool,
};
use super::project_tools::ToolResult;
use crate::server::books_routes::BooksRuntime;
use crate::skills::SkillRegistry;

/// 变更面定性（dsh restrictions 收拢；suppressProductionTools 消费面）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MutationKind {
    /// 只读：无落盘、无项目外副作用。
    ReadOnly,
    /// 项目内落盘写入（素材入库、play 世界推进等），不在后台生产剔除名单。
    ProjectWrite,
    /// 生产变更面（章节/真相文件写入、子代理生产链）——后台生产任务运行时
    /// 从工具表硬剔除并在分发面拒绝（原 PRODUCTION_MUTATION_TOOL_NAMES 九件）。
    ProductionMutation,
}

/// 作用域层：同名工具最近层胜出（dsh scope layers 语义）——书会话层遮蔽
/// 项目层的 read/ls/grep，其余各族工具在书会话继续可达。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ToolScope {
    /// 项目根作用域（文件三件项目层，回环尾面全会话可达）。
    Project,
    /// 书会话作用域（文件三件书层，book/edit 会话，books/ 相对解析）。
    BookSession,
    /// 会话装配作用域（文件六件以外全部族——可用性由 available 门控，
    /// 不参与文件投影分层）。
    Session,
}

/// import_chapters 依赖（85 号）：runtime + 活动书 + 聊天轮中止句柄（102 号）。
/// （原 agent_route.rs 私有 ImportDeps，R38b 随 ToolCtx 收拢迁入注册表。）
pub struct ImportToolDeps<'a> {
    pub runtime: &'a BooksRuntime,
    pub active_book_id: Option<&'a str>,
    pub abort: Option<super::agent_loop::AbortHandle>,
}

/// 工具执行上下文（R38b 全族收拢）：原 ChatToolRouter 字段组同形搬入——
/// 各族 deps 在场即该族工具可用（dsh `available` 门控的装配对应物）。
/// 借用不克隆；装配每请求一次（agent_route），分发零堆分配。
pub struct ToolCtx<'a> {
    pub root: &'a Path,
    /// 256 号：interactive-film-authoring 七件作者工具（该会话独占面）。
    pub film_authoring_deps: Option<&'a film_authoring_tools::FilmAuthoringDeps<'a>>,
    pub play_deps: Option<&'a play_tools::PlayToolDeps<'a>>,
    pub propose_deps: Option<&'a propose_action_tool::ProposeDeps<'a>>,
    pub research_enabled: bool,
    pub import_deps: Option<&'a ImportToolDeps<'a>>,
    pub sub_agent_deps: Option<&'a sub_agent_tool::SubAgentDeps<'a>>,
    pub book_edit_deps: Option<&'a book_edit_tools::BookEditDeps<'a>>,
    pub forecast_deps: Option<&'a forecast_tools::ForecastDeps<'a>>,
    /// 216 号：manage_book_reference 的活动书（book/edit 会话恒有）。
    pub reference_book_id: Option<&'a str>,
    /// 240 号：use_skill 的技能注册表 + 禁用集。
    pub skill_deps: Option<(&'a dyn SkillRegistry, &'a [String])>,
    /// 215 号：suppressProductionTools——true 时生产变更面工具在分发面拒绝。
    pub suppress_production: bool,
}

impl<'a> ToolCtx<'a> {
    /// 仅项目根（execute_tool 兜底链/内部调用形态：文件三件 + material 双件）。
    pub fn root_only(root: &'a Path) -> Self {
        Self {
            root,
            film_authoring_deps: None,
            play_deps: None,
            propose_deps: None,
            research_enabled: false,
            import_deps: None,
            sub_agent_deps: None,
            book_edit_deps: None,
            forecast_deps: None,
            reference_book_id: None,
            skill_deps: None,
            suppress_production: false,
        }
    }
}

/// 工具定义：schema 三元组 + 变更面定性 + 可用性门控 + 执行器（dsh
/// ToolDefinition 同址形态）。实现体为 ZST，由 [`tool_def`] 宏生成，各族
/// 模块就地注册（schema/执行同文件单一事实源），注册表启动期汇聚。
///
/// schema 文本两种形态并存（R38b 备案）：文件六件 + material 双件 +
/// film 七件的 json 直接写在实现体（R38a 先例延续）；其余各族经
/// [`schema_description`]/[`schema_parameters`] 从同文件 schema 函数
/// 拆解引用——码点零搬移。两条路都保证每个工具的 schema 只定义一次。
#[async_trait::async_trait]
pub trait ToolDef: Send + Sync {
    /// 工具名（查找键；文件三件双作用域族允许同名，分发由声明序遮蔽消解）。
    fn name(&self) -> &'static str;
    /// 描述（schema 面；书 read 随 INKOS_AGENT_ALLOW_SYSTEM_READ 分流）。
    fn description(&self) -> String;
    /// 参数 schema（json! 每次构建——与原 schema 函数行为一致；
    /// 装配面每请求一次，非热路径）。
    fn parameters(&self) -> Value;
    fn mutation_kind(&self) -> MutationKind;
    /// 作用域层（schema 分层投影面；仅文件三件双作用域族分层，
    /// 其余族默认会话装配层）。
    fn scope(&self) -> ToolScope {
        ToolScope::Session
    }
    /// 可用性门控（原 ChatToolRouter `if let Some(deps)` 在场判断显式化）。
    fn available(&self, _ctx: &ToolCtx<'_>) -> bool {
        true
    }
    async fn execute(&self, ctx: &ToolCtx<'_>, args: &Value) -> ToolResult;
}

/// 工具定义声明宏：ZST + [`ToolDef`] 实现体一步生成（schema/执行/定性/门控
/// 同址）。各族模块在执行体旁就地调用，`defs()` 汇聚进注册表。
///
/// - `$ctx`/`$args`：绑定名显式传递（宏卫生——调用处表达式内的 `ctx`/`args`
///   解析到这两个 ident）；
/// - `$desc`：impl Into\<String\> 表达式（'static 字面量或 schema 拆解 String）；
/// - `$params`：json! 表达式（schema json 单一事实源）；
/// - `$avail`：bool 表达式；
/// - `$exec`：async 块体，求值为 [`ToolResult`]；
/// - 10 参变体多一 `$scope`（[`ToolScope`]），供文件三件双作用域族分层。
macro_rules! tool_def {
    ($zst:ident, $name:literal, $kind:expr, $ctx:ident, $args:ident, $scope:expr, $desc:expr, $params:expr, $avail:expr, $exec:expr) => {
        crate::interaction::registry::tool_def!(@impl $zst, $name, $kind, $ctx, $args, $desc, $params, $avail, $exec, fn scope(&self) -> $crate::interaction::registry::ToolScope { $scope });
    };
    ($zst:ident, $name:literal, $kind:expr, $ctx:ident, $args:ident, $desc:expr, $params:expr, $avail:expr, $exec:expr) => {
        crate::interaction::registry::tool_def!(@impl $zst, $name, $kind, $ctx, $args, $desc, $params, $avail, $exec,);
    };
    (@impl $zst:ident, $name:literal, $kind:expr, $ctx:ident, $args:ident, $desc:expr, $params:expr, $avail:expr, $exec:expr, $($scope_fn:tt)*) => {
        struct $zst;
        #[async_trait::async_trait]
        impl $crate::interaction::registry::ToolDef for $zst {
            fn name(&self) -> &'static str {
                $name
            }
            fn description(&self) -> String {
                ($desc).into()
            }
            fn parameters(&self) -> ::serde_json::Value {
                $params
            }
            fn mutation_kind(&self) -> $crate::interaction::registry::MutationKind {
                $kind
            }
            $($scope_fn)*
            fn available(&self, $ctx: &$crate::interaction::registry::ToolCtx<'_>) -> bool {
                let _ = &$ctx;
                $avail
            }
            async fn execute(&self, $ctx: &$crate::interaction::registry::ToolCtx<'_>, $args: &::serde_json::Value) -> $crate::interaction::project_tools::ToolResult {
                $exec
            }
        }
    };
}
pub(crate) use tool_def;

/// 从同文件 schema 函数输出拆解描述（R38b 其余各族通道——码点零搬移；
/// 未命中返回空串，由族完整性测试兜底）。
pub(crate) fn schema_description(schemas: &[Value], name: &str) -> String {
    schemas
        .iter()
        .find(|s| s["function"]["name"] == *name)
        .and_then(|s| s["function"]["description"].as_str())
        .unwrap_or_default()
        .to_string()
}

/// 从同文件 schema 函数输出拆解参数 schema（未命中回退空对象，测试兜底）。
pub(crate) fn schema_parameters(schemas: &[Value], name: &str) -> Value {
    schemas
        .iter()
        .find(|s| s["function"]["name"] == *name)
        .map(|s| s["function"]["parameters"].clone())
        .unwrap_or_else(|| json!({ "type": "object", "properties": {} }))
}

/// 工具定义声明宏的 trait 面投影：单条 OpenAI function schema。
pub fn openai_schema(def: &dyn ToolDef) -> Value {
    json!({
        "type": "function",
        "function": {
            "name": def.name(),
            "description": def.description(),
            "parameters": def.parameters(),
        },
    })
}

// ── 项目作用域族（read/ls/grep，TS project-tools 移植面） ──────────────────

tool_def!(
    ProjectRead,
    "read",
    MutationKind::ReadOnly,
    ctx, args,
    ToolScope::Project,
    "读取项目内文本文件内容",
    json!({
        "type": "object",
        "properties": { "path": { "type": "string", "description": "项目相对路径" } },
        "required": ["path"],
    }),
    true,
    project_tools::tool_read(ctx.root, args).await
);

tool_def!(
    ProjectLs,
    "ls",
    MutationKind::ReadOnly,
    ctx, args,
    ToolScope::Project,
    "列出项目目录内容",
    json!({
        "type": "object",
        "properties": { "path": { "type": "string", "description": "项目相对路径（默认 .）" } },
    }),
    true,
    project_tools::tool_ls(ctx.root, args).await
);

tool_def!(
    ProjectGrep,
    "grep",
    MutationKind::ReadOnly,
    ctx, args,
    ToolScope::Project,
    "在项目文本文件中搜索",
    json!({
        "type": "object",
        "properties": {
            "query": { "type": "string" },
            "path": { "type": "string", "description": "搜索根（默认 .）" },
        },
        "required": ["query"],
    }),
    true,
    project_tools::tool_grep(ctx.root, args).await
);

// ── 书会话作用域族（105 号，TS createReadTool/createLsTool/createGrepTool 逐字） ──

tool_def!(
    BookRead,
    "read",
    MutationKind::ReadOnly,
    ctx, args,
    ToolScope::BookSession,
    {
        if project_tools::allow_system_read() {
            "Read a file. Relative paths resolve under books/; absolute paths read from the system filesystem."
        } else {
            "Read a file from the book directory. Path is relative to books/."
        }
    },
    json!({
        "type": "object",
        "properties": {
            "path": { "type": "string", "description": "File path relative to books/, or an absolute path when system path reading is enabled." },
        },
        "required": ["path"],
    }),
    ctx.book_edit_deps.is_some(),
    project_tools::tool_read_book(ctx.root, args).await
);

tool_def!(
    BookLs,
    "ls",
    MutationKind::ReadOnly,
    ctx, args,
    ToolScope::BookSession,
    "List files in a book directory. Optionally specify a subdirectory like 'story' or 'chapters'.",
    json!({
        "type": "object",
        "properties": {
            "bookId": { "type": "string", "description": "Book ID" },
            "subdir": { "type": "string", "description": "Subdirectory within the book, e.g. 'story', 'chapters', 'story/runtime'" },
        },
        "required": ["bookId"],
    }),
    ctx.book_edit_deps.is_some(),
    project_tools::tool_ls_book(ctx.root, args).await
);

tool_def!(
    BookGrep,
    "grep",
    MutationKind::ReadOnly,
    ctx, args,
    ToolScope::BookSession,
    "Search for a text pattern across a book's story/ and chapters/ directories. Returns matching lines.",
    json!({
        "type": "object",
        "properties": {
            "bookId": { "type": "string", "description": "Book ID to search within" },
            "pattern": { "type": "string", "description": "Search pattern (plain text or regex)" },
        },
        "required": ["bookId", "pattern"],
    }),
    ctx.book_edit_deps.is_some(),
    project_tools::tool_grep_book(ctx.root, args).await
);

// ── 注册表与分发面 ─────────────────────────────────────────────────────────

/// 单条 schema 投影（名称/描述/参数——schema 面消费的中间形态）。
pub struct ToolEntry {
    pub name: &'static str,
    pub description: String,
    pub parameters: Value,
}

/// 全族工具注册表（静态装配，查找零分配）。`defs` 声明序 = 原 ChatToolRouter
/// 分发链序（R38a 前形态）：同名 read/ls/grep 书层先查（遮蔽项目层），
/// 其余各族名称集互不相交，链序仅决定投影序。
pub struct ToolRegistry {
    defs: Vec<Box<dyn ToolDef>>,
}

impl ToolRegistry {
    /// 全局注册表（OnceLock 装配一次；ZST 单元无堆泄漏面）。
    pub fn global() -> &'static Self {
        static REGISTRY: OnceLock<ToolRegistry> = OnceLock::new();
        REGISTRY.get_or_init(|| Self {
            defs: {
                let mut defs: Vec<Box<dyn ToolDef>> = Vec::new();
                defs.extend(film_authoring_tools::defs());
                defs.extend(propose_action_tool::defs());
                defs.extend(research_tool::defs());
                defs.extend(import_chapters_tool::defs());
                defs.extend(sub_agent_tool::defs());
                defs.extend(skill_tool::defs());
                defs.extend(book_reference_tool::defs());
                defs.extend(book_edit_tools::defs());
                defs.extend(forecast_tools::defs());
                defs.extend(play_tools::defs());
                let file_defs: Vec<Box<dyn ToolDef>> = vec![
                    Box::new(BookRead),
                    Box::new(BookLs),
                    Box::new(BookGrep),
                    Box::new(ProjectRead),
                    Box::new(ProjectLs),
                    Box::new(ProjectGrep),
                ];
                defs.extend(file_defs);
                defs.extend(material_tools::defs());
                defs
            },
        })
    }

    /// 全表遍历（声明序；debug/tools 投影与测试面消费）。
    pub fn all(&self) -> impl Iterator<Item = &dyn ToolDef> {
        self.defs.iter().map(|def| def.as_ref())
    }

    /// 全表查找：声明序第一个「名称匹配且可用」的定义
    ///（原分发链序 + `if let Some(deps)` 在场门控的显式化）。
    pub fn find(&self, name: &str, ctx: &ToolCtx<'_>) -> Option<&dyn ToolDef> {
        self.all().find(|def| def.name() == name && def.available(ctx))
    }

    /// 按作用域层 schema 投影（文件三件面消费；声明序稳定）。
    pub fn entries(&self, scope: ToolScope) -> Vec<ToolEntry> {
        self.all()
            .filter(|def| def.scope() == scope)
            .map(|def| ToolEntry { name: def.name(), description: def.description(), parameters: def.parameters() })
            .collect()
    }

    /// 按作用域层 OpenAI function schema 投影（book_file_tool_schemas 消费）。
    pub fn schemas(&self, scope: ToolScope) -> Vec<Value> {
        self.all()
            .filter(|def| def.scope() == scope)
            .map(openai_schema)
            .collect()
    }

    /// 按名称列表 schema 投影（各族 schema 函数薄壳；声明序内过滤保序）。
    pub fn schemas_for(&self, names: &[&str]) -> Vec<Value> {
        self.all()
            .filter(|def| names.contains(&def.name()))
            .map(openai_schema)
            .collect()
    }

    /// 按名称查变更面定性（suppressProductionTools 剔除面消费；
    /// 可用性无关——注册面定性）。
    pub fn mutation_kind(&self, name: &str) -> Option<MutationKind> {
        self.all().find(|def| def.name() == name).map(|def| def.mutation_kind())
    }
}

/// 全路由分发（R38b 单点化）：注册表查找 → 生产变更面抑制判定 → 执行；
/// 未注册/不可用 → `Unknown tool` 错误文本（原兜底链语义）。
pub async fn execute_routed(ctx: &ToolCtx<'_>, name: &str, args: &Value) -> ToolResult {
    let registry = ToolRegistry::global();
    if let Some(def) = registry.find(name, ctx) {
        if ctx.suppress_production && def.mutation_kind() == MutationKind::ProductionMutation {
            return project_tools::error_result(format!("Unknown tool: {name}"));
        }
        return def.execute(ctx, args).await;
    }
    project_tools::error_result(format!("Unknown tool: {name}"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    /// 测试轻量 runtime（books_routes 测试 runtime_for 同法——registry 测试
    /// 只需要引用在场，router 不出网）。
    fn runtime_for(root: &Path) -> BooksRuntime {
        use crate::llm::agent_router::{AgentRouter, LlmEndpointConfig};
        use crate::server::sse::BroadcastHub;
        use crate::state::manager::StateManager;
        BooksRuntime {
            hub: std::sync::Arc::new(BroadcastHub::new()),
            state: std::sync::Arc::new(StateManager::new(root)),
            router: std::sync::Arc::new(AgentRouter::new(
                LlmEndpointConfig {
                    context_window_tokens: 128_000,
                    base_url: "http://127.0.0.1:9".into(),
                    api_key: "k".into(),
                    model: "m".into(),
                    max_tokens: 1024,
                    extra_headers: Default::default(),
                },
                Default::default(),
            )),
            builtin_genres_dir: root.to_path_buf(),
            revision_gate: crate::pipeline::merged_audit::RevisionGate::default(),
        }
    }

    #[test]
    fn registry_total_count_and_name_uniqueness() {
        let registry = ToolRegistry::global();
        let names: Vec<&str> = registry.all().map(|def| def.name()).collect();
        // 34 = film 7 + propose 1 + research 1 + import 1 + sub_agent 1
        //      + use_skill 1 + book_reference 1 + book_edit 7 + forecast 3
        //      + play 3 + 文件三件双作用域 6 + material 2。
        // 新族注册须同步此计数（施工图 §4.1 穷举测试思想：防漏登记）。
        assert_eq!(names.len(), 34, "注册表总件数漂移：{names:?}");
        let mut sorted = names;
        sorted.sort_unstable();
        let total = sorted.len();
        sorted.dedup();
        assert_eq!(total - sorted.len(), 3, "唯一同名 = read/ls/grep 双作用域族");
    }

    #[test]
    fn shadowing_order_book_layer_precedes_project_layer() {
        // 遮蔽语义的序保证：同名三件，书会话层声明在前（最近层胜出）。
        let names: Vec<&str> = ToolRegistry::global().all().map(|def| def.name()).collect();
        let book_read = names.iter().position(|n| *n == "read").unwrap();
        let project_read = names[book_read + 1..].iter().position(|n| *n == "read").unwrap() + book_read + 1;
        assert!(book_read < project_read);
    }

    #[test]
    fn project_layer_projection_order_and_shape() {
        let schemas: Vec<Value> = ToolRegistry::global()
            .all()
            .filter(|def| def.scope() == ToolScope::Project)
            .map(openai_schema)
            .collect();
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
        let schemas: Vec<Value> = ToolRegistry::global()
            .all()
            .filter(|def| def.scope() == ToolScope::BookSession)
            .map(openai_schema)
            .collect();
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
    fn production_mutation_set_matches_ts_suppression_list() {
        // 原 PRODUCTION_MUTATION_TOOL_NAMES 九件精确集（agent-session.ts
        // 可注册子集）；定性漂移直接在此红——suppressProductionTools 消费面。
        let registry = ToolRegistry::global();
        let mut production: Vec<&str> = registry
            .all()
            .filter(|def| def.mutation_kind() == MutationKind::ProductionMutation)
            .map(|def| def.name())
            .collect();
        production.sort_unstable();
        assert_eq!(
            production,
            vec![
                "delete_latest_chapter",
                "generate_cover",
                "import_chapters",
                "patch_chapter_text",
                "rename_entity",
                "replace_chapter_text",
                "resync_chapter_state",
                "sub_agent",
                "write_truth_file",
            ]
        );
    }

    #[test]
    fn read_only_families_stay_out_of_production_list() {
        // 文件六件 + material retrieve + research/use_skill 等定性为非生产
        // 变更面（TS 一致保留面——215 号剔除名单不含）。
        let registry = ToolRegistry::global();
        for name in ["read", "ls", "grep", "retrieve_material", "research_web", "use_skill", "propose_action"] {
            assert_ne!(
                registry.mutation_kind(name),
                Some(MutationKind::ProductionMutation),
                "{name} 不在生产剔除名单"
            );
        }
    }

    #[test]
    fn book_layer_gated_on_book_edit_deps_presence() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        let ctx = ToolCtx::root_only(root);
        // root_only：书三件不可用，read 落项目层。
        let def = ToolRegistry::global().find("read", &ctx).unwrap();
        assert_eq!(def.scope(), ToolScope::Project, "无书会话依赖时 read 落项目层");
        assert!(ToolRegistry::global().find("write_truth_file", &ctx).is_none(), "编辑族不可用");
    }

    #[tokio::test]
    async fn shadowing_book_layer_wins_in_book_sessions() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        std::fs::write(root.join("note.md"), "project-scope").unwrap();
        std::fs::create_dir_all(root.join("books").join("b1")).unwrap();
        std::fs::write(root.join("books").join("b1").join("book.json"), "{\"id\":\"b1\"}").unwrap();

        let runtime = runtime_for(root);
        let edit_deps = book_edit_tools::BookEditDeps {
            runtime: &runtime,
            active_book_id: "b1",
            language: "zh",
        };
        let mut ctx = ToolCtx::root_only(root);
        ctx.book_edit_deps = Some(&edit_deps);

        // 书会话：read 命中书层（books/ 相对解析）。
        let book = execute_routed(&ctx, "read", &json!({ "path": "b1/book.json" })).await;
        assert!(book.text.contains("b1"), "书层遮蔽生效：{}", book.text);
        // 书层失败为非错误文本（TS textResult 逐字），项目根文件按书层语义不可达。
        let miss = execute_routed(&ctx, "read", &json!({ "path": "note.md" })).await;
        assert!(!miss.is_error, "书 read 失败为非错误文本（TS textResult 形态）");
        // 非书会话：项目层。
        let project_ctx = ToolCtx::root_only(root);
        let project = execute_routed(&project_ctx, "read", &json!({ "path": "note.md" })).await;
        assert!(project.text.contains("project-scope"));
        assert!(!project.is_error);
    }

    #[tokio::test]
    async fn unknown_tool_and_suppressed_production_tool_error_verbatim() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        let runtime = runtime_for(root);
        let edit_deps = book_edit_tools::BookEditDeps {
            runtime: &runtime,
            active_book_id: "b1",
            language: "zh",
        };
        let mut ctx = ToolCtx::root_only(root);
        ctx.book_edit_deps = Some(&edit_deps);

        // 未注册工具 → Unknown（原兜底链逐字）。
        let unknown = execute_routed(&ctx, "nope", &json!({})).await;
        assert!(unknown.is_error && unknown.text == "Unknown tool: nope");

        // suppress_production：生产变更面在分发面拒绝（原链首行语义逐字）。
        ctx.suppress_production = true;
        let suppressed = execute_routed(&ctx, "write_truth_file", &json!({})).await;
        assert!(suppressed.is_error && suppressed.text == "Unknown tool: write_truth_file");
        // 只读工具不受抑制。
        let read = execute_routed(&ctx, "read", &json!({ "path": "b1/book.json" })).await;
        assert!(!read.is_error);
    }

    #[tokio::test]
    async fn material_pair_routes_through_registry() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        std::fs::write(root.join("note.md"), "账页记载着冷库赔偿款。").unwrap();
        let ctx = ToolCtx::root_only(root);
        let ingested = execute_routed(&ctx, "ingest_material", &json!({ "sourceKind": "file", "filePath": "note.md", "title": "账页资料" })).await;
        assert!(!ingested.is_error, "{}", ingested.text);
        assert!(ingested.text.starts_with("Material ingested: .inkos/materials/"));
        let retrieved = execute_routed(&ctx, "retrieve_material", &json!({ "query": "冷库赔偿款" })).await;
        assert!(!retrieved.is_error && retrieved.text.contains("Retrieved 1 material snippet."));
    }

    #[test]
    fn schemas_for_preserves_declaration_order() {
        // 族 schema 薄壳（R38b）：按名称过滤声明序保序——
        // film_authoring_tool_schemas 消费面的投影序契约。
        let registry = ToolRegistry::global();
        let schemas = registry.schemas_for(&["revise_node", "set_world_anchor"]);
        let names: Vec<&str> = schemas
            .iter()
            .map(|s| s["function"]["name"].as_str().unwrap())
            .collect();
        assert_eq!(names, vec!["set_world_anchor", "revise_node"], "声明序胜过查询序");
    }
}
