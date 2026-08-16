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

/// 工具注册表项：OpenAI function schema + 执行器。
pub struct InteractionTool {
    pub name: &'static str,
    pub description: &'static str,
    pub parameters: Value,
}

/// 本轮工具集（read/ls/grep）。
pub fn interaction_tools() -> Vec<InteractionTool> {
    vec![
        InteractionTool {
            name: "read",
            description: "读取项目内文本文件内容",
            parameters: json!({
                "type": "object",
                "properties": { "path": { "type": "string", "description": "项目相对路径" } },
                "required": ["path"],
            }),
        },
        InteractionTool {
            name: "ls",
            description: "列出项目目录内容",
            parameters: json!({
                "type": "object",
                "properties": { "path": { "type": "string", "description": "项目相对路径（默认 .）" } },
            }),
        },
        InteractionTool {
            name: "grep",
            description: "在项目文本文件中搜索",
            parameters: json!({
                "type": "object",
                "properties": {
                    "query": { "type": "string" },
                    "path": { "type": "string", "description": "搜索根（默认 .）" },
                },
                "required": ["query"],
            }),
        },
    ]
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

/// 分发执行；未知工具 → 错误文本。
pub async fn execute_tool(root: &Path, name: &str, args: &Value) -> ToolResult {
    match name {
        "read" => tool_read(root, args).await,
        "ls" => tool_ls(root, args).await,
        "grep" => tool_grep(root, args).await,
        // material 双工具（83 号）：全部聊天会话注册（TS agent-session 各分支）。
        "ingest_material" => crate::interaction::material_tools::tool_ingest_material(root, args).await,
        "retrieve_material" => crate::interaction::material_tools::tool_retrieve_material(root, args).await,
        other => error_result(format!("Unknown tool: {other}")),
    }
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
