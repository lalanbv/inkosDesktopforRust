//! 规则读取链（rules-reader）。
//!
//! 移植自 `packages/core/src/agents/rules-reader.ts`（143 行）。ContinuityAuditor 等
//! agent 的规则面数据入口：
//! - [`read_genre_profile`]：题材画像三级查找（项目级 → 内置 → other.md 兜底）
//! - [`list_available_genres`]：项目级 + 内置题材清单（项目级覆盖同 id，按 id 排序）
//! - [`read_book_rules`]：story/book_rules.md → story/outline/story_frame.md frontmatter
//!   （legacy 回退）→ None（shim / 全缺失）
//! - [`read_book_language`]：book.json 的 language 字段（zh/en）
//!
//! ## 与 TS 的差异
//! - 内置 genres 目录：TS 经 `import.meta.url` 定死 `packages/core/genres`；Rust 作为
//!   参数注入（`builtin_genres_dir`），由调用方（src-tauri / axum 服务）决定路径。
//! - `console.warn` → `tracing::warn!`；`getBuiltinGenresDir()` 不再单独暴露。
//! - TS `tryReadFile` 吞掉一切读错误（含编码错误）返回 null——本模块 `try_read_file`
//!   同语义：任何失败 = 文件不存在。

use std::path::Path;

use serde::Serialize;
use tokio::fs;
#[cfg(feature = "export-bindings")]
use ts_rs::TS;

use crate::models::book_rules::{
    parse_book_rules, try_parse_book_rules_frontmatter, BookRulesFrontmatterError, ParsedBookRules,
};
use crate::models::genre_profile::{
    parse_genre_profile, GenreProfileParseError, ParsedGenreProfile,
};

/// 读取失败（三级查找全 miss）。对齐 TS 抛出的
/// `Genre profile not found for "{id}" and fallback "other.md" is missing`。
#[derive(Debug, thiserror::Error)]
pub enum ReadGenreProfileError {
    #[error("Genre profile not found for \"{genre_id}\" and fallback \"other.md\" is missing")]
    NotFound { genre_id: String },
    #[error(transparent)]
    Parse(#[from] GenreProfileParseError),
}

/// 题材来源。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[cfg_attr(feature = "export-bindings", derive(TS))]
#[cfg_attr(feature = "export-bindings", ts(export))]
#[serde(rename_all = "lowercase")]
pub enum GenreSource {
    Project,
    Builtin,
}

/// 可用题材条目。对齐 TS `listAvailableGenres` 的返回元素。
#[derive(Debug, Clone, PartialEq, Serialize)]
#[cfg_attr(feature = "export-bindings", derive(TS))]
#[cfg_attr(feature = "export-bindings", ts(export))]
pub struct GenreInfo {
    pub id: String,
    pub name: String,
    pub source: GenreSource,
}

/// 读文件，任何失败（不存在 / IO / 编码）→ None。对齐 TS `tryReadFile`。
async fn try_read_file(path: &Path) -> Option<String> {
    fs::read_to_string(path).await.ok()
}

/// 加载题材画像。查找顺序：项目级 `{projectRoot}/genres/{genreId}.md` →
/// `{builtinDir}/{genreId}.md` → `{builtinDir}/other.md`。对齐 TS `readGenreProfile`。
pub async fn read_genre_profile(
    project_root: &Path,
    genre_id: &str,
    builtin_genres_dir: &Path,
) -> Result<ParsedGenreProfile, ReadGenreProfileError> {
    let project_path = project_root.join("genres").join(format!("{genre_id}.md"));
    let builtin_path = builtin_genres_dir.join(format!("{genre_id}.md"));
    let fallback_path = builtin_genres_dir.join("other.md");

    let raw = match try_read_file(&project_path).await {
        Some(s) => Some(s),
        None => match try_read_file(&builtin_path).await {
            Some(s) => Some(s),
            None => try_read_file(&fallback_path).await,
        },
    };
    let raw = raw.ok_or(ReadGenreProfileError::NotFound {
        genre_id: genre_id.to_string(),
    })?;
    Ok(parse_genre_profile(&raw)?)
}

/// 列出全部可用题材（内置先入表，项目级覆盖同 id；按 id 排序）。
/// 对齐 TS `listAvailableGenres`——**含其容错语义**：单阶段（内置/项目）中任一文件
/// 解析失败会中断该阶段剩余文件（TS 的 try/catch 包住整个循环），已入表条目保留。
pub async fn list_available_genres(
    project_root: &Path,
    builtin_genres_dir: &Path,
) -> Vec<GenreInfo> {
    let mut results: Vec<GenreInfo> = Vec::new();

    // 内置题材先入表（吞错：无目录 / 中途解析失败 → 阶段中断）。
    scan_genre_dir(builtin_genres_dir, GenreSource::Builtin, &mut results).await;

    // 项目级题材覆盖同 id 条目。
    scan_genre_dir(
        &project_root.join("genres"),
        GenreSource::Project,
        &mut results,
    )
    .await;

    results.sort_by(|a, b| a.id.cmp(&b.id));
    results
}

/// 扫描一个 genres 目录并入表（同 id 覆盖）。返回 false = 阶段中断（对齐 TS try/catch）。
async fn scan_genre_dir(dir: &Path, source: GenreSource, results: &mut Vec<GenreInfo>) -> bool {
    let mut entries = match fs::read_dir(dir).await {
        Ok(entries) => entries,
        Err(_) => return false, // 无该目录（TS：catch 整个 try 块）
    };
    loop {
        let entry = match entries.next_entry().await {
            Ok(Some(entry)) => entry,
            Ok(None) => return true,
            Err(_) => return false,
        };
        let file_name = entry.file_name();
        let Some(name) = file_name.to_str() else {
            continue;
        };
        let Some(id) = name.strip_suffix(".md") else {
            continue;
        };
        let Some(raw) = try_read_file(&entry.path()).await else {
            continue;
        };
        // 解析失败 → 中断本阶段（对齐 TS 循环内 throw 被外层 catch 吞掉）。
        let Ok(parsed) = parse_genre_profile(&raw) else {
            return false;
        };
        upsert_genre(
            results,
            GenreInfo {
                id: id.to_string(),
                name: parsed.profile.name,
                source,
            },
        );
    }
}

/// 同 id 覆盖入表（对齐 TS Map.set 语义）。
fn upsert_genre(results: &mut Vec<GenreInfo>, info: GenreInfo) {
    if let Some(existing) = results.iter_mut().find(|g| g.id == info.id) {
        *existing = info;
    } else {
        results.push(info);
    }
}

/// 加载结构化书籍规则。对齐 TS `readBookRules`：
/// 1. `story/book_rules.md`（新布局：普通 markdown / frontmatter；shim → 跳过）
/// 2. `story/outline/story_frame.md` 的 frontmatter（legacy 回退；坏 YAML → warn 后继续）
/// 3. 全缺失 / 仅 shim → None
pub async fn read_book_rules(book_dir: &Path) -> Option<ParsedBookRules> {
    let rules_path = book_dir.join("story").join("book_rules.md");
    let rules_raw = try_read_file(&rules_path).await;
    if let Some(raw) = rules_raw.as_deref().filter(|s| !s.is_empty()) {
        if let Some(parsed) = parse_book_rules(raw) {
            return Some(parsed);
        }
    }

    let story_frame_path = book_dir
        .join("story")
        .join("outline")
        .join("story_frame.md");
    let story_frame_raw = try_read_file(&story_frame_path).await;
    if let Some(raw) = story_frame_raw.as_deref().filter(|s| !s.is_empty()) {
        // 仅取开头的 `---\n...\n---` 块；其后的大纲正文不得泄入 ParsedBookRules.body。
        if let Some(block) = story_frame_frontmatter_block(raw) {
            match try_parse_book_rules_frontmatter(block) {
                Ok(parsed) => return Some(parsed),
                Err(BookRulesFrontmatterError::Invalid(err)) => {
                    // Phase 5 hotfix 3：坏 frontmatter 不静默清零规则，回退 legacy。
                    tracing::warn!(
                        "[rules-reader] story_frame.md frontmatter is malformed at {} — falling back to legacy book_rules.md. Error: {}",
                        story_frame_path.display(),
                        err
                    );
                }
                Err(BookRulesFrontmatterError::NoFrontmatter) => {
                    // 提取出的块必含 frontmatter，理论不可达。
                }
            }
        }
    }

    if rules_raw.as_deref().is_some_and(|s| !s.is_empty()) {
        tracing::warn!(
            "[rules-reader] book_rules.md at {} is a compat shim and no legacy story_frame frontmatter was parseable — returning null instead of silently zeroing out rules.",
            rules_path.display()
        );
    }
    None
}

/// story_frame 开头 frontmatter 块。逐字移植 TS
/// `^\s*(---\s*\n[\s\S]*?\n---\s*)(?:\n|$)`（含闭合 `---`）。
fn story_frame_frontmatter_block(raw: &str) -> Option<&str> {
    use regex::Regex;
    use std::sync::OnceLock;
    static R: OnceLock<Regex> = OnceLock::new();
    R.get_or_init(|| Regex::new(r"^\s*(---\s*\n[\s\S]*?\n---\s*)(?:\n|$)").unwrap())
        .captures(raw)
        .map(|c| c.get(1).expect("组 1 必在").as_str())
}

/// 读 book.json 的 language（zh/en）。对齐 TS `readBookLanguage`
/// （`BookConfigSchema.pick({language})`——只校验 language，其余字段坏不影响）。
pub async fn read_book_language(book_dir: &Path) -> Option<String> {
    let raw = try_read_file(&book_dir.join("book.json")).await?;
    let value: serde_json::Value = serde_json::from_str(&raw).ok()?;
    match value.get("language")?.as_str()? {
        "zh" => Some("zh".to_string()),
        "en" => Some("en".to_string()),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 建 genres fixtures 目录，返回 (TempDir 保持存活, project_root, builtin_dir)。
    async fn fixture() -> (tempfile::TempDir, std::path::PathBuf, std::path::PathBuf) {
        let dir = tempfile::tempdir().expect("临时目录");
        let project = dir.path().join("project");
        let builtin = dir.path().join("builtin");
        tokio::fs::create_dir_all(project.join("genres"))
            .await
            .expect("建 project genres");
        tokio::fs::create_dir_all(&builtin)
            .await
            .expect("建 builtin");
        (dir, project, builtin)
    }

    async fn write(path: &Path, content: &str) {
        if let Some(parent) = path.parent() {
            tokio::fs::create_dir_all(parent).await.expect("建父目录");
        }
        tokio::fs::write(path, content).await.expect("写文件");
    }

    const GENRE_MD: &str = "---\nname: 仙侠\nid: xianxia\nchapterTypes: [\"推进章\"]\nfatigueWords: [\"震惊\"]\n---\n\n正文指导\n";

    #[tokio::test]
    async fn read_genre_profile_prefers_project_level() {
        let (_tmp, project, builtin) = fixture().await;
        write(
            &project.join("genres/my.md"),
            "---\nname: 项目版\nid: my\nchapterTypes: []\nfatigueWords: []\n---\nb",
        )
        .await;
        write(
            &builtin.join("my.md"),
            "---\nname: 内置版\nid: my\nchapterTypes: []\nfatigueWords: []\n---\nb",
        )
        .await;

        let parsed = read_genre_profile(&project, "my", &builtin)
            .await
            .expect("应命中项目级");
        assert_eq!(parsed.profile.name, "项目版");
    }

    #[tokio::test]
    async fn read_genre_profile_falls_back_to_builtin_then_other() {
        let (_tmp, project, builtin) = fixture().await;
        // 项目级无 → 内置命中。
        write(&builtin.join("xianxia.md"), GENRE_MD).await;
        let parsed = read_genre_profile(&project, "xianxia", &builtin)
            .await
            .expect("应命中内置");
        assert_eq!(parsed.profile.name, "仙侠");

        // 内置也无 → other.md 兜底。
        write(
            &builtin.join("other.md"),
            "---\nname: 通用\nid: other\nchapterTypes: []\nfatigueWords: []\n---\nb",
        )
        .await;
        let parsed = read_genre_profile(&project, "nope", &builtin)
            .await
            .expect("应命中 other 兜底");
        assert_eq!(parsed.profile.name, "通用");
    }

    #[tokio::test]
    async fn read_genre_profile_not_found_errors() {
        let (_tmp, project, builtin) = fixture().await;
        let err = read_genre_profile(&project, "ghost", &builtin)
            .await
            .expect_err("全 miss 应报错");
        assert!(
            err.to_string().contains("ghost"),
            "错误信息应含 genre id：{err}"
        );
    }

    #[tokio::test]
    async fn list_available_genres_merges_and_sorts() {
        let (_tmp, project, builtin) = fixture().await;
        write(
            &builtin.join("zeta.md"),
            "---\nname: Z\nid: zeta\nchapterTypes: []\nfatigueWords: []\n---\nb",
        )
        .await;
        write(
            &builtin.join("alpha.md"),
            "---\nname: A\nid: alpha\nchapterTypes: []\nfatigueWords: []\n---\nb",
        )
        .await;
        write(&builtin.join("skip.txt"), "非 md").await;
        // 项目级覆盖 alpha。
        write(
            &project.join("genres/alpha.md"),
            "---\nname: 项目A\nid: alpha\nchapterTypes: []\nfatigueWords: []\n---\nb",
        )
        .await;

        let list = list_available_genres(&project, &builtin).await;
        let ids: Vec<&str> = list.iter().map(|g| g.id.as_str()).collect();
        assert_eq!(ids, vec!["alpha", "zeta"]);
        let alpha = &list[0];
        assert_eq!(alpha.name, "项目A");
        assert_eq!(alpha.source, GenreSource::Project);
        assert_eq!(list[1].source, GenreSource::Builtin);
    }

    #[tokio::test]
    async fn read_book_rules_prefers_book_rules_md() {
        let (_tmp, project, _builtin) = fixture().await;
        let book = project.join("book");
        write(
            &book.join("story/book_rules.md"),
            "---\nprohibitions: [新规则]\n---\n规则正文",
        )
        .await;
        write(
            &book.join("story/outline/story_frame.md"),
            "---\nprohibitions: [旧规则]\n---\n大纲",
        )
        .await;

        let parsed = read_book_rules(&book).await.expect("应命中 book_rules.md");
        assert_eq!(parsed.rules.prohibitions, vec!["新规则".to_string()]);
        assert_eq!(parsed.body, "规则正文");
    }

    #[tokio::test]
    async fn read_book_rules_falls_back_to_story_frame_frontmatter() {
        let (_tmp, project, _builtin) = fixture().await;
        let book = project.join("book");
        // shim 指针 → 跳过；story_frame frontmatter 承接。
        write(
            &book.join("story/book_rules.md"),
            "# 本书规则（兼容指针——已废弃）\n本文件仅为外部读取保留",
        )
        .await;
        write(
            &book.join("story/outline/story_frame.md"),
            "---\nprohibitions: [旧规则]\nprotagonist:\n  name: 林动\n---\n\n大纲正文不泄入 body",
        )
        .await;

        let parsed = read_book_rules(&book).await.expect("应回退 story_frame");
        assert_eq!(parsed.rules.prohibitions, vec!["旧规则".to_string()]);
        assert_eq!(parsed.rules.protagonist.as_ref().unwrap().name, "林动");
        // 仅 frontmatter 块被解析；其后大纲正文不得泄入 body。
        assert_eq!(parsed.body, "");
    }

    #[tokio::test]
    async fn read_book_rules_returns_none_when_all_missing() {
        let (_tmp, project, _builtin) = fixture().await;
        let book = project.join("book");
        assert!(read_book_rules(&book).await.is_none());
    }

    #[tokio::test]
    async fn read_book_rules_malformed_story_frame_warns_and_returns_none() {
        let (_tmp, project, _builtin) = fixture().await;
        let book = project.join("book");
        // 仅 shim + 坏 frontmatter（fanficMode 非法 → Invalid）→ None。
        write(
            &book.join("story/book_rules.md"),
            "# 本书规则（兼容指针——已废弃）",
        )
        .await;
        write(
            &book.join("story/outline/story_frame.md"),
            "---\nfanficMode: bogus\n---\n大纲",
        )
        .await;
        assert!(read_book_rules(&book).await.is_none());
    }

    #[tokio::test]
    async fn read_book_language_extracts_language_only() {
        let (_tmp, project, _builtin) = fixture().await;
        let book = project.join("book");
        write(
            &book.join("book.json"),
            r#"{"id":"b1","title":"t","language":"en"}"#,
        )
        .await;
        assert_eq!(read_book_language(&book).await.as_deref(), Some("en"));

        // 非法 language（safeParse fail）→ None；其余字段坏也不影响 pick 语义。
        write(&book.join("book.json"), r#"{"language":"fr"}"#).await;
        assert_eq!(read_book_language(&book).await, None);
        write(
            &book.join("book.json"),
            r#"{"language":"zh","platform":123}"#,
        )
        .await;
        assert_eq!(read_book_language(&book).await.as_deref(), Some("zh"));
        // 坏 JSON / 缺文件 → None。
        write(&book.join("book.json"), "not json").await;
        assert_eq!(read_book_language(&book).await, None);
    }
}
