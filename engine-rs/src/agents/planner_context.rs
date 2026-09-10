//! planner 上下文提取器（纯函数群 + story/ 真相文件读取）。
//!
//! 移植自 `packages/core/src/agents/planner-context.ts`（297 行）。
//! 为 planner 的 user message 提供结构化上下文块：主角行 / 对手行 / 协作者行 /
//! 活跃支线 / 近期情感线 / 相关线索 / 陈旧 hook 账 / 近章摘要表 / 规则块。
//!
//! ## 移植纪律
//! - 表格行选择器的正则（活跃/休眠/对手/协作者/头行）逐字移植，大小写不敏感
//! - `row.find(...)` 首个匹配单元格语义：`找到任一活跃 cell 且无休眠 cell`
//! - `row.filter(Boolean).join(" | ")`——空单元格剔除后再拼接
//! - 情感线章节号在**第 2 列**（row`[1]`），支线/摘要在第 1 列（row`[0]`）
//! - `slice(-limit)` 取**末尾** N 行（近期语义），非前 N

use std::path::Path;

use regex::Regex;
use std::sync::OnceLock;

use crate::agents::rules_reader::read_book_rules;
use crate::models::runtime_state::HookRecord;
use crate::utils::hook_lifecycle::hook_status_text;
use crate::utils::language::WritingLanguage;
use crate::utils::outline_paths::read_character_context;
use crate::utils::story_markdown::parse_markdown_table_rows;

async fn read_or_empty(path: &Path) -> String {
    tokio::fs::read_to_string(path).await.unwrap_or_default()
}

/// 主角矩阵（Phase 5：roles/ 优先，legacy character_matrix.md 回退）。
/// storyDir 即 `<bookDir>/story`，bookDir 经 dirname 反推。
pub async fn read_character_matrix(story_dir: &Path) -> String {
    let book_dir = story_dir.parent().unwrap_or(story_dir);
    read_character_context(book_dir, "").await
}

pub async fn read_subplot_board(story_dir: &Path) -> String {
    read_or_empty(&story_dir.join("subplot_board.md")).await
}

pub async fn read_emotional_arcs(story_dir: &Path) -> String {
    read_or_empty(&story_dir.join("emotional_arcs.md")).await
}

pub async fn read_pending_hooks(story_dir: &Path) -> String {
    read_or_empty(&story_dir.join("pending_hooks.md")).await
}

pub async fn read_brief(story_dir: &Path) -> String {
    read_or_empty(&story_dir.join("brief.md")).await
}

/// 结构化规则（主角 / 禁忌 / 题材锁 / 行为约束）→ planner prompt 的紧凑块。
///
/// Phase 5 权威加载链（story_frame.md 优先 / legacy book_rules.md 回退 / shim 拒绝清零）。
/// 无结构化规则返回 ""——planner 模板自带占位。
pub async fn read_book_rules_block(story_dir: &Path) -> String {
    let book_dir = story_dir.parent().unwrap_or(story_dir);
    let Some(parsed) = read_book_rules(book_dir).await else {
        return String::new();
    };

    let mut lines: Vec<String> = Vec::new();

    if let Some(protagonist) = &parsed.rules.protagonist {
        let personality = protagonist.personality_lock.join("、");
        let constraints = protagonist.behavioral_constraints.join("、");
        let mut line = format!("- 主角 {}", protagonist.name);
        if !personality.is_empty() {
            line.push_str(&format!(" / 人设锁：{personality}"));
        }
        if !constraints.is_empty() {
            line.push_str(&format!(" / 行为约束：{constraints}"));
        }
        lines.push(line);
    }

    if !parsed.rules.prohibitions.is_empty() {
        lines.push("- 本书禁忌：".to_string());
        for prohibition in &parsed.rules.prohibitions {
            lines.push(format!("  - {prohibition}"));
        }
    }

    if let Some(genre_lock) = &parsed.rules.genre_lock {
        let forbidden = genre_lock.forbidden.join("、");
        let mut line = format!("- 题材锁：{}", genre_lock.primary);
        if !forbidden.is_empty() {
            line.push_str(&format!(" / 禁止混入：{forbidden}"));
        }
        lines.push(line);
    }

    if let Some(fanfic_mode) = &parsed.rules.fanfic_mode {
        lines.push(format!("- 同人模式：{}", fanfic_mode_label(fanfic_mode)));
    }

    // 正文承载叙事指引（如叙事视角），原文透传使 planner 看到与清理前一致的文本。
    let trimmed_body = parsed.body.trim();
    if !trimmed_body.is_empty() {
        lines.push(String::new());
        lines.push(trimmed_body.to_string());
    }

    lines.join("\n").trim().to_string()
}

fn fanfic_mode_label(mode: &crate::models::book::FanficMode) -> &'static str {
    use crate::models::book::FanficMode;
    match mode {
        FanficMode::Canon => "canon",
        FanficMode::Au => "au",
        FanficMode::Ooc => "ooc",
        FanficMode::Cp => "cp",
    }
}

/// chapter_summaries.md 的末 N 行（含表头重渲染）。
/// 返回原始表切片（带表头）使 planner 隐式获得列含义。
pub fn format_recent_summaries(
    chapter_summaries_raw: &str,
    chapter_number: u32,
    limit: usize,
) -> String {
    let mut rows: Vec<Vec<String>> = parse_markdown_table_rows(chapter_summaries_raw)
        .into_iter()
        .filter(|row| row.first().is_some_and(|cell| digit_only_re().is_match(cell)))
        .filter(|row| {
            row.first()
                .and_then(|cell| cell.parse::<i64>().ok())
                .map(|chapter| chapter < i64::from(chapter_number))
                .unwrap_or(false)
        })
        .collect();
    rows.sort_by(|a, b| {
        let left = a.first().and_then(|c| c.parse::<i64>().ok()).unwrap_or(0);
        let right = b.first().and_then(|c| c.parse::<i64>().ok()).unwrap_or(0);
        left.cmp(&right)
    });

    let start = rows.len().saturating_sub(limit);
    let recent = &rows[start..];
    if recent.is_empty() {
        return "（暂无前章摘要）".to_string();
    }

    let header = "| 章节 | 标题 | 出场人物 | 关键事件 | 状态变化 | 伏笔动态 | 情绪基调 | 章节类型 |";
    let divider = "| --- | --- | --- | --- | --- | --- | --- | --- |";
    let body = recent
        .iter()
        .map(|row| format!("| {} |", row.join(" | ")))
        .collect::<Vec<_>>()
        .join("\n");
    [header, divider, &body].join("\n")
}

/// 由 subplot_board.md 活跃行 + emotional_arcs.md 近期行临时拼装当前 arc 散文
/// （Phase 8 将以独立 tier2_current_arc.md 替换该来源）。
pub fn compose_current_arc_prose(
    subplot_board_raw: &str,
    emotional_arcs_raw: &str,
    chapter_number: u32,
) -> String {
    let active_subplots = extract_active_subplot_lines(subplot_board_raw);
    let recent_arcs = extract_recent_emotional_arc_lines(emotional_arcs_raw, chapter_number, 3);

    let mut parts: Vec<String> = Vec::new();
    if !active_subplots.is_empty() {
        parts.push(format!(
            "活跃支线：\n{}",
            active_subplots
                .iter()
                .map(|line| format!("- {line}"))
                .collect::<Vec<_>>()
                .join("\n")
        ));
    }
    if !recent_arcs.is_empty() {
        parts.push(format!(
            "近期情感线：\n{}",
            recent_arcs
                .iter()
                .map(|line| format!("- {line}"))
                .collect::<Vec<_>>()
                .join("\n")
        ));
    }
    if parts.is_empty() {
        return "（暂无 arc 数据——可能是新书起始阶段）".to_string();
    }
    parts.join("\n\n")
}

fn extract_active_subplot_lines(raw: &str) -> Vec<String> {
    let rows = parse_markdown_table_rows(raw);
    if rows.is_empty() {
        // 非表格形态：取 bullet 行（去 "- " 前缀），前 6 条。
        return raw
            .split('\n')
            .map(|line| line.trim())
            .filter(|line| line.starts_with('-'))
            .map(|line| bullet_prefix_re().replace(line, "").to_string())
            .filter(|line| !line.is_empty())
            .take(6)
            .collect();
    }
    rows.into_iter()
        .filter(|row| {
            !row.first()
                .is_some_and(|cell| subplot_header_re().is_match(cell))
        })
        .filter(|row| {
            // 任一 cell 命中活跃词，且无 cell 命中休眠词（TS row.find 语义）。
            let active = row.iter().any(|cell| subplot_active_re().is_match(cell));
            let dormant = row.iter().any(|cell| subplot_dormant_re().is_match(cell));
            active && !dormant
        })
        .map(|row| join_non_empty_cells(&row))
        .take(6)
        .collect()
}

fn extract_recent_emotional_arc_lines(raw: &str, chapter_number: u32, limit: usize) -> Vec<String> {
    let rows = parse_markdown_table_rows(raw);
    if rows.is_empty() {
        // TS：先取末尾 limit 条 bullet，再剥前缀（无空行过滤，逐字对齐）。
        let bullets: Vec<&str> = raw
            .split('\n')
            .map(|line| line.trim())
            .filter(|line| line.starts_with('-'))
            .collect();
        let start = bullets.len().saturating_sub(limit);
        return bullets[start..]
            .iter()
            .map(|line| bullet_prefix_re().replace(line, "").to_string())
            .collect();
    }
    // emotional_arcs.md 列布局：角色 | 章节 | 情绪状态 | 触发事件 | 强度 | 弧线方向
    // 章节号在第 2 列（row[1]）。
    let filtered: Vec<Vec<String>> = rows
        .into_iter()
        .filter(|row| row.get(1).is_some_and(|cell| digit_only_re().is_match(cell)))
        .filter(|row| {
            row.get(1)
                .and_then(|cell| cell.parse::<i64>().ok())
                .map(|chapter| chapter < i64::from(chapter_number))
                .unwrap_or(false)
        })
        .collect();
    let start = filtered.len().saturating_sub(limit);
    filtered[start..]
        .iter()
        .map(|row| join_non_empty_cells(row))
        .collect()
}

/// 主角行：与主角关系列命中「主角本人/主角/protagonist」；无显式命中回退
/// 首个非头行数据行（惯例上主角几乎总在首行）。
pub fn extract_protagonist_row(character_matrix_raw: &str) -> String {
    let rows = parse_markdown_table_rows(character_matrix_raw);
    if let Some(protagonist) = rows
        .iter()
        .find(|row| row.iter().any(|cell| protagonist_cell_re().is_match(cell.trim())))
    {
        return format!("| {} |", protagonist.join(" | "));
    }
    if let Some(first_data_row) = rows.iter().find(|row| !is_likely_header_row(row)) {
        return format!("| {} |", first_data_row.join(" | "));
    }
    "（未找到主角行——请检查 character_matrix.md）".to_string()
}

fn is_likely_header_row(row: &[String]) -> bool {
    row.iter().any(|cell| matrix_header_cell_re().is_match(cell.trim()))
}

pub fn extract_opponent_rows(character_matrix_raw: &str, limit: usize) -> String {
    extract_rows_by_relation(character_matrix_raw, opponent_re(), limit, "（暂无明确对手登场）")
}

pub fn extract_collaborator_rows(character_matrix_raw: &str, limit: usize) -> String {
    extract_rows_by_relation(
        character_matrix_raw,
        collaborator_re(),
        limit,
        "（暂无明确协作者登场）",
    )
}

fn extract_rows_by_relation(
    character_matrix_raw: &str,
    pattern: &'static Regex,
    limit: usize,
    empty_text: &str,
) -> String {
    let rows: Vec<Vec<String>> = parse_markdown_table_rows(character_matrix_raw)
        .into_iter()
        .filter(|row| row.iter().any(|cell| pattern.is_match(cell)))
        .filter(|row| {
            !row
                .iter()
                .any(|cell| relation_protagonist_re().is_match(cell.trim()))
        })
        .take(limit)
        .collect();
    if rows.is_empty() {
        return empty_text.to_string();
    }
    rows.into_iter()
        .map(|row| format!("| {} |", row.join(" | ")))
        .collect::<Vec<_>>()
        .join("\n")
}

/// 可能被牵动的线索（pending_hooks + subplot_board 的活跃行）。
pub fn extract_relevant_threads(pending_hooks_raw: &str, subplot_board_raw: &str) -> String {
    let hook_rows = parse_markdown_table_rows(pending_hooks_raw)
        .into_iter()
        .filter(|row| {
            !row
                .first()
                .is_some_and(|cell| hook_id_header_re().is_match(cell))
        })
        .filter(|row| row.iter().any(|cell| thread_relevant_re().is_match(cell)))
        .filter(|row| !row.iter().any(|cell| thread_stale_re().is_match(cell)))
        .map(|row| format!("- {}: {}", row[0], join_non_empty_cells(&row[1..])));

    let subplot_rows = parse_markdown_table_rows(subplot_board_raw)
        .into_iter()
        .filter(|row| {
            !row
                .first()
                .is_some_and(|cell| subplot_id_header_re().is_match(cell))
        })
        .filter(|row| row.iter().any(|cell| thread_relevant_re().is_match(cell)))
        .filter(|row| !row.iter().any(|cell| thread_stale_re().is_match(cell)))
        .map(|row| format!("- {}: {}", row[0], join_non_empty_cells(&row[1..])));

    let lines: Vec<String> = hook_rows.chain(subplot_rows).collect();
    if lines.is_empty() {
        return "（暂无活跃线索）".to_string();
    }
    lines.join("\n")
}

/// 陈旧 hook 账（「## 本章 hook 账」的输入侧格式化）。
/// 调用方已按 computeRecyclableHooks 过滤，此处仅负责渲染。
/// 语言开关镜像 planner prompt 其余部分：默认 zh，英文书 en。
pub fn format_recyclable_hooks(
    hooks: &[HookRecord],
    chapter_number: u32,
    language: WritingLanguage,
) -> String {
    if hooks.is_empty() {
        return if language == WritingLanguage::En {
            "(no stale hooks — the ledger is clean)".to_string()
        } else {
            "（暂无陈旧 hook——账本干净）".to_string()
        };
    }

    let lines: Vec<String> = hooks
        .iter()
        .take(6)
        .map(|hook| {
            let last_touch = hook.start_chapter.max(hook.last_advanced_chapter);
            let silence = if last_touch == 0 {
                i64::from(chapter_number)
            } else {
                i64::from(chapter_number).saturating_sub(i64::from(last_touch)).max(0)
            };
            let payoff = if hook.expected_payoff.trim().is_empty() {
                hook.notes.trim()
            } else {
                hook.expected_payoff.trim()
            };
            let core = if hook.core_hook == Some(true) {
                if language == WritingLanguage::En { " [core]" } else { " [核心]" }
            } else {
                ""
            };
            if language == WritingLanguage::En {
                format!(
                    "- {} \"{}\" — status={}, silent {} ch{}",
                    hook.hook_id, payoff, hook_status_text(hook), silence, core
                )
            } else {
                format!(
                    "- {} \"{}\" — 状态={}，已沉默 {} 章{}",
                    hook.hook_id, payoff, hook_status_text(hook), silence, core
                )
            }
        })
        .collect();

    let header = if language == WritingLanguage::En {
        "The planner MUST place each of these under advance / resolve / defer in the hook ledger (deferring requires an explicit reason):"
    } else {
        "规划时必须把以下每个 hook 放入 advance / resolve / defer（若 defer，必须写出理由）："
    };
    let mut out = vec![header.to_string()];
    out.extend(lines);
    out.join("\n")
}

fn join_non_empty_cells(row: &[String]) -> String {
    row.iter()
        .filter(|cell| !cell.is_empty())
        .cloned()
        .collect::<Vec<_>>()
        .join(" | ")
}

fn digit_only_re() -> &'static Regex {
    static R: OnceLock<Regex> = OnceLock::new();
    R.get_or_init(|| Regex::new(r"^\d+$").unwrap())
}

fn bullet_prefix_re() -> &'static Regex {
    static R: OnceLock<Regex> = OnceLock::new();
    R.get_or_init(|| Regex::new(r"^-\s*").unwrap())
}

fn subplot_header_re() -> &'static Regex {
    static R: OnceLock<Regex> = OnceLock::new();
    R.get_or_init(|| Regex::new(r"(?i)^(id|subplot_id|subplot|status|状态)$").unwrap())
}

fn subplot_active_re() -> &'static Regex {
    static R: OnceLock<Regex> = OnceLock::new();
    R.get_or_init(|| Regex::new(r"(?i)进行|推进|高压|激活|activ|progress|partial").unwrap())
}

fn subplot_dormant_re() -> &'static Regex {
    static R: OnceLock<Regex> = OnceLock::new();
    R.get_or_init(|| Regex::new(r"(?i)暂稳待续|暂挂|dormant|paused").unwrap())
}

fn matrix_header_cell_re() -> &'static Regex {
    static R: OnceLock<Regex> = OnceLock::new();
    R.get_or_init(|| {
        Regex::new(r"(?i)^(角色|character|name|核心标签|与主角关系|relation)$").unwrap()
    })
}

fn protagonist_cell_re() -> &'static Regex {
    static R: OnceLock<Regex> = OnceLock::new();
    R.get_or_init(|| Regex::new(r"(?i)^(主角本人|主角|protagonist)$").unwrap())
}

fn relation_protagonist_re() -> &'static Regex {
    static R: OnceLock<Regex> = OnceLock::new();
    R.get_or_init(|| Regex::new(r"(?i)^(主角|protagonist)$").unwrap())
}

fn opponent_re() -> &'static Regex {
    static R: OnceLock<Regex> = OnceLock::new();
    R.get_or_init(|| Regex::new(r"(?i)敌对|对手|阻力|opponent|antagonist|foe").unwrap())
}

fn collaborator_re() -> &'static Regex {
    static R: OnceLock<Regex> = OnceLock::new();
    R.get_or_init(|| Regex::new(r"(?i)协力|盟友|临时助力|ally|collaborator|mentor").unwrap())
}

fn hook_id_header_re() -> &'static Regex {
    static R: OnceLock<Regex> = OnceLock::new();
    R.get_or_init(|| Regex::new(r"(?i)^(hook_id)$").unwrap())
}

fn subplot_id_header_re() -> &'static Regex {
    static R: OnceLock<Regex> = OnceLock::new();
    R.get_or_init(|| Regex::new(r"(?i)^(id|subplot_id|subplot)$").unwrap())
}

fn thread_relevant_re() -> &'static Regex {
    static R: OnceLock<Regex> = OnceLock::new();
    R.get_or_init(|| Regex::new(r"(?i)activat|partial_payoff|推进|高压|open|progress").unwrap())
}

fn thread_stale_re() -> &'static Regex {
    static R: OnceLock<Regex> = OnceLock::new();
    R.get_or_init(|| Regex::new(r"(?i)resolved|deferred|dormant|暂稳待续|暂挂|已回收").unwrap())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn format_recent_summaries_slices_last_n_with_header() {
        let md = "| 章节 | 标题 | 出场人物 | 关键事件 | 状态变化 | 伏笔动态 | 情绪基调 | 章节类型 |\n\
                  | --- | --- | --- | --- | --- | --- | --- | --- |\n\
                  | 1 | a | 甲 | e | s | h | m | t |\n\
                  | 2 | b | 乙 | e | s | h | m | t |\n\
                  | 3 | c | 丙 | e | s | h | m | t |\n";
        let out = format_recent_summaries(md, 10, 2);
        assert!(out.contains("| 章节 |"));
        assert!(out.contains("| 2 | b |"));
        assert!(out.contains("| 3 | c |"));
        assert!(!out.contains("| 1 | a |"));

        assert_eq!(format_recent_summaries(md, 1, 3), "（暂无前章摘要）");
    }

    #[test]
    fn compose_current_arc_prose_merges_active_and_recent() {
        let subplot = "| id | 状态 |\n| --- | --- |\n| S1 | 推进中 |\n| S2 | 暂挂 |\n";
        let arcs = "| 角色 | 章节 | 情绪 |\n| --- | --- | --- |\n| 甲 | 1 | 焦虑 |\n| 乙 | 2 | 坚定 |\n";
        let out = compose_current_arc_prose(subplot, arcs, 5);
        assert!(out.contains("活跃支线：\n- S1 | 推进中"));
        assert!(out.contains("近期情感线：\n- 甲 | 1 | 焦虑"));

        assert_eq!(
            compose_current_arc_prose("", "", 5),
            "（暂无 arc 数据——可能是新书起始阶段）"
        );
    }

    #[test]
    fn extract_protagonist_prefers_explicit_then_first_data_row() {
        let matrix = "| 角色 | 与主角关系 |\n| --- | --- |\n| 甲 | 兄长 |\n| 乙 | protagonist |\n";
        assert_eq!(extract_protagonist_row(matrix), "| 乙 | protagonist |");

        let no_explicit = "| 角色 | 与主角关系 |\n| --- | --- |\n| 甲 | 兄长 |\n";
        assert_eq!(extract_protagonist_row(no_explicit), "| 甲 | 兄长 |");

        assert_eq!(
            extract_protagonist_row("无表格"),
            "（未找到主角行——请检查 character_matrix.md）"
        );
    }

    #[test]
    fn extract_opponent_and_collaborator_rows() {
        let matrix = "| 名字 | 关系 |\n| --- | --- |\n| 主角 | 主角 |\n| 甲 | 敌对 |\n| 乙 | 盟友 |\n| 丙 | 敌对 |\n";
        assert_eq!(extract_opponent_rows(matrix, 1), "| 甲 | 敌对 |");
        // 主角行被排除（关系列命中 ^(主角|protagonist)$）。
        let opponents = extract_opponent_rows(matrix, 3);
        assert!(opponents.contains("甲") && opponents.contains("丙") && !opponents.contains("主角"));
        assert_eq!(extract_collaborator_rows(matrix, 3), "| 乙 | 盟友 |");
        assert_eq!(extract_opponent_rows("空", 3), "（暂无明确对手登场）");
    }

    #[test]
    fn extract_relevant_threads_filters_stale_and_headers() {
        let hooks = "| hook_id | 状态 |\n| --- | --- |\n| H01 | progressing |\n| H02 | resolved |\n";
        let subplots = "| id | 状态 |\n| --- | --- |\n| S1 | open |\n";
        let out = extract_relevant_threads(hooks, subplots);
        // TS row.slice(1)——首列（id）只作前缀，不进尾部拼接。
        assert!(out.contains("- H01: progressing"));
        assert!(out.contains("- S1: open"));
        assert!(!out.contains("H02"));

        assert_eq!(extract_relevant_threads("", ""), "（暂无活跃线索）");
    }

    #[test]
    fn format_recyclable_hooks_renders_zh_and_en() {
        use crate::models::runtime_state::HookStatus;
        let mut hook = HookRecord {
            hook_id: "H01".into(),
            start_chapter: 1,
            hook_type: "plot".into(),
            status: HookStatus::Open,
            status_raw: "pressured".into(),
            last_advanced_chapter: 2,
            expected_payoff: "第10章兑现".into(),
            payoff_timing: None,
            notes: "备注".into(),
            depends_on: None,
            pays_off_in_arc: None,
            core_hook: Some(true),
            half_life_chapters: None,
            advanced_count: None,
            promoted: None,
        };
        let zh = format_recyclable_hooks(&[hook.clone()], 9, WritingLanguage::Zh);
        assert!(zh.contains("规划时必须把以下每个 hook"));
        assert!(zh.contains("- H01 \"第10章兑现\" — 状态=pressured，已沉默 7 章 [核心]"));

        let en = format_recyclable_hooks(&[hook.clone()], 9, WritingLanguage::En);
        assert!(en.contains("- H01 \"第10章兑现\" — status=pressured, silent 7 ch [core]"));

        // payoff 空串回退 notes（TS `||` falsy 链）。
        hook.expected_payoff = String::new();
        let zh = format_recyclable_hooks(&[hook], 9, WritingLanguage::Zh);
        assert!(zh.contains("\"备注\""));

        assert_eq!(
            format_recyclable_hooks(&[], 9, WritingLanguage::Zh),
            "（暂无陈旧 hook——账本干净）"
        );
    }
}
