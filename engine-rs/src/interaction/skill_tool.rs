//! `use_skill` 运行时工具（240 号）。
//!
//! 移植自 `packages/core/src/agent/skill-tool.ts` 核心子集：skillId 校验
//! （disabled / registry）→ skill body 返回（toolResult 即指令注入）→
//! resourcePath 资源读取（safeChildPath + 512KB + UTF-8 校验）；query 分支
//! 走 LocalSearchIndex 内存 BM25 检索（241 号 `utils/local_search.rs`）。
//! catalog 提示段由 agent_route 组装。

use std::path::Path;

use serde_json::{json, Value};

use crate::interaction::project_tools::{error_result, ToolResult};
use crate::skills::{normalize_skill_id_strict, SkillRegistry};

/// 与 TS `MAX_SKILL_RESOURCE_BYTES` 一致。
const MAX_SKILL_RESOURCE_BYTES: u64 = 512 * 1024;

/// `use_skill` schema（UseSkillParams 逐字）。
pub fn use_skill_schema() -> Value {
    json!({
        "type": "function",
        "function": {
            "name": "use_skill",
            "description": "Load one available professional skill because the current user intent needs it. This only loads instructions and static references; it grants no tools or execution permissions.",
            "parameters": {
                "type": "object",
                "properties": {
                    "skillId": { "type": "string", "description": "Exact skill id from the available skill catalog." },
                    "resourcePath": { "type": "string", "description": "Optional relative text resource inside the skill folder, after the main skill has been activated." },
                    "query": { "type": "string", "description": "Natural-language query for retrieving relevant sections from this skill's references. Prefer this when the exact resource path is unknown." },
                },
                "required": ["skillId"],
            },
        },
    })
}

/// TS `toPosixPath`。
fn to_posix(path: &Path) -> String {
    path.to_string_lossy().replace('\\', "/")
}

/// 工具执行。`registry` 由装配方按项目加载；`disabled` 为会话禁用集。
pub async fn tool_use_skill(
    registry: &dyn SkillRegistry,
    disabled: &[String],
    args: &Value,
) -> ToolResult {
    let Some(raw_id) = args.get("skillId").and_then(Value::as_str).map(str::trim).filter(|s| !s.is_empty()) else {
        return error_result("use_skill requires skillId.");
    };
    let Ok(skill_id) = normalize_skill_id_strict(raw_id) else {
        return error_result(format!("Skill is not available: {raw_id}"));
    };
    if disabled.iter().any(|d| d.eq_ignore_ascii_case(&skill_id)) {
        return error_result(format!("Skill is disabled: {skill_id}"));
    }
    let Some(skill) = registry.get_skill(&skill_id) else {
        return error_result(format!("Skill is not available: {skill_id}"));
    };

    // resourcePath 分支：skill 目录内的静态文本资源。
    let mut resource: Option<(String, String)> = None;
    if let Some(resource_path) = args
        .get("resourcePath")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|s| !s.is_empty())
    {
        let Some(base_dir) = skill.base_dir.as_deref() else {
            return error_result(format!("Skill has no readable resource directory: {skill_id}"));
        };
        let full_path = match crate::utils::path::safe_child_path(base_dir, resource_path) {
            Ok(path) => path,
            Err(message) => return error_result(message),
        };
        let Ok(meta) = tokio::fs::metadata(&full_path).await else {
            return error_result(format!("Skill resource is not a file: {resource_path}"));
        };
        if !meta.is_file() {
            return error_result(format!("Skill resource is not a file: {resource_path}"));
        }
        if meta.len() > MAX_SKILL_RESOURCE_BYTES {
            return error_result(format!(
                "Skill resource is too large to load: {resource_path}"
            ));
        }
        match tokio::fs::read_to_string(&full_path).await {
            Ok(body) if !body.contains('\0') => {
                resource = Some((resource_path.to_string(), body));
            }
            Ok(_) => {
                return error_result(format!(
                    "Skill resource is not UTF-8 text: {resource_path}"
                ));
            }
            Err(e) => return error_result(format!("read failed: {e}")),
        }
    }
    // query 分支：skill 目录内 markdown 分段 → 内存 BM25 → top-4 相关段
    //（TS retrieveSkillResources 对应面）。
    let mut retrieved: Vec<Value> = Vec::new();
    if resource.is_none() {
        if let Some(query) = args
            .get("query")
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|s| !s.is_empty())
        {
            let Some(base_dir) = skill.base_dir.as_deref() else {
                return error_result(format!("Skill has no readable resource directory: {skill_id}"));
            };
            retrieved = retrieve_skill_resources(&skill_id, base_dir, query).await;
        }
    }

    let body = skill.body.trim();
    let body = if body.is_empty() { skill.description.as_str() } else { body };
    let mut text = vec![
        format!("Skill activated: {}", skill.id),
        format!("Purpose: {}", skill.description),
        String::new(),
        body.to_string(),
    ];
    if let Some((path, resource_body)) = &resource {
        text.push(String::new());
        text.push(format!("Static resource ({path}):"));
        text.push(resource_body.clone());
    }
    if !retrieved.is_empty() {
        text.push(String::new());
        text.push("Relevant static references:".to_string());
        for item in &retrieved {
            let header = format!(
                "{}:{}-{}{}",
                item["path"].as_str().unwrap_or_default(),
                item["charStart"].as_u64().unwrap_or(0),
                item["charEnd"].as_u64().unwrap_or(0),
                item["heading"]
                    .as_str()
                    .map(|h| format!(" \u{b7} {h}"))
                    .unwrap_or_default(),
            );
            text.push(format!("## {header}\n{}", item["body"].as_str().unwrap_or_default()));
        }
    }
    text.push(String::new());
    text.push(
        "This skill provides instructions only. Continue using the current session's existing tools and confirmation rules."
            .to_string(),
    );

    let mut details = json!({
        "kind": "skill_activated",
        "skillId": skill.id,
    });
    if let Some((path, _)) = &resource {
        details["resourcePath"] = json!(path);
    }
    if args.get("query").and_then(Value::as_str).map(str::trim).map(|q| !q.is_empty()).unwrap_or(false) {
        details["query"] = json!(args["query"].as_str().unwrap_or_default().trim());
        details["retrievedResources"] = json!(retrieved);
    }
    let _ = to_posix; // posix 形态仅 resource 头部展示需要；保留 helper 供后续 query 分支
    text_result(text.join("\n"), Some(details))
}

/// `retrieveSkillResources`：skill 目录文本分段 → 内存 BM25 → top-4 段。
async fn retrieve_skill_resources(
    skill_id: &str,
    base_dir: &str,
    query: &str,
) -> Vec<Value> {
    let files = list_skill_text_files(base_dir).await;
    let mut documents: Vec<crate::utils::local_search::SearchDocument> = Vec::new();
    for path in &files {
        let Ok(full_path) = crate::utils::path::safe_child_path(base_dir, path) else {
            continue;
        };
        let Ok(meta) = tokio::fs::metadata(&full_path).await else {
            continue;
        };
        if !meta.is_file() || meta.len() > MAX_SKILL_RESOURCE_BYTES {
            continue;
        }
        let Ok(body) = tokio::fs::read_to_string(&full_path).await else {
            continue;
        };
        if body.contains('\0') {
            continue;
        }
        for (index, segment) in crate::utils::local_search::split_markdown_for_search(&body)
            .into_iter()
            .enumerate()
        {
            documents.push(crate::utils::local_search::SearchDocument {
                id: format!("skill:{skill_id}:{path}:{index}"),
                scope: format!("skill:{skill_id}"),
                kind: "skill-reference".to_string(),
                source: format!("{}:{}-{}", path, segment.char_start, segment.char_end),
                title: [path.as_str(), segment.heading.as_str()]
                    .iter()
                    .filter(|part| !part.is_empty())
                    .cloned()
                    .collect::<Vec<_>>()
                    .join(" \u{b7} "),
                body: segment.body,
            });
        }
    }

    let Ok(index) = crate::utils::local_search::LocalSearchIndex::new(":memory:") else {
        return Vec::new();
    };
    let scope = format!("skill:{skill_id}");
    if index.replace_scope(&scope, &documents).is_err() {
        return Vec::new();
    }
    let hits = index.search(
        query,
        &crate::utils::local_search::SearchOptions {
            scope: &scope,
            kinds: &[],
            limit: 4,
        },
    );
    index.close();
    hits.iter()
        .map(|hit| {
            json!({
                "path": hit.source.split(':').next().unwrap_or(""),
                "heading": Value::Null,
                "body": hit.body,
                "charStart": 0,
                "charEnd": hit.body.chars().count(),
                "score": hit.score,
            })
        })
        .collect()
}

/// 列 skill 目录内文本资源（浅层 + references/ 一层；跳过隐藏文件）。
async fn list_skill_text_files(base_dir: &str) -> Vec<String> {
    let mut files = Vec::new();
    let mut stack = vec![base_dir.to_string()];
    while let Some(dir) = stack.pop() {
        let Ok(mut entries) = tokio::fs::read_dir(&dir).await else {
            continue;
        };
        while let Ok(Some(entry)) = entries.next_entry().await {
            let name = entry.file_name().to_string_lossy().to_string();
            if name.starts_with('.') {
                continue;
            }
            let path = format!("{}/{}", dir.trim_end_matches('/'), name);
            let is_dir = entry.file_type().await.map(|t| t.is_dir()).unwrap_or(false);
            if is_dir {
                stack.push(path);
                continue;
            }
            let is_text = ["md", "txt", "json", "yaml", "yml", "csv"]
                .iter()
                .any(|ext| path.ends_with(&format!(".{ext}")));
            if is_text {
                files.push(path);
            }
        }
    }
    files
}

fn text_result(text: impl Into<String>, details: Option<Value>) -> ToolResult {
    ToolResult { text: text.into(), details, is_error: false }
}

/// TS `serializeSkillCatalog`：目录 JSON（`<`/`>` 转义防提示注入）。
pub fn serialize_skill_catalog(skills: &[(String, String, String)]) -> String {
    let entries: Vec<Value> = skills
        .iter()
        .map(|(id, name, description)| {
            json!({
                "id": id.replace('<', "\\u003c").replace('>', "\\u003e"),
                "name": name.replace('<', "\\u003c").replace('>', "\\u003e"),
                "description": description.replace('<', "\\u003c").replace('>', "\\u003e"),
            })
        })
        .collect();
    serde_json::to_string(&entries).unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::skills::{create_skill_registry, AgentSkill, SkillSource};

    fn registry() -> impl SkillRegistry {
        create_skill_registry(vec![AgentSkill {
            id: "combat-tactics".into(),
            name: "Combat Tactics".into(),
            description: "战斗策略".into(),
            body: "## 规则\n\n以静制动。".into(),
            source: SkillSource::Project,
            base_dir: None,
        }])
    }

    #[test]
    fn schema_shape_matches_ts_params() {
        let schema = use_skill_schema();
        assert_eq!(schema["function"]["name"], "use_skill");
        assert_eq!(
            schema["function"]["parameters"]["required"],
            json!(["skillId"])
        );
    }

    #[tokio::test]
    async fn activate_returns_body_and_details() {
        let registry = registry();
        let result = tool_use_skill(&registry, &[], &json!({ "skillId": "combat-tactics" })).await;
        assert!(!result.is_error);
        assert!(result.text.contains("Skill activated: combat-tactics"), "{}", result.text);
        assert!(result.text.contains("Purpose: 战斗策略"));
        assert!(result.text.contains("以静制动"));
        assert!(result.text.contains("This skill provides instructions only."));
        assert_eq!(result.details.as_ref().unwrap()["kind"], "skill_activated");
        assert_eq!(result.details.as_ref().unwrap()["skillId"], "combat-tactics");
    }

    #[tokio::test]
    async fn disabled_and_missing_faces() {
        let registry = registry();
        let result = tool_use_skill(
            &registry,
            &["combat-tactics".to_string()],
            &json!({ "skillId": "combat-tactics" }),
        )
        .await;
        assert_eq!(result.text, "Skill is disabled: combat-tactics");
        let result = tool_use_skill(&registry, &[], &json!({ "skillId": "nope" })).await;
        assert_eq!(result.text, "Skill is not available: nope");
        let result = tool_use_skill(&registry, &[], &json!({})).await;
        assert_eq!(result.text, "use_skill requires skillId.");
    }

    #[test]
    fn catalog_escapes_angle_brackets() {
        let catalog = serialize_skill_catalog(&[(
            "x".into(),
            "<script>".into(),
            "desc".into(),
        )]);
        // JSON 文本层：内容中的反斜杠被 stringify 转义为 `\\`（TS JSON.stringify 同款）。
        assert!(catalog.contains("\\\\u003cscript\\\\u003e"), "{catalog}");
    }
}

#[cfg(test)]
mod query_retrieval_tests {
    use super::*;
    use crate::skills::{create_skill_registry, AgentSkill, SkillSource};

    /// 241 号：query 分支——skill 目录分段检索，details 携带 retrievedResources。
    #[tokio::test]
    async fn query_retrieves_relevant_references() {
        let dir = tempfile::tempdir().unwrap();
        let base = dir.path().join("combat-tactics");
        std::fs::create_dir_all(&base).unwrap();
        std::fs::write(
            base.join("SKILL.md"),
            "---\nname: 战斗策略\ndescription: 战斗规则\n---\n\n以静制动，后发制人。\n",
        )
        .unwrap();
        std::fs::create_dir_all(base.join("references")).unwrap();
        std::fs::write(
            base.join("references").join("opening.md"),
            "# 开局布局\n\n开篇先建立核心冲突与主角压力。\n",
        )
        .unwrap();
        std::fs::write(
            base.join("references").join("dialogue.md"),
            "# 对白\n\n对白要短，口语化。\n",
        )
        .unwrap();

        let registry = create_skill_registry(vec![AgentSkill {
            id: "combat-tactics".into(),
            name: "Combat Tactics".into(),
            description: "战斗策略".into(),
            body: "以静制动。".into(),
            source: SkillSource::Project,
            base_dir: Some(base.to_string_lossy().into_owned()),
        }]);

        let result = tool_use_skill(
            &registry,
            &[],
            &json!({ "skillId": "combat-tactics", "query": "开篇 核心冲突 布局" }),
        )
        .await;
        assert!(!result.is_error, "{}", result.text);
        let details = result.details.unwrap();
        assert_eq!(details["kind"], "skill_activated");
        assert_eq!(details["query"], "开篇 核心冲突 布局");
        let resources = details["retrievedResources"].as_array().unwrap();
        assert!(!resources.is_empty(), "应至少召回 opening.md 分段");
        assert!(resources.iter().any(|r| {
            r["body"].as_str().unwrap_or_default().contains("核心冲突")
        }), "{resources:?}");
        assert!(
            result.text.contains("Relevant static references:"),
            "{}",
            result.text
        );
    }
}
