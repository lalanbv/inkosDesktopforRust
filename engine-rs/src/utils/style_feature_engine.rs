//! 写法引擎资产化（G4/339 号，Phase B 批次二首项）。
//!
//! TS 真源：`packages/core/src/utils/style-feature-engine.ts`；共享向量：
//! `packages/core/src/__tests__/golden/style-feature-engine-vectors.json`
//! （差分测试 `tests/golden_style_feature_engine_diff.rs`）。
//!
//! 特征池推导（metric/pattern/rhetoric）→ 启停组合（白名单优先）→
//! 双语 guidance 渲染 → 试写 prompt；附专名泄露检测（长名优先掩码计数）。

use serde::Deserialize;
use serde::Serialize;

use crate::models::style_profile::StyleProfile;

pub const GUIDANCE_ITEM_LIMIT: usize = 12;

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct StyleFeature {
    pub id: String,
    pub kind: &'static str,
    pub label: String,
    #[serde(rename = "guidanceZh")]
    pub guidance_zh: String,
    #[serde(rename = "guidanceEn")]
    pub guidance_en: String,
}

fn round1(value: f64) -> f64 {
    (value * 10.0).round() / 10.0
}

/// 特征池推导：确定性；空列表维度跳过。
pub fn derive_feature_pool(profile: &StyleProfile) -> Vec<StyleFeature> {
    let mut features = Vec::new();
    let sentence_length = round1(profile.avg_sentence_length);
    features.push(StyleFeature {
        id: "metric:sentence-length".into(),
        kind: "metric",
        label: "sentence-length".into(),
        guidance_zh: format!("单句平均长度控制在约 {sentence_length} 字（允许小幅波动）。"),
        guidance_en: format!("Keep the average sentence length near {sentence_length} (small variance allowed)."),
    });
    let paragraph_low = profile.paragraph_length_range.min.round();
    let paragraph_high = profile.paragraph_length_range.max.round();
    features.push(StyleFeature {
        id: "metric:paragraph-length".into(),
        kind: "metric",
        label: "paragraph-length".into(),
        guidance_zh: format!("单段长度大致落在 {paragraph_low}–{paragraph_high} 字区间。"),
        guidance_en: format!("Keep paragraph lengths roughly within {paragraph_low}–{paragraph_high} characters."),
    });
    let diversity_pct = (profile.vocabulary_diversity * 100.0).round() as i64;
    features.push(StyleFeature {
        id: "metric:vocabulary-diversity".into(),
        kind: "metric",
        label: "vocabulary-diversity".into(),
        guidance_zh: format!("保持词汇多样性约 {diversity_pct}%（TTR），避免高频重复用词。"),
        guidance_en: format!("Keep vocabulary diversity near {diversity_pct}% (TTR); avoid repeated word choices."),
    });
    for pattern in &profile.top_patterns {
        features.push(StyleFeature {
            id: format!("pattern:{pattern}"),
            kind: "pattern",
            label: pattern.clone(),
            guidance_zh: format!("写作时体现句式特征「{pattern}」。"),
            guidance_en: format!("Reflect the sentence pattern \"{pattern}\" in the prose."),
        });
    }
    for feature in &profile.rhetorical_features {
        features.push(StyleFeature {
            id: format!("rhetoric:{feature}"),
            kind: "rhetoric",
            label: feature.clone(),
            guidance_zh: format!("适度运用修辞手法「{feature}」，不要堆砌。"),
            guidance_en: format!("Use the rhetorical device \"{feature}\" with restraint."),
        });
    }
    features
}

/// 启停与组合：显式白名单优先；否则全集剔除黑名单；未知 id 忽略。
pub fn apply_feature_selection(
    pool: &[StyleFeature],
    enabled_ids: Option<&[String]>,
    disabled_ids: Option<&[String]>,
) -> Vec<StyleFeature> {
    if let Some(enabled) = enabled_ids.filter(|ids| !ids.is_empty()) {
        let whitelist: std::collections::HashSet<&String> = enabled.iter().collect();
        return pool.iter().filter(|f| whitelist.contains(&f.id)).cloned().collect();
    }
    if let Some(disabled) = disabled_ids.filter(|ids| !ids.is_empty()) {
        let blacklist: std::collections::HashSet<&String> = disabled.iter().collect();
        return pool.iter().filter(|f| !blacklist.contains(&f.id)).cloned().collect();
    }
    pool.to_vec()
}

/// 双语 guidance 段渲染（超出 max_chars 截断，带省略号）。
pub fn compose_style_guidance(
    enabled: &[StyleFeature],
    language: &str,
    max_chars: Option<usize>,
) -> String {
    if enabled.is_empty() {
        return String::new();
    }
    let is_en = language == "en";
    let header = if is_en {
        "## Style features (bound profile — follow while writing)"
    } else {
        "## 写法特征（绑定档案——写作时遵循）"
    };
    let mut lines = vec![header.to_string()];
    for feature in enabled.iter().take(GUIDANCE_ITEM_LIMIT) {
        lines.push(format!(
            "- {}",
            if is_en { &feature.guidance_en } else { &feature.guidance_zh }
        ));
    }
    let text = lines.join("\n");
    match max_chars {
        Some(max) if text.chars().count() > max => {
            let cut: String = text.chars().take(max.saturating_sub(1)).collect();
            format!("{cut}…")
        }
        _ => text,
    }
}

/// 试写入口：绑定特征 + 场景前提 → 试写提示词（单块输出，禁止越题）。
pub fn build_trial_write_prompt(
    guidance: &str,
    premise: &str,
    scene_brief: &str,
    target_chars: u32,
    language: &str,
) -> String {
    if language == "en" {
        format!(
            "Trial-write a passage under the bound style profile.\n\n{}\n\n## Premise\n{}\n\n## Scene brief\n{}\n\nRequirements:\n- Around {} characters of prose\n- Output a single TRIAL_CONTENT block and nothing else\n- Do not step outside the scene brief; do not invent named characters beyond it",
            if guidance.is_empty() { "(no style features bound)" } else { guidance },
            premise,
            scene_brief,
            target_chars
        )
    } else {
        format!(
            "按绑定的写法档案试写一段正文。\n\n{}\n\n## 前提\n{}\n\n## 场景简报\n{}\n\n要求：\n- 正文约 {} 字\n- 只输出一个 TRIAL_CONTENT 区块，不要输出其他内容\n- 不要超出场景简报的范围；不要自行新增具名角色",
            if guidance.is_empty() { "（未绑定任何写法特征）" } else { guidance },
            premise,
            scene_brief,
            target_chars
        )
    }
}

// ── 每书绑定 ──

#[derive(Debug, Clone, Default, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct StyleBinding {
    #[serde(default)]
    pub profile_name: String,
    #[serde(default)]
    pub enabled_ids: Vec<String>,
    #[serde(default)]
    pub disabled_ids: Vec<String>,
    #[serde(default, rename = "maxGuidanceChars")]
    pub max_guidance_chars: Option<usize>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BindingResolution {
    pub profile_name: String,
    pub pool_size: usize,
    pub enabled: Vec<StyleFeature>,
    #[serde(rename = "guidanceZh")]
    pub guidance_zh: String,
    #[serde(rename = "guidanceEn")]
    pub guidance_en: String,
}

/// 绑定解析：按 sourceName 命中档案 → 池 → 启停 → 双语 guidance；未命中返回 None。
pub fn resolve_style_binding(
    profiles: &[StyleProfile],
    binding: &StyleBinding,
) -> Option<BindingResolution> {
    let profile = profiles
        .iter()
        .find(|profile| profile.source_name.as_deref() == Some(binding.profile_name.as_str()))?;
    let pool = derive_feature_pool(profile);
    let enabled = apply_feature_selection(
        &pool,
        Some(&binding.enabled_ids),
        Some(&binding.disabled_ids),
    );
    if enabled.is_empty() {
        return None;
    }
    Some(BindingResolution {
        profile_name: binding.profile_name.clone(),
        pool_size: pool.len(),
        guidance_zh: compose_style_guidance(&enabled, "zh", binding.max_guidance_chars),
        guidance_en: compose_style_guidance(&enabled, "en", binding.max_guidance_chars),
        enabled,
    })
}

// ── 专名泄露检测（ANWA A9）──

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProperNounLeakHit {
    pub name: String,
    pub occurrences: u32,
}

/// 泄露检测：protectedNames 在正文中的出现计数（大小写不敏感、长名优先防重叠）。
pub fn detect_proper_noun_leak(
    content: &str,
    protected_names: &[String],
    min_occurrences: u32,
) -> Vec<ProperNounLeakHit> {
    if content.is_empty() || protected_names.is_empty() {
        return Vec::new();
    }
    let names: Vec<String> = {
        let mut seen: Vec<String> = Vec::new();
        for name in protected_names {
            let trimmed = name.trim();
            if !trimmed.is_empty() && !seen.iter().any(|x| x == trimmed) {
                seen.push(trimmed.to_string());
            }
        }
        // TS sort((a,b)=>b.length-a.length)：UTF-16 码元长度降序（长名优先掩码）。
        seen.sort_by_key(|s| std::cmp::Reverse(s.chars().count()));
        seen
    };

    let mut masked = content.to_string();
    let mut hits: Vec<ProperNounLeakHit> = Vec::new();
    for name in &names {
        let lower_name = name.to_lowercase();
        let mut occurrences: u32 = 0;
        loop {
            let lower_masked = masked.to_lowercase();
            match lower_masked.find(&lower_name) {
                Some(index) => {
                    occurrences += 1;
                    // TS "\u0000".repeat(name.length)：按 name 的码元数占位防重叠计数。
                    let replacement: String = "\u{0}".repeat(name.chars().count());
                    masked = format!(
                        "{}{replacement}{}",
                        &masked[..index],
                        &masked[index + name.len()..]
                    );
                }
                None => break,
            }
        }
        if occurrences >= min_occurrences {
            hits.push(ProperNounLeakHit { name: name.clone(), occurrences });
        }
    }
    hits.sort_by(|a, b| b.occurrences.cmp(&a.occurrences).then(a.name.cmp(&b.name)));
    hits
}

/// 机器可读契约（双端 golden 锁形状）。
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct StyleFeatureEngineContract {
    pub kinds: Vec<&'static str>,
    #[serde(rename = "metricFeatureIds")]
    pub metric_feature_ids: Vec<&'static str>,
    #[serde(rename = "guidanceItemLimit")]
    pub guidance_item_limit: usize,
    pub defaults: StyleFeatureEngineDefaults,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct StyleFeatureEngineDefaults {
    pub min_occurrences: u32,
}

pub fn style_feature_engine_contract() -> StyleFeatureEngineContract {
    StyleFeatureEngineContract {
        kinds: vec!["metric", "pattern", "rhetoric"],
        metric_feature_ids: vec![
            "metric:sentence-length",
            "metric:paragraph-length",
            "metric:vocabulary-diversity",
        ],
        guidance_item_limit: GUIDANCE_ITEM_LIMIT,
        defaults: StyleFeatureEngineDefaults { min_occurrences: 1 },
    }
}
