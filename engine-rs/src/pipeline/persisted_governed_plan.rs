//! 治理计划持久化（plan.md 存取 + legacy intent.md 回退）。
//!
//! 移植自 `packages/core/src/pipeline/persisted-governed-plan.ts`（275 行）。
//! 持久化计划是人读 markdown（`story/runtime/chapter-NNNN.plan.md`），memo 协议
//! 同为 Markdown——不要求 LLM 产出 YAML frontmatter；遇到旧版 frontmatter 缓存
//! 直接返回 None 让 runner 重规划。
//!
//! 兄弟文件 `chapter-NNNN.intent.md` 是人读渲染，**不**回读解析；仅 `.plan.md`
//! 是恢复的权威源。解析以任何方式失败都返回 None 重规划——静默降级比重规划
//! 更糟。

use std::path::{Path, PathBuf};

use regex::Regex;
use std::sync::OnceLock;

use crate::agents::planner::PlanChapterOutput;
use crate::models::input_governance::{ChapterIntent, ChapterMemo};
use crate::utils::chapter_memo_parser::{parse_memo, PlannerParseError};

fn plan_path(book_dir: &Path, chapter_number: u32) -> PathBuf {
    book_dir
        .join("story")
        .join("runtime")
        .join(format!("chapter-{:04}.plan.md", chapter_number))
}

fn intent_path(book_dir: &Path, chapter_number: u32) -> PathBuf {
    book_dir
        .join("story")
        .join("runtime")
        .join(format!("chapter-{:04}.intent.md", chapter_number))
}

/// 保存治理计划（markdown 渲染 + 落盘）。
pub async fn save_persisted_plan(book_dir: &Path, plan: &PlanChapterOutput) -> std::io::Result<()> {
    let content = render_persisted_plan_markdown(&plan.intent, &plan.memo, &plan.planner_inputs);
    tokio::fs::write(plan_path(book_dir, plan.memo.chapter), content).await
}

/// 读取治理计划；任何解析失败返回 None（调用方重规划）。
pub async fn load_persisted_plan(
    book_dir: &Path,
    chapter_number: u32,
) -> Option<PlanChapterOutput> {
    let raw = match tokio::fs::read_to_string(plan_path(book_dir, chapter_number)).await {
        Ok(raw) => raw,
        Err(_) => return load_legacy_intent_plan(book_dir, chapter_number).await,
    };

    if raw.trim_start().starts_with("---") {
        return None;
    }

    // memo 经与 planner 相同的严格解析器重建——7 个必备小节仍需齐全，
    // 任何漂移触发重规划（None）。
    let memo_block = extract_marked_block(&raw, "MEMO")?;
    let is_golden = read_boolean_field(&raw, "Golden Opening");
    let memo = match parse_memo(&memo_block, chapter_number, is_golden.unwrap_or(false)) {
        Ok(memo) => memo,
        Err(PlannerParseError(_)) => return None,
    };

    let goal = read_field(&raw, "Intent Goal").unwrap_or_else(|| memo.goal.clone());
    let intent = ChapterIntent {
        chapter: chapter_number,
        goal,
        outline_node: read_optional_field(&raw, "Outline Node"),
        arc_context: read_optional_field(&raw, "Arc Context"),
        must_keep: read_list_section(&raw, "Must Keep"),
        must_avoid: read_list_section(&raw, "Must Avoid"),
        style_emphasis: read_list_section(&raw, "Style Emphasis"),
    };

    let planner_inputs = read_list_section(&raw, "Planner Inputs");

    // intentMarkdown 是展示工件——优先读兄弟 .intent.md（下游消费同一内容）；
    // 缺失时回退 memo body（可用但信息较少）。
    let mut intent_markdown = memo.body.clone();
    if let Ok(rendered) = tokio::fs::read_to_string(intent_path(book_dir, chapter_number)).await {
        intent_markdown = rendered;
    }

    Some(PlanChapterOutput {
        intent,
        memo,
        intent_markdown,
        planner_inputs,
        runtime_path: intent_path(book_dir, chapter_number),
    })
}

/// 渲染持久化计划 markdown。golden 守门（`persisted_plan_roundtrip` 等）。
pub fn render_persisted_plan_markdown(
    intent: &ChapterIntent,
    memo: &ChapterMemo,
    planner_inputs: &[String],
) -> String {
    [
        format!("# Chapter {} Plan", memo.chapter),
        String::new(),
        "## Metadata".to_string(),
        format!("Chapter: {}", memo.chapter),
        format!("Golden Opening: {}", if memo.is_golden_opening { "yes" } else { "no" }),
        String::new(),
        "<!-- INKOS_PLAN_MEMO_START -->".to_string(),
        render_memo_markdown(memo),
        "<!-- INKOS_PLAN_MEMO_END -->".to_string(),
        String::new(),
        "## Intent".to_string(),
        format!("Intent Goal: {}", intent.goal),
        format!("Outline Node: {}", intent.outline_node.as_deref().unwrap_or("(none)")),
        format!("Arc Context: {}", intent.arc_context.as_deref().unwrap_or("(none)")),
        String::new(),
        "### Must Keep".to_string(),
        render_list(&intent.must_keep),
        String::new(),
        "### Must Avoid".to_string(),
        render_list(&intent.must_avoid),
        String::new(),
        "### Style Emphasis".to_string(),
        render_list(&intent.style_emphasis),
        String::new(),
        "## Planner Inputs".to_string(),
        render_list(planner_inputs),
        String::new(),
    ]
    .join("\n")
}

fn render_memo_markdown(memo: &ChapterMemo) -> String {
    [
        format!("# 第 {} 章 memo", memo.chapter),
        String::new(),
        "## 本章目标".to_string(),
        memo.goal.clone(),
        String::new(),
        "## 关联线索".to_string(),
        render_list(&memo.thread_refs),
        String::new(),
        memo.body.trim().to_string(),
    ]
    .join("\n")
}

fn render_list(items: &[String]) -> String {
    if items.is_empty() {
        "- none".to_string()
    } else {
        items
            .iter()
            .map(|item| format!("- {item}"))
            .collect::<Vec<_>>()
            .join("\n")
    }
}

fn extract_marked_block(markdown: &str, name: &str) -> Option<String> {
    // <!-- INKOS_PLAN_{name}_START --> ... <!-- INKOS_PLAN_{name}_END -->（m 标志）。
    // 每次加载仅一次调用，直接构建（非热路径）。
    let pattern = format!(
        r"(?m)<!--\s*INKOS_PLAN_{}_START\s*-->\s*([\s\S]*?)\s*<!--\s*INKOS_PLAN_{}_END\s*-->",
        name, name
    );
    let regex = Regex::new(&pattern).ok()?;
    regex
        .captures(markdown)
        .and_then(|captures| captures.get(1).map(|m| m.as_str().trim().to_string()))
}

fn read_field(markdown: &str, label: &str) -> Option<String> {
    let pattern = format!(r"(?m)^{}:\s*(.*)$", regex_escape(label));
    let regex = Regex::new(&pattern).ok()?;
    let value = regex
        .captures(markdown)
        .and_then(|captures| captures.get(1).map(|m| m.as_str().trim().to_string()));
    match value {
        Some(value) if !value.is_empty() && value != "(none)" => Some(value),
        _ => None,
    }
}

fn read_optional_field(markdown: &str, label: &str) -> Option<String> {
    read_field(markdown, label).filter(|value| is_meaningful_legacy_value(value))
}

fn read_boolean_field(markdown: &str, label: &str) -> Option<bool> {
    let value = read_field(markdown, label)?;
    if boolean_true_re().is_match(&value) {
        Some(true)
    } else if boolean_false_re().is_match(&value) {
        Some(false)
    } else {
        None
    }
}

fn read_list_section(markdown: &str, heading: &str) -> Vec<String> {
    // TS lookahead `(?=\n#{2,3}\s+|(?![\s\S]))`：Rust regex 无 lookahead，
    // 改为消费式终止符（下一标题行或串尾）；捕获组内容等价。
    let pattern = format!(
        r"(?ms)^#{{2,3}}\s+{}\s*\n(.*?)(?:\n#{{2,3}}\s|\z)",
        regex_escape(heading)
    );
    let Ok(regex) = Regex::new(&pattern) else {
        return Vec::new();
    };
    let Some(section) = regex
        .captures(markdown)
        .and_then(|captures| captures.get(1).map(|m| m.as_str().trim().to_string()))
    else {
        return Vec::new();
    };
    section
        .split("\r\n")
        .flat_map(|chunk| chunk.split('\n'))
        .map(|line| line.trim())
        .filter(|line| line.starts_with('-'))
        .map(|line| bullet_re().replace(line, "").trim().to_string())
        .filter(|line| !line.is_empty() && line.to_lowercase() != "none")
        .collect()
}

async fn load_legacy_intent_plan(
    book_dir: &Path,
    chapter_number: u32,
) -> Option<PlanChapterOutput> {
    let runtime_path = intent_path(book_dir, chapter_number);
    let intent_markdown = tokio::fs::read_to_string(&runtime_path).await.ok()?;

    let raw_goal = extract_section(&intent_markdown, "Goal")?;
    if !is_meaningful_legacy_value(&raw_goal) {
        return None;
    }
    let goal = raw_goal;
    let outline_node = extract_section(&intent_markdown, "Outline Node")
        .filter(|value| is_meaningful_legacy_value(value));

    let intent = ChapterIntent {
        chapter: chapter_number,
        goal: goal.clone(),
        outline_node,
        arc_context: None,
        must_keep: extract_list_section(&intent_markdown, "Must Keep"),
        must_avoid: extract_list_section(&intent_markdown, "Must Avoid"),
        style_emphasis: extract_list_section(&intent_markdown, "Style Emphasis"),
    };

    Some(PlanChapterOutput {
        intent,
        memo: ChapterMemo {
            chapter: chapter_number,
            // TS goal.slice(0, 50)：UTF-16 码元窗口。
            goal: utf16_head(&goal, 50),
            is_golden_opening: false,
            body: intent_markdown.clone(),
            thread_refs: Vec::new(),
        },
        intent_markdown,
        planner_inputs: vec![relative_to_book_dir(book_dir, &runtime_path)],
        runtime_path,
    })
}

fn extract_section(markdown: &str, heading: &str) -> Option<String> {
    // TS 的 `(?=\n## |\n### |$)`（m 标志）中 $ 于每个行尾成立——懒惰匹配
    // 恒停于首个行尾，实际只捕获标题后**第一行**。`[^\n]*` 等价还原该怪癖。
    let pattern = format!(
        r"(?m)^## {}\s*\n([^\n]*)",
        regex_escape(heading)
    );
    let regex = Regex::new(&pattern).ok()?;
    let value = regex
        .captures(markdown)
        .and_then(|captures| captures.get(1).map(|m| m.as_str().trim().to_string()));
    match value {
        Some(value) if !value.is_empty() && value != "- none" => Some(value),
        _ => None,
    }
}

fn extract_list_section(markdown: &str, heading: &str) -> Vec<String> {
    let Some(section) = extract_section(markdown, heading) else {
        return Vec::new();
    };
    section
        .split("\r\n")
        .flat_map(|chunk| chunk.split('\n'))
        .map(|line| line.trim())
        .filter(|line| line.starts_with('-'))
        .map(|line| bullet_re().replace(line, "").trim().to_string())
        .filter(|line| !line.is_empty() && line.to_lowercase() != "none")
        .collect()
}

fn is_meaningful_legacy_value(value: &str) -> bool {
    let normalized = value.trim();
    if normalized.is_empty() {
        return false;
    }
    if not_found_re().is_match(normalized) {
        return false;
    }
    if legacy_null_re().is_match(normalized) {
        return false;
    }
    if legacy_noise_re().is_match(normalized) {
        return false;
    }
    true
}

fn regex_escape(value: &str) -> String {
    // TS escapeRegExp：(/[.*+?^${}()|[\]\\]/g, "\\$&")
    escape_re().replace_all(value, r"\$0").into_owned()
}

fn escape_re() -> &'static Regex {
    static R: OnceLock<Regex> = OnceLock::new();
    R.get_or_init(|| Regex::new(r"[.*+?^${}()|\[\]\\]").unwrap())
}

fn bullet_re() -> &'static Regex {
    static R: OnceLock<Regex> = OnceLock::new();
    R.get_or_init(|| Regex::new(r"^-\s*").unwrap())
}

fn boolean_true_re() -> &'static Regex {
    static R: OnceLock<Regex> = OnceLock::new();
    R.get_or_init(|| Regex::new(r"(?i)^(yes|true|是)$").unwrap())
}

fn boolean_false_re() -> &'static Regex {
    static R: OnceLock<Regex> = OnceLock::new();
    R.get_or_init(|| Regex::new(r"(?i)^(no|false|否)$").unwrap())
}

fn not_found_re() -> &'static Regex {
    static R: OnceLock<Regex> = OnceLock::new();
    R.get_or_init(|| Regex::new(r"(?i)^\(?not found\)?$").unwrap())
}

fn legacy_null_re() -> &'static Regex {
    static R: OnceLock<Regex> = OnceLock::new();
    R.get_or_init(|| Regex::new(r"(?i)^(?:none|null|undefined|n/a)$").unwrap())
}

fn legacy_noise_re() -> &'static Regex {
    static R: OnceLock<Regex> = OnceLock::new();
    R.get_or_init(|| Regex::new(r"^[*_`\-\s]+$").unwrap())
}

/// 头部 UTF-16 码元窗口（对齐 JS `slice(0, n)`）。
fn utf16_head(value: &str, take: usize) -> String {
    let mut units = 0usize;
    let mut out = String::with_capacity(value.len());
    for c in value.chars() {
        let len = c.len_utf16();
        if units + len > take {
            break;
        }
        out.push(c);
        units += len;
    }
    out
}

/// 相对 bookDir 的 POSIX 路径（对齐 TS relativeToBookDir）。
pub fn relative_to_book_dir(book_dir: &Path, absolute_path: &Path) -> String {
    let relative = absolute_path
        .strip_prefix(book_dir)
        .unwrap_or(absolute_path);
    relative.to_string_lossy().replace('\\', "/")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_plan() -> PlanChapterOutput {
        PlanChapterOutput {
            intent: ChapterIntent {
                chapter: 3,
                goal: "夺回祖符".into(),
                outline_node: Some("夜探藏书阁".into()),
                arc_context: Some("卷纲节点：夜探藏书阁".into()),
                must_keep: vec!["保一".into()],
                must_avoid: vec!["禁止降智".into()],
                style_emphasis: vec!["紧凑".into()],
            },
            memo: ChapterMemo {
                chapter: 3,
                goal: "夺回祖符，兑现第一步".into(),
                is_golden_opening: true,
                body: "## 当前任务\n林动夜探藏书阁夺回祖符，避开巡夜执事的封锁线。\n\n## 读者此刻在等什么\n期待玉符来历揭开一部分；本章部分兑现并制造更强缺口。\n\n## 该兑现的 / 暂不掀的\n- 该兑现：玉符效力 → 兑现到第一层；暂不掀：幕后主使身份继续压住。\n\n## 日常/过渡承担什么任务\n不适用 - 本章无日常过渡，全程高压推进不留闲笔。\n\n## 关键抉择过三连问\n- 主角：为救族人冒险夺符；符合当前利益；符合坚忍人设。\n\n## 章尾必须发生的改变\n信息改变：林动得知符中封印之物的一角真相。\n\n## 本章 hook 账\nadvance:\n- H01 \"祖符来历\" → 推进（planted → pressured）\n\n## 不要做\n- 不要让反派降智，不要新增第三条支线。".into(),
                thread_refs: vec!["H01".into()],
            },
            intent_markdown: "# Chapter Intent".into(),
            planner_inputs: vec!["story/author_intent.md".into()],
            runtime_path: PathBuf::from("/tmp/x"),
        }
    }

    #[tokio::test]
    async fn plan_roundtrip_preserves_fields() {
        let dir = tempfile::tempdir().unwrap();
        tokio::fs::create_dir_all(dir.path().join("story").join("runtime"))
            .await
            .unwrap();
        let plan = sample_plan();
        save_persisted_plan(dir.path(), &plan).await.unwrap();

        let loaded = load_persisted_plan(dir.path(), 3).await.expect("应可恢复");
        assert_eq!(loaded.intent.goal, "夺回祖符");
        assert_eq!(loaded.intent.outline_node.as_deref(), Some("夜探藏书阁"));
        assert_eq!(loaded.intent.must_keep, vec!["保一"]);
        assert_eq!(loaded.memo.thread_refs, vec!["H01"]);
        assert!(loaded.memo.is_golden_opening);
        assert_eq!(loaded.planner_inputs, vec!["story/author_intent.md"]);
        // intent.md 兄弟文件缺失 → 回退 memo body。
        assert!(loaded.intent_markdown.contains("## 当前任务"));
    }

    #[tokio::test]
    async fn plan_missing_returns_none_or_legacy() {
        let dir = tempfile::tempdir().unwrap();
        assert!(load_persisted_plan(dir.path(), 9).await.is_none());
    }

    #[tokio::test]
    async fn legacy_yaml_frontmatter_rejected() {
        let dir = tempfile::tempdir().unwrap();
        let runtime = dir.path().join("story").join("runtime");
        tokio::fs::create_dir_all(&runtime).await.unwrap();
        tokio::fs::write(plan_path(dir.path(), 2), "---\nchapter: 2\n---\n旧缓存")
            .await
            .unwrap();
        // frontmatter + 无 legacy intent.md → None。
        assert!(load_persisted_plan(dir.path(), 2).await.is_none());
    }

    #[tokio::test]
    async fn legacy_intent_plan_fallback() {
        let dir = tempfile::tempdir().unwrap();
        let runtime = dir.path().join("story").join("runtime");
        tokio::fs::create_dir_all(&runtime).await.unwrap();
        let intent_md = "# Chapter Intent\n\n## Goal\n延续主线推进目标\n\n## Outline Node\n节点甲\n\n## Must Keep\n- 保一\n\n## Must Avoid\n- none\n";
        tokio::fs::write(intent_path(dir.path(), 5), intent_md)
            .await
            .unwrap();

        let loaded = load_persisted_plan(dir.path(), 5).await.expect("legacy 应回读");
        assert_eq!(loaded.intent.goal, "延续主线推进目标");
        assert_eq!(loaded.intent.outline_node.as_deref(), Some("节点甲"));
        assert_eq!(loaded.intent.must_keep, vec!["保一"]);
        assert_eq!(loaded.memo.thread_refs, Vec::<String>::new());
        assert_eq!(
            loaded.planner_inputs,
            vec!["story/runtime/chapter-0005.intent.md".to_string()]
        );
    }

    #[test]
    fn meaningful_legacy_value_matrix() {
        assert!(is_meaningful_legacy_value("真值"));
        assert!(!is_meaningful_legacy_value("(not found)"));
        assert!(!is_meaningful_legacy_value("NOT FOUND"));
        assert!(!is_meaningful_legacy_value("n/a"));
        assert!(!is_meaningful_legacy_value("***"));
        assert!(!is_meaningful_legacy_value(" "));
    }
}
