//! 365 号：反AI规则资产 + G13 经验条目共享 golden 差分（R5，二轮 P1）。
//!
//! 唯一事实源 = `packages/core/src/__tests__/golden/rule-experience-vectors.json`；
//! core 侧 `src/__tests__/golden-rule-experience.test.ts` 断言同文件。

use inkos_engine::utils::rule_experience_engine::{
    anti_ai_rule_canonical_form, anti_ai_rule_content_hash, anti_ai_rule_pack_hash,
    anti_ai_rule_seeds, compose_anti_ai_guidance, merge_experience_entries,
    render_anti_ai_fix_guidance, render_experience_guidance, resolve_anti_ai_rules_with_seeds,
    scan_anti_ai_rules, validate_anti_ai_rule, AntiAiHit, AntiAiRule, AntiAiRuleType,
    ExperienceEntry, GuidanceLanguage,
};
use serde_json::Value;

const VECTORS: &str =
    include_str!("../../packages/core/src/__tests__/golden/rule-experience-vectors.json");

fn language_of(raw: &str) -> GuidanceLanguage {
    match raw {
        "en" => GuidanceLanguage::En,
        _ => GuidanceLanguage::Zh,
    }
}

fn as_rule(raw: &Value) -> AntiAiRule {
    validate_anti_ai_rule(raw)
        .rule
        .unwrap_or_else(|| panic!("golden rule should be valid"))
}

#[test]
fn validate_matches_shared_vectors() {
    let vectors: Value = serde_json::from_str(VECTORS).unwrap();
    for vector in vectors["validate"].as_array().expect("validate array") {
        let result = validate_anti_ai_rule(&vector["input"]);
        let valid = result.errors.is_empty();
        assert_eq!(
            valid,
            vector["valid"].as_bool().unwrap(),
            "validate vector '{}' validity drifted",
            vector["name"].as_str().unwrap()
        );
        if let Some(field) = vector["errorField"].as_str() {
            assert!(
                result.errors.iter().any(|message| message.starts_with(field)),
                "validate vector '{}' error field drifted",
                vector["name"].as_str().unwrap()
            );
        }
    }
}

#[test]
fn scan_matches_shared_vectors() {
    let vectors: Value = serde_json::from_str(VECTORS).unwrap();
    for vector in vectors["scan"].as_array().expect("scan array") {
        let rules: Vec<AntiAiRule> = vector["rules"]
            .as_array()
            .unwrap()
            .iter()
            .map(as_rule)
            .collect();
        let got = scan_anti_ai_rules(vector["text"].as_str().unwrap(), &rules);
        let got_json = serde_json::to_value(&got).unwrap();
        assert_eq!(
            got_json, vector["expected"],
            "scan vector '{}' drifted",
            vector["name"].as_str().unwrap()
        );
    }
}

#[test]
fn fix_guidance_matches_shared_vectors() {
    let vectors: Value = serde_json::from_str(VECTORS).unwrap();
    for vector in vectors["fixGuidance"].as_array().expect("fixGuidance array") {
        let hits: Vec<AntiAiHit> =
            serde_json::from_value(vector["hits"].clone()).expect("hits shape");
        let got = render_anti_ai_fix_guidance(&hits, language_of(vector["language"].as_str().unwrap()));
        let expected = &vector["expected"];
        if expected.is_null() {
            assert!(got.is_none(), "fix vector '{}' should be none", vector["name"].as_str().unwrap());
            continue;
        }
        assert_eq!(
            got.as_deref(),
            expected.as_str(),
            "fix vector '{}' drifted",
            vector["name"].as_str().unwrap()
        );
    }
}

#[test]
fn anti_ai_guidance_matches_shared_vectors() {
    let vectors: Value = serde_json::from_str(VECTORS).unwrap();
    for vector in vectors["antiAiGuidance"].as_array().expect("guidance array") {
        let rules: Vec<AntiAiRule> = vector["rules"]
            .as_array()
            .unwrap()
            .iter()
            .map(as_rule)
            .collect();
        let max_chars = vector["maxChars"].as_u64().map(|value| value as usize);
        let got = compose_anti_ai_guidance(&rules, language_of(vector["language"].as_str().unwrap()), max_chars);
        assert_eq!(
            got.as_deref(),
            vector["expected"].as_str(),
            "guidance vector '{}' drifted",
            vector["name"].as_str().unwrap()
        );
    }
}

#[test]
fn experience_matches_shared_vectors() {
    let vectors: Value = serde_json::from_str(VECTORS).unwrap();
    for vector in vectors["experience"].as_array().expect("experience array") {
        let name = vector["name"].as_str().unwrap();
        if let Some(existing) = vector["existing"].as_array() {
            let existing: Vec<ExperienceEntry> = existing
                .iter()
                .map(|raw| serde_json::from_value(raw.clone()).expect("entry shape"))
                .collect();
            let incoming: Vec<ExperienceEntry> = vector["incoming"]
                .as_array()
                .unwrap()
                .iter()
                .map(|raw| serde_json::from_value(raw.clone()).expect("entry shape"))
                .collect();
            let merged = merge_experience_entries(&existing, &incoming);
            let got_ids: Vec<String> = merged.merged.iter().map(|entry| entry.id.clone()).collect();
            let expected_ids: Vec<String> = vector["expectedIds"]
                .as_array()
                .unwrap()
                .iter()
                .map(|value| value.as_str().unwrap().to_string())
                .collect();
            assert_eq!(got_ids, expected_ids, "merge vector '{name}' drifted");
            continue;
        }
        let entries: Vec<ExperienceEntry> = vector["entries"]
            .as_array()
            .unwrap()
            .iter()
            .map(|raw| serde_json::from_value(raw.clone()).expect("entry shape"))
            .collect();
        let max_chars = vector["maxChars"].as_u64().map(|value| value as usize);
        let got = render_experience_guidance(&entries, language_of(vector["language"].as_str().unwrap()), max_chars);
        assert_eq!(
            got.as_deref(),
            vector["expected"].as_str(),
            "render vector '{name}' drifted"
        );
    }
}

// ── R22/392 号：反AI规则种子包差分 ──

#[test]
fn seed_pack_matches_shared_contract() {
    let vectors: Value = serde_json::from_str(VECTORS).unwrap();
    let seeds = anti_ai_rule_seeds();
    let seeds_section = &vectors["seeds"];
    assert_eq!(
        seeds.len(),
        seeds_section["count"].as_u64().unwrap() as usize,
        "seed pack count drifted"
    );
    let got_ids: Vec<String> = seeds.iter().map(|rule| rule.id.clone()).collect();
    let expected_ids: Vec<String> = seeds_section["ids"]
        .as_array()
        .unwrap()
        .iter()
        .map(|value| value.as_str().unwrap().to_string())
        .collect();
    assert_eq!(got_ids, expected_ids, "seed pack ids drifted");
    let mut got_types: Vec<String> = seeds
        .iter()
        .map(|rule| rule_type_str_for_test(&rule.r#type))
        .collect();
    got_types.sort();
    got_types.dedup();
    let mut expected_types: Vec<String> = seeds_section["typesCovered"]
        .as_array()
        .unwrap()
        .iter()
        .map(|value| value.as_str().unwrap().to_string())
        .collect();
    expected_types.sort();
    assert_eq!(got_types, expected_types, "seed type coverage drifted");
    // 每条种子都必须通过自家校验器（种子即合法规则）。
    for rule in &seeds {
        let raw = serde_json::to_value(rule).unwrap();
        let result = validate_anti_ai_rule(&raw);
        assert!(
            result.errors.is_empty(),
            "seed '{}' should validate: {:?}",
            rule.id,
            result.errors
        );
    }
    // 种子包完整性锚点：逐条 canonical 以 \n 连接后的 FNV 指纹（Python 独立计算）。
    assert_eq!(
        anti_ai_rule_pack_hash(&seeds),
        seeds_section["packHash"].as_str().unwrap(),
        "seed pack hash drifted"
    );
}

fn rule_type_str_for_test(rule_type: &AntiAiRuleType) -> String {
    let text = serde_json::to_value(rule_type).unwrap();
    text.as_str().unwrap().to_string()
}

#[test]
fn seed_canonical_and_hash_match_shared_vectors() {
    let vectors: Value = serde_json::from_str(VECTORS).unwrap();
    let seeds = anti_ai_rule_seeds();
    for section in ["seedCanonical", "seedHash"] {
        for vector in vectors[section].as_array().expect("seed section array") {
            let name = vector["name"].as_str().unwrap();
            let rule_id = vector["ruleId"].as_str().unwrap();
            let seed = seeds
                .iter()
                .find(|rule| rule.id == rule_id)
                .unwrap_or_else(|| panic!("seed '{rule_id}' missing from pack"));
            if section == "seedCanonical" {
                assert_eq!(
                    anti_ai_rule_canonical_form(seed),
                    vector["expected"].as_str().unwrap(),
                    "canonical vector '{name}' drifted"
                );
            } else {
                assert_eq!(
                    anti_ai_rule_content_hash(seed),
                    vector["expected"].as_str().unwrap(),
                    "hash vector '{name}' drifted"
                );
            }
        }
    }
}

#[test]
fn seed_fallback_matches_shared_vectors() {
    let vectors: Value = serde_json::from_str(VECTORS).unwrap();
    for vector in vectors["seedFallback"].as_array().expect("fallback array") {
        let name = vector["name"].as_str().unwrap();
        let parsed: Option<Vec<AntiAiRule>> = match vector["parsed"].as_array() {
            Some(items) => Some(
                items
                    .iter()
                    .map(|raw| serde_json::from_value(raw.clone()).expect("rule shape"))
                    .collect(),
            ),
            // JSON null = 文件缺失/损坏（TS 侧 undefined）。
            None => None,
        };
        let got = resolve_anti_ai_rules_with_seeds(parsed.clone());
        assert_eq!(
            got.seeded,
            vector["seeded"].as_bool().unwrap(),
            "fallback vector '{name}' seeded drifted"
        );
        assert_eq!(
            got.rules.len(),
            vector["ruleCount"].as_u64().unwrap() as usize,
            "fallback vector '{name}' count drifted"
        );
        if vector["parsed"].is_null() {
            assert_eq!(
                got.rules,
                anti_ai_rule_seeds(),
                "fallback vector '{name}' must yield the seed pack"
            );
        } else {
            assert_eq!(
                got.rules,
                parsed.unwrap(),
                "fallback vector '{name}' must pass rules through"
            );
        }
    }
}
