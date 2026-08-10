//! 章节聚合统计。
//!
//! 移植自 `packages/core/src/utils/analytics.ts`（92 行，纯函数）。
//! computeAnalytics 汇总一本书的章节字数/审核通过率/问题分类/状态分布/token 趋势。
//!
//! ## 移植要点
//! - TS Map 插入序 → Rust Vec 保插入序 + stable sort_by 处理计数并列
//! - 问题分类正则 `\[(?:critical|warning|info)\]\s*(.+?)[:：]`
//! - statusDistribution: serde JSON 对象比较无序，HashMap 即可

use regex::Regex;
use serde::{Deserialize, Serialize};
#[cfg(feature = "export-bindings")]
use ts_rs::TS;
use std::sync::OnceLock;

/// token 用量统计 + 最近 5 章趋势。
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[cfg_attr(feature = "export-bindings", derive(TS))]
#[cfg_attr(feature = "export-bindings", ts(export))]
#[serde(rename_all = "camelCase")]
pub struct TokenStats {
    pub total_prompt_tokens: u64,
    pub total_completion_tokens: u64,
    pub total_tokens: u64,
    pub avg_tokens_per_chapter: u64,
    pub recent_trend: Vec<ChapterTokenPoint>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[cfg_attr(feature = "export-bindings", derive(TS))]
#[cfg_attr(feature = "export-bindings", ts(export))]
#[serde(rename_all = "camelCase")]
pub struct ChapterTokenPoint {
    pub chapter: u32,
    pub total_tokens: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[cfg_attr(feature = "export-bindings", derive(TS))]
#[cfg_attr(feature = "export-bindings", ts(export))]
#[serde(rename_all = "camelCase")]
pub struct IssueCategoryCount {
    pub category: String,
    pub count: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[cfg_attr(feature = "export-bindings", derive(TS))]
#[cfg_attr(feature = "export-bindings", ts(export))]
#[serde(rename_all = "camelCase")]
pub struct ChapterIssueCount {
    pub chapter: u32,
    pub issue_count: u32,
}

/// 聚合统计结果。移植 TS `AnalyticsData`。
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[cfg_attr(feature = "export-bindings", derive(TS))]
#[cfg_attr(feature = "export-bindings", ts(export))]
#[serde(rename_all = "camelCase")]
pub struct AnalyticsData {
    pub book_id: String,
    pub total_chapters: u32,
    pub total_words: u64,
    pub avg_words_per_chapter: u64,
    pub audit_pass_rate: u32,
    pub top_issue_categories: Vec<IssueCategoryCount>,
    pub chapters_with_most_issues: Vec<ChapterIssueCount>,
    pub status_distribution: std::collections::HashMap<String, u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub token_stats: Option<TokenStats>,
}

/// 输入章节（聚合用最小字段集）。
#[derive(Debug, Clone)]
pub struct AnalyticsChapter {
    pub number: u32,
    pub status: String,
    pub word_count: u64,
    pub audit_issues: Vec<String>,
    pub token_usage: Option<AnalyticsTokenUsage>,
}

#[derive(Debug, Clone)]
pub struct AnalyticsTokenUsage {
    pub prompt_tokens: u64,
    pub completion_tokens: u64,
    pub total_tokens: u64,
}

fn issue_category_re() -> &'static Regex {
    static R: OnceLock<Regex> = OnceLock::new();
    R.get_or_init(|| Regex::new(r"\[(?:critical|warning|info)\]\s*(.+?)[:：]").unwrap())
}

const PASSED_STATUSES: &[&str] = &["ready-for-review", "approved", "published"];
const EXCLUDED_FROM_AUDITED: &[&str] = &["drafted", "drafting", "card-generated"];

/// 计算一本书的聚合统计。
pub fn compute_analytics(book_id: &str, chapters: &[AnalyticsChapter]) -> AnalyticsData {
    let total_chapters = chapters.len() as u32;
    let total_words: u64 = chapters.iter().map(|c| c.word_count).sum();
    let avg_words_per_chapter = if total_chapters > 0 {
        total_words / total_chapters as u64
    } else {
        0
    };

    let audited: Vec<&AnalyticsChapter> = chapters
        .iter()
        .filter(|c| !EXCLUDED_FROM_AUDITED.contains(&c.status.as_str()))
        .collect();
    let passed: Vec<&AnalyticsChapter> = audited
        .iter()
        .filter(|c| PASSED_STATUSES.contains(&c.status.as_str()))
        .copied()
        .collect();
    let audit_pass_rate = if !audited.is_empty() {
        ((passed.len() as f64 / audited.len() as f64) * 100.0).round() as u32
    } else {
        100
    };

    // 问题分类计数（Vec 保插入序，stable sort 处理并列）
    let mut category_counts: Vec<(String, u32)> = Vec::new();
    let mut index_map: std::collections::HashMap<String, usize> = std::collections::HashMap::new();
    for ch in chapters {
        for issue in &ch.audit_issues {
            let category = issue_category_re()
                .captures(issue)
                .and_then(|c| c.get(1))
                .map(|m| m.as_str().to_string())
                .unwrap_or_else(|| "未分类".to_string());
            if let Some(&idx) = index_map.get(&category) {
                category_counts[idx].1 += 1;
            } else {
                let idx = category_counts.len();
                index_map.insert(category.clone(), idx);
                category_counts.push((category, 1));
            }
        }
    }
    category_counts.sort_by(|a, b| b.1.cmp(&a.1));
    let top_issue_categories: Vec<IssueCategoryCount> = category_counts
        .into_iter()
        .take(10)
        .map(|(category, count)| IssueCategoryCount { category, count })
        .collect();

    // 问题最多的章节（issueCount desc，并列保原序）
    let mut with_issues: Vec<&AnalyticsChapter> = chapters.iter().filter(|c| !c.audit_issues.is_empty()).collect();
    with_issues.sort_by(|a, b| (b.audit_issues.len() as u32).cmp(&(a.audit_issues.len() as u32)));
    let chapters_with_most_issues: Vec<ChapterIssueCount> = with_issues
        .into_iter()
        .take(5)
        .map(|c| ChapterIssueCount { chapter: c.number, issue_count: c.audit_issues.len() as u32 })
        .collect();

    // 状态分布
    let mut status_distribution: std::collections::HashMap<String, u32> = std::collections::HashMap::new();
    for ch in chapters {
        *status_distribution.entry(ch.status.clone()).or_insert(0) += 1;
    }

    // token 统计（仅有 tokenUsage 的章节参与）
    let chapters_with_usage: Vec<&AnalyticsChapter> = chapters.iter().filter(|c| c.token_usage.is_some()).collect();
    let token_stats = if !chapters_with_usage.is_empty() {
        let total_prompt_tokens: u64 = chapters_with_usage.iter().map(|c| c.token_usage.as_ref().unwrap().prompt_tokens).sum();
        let total_completion_tokens: u64 = chapters_with_usage.iter().map(|c| c.token_usage.as_ref().unwrap().completion_tokens).sum();
        let total_tokens: u64 = chapters_with_usage.iter().map(|c| c.token_usage.as_ref().unwrap().total_tokens).sum();
        let avg_tokens_per_chapter = total_tokens / chapters_with_usage.len() as u64;
        let mut sorted_usage = chapters_with_usage.clone();
        sorted_usage.sort_by_key(|c| c.number);
        let recent_trend: Vec<ChapterTokenPoint> = sorted_usage
            .iter()
            .rev()
            .take(5)
            .collect::<Vec<_>>()
            .into_iter()
            .rev()
            .map(|c| ChapterTokenPoint { chapter: c.number, total_tokens: c.token_usage.as_ref().unwrap().total_tokens })
            .collect();
        Some(TokenStats { total_prompt_tokens, total_completion_tokens, total_tokens, avg_tokens_per_chapter, recent_trend })
    } else {
        None
    };

    AnalyticsData {
        book_id: book_id.to_string(),
        total_chapters,
        total_words,
        avg_words_per_chapter,
        audit_pass_rate,
        top_issue_categories,
        chapters_with_most_issues,
        status_distribution,
        token_stats,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ch(n: u32, status: &str, words: u64, issues: &[&str], usage: Option<AnalyticsTokenUsage>) -> AnalyticsChapter {
        AnalyticsChapter {
            number: n,
            status: status.into(),
            word_count: words,
            audit_issues: issues.iter().map(|s| s.to_string()).collect(),
            token_usage: usage,
        }
    }

    #[test]
    fn empty_chapters_default_rate() {
        let a = compute_analytics("b1", &[]);
        assert_eq!(a.total_chapters, 0);
        assert_eq!(a.audit_pass_rate, 100);
        assert!(a.token_stats.is_none());
    }

    #[test]
    fn aggregates_words_and_pass_rate() {
        let chapters = vec![
            ch(1, "approved", 3000, &[], None),
            ch(2, "rejected", 2000, &["[critical] 节奏: 太慢"], None),
            ch(3, "drafting", 1000, &[], None), // 不计入 audited
        ];
        let a = compute_analytics("b", &chapters);
        assert_eq!(a.total_chapters, 3);
        assert_eq!(a.total_words, 6000);
        assert_eq!(a.avg_words_per_chapter, 2000);
        // audited = approved + rejected = 2; passed = approved = 1 → 50%
        assert_eq!(a.audit_pass_rate, 50);
    }

    #[test]
    fn top_issue_categories_extract_and_rank() {
        let chapters = vec![
            ch(1, "approved", 0, &["[critical] 节奏: a", "[warning] 节奏: b", "[info] 对话: c"], None),
            ch(2, "approved", 0, &["[critical] 节奏: d"], None),
        ];
        let a = compute_analytics("b", &chapters);
        assert_eq!(a.top_issue_categories[0].category, "节奏");
        assert_eq!(a.top_issue_categories[0].count, 3);
        assert_eq!(a.top_issue_categories[1].category, "对话");
    }

    #[test]
    fn chapters_with_most_issues_sorted_desc() {
        let chapters = vec![
            ch(1, "approved", 0, &["x", "y", "z"], None),
            ch(2, "approved", 0, &["x"], None),
            ch(3, "approved", 0, &["x", "y"], None),
        ];
        let a = compute_analytics("b", &chapters);
        assert_eq!(a.chapters_with_most_issues[0].chapter, 1);
        assert_eq!(a.chapters_with_most_issues[0].issue_count, 3);
        assert_eq!(a.chapters_with_most_issues[1].chapter, 3);
    }

    #[test]
    fn status_distribution_counts() {
        let chapters = vec![
            ch(1, "approved", 0, &[], None),
            ch(2, "approved", 0, &[], None),
            ch(3, "drafting", 0, &[], None),
        ];
        let a = compute_analytics("b", &chapters);
        assert_eq!(a.status_distribution.get("approved"), Some(&2));
        assert_eq!(a.status_distribution.get("drafting"), Some(&1));
    }

    #[test]
    fn token_stats_recent_trend_last_five() {
        let usage = |t: u64| AnalyticsTokenUsage { prompt_tokens: t, completion_tokens: 0, total_tokens: t };
        let chapters: Vec<AnalyticsChapter> = (1..=7u32).map(|n| ch(n, "approved", 0, &[], Some(usage(n as u64 * 100)))).collect();
        let a = compute_analytics("b", &chapters);
        let ts = a.token_stats.unwrap();
        assert_eq!(ts.recent_trend.len(), 5);
        assert_eq!(ts.recent_trend[0].chapter, 3); // 末 5：3..7
        assert_eq!(ts.recent_trend[4].chapter, 7);
        assert_eq!(ts.avg_tokens_per_chapter, 400); // (100+200+...+700)/7 = 2800/7 = 400
    }
}
