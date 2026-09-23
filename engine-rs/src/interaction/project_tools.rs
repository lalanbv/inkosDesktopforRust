//! 交互 agent 工具集（read / ls / grep）。
//!
//! 移植自 `packages/core/src/interaction/project-tools.ts` 的文件工具子集。
//! 生产工具（write/edit/propose_action/sub_agent/play_*/short_fiction_run 等）
//! 随确认式生产任务分支（67 号）移植。

use std::path::{Path, PathBuf};

use serde_json::{json, Value};

/// 工具执行结果：文本 + details（透传给 toolResult 事件）。
pub struct ToolResult {
    pub text: String,
    pub details: Option<Value>,
    /// 显式错误标记（80 号：play 工具的透传错误靠它进入 error 执行卡；
    /// 文件工具沿用文本启发式兼容）。
    pub is_error: bool,
}

/// 项目根内路径解析（拒绝逃逸）。
fn safe_join(root: &Path, raw: &str) -> Option<PathBuf> {
    let trimmed = raw.trim();
    if trimmed.is_empty() || trimmed.contains('\0') {
        return None;
    }
    let candidate = root.join(trimmed.trim_start_matches('/'));
    let normalized_root = root.canonicalize().unwrap_or_else(|_| root.to_path_buf());
    let Ok(resolved) = candidate.canonicalize() else {
        // 不存在：做词法检查（跟随父目录）
        let mut lexical = normalized_root.clone();
        for part in trimmed.trim_start_matches('/').split('/') {
            match part {
                "" | "." => {}
                ".." => {
                    lexical.pop();
                }
                other => lexical.push(other),
                }
        }
        return lexical.starts_with(&normalized_root).then_some(lexical);
    };
    resolved.starts_with(&normalized_root).then_some(resolved)
}

/// `read` 工具：读项目内文本文件（1MB 上限）。
pub async fn tool_read(root: &Path, args: &Value) -> ToolResult {
    let Some(path) = args.get("path").and_then(Value::as_str) else {
        return error_result("read requires a path argument");
    };
    let Some(resolved) = safe_join(root, path) else {
        return error_result("path escapes project root");
    };
    match tokio::fs::metadata(&resolved).await {
        Ok(meta) if meta.len() > 1024 * 1024 => {
            return error_result("file exceeds 1MB limit")
        }
        _ => {}
    }
    match tokio::fs::read_to_string(&resolved).await {
        Ok(content) => ToolResult { text: content, details: None, is_error: false },
        Err(e) => error_result(format!("read failed: {e}")),
    }
}

/// `ls` 工具：列目录（名字 + 类型标记）。
pub async fn tool_ls(root: &Path, args: &Value) -> ToolResult {
    let path = args.get("path").and_then(Value::as_str).unwrap_or(".");
    let Some(resolved) = safe_join(root, path) else {
        return error_result("path escapes project root");
    };
    let Ok(mut entries) = tokio::fs::read_dir(&resolved).await else {
        return error_result(format!("ls failed: not a directory: {path}"));
    };
    let mut names: Vec<String> = Vec::new();
    while let Ok(Some(entry)) = entries.next_entry().await {
        let marker = if entry.file_type().await.map(|t| t.is_dir()).unwrap_or(false) {
            "/"
        } else {
            ""
        };
        names.push(format!("{}{}", entry.file_name().to_string_lossy(), marker));
    }
    names.sort();
    ToolResult {
        text: names.join("\n"),
        details: Some(json!({ "count": names.len() })),
        is_error: false,
    }
}

/// `grep` 工具：项目内文本搜索（当前目录浅层递归，返回命中行，上限 200 行）。
pub async fn tool_grep(root: &Path, args: &Value) -> ToolResult {
    let Some(query) = args.get("query").and_then(Value::as_str) else {
        return error_result("grep requires a query argument");
    };
    if query.is_empty() {
        return error_result("grep query must not be empty");
    }
    let path = args.get("path").and_then(Value::as_str).unwrap_or(".");
    let Some(base) = safe_join(root, path) else {
        return error_result("path escapes project root");
    };
    let mut files: Vec<PathBuf> = Vec::new();
    grep_walk(&base, &mut files, 0);
    let mut hits: Vec<String> = Vec::new();
    for file in &files {
        let Ok(content) = tokio::fs::read_to_string(file).await else {
            continue;
        };
        let rel = file.strip_prefix(root).unwrap_or(file);
        for (line_no, line) in content.lines().enumerate() {
            if line.contains(query) {
                hits.push(format!("{}:{}:{}", rel.display(), line_no + 1, line.trim()));
                if hits.len() >= 200 {
                    break;
                }
            }
        }
        if hits.len() >= 200 {
            break;
        }
    }
    ToolResult {
        text: hits.join("\n"),
        details: Some(json!({ "matches": hits.len() })),
        is_error: false,
    }
}

pub(crate) fn error_result(message: impl Into<String>) -> ToolResult {
    ToolResult { text: message.into(), details: None, is_error: true }
}

/// 先收集文本文件路径（同步 walk，深度 3），再逐文件搜行。
fn grep_walk(dir: &Path, out: &mut Vec<PathBuf>, depth: u32) {
    if depth > 3 || out.len() >= 500 {
        return;
    }
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let name = entry.file_name().to_string_lossy().to_string();
        if name.starts_with('.') || name == "node_modules" || name == "target" {
            continue;
        }
        let path = entry.path();
        if path.is_dir() {
            grep_walk(&path, out, depth + 1);
            continue;
        }
        let ext_ok = path
            .extension()
            .and_then(|e| e.to_str())
            .map(|e| ["md", "txt", "json", "ts", "js", "rs", "yaml", "yml"].contains(&e))
            .unwrap_or(false);
        if ext_ok {
            out.push(path);
        }
    }
}

/// 工具注册表项：OpenAI function schema + 执行器（R38a 起本体在
/// registry.rs 项目作用域层，此结构保留给 tools_payload 消费面）。
pub struct InteractionTool {
    pub name: &'static str,
    pub description: String,
    pub parameters: Value,
}

/// 本轮工具集（read/ls/grep）——schema 由注册表项目作用域层投影
/// （单一事实源；新增文件工具 = 注册一个 ToolDef）。
pub fn interaction_tools() -> Vec<InteractionTool> {
    crate::interaction::registry::ToolRegistry::global()
        .entries(crate::interaction::registry::ToolScope::Project)
        .into_iter()
        .map(|entry| InteractionTool {
            name: entry.name,
            description: entry.description,
            parameters: entry.parameters,
        })
        .collect()
}

/// tools 数组（OpenAI 形态）。
pub fn tools_payload() -> Value {
    json!(interaction_tools()
        .iter()
        .map(|tool| {
            json!({
                "type": "function",
                "function": {
                    "name": tool.name,
                    "description": tool.description,
                    "parameters": tool.parameters,
                },
            })
        })
        .collect::<Vec<_>>())
}

// ── 105 号：书会话文件三件（books/ 作用域，TS createReadTool/createLsTool/
// createGrepTool 逐字——替代项目作用域三件的书会话注册） ──────────────────

/// `envFlagEnabled`（agent-session.ts 逐字）：未设 → 默认；"1"/"true" → 真；
/// "0"/"false" → 假；其它 → 默认。
fn env_flag_enabled(value: Option<String>, default: bool) -> bool {
    match value {
        None => default,
        Some(value) if value == "1" || value.eq_ignore_ascii_case("true") => true,
        Some(value) if value == "0" || value.eq_ignore_ascii_case("false") => false,
        _ => default,
    }
}

/// `INKOS_AGENT_ALLOW_SYSTEM_READ`（缺省 false）——read 工具绝对路径分支
/// （注册表 BookRead 描述分流同源）。
pub(crate) fn allow_system_read() -> bool {
    env_flag_enabled(std::env::var("INKOS_AGENT_ALLOW_SYSTEM_READ").ok(), false)
}

/// `read`（书会话）：path 相对 books/；系统读开启时绝对路径直读。
/// 失败为非错误文本结果（TS textResult 形态逐字）。
pub async fn tool_read_book(root: &Path, args: &Value) -> ToolResult {
    let Some(path) = args.get("path").and_then(Value::as_str) else {
        return error_result("read requires a path argument");
    };
    let books_root = root.join("books");
    let resolved = if allow_system_read() && Path::new(path).is_absolute() {
        PathBuf::from(path)
    } else {
        match crate::utils::path::safe_child_path(&books_root.to_string_lossy(), path) {
            Ok(resolved) => resolved,
            Err(message) => {
                return ToolResult {
                    text: format!("Failed to read \"{path}\": {message}"),
                    details: None,
                    is_error: false,
                }
            }
        }
    };
    match tokio::fs::read_to_string(&resolved).await {
        Ok(content) => ToolResult { text: content, details: None, is_error: false },
        Err(e) => ToolResult {
            text: format!("Failed to read \"{path}\": {e}"),
            details: None,
            is_error: false,
        },
    }
}

/// `ls`（书会话）：bookId + 可选 subdir；目录项 `/` 后缀或 ` (N bytes)`。
pub async fn tool_ls_book(root: &Path, args: &Value) -> ToolResult {
    let failed = |book_id: &str, subdir: Option<&str>, message: String| ToolResult {
        text: format!("Failed to list \"{book_id}/{}\": {message}", subdir.unwrap_or("")),
        details: None,
        is_error: false,
    };
    let Some(book_id) = args.get("bookId").and_then(Value::as_str) else {
        return error_result("ls requires a bookId argument");
    };
    let subdir = args.get("subdir").and_then(Value::as_str);
    let books_root = root.join("books");
    let Ok(mut base) = crate::utils::path::safe_child_path(&books_root.to_string_lossy(), book_id)
    else {
        return failed(book_id, subdir, "Path traversal blocked".to_string());
    };
    if let Some(subdir) = subdir {
        match crate::utils::path::safe_child_path(&base.to_string_lossy(), subdir) {
            Ok(resolved) => base = resolved,
            Err(message) => return failed(book_id, Some(subdir), message),
        }
    }
    let Ok(mut entries) = tokio::fs::read_dir(&base).await else {
        return failed(
            book_id,
            subdir,
            "not a directory".to_string(),
        );
    };
    let mut details: Vec<String> = Vec::new();
    while let Ok(Some(entry)) = entries.next_entry().await {
        let name = entry.file_name().to_string_lossy().into_owned();
        match tokio::fs::metadata(entry.path()).await {
            Ok(meta) if meta.is_dir() => details.push(format!("{name}/")),
            Ok(meta) => details.push(format!("{name} ({} bytes)", meta.len())),
            Err(_) => details.push(name),
        }
    }
    if details.is_empty() {
        return ToolResult {
            text: format!("Directory is empty: {book_id}/{}", subdir.unwrap_or("")),
            details: None,
            is_error: false,
        };
    }
    ToolResult { text: details.join("\n"), details: None, is_error: false }
}

/// `grep`（书会话）：bookId + pattern（不区分大小写正则）；只搜 story/ 与
/// chapters/（md/txt/json），输出 `{前缀}{文件}:{行号}: {行}`；>100 截断。
pub async fn tool_grep_book(root: &Path, args: &Value) -> ToolResult {
    let failed = |message: String| ToolResult {
        text: format!("Grep failed: {message}"),
        details: None,
        is_error: false,
    };
    let (Some(book_id), Some(pattern)) = (
        args.get("bookId").and_then(Value::as_str),
        args.get("pattern").and_then(Value::as_str),
    ) else {
        return error_result("grep requires bookId and pattern arguments");
    };
    let books_root = root.join("books");
    let Ok(book_dir) = crate::utils::path::safe_child_path(&books_root.to_string_lossy(), book_id)
    else {
        return failed("Path traversal blocked".to_string());
    };
    let Ok(regex) = regex::RegexBuilder::new(pattern).case_insensitive(true).build() else {
        let message = format!("invalid pattern: {pattern}");
        return failed(message);
    };
    let mut results: Vec<String> = Vec::new();
    for (dir, prefix) in [(book_dir.join("story"), "story/"), (book_dir.join("chapters"), "chapters/")] {
        // 迭代 worklist（LIFO）复刻 readdir 深度优先；目录不存在静默跳过。
        let mut stack = vec![(dir, prefix.to_string())];
        while let Some((dir, prefix)) = stack.pop() {
            let Ok(mut entries) = tokio::fs::read_dir(&dir).await else {
                continue;
            };
            let mut nested = Vec::new();
            while let Ok(Some(entry)) = entries.next_entry().await {
                let name = entry.file_name().to_string_lossy().into_owned();
                let full = entry.path();
                let Ok(meta) = tokio::fs::metadata(&full).await else {
                    // TS stat 失败整环抛 → 工具失败文本。
                    return failed(format!("stat failed for {prefix}{name}"));
                };
                if meta.is_dir() {
                    nested.push((full, format!("{prefix}{name}/")));
                } else if name.ends_with(".md") || name.ends_with(".txt") || name.ends_with(".json") {
                    let Ok(content) = tokio::fs::read_to_string(&full).await else {
                        return failed(format!("read failed for {prefix}{name}"));
                    };
                    for (index, line) in content.lines().enumerate() {
                        if regex.is_match(line) {
                            results.push(format!("{prefix}{name}:{}: {line}", index + 1));
                        }
                    }
                }
            }
            // 保持 readdir 序（先本层文件已入 results，子目录后进栈）。
            stack.extend(nested);
        }
    }
    if results.is_empty() {
        return ToolResult {
            text: format!("No matches for \"{pattern}\" in book \"{book_id}\"."),
            details: None,
            is_error: false,
        };
    }
    if results.len() > 100 {
        let mut text = results[..100].join("\n");
        text.push_str(&format!("\n\n... [{} more matches]", results.len() - 100));
        return ToolResult { text, details: None, is_error: false };
    }
    ToolResult { text: results.join("\n"), details: None, is_error: false }
}

/// 书会话文件三件 schema（TS 逐字；read 描述随系统读开关分流）——
/// R38a 起由注册表书会话作用域层投影（单一事实源）。
pub fn book_file_tool_schemas() -> Vec<Value> {
    crate::interaction::registry::ToolRegistry::global().schemas(crate::interaction::registry::ToolScope::BookSession)
}

/// 文件工具的回环执行器（agent_loop 的 LoopToolExecutor 适配）。
pub struct ProjectToolExecutor<'a> {
    pub root: &'a Path,
}

#[async_trait::async_trait]
impl crate::interaction::agent_loop::LoopToolExecutor for ProjectToolExecutor<'_> {
    async fn execute(&self, name: &str, args: &Value) -> ToolResult {
        execute_tool(self.root, name, args).await
    }
}

/// 分发执行（R38b 单点化）：全路由注册表分发——文件三件（R38a）与
/// material 双件（R38b）均由注册表承接，未注册/不可用 → 错误文本。
pub async fn execute_tool(root: &Path, name: &str, args: &Value) -> ToolResult {
    crate::interaction::registry::execute_routed(
        &crate::interaction::registry::ToolCtx::root_only(root),
        name,
        args,
    )
    .await
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn read_ls_grep_roundtrip() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        std::fs::write(root.join("note.md"), "# 标题\n\n正文内容。").unwrap();
        std::fs::create_dir_all(root.join("books").join("b1")).unwrap();
        std::fs::write(root.join("books").join("b1").join("book.json"), "{\"id\":\"b1\"}").unwrap();

        let read = tool_read(root, &json!({ "path": "note.md" })).await;
        assert!(read.text.contains("正文内容"));
        let escaped = tool_read(root, &json!({ "path": "../../etc/passwd" })).await;
        assert!(escaped.text.contains("escapes"), "逃逸路径被拒绝：{}", escaped.text);

        let ls = tool_ls(root, &json!({ "path": "." })).await;
        assert!(ls.text.contains("note.md"));
        assert!(ls.text.contains("books/"));

        let grep = tool_grep(root, &json!({ "query": "b1" })).await;
        assert!(grep.text.contains("book.json"), "{}", grep.text);
    }

    #[test]
    fn tools_payload_shape() {
        let payload = tools_payload();
        let names: Vec<&str> = payload
            .as_array()
            .unwrap()
            .iter()
            .map(|t| t["function"]["name"].as_str().unwrap())
            .collect();
        assert_eq!(names, vec!["read", "ls", "grep"]);
        assert_eq!(payload[0]["type"], "function");
    }
}
