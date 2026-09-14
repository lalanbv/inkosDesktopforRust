//! 拆书工作台契约（G5/350 号，Phase C 第二大件首批）。
//!
//! TS 真源：`packages/core/src/utils/deconstruction.ts`；共享向量：
//! `packages/core/src/__tests__/golden/deconstruction-vectors.json`
//! （差分测试 `tests/golden_deconstruction_diff.rs`）。
//!
//! 四档人物档案 / 章节证据回溯 / 节奏卖点统计 / 参考资料兼容导出，
//! 语义详见 TS 模块 doc。

use serde::Deserialize;
use serde::Serialize;

pub const DECONSTRUCTION_DEPTHS: [&str; 4] = ["brief", "standard", "deep", "full"];

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DeconChapter {
    pub chapter: i64,
    #[serde(default)]
    pub characters: Vec<String>,
    #[serde(default)]
    pub events: String,
    #[serde(default)]
    pub chapter_type: Option<String>,
    #[serde(default)]
    pub hook_activity: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct EvidenceEntry {
    pub chapter: i64,
    pub evidence: String,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct EvidenceIndex {
    /// 角色首次出现序（对齐 TS 对象插入序；BTreeMap 会按码元重序破坏 duel 一致）。
    pub by_character: Vec<(String, Vec<EvidenceEntry>)>,
    pub chapter_types: Vec<(i64, String)>,
}

/// 证据索引构建（章号升序稳定；events 空白行不入索引）。
pub fn build_evidence_index(chapters: &[DeconChapter]) -> EvidenceIndex {
    let mut ordered: Vec<&DeconChapter> = chapters.iter().collect();
    ordered.sort_by_key(|chapter| chapter.chapter);

    let mut by_character: Vec<(String, Vec<EvidenceEntry>)> = Vec::new();
    let mut chapter_types: Vec<(i64, String)> = Vec::new();
    for chapter in ordered {
        let evidence = chapter.events.trim().to_string();
        if !evidence.is_empty() {
            for character in &chapter.characters {
                let name = character.trim().to_string();
                if name.is_empty() {
                    continue;
                }
                let slot = by_character
                    .iter_mut()
                    .find(|(existing, _)| existing == &name)
                    .map(|(_, entries)| entries);
                match slot {
                    Some(entries) => entries.push(EvidenceEntry {
                        chapter: chapter.chapter,
                        evidence: evidence.clone(),
                    }),
                    None => by_character.push((
                        name,
                        vec![EvidenceEntry {
                            chapter: chapter.chapter,
                            evidence: evidence.clone(),
                        }],
                    )),
                }
            }
        }
        if let Some(kind) = chapter.chapter_type.as_ref().map(|kind| kind.trim().to_string()).filter(|kind| !kind.is_empty()) {
            chapter_types.push((chapter.chapter, kind));
        }
    }
    EvidenceIndex { by_character, chapter_types }
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CharacterDossier {
    pub name: String,
    pub appearances: Vec<i64>,
    pub first_chapter: i64,
    pub last_chapter: i64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub frequency: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub longest_absence: Option<i64>,
    pub evidence: Vec<EvidenceEntry>,
}

/// 四档档案推导：depth 决定 detail 字段填充与证据截断
/// （brief 0 条 / standard 3 条 / deep 10 条 / full 全量）。
pub fn derive_character_dossier(
    index: &EvidenceIndex,
    name: &str,
    depth: &str,
    total_chapters: Option<i64>,
) -> CharacterDossier {
    let empty: Vec<EvidenceEntry> = Vec::new();
    let evidence_full = index
        .by_character
        .iter()
        .find(|(existing, _)| existing == name)
        .map(|(_, entries)| entries)
        .unwrap_or(&empty);
    let appearances: Vec<i64> = evidence_full.iter().map(|entry| entry.chapter).collect();
    let first_chapter = appearances.first().copied().unwrap_or(0);
    let last_chapter = appearances.last().copied().unwrap_or(0);

    let mut longest_absence: i64 = 0;
    for pair in appearances.windows(2) {
        longest_absence = longest_absence.max(pair[1] - pair[0] - 1);
    }
    let span = total_chapters.unwrap_or(if last_chapter == 0 { 1 } else { last_chapter });
    let frequency = if span > 0 && !appearances.is_empty() {
        ((appearances.len() as f64 / span as f64) * 10.0).round() / 10.0
    } else {
        0.0
    };

    let cap = match depth {
        "brief" => 0usize,
        "standard" => 3,
        "deep" => 10,
        _ => usize::MAX,
    };
    CharacterDossier {
        name: name.to_string(),
        appearances,
        first_chapter,
        last_chapter,
        frequency: (depth == "deep" || depth == "full").then_some(frequency),
        longest_absence: (depth == "deep" || depth == "full").then_some(longest_absence),
        evidence: evidence_full.iter().take(cap).cloned().collect(),
    }
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LongestRun {
    pub chapter_type: String,
    pub length: i64,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PacingStats {
    pub counts: std::collections::BTreeMap<String, i64>,
    pub longest_run: LongestRun,
    pub strong_hook_density: f64,
}

/// 节奏/卖点统计：章型分布 + 最长连续同型 + 强钩密度（非空 hookActivity 占比）。
pub fn analyze_pacing_stats(chapters: &[DeconChapter]) -> PacingStats {
    let mut counts: std::collections::BTreeMap<String, i64> = std::collections::BTreeMap::new();
    let mut longest_type = String::new();
    let mut longest_length: i64 = 0;
    let mut run_type = String::new();
    let mut run_length: i64 = 0;
    let mut strong = 0i64;

    for chapter in chapters {
        let kind = chapter
            .chapter_type
            .as_ref()
            .map(|kind| kind.trim().to_string())
            .filter(|kind| !kind.is_empty())
            .unwrap_or_else(|| "untyped".to_string());
        *counts.entry(kind.clone()).or_insert(0) += 1;
        if kind == run_type {
            run_length += 1;
        } else {
            run_type = kind.clone();
            run_length = 1;
        }
        if run_length > longest_length {
            longest_length = run_length;
            longest_type = run_type.clone();
        }
        if !chapter.hook_activity.as_ref().map(|text| text.trim().is_empty()).unwrap_or(true) {
            strong += 1;
        }
    }
    let total = chapters.len().max(1) as f64;
    PacingStats {
        counts,
        longest_run: LongestRun { chapter_type: longest_type, length: longest_length },
        strong_hook_density: ((strong as f64 / total) * 10.0).round() / 10.0,
    }
}

/// 导出渲染：参考资料兼容 markdown（`## Extracted content` 后为拆书正文）。
pub fn render_deconstruction_export(
    dossiers: &[CharacterDossier],
    pacing: &PacingStats,
    language: &str,
) -> String {
    let is_en = language == "en";
    let header = if is_en {
        "# Deconstruction Export"
    } else {
        "# 拆书产物（人物档案 / 节奏 / 卖点）"
    };
    let mut body = vec![header.to_string(), String::new(), "## Extracted content".into()];
    body.push(if is_en { "### Character dossiers".into() } else { "### 人物档案".into() });
    for dossier in dossiers {
        body.push(format!("### {}", dossier.name));
        let appearances = if dossier.appearances.is_empty() {
            "-".to_string()
        } else {
            dossier
                .appearances
                .iter()
                .map(|chapter| chapter.to_string())
                .collect::<Vec<_>>()
                .join(", ")
        };
        body.push(if is_en {
            format!(
                "- Chapters: {appearances} | span {}–{} | frequency {}",
                dossier.first_chapter, dossier.last_chapter,
                dossier.frequency.unwrap_or(0.0)
            )
        } else {
            format!(
                "- 出场章：{appearances}｜跨度 {}–{}｜出场率 {}",
                dossier.first_chapter, dossier.last_chapter,
                dossier.frequency.unwrap_or(0.0)
            )
        });
        for entry in &dossier.evidence {
            body.push(format!("  - ch{}: {}", entry.chapter, entry.evidence));
        }
    }
    body.push(String::new());
    body.push(if is_en {
        format!(
            "### Pacing: longest same-type run = {} ({}); strong-hook density {}",
            pacing.longest_run.length, pacing.longest_run.chapter_type, pacing.strong_hook_density
        )
    } else {
        format!(
            "### 节奏：最长连续同型 {} 章（{}）；强钩密度 {}",
            pacing.longest_run.length, pacing.longest_run.chapter_type, pacing.strong_hook_density
        )
    });
    body.join("\n")
}

/// 机器可读契约（双端 golden 锁形状）。
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DeconstructionContract {
    pub depths: Vec<&'static str>,
    pub evidence_caps: std::collections::BTreeMap<&'static str, usize>,
    pub export_marker: &'static str,
    pub consumed_by: &'static str,
}

pub fn deconstruction_contract() -> DeconstructionContract {
    let mut evidence_caps = std::collections::BTreeMap::new();
    evidence_caps.insert("brief", 0usize);
    evidence_caps.insert("standard", 3usize);
    evidence_caps.insert("deep", 10usize);
    DeconstructionContract {
        depths: DECONSTRUCTION_DEPTHS.to_vec(),
        evidence_caps,
        export_marker: "## Extracted content",
        consumed_by: "reference-context.extractMaterialContent",
    }
}

// ── 一括聚合（统一拆书面端点核心）──

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DeconstructionResult {
    pub index: EvidenceIndex,
    pub dossiers: Vec<CharacterDossier>,
    pub pacing: PacingStats,
    pub markdown: String,
}

/// 一括聚合：按出场证据数取前 top_characters 个角色的 full 档 + 节奏统计
/// 渲染为可发布 markdown。
pub fn build_deconstruction_export(
    chapters: &[DeconChapter],
    _depth: &str,
    language: &str,
    top_characters: Option<usize>,
) -> DeconstructionResult {
    let index = build_evidence_index(chapters);
    let pacing = analyze_pacing_stats(chapters);
    let top = top_characters.unwrap_or(5).max(1);
    let mut order: Vec<(String, usize)> = index
        .by_character
        .iter()
        .map(|(name, entries)| (name.clone(), entries.len()))
        .collect();
    order.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(&b.0)));
    let dossiers: Vec<CharacterDossier> = order
        .into_iter()
        .take(top)
        .map(|(name, _)| derive_character_dossier(&index, &name, "full", None))
        .collect();
    let markdown = render_deconstruction_export(&dossiers, &pacing, language);
    DeconstructionResult { index, dossiers, pacing, markdown }
}
