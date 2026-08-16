//! material 聊天工具面（83 号）：ingest_material / retrieve_material。
//!
//! 移植自 `packages/core/src/agent/agent-tools.ts` 的
//! `createIngestMaterialTool` / `createRetrieveMaterialTool`（文本/details
//! 逐字）；域本体见 [`crate::materials`]。TS 侧两工具注册于几乎全部会话
//! （chat/short/script/storyboard/film/play/book-create/edit），Rust 聊天面
//! 经 [`crate::interaction::project_tools::execute_tool`] 恒分发。

use std::path::Path;

use serde_json::{json, Value};

use crate::interaction::project_tools::{error_result, ToolResult};
use crate::materials;

fn text_result(text: impl Into<String>, details: Option<Value>) -> ToolResult {
    ToolResult { text: text.into(), details, is_error: false }
}

/// `ingest_material`：归档 URL/文件为可追溯 Markdown 材料卡。
pub async fn tool_ingest_material(root: &Path, args: &Value) -> ToolResult {
    let source_kind = args.get("sourceKind").and_then(Value::as_str).unwrap_or_default();
    if !matches!(source_kind, "url" | "file") {
        return error_result(format!(
            "Invalid ingest_material.sourceKind: {source_kind}（expected url | file）"
        ));
    }
    if let Some(purpose) = args.get("purpose").and_then(Value::as_str) {
        if !materials::PURPOSES.contains(&purpose) {
            return error_result(format!(
                "Invalid ingest_material.purpose: {purpose}（expected one of reference | worldbuilding | script | storyboard | research | general）"
            ));
        }
    }
    let field = |name: &str| args.get(name).and_then(Value::as_str).filter(|v| !v.is_empty());
    let input = materials::IngestMaterialInput {
        source_kind,
        url: field("url"),
        file_path: field("filePath"),
        filename: field("filename"),
        mime_type: field("mimeType"),
        title: field("title"),
        purpose: field("purpose"),
    };
    match materials::ingest_material(root, &input).await {
        Ok(asset) => {
            // TS lines.filter(Boolean).join("\n")：模板空行被滤（无 PDF pages
            // 行时 Kind 行直连 Excerpt:）。
            let mut lines = vec![
                format!("Material ingested: {}", asset.markdown_path),
                format!("Kind: {}; chars: {}; source: {}", asset.kind, asset.char_count, asset.source),
            ];
            if let Some(pages) = asset.total_pages {
                lines.push(format!("PDF pages: {pages}"));
            }
            lines.push("Excerpt:".to_string());
            lines.push(asset.excerpt.clone());
            text_result(
                lines.join("\n"),
                Some(json!({ "kind": "material_ingested", "asset": asset })),
            )
        }
        Err(message) => error_result(message),
    }
}

/// `retrieve_material`：按语义查询召回已归档材料片段。
pub async fn tool_retrieve_material(root: &Path, args: &Value) -> ToolResult {
    let Some(query) = args.get("query").and_then(Value::as_str).map(str::trim).filter(|q| !q.is_empty()) else {
        return error_result("retrieve_material requires a query argument");
    };
    let purpose = args.get("purpose").and_then(Value::as_str);
    if let Some(purpose) = purpose {
        if !materials::PURPOSES.contains(&purpose) {
            return error_result(format!(
                "Invalid retrieve_material.purpose: {purpose}（expected one of reference | worldbuilding | script | storyboard | research | general）"
            ));
        }
    }
    let limit = args.get("limit").and_then(Value::as_f64);
    let input = materials::RetrieveMaterialsInput {
        query: query.to_string(),
        purpose: purpose.map(String::from),
        limit,
    };
    let results = materials::retrieve_materials(root, &input).await;
    let mut details = json!({
        "kind": "material_retrieval",
        "query": query,
        "results": results,
    });
    if let Some(purpose) = purpose {
        details
            .as_object_mut()
            .unwrap()
            .insert("purpose".into(), json!(purpose));
    }
    if results.is_empty() {
        return text_result(
            "No matching archived materials were found. Ask the user to upload or ingest relevant material if needed.",
            Some(details),
        );
    }
    let plural = if results.len() == 1 { "" } else { "s" };
    let mut lines = vec![format!("Retrieved {} material snippet{plural}.", results.len()), String::new()];
    for (index, result) in results.iter().enumerate() {
        lines.push(format!("## {}. {}", index + 1, result.title));
        lines.push(format!("- source: {}", result.source));
        lines.push(format!(
            "- path: {}:{}-{}",
            result.markdown_path, result.char_start, result.char_end
        ));
        lines.push(format!("- purpose: {}", result.purpose));
        lines.push(format!("- score: {:.2}", result.score));
        lines.push(String::new());
        lines.push(result.excerpt.clone());
        lines.push(String::new());
    }
    lines.pop();
    text_result(lines.join("\n"), Some(details))
}

/// 两工具 schema（OpenAI function 形态，参数描述逐字）。
pub fn material_tool_schemas() -> Vec<Value> {
    vec![
        json!({
            "type": "function",
            "function": {
                "name": "ingest_material",
                "description": "Extract and archive a user-provided URL or uploaded file into .inkos/materials as traceable Markdown. Supports HTML/text/JSON/Markdown/PDF. This creates reference material only; it must not mutate canon, chapters, scripts, or play state.",
                "parameters": {
                    "type": "object",
                    "properties": {
                        "sourceKind": {
                            "type": "string",
                            "enum": ["url", "file"],
                            "description": "Use url for an external URL; use file for a user-uploaded file path shown in the Uploaded Files block.",
                        },
                        "url": {
                            "type": "string",
                            "description": "HTTP/HTTPS URL to fetch and extract. Supports HTML/text/JSON/PDF.",
                        },
                        "filePath": {
                            "type": "string",
                            "description": "Project-relative stored_path from the Uploaded Files block, e.g. .inkos/uploads/session/file.pdf.",
                        },
                        "filename": {
                            "type": "string",
                            "description": "Original filename when known.",
                        },
                        "mimeType": {
                            "type": "string",
                            "description": "MIME type when known, e.g. application/pdf or text/markdown.",
                        },
                        "title": {
                            "type": "string",
                            "description": "Human-readable material title.",
                        },
                        "purpose": {
                            "type": "string",
                            "enum": ["reference", "worldbuilding", "script", "storyboard", "research", "general"],
                            "description": "Why this material is being ingested. It remains reference material unless the user explicitly promotes it.",
                        },
                    },
                    "required": ["sourceKind"],
                },
            },
        }),
        json!({
            "type": "function",
            "function": {
                "name": "retrieve_material",
                "description": "Retrieve traceable snippets from previously ingested .inkos/materials reference cards. The agent supplies the semantic query; InkOS returns evidence pointers. This must not mutate canon, chapters, scripts, or play state.",
                "parameters": {
                    "type": "object",
                    "properties": {
                        "query": {
                            "type": "string",
                            "description": "Natural-language query written by the agent from the user's current task, e.g. 冷库赔偿款 0607 账页 or storyboard shot requirements.",
                        },
                        "purpose": {
                            "type": "string",
                            "enum": ["reference", "worldbuilding", "script", "storyboard", "research", "general"],
                            "description": "Optional material purpose filter.",
                        },
                        "limit": {
                            "type": "number",
                            "description": "Maximum number of material snippets to return. Default 5.",
                        },
                    },
                    "required": ["query"],
                },
            },
        }),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn ingest_tool_roundtrip_and_retrieve_text() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        std::fs::write(root.join("note.md"), "账页记载着冷库赔偿款。").unwrap();

        let ingested = tool_ingest_material(
            root,
            &json!({ "sourceKind": "file", "filePath": "note.md", "title": "账页资料" }),
        )
        .await;
        assert!(!ingested.is_error, "{}", ingested.text);
        assert!(ingested.text.starts_with("Material ingested: .inkos/materials/"), "{}", ingested.text);
        assert!(ingested.text.contains("Kind: text; chars: 11; source: note.md"), "{}", ingested.text);
        assert!(ingested.text.contains("Excerpt:\n账页记载着冷库赔偿款。"), "{}", ingested.text);
        let details = ingested.details.unwrap();
        assert_eq!(details["kind"], "material_ingested");
        assert_eq!(details["asset"]["title"], "账页资料");

        // 召回：文本形态（编号/四行元数据/空行保留）。
        let retrieved = tool_retrieve_material(root, &json!({ "query": "冷库赔偿款" })).await;
        assert!(!retrieved.is_error);
        assert!(retrieved.text.starts_with("Retrieved 1 material snippet.\n\n## 1. 账页资料"), "{}", retrieved.text);
        assert!(retrieved.text.contains("- purpose: reference"), "{}", retrieved.text);
        assert!(retrieved.text.contains("- score: "), "{}", retrieved.text);
        assert!(retrieved.text.contains(".md:0-"), "{}", retrieved.text);
        let details = retrieved.details.unwrap();
        assert_eq!(details["kind"], "material_retrieval");
        assert_eq!(details["query"], "冷库赔偿款");
        assert_eq!(details["results"].as_array().unwrap().len(), 1);
        assert!(details.get("purpose").is_none(), "未过滤时 purpose 键不出现");

        // purpose 过滤出现在 details。
        let filtered = tool_retrieve_material(
            root,
            &json!({ "query": "冷库", "purpose": "worldbuilding" }),
        )
        .await;
        assert_eq!(
            filtered.text,
            "No matching archived materials were found. Ask the user to upload or ingest relevant material if needed."
        );
        assert_eq!(filtered.details.unwrap()["purpose"], "worldbuilding");
    }

    #[tokio::test]
    async fn tool_validation_errors() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        let bad_kind = tool_ingest_material(root, &json!({ "sourceKind": "ftp" })).await;
        assert!(bad_kind.is_error && bad_kind.text.contains("Invalid ingest_material.sourceKind"));
        let bad_purpose = tool_ingest_material(
            root,
            &json!({ "sourceKind": "file", "filePath": "x", "purpose": "nope" }),
        )
        .await;
        assert!(bad_purpose.is_error && bad_purpose.text.contains("Invalid ingest_material.purpose"));
        let missing_query = tool_retrieve_material(root, &json!({})).await;
        assert!(missing_query.is_error && missing_query.text.contains("requires a query"));
        let bad_retrieve_purpose = tool_retrieve_material(root, &json!({ "query": "x", "purpose": "nope" })).await;
        assert!(bad_retrieve_purpose.is_error && bad_retrieve_purpose.text.contains("Invalid retrieve_material.purpose"));
    }

    #[test]
    fn schemas_shape() {
        let schemas = material_tool_schemas();
        assert_eq!(schemas.len(), 2);
        assert_eq!(schemas[0]["function"]["name"], "ingest_material");
        assert_eq!(schemas[0]["function"]["parameters"]["required"][0], "sourceKind");
        assert_eq!(
            schemas[0]["function"]["parameters"]["properties"]["sourceKind"]["enum"]
                .as_array()
                .unwrap()
                .len(),
            2
        );
        assert_eq!(schemas[1]["function"]["name"], "retrieve_material");
        assert_eq!(schemas[1]["function"]["parameters"]["required"][0], "query");
    }
}
