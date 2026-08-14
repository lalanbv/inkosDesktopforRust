//! golden 差分测试：消费 TS 真值，断言 Rust 移植实现逐例相等。
//!
//! 真值来源：`packages/core/src/__tests__/golden-leaf-dump.test.ts` 调用**真实 TS 实现**
//! 生成 `tests/golden/utils/leaf.json`。本测试读该 JSON，对 engine-rs 移植实现逐例差分。
//!
//! 任一用例不等 → 差分失败 → 移植有 bug，定位到具体 case 名。
//!
//! 更新真值：`cd packages/core && npx pnpm@9 exec vitest run src/__tests__/golden-leaf-dump.test.ts`
//! 然后提交重新生成的 leaf.json。

use serde::Deserialize;
use serde_json::Value;

/// 通用用例：input/expected 用 serde_json::Value 承载，按域解释。
#[derive(Debug, Deserialize)]
struct Case {
    name: String,
    #[serde(default)]
    input: Value,
    #[serde(default)]
    expected: Value,
}

#[derive(Debug, Deserialize)]
struct LeafGolden {
    derive_book_id: Vec<Case>,
    is_safe_book_id: Vec<Case>,
    infer_language: Vec<Case>,
    to_posix_path: Vec<Case>,
    count_chapter_length: Vec<Case>,
    build_length_spec: Vec<Case>,
    format_length_count: Vec<Case>,
    resolve_length_counting_mode: Vec<Case>,
    resolve_cadence_pressure: Vec<Case>,
    extract_pov_from_outline: Vec<Case>,
    filter_matrix_by_pov: Vec<Case>,
    filter_hooks_by_pov: Vec<Case>,
    split_chapters: Vec<Case>,
    is_high_tension_mood: Vec<Case>,
    analyze_chapter_cadence: Vec<Case>,
    cap_context_block: Vec<Case>,
    filter_hooks: Vec<Case>,
    filter_summaries: Vec<Case>,
    normalize_platform_id: Vec<Case>,
    resolve_chapter_review_mode: Vec<Case>,
    resolve_revision_gate: Vec<Case>,
    parse_memo: Vec<Case>,
    parse_genre_profile: Vec<Case>,
    parse_book_rules: Vec<Case>,
    build_governed_memory_evidence_blocks: Vec<Case>,
    get_fanfic_dimension_config: Vec<Case>,
    is_current_state_seed_placeholder: Vec<Case>,
    build_golden_opening_discipline: Vec<Case>,
    build_fanfic_canon_section: Vec<Case>,
    build_settler_system_prompt: Vec<Case>,
    build_settler_user_prompt: Vec<Case>,
    build_observer_system_prompt: Vec<Case>,
    build_observer_user_prompt: Vec<Case>,
    normalize_post_write_surface: Vec<Case>,
    validate_post_write: Vec<Case>,
    detect_cross_chapter_repetition: Vec<Case>,
    detect_paragraph_length_drift: Vec<Case>,
    detect_duplicate_title: Vec<Case>,
    resolve_duplicate_title: Vec<Case>,
    render_hook_snapshot: Vec<Case>,
    build_governed_hook_working_set: Vec<Case>,
    merge_table_markdown_by_key: Vec<Case>,
    merge_character_matrix_markdown: Vec<Case>,
    build_governed_character_matrix_working_set: Vec<Case>,
    writer_build_user_prompt: Vec<Case>,
    writer_build_governed_user_prompt: Vec<Case>,
    writer_build_chapter_context_block: Vec<Case>,
    writer_build_settler_governed_control_block: Vec<Case>,
    writer_build_length_requirement_block: Vec<Case>,
    writer_sanitize_filename: Vec<Case>,
    writer_extract_dialogue_fingerprints: Vec<Case>,
    writer_find_relevant_summaries: Vec<Case>,
    writer_build_style_fingerprint: Vec<Case>,
    writer_render_delta_summary_row: Vec<Case>,
    writer_normalize_runtime_state_delta_chapter: Vec<Case>,
    compute_recyclable_hooks: Vec<Case>,
    extract_query_terms: Vec<Case>,
    render_summary_snapshot: Vec<Case>,
    planner_system_prompt: Vec<Case>,
    planner_build_user_message: Vec<Case>,
    planner_golden_opening_guidance: Vec<Case>,
    planner_format_recent_summaries: Vec<Case>,
    planner_compose_current_arc_prose: Vec<Case>,
    planner_extract_protagonist_row: Vec<Case>,
    planner_extract_relation_rows: Vec<Case>,
    planner_extract_relevant_threads: Vec<Case>,
    planner_format_recyclable_hooks: Vec<Case>,
    planner_private_suite: Vec<Case>,
    build_governed_rule_stack: Vec<Case>,
    build_governed_trace: Vec<Case>,
    is_protected_context_source: Vec<Case>,
    reviser_private_suite: Vec<Case>,
    length_normalizer_suite: Vec<Case>,
    state_degraded_note: Vec<Case>,
}

const LEAF_JSON: &str = include_str!("golden/utils/leaf.json");

fn load() -> LeafGolden {
    serde_json::from_str(LEAF_JSON).expect("leaf.json 应为合法 JSON")
}

#[test]
fn derive_book_id_matches_ts() {
    for c in &load().derive_book_id {
        let input = c
            .input
            .as_str()
            .unwrap_or_else(|| panic!("case {}: input 非 string", c.name));
        let got = inkos_engine::utils::derive_book_id_from_title(input);
        let want = c
            .expected
            .as_str()
            .unwrap_or_else(|| panic!("case {}: expected 非 string", c.name));
        assert_eq!(got, want, "case `{}`: derive_book_id 与 TS 不一致", c.name);
    }
}

#[test]
fn is_safe_book_id_matches_ts() {
    for c in &load().is_safe_book_id {
        // TS isSafeBookId 对非 string 返回 false（类型守卫）；Rust 侧对称处理
        let got = match &c.input {
            Value::String(s) => inkos_engine::utils::is_safe_book_id(s),
            _ => false,
        };
        let want = c
            .expected
            .as_bool()
            .unwrap_or_else(|| panic!("case {}: expected 非 bool", c.name));
        assert_eq!(
            got, want,
            "case `{}`: is_safe_book_id 与 TS 不一致 (input={:?})",
            c.name, c.input
        );
    }
}

#[test]
fn infer_language_matches_ts() {
    for c in &load().infer_language {
        // TS: string | null | undefined。null/absent → None；string → Some
        let input_opt: Option<&str> = c.input.as_str();
        let got = inkos_engine::utils::infer_language(input_opt);
        let want = c
            .expected
            .as_str()
            .unwrap_or_else(|| panic!("case {}: expected 非 string", c.name));
        let got_str = match got {
            inkos_engine::utils::WritingLanguage::Zh => "zh",
            inkos_engine::utils::WritingLanguage::En => "en",
        };
        assert_eq!(
            got_str, want,
            "case `{}`: infer_language 与 TS 不一致 (input={:?})",
            c.name, c.input
        );
    }
}

#[test]
fn to_posix_path_matches_ts() {
    for c in &load().to_posix_path {
        let input = c
            .input
            .as_str()
            .unwrap_or_else(|| panic!("case {}: input 非 string", c.name));
        let got = inkos_engine::utils::to_posix_path(input);
        let want = c
            .expected
            .as_str()
            .unwrap_or_else(|| panic!("case {}: expected 非 string", c.name));
        assert_eq!(got, want, "case `{}`: to_posix_path 与 TS 不一致", c.name);
    }
}

#[test]
fn count_chapter_length_matches_ts() {
    use inkos_engine::models::length_governance::LengthCountingMode;
    for c in &load().count_chapter_length {
        let content = c.input["content"]
            .as_str()
            .unwrap_or_else(|| panic!("case {}: content 缺失", c.name));
        let mode = match c.input["mode"]
            .as_str()
            .unwrap_or_else(|| panic!("case {}: mode 缺失", c.name))
        {
            "zh_chars" => LengthCountingMode::ZhChars,
            "en_words" => LengthCountingMode::EnWords,
            other => panic!("case {}: 未知 mode {other}", c.name),
        };
        let got = inkos_engine::utils::count_chapter_length(content, mode);
        let want = c
            .expected
            .as_u64()
            .unwrap_or_else(|| panic!("case {}: expected 非 u64", c.name));
        assert_eq!(
            got as u64, want,
            "case `{}`: count_chapter_length 与 TS 不一致",
            c.name
        );
    }
}

#[test]
fn build_length_spec_matches_ts() {
    use inkos_engine::utils::WritingLanguage;
    for c in &load().build_length_spec {
        let target = c.input["target"]
            .as_u64()
            .unwrap_or_else(|| panic!("case {}: target 缺失", c.name)) as u32;
        let lang = match c.input["language"]
            .as_str()
            .unwrap_or_else(|| panic!("case {}: language 缺失", c.name))
        {
            "zh" => WritingLanguage::Zh,
            "en" => WritingLanguage::En,
            other => panic!("case {}: 未知 language {other}", c.name),
        };
        let got = inkos_engine::utils::build_length_spec(target, lang);
        // 序列化为 JSON 后逐字段比对（验证 camelCase 字段名 + 数值都对齐 TS）
        let got_json = serde_json::to_value(&got).expect("LengthSpec 序列化失败");
        assert_eq!(
            got_json, c.expected,
            "case `{}`: build_length_spec 与 TS 不一致",
            c.name
        );
    }
}

#[test]
fn format_length_count_matches_ts() {
    use inkos_engine::models::length_governance::LengthCountingMode;
    for c in &load().format_length_count {
        let count = c.input["count"]
            .as_u64()
            .unwrap_or_else(|| panic!("case {}: count 缺失", c.name)) as u32;
        let mode = match c.input["mode"]
            .as_str()
            .unwrap_or_else(|| panic!("case {}: mode 缺失", c.name))
        {
            "zh_chars" => LengthCountingMode::ZhChars,
            "en_words" => LengthCountingMode::EnWords,
            other => panic!("case {}: 未知 mode {other}", c.name),
        };
        let got = inkos_engine::utils::format_length_count(count, mode);
        let want = c
            .expected
            .as_str()
            .unwrap_or_else(|| panic!("case {}: expected 非 string", c.name));
        assert_eq!(
            got, want,
            "case `{}`: format_length_count 与 TS 不一致",
            c.name
        );
    }
}

#[test]
fn resolve_length_counting_mode_matches_ts() {
    use inkos_engine::utils::WritingLanguage;
    for c in &load().resolve_length_counting_mode {
        let lang = match c
            .input
            .as_str()
            .unwrap_or_else(|| panic!("case {}: input 非 string", c.name))
        {
            "zh" => WritingLanguage::Zh,
            "en" => WritingLanguage::En,
            other => panic!("case {}: 未知 language {other}", c.name),
        };
        let got = inkos_engine::utils::resolve_length_counting_mode(lang);
        let got_str = match got {
            inkos_engine::models::length_governance::LengthCountingMode::ZhChars => "zh_chars",
            inkos_engine::models::length_governance::LengthCountingMode::EnWords => "en_words",
        };
        let want = c
            .expected
            .as_str()
            .unwrap_or_else(|| panic!("case {}: expected 非 string", c.name));
        assert_eq!(
            got_str, want,
            "case `{}`: resolve_length_counting_mode 与 TS 不一致",
            c.name
        );
    }
}

#[test]
fn resolve_cadence_pressure_matches_ts() {
    use inkos_engine::utils::{resolve_cadence_pressure, CadencePressure, CadencePressureParams};
    for c in &load().resolve_cadence_pressure {
        let params = CadencePressureParams {
            count: c.input["count"].as_u64().unwrap_or(0) as u32,
            total: c.input["total"].as_u64().unwrap_or(0) as u32,
            high_threshold: c.input["high"].as_u64().unwrap_or(0) as u32,
            medium_threshold: c.input["medium"].as_u64().unwrap_or(0) as u32,
            medium_window_floor: c.input["floor"].as_u64().unwrap_or(0) as u32,
        };
        let got = resolve_cadence_pressure(params);
        let got_str = got.map(|p| match p {
            CadencePressure::Medium => "medium",
            CadencePressure::High => "high",
        });
        // TS expected 是字符串（"medium"/"high"）或 null
        let want: Option<&str> = match &c.expected {
            Value::String(s) => Some(s.as_str()),
            Value::Null => None,
            _ => panic!("case `{}`: expected 非 string/null", c.name),
        };
        assert_eq!(
            got_str, want,
            "case `{}`: resolve_cadence_pressure 与 TS 不一致",
            c.name
        );
    }
}

#[test]
fn extract_pov_from_outline_matches_ts() {
    use inkos_engine::utils::extract_pov_from_outline;
    for c in &load().extract_pov_from_outline {
        let outline = c.input["outline"]
            .as_str()
            .unwrap_or_else(|| panic!("case {}: outline 缺失", c.name));
        let chapter = c.input["chapter"].as_u64().unwrap_or(0) as u32;
        let got = extract_pov_from_outline(outline, chapter);
        let want: Option<&str> = match &c.expected {
            Value::String(s) => Some(s.as_str()),
            Value::Null => None,
            _ => panic!("case `{}`: expected 非 string/null", c.name),
        };
        assert_eq!(
            got.as_deref(),
            want,
            "case `{}`: extract_pov_from_outline 与 TS 不一致",
            c.name
        );
    }
}

#[test]
fn filter_matrix_by_pov_matches_ts() {
    use inkos_engine::utils::filter_matrix_by_pov;
    for c in &load().filter_matrix_by_pov {
        let matrix = c.input["matrix"]
            .as_str()
            .unwrap_or_else(|| panic!("case {}: matrix 缺失", c.name));
        let pov = c.input["pov"].as_str().unwrap_or_default();
        let got = filter_matrix_by_pov(matrix, pov);
        let want = c
            .expected
            .as_str()
            .unwrap_or_else(|| panic!("case {}: expected 非 string", c.name));
        assert_eq!(
            got, want,
            "case `{}`: filter_matrix_by_pov 与 TS 不一致",
            c.name
        );
    }
}

#[test]
fn filter_hooks_by_pov_matches_ts() {
    use inkos_engine::utils::filter_hooks_by_pov;
    for c in &load().filter_hooks_by_pov {
        let hooks = c.input["hooks"]
            .as_str()
            .unwrap_or_else(|| panic!("case {}: hooks 缺失", c.name));
        let pov = c.input["pov"].as_str().unwrap_or_default();
        let summaries = c.input["summaries"].as_str().unwrap_or_default();
        let got = filter_hooks_by_pov(hooks, pov, summaries);
        let want = c
            .expected
            .as_str()
            .unwrap_or_else(|| panic!("case {}: expected 非 string", c.name));
        assert_eq!(
            got, want,
            "case `{}`: filter_hooks_by_pov 与 TS 不一致",
            c.name
        );
    }
}

#[test]
fn split_chapters_matches_ts() {
    use inkos_engine::utils::split_chapters;
    for c in &load().split_chapters {
        let text = c.input["text"]
            .as_str()
            .unwrap_or_else(|| panic!("case {}: text 缺失", c.name));
        let pattern = c.input["pattern"].as_str(); // null → None
        let got = split_chapters(text, pattern);
        let got_json = serde_json::to_value(&got).expect("SplitChapter vec 序列化失败");
        assert_eq!(
            got_json, c.expected,
            "case `{}`: split_chapters 与 TS 不一致",
            c.name
        );
    }
}

#[test]
fn is_high_tension_mood_matches_ts() {
    use inkos_engine::utils::is_high_tension_mood;
    for c in &load().is_high_tension_mood {
        let input = c
            .input
            .as_str()
            .unwrap_or_else(|| panic!("case {}: input 非 string", c.name));
        let got = is_high_tension_mood(input);
        let want = c
            .expected
            .as_bool()
            .unwrap_or_else(|| panic!("case {}: expected 非 bool", c.name));
        assert_eq!(
            got, want,
            "case `{}`: is_high_tension_mood 与 TS 不一致",
            c.name
        );
    }
}

#[test]
fn analyze_chapter_cadence_matches_ts() {
    use inkos_engine::utils::{analyze_chapter_cadence, CadenceSummaryRow, WritingLanguage};
    for c in &load().analyze_chapter_cadence {
        let lang = match c.input["language"].as_str().unwrap_or("zh") {
            "zh" => WritingLanguage::Zh,
            "en" => WritingLanguage::En,
            _ => WritingLanguage::Zh,
        };
        let rows: Vec<CadenceSummaryRow> = c.input["rows"]
            .as_array()
            .unwrap_or_else(|| panic!("case {}: rows 非数组", c.name))
            .iter()
            .map(|r| CadenceSummaryRow {
                chapter: r["chapter"].as_u64().unwrap_or(0) as u32,
                title: r["title"].as_str().unwrap_or("").to_string(),
                mood: r["mood"].as_str().unwrap_or("").to_string(),
                chapter_type: r["chapterType"].as_str().unwrap_or("").to_string(),
            })
            .collect();
        let got = analyze_chapter_cadence(&rows, lang);
        let got_json = serde_json::to_value(&got).expect("ChapterCadenceAnalysis 序列化失败");
        assert_eq!(
            got_json, c.expected,
            "case `{}`: analyze_chapter_cadence 与 TS 不一致",
            c.name
        );
    }
}

#[test]
fn cap_context_block_matches_ts() {
    use inkos_engine::utils::context_filter::{cap_context_block, ContextCapOptions};
    for c in &load().cap_context_block {
        let content = c.input["content"].as_str().unwrap_or("");
        let label = c.input["label"].as_str().unwrap_or("x");
        let max_chars = c.input["maxChars"].as_u64().unwrap_or(0) as usize;
        let head_ratio = c.input["headRatio"].as_f64();
        let opts = ContextCapOptions {
            label,
            max_chars,
            head_ratio,
        };
        let got = cap_context_block(content, opts);
        let want = c
            .expected
            .as_str()
            .unwrap_or_else(|| panic!("case {}: expected 非 string", c.name));
        assert_eq!(
            got, want,
            "case `{}`: cap_context_block 与 TS 不一致",
            c.name
        );
    }
}

#[test]
fn filter_hooks_matches_ts() {
    use inkos_engine::utils::context_filter::filter_hooks;
    for c in &load().filter_hooks {
        let input = c
            .input
            .as_str()
            .unwrap_or_else(|| panic!("case {}: input 非 string", c.name));
        let got = filter_hooks(input);
        let want = c.expected.as_str().unwrap_or("");
        assert_eq!(got, want, "case `{}`: filter_hooks 与 TS 不一致", c.name);
    }
}

#[test]
fn filter_summaries_matches_ts() {
    use inkos_engine::utils::context_filter::filter_summaries;
    for c in &load().filter_summaries {
        let s = c.input["summaries"].as_str().unwrap_or("");
        let cur = c.input["currentChapter"].as_u64().unwrap_or(0) as u32;
        let keep = c.input["keepRecent"].as_u64().map(|n| n as u32);
        let got = filter_summaries(s, cur, keep);
        let want = c.expected.as_str().unwrap_or("");
        assert_eq!(
            got, want,
            "case `{}`: filter_summaries 与 TS 不一致",
            c.name
        );
    }
}

#[test]
fn normalize_platform_id_matches_ts() {
    use inkos_engine::models::book::{normalize_platform_id, Platform};
    for c in &load().normalize_platform_id {
        let input = c
            .input
            .as_str()
            .unwrap_or_else(|| panic!("case {}: input 非 string", c.name));
        let got = normalize_platform_id(input);
        let got_str = got.map(|p| match p {
            Platform::Tomato => "tomato",
            Platform::Feilu => "feilu",
            Platform::Qidian => "qidian",
            Platform::Other => "other",
        });
        let want: Option<&str> = c.expected.as_str();
        assert_eq!(
            got_str, want,
            "case `{}`: normalize_platform_id 与 TS 不一致",
            c.name
        );
    }
}

#[test]
fn resolve_chapter_review_mode_matches_ts() {
    use inkos_engine::models::book::{
        resolve_chapter_review_mode, BookWritingConfig, ChapterReviewModeVal,
    };
    for c in &load().resolve_chapter_review_mode {
        let book_cfg = c.input["bookReviewMode"]
            .as_str()
            .map(|s| BookWritingConfig {
                review_mode: if s == "manual" {
                    Some(ChapterReviewModeVal::Manual)
                } else {
                    Some(ChapterReviewModeVal::Auto)
                },
                revision_gate: None,
            });
        let proj = c.input["projectReviewMode"].as_str().map(|s| {
            if s == "manual" {
                ChapterReviewModeVal::Manual
            } else {
                ChapterReviewModeVal::Auto
            }
        });
        let got = resolve_chapter_review_mode(book_cfg.as_ref(), proj);
        let got_str = match got {
            ChapterReviewModeVal::Auto => "auto",
            ChapterReviewModeVal::Manual => "manual",
        };
        let want = c.expected.as_str().unwrap_or("auto");
        assert_eq!(
            got_str, want,
            "case `{}`: resolve_chapter_review_mode 与 TS 不一致",
            c.name
        );
    }
}

#[test]
fn resolve_revision_gate_matches_ts() {
    use inkos_engine::models::book::{resolve_revision_gate, BookWritingConfig, RevisionGateVal};
    for c in &load().resolve_revision_gate {
        let book_cfg = c.input["bookGate"].as_str().map(|s| BookWritingConfig {
            review_mode: None,
            revision_gate: Some(match s {
                "lenient" => RevisionGateVal::Lenient,
                "always" => RevisionGateVal::Always,
                _ => RevisionGateVal::Strict,
            }),
        });
        let proj = c.input["projectGate"].as_str().map(|s| match s {
            "lenient" => RevisionGateVal::Lenient,
            "always" => RevisionGateVal::Always,
            _ => RevisionGateVal::Strict,
        });
        let got = resolve_revision_gate(book_cfg.as_ref(), proj);
        let got_str = match got {
            RevisionGateVal::Strict => "strict",
            RevisionGateVal::Lenient => "lenient",
            RevisionGateVal::Always => "always",
        };
        let want = c.expected.as_str().unwrap_or("strict");
        assert_eq!(
            got_str, want,
            "case `{}`: resolve_revision_gate 与 TS 不一致",
            c.name
        );
    }
}

#[test]
fn parse_memo_matches_ts() {
    use inkos_engine::utils::chapter_memo_parser::parse_memo;
    for c in &load().parse_memo {
        let raw = c.input["raw"]
            .as_str()
            .unwrap_or_else(|| panic!("case {}: raw 缺失", c.name));
        let chapter = c.input["chapter"]
            .as_u64()
            .unwrap_or_else(|| panic!("case {}: chapter 缺失", c.name))
            as u32;
        let golden = c.input["isGoldenOpening"].as_bool().unwrap_or(false);
        let got = parse_memo(raw, chapter, golden);

        let ts_ok = c.expected["ok"]
            .as_bool()
            .unwrap_or_else(|| panic!("case {}: expected.ok 缺失", c.name));
        match (got, ts_ok) {
            (Ok(memo), true) => {
                let got_json = serde_json::to_value(&memo).expect("ChapterMemo 序列化失败");
                assert_eq!(
                    got_json, c.expected["value"],
                    "case `{}`: parse_memo 成功值与 TS 不一致",
                    c.name
                );
            }
            (Err(e), false) => {
                let want = c.expected["error"]
                    .as_str()
                    .unwrap_or_else(|| panic!("case {}: expected.error 缺失", c.name));
                assert_eq!(
                    e.0, want,
                    "case `{}`: parse_memo 错误消息与 TS 不一致",
                    c.name
                );
            }
            (Ok(_), false) => panic!("case `{}`: TS 失败但 Rust 成功", c.name),
            (Err(e), true) => panic!("case `{}`: TS 成功但 Rust 失败: {}", c.name, e.0),
        }
    }
}

/// 数字归一化：TS JSON 整数（如 auditDimensions: [1,2]）与 Rust f64 序列化（1.0）
/// 在 serde_json Number 上表示不同，差分前双侧统一为 f64。
fn norm_numbers(v: Value) -> Value {
    match v {
        Value::Number(n) => match n.as_f64() {
            Some(f) => serde_json::Number::from_f64(f)
                .map(Value::Number)
                .unwrap_or(Value::Number(n)),
            None => Value::Number(n),
        },
        Value::Array(a) => Value::Array(a.into_iter().map(norm_numbers).collect()),
        Value::Object(o) => {
            Value::Object(o.into_iter().map(|(k, v)| (k, norm_numbers(v))).collect())
        }
        other => other,
    }
}

#[test]
fn parse_genre_profile_matches_ts() {
    use inkos_engine::models::genre_profile::parse_genre_profile;
    for c in &load().parse_genre_profile {
        let raw = c
            .input
            .as_str()
            .unwrap_or_else(|| panic!("case {}: input 非 string", c.name));
        let got = parse_genre_profile(raw);
        let ts_ok = c.expected["ok"]
            .as_bool()
            .unwrap_or_else(|| panic!("case {}: expected.ok 缺失", c.name));

        match (got, ts_ok) {
            (Ok(parsed), true) => {
                let got_json = norm_numbers(
                    serde_json::to_value(&parsed).expect("ParsedGenreProfile 序列化失败"),
                );
                let want = norm_numbers(c.expected["value"].clone());
                assert_eq!(
                    got_json, want,
                    "case `{}`: parse_genre_profile 字段与 TS 不一致",
                    c.name
                );
            }
            // 错误消息仅比对成败态（zod/serde 消息文本天然不同）。
            (Err(_), false) => {}
            (Ok(_), false) => panic!("case `{}`: TS 失败但 Rust 成功", c.name),
            (Err(e), true) => panic!("case `{}`: TS 成功但 Rust 失败: {e}", c.name),
        }
    }
}

#[test]
fn parse_book_rules_matches_ts() {
    use inkos_engine::models::book_rules::parse_book_rules;
    for c in &load().parse_book_rules {
        let raw = c
            .input
            .as_str()
            .unwrap_or_else(|| panic!("case {}: input 非 string", c.name));
        let got = parse_book_rules(raw);
        // TS parseBookRules 不抛错：null（shim）也是合法返回，与 Rust None 对齐。
        let want = c.expected["value"].clone();
        match (got, want.is_null()) {
            (Some(parsed), false) => {
                let got_json = norm_numbers(
                    serde_json::to_value(&parsed).expect("ParsedBookRules 序列化失败"),
                );
                let want = norm_numbers(want);
                assert_eq!(
                    got_json, want,
                    "case `{}`: parse_book_rules 字段与 TS 不一致",
                    c.name
                );
            }
            (None, true) => {}
            (Some(_), true) => panic!("case `{}`: TS 为 null（shim）但 Rust 解析出规则", c.name),
            (None, false) => panic!("case `{}`: TS 解析成功但 Rust 为 None", c.name),
        }
    }
}

#[test]
fn build_governed_memory_evidence_blocks_matches_ts() {
    use inkos_engine::models::input_governance::ContextPackage;
    use inkos_engine::utils::governed_context::build_governed_memory_evidence_blocks;
    use inkos_engine::utils::language::WritingLanguage;
    for c in &load().build_governed_memory_evidence_blocks {
        let pkg: ContextPackage = serde_json::from_value(c.input["contextPackage"].clone())
            .unwrap_or_else(|e| panic!("case {}: contextPackage 反序列化失败: {e}", c.name));
        let language = match c.input["language"].as_str() {
            Some("en") => Some(WritingLanguage::En),
            Some("zh") => Some(WritingLanguage::Zh),
            _ => None, // null / 缺失 → 默认 zh（对齐 TS language ?? "zh"）
        };
        let got = build_governed_memory_evidence_blocks(&pkg, language);
        let got_json =
            serde_json::to_value(&got).expect("GovernedMemoryEvidenceBlocks 序列化失败");
        // Rust None 字段经 skip_serializing_if 省略，与 TS undefined 被 JSON.stringify 丢弃对齐。
        assert_eq!(
            got_json, c.expected,
            "case `{}`: build_governed_memory_evidence_blocks 与 TS 不一致",
            c.name
        );
    }
}

#[test]
fn get_fanfic_dimension_config_matches_ts() {
    use inkos_engine::agents::fanfic_dimensions::get_fanfic_dimension_config;
    use inkos_engine::models::book::FanficMode;
    for c in &load().get_fanfic_dimension_config {
        let mode = match c.input["mode"].as_str() {
            Some("canon") => FanficMode::Canon,
            Some("au") => FanficMode::Au,
            Some("ooc") => FanficMode::Ooc,
            Some("cp") => FanficMode::Cp,
            other => panic!("case {}: 未知 fanfic mode {other:?}", c.name),
        };
        // allowedDeviations 参数未被实现使用（对齐 TS），空切片即可。
        let cfg = get_fanfic_dimension_config(mode, &[]);
        let got_json = serde_json::to_value(&cfg).expect("FanficDimensionConfig 序列化失败");
        // TS 侧 Map 经 Object.fromEntries → 数字键字符串化；Rust BTreeMap<u32> 序列化同形。
        assert_eq!(
            got_json, c.expected,
            "case `{}`: get_fanfic_dimension_config 与 TS 不一致",
            c.name
        );
    }
}

#[test]
fn is_current_state_seed_placeholder_matches_ts() {
    use inkos_engine::utils::outline_paths::is_current_state_seed_placeholder;
    for c in &load().is_current_state_seed_placeholder {
        let input = c
            .input
            .as_str()
            .unwrap_or_else(|| panic!("case {}: input 非 string", c.name));
        let got = is_current_state_seed_placeholder(input);
        let want = c
            .expected
            .as_bool()
            .unwrap_or_else(|| panic!("case {}: expected 非 bool", c.name));
        assert_eq!(
            got, want,
            "case `{}`: is_current_state_seed_placeholder 与 TS 不一致 (input 前缀={:?})",
            c.name,
            &input[..input.len().min(20)]
        );
    }
}

#[test]
fn build_golden_opening_discipline_matches_ts() {
    use inkos_engine::agents::writer_prompts::build_golden_opening_discipline;
    use inkos_engine::utils::language::WritingLanguage;
    for c in &load().build_golden_opening_discipline {
        let lang = match c.input["language"].as_str() {
            Some("en") => WritingLanguage::En,
            _ => WritingLanguage::Zh,
        };
        let chapter = c.input["chapterNumber"].as_i64().map(|n| n as u32);
        let got = build_golden_opening_discipline(chapter, lang);
        let want = c
            .expected
            .as_str()
            .unwrap_or_else(|| panic!("case {}: expected 非 string", c.name));
        assert_eq!(
            got, want,
            "case `{}`: build_golden_opening_discipline 与 TS 不一致（字节级文案 diff）",
            c.name
        );
    }
}

#[test]
fn build_fanfic_canon_section_matches_ts() {
    use inkos_engine::agents::fanfic_prompt_sections::build_fanfic_canon_section;
    use inkos_engine::models::book::FanficMode;
    for c in &load().build_fanfic_canon_section {
        let mode = match c.input["mode"].as_str() {
            Some("canon") => FanficMode::Canon,
            Some("au") => FanficMode::Au,
            Some("ooc") => FanficMode::Ooc,
            Some("cp") => FanficMode::Cp,
            other => panic!("case {}: 未知 mode {other:?}", c.name),
        };
        let canon = c
            .input["fanficCanon"]
            .as_str()
            .unwrap_or_else(|| panic!("case {}: fanficCanon 非 string", c.name));
        let got = build_fanfic_canon_section(canon, mode);
        let want = c
            .expected
            .as_str()
            .unwrap_or_else(|| panic!("case {}: expected 非 string", c.name));
        assert_eq!(
            got, want,
            "case `{}`: build_fanfic_canon_section 与 TS 不一致（字节级文案 diff）",
            c.name
        );
    }
}

// ── settler/observer prompt 差分 fixture（对齐 TS golden-leaf-dump 的 settlerBook/settlerGp）──
// prompt 输出差分：fixture 仅取 prompt 真正读取的字段；未被读取的字段用 default 填充
// 不影响输出字符串（settler/observer 只读 name/language/numericalSystem/chapterTypes）。
fn settler_book() -> inkos_engine::models::book::BookConfig {
    use inkos_engine::models::book::{BookConfig, BookStatus, Platform};
    BookConfig {
        id: "golden".into(),
        title: "黄金之书".into(),
        platform: Platform::Tomato,
        genre: "都市脑洞".into(),
        status: BookStatus::Active,
        target_chapters: 300,
        chapter_word_count: 2000,
        language: None,
        created_at: "2026-01-01T00:00:00.000Z".into(),
        updated_at: "2026-01-01T00:00:00.000Z".into(),
        parent_book_id: None,
        fanfic_mode: None,
        writing: None,
    }
}

fn settler_gp(numerical: bool, lang: &str, types: &[&str]) -> inkos_engine::models::genre_profile::GenreProfile {
    inkos_engine::models::genre_profile::GenreProfile {
        name: "都市脑洞".into(),
        id: "urban".into(),
        language: lang.into(),
        chapter_types: types.iter().map(|&s| s.to_string()).collect(),
        fatigue_words: vec!["震惊".into(), "仿佛".into()],
        numerical_system: numerical,
        ..Default::default()
    }
}

/// 从 dump input 读 language 标志：undefined/缺失 → None（对齐 TS `=== "en"` 判定）。
fn lang_opt(input: &Value) -> Option<inkos_engine::utils::language::WritingLanguage> {
    use inkos_engine::utils::language::WritingLanguage;
    match input["language"].as_str() {
        Some("en") => Some(WritingLanguage::En),
        Some("zh") => Some(WritingLanguage::Zh),
        _ => None,
    }
}

#[test]
fn build_settler_system_prompt_matches_ts() {
    use inkos_engine::agents::settler_prompts::build_settler_system_prompt;
    use inkos_engine::models::book_rules::BookRules;
    let book = settler_book();
    for c in &load().build_settler_system_prompt {
        let lang = lang_opt(&c.input);
        let genre_lang = c.input["genreLanguage"].as_str().unwrap_or("zh");
        let numerical = c.input["numericalSystem"].as_bool().unwrap_or(false);
        let types: Vec<&str> = c.input["chapterTypes"]
            .as_array()
            .map(|a| a.iter().filter_map(|v| v.as_str()).collect())
            .unwrap_or_default();
        let gp = settler_gp(numerical, genre_lang, &types);
        let full_cast = c.input["fullCast"].as_bool().unwrap_or(false);
        let rules = if full_cast {
            Some(BookRules {
                enable_full_cast_tracking: true,
                ..BookRules::default()
            })
        } else {
            None
        };
        let got = build_settler_system_prompt(&book, &gp, rules.as_ref(), lang);
        let want = c
            .expected
            .as_str()
            .unwrap_or_else(|| panic!("case {}: expected 非 string", c.name));
        assert_eq!(
            got, want,
            "case `{}`: build_settler_system_prompt 与 TS 不一致（字节级文案 diff）",
            c.name
        );
    }
}

#[test]
fn build_settler_user_prompt_matches_ts() {
    use inkos_engine::agents::settler_prompts::{build_settler_user_prompt, SettlerUserPromptInput};
    const PH: &str = "(文件尚未创建)";
    for c in &load().build_settler_user_prompt {
        // 3 个固定向量：params 全字段在 Rust 侧按命名重建（对齐 TS dump 的字面量）。
        let params = match c.name.as_str() {
            "minimal" => SettlerUserPromptInput {
                chapter_number: 12,
                title: "试炼",
                content: "正文内容。",
                current_state: "状态卡内容",
                ledger: "",
                hooks: "伏笔池内容",
                chapter_summaries: PH,
                subplot_board: PH,
                emotional_arcs: PH,
                character_matrix: PH,
                volume_outline: "第一卷：开局",
                observations: None,
                selected_evidence_block: None,
                governed_control_block: None,
                validation_feedback: None,
            },
            "all-blocks" => SettlerUserPromptInput {
                chapter_number: 3,
                title: "转折",
                content: "内容",
                current_state: "状态",
                ledger: "灵石 120",
                hooks: "H01",
                chapter_summaries: "| 章节 |",
                subplot_board: "支线A",
                emotional_arcs: "弧线",
                character_matrix: "矩阵",
                volume_outline: "不该出现的卷纲",
                observations: Some("观察1"),
                selected_evidence_block: Some("证据块"),
                governed_control_block: Some("\n## 本章控制输入\nintent"),
                validation_feedback: Some("状态矛盾：X"),
            },
            "governed-mutex-outline-hidden" => SettlerUserPromptInput {
                chapter_number: 1,
                title: "t",
                content: "c",
                current_state: "s",
                ledger: "L",
                hooks: "h",
                chapter_summaries: "摘要",
                subplot_board: "支",
                emotional_arcs: "情",
                character_matrix: "矩",
                volume_outline: "卷纲应被隐藏",
                observations: Some("obs"),
                selected_evidence_block: Some("ev"),
                governed_control_block: Some("\n## 本章控制输入\nctrl"),
                validation_feedback: Some("fb"),
            },
            other => panic!("未知 build_settler_user_prompt case: {other}"),
        };
        let got = build_settler_user_prompt(&params);
        let want = c
            .expected
            .as_str()
            .unwrap_or_else(|| panic!("case {}: expected 非 string", c.name));
        assert_eq!(
            got, want,
            "case `{}`: build_settler_user_prompt 与 TS 不一致（字节级文案 diff）",
            c.name
        );
    }
}

#[test]
fn build_observer_system_prompt_matches_ts() {
    use inkos_engine::agents::observer_prompts::build_observer_system_prompt;
    let book = settler_book();
    for c in &load().build_observer_system_prompt {
        let lang = lang_opt(&c.input);
        let genre_lang = c.input["genreLanguage"].as_str().unwrap_or("zh");
        let gp = settler_gp(false, genre_lang, &[]);
        let got = build_observer_system_prompt(&book, &gp, lang);
        let want = c
            .expected
            .as_str()
            .unwrap_or_else(|| panic!("case {}: expected 非 string", c.name));
        assert_eq!(
            got, want,
            "case `{}`: build_observer_system_prompt 与 TS 不一致（字节级文案 diff）",
            c.name
        );
    }
}

#[test]
fn build_observer_user_prompt_matches_ts() {
    use inkos_engine::agents::observer_prompts::build_observer_user_prompt;
    for c in &load().build_observer_user_prompt {
        let chapter = c.input["chapterNumber"].as_i64().unwrap() as u32;
        let title = c.input["title"].as_str().unwrap();
        let content = c.input["content"].as_str().unwrap();
        let lang = lang_opt(&c.input);
        let got = build_observer_user_prompt(chapter, title, content, lang);
        let want = c
            .expected
            .as_str()
            .unwrap_or_else(|| panic!("case {}: expected 非 string", c.name));
        assert_eq!(
            got, want,
            "case `{}`: build_observer_user_prompt 与 TS 不一致（字节级文案 diff）",
            c.name
        );
    }
}

#[test]
fn normalize_post_write_surface_matches_ts() {
    use inkos_engine::agents::post_write_validator::normalize_post_write_surface;
    for c in &load().normalize_post_write_surface {
        let content = c.input["content"].as_str().unwrap();
        let lang = lang_opt(&c.input);
        let got = normalize_post_write_surface(content, lang);
        let want = c
            .expected
            .as_str()
            .unwrap_or_else(|| panic!("case {}: expected 非 string", c.name));
        assert_eq!(
            got, want,
            "case `{}`: normalize_post_write_surface 与 TS 不一致",
            c.name
        );
    }
}

#[test]
fn validate_post_write_matches_ts() {
    use inkos_engine::agents::post_write_validator::validate_post_write;
    use inkos_engine::models::book_rules::{BookRules, NarrativePerson, Protagonist};
    let gp = settler_gp(false, "zh", &[]);
    for c in &load().validate_post_write {
        let content = c.input["content"].as_str().unwrap();
        let lang = lang_opt(&c.input);
        let rules = match c.input["rules"].as_str() {
            Some("first") => Some(BookRules {
                narrative_person: Some(NarrativePerson::First),
                protagonist: Some(Protagonist {
                    name: "陆承烬".into(),
                    ..Protagonist::default()
                }),
                ..BookRules::default()
            }),
            _ => None,
        };
        let got = validate_post_write(content, &gp, rules.as_ref(), lang);
        let got_val = serde_json::to_value(&got).expect("violations 序列化");
        assert_eq!(
            got_val, c.expected,
            "case `{}`: validate_post_write 与 TS 不一致（violations 数组形状/顺序/文案 diff）",
            c.name
        );
    }
}

#[test]
fn detect_cross_chapter_repetition_matches_ts() {
    use inkos_engine::agents::post_write_validator::detect_cross_chapter_repetition;
    use inkos_engine::utils::language::WritingLanguage;
    for c in &load().detect_cross_chapter_repetition {
        // current/recent 由 TS IIFE 构造，Rust 按 scenario 名重建相同字符串。
        let (got, lang) = match c.name.as_str() {
            "zh-three" => {
                let p1 = "风吹过山岗上";
                let p2 = "雨落在屋檐下";
                let p3 = "雪覆盖了田野";
                let current = format!("{p1}{p1}{p2}{p2}{p3}{p3}其他内容填充。");
                let recent = format!("历史章节提到{p1}和{p2}与{p3}。{}填充。", "长".repeat(100));
                (detect_cross_chapter_repetition(&current, &recent, WritingLanguage::Zh), WritingLanguage::Zh)
            }
            "en-three" => {
                let current = "the dark shadow moved the dark shadow moved the silent figure stood the silent figure stood the cold wind blew the cold wind blew trailing prose.";
                let recent = format!(
                    "earlier the dark shadow moved and the silent figure stood while the cold wind blew. {}",
                    "x".repeat(100)
                );
                (detect_cross_chapter_repetition(current, &recent, WritingLanguage::En), WritingLanguage::En)
            }
            other => panic!("未知 detect_cross_chapter_repetition case: {other}"),
        };
        let _ = lang;
        let got_val = serde_json::to_value(&got).expect("violations 序列化");
        assert_eq!(
            got_val, c.expected,
            "case `{}`: detect_cross_chapter_repetition 与 TS 不一致",
            c.name
        );
    }
}

#[test]
fn detect_paragraph_length_drift_matches_ts() {
    use inkos_engine::agents::post_write_validator::detect_paragraph_length_drift;
    use inkos_engine::utils::language::WritingLanguage;
    for c in &load().detect_paragraph_length_drift {
        let got = match c.name.as_str() {
            "zh-shrink" => {
                let long_para = "长段落内容".repeat(20);
                let recent = format!("{long_para}\n\n{long_para}\n\n{long_para}\n\n{long_para}");
                let current = "短。\n\n短。\n\n短。\n\n短。";
                detect_paragraph_length_drift(current, &recent, WritingLanguage::Zh)
            }
            other => panic!("未知 detect_paragraph_length_drift case: {other}"),
        };
        let got_val = serde_json::to_value(&got).expect("violations 序列化");
        assert_eq!(
            got_val, c.expected,
            "case `{}`: detect_paragraph_length_drift 与 TS 不一致",
            c.name
        );
    }
}

#[test]
fn detect_duplicate_title_matches_ts() {
    use inkos_engine::agents::post_write_validator::detect_duplicate_title;
    for c in &load().detect_duplicate_title {
        let new_title = c.input["newTitle"].as_str().unwrap();
        let existing: Vec<String> = c.input["existing"]
            .as_array()
            .map(|a| a.iter().filter_map(|v| v.as_str().map(String::from)).collect())
            .unwrap_or_default();
        let got = detect_duplicate_title(new_title, &existing);
        let got_val = serde_json::to_value(&got).expect("violations 序列化");
        assert_eq!(
            got_val, c.expected,
            "case `{}`: detect_duplicate_title 与 TS 不一致",
            c.name
        );
    }
}

#[test]
fn resolve_duplicate_title_matches_ts() {
    use inkos_engine::agents::post_write_validator::resolve_duplicate_title;
    use inkos_engine::utils::language::WritingLanguage;
    for c in &load().resolve_duplicate_title {
        let new_title = c.input["newTitle"].as_str().unwrap();
        let existing: Vec<String> = c.input["existing"]
            .as_array()
            .map(|a| a.iter().filter_map(|v| v.as_str().map(String::from)).collect())
            .unwrap_or_default();
        let lang = lang_opt(&c.input).unwrap_or(WritingLanguage::Zh);
        let got = resolve_duplicate_title(new_title, &existing, lang, None);
        let got_val = serde_json::to_value(&got).expect("result 序列化");
        assert_eq!(
            got_val, c.expected,
            "case `{}`: resolve_duplicate_title 与 TS 不一致（title/issues 形状 diff）",
            c.name
        );
    }
}

// ---- 32 号：governed-working-set / renderHookSnapshot / writer 私有纯函数 ----

#[test]
fn render_hook_snapshot_matches_ts() {
    use inkos_engine::models::runtime_state::HookRecord;
    use inkos_engine::utils::story_markdown::render_hook_snapshot;
    for c in &load().render_hook_snapshot {
        let hooks: Vec<HookRecord> = serde_json::from_value(c.input["hooks"].clone())
            .unwrap_or_else(|e| panic!("case {}: hooks 反序列化失败: {e}", c.name));
        let got = render_hook_snapshot(&hooks, lang_opt(&c.input).expect("language 必填"));
        assert_eq!(got, c.expected.as_str().unwrap_or(""), "case `{}`", c.name);
    }
}

#[test]
fn build_governed_hook_working_set_matches_ts() {
    use inkos_engine::models::input_governance::ContextPackage;
    use inkos_engine::utils::governed_working_set::{
        build_governed_hook_working_set, GovernedHookWorkingSetInput,
    };
    for c in &load().build_governed_hook_working_set {
        let pkg: ContextPackage = serde_json::from_value(c.input["contextPackage"].clone())
            .unwrap_or_else(|e| panic!("case {}: contextPackage: {e}", c.name));
        let got = build_governed_hook_working_set(&GovernedHookWorkingSetInput {
            hooks_markdown: c.input["hooksMarkdown"].as_str().unwrap(),
            context_package: &pkg,
            chapter_intent: c.input["chapterIntent"].as_str(),
            chapter_number: c.input["chapterNumber"].as_u64().unwrap() as u32,
            language: lang_opt(&c.input).expect("language 必填"),
            keep_recent: c.input["keepRecent"].as_u64().map(|v| v as u32),
        });
        assert_eq!(got, c.expected.as_str().unwrap_or(""), "case `{}`", c.name);
    }
}

#[test]
fn merge_table_markdown_by_key_matches_ts() {
    use inkos_engine::utils::governed_working_set::merge_table_markdown_by_key;
    for c in &load().merge_table_markdown_by_key {
        let keys: Vec<usize> = c.input["keyColumns"]
            .as_array()
            .map(|a| a.iter().map(|v| v.as_u64().unwrap() as usize).collect())
            .unwrap_or_default();
        let got = merge_table_markdown_by_key(
            c.input["original"].as_str().unwrap(),
            c.input["updated"].as_str().unwrap(),
            &keys,
        );
        assert_eq!(got, c.expected.as_str().unwrap_or(""), "case `{}`", c.name);
    }
}

#[test]
fn merge_character_matrix_markdown_matches_ts() {
    use inkos_engine::utils::governed_working_set::merge_character_matrix_markdown;
    for c in &load().merge_character_matrix_markdown {
        let got = merge_character_matrix_markdown(
            c.input["original"].as_str().unwrap(),
            c.input["updated"].as_str().unwrap(),
        );
        assert_eq!(got, c.expected.as_str().unwrap_or(""), "case `{}`", c.name);
    }
}

#[test]
fn build_governed_character_matrix_working_set_matches_ts() {
    use inkos_engine::models::input_governance::ContextPackage;
    use inkos_engine::utils::governed_working_set::{
        build_governed_character_matrix_working_set, GovernedMatrixWorkingSetInput,
    };
    for c in &load().build_governed_character_matrix_working_set {
        let pkg: ContextPackage = serde_json::from_value(c.input["contextPackage"].clone())
            .unwrap_or_else(|e| panic!("case {}: contextPackage: {e}", c.name));
        let got = build_governed_character_matrix_working_set(&GovernedMatrixWorkingSetInput {
            matrix_markdown: c.input["matrixMarkdown"].as_str().unwrap(),
            chapter_intent: c.input["chapterIntent"].as_str().unwrap(),
            context_package: &pkg,
            protagonist_name: c.input["protagonistName"].as_str(),
        });
        assert_eq!(got, c.expected.as_str().unwrap_or(""), "case `{}`", c.name);
    }
}

#[test]
fn writer_build_user_prompt_matches_ts() {
    use inkos_engine::agents::writer::{build_user_prompt, UserPromptInput};
    use inkos_engine::models::length_governance::LengthSpec;
    for c in &load().writer_build_user_prompt {
        let i = &c.input;
        let spec: LengthSpec = serde_json::from_value(i["lengthSpec"].clone())
            .unwrap_or_else(|e| panic!("case {}: lengthSpec: {e}", c.name));
        let got = build_user_prompt(&UserPromptInput {
            chapter_number: i["chapterNumber"].as_u64().unwrap() as u32,
            story_bible: i["storyBible"].as_str().unwrap(),
            current_state: i["currentState"].as_str().unwrap(),
            ledger: i["ledger"].as_str().unwrap(),
            hooks: i["hooks"].as_str().unwrap(),
            recent_chapters: i["recentChapters"].as_str().unwrap(),
            length_spec: &spec,
            external_context: i["externalContext"].as_str(),
            chapter_summaries: i["chapterSummaries"].as_str().unwrap(),
            subplot_board: i["subplotBoard"].as_str().unwrap(),
            emotional_arcs: i["emotionalArcs"].as_str().unwrap(),
            character_matrix: i["characterMatrix"].as_str().unwrap(),
            dialogue_fingerprints: i["dialogueFingerprints"].as_str(),
            relevant_summaries: i["relevantSummaries"].as_str(),
            parent_canon: i["parentCanon"].as_str(),
            language: lang_opt(i),
        });
        assert_eq!(got, c.expected.as_str().unwrap_or(""), "case `{}`", c.name);
    }
}

#[test]
fn writer_build_governed_user_prompt_matches_ts() {
    use inkos_engine::agents::writer::{build_governed_user_prompt, GovernedUserPromptInput};
    use inkos_engine::models::input_governance::{ChapterMemo, ContextPackage, RuleStack};
    use inkos_engine::models::length_governance::LengthSpec;
    for c in &load().writer_build_governed_user_prompt {
        let i = &c.input;
        let memo: ChapterMemo = serde_json::from_value(i["chapterMemo"].clone())
            .unwrap_or_else(|e| panic!("case {}: chapterMemo: {e}", c.name));
        let pkg: ContextPackage = serde_json::from_value(i["contextPackage"].clone())
            .unwrap_or_else(|e| panic!("case {}: contextPackage: {e}", c.name));
        let stack: RuleStack = serde_json::from_value(i["ruleStack"].clone())
            .unwrap_or_else(|e| panic!("case {}: ruleStack: {e}", c.name));
        let spec: LengthSpec = serde_json::from_value(i["lengthSpec"].clone())
            .unwrap_or_else(|e| panic!("case {}: lengthSpec: {e}", c.name));
        let got = build_governed_user_prompt(&GovernedUserPromptInput {
            chapter_number: i["chapterNumber"].as_u64().unwrap() as u32,
            chapter_memo: &memo,
            chapter_intent_data: None,
            context_package: &pkg,
            rule_stack: &stack,
            external_context: i["externalContext"].as_str(),
            length_spec: &spec,
            language: lang_opt(i),
            variance_brief: None,
            selected_evidence_block: None,
        });
        assert_eq!(got, c.expected.as_str().unwrap_or(""), "case `{}`", c.name);
    }
}

#[test]
fn writer_build_chapter_context_block_matches_ts() {
    use inkos_engine::agents::writer::build_chapter_context_block;
    for c in &load().writer_build_chapter_context_block {
        let got = build_chapter_context_block(
            c.input["externalContext"].as_str(),
            lang_opt(&c.input).expect("language 必填"),
        );
        assert_eq!(got, c.expected.as_str().unwrap_or(""), "case `{}`", c.name);
    }
}

#[test]
fn writer_build_settler_governed_control_block_matches_ts() {
    use inkos_engine::agents::writer::build_settler_governed_control_block;
    use inkos_engine::models::input_governance::{ContextPackage, RuleStack};
    for c in &load().writer_build_settler_governed_control_block {
        let pkg: ContextPackage = serde_json::from_value(c.input["contextPackage"].clone())
            .unwrap_or_else(|e| panic!("case {}: contextPackage: {e}", c.name));
        let stack: RuleStack = serde_json::from_value(c.input["ruleStack"].clone())
            .unwrap_or_else(|e| panic!("case {}: ruleStack: {e}", c.name));
        let got = build_settler_governed_control_block(
            c.input["chapterIntent"].as_str().unwrap(),
            &pkg,
            &stack,
            lang_opt(&c.input).expect("language 必填"),
        );
        assert_eq!(got, c.expected.as_str().unwrap_or(""), "case `{}`", c.name);
    }
}

#[test]
fn writer_build_length_requirement_block_matches_ts() {
    use inkos_engine::agents::writer::build_length_requirement_block;
    use inkos_engine::utils::length_metrics::build_length_spec;
    for c in &load().writer_build_length_requirement_block {
        let spec = build_length_spec(
            c.input["target"].as_u64().unwrap() as u32,
            lang_opt(&c.input).expect("language 必填"),
        );
        let got = build_length_requirement_block(&spec, Some(lang_opt(&c.input).expect("language 必填")));
        assert_eq!(got, c.expected.as_str().unwrap_or(""), "case `{}`", c.name);
    }
}

#[test]
fn writer_sanitize_filename_matches_ts() {
    use inkos_engine::agents::writer::sanitize_filename;
    for c in &load().writer_sanitize_filename {
        let got = sanitize_filename(c.input.as_str().unwrap());
        assert_eq!(got, c.expected.as_str().unwrap_or(""), "case `{}`", c.name);
    }
}

#[test]
fn writer_extract_dialogue_fingerprints_matches_ts() {
    use inkos_engine::agents::writer::extract_dialogue_fingerprints;
    for c in &load().writer_extract_dialogue_fingerprints {
        let got = extract_dialogue_fingerprints(c.input.as_str().unwrap());
        assert_eq!(got, c.expected.as_str().unwrap_or(""), "case `{}`", c.name);
    }
}

#[test]
fn writer_find_relevant_summaries_matches_ts() {
    use inkos_engine::agents::writer::find_relevant_summaries;
    for c in &load().writer_find_relevant_summaries {
        let got = find_relevant_summaries(
            c.input["chapterSummaries"].as_str().unwrap(),
            c.input["volumeOutline"].as_str().unwrap(),
            c.input["chapterNumber"].as_u64().unwrap() as u32,
        );
        assert_eq!(got, c.expected.as_str().unwrap_or(""), "case `{}`", c.name);
    }
}

#[test]
fn writer_build_style_fingerprint_matches_ts() {
    use inkos_engine::agents::writer::build_style_fingerprint;
    for c in &load().writer_build_style_fingerprint {
        let got = build_style_fingerprint(c.input["raw"].as_str().unwrap());
        assert_eq!(
            got.as_deref(),
            c.expected.as_str(),
            "case `{}`: build_style_fingerprint 与 TS 不一致",
            c.name
        );
    }
}

#[test]
fn writer_render_delta_summary_row_matches_ts() {
    use inkos_engine::agents::writer::render_delta_summary_row;
    use inkos_engine::models::runtime_state::RuntimeStateDelta;
    for c in &load().writer_render_delta_summary_row {
        let delta: RuntimeStateDelta = serde_json::from_value(c.input["delta"].clone())
            .unwrap_or_else(|e| panic!("case {}: delta: {e}", c.name));
        let got = render_delta_summary_row(&delta);
        assert_eq!(got, c.expected.as_str().unwrap_or(""), "case `{}`", c.name);
    }
}

#[test]
fn writer_normalize_runtime_state_delta_chapter_matches_ts() {
    use inkos_engine::agents::writer::normalize_runtime_state_delta_chapter;
    use inkos_engine::models::runtime_state::RuntimeStateDelta;
    for c in &load().writer_normalize_runtime_state_delta_chapter {
        let delta: RuntimeStateDelta = serde_json::from_value(c.input["delta"].clone())
            .unwrap_or_else(|e| panic!("case {}: delta: {e}", c.name));
        let authority = c.input["authority"].as_u64().unwrap() as u32;
        let got = normalize_runtime_state_delta_chapter(&delta, authority);
        let got_val = serde_json::to_value(&got).expect("序列化");
        assert_eq!(got_val, c.expected, "case `{}`", c.name);
    }
}

// ---- 33 号：memory-retrieval + renderSummarySnapshot ----

#[test]
fn compute_recyclable_hooks_matches_ts() {
    use inkos_engine::models::runtime_state::HookRecord;
    use inkos_engine::utils::memory_retrieval::compute_recyclable_hooks;
    for c in &load().compute_recyclable_hooks {
        let hooks: Vec<HookRecord> = serde_json::from_value(c.input["hooks"].clone())
            .unwrap_or_else(|e| panic!("case {}: hooks 反序列化失败: {e}", c.name));
        let chapter = c.input["chapterNumber"].as_u64().unwrap() as u32;
        let recycled = compute_recyclable_hooks(&hooks, chapter);
        let got: Vec<&str> = recycled.iter().map(|h| h.hook_id.as_str()).collect();
        let expected: Vec<&str> = c
            .expected
            .as_array()
            .unwrap_or_else(|| panic!("case {}: expected 非 array", c.name))
            .iter()
            .map(|v| v.as_str().unwrap())
            .collect();
        assert_eq!(got, expected, "case `{}`", c.name);
    }
}

#[test]
fn extract_query_terms_matches_ts() {
    use inkos_engine::utils::memory_retrieval::extract_query_terms;
    for c in &load().extract_query_terms {
        let goal = c.input["goal"].as_str().unwrap_or("");
        let outline_node = c.input["outlineNode"].as_str();
        let must_keep: Vec<String> = c
            .input["mustKeep"]
            .as_array()
            .map(|v| v.iter().filter_map(|x| x.as_str().map(String::from)).collect())
            .unwrap_or_default();
        let got = extract_query_terms(goal, outline_node, &must_keep);
        let expected: Vec<String> = c
            .expected
            .as_array()
            .unwrap_or_else(|| panic!("case {}: expected 非 array", c.name))
            .iter()
            .map(|v| v.as_str().unwrap().to_string())
            .collect();
        assert_eq!(got, expected, "case `{}`", c.name);
    }
}

#[test]
fn render_summary_snapshot_matches_ts() {
    use inkos_engine::state::memory_db::StoredSummary;
    use inkos_engine::utils::story_markdown::render_summary_snapshot;
    for c in &load().render_summary_snapshot {
        let summaries: Vec<StoredSummary> = serde_json::from_value(c.input["summaries"].clone())
            .unwrap_or_else(|e| panic!("case {}: summaries 反序列化失败: {e}", c.name));
        let got = render_summary_snapshot(&summaries, lang_opt(&c.input).expect("language 必填"));
        assert_eq!(got, c.expected.as_str().unwrap_or(""), "case `{}`", c.name);
    }
}

// ---- 34 号：planner 三件套 ----

#[test]
fn planner_system_prompt_matches_ts() {
    use inkos_engine::agents::planner_prompts::get_planner_memo_system_prompt;
    for c in &load().planner_system_prompt {
        let got = get_planner_memo_system_prompt(lang_opt(&c.input).expect("language 必填"));
        assert_eq!(got, c.expected.as_str().unwrap_or(""), "case `{}`", c.name);
    }
}

#[test]
fn planner_build_user_message_matches_ts() {
    use inkos_engine::agents::planner_prompts::{build_planner_user_message, PlannerUserMessageInput};
    for c in &load().planner_build_user_message {
        let input = &c.input;
        let got = build_planner_user_message(&PlannerUserMessageInput {
            chapter_number: input["chapterNumber"].as_u64().unwrap() as u32,
            previous_chapter_ending_excerpt: input["previousChapterEndingExcerpt"].as_str().unwrap_or(""),
            recent_summaries: input["recentSummaries"].as_str().unwrap_or(""),
            current_arc_prose: input["currentArcProse"].as_str().unwrap_or(""),
            protagonist_matrix_row: input["protagonistMatrixRow"].as_str().unwrap_or(""),
            opponent_rows: input["opponentRows"].as_str().unwrap_or(""),
            collaborator_rows: input["collaboratorRows"].as_str().unwrap_or(""),
            relevant_threads: input["relevantThreads"].as_str().unwrap_or(""),
            recyclable_hooks: input["recyclableHooks"].as_str().unwrap_or(""),
            is_golden_opening: input["isGoldenOpening"].as_bool().unwrap_or(false),
            book_rules_relevant: input["bookRulesRelevant"].as_str().unwrap_or(""),
            brief: input["brief"].as_str(),
            chapter_context: input["chapterContext"].as_str(),
            language: lang_opt(&c.input).expect("language 必填"),
        });
        assert_eq!(got, c.expected.as_str().unwrap_or(""), "case `{}`", c.name);
    }
}

#[test]
fn planner_golden_opening_guidance_matches_ts() {
    use inkos_engine::agents::planner_prompts::build_golden_opening_guidance;
    for c in &load().planner_golden_opening_guidance {
        let got = build_golden_opening_guidance(
            c.input["chapterNumber"].as_u64().unwrap() as u32,
            lang_opt(&c.input).expect("language 必填"),
        );
        assert_eq!(got, c.expected.as_str().unwrap_or(""), "case `{}`", c.name);
    }
}

#[test]
fn planner_format_recent_summaries_matches_ts() {
    use inkos_engine::agents::planner_context::format_recent_summaries;
    for c in &load().planner_format_recent_summaries {
        let got = format_recent_summaries(
            c.input["raw"].as_str().unwrap(),
            c.input["chapterNumber"].as_u64().unwrap() as u32,
            c.input["limit"].as_u64().unwrap() as usize,
        );
        assert_eq!(got, c.expected.as_str().unwrap_or(""), "case `{}`", c.name);
    }
}

#[test]
fn planner_compose_current_arc_prose_matches_ts() {
    use inkos_engine::agents::planner_context::compose_current_arc_prose;
    for c in &load().planner_compose_current_arc_prose {
        let got = compose_current_arc_prose(
            c.input["subplotBoardRaw"].as_str().unwrap(),
            c.input["emotionalArcsRaw"].as_str().unwrap(),
            c.input["chapterNumber"].as_u64().unwrap() as u32,
        );
        assert_eq!(got, c.expected.as_str().unwrap_or(""), "case `{}`", c.name);
    }
}

#[test]
fn planner_extract_protagonist_row_matches_ts() {
    use inkos_engine::agents::planner_context::extract_protagonist_row;
    for c in &load().planner_extract_protagonist_row {
        let got = extract_protagonist_row(c.input["raw"].as_str().unwrap());
        assert_eq!(got, c.expected.as_str().unwrap_or(""), "case `{}`", c.name);
    }
}

#[test]
fn planner_extract_relation_rows_matches_ts() {
    use inkos_engine::agents::planner_context::{
        extract_collaborator_rows, extract_opponent_rows,
    };
    for c in &load().planner_extract_relation_rows {
        let raw = c.input["raw"].as_str().unwrap();
        let limit = c.input["limit"].as_u64().unwrap() as usize;
        let got = if c.input["kind"].as_str() == Some("opponent") {
            extract_opponent_rows(raw, limit)
        } else {
            extract_collaborator_rows(raw, limit)
        };
        assert_eq!(got, c.expected.as_str().unwrap_or(""), "case `{}`", c.name);
    }
}

#[test]
fn planner_extract_relevant_threads_matches_ts() {
    use inkos_engine::agents::planner_context::extract_relevant_threads;
    for c in &load().planner_extract_relevant_threads {
        let got = extract_relevant_threads(
            c.input["pendingHooksRaw"].as_str().unwrap(),
            c.input["subplotBoardRaw"].as_str().unwrap(),
        );
        assert_eq!(got, c.expected.as_str().unwrap_or(""), "case `{}`", c.name);
    }
}

#[test]
fn planner_format_recyclable_hooks_matches_ts() {
    use inkos_engine::agents::planner_context::format_recyclable_hooks;
    use inkos_engine::models::runtime_state::HookRecord;
    for c in &load().planner_format_recyclable_hooks {
        let hooks: Vec<HookRecord> = serde_json::from_value(c.input["hooks"].clone())
            .unwrap_or_else(|e| panic!("case {}: hooks 反序列化失败: {e}", c.name));
        let got = format_recyclable_hooks(
            &hooks,
            c.input["chapterNumber"].as_u64().unwrap() as u32,
            lang_opt(&c.input).expect("language 必填"),
        );
        assert_eq!(got, c.expected.as_str().unwrap_or(""), "case `{}`", c.name);
    }
}

#[test]
fn planner_private_suite_matches_ts() {
    use inkos_engine::agents::planner::{
        build_arc_context, collect_must_avoid, collect_must_keep, collect_style_emphasis,
        derive_goal, extract_section, find_outline_node, is_golden_opening_chapter,
        render_hook_budget, render_intent_markdown,
    };
    use inkos_engine::models::input_governance::{ChapterIntent, ChapterMemo};

    for c in &load().planner_private_suite {
        let input = &c.input;
        let got: serde_json::Value = match c.name.as_str() {
            "derive-goal-chain" => serde_json::to_value(derive_goal(
                input["externalContext"].as_str(),
                input["currentFocus"].as_str().unwrap(),
                input["authorIntent"].as_str().unwrap(),
                input["outlineNode"].as_str(),
                input["chapterNumber"].as_u64().unwrap() as u32,
            ))
            .unwrap(),
            "derive-goal-default" => serde_json::to_value(derive_goal(
                input["externalContext"].as_str(),
                input["currentFocus"].as_str().unwrap(),
                input["authorIntent"].as_str().unwrap(),
                input["outlineNode"].as_str(),
                input["chapterNumber"].as_u64().unwrap() as u32,
            ))
            .unwrap(),
            "find-outline-exact" | "find-outline-range-beats" | "find-outline-tricky-numbers" => {
                serde_json::to_value(find_outline_node(
                    input["volumeOutline"].as_str().unwrap(),
                    input["chapterNumber"].as_u64().unwrap() as u32,
                ))
                .unwrap()
            }
            "collect-must-keep" => serde_json::to_value(collect_must_keep(
                input["currentState"].as_str().unwrap(),
                input["storyBible"].as_str().unwrap(),
            ))
            .unwrap(),
            "collect-must-avoid" => {
                let prohibitions: Vec<String> = input["prohibitions"]
                    .as_array()
                    .map(|v| v.iter().filter_map(|x| x.as_str().map(String::from)).collect())
                    .unwrap_or_default();
                serde_json::to_value(collect_must_avoid(
                    input["currentFocus"].as_str().unwrap(),
                    &prohibitions,
                ))
                .unwrap()
            }
            "collect-style-emphasis" => serde_json::to_value(collect_style_emphasis(
                input["authorIntent"].as_str().unwrap(),
                input["currentFocus"].as_str().unwrap(),
            ))
            .unwrap(),
            "extract-section" => {
                let headings: Vec<&str> = input["headings"]
                    .as_array()
                    .map(|v| v.iter().filter_map(|x| x.as_str()).collect())
                    .unwrap_or_default();
                serde_json::to_value(extract_section(
                    input["content"].as_str().unwrap(),
                    &headings,
                ))
                .unwrap()
            }
            "arc-context" | "arc-context-placeholder" => serde_json::to_value(build_arc_context(
                input["language"].as_str(),
                input["volumeOutline"].as_str().unwrap(),
                input["outlineNode"].as_str(),
            ))
            .unwrap(),
            "golden-window-zh" | "golden-window-en" => serde_json::to_value(
                is_golden_opening_chapter(
                    input["language"].as_str(),
                    input["chapterNumber"].as_u64().unwrap() as u32,
                ),
            )
            .unwrap(),
            "hook-budget-under" | "hook-budget-over" => serde_json::to_value(render_hook_budget(
                input["activeCount"].as_u64().unwrap() as usize,
                lang_opt(input).expect("language 必填"),
            ))
            .unwrap(),
            "render-intent-markdown" => {
                let intent: ChapterIntent =
                    serde_json::from_value(input["intent"].clone()).unwrap();
                let memo: ChapterMemo = serde_json::from_value(input["memo"].clone()).unwrap();
                serde_json::to_value(render_intent_markdown(
                    &intent,
                    &memo,
                    lang_opt(input).expect("language 必填"),
                    input["pendingHooks"].as_str().unwrap(),
                    input["chapterSummaries"].as_str().unwrap(),
                    input["activeHookCount"].as_u64().unwrap() as usize,
                ))
                .unwrap()
            }
            other => panic!("未知 private suite case: {other}"),
        };
        assert_eq!(got, c.expected, "case `{}`", c.name);
    }
}

// ---- 35 号：context-assembly ----

#[test]
fn build_governed_rule_stack_matches_ts() {
    use inkos_engine::utils::context_assembly::build_governed_rule_stack;
    for c in &load().build_governed_rule_stack {
        let must_avoid: Vec<String> = serde_json::from_value(c.input["mustAvoid"].clone()).unwrap();
        let style_emphasis: Vec<String> =
            serde_json::from_value(c.input["styleEmphasis"].clone()).unwrap();
        let chapter = c.input["chapterNumber"].as_u64().unwrap() as u32;
        let got = build_governed_rule_stack(&must_avoid, &style_emphasis, chapter);
        let expected = serde_json::to_value(&got).unwrap();
        let want: serde_json::Value = serde_json::from_value(c.expected.clone()).unwrap();
        assert_eq!(expected, want, "case `{}`", c.name);
    }
}

#[test]
fn build_governed_trace_matches_ts() {
    use inkos_engine::models::input_governance::ContextPackage;
    use inkos_engine::utils::context_assembly::{build_governed_trace, GovernedTraceParams};
    for c in &load().build_governed_trace {
        let context_package: ContextPackage =
            serde_json::from_value(c.input["contextPackage"].clone()).unwrap();
        let notes: Vec<String> = serde_json::from_value(c.input["notes"].clone()).unwrap();
        let composer_inputs: Vec<String> =
            serde_json::from_value(c.input["composerInputs"].clone()).unwrap();
        let planner_inputs: Vec<String> =
            serde_json::from_value(c.input["plan"]["plannerInputs"].clone()).unwrap();
        let chapter = c.input["plan"]["intent"]["chapter"].as_u64().unwrap() as u32;
        let got = build_governed_trace(&GovernedTraceParams {
            chapter_number: chapter,
            planner_inputs: &planner_inputs,
            composer_inputs: &composer_inputs,
            context_package: &context_package,
            notes: &notes,
            prompt_packs: None,
            compression: None,
        });
        assert_eq!(serde_json::to_value(&got).unwrap(), c.expected, "case `{}`", c.name);
    }
}

#[test]
fn is_protected_context_source_matches_ts() {
    use inkos_engine::utils::context_assembly::is_protected_context_source;
    for c in &load().is_protected_context_source {
        let got = is_protected_context_source(c.input["source"].as_str().unwrap());
        assert_eq!(got, c.expected.as_bool().unwrap(), "case `{}`", c.name);
    }
}

// ---- 36 号：reviser ----

#[test]
fn reviser_private_suite_matches_ts() {
    use inkos_engine::agents::reviser::{
        build_auto_system_prompt, build_legacy_system_prompt, build_reduced_control_block,
        parse_reviser_output, AutoOutputMode, ReviseMode,
    };
    use inkos_engine::models::genre_profile::GenreProfile;
    use inkos_engine::models::input_governance::{ContextPackage, RuleStack};
    use inkos_engine::models::length_governance::LengthSpec;

    let auto_mode = |value: &str| match value {
        "patch-only" => AutoOutputMode::PatchOnly,
        "rewrite-only" => AutoOutputMode::RewriteOnly,
        _ => AutoOutputMode::AllowFull,
    };
    let mode = |value: &str| match value {
        "polish" => ReviseMode::Polish,
        "rewrite" => ReviseMode::Rewrite,
        "rework" => ReviseMode::Rework,
        "anti-detect" => ReviseMode::AntiDetect,
        "spot-fix" => ReviseMode::SpotFix,
        _ => ReviseMode::Auto,
    };
    let lang = |value: &str| {
        if value == "en" {
            inkos_engine::utils::language::WritingLanguage::En
        } else {
            inkos_engine::utils::language::WritingLanguage::Zh
        }
    };

    for c in &load().reviser_private_suite {
        let input = &c.input;
        let got: serde_json::Value = match c.name.as_str() {
            name if name.starts_with("parse-") => {
                let numerical = input["numericalSystem"].as_bool().unwrap();
                let out = parse_reviser_output(
                    input["content"].as_str().unwrap(),
                    numerical,
                    mode(input["mode"].as_str().unwrap()),
                    input["originalChapter"].as_str().unwrap(),
                    auto_mode(input["autoOutputMode"].as_str().unwrap()),
                );
                serde_json::json!({
                    "revisedContent": out.revised_content,
                    "wordCount": out.word_count,
                    "fixedIssues": out.fixed_issues,
                    "updatedState": out.updated_state,
                    "updatedLedger": out.updated_ledger,
                    "updatedHooks": out.updated_hooks,
                })
            }
            name if name.starts_with("auto-system-prompt-") => {
                let gp: GenreProfile = serde_json::from_value(input["genreProfile"].clone()).unwrap();
                let length_spec: Option<LengthSpec> =
                    serde_json::from_value(input["lengthSpec"].clone()).ok().flatten();
                let out = build_auto_system_prompt(
                    input["langPrefix"].as_str().unwrap(),
                    &gp,
                    input["protagonistBlock"].as_str().unwrap(),
                    input["numericalRule"].as_str().unwrap(),
                    lang(input["language"].as_str().unwrap()),
                    length_spec.as_ref(),
                    auto_mode(input["autoOutputMode"].as_str().unwrap()),
                );
                serde_json::to_value(out).unwrap()
            }
            name if name.starts_with("legacy-system-prompt-") => {
                let gp: GenreProfile = serde_json::from_value(input["genreProfile"].clone()).unwrap();
                let out = build_legacy_system_prompt(
                    input["langPrefix"].as_str().unwrap(),
                    &gp,
                    input["protagonistBlock"].as_str().unwrap(),
                    input["numericalRule"].as_str().unwrap(),
                    input["lengthGuardrail"].as_str().unwrap(),
                    mode(input["mode"].as_str().unwrap()),
                );
                serde_json::to_value(out).unwrap()
            }
            "reduced-control-block" => {
                let package: ContextPackage =
                    serde_json::from_value(input["contextPackage"].clone()).unwrap();
                let rule_stack: RuleStack =
                    serde_json::from_value(input["ruleStack"].clone()).unwrap();
                let out = build_reduced_control_block(
                    None,
                    None,
                    input["chapterIntent"].as_str(),
                    &package,
                    &rule_stack,
                );
                serde_json::to_value(out).unwrap()
            }
            other => panic!("未知 reviser suite case: {other}"),
        };
        assert_eq!(got, c.expected, "case `{}`", c.name);
    }
}

// ---- 37 号：length-normalizer + state-degraded note ----

#[test]
fn length_normalizer_suite_matches_ts() {
    use inkos_engine::agents::length_normalizer::{
        build_system_prompt, build_user_prompt, build_warning, crosses_opposite_hard_bound,
        looks_truncated, sanitize_normalized_content, NormalizeLengthInput,
    };
    use inkos_engine::models::length_governance::{
        LengthCountingMode, LengthNormalizeMode, LengthSpec,
    };

    let mode = |value: &str| match value {
        "compress" => LengthNormalizeMode::Compress,
        "expand" => LengthNormalizeMode::Expand,
        _ => LengthNormalizeMode::None,
    };
    let spec_from = |value: &serde_json::Value| LengthSpec {
        target: value["target"].as_u64().unwrap() as u32,
        soft_min: value["softMin"].as_u64().unwrap() as u32,
        soft_max: value["softMax"].as_u64().unwrap() as u32,
        hard_min: value["hardMin"].as_u64().unwrap() as u32,
        hard_max: value["hardMax"].as_u64().unwrap() as u32,
        counting_mode: LengthCountingMode::ZhChars,
        normalize_mode: LengthNormalizeMode::None,
    };

    for c in &load().length_normalizer_suite {
        let input = &c.input;
        let got: serde_json::Value = match c.name.as_str() {
            "system-compress" | "system-expand" => {
                serde_json::to_value(build_system_prompt(mode(input["mode"].as_str().unwrap())))
                    .unwrap()
            }
            "user-full" | "user-minimal" => {
                let spec = spec_from(&input["input"]["lengthSpec"]);
                let params = NormalizeLengthInput {
                    chapter_content: input["input"]["chapterContent"].as_str().unwrap(),
                    length_spec: &spec,
                    chapter_intent: input["input"]["chapterIntent"].as_str(),
                    reduced_control_block: input["input"]["reducedControlBlock"].as_str(),
                };
                let count = input["originalCount"].as_u64().unwrap() as u32;
                serde_json::to_value(build_user_prompt(
                    &params,
                    count,
                    mode(input["mode"].as_str().unwrap()),
                ))
                .unwrap()
            }
            name if name.starts_with("sanitize-") => {
                serde_json::to_value(sanitize_normalized_content(
                    input["raw"].as_str().unwrap(),
                    input["fallback"].as_str().unwrap(),
                ))
                .unwrap()
            }
            "truncated-matrix" => {
                let contents: Vec<&str> = input["contents"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .map(|v| v.as_str().unwrap())
                    .collect();
                serde_json::to_value(
                    contents.iter().map(|c| looks_truncated(c)).collect::<Vec<bool>>(),
                )
                .unwrap()
            }
            name if name.starts_with("warning-") => {
                let spec = spec_from(&input["lengthSpec"]);
                serde_json::to_value(build_warning(
                    input["finalCount"].as_u64().unwrap() as u32,
                    &spec,
                ))
                .unwrap()
            }
            name if name.starts_with("cross-") => {
                let spec = spec_from(&input["lengthSpec"]);
                serde_json::to_value(crosses_opposite_hard_bound(
                    input["originalCount"].as_u64().unwrap() as u32,
                    input["candidateCount"].as_u64().unwrap() as u32,
                    &spec,
                ))
                .unwrap()
            }
            other => panic!("未知 length normalizer case: {other}"),
        };
        assert_eq!(got, c.expected, "case `{}`", c.name);
    }
}

#[test]
fn state_degraded_note_matches_ts() {
    use inkos_engine::agents::continuity::{AuditIssue, AuditSeverity};
    use inkos_engine::models::chapter::{ChapterMeta, ChapterStatus};
    use inkos_engine::pipeline::chapter_state_recovery::{
        build_state_degraded_review_note, parse_state_degraded_review_note,
        resolve_state_degraded_base_status,
    };

    let issue_from = |value: &serde_json::Value| AuditIssue {
        severity: match value["severity"].as_str().unwrap() {
            "critical" => AuditSeverity::Critical,
            "warning" => AuditSeverity::Warning,
            _ => AuditSeverity::Info,
        },
        category: value["category"].as_str().unwrap().to_string(),
        description: value["description"].as_str().unwrap().to_string(),
        suggestion: value["suggestion"].as_str().unwrap_or("").to_string(),
        repair_scope: None,
    };

    let meta_from = |audit_issues: Vec<String>, review_note: Option<String>| ChapterMeta {
        number: 3,
        title: "t".to_string(),
        status: ChapterStatus::StateDegraded,
        word_count: 0,
        created_at: String::new(),
        updated_at: String::new(),
        audit_issues,
        length_warnings: vec![],
        review_note,
        detection_score: None,
        detection_provider: None,
        detected_at: None,
        length_telemetry: None,
        token_usage: None,
    };

    for c in &load().state_degraded_note {
        let input = &c.input;
        let got: serde_json::Value = match c.name.as_str() {
            "build" => {
                let issues: Vec<AuditIssue> = input["issues"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .map(&issue_from)
                    .collect();
                serde_json::to_value(build_state_degraded_review_note(
                    input["baseStatus"].as_str().unwrap(),
                    &issues,
                ))
                .unwrap()
            }
            name if name.starts_with("parse-") => {
                serde_json::to_value(parse_state_degraded_review_note(
                    input["note"].as_str(),
                ))
                .unwrap_or(serde_json::Value::Null)
            }
            name if name.starts_with("resolve-") => {
                let meta = match input["meta"].as_str().unwrap() {
                    "with-note" => {
                        let note = build_state_degraded_review_note("audit-failed", &[]);
                        meta_from(vec!["[critical] 主线偏离".to_string()], Some(note))
                    }
                    "critical" => meta_from(vec!["[critical] 主线偏离".to_string()], None),
                    "warning" => meta_from(vec!["[warning] 小问题".to_string()], None),
                    other => panic!("未知 meta 变体: {other}"),
                };
                serde_json::to_value(resolve_state_degraded_base_status(&meta)).unwrap()
            }
            other => panic!("未知 note case: {other}"),
        };
        assert_eq!(got, c.expected, "case `{}`", c.name);
    }
}
