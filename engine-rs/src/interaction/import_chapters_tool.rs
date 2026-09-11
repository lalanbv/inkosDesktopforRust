//! import_chapters 聊天工具（85 号）。
//!
//! 移植自 `packages/core/src/agent/chapter-import-source.ts`（目录/单文件
//! 源装载）与 `createImportChaptersTool` 的守卫面（bookId 解析 + 既有章节
//! 守卫）；导入全链复用 [`crate::server::book_create_routes::
//! import_chapters_chain`]（Step1 地基 + Step2 逐章回放）。
//! resumeFrom 续放与 importMode=series 已落地（91 号）。

use std::path::{Path, PathBuf};

use serde_json::{json, Value};

use crate::interaction::project_tools::{error_result, ToolResult};
use crate::server::books_routes::BooksRuntime;
use crate::utils::chapter_splitter::{split_chapters, SplitChapter};

/// `resolveToolBookId`：params.bookId ?? activeBookId；缺失 → 固定错误；
/// 显式 bookId 与 active 不一致 → 固定错误。
pub fn resolve_tool_book_id(
    tool_name: &str,
    params_book_id: Option<&str>,
    active_book_id: Option<&str>,
) -> Result<String, String> {
    let resolved = params_book_id.or(active_book_id).map(str::trim).filter(|v| !v.is_empty());
    let Some(book_id) = resolved else {
        return Err(format!("{tool_name} requires bookId when there is no active book."));
    };
    if !crate::interaction::session::is_safe_book_id(book_id) {
        return Err(format!("Invalid {tool_name}.bookId: \"{book_id}\""));
    }
    if let (Some(_), Some(active)) = (params_book_id, active_book_id) {
        if book_id != active.trim() {
            return Err(format!("{tool_name}.bookId must match the active book."));
        }
    }
    Ok(book_id.to_string())
}

/// `loadChaptersFromPath`：目录模式（.md/.txt 按名排序，每文件一章，标题
/// 去扩展名与前导数字前缀）与单文件模式（splitChapters + 自定义正则）。
pub async fn load_chapters_from_path(
    source_path: &Path,
    split_pattern: Option<&str>,
) -> Result<Vec<SplitChapter>, String> {
    if tokio::fs::metadata(source_path)
        .await
        .map(|m| m.is_dir())
        .unwrap_or(false)
    {
        let mut entries: Vec<String> = Vec::new();
        let mut reader = tokio::fs::read_dir(source_path)
            .await
            .map_err(|e| e.to_string())?;
        while let Ok(Some(entry)) = reader.next_entry().await {
            let name = entry.file_name().to_string_lossy().to_string();
            if name.ends_with(".md") || name.ends_with(".txt") {
                entries.push(name);
            }
        }
        entries.sort();
        if entries.is_empty() {
            return Err(format!("No .md or .txt files found in {}.", source_path.display()));
        }
        let mut chapters = Vec::new();
        for name in entries {
            let content = tokio::fs::read_to_string(source_path.join(&name))
                .await
                .map_err(|e| e.to_string())?;
            let title = dir_file_title(&name);
            chapters.push(SplitChapter { title, content });
        }
        return Ok(chapters);
    }
    let text = tokio::fs::read_to_string(source_path)
        .await
        .map_err(|e| format!("read failed: {e}"))?;
    let chapters = split_chapters(&text, split_pattern);
    if chapters.is_empty() {
        return Err(format!(
            "No chapters found in {}. The default pattern matches \"第X章/第X回\" and \"Chapter N\" heading lines. Pass splitPattern with a custom regex if the source uses a different heading style.",
            source_path.display()
        ));
    }
    Ok(chapters)
}

/// 目录文件名 → 章节标题（去扩展名 + 前导数字前缀 `03_风暴` → `风暴`）。
fn dir_file_title(name: &str) -> String {
    let base = name
        .strip_suffix(".md")
        .or_else(|| name.strip_suffix(".txt"))
        .unwrap_or(name);
    static NUMERIC_PREFIX: std::sync::OnceLock<regex::Regex> = std::sync::OnceLock::new();
    let re = NUMERIC_PREFIX.get_or_init(|| regex::Regex::new(r"^\d+[_\-\s ]*").unwrap());
    re.replace(base, "").to_string()
}

fn text_result(text: impl Into<String>, details: Option<Value>) -> ToolResult {
    ToolResult { text: text.into(), details, is_error: false }
}

/// `import_chapters` 执行器：守卫 → 源装载 → 导入全链（abort：链内安全点
/// 中止——TS runPipelineWithAbortSignal(signal, () => pipeline.importChapters)
/// 等价，102 号）。
pub async fn tool_import_chapters(
    runtime: &BooksRuntime,
    project_root: &Path,
    active_book_id: Option<&str>,
    args: &Value,
    abort: Option<&crate::interaction::agent_loop::AbortHandle>,
) -> ToolResult {
    let params_book_id = args
        .get("bookId")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|v| !v.is_empty());
    let book_id = match resolve_tool_book_id("import_chapters", params_book_id, active_book_id) {
        Ok(book_id) => book_id,
        Err(message) => return error_result(message),
    };
    // 既有章节守卫：有章且无 resumeFrom → 固定错误（TS 逐字）。
    let resume_from = args
        .get("resumeFrom")
        .and_then(Value::as_f64)
        .filter(|v| v.is_finite() && *v >= 1.0 && v.fract() == 0.0)
        .map(|v| v as u32);
    let existing = runtime
        .state
        .get_next_chapter_number(&book_id)
        .await
        .unwrap_or(1)
        .saturating_sub(1);
    if existing > 0 && resume_from.is_none() {
        return error_result(format!(
            "Book \"{book_id}\" already has {existing} chapter(s). {}. Pass resumeFrom=<n> to resume/append from chapter n, or ask the user to clear the existing chapters first.",
            crate::utils::resume_advice::format_resume_hint(i64::from(existing), Some("en"))
        ));
    }
    // importMode：continuation（缺省）/ series（地基生成模式直通）。
    let import_mode = match args.get("importMode").and_then(Value::as_str) {
        Some("series") => crate::agents::architect::ImportMode::Series,
        _ => crate::agents::architect::ImportMode::Continuation,
    };
    let Some(source_path_text) = args
        .get("sourcePath")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|v| !v.is_empty())
    else {
        return error_result("import_chapters requires a sourcePath argument".to_string());
    };
    let source_path: PathBuf = if Path::new(source_path_text).is_absolute() {
        PathBuf::from(source_path_text)
    } else {
        project_root.join(source_path_text)
    };
    let split_pattern = args
        .get("splitPattern")
        .and_then(Value::as_str)
        .map(str::to_string);
    let chapters = match load_chapters_from_path(&source_path, split_pattern.as_deref()).await {
        Ok(chapters) => chapters,
        Err(message) => return error_result(message),
    };
    let result = crate::server::book_create_routes::import_chapters_chain_with_resume(
        runtime,
        &book_id,
        &chapters,
        resume_from.unwrap_or(1),
        import_mode,
        abort,
    )
    .await;
    let result = match result {
        Ok(result) => result,
        Err(message) => return error_result(message),
    };
    let imported_count = result.get("importedCount").cloned().unwrap_or(json!(0));
    let total_words = result.get("totalWords").cloned().unwrap_or(json!(0));
    let next_chapter = result.get("nextChapter").cloned().unwrap_or(json!(1));
    text_result(
        [
            format!("Imported {} chapter(s) into book \"{}\".", imported_count.as_u64().unwrap_or(0), book_id),
            format!(
                "Total imported length: {}. Next chapter to write: {}.",
                total_words.as_u64().unwrap_or(0),
                next_chapter.as_u64().unwrap_or(1)
            ),
            if resume_from.unwrap_or(1) == 1 {
                "Foundation and truth files were reverse-engineered from the imported text; chapter files and the chapter index were rebuilt by sequential replay.".to_string()
            } else {
                format!("Resumed replay from chapter {}; earlier chapters and the existing foundation were kept.", resume_from.unwrap_or(1))
            },
            "The book can now be continued with sub_agent(agent=\"writer\") in the book session.".to_string(),
        ]
        .join("\n"),
        Some(json!({
            "kind": "chapters_imported",
            "bookId": book_id,
            "importedCount": imported_count,
            "totalWords": total_words,
            "nextChapter": next_chapter,
            "importMode": if matches!(import_mode, crate::agents::architect::ImportMode::Series) { "series" } else { "continuation" },
        })),
    )
}

/// `import_chapters` schema（ImportChaptersParams 逐字）。
pub fn import_chapters_schema() -> Value {
    json!({
        "type": "function",
        "function": {
            "name": "import_chapters",
            "description": "Import an existing novel's chapters from a local file or directory into an InkOS book as real chapters (not reference material). InkOS reverse-engineers foundation/truth files from the imported text and replays every chapter to rebuild story state, so the book can be continued afterwards. Use ingest_material instead when the user only wants to archive reference material without touching book chapters.",
            "parameters": {
                "type": "object",
                "properties": {
                    "bookId": {
                        "type": "string",
                        "description": "Target book ID to import into. In active-book sessions, omit it to use the current active book; if provided, it must match the active book. In general chat there is no active book, so it is required and must be an existing book.",
                    },
                    "sourcePath": {
                        "type": "string",
                        "description": "Local path of the chapter source: either the stored_path from the Uploaded Files block (project-relative, e.g. .inkos/uploads/<session>/novel.txt) or an absolute path on this machine that the user provided. A directory imports each .md/.txt file as one chapter in filename order; a single file is split into chapters automatically by heading lines.",
                    },
                    "splitPattern": {
                        "type": "string",
                        "description": "Single-file mode only: custom JavaScript regex source matching chapter heading lines. Omit to use the default pattern, which matches \"第X章/第X回\" and \"Chapter N\" headings.",
                    },
                    "resumeFrom": {
                        "type": "number",
                        "description": "Resume an interrupted import from chapter N (1-based). Required when the book already has chapters: replay starts at chapter N and earlier chapters are kept. Omit for a fresh import into an empty book.",
                    },
                    "importMode": {
                        "type": "string",
                        "enum": ["continuation", "series"],
                        "description": "continuation (default): the book picks up exactly where the imported text left off, no new spacetime. series: shared universe but an independent new story, so a new spacetime is generated.",
                    },
                },
                "required": ["sourcePath"],
            },
        },
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resolve_tool_book_id_rules() {
        assert_eq!(
            resolve_tool_book_id("import_chapters", Some(" b1 "), Some("b1")).unwrap(),
            "b1"
        );
        assert_eq!(resolve_tool_book_id("import_chapters", None, Some("b1")).unwrap(), "b1");
        assert_eq!(resolve_tool_book_id("import_chapters", Some("b1"), None).unwrap(), "b1");
        let missing = resolve_tool_book_id("import_chapters", None, None).unwrap_err();
        assert_eq!(missing, "import_chapters requires bookId when there is no active book.");
        let mismatch = resolve_tool_book_id("import_chapters", Some("b2"), Some("b1")).unwrap_err();
        assert_eq!(mismatch, "import_chapters.bookId must match the active book.");
        let unsafe_id = resolve_tool_book_id("import_chapters", Some("../evil"), None).unwrap_err();
        assert!(unsafe_id.contains("Invalid import_chapters.bookId"), "{unsafe_id}");
    }

    #[test]
    fn dir_file_title_strips_extension_and_numeric_prefix() {
        assert_eq!(dir_file_title("03_风暴.md"), "风暴");
        assert_eq!(dir_file_title("12-夜雨.txt"), "夜雨");
        assert_eq!(dir_file_title("7 长街.md"), "长街");
        assert_eq!(dir_file_title("序章.md"), "序章");
    }

    #[tokio::test]
    async fn load_chapters_directory_and_single_file() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        // 目录模式：文件名排序 + 标题清洗。
        let source = root.join("chapters_src");
        std::fs::create_dir_all(&source).unwrap();
        std::fs::write(source.join("02_云涌.md"), "坊市喧闹。").unwrap();
        std::fs::write(source.join("01_风起.md"), "林动睁眼。").unwrap();
        std::fs::write(source.join("notes.json"), "{}").unwrap();
        let chapters = load_chapters_from_path(&source, None).await.unwrap();
        assert_eq!(chapters.len(), 2, "仅 .md/.txt");
        assert_eq!(chapters[0].title, "风起", "文件名排序");
        assert_eq!(chapters[1].title, "云涌");
        assert_eq!(chapters[1].content, "坊市喧闹。");
        // 空目录错误。
        let empty = root.join("empty_src");
        std::fs::create_dir_all(&empty).unwrap();
        let error = load_chapters_from_path(&empty, None).await.unwrap_err();
        assert!(error.starts_with("No .md or .txt files found in"), "{error}");
        // 单文件：默认切分 + 无章错误。
        std::fs::write(root.join("novel.txt"), "# 第一章 风起\n\n正文一。\n\n# 第二章 云涌\n\n正文二。").unwrap();
        let single = load_chapters_from_path(&root.join("novel.txt"), None).await.unwrap();
        assert_eq!(single.len(), 2);
        assert_eq!(single[0].title, "风起");
        std::fs::write(root.join("plain.txt"), "没有任何章节标题的散文。").unwrap();
        let no_chapters = load_chapters_from_path(&root.join("plain.txt"), None).await.unwrap_err();
        assert!(no_chapters.contains("No chapters found in"), "{no_chapters}");
        assert!(no_chapters.contains("第X章/第X回"), "{no_chapters}");
        // 缺失文件。
        let missing = load_chapters_from_path(&root.join("ghost.txt"), None).await.unwrap_err();
        assert!(missing.contains("read failed"), "{missing}");
    }
}
