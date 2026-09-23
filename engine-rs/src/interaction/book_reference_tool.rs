//! `manage_book_reference` 工具（216 号）。
//!
//! 移植自 `packages/core/src/agent/agent-tools.ts` 的
//! `createManageBookReferenceTool`：把已入库素材绑定到活动书（用户自定义
//! 用途），或解绑/列出绑定。素材本体只在 `.inkos/materials` 存一份，绑定
//! 不复制正文、不直接改写正典；写作期由 composer 引用选段注入。
//! 注册面：book/edit 会话（TS edit 过滤器不剔除）；不在
//! PRODUCTION_MUTATION_TOOL_NAMES（绑定清单非章节/真相写入面）。

use serde_json::{json, Value};

use crate::interaction::project_tools::{error_result, ToolResult};
use crate::interaction::registry::{schema_description, schema_parameters, MutationKind, ToolDef};
use crate::references::{bind_book_reference, list_book_references, unbind_book_reference, BindBookReferenceInput};

fn text_result(text: impl Into<String>, details: Option<Value>) -> ToolResult {
    ToolResult { text: text.into(), details, is_error: false }
}

fn field_str<'a>(args: &'a Value, name: &str) -> Option<&'a str> {
    args.get(name)
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|v| !v.is_empty())
}

/// `manage_book_reference` schema（ManageBookReferenceParams 逐字）。
pub fn manage_book_reference_schema() -> Value {
    json!({
        "type": "function",
        "function": {
            "name": "manage_book_reference",
            "description": "Bind already-ingested project materials to the active book with user-defined purposes, list current bindings, or unbind them. The material remains stored once under .inkos/materials. Binding never copies prose into the book and never changes canon by itself.",
            "parameters": {
                "type": "object",
                "properties": {
                    "action": {
                        "type": "string",
                        "enum": ["list", "bind", "unbind"],
                        "description": "list = inspect bindings; bind = attach an ingested material to this book; unbind = remove that attachment without deleting the project asset."
                    },
                    "materialId": { "type": "string", "description": "Exact material asset id returned by ingest_material. Required for bind and unbind." },
                    "uses": {
                        "type": "array",
                        "items": { "type": "string" },
                        "description": "bind only: user-defined natural-language purposes, e.g. 开篇机制, 人物关系, 调查节奏. Preserve the user's words instead of mapping them to a fixed taxonomy."
                    },
                    "note": { "type": "string", "description": "bind only: optional user instruction that limits how this reference may be used." },
                },
                "required": ["action"],
            },
        },
    })
}

/// 工具执行（TS execute 逐字文本面；失败为错误文本）。
pub async fn tool_manage_book_reference(root: &std::path::Path, active_book_id: &str, args: &Value) -> ToolResult {
    let Some(action) = field_str(args, "action") else {
        return error_result("manage_book_reference requires action.");
    };
    match action {
        "list" => {
            let listed = match list_book_references(root, active_book_id) {
                Ok(listed) => listed,
                Err(message) => return error_result(message),
            };
            let references: Vec<Value> = listed
                .references
                .iter()
                .map(|reference| {
                    json!({
                        "materialId": reference.material_id,
                        "title": reference.title,
                        "uses": reference.uses,
                        "note": reference.note,
                        "available": reference.available,
                        "error": reference.error,
                    })
                })
                .collect();
            let text = if references.is_empty() {
                format!("No reference materials are bound to book \"{active_book_id}\".")
            } else {
                let mut lines = vec![format!("Bound references for \"{active_book_id}\":")];
                for reference in &listed.references {
                    let mut entry = vec![format!(
                        "- {} ({})",
                        reference.title.as_deref().unwrap_or(&reference.material_id),
                        reference.material_id
                    )];
                    entry.push(format!("  uses: {}", reference.uses.join("; ")));
                    if let Some(note) = &reference.note {
                        entry.push(format!("  note: {note}"));
                    }
                    if !reference.available {
                        entry.push(format!(
                            "  unavailable: {}",
                            reference.error.as_deref().unwrap_or("material missing")
                        ));
                    }
                    lines.push(entry.join("\n"));
                }
                lines.join("\n")
            };
            text_result(
                text,
                Some(json!({
                    "kind": "book_reference_list",
                    "bookId": active_book_id,
                    "references": references,
                })),
            )
        }
        "unbind" | "bind" => {
            let Some(material_id) = field_str(args, "materialId") else {
                return error_result(format!("manage_book_reference.{action} requires materialId."));
            };
            if action == "unbind" {
                let (removed, _) = match unbind_book_reference(root, active_book_id, material_id) {
                    Ok(result) => result,
                    Err(message) => return error_result(message),
                };
                let text = if removed {
                    format!("Reference {material_id} was unbound from \"{active_book_id}\". The project asset was kept.")
                } else {
                    format!("Reference {material_id} was not bound to \"{active_book_id}\".")
                };
                return text_result(
                    text,
                    Some(json!({
                        "kind": "book_reference_unbound",
                        "bookId": active_book_id,
                        "materialId": material_id,
                        "removed": removed,
                    })),
                );
            }
            let uses: Vec<String> = args
                .get("uses")
                .and_then(Value::as_array)
                .map(|items| {
                    items
                        .iter()
                        .filter_map(Value::as_str)
                        .map(str::to_string)
                        .collect()
                })
                .unwrap_or_default();
            let note = field_str(args, "note");
            let manifest = match bind_book_reference(
                root,
                active_book_id,
                &BindBookReferenceInput { material_id, uses: &uses, note },
            ) {
                Ok(manifest) => manifest,
                Err(message) => return error_result(message),
            };
            let binding = manifest
                .bindings
                .iter()
                .find(|entry| entry.material_id == material_id.trim())
                .expect("just bound");
            let mut lines = vec![
                format!("Reference {} was bound to \"{active_book_id}\".", binding.material_id),
                format!("Uses: {}", binding.uses.join("; ")),
            ];
            if let Some(note) = &binding.note {
                lines.push(format!("Note: {note}"));
            }
            lines.push(
                "Future chapter composition may select relevant sections; the reference does not override author intent or canon."
                    .to_string(),
            );
            text_result(
                lines.join("\n"),
                Some(json!({
                    "kind": "book_reference_bound",
                    "bookId": active_book_id,
                    "materialId": binding.material_id,
                    "uses": binding.uses,
                    "note": binding.note,
                })),
            )
        }
        other => error_result(format!("Unknown action: {other}")),
    }
}


// ── 注册模块（R38b）：manage_book_reference 单件——绑定清单写面 →
//    ProjectWrite（216 号注释：非生产写入面，不在剔除名单）；available =
//    活动书在场（book/edit 会话恒有）。 ──

crate::interaction::registry::tool_def!(
    ManageBookReference,
    "manage_book_reference",
    MutationKind::ProjectWrite,
    ctx, args,
    { schema_description(&[manage_book_reference_schema()], "manage_book_reference") },
    schema_parameters(&[manage_book_reference_schema()], "manage_book_reference"),
    ctx.reference_book_id.is_some(),
    tool_manage_book_reference(ctx.root, ctx.reference_book_id.expect("available 门控"), args).await
);

/// 注册表汇聚口（registry 装配序 = 原 ChatToolRouter 分发链序）。
pub(crate) fn defs() -> Vec<Box<dyn ToolDef>> {
    vec![Box::new(ManageBookReference)]
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture(root: &std::path::Path) {
        std::fs::create_dir_all(root.join("books").join("b1")).unwrap();
        let materials = root.join(".inkos").join("materials");
        std::fs::create_dir_all(&materials).unwrap();
        std::fs::write(
            materials.join("mat1.json"),
            json!({
                "id": "mat1", "title": "开篇参考", "kind": "text", "purpose": "reference",
                "source": "upload", "mimeType": "text/markdown",
                "markdownPath": ".inkos/materials/mat1.md",
                "manifestPath": ".inkos/materials/mat1.json",
                "charCount": 100, "excerpt": "..."
            })
            .to_string(),
        )
        .unwrap();
        std::fs::write(materials.join("mat1.md"), "正文。").unwrap();
    }

    #[test]
    fn schema_shape_matches_ts_params() {
        let schema = manage_book_reference_schema();
        assert_eq!(schema["function"]["name"], "manage_book_reference");
        assert_eq!(schema["function"]["parameters"]["required"], json!(["action"]));
        assert_eq!(
            schema["function"]["parameters"]["properties"]["action"]["enum"],
            json!(["list", "bind", "unbind"])
        );
    }

    #[tokio::test]
    async fn list_bind_unbind_text_faces() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        fixture(root);
        // list 空态。
        let result = tool_manage_book_reference(root, "b1", &json!({"action": "list"})).await;
        assert!(result.text.contains("No reference materials are bound to book \"b1\"."));
        assert_eq!(result.details.as_ref().unwrap()["kind"], "book_reference_list");
        // bind。
        let result = tool_manage_book_reference(
            root,
            "b1",
            &json!({"action": "bind", "materialId": "mat1", "uses": ["开篇机制", "人物关系"], "note": "慢节奏"}),
        )
        .await;
        assert!(result.text.contains("Reference mat1 was bound to \"b1\"."), "{}", result.text);
        assert!(result.text.contains("Uses: 开篇机制; 人物关系"));
        assert!(result.text.contains("Note: 慢节奏"));
        assert!(result.text.contains("Future chapter composition may select relevant sections"));
        assert_eq!(result.details.as_ref().unwrap()["kind"], "book_reference_bound");
        assert_eq!(result.details.as_ref().unwrap()["uses"], json!(["开篇机制", "人物关系"]));
        // list 有绑定。
        let result = tool_manage_book_reference(root, "b1", &json!({"action": "list"})).await;
        assert!(result.text.contains("- 开篇参考 (mat1)"), "{}", result.text);
        // unbind。
        let result = tool_manage_book_reference(root, "b1", &json!({"action": "unbind", "materialId": "mat1"})).await;
        assert!(
            result.text.contains("Reference mat1 was unbound from \"b1\". The project asset was kept."),
            "{}",
            result.text
        );
        assert_eq!(result.details.as_ref().unwrap()["kind"], "book_reference_unbound");
        // 再 unbind：未绑定面。
        let result = tool_manage_book_reference(root, "b1", &json!({"action": "unbind", "materialId": "mat1"})).await;
        assert!(result.text.contains("was not bound to"));
    }

    #[tokio::test]
    async fn error_faces() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        fixture(root);
        // materialId 缺失。
        let result = tool_manage_book_reference(root, "b1", &json!({"action": "bind"})).await;
        assert_eq!(result.text, "manage_book_reference.bind requires materialId.");
        // 非法素材 id → 校验错误文本。
        let result = tool_manage_book_reference(root, "b1", &json!({"action": "bind", "materialId": "../x", "uses": ["u"]})).await;
        assert_eq!(result.text, "Invalid materialId: \"../x\"");
        // 未知 action。
        let result = tool_manage_book_reference(root, "b1", &json!({"action": "destroy"})).await;
        assert!(result.text.starts_with("Unknown action:"));
    }
}
