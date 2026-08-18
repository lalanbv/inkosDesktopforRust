//! 预测上下文构建（context-builder.ts）。
//!
//! 正史的只读视图（forecast 输入）。全部操作零副作用：构建预测上下文
//! 永不创建或修复正史文件（故不走会种默认值的 StateManager 控制文档面）。

use std::path::Path;

use sha2::{Digest, Sha256};

use crate::agents::planner_context::{format_recent_summaries, read_subplot_board};
use crate::utils::outline_paths::{read_character_context, read_story_frame, read_volume_map};

const RECENT_SUMMARY_LIMIT: usize = 8;

#[derive(Debug, Clone, Default, PartialEq)]
pub struct ForecastContextSections {
    pub author_intent: String,
    pub current_focus: String,
    pub current_state: String,
    pub pending_hooks: String,
    pub story_frame: String,
    pub volume_map: String,
    pub recent_chapter_summaries: String,
    pub character_context: String,
    pub subplot_board: String,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ForecastContext {
    pub book_id: String,
    pub book_title: String,
    pub language: String,
    pub base_chapter: u32,
    pub context_fingerprint: String,
    pub sections: ForecastContextSections,
}

pub async fn build_forecast_context(book_dir: &Path, book_id: &str) -> ForecastContext {
    let story_dir = book_dir.join("story");

    let book_config = read_book_config(book_dir).await;
    let base_chapter = resolve_base_chapter(book_dir).await;
    let fingerprint_files = collect_fingerprint_files(book_dir).await;

    let author_intent = read_or_empty(&story_dir.join("author_intent.md")).await;
    let current_focus = read_or_empty(&story_dir.join("current_focus.md")).await;
    let current_state = read_or_empty(&story_dir.join("current_state.md")).await;
    let pending_hooks = read_or_empty(&story_dir.join("pending_hooks.md")).await;

    let story_frame = read_story_frame(book_dir, "").await;
    let volume_map = read_volume_map(book_dir, "").await;
    let character_context = read_character_context(book_dir, "").await;
    let subplot_board = read_subplot_board(&story_dir).await;
    let chapter_summaries_raw = read_or_empty(&story_dir.join("chapter_summaries.md")).await;

    let context_fingerprint = compute_context_fingerprint(base_chapter, &fingerprint_files);

    let recent_chapter_summaries = if chapter_summaries_raw.trim().is_empty() {
        String::new()
    } else {
        format_recent_summaries(&chapter_summaries_raw, base_chapter + 1, RECENT_SUMMARY_LIMIT)
    };

    ForecastContext {
        book_id: book_id.to_string(),
        book_title: if book_config.0.is_empty() { book_id.to_string() } else { book_config.0 },
        language: book_config.1,
        base_chapter,
        context_fingerprint,
        sections: ForecastContextSections {
            author_intent,
            current_focus,
            current_state,
            pending_hooks,
            story_frame,
            volume_map,
            recent_chapter_summaries,
            character_context,
            subplot_board,
        },
    }
}

/// 正史预测输入的内容哈希（章数 + 送入提示的每个文件）。刻意无 mtime、
/// 顺序无关——副本/检出/CI 对相同正史产出相同指纹。
pub fn compute_context_fingerprint(base_chapter: u32, files: &[(String, String)]) -> String {
    let canonical = super::store::fingerprint_canonical_json(base_chapter, files);
    let mut hasher = Sha256::new();
    hasher.update(canonical.as_bytes());
    format!("{:x}", hasher.finalize())
}

/// buildForecastContext（直接或经助手）读入提示的固定路径全集，含
/// readStoryFrame/readVolumeMap/readCharacterContext 可能解析到的 legacy
/// 回退。story/runtime/**（预测自身所在）刻意不在列——预测不会失效自己。
const FINGERPRINT_FIXED_INPUTS: &[&str] = &[
    "book.json",
    "story/author_intent.md",
    "story/chapter_summaries.md",
    "story/character_matrix.md",
    "story/current_focus.md",
    "story/current_state.md",
    "story/outline/story_frame.md",
    "story/outline/volume_map.md",
    "story/pending_hooks.md",
    "story/story_bible.md",
    "story/subplot_board.md",
    "story/volume_outline.md",
];

/// 与 readRoleCards 枚举的同层目录。
const FINGERPRINT_ROLE_DIRS: &[&str] = &["主要角色", "次要角色", "major", "minor"];

/// 枚举全部上下文输入为 [相对 posix 路径, 内容] 对。缺失文件不产生条目，
/// 存在的空文件产生 ["path", ""]——创建/删除/重命名输入文件都会改变指纹，
/// 不仅是编辑内容。
async fn collect_fingerprint_files(book_dir: &Path) -> Vec<(String, String)> {
    let mut files = Vec::new();
    for rel_path in FINGERPRINT_FIXED_INPUTS {
        if let Some(content) = read_if_exists(&book_dir.join(rel_path)).await {
            files.push((rel_path.to_string(), content));
        }
    }
    // story/state/*.json（排序）。
    let state_dir = book_dir.join("story").join("state");
    if let Ok(mut entries) = tokio::fs::read_dir(&state_dir).await {
        let mut names: Vec<String> = Vec::new();
        while let Ok(Some(entry)) = entries.next_entry().await {
            let name = entry.file_name().to_string_lossy().into_owned();
            if name.ends_with(".json") {
                names.push(name);
            }
        }
        names.sort();
        for name in names {
            files.push((
                format!("story/state/{name}"),
                read_or_empty(&state_dir.join(&name)).await,
            ));
        }
    }
    for tier in FINGERPRINT_ROLE_DIRS {
        let dir = book_dir.join("story").join("roles").join(tier);
        let Ok(mut entries) = tokio::fs::read_dir(&dir).await else {
            continue;
        };
        let mut names: Vec<String> = Vec::new();
        while let Ok(Some(entry)) = entries.next_entry().await {
            let name = entry.file_name().to_string_lossy().into_owned();
            if name.ends_with(".md") {
                names.push(name);
            }
        }
        for name in names {
            files.push((
                format!("story/roles/{tier}/{name}"),
                read_or_empty(&dir.join(&name)).await,
            ));
        }
    }
    files
}

async fn read_if_exists(path: &Path) -> Option<String> {
    tokio::fs::read_to_string(path).await.ok()
}

/// TS `renderForecastContextMarkdown`：双语分段（空段滤除）。
pub fn render_forecast_context_markdown(context: &ForecastContext) -> String {
    let zh = context.language == "zh";
    let sections: Vec<(&str, &str)> = vec![
        (if zh { "作者意图" } else { "Author intent" }, &context.sections.author_intent),
        (if zh { "当前聚焦" } else { "Current focus" }, &context.sections.current_focus),
        (if zh { "当前状态" } else { "Current state" }, &context.sections.current_state),
        (if zh { "伏笔与钩子" } else { "Pending hooks" }, &context.sections.pending_hooks),
        (if zh { "故事框架" } else { "Story frame" }, &context.sections.story_frame),
        (if zh { "卷映射" } else { "Volume map" }, &context.sections.volume_map),
        (
            if zh { "近期章节摘要" } else { "Recent chapter summaries" },
            &context.sections.recent_chapter_summaries,
        ),
        (
            if zh { "人物与关系" } else { "Characters and relationships" },
            &context.sections.character_context,
        ),
        (if zh { "支线看板" } else { "Subplot board" }, &context.sections.subplot_board),
    ];

    let mut blocks = vec![if zh {
        format!(
            "# 正史上下文（《{}》，已完成至第 {} 章）",
            context.book_title, context.base_chapter
        )
    } else {
        format!(
            "# Canonical context (\"{}\", written through chapter {})",
            context.book_title, context.base_chapter
        )
    }];
    for (heading, content) in sections {
        if !content.trim().is_empty() {
            blocks.push(format!("## {heading}\n\n{}", content.trim()));
        }
    }
    blocks.join("\n\n")
}

async fn read_book_config(book_dir: &Path) -> (String, String) {
    let Ok(raw) = tokio::fs::read_to_string(book_dir.join("book.json")).await else {
        return (String::new(), "zh".to_string());
    };
    let Ok(parsed) = serde_json::from_str::<serde_json::Value>(&raw) else {
        return (String::new(), "zh".to_string());
    };
    let title = parsed.get("title").and_then(serde_json::Value::as_str).unwrap_or_default().to_string();
    let language = if parsed.get("language").and_then(serde_json::Value::as_str) == Some("en") {
        "en"
    } else {
        "zh"
    };
    (title, language.to_string())
}

/// 磁盘上存在的最高章号。预测的过期判定以此为键：新的正史章节使旧预测失效。
async fn resolve_base_chapter(book_dir: &Path) -> u32 {
    let Ok(mut entries) = tokio::fs::read_dir(book_dir.join("chapters")).await else {
        return 0;
    };
    static CHAPTER_RE: std::sync::OnceLock<regex::Regex> = std::sync::OnceLock::new();
    let chapter_re = CHAPTER_RE
        .get_or_init(|| regex::Regex::new(r"^(\d+)[_-]?.*\.md$").unwrap());
    let mut max = 0u32;
    while let Ok(Some(entry)) = entries.next_entry().await {
        let name = entry.file_name().to_string_lossy().into_owned();
        if let Some(caps) = chapter_re.captures(&name) {
            if let Ok(number) = caps[1].parse::<u32>() {
                if number > max {
                    max = number;
                }
            }
        }
    }
    max
}

async fn read_or_empty(path: &Path) -> String {
    tokio::fs::read_to_string(path).await.unwrap_or_default()
}
