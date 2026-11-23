//! Prompt pack 覆盖加载（prompt-pack）。
//!
//! 移植自 `packages/core/src/prompts/prompt-pack.ts`（110 行）。
//! project → user → builtin 三级优先级加载 prompt 内容：项目/用户目录的 `.md` 覆盖文件优先于 builtin。
//!
//! ## 架构
//! fs 经 `StateStore`（crate::state::store）trait 注入（与 state 域 async 编排同基座），
//! 可用 [`InMemoryStateStore`](crate::state::store::InMemoryStateStore) 单测。

use crate::prompts::{get_builtin_prompt, PromptSource};
use crate::state::store::StateStore;

/// 已加载的 prompt pack prompt。对齐 TS `LoadedPromptPackPrompt`。
#[derive(Debug, Clone, PartialEq)]
pub struct LoadedPromptPackPrompt {
    pub prompt_id: String,
    pub content: String,
    pub source: PromptSource,
    pub path: Option<String>,
    pub title: Option<String>,
    pub pack_id: Option<String>,
}

/// 加载输入。对齐 TS `LoadPromptPackPromptInput`。
#[derive(Debug, Clone, Default)]
pub struct LoadPromptPackPromptInput {
    pub prompt_id: String,
    pub project_root: Option<String>,
    pub user_root: Option<String>,
}

/// prompt 未找到错误。对齐 TS `PromptPackPromptNotFoundError`。
#[derive(Debug, thiserror::Error)]
#[error("Prompt pack prompt not found: {prompt_id}")]
pub struct PromptPackPromptNotFoundError {
    pub prompt_id: String,
}

/// 三级优先级加载 prompt：project 覆盖 → user 覆盖 → builtin。对齐 TS `loadPromptPackPrompt`。
///
/// 三级都无 → [`PromptPackPromptNotFoundError`]。
pub async fn load_prompt_pack_prompt(
    store: &dyn StateStore,
    input: &LoadPromptPackPromptInput,
) -> Result<LoadedPromptPackPrompt, PromptPackPromptNotFoundError> {
    let prompt_id = normalize_prompt_id(&input.prompt_id);

    if let Some(project_root) = &input.project_root {
        let project_path = prompt_override_path(project_root, &prompt_id);
        if let Some(content) = store.read_to_string(&project_path).await.ok().flatten() {
            return Ok(LoadedPromptPackPrompt {
                prompt_id,
                content,
                source: PromptSource::Project,
                path: Some(project_path),
                title: None,
                pack_id: None,
            });
        }
    }

    if let Some(user_root) = &input.user_root {
        let user_path = prompt_override_path(user_root, &prompt_id);
        if let Some(content) = store.read_to_string(&user_path).await.ok().flatten() {
            return Ok(LoadedPromptPackPrompt {
                prompt_id,
                content,
                source: PromptSource::User,
                path: Some(user_path),
                title: None,
                pack_id: None,
            });
        }
    }

    if let Some(b) = get_builtin_prompt(&prompt_id) {
        return Ok(LoadedPromptPackPrompt {
            prompt_id: b.id.to_string(),
            content: b.content.to_string(),
            source: PromptSource::Builtin,
            path: None,
            title: Some(b.title.to_string()),
            pack_id: Some(b.pack_id.to_string()),
        });
    }

    Err(PromptPackPromptNotFoundError { prompt_id })
}

/// 在 basePrompt 后追加 prompt pack 指导。对齐 TS `appendPromptPackGuidance`。
///
/// 加载的 prompt 内容为空（trim 后）→ 原样返回 base；否则追加 `## Prompt Pack Guidance (...)` 段。
pub async fn append_prompt_pack_guidance(
    store: &dyn StateStore,
    base_prompt: &str,
    input: &LoadPromptPackPromptInput,
) -> Result<String, PromptPackPromptNotFoundError> {
    let prompt = load_prompt_pack_prompt(store, input).await?;
    let content = prompt.content.trim();
    if content.is_empty() {
        return Ok(base_prompt.to_string());
    }
    let source_str = match prompt.source {
        PromptSource::Project => "project",
        PromptSource::User => "user",
        PromptSource::Builtin => "builtin",
        PromptSource::External => "external",
    };
    Ok(format!(
        "{base_prompt}\n\n## Prompt Pack Guidance ({}, source: {source_str})\n{content}",
        prompt.prompt_id
    ))
}

/// 计算 prompt 覆盖文件路径：`{root}/prompt/{seg...}/{last}.md`。
///
/// 对齐 TS `promptOverridePath`：`longform.writer` → `{root}/prompt/longform/writer.md`。
pub fn prompt_override_path(root: &str, prompt_id: &str) -> String {
    let normalized = normalize_prompt_id(prompt_id);
    let parts: Vec<&str> = normalized.split('.').collect();
    if parts.len() < 2 {
        // 无分隔符：整体作为文件名。
        return crate::state::store::join_path(
            &crate::state::store::join_path(root, "prompt"),
            &format!("{normalized}.md"),
        );
    }
    let (last, dirs) = parts.split_last().unwrap();
    let mut path = crate::state::store::join_path(root, "prompt");
    for d in dirs {
        path = crate::state::store::join_path(&path, d);
    }
    crate::state::store::join_path(&path, &format!("{last}.md"))
}

/// 规范化 prompt id：trim + lower。对齐 TS `normalizePromptId`。
fn normalize_prompt_id(prompt_id: &str) -> String {
    prompt_id.trim().to_lowercase()
}

// ── R54/1658 号：prompt 覆盖版本账本 ─────────────────────────────
//
// `{root}/prompt/.versions.jsonl` append-only（沿 writing-log 惯例：读旧+
// 拼接+原子写；读取宽容坏行跳过）。行 = {ts, promptId, action, content}
// （action = put | delete | restore；delete 行 content 空）。回滚 = 取版本行
// 内容复写覆盖文件（复用 PUT 语义）+ 追加 restore 行——回滚本身也是一次
// 可再回滚的变更（append-only 语义闭环）。

/// 版本行（serde camelCase）。
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PromptVersionRecord {
    pub ts: String,
    pub prompt_id: String,
    pub action: String,
    pub content: String,
}

fn prompt_versions_path(root: &str) -> std::path::PathBuf {
    std::path::Path::new(root)
        .join("prompt")
        .join(".versions.jsonl")
}

/// 版本文本 → 行（坏行跳过：serde 要求四字段齐全且为串——TS
/// `isPromptVersionRecord` 同款形状守卫）。
fn parse_version_lines(text: &str) -> Vec<PromptVersionRecord> {
    text.lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .filter_map(|line| serde_json::from_str(line).ok())
        .collect()
}

/// 读全部版本行（跨 prompt；坏行跳过）。**读不出来（权限/IO）= Err**。
///
/// 1666 号：此前任何读错误都返回空——追加面据此用「只有新行」的内容覆盖整本
/// 版本历史（对应版本从此无法回滚）；读面把「读不到」报成「没有版本」。
pub async fn load_prompt_versions(root: &str) -> std::io::Result<Vec<PromptVersionRecord>> {
    let path = prompt_versions_path(root);
    let text = match tokio::fs::read_to_string(&path).await {
        Ok(text) => text,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => String::new(),
        Err(error) => return Err(error),
    };
    Ok(parse_version_lines(&text))
}

/// 追加版本行（best-effort：失败仅日志，不 fail 覆盖写主链）。
///
/// 1666 号：经 [`crate::utils::jsonl_ledger::append_jsonl`]——同一路径的追加
/// 进程内串行（并发追加此前丢行：TS 腿 20 路并发实测只剩 1 行）；读不出来
/// 即中止（既有历史保留）。
pub async fn append_prompt_version(
    root: &str,
    prompt_id: &str,
    action: &str,
    content: &str,
    ts: &str,
) {
    let row = PromptVersionRecord {
        ts: ts.to_string(),
        prompt_id: prompt_id.to_string(),
        action: action.to_string(),
        content: content.to_string(),
    };
    let appended = crate::utils::jsonl_ledger::append_jsonl(
        &prompt_versions_path(root),
        |text| {
            parse_version_lines(text)
                .iter()
                .map(|record| serde_json::to_string(record).unwrap_or_default())
                .collect()
        },
        vec![serde_json::to_string(&row).unwrap_or_default()],
    )
    .await;
    if let Err(error) = appended {
        eprintln!("[prompt-versions] append aborted, existing history kept (ignored): {error}");
    }
}

/// 某 prompt 的版本列表（ts 降序=最新在前）。读不出来 = Err。
pub async fn list_prompt_versions(
    root: &str,
    prompt_id: &str,
) -> std::io::Result<Vec<PromptVersionRecord>> {
    let mut versions: Vec<PromptVersionRecord> = load_prompt_versions(root)
        .await?
        .into_iter()
        .filter(|record| record.prompt_id == prompt_id)
        .collect();
    versions.sort_by(|a, b| b.ts.cmp(&a.ts));
    Ok(versions)
}

/// 读指定版本（promptId+ts 定位；找不到 None）。读不出来 = Err。
pub async fn read_prompt_version(
    root: &str,
    prompt_id: &str,
    ts: &str,
) -> std::io::Result<Option<PromptVersionRecord>> {
    Ok(load_prompt_versions(root)
        .await?
        .into_iter()
        .find(|record| record.prompt_id == prompt_id && record.ts == ts))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::state::store::InMemoryStateStore;

    fn input(id: &str, project: Option<&str>, user: Option<&str>) -> LoadPromptPackPromptInput {
        LoadPromptPackPromptInput {
            prompt_id: id.to_string(),
            project_root: project.map(|s| s.to_string()),
            user_root: user.map(|s| s.to_string()),
        }
    }

    #[test]
    fn prompt_override_path_splits_dotted_id() {
        assert_eq!(
            prompt_override_path("root", "longform.writer"),
            "root/prompt/longform/writer.md".replace('/', std::path::MAIN_SEPARATOR_STR)
        );
        assert_eq!(
            prompt_override_path("root", "interactive-film.story-graph"),
            "root/prompt/interactive-film/story-graph.md"
                .replace('/', std::path::MAIN_SEPARATOR_STR)
        );
    }

    #[test]
    fn prompt_override_path_no_dot_uses_id_as_filename() {
        let p = prompt_override_path("root", "simple");
        assert!(p.ends_with("simple.md"));
    }

    #[test]
    fn normalize_lowercases_and_trims() {
        assert_eq!(
            normalize_prompt_id("  Longform.Writer  "),
            "longform.writer"
        );
    }

    #[tokio::test]
    async fn load_falls_back_to_builtin_when_no_override() {
        let store = InMemoryStateStore::new();
        let loaded = load_prompt_pack_prompt(&store, &input("longform.writer", None, None))
            .await
            .unwrap();
        assert_eq!(loaded.source, PromptSource::Builtin);
        assert!(loaded.content.contains("long-form chapter writer"));
        assert_eq!(loaded.pack_id.as_deref(), Some("longform"));
        assert!(loaded.path.is_none());
    }

    #[tokio::test]
    async fn project_override_takes_precedence_over_user_and_builtin() {
        let store = InMemoryStateStore::new();
        let path = prompt_override_path("project", "longform.writer");
        store.set(&path, "PROJECT OVERRIDE");
        let user_path = prompt_override_path("user", "longform.writer");
        store.set(&user_path, "USER OVERRIDE");

        let loaded = load_prompt_pack_prompt(
            &store,
            &input("longform.writer", Some("project"), Some("user")),
        )
        .await
        .unwrap();
        assert_eq!(loaded.source, PromptSource::Project);
        assert_eq!(loaded.content, "PROJECT OVERRIDE");
    }

    #[tokio::test]
    async fn user_override_takes_precedence_over_builtin() {
        let store = InMemoryStateStore::new();
        let user_path = prompt_override_path("user", "play.mutator");
        store.set(&user_path, "USER VERSION");

        let loaded = load_prompt_pack_prompt(&store, &input("play.mutator", None, Some("user")))
            .await
            .unwrap();
        assert_eq!(loaded.source, PromptSource::User);
        assert_eq!(loaded.content, "USER VERSION");
    }

    #[tokio::test]
    async fn unknown_prompt_id_without_override_is_not_found() {
        let store = InMemoryStateStore::new();
        let err = load_prompt_pack_prompt(&store, &input("nope.nope", None, None))
            .await
            .unwrap_err();
        assert_eq!(err.prompt_id, "nope.nope");
    }

    #[tokio::test]
    async fn append_guidance_concatenates_loaded_prompt() {
        let store = InMemoryStateStore::new();
        let out =
            append_prompt_pack_guidance(&store, "BASE", &input("longform.writer", None, None))
                .await
                .unwrap();
        assert!(
            out.starts_with("BASE\n\n## Prompt Pack Guidance (longform.writer, source: builtin)\n")
        );
    }

    #[tokio::test]
    async fn append_guidance_empty_override_returns_base() {
        let store = InMemoryStateStore::new();
        let path = prompt_override_path("project", "longform.writer");
        store.set(&path, "   "); // 仅空白
        let out = append_prompt_pack_guidance(
            &store,
            "BASE",
            &input("longform.writer", Some("project"), None),
        )
        .await
        .unwrap();
        assert_eq!(out, "BASE", "覆盖内容 trim 后为空 → 原样返回 base");
    }
}
