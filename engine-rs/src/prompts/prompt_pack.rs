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
//
// 1666 号不变量：行的 (action, content) 唯一决定「回滚到该行」的结果——
// action 即对覆盖文件的效果：put(c)=写、restore(c)=按历史内容写、delete=移除
// （回 builtin）。回滚到 delete 版的效果是移除，故新行记 delete 而非
// `restore`+空内容（后者与「回滚到空 put 版」同形，再回滚会写入空覆盖；
// 空覆盖是合法状态，不能拿空内容当「回 builtin」的判据）。修复前账本里
// 已有的 `restore`+空内容行仍按「空覆盖」回滚——如遇，对该 prompt 再点一次
// 「恢复 builtin」即可。

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

/// 版本以 (promptId, ts) 定位——同一 prompt 不得出现同 ts 两行。同一毫秒内的
/// 两次写入（并发保存 / 回滚后紧接保存）会让后一行被前一行遮蔽：读取只认
/// 第一条、列表里同 ts 的两行「旧的排在前」。撞键时把新行的 ts 顺推 1ms 直到
/// 不撞（顺推后的顺序 = 写入顺序）。非 ISO 毫秒形态的 ts 原样写入（无法顺推）。
fn unique_version_ts(existing: &[PromptVersionRecord], prompt_id: &str, ts: &str) -> String {
    let Some(mut millis) = crate::utils::utc_time::utc_iso_to_unix_millis(ts) else {
        return ts.to_string();
    };
    let mut candidate = ts.to_string();
    while existing
        .iter()
        .any(|record| record.prompt_id == prompt_id && record.ts == candidate)
    {
        millis += 1;
        candidate = crate::utils::utc_time::unix_millis_to_utc_iso(millis);
    }
    candidate
}

/// 追加版本行（best-effort：失败仅日志，不 fail 覆盖写主链）。
///
/// 1666 号：经 [`crate::utils::jsonl_ledger::append_jsonl_derived`]——同一路径
/// 的追加进程内串行（并发追加此前丢行：TS 腿 20 路并发实测只剩 1 行），且新行
/// 在串行段内据最新账本定 ts（见 [`unique_version_ts`]）；读不出来即中止
/// （既有历史保留）。
pub async fn append_prompt_version(
    root: &str,
    prompt_id: &str,
    action: &str,
    content: &str,
    ts: &str,
) {
    let appended = crate::utils::jsonl_ledger::append_jsonl_derived(
        &prompt_versions_path(root),
        |text| {
            parse_version_lines(text)
                .iter()
                .map(|record| serde_json::to_string(record).unwrap_or_default())
                .collect()
        },
        |text| {
            let existing = parse_version_lines(text);
            let row = PromptVersionRecord {
                ts: unique_version_ts(&existing, prompt_id, ts),
                prompt_id: prompt_id.to_string(),
                action: action.to_string(),
                content: content.to_string(),
            };
            vec![serde_json::to_string(&row).unwrap_or_default()]
        },
    )
    .await;
    if let Err(error) = appended {
        eprintln!("[prompt-versions] append aborted, existing history kept (ignored): {error}");
    }
}

/// 某 prompt 的版本列表（ts 降序=最新在前；同 ts 的历史行按写入序倒排）。
/// 读不出来 = Err。
pub async fn list_prompt_versions(
    root: &str,
    prompt_id: &str,
) -> std::io::Result<Vec<PromptVersionRecord>> {
    let mut versions: Vec<(usize, PromptVersionRecord)> = load_prompt_versions(root)
        .await?
        .into_iter()
        .enumerate()
        .filter(|(_, record)| record.prompt_id == prompt_id)
        .collect();
    versions.sort_by(|(a_index, a), (b_index, b)| b.ts.cmp(&a.ts).then(b_index.cmp(a_index)));
    Ok(versions.into_iter().map(|(_, record)| record).collect())
}

/// 读指定版本（promptId+ts 定位；找不到 None；同 ts 的历史行取最后写入的）。
/// 读不出来 = Err。
pub async fn read_prompt_version(
    root: &str,
    prompt_id: &str,
    ts: &str,
) -> std::io::Result<Option<PromptVersionRecord>> {
    Ok(load_prompt_versions(root)
        .await?
        .into_iter()
        .rev()
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

    /// 1666 号：(promptId, ts) 是版本定位键——同一 prompt 同一毫秒写入两行时
    /// 新行 ts 顺推 1ms，两行各可按 ts 取到、列表「最新在前」；别的 prompt 的同
    /// ts 不受影响。
    #[tokio::test]
    async fn same_millisecond_versions_get_distinct_keys() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().to_string_lossy().into_owned();
        let ts = "2026-11-24T12:00:00.000Z";
        let bumped = "2026-11-24T12:00:00.001Z";
        append_prompt_version(&root, "p.one", "put", "第一版", ts).await;
        append_prompt_version(&root, "p.one", "put", "第二版", ts).await;
        append_prompt_version(&root, "p.two", "put", "别的", ts).await;

        let versions = list_prompt_versions(&root, "p.one").await.unwrap();
        assert_eq!(
            versions.iter().map(|v| v.ts.as_str()).collect::<Vec<_>>(),
            [bumped, ts],
            "最新在前"
        );
        for (key, content) in [(ts, "第一版"), (bumped, "第二版")] {
            assert_eq!(
                read_prompt_version(&root, "p.one", key)
                    .await
                    .unwrap()
                    .map(|r| r.content),
                Some(content.to_string()),
                "两行各可按 ts 取到"
            );
        }
        assert_eq!(
            read_prompt_version(&root, "p.two", ts)
                .await
                .unwrap()
                .map(|r| r.content),
            Some("别的".to_string())
        );
        // 非 ISO 毫秒形态的 ts 无法顺推：原样写入（不改写调用方给的键）。
        append_prompt_version(&root, "p.three", "put", "任意键", "v1").await;
        assert_eq!(
            load_prompt_versions(&root)
                .await
                .unwrap()
                .last()
                .map(|r| r.ts.clone()),
            Some("v1".to_string())
        );
    }

    /// 1666 号：修复前落盘的同 ts 两行（撞键历史数据）——列表按写入序倒排、
    /// 读取取最后写入的那行（与「最新在前」同一口径）。
    #[tokio::test]
    async fn legacy_tied_versions_read_latest_first() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        tokio::fs::create_dir_all(root.join("prompt"))
            .await
            .unwrap();
        let ts = "2026-11-24T12:00:00.000Z";
        let rows = [("旧的", ts), ("新的", ts)]
            .map(|(content, ts)| {
                serde_json::to_string(&PromptVersionRecord {
                    ts: ts.to_string(),
                    prompt_id: "p.one".to_string(),
                    action: "put".to_string(),
                    content: content.to_string(),
                })
                .unwrap()
            })
            .join("\n");
        tokio::fs::write(
            prompt_versions_path(&root.to_string_lossy()),
            format!("{rows}\n"),
        )
        .await
        .unwrap();
        let root = root.to_string_lossy().into_owned();
        let versions = list_prompt_versions(&root, "p.one").await.unwrap();
        assert_eq!(
            versions
                .iter()
                .map(|v| v.content.as_str())
                .collect::<Vec<_>>(),
            ["新的", "旧的"]
        );
        assert_eq!(
            read_prompt_version(&root, "p.one", ts)
                .await
                .unwrap()
                .map(|r| r.content),
            Some("新的".to_string())
        );
    }
}
