//! 信息差账本（G7a/341 号，Phase B 批次二）。
//!
//! TS 真源：`packages/core/src/utils/info-gap-ledger.ts`；共享向量：
//! `packages/core/src/__tests__/golden/info-gap-vectors.json`
//! （差分测试 `tests/golden_info_gap_diff.rs`）。
//!
//! truth 类目 `story/info_gaps.md`（作者可手改）+ 泄密机检 / 废笔机检
//! （continuity 审计维度 38/39 的本地预扫描）。

use serde::Deserialize;
use serde::Serialize;

pub const DIALOGUE_WINDOW_CHARS: usize = 24;

#[derive(Debug, Clone, PartialEq, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct InfoGapEntry {
    pub id: String,
    #[serde(default)]
    pub secret: String,
    #[serde(default)]
    pub knows: Vec<String>,
    #[serde(default)]
    pub reader_knows: bool,
    #[serde(default)]
    pub registered_at: i64,
    #[serde(default)]
    pub keywords: Vec<String>,
}

const GAP_HEADING: &str = "## gap-";

/// 解析信息差账本 markdown（作者手改容错：缺字段用空值，坏节跳过）。
pub fn parse_info_gaps_markdown(markdown: &str) -> Vec<InfoGapEntry> {
    let mut entries: Vec<InfoGapEntry> = Vec::new();
    let mut current_id: Option<String> = None;
    let mut fields: std::collections::HashMap<String, String> = std::collections::HashMap::new();
    let mut has_current = false;

    macro_rules! flush {
        () => {
            if has_current {
                let secret = fields.get("secret").map(|s| s.trim().to_string()).unwrap_or_default();
                let knows = split_list(fields.get("knows").map(String::as_str).unwrap_or(""));
                let reader_knows = matches!(
                    fields.get("readerKnows").map(String::as_str).unwrap_or("").trim().to_lowercase().as_str(),
                    "true" | "是" | "yes" | "1"
                );
                let registered_at = fields
                    .get("registeredAt")
                    .map(String::as_str)
                    .unwrap_or("")
                    .trim()
                    .parse::<i64>()
                    .unwrap_or(0);
                let keywords = split_list(fields.get("keywords").map(String::as_str).unwrap_or(""));
                if !secret.is_empty() || !keywords.is_empty() {
                    entries.push(InfoGapEntry {
                        id: current_id.clone().unwrap_or_default(),
                        secret,
                        knows,
                        reader_knows,
                        registered_at,
                        keywords,
                    });
                }
            }
            has_current = false;
        };
    }

    for raw_line in markdown.lines() {
        let line = raw_line.trim();
        if let Some(rest) = line.strip_prefix("## ") {
            let name = rest.trim();
            if name.starts_with(GAP_HEADING.trim_start_matches("## ")) && name.len() >= 5 {
                flush!();
                current_id = Some(name.to_string());
                has_current = true;
                fields.clear();
                continue;
            }
        }
        if !has_current {
            continue;
        }
        if let Some(rest) = line.strip_prefix("- ") {
            if let Some(colon) = rest.find(':') {
                let key = rest[..colon].trim().to_string();
                let value = rest[colon + 1..].trim_start().to_string();
                fields.insert(key, value);
            }
        }
    }
    flush!();
    entries
}

fn split_list(value: &str) -> Vec<String> {
    value
        .split([',', '，', '、', ';', '；'])
        .map(|item| item.trim().to_string())
        .filter(|item| !item.is_empty())
        .collect()
}

/// 渲染信息差账本（防呆方言：每条一节、平铺 key: value 行）。
pub fn render_info_gaps_markdown(entries: &[InfoGapEntry]) -> String {
    let sections: Vec<String> = entries
        .iter()
        .map(|entry| {
            format!(
                "## {}\n- secret: {}\n- knows: {}\n- readerKnows: {}\n- registeredAt: {}\n- keywords: {}",
                entry.id,
                entry.secret,
                entry.knows.join(", "),
                entry.reader_knows,
                entry.registered_at,
                entry.keywords.join(", "),
            )
        })
        .collect();
    let mut parts = vec!["# 信息差账本（Info Gaps）".to_string()];
    parts.extend(sections);
    parts.join("\n\n")
}

// ── 机检 ──

fn has_dialogue_mark(text: &str) -> bool {
    text.chars().any(|c| "「」『』“”‘’\"'".contains(c))
}

fn count_occurrences(content: &str, keyword: &str) -> u32 {
    if keyword.is_empty() {
        return 0;
    }
    let lower = content.to_lowercase();
    let needle = keyword.to_lowercase();
    let mut count = 0u32;
    let mut index = 0usize;
    while let Some(found) = lower[index..].find(&needle) {
        count += 1;
        index += found + needle.len();
    }
    count
}

/// 命中窗口（前后 window 字）是否含对白引号。
fn window_has_dialogue(content: &str, keyword: &str, window: usize) -> bool {
    let lower = content.to_lowercase();
    let needle = keyword.to_lowercase();
    let needle_chars = keyword.chars().count();
    let mut index = 0usize;
    while let Some(found) = lower[index..].find(&needle) {
        let byte_start = index + found;
        // 字节区间近似 TS 的码元窗口：以字符为单位计算前后窗。
        let char_start = lower[..byte_start].chars().count();
        let window_from = char_start.saturating_sub(window);
        let window_to = char_start + needle_chars + window;
        let window_text: String = content
            .chars()
            .skip(window_from)
            .take(window_to - window_from)
            .collect();
        if has_dialogue_mark(&window_text) {
            return true;
        }
        index += found + needle.len();
    }
    false
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SecretLeakHit {
    pub gap_id: String,
    pub keyword: String,
    pub occurrences: u32,
    pub in_dialogue: bool,
    pub knows: Vec<String>,
}

/// 泄密机检：readerKnows=false 且已登记的秘密关键词命中即候选。
pub fn detect_secret_leaks(
    content: &str,
    gaps: &[InfoGapEntry],
    current_chapter: i64,
) -> Vec<SecretLeakHit> {
    let mut hits: Vec<SecretLeakHit> = Vec::new();
    for gap in gaps {
        if gap.reader_knows || gap.registered_at > current_chapter {
            continue;
        }
        for keyword in &gap.keywords {
            if keyword.is_empty() {
                continue;
            }
            let occurrences = count_occurrences(content, keyword);
            if occurrences == 0 {
                continue;
            }
            let in_dialogue = window_has_dialogue(content, keyword, DIALOGUE_WINDOW_CHARS);
            hits.push(SecretLeakHit {
                gap_id: gap.id.clone(),
                keyword: keyword.clone(),
                occurrences,
                in_dialogue,
                knows: gap.knows.clone(),
            });
        }
    }
    hits.sort_by(|a, b| {
        b.in_dialogue
            .cmp(&a.in_dialogue)
            .then(b.occurrences.cmp(&a.occurrences))
            .then(a.gap_id.cmp(&b.gap_id))
            .then(a.keyword.cmp(&b.keyword))
    });
    hits
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ReaderRedundancyHit {
    pub gap_id: String,
    pub keyword: String,
    pub occurrences: u32,
}

/// 废笔机检：读者已知的秘密关键词再次出现——向读者复述已知信息。
pub fn detect_reader_redundancy(
    content: &str,
    gaps: &[InfoGapEntry],
) -> Vec<ReaderRedundancyHit> {
    let mut hits: Vec<ReaderRedundancyHit> = Vec::new();
    for gap in gaps {
        if !gap.reader_knows {
            continue;
        }
        for keyword in &gap.keywords {
            if keyword.is_empty() {
                continue;
            }
            let occurrences = count_occurrences(content, keyword);
            if occurrences > 0 {
                hits.push(ReaderRedundancyHit {
                    gap_id: gap.id.clone(),
                    keyword: keyword.clone(),
                    occurrences,
                });
            }
        }
    }
    hits.sort_by(|a, b| {
        b.occurrences
            .cmp(&a.occurrences)
            .then(a.gap_id.cmp(&b.gap_id))
            .then(a.keyword.cmp(&b.keyword))
    });
    hits
}

/// 审计注入文本：continuity 维度 38/39 的本地机检摘要（无命中返回 None）。
pub fn render_info_gap_audit_notes(
    leaks: &[SecretLeakHit],
    redundancies: &[ReaderRedundancyHit],
    language: Option<&str>,
) -> Option<String> {
    if leaks.is_empty() && redundancies.is_empty() {
        return None;
    }
    let is_en = language == Some("en");
    let mut lines: Vec<String> = Vec::new();
    if !leaks.is_empty() {
        lines.push(
            if is_en {
                "Secret-leak pre-scan flagged (verify whether the speaker actually knows the secret):"
            } else {
                "泄密机检命中（请核实说话人是否知情）："
            }
            .to_string(),
        );
        for hit in leaks {
            let dialogue_note = if hit.in_dialogue {
                if is_en { " (in dialogue)".to_string() } else { "（对白内）".to_string() }
            } else {
                String::new()
            };
            let knows_note = if is_en {
                String::new()
            } else if hit.knows.is_empty() {
                "；知情人：无".to_string()
            } else {
                format!("；知情人：{}", hit.knows.join("、"))
            };
            lines.push(format!(
                "- [{}] \"{}\" ×{}{}{}",
                hit.gap_id, hit.keyword, hit.occurrences, dialogue_note, knows_note
            ));
        }
    }
    if !redundancies.is_empty() {
        lines.push(
            if is_en {
                "Reader-redundancy pre-scan flagged (reader already knows these):"
            } else {
                "废笔机检命中（读者已知，勿复述）："
            }
            .to_string(),
        );
        for hit in redundancies {
            lines.push(format!("- [{}] \"{}\" ×{}", hit.gap_id, hit.keyword, hit.occurrences));
        }
    }
    Some(lines.join("\n"))
}

/// 机器可读契约（双端 golden 锁形状）。
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct InfoGapContract {
    pub dimensions: Vec<InfoGapDimension>,
    pub truth_file: &'static str,
    pub dialogue_window: usize,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct InfoGapDimension {
    pub id: u32,
    pub zh: String,
    pub en: String,
}

pub fn info_gap_contract() -> InfoGapContract {
    InfoGapContract {
        dimensions: vec![
            InfoGapDimension { id: 38, zh: "泄密机检".into(), en: "Secret Leak Check".into() },
            InfoGapDimension { id: 39, zh: "废笔机检".into(), en: "Reader Redundancy Check".into() },
        ],
        truth_file: "story/info_gaps.md",
        dialogue_window: DIALOGUE_WINDOW_CHARS,
    }
}
