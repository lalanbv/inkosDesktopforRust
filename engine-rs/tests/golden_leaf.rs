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
}

const LEAF_JSON: &str = include_str!("golden/utils/leaf.json");

fn load() -> LeafGolden {
    serde_json::from_str(LEAF_JSON).expect("leaf.json 应为合法 JSON")
}

#[test]
fn derive_book_id_matches_ts() {
    for c in &load().derive_book_id {
        let input = c.input.as_str().unwrap_or_else(|| panic!("case {}: input 非 string", c.name));
        let got = inkos_engine::utils::derive_book_id_from_title(input);
        let want = c.expected.as_str().unwrap_or_else(|| panic!("case {}: expected 非 string", c.name));
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
        let want = c.expected.as_bool().unwrap_or_else(|| panic!("case {}: expected 非 bool", c.name));
        assert_eq!(got, want, "case `{}`: is_safe_book_id 与 TS 不一致 (input={:?})", c.name, c.input);
    }
}

#[test]
fn infer_language_matches_ts() {
    for c in &load().infer_language {
        // TS: string | null | undefined。null/absent → None；string → Some
        let input_opt: Option<&str> = c.input.as_str();
        let got = inkos_engine::utils::infer_language(input_opt);
        let want = c.expected.as_str().unwrap_or_else(|| panic!("case {}: expected 非 string", c.name));
        let got_str = match got {
            inkos_engine::utils::WritingLanguage::Zh => "zh",
            inkos_engine::utils::WritingLanguage::En => "en",
        };
        assert_eq!(got_str, want, "case `{}`: infer_language 与 TS 不一致 (input={:?})", c.name, c.input);
    }
}

#[test]
fn to_posix_path_matches_ts() {
    for c in &load().to_posix_path {
        let input = c.input.as_str().unwrap_or_else(|| panic!("case {}: input 非 string", c.name));
        let got = inkos_engine::utils::to_posix_path(input);
        let want = c.expected.as_str().unwrap_or_else(|| panic!("case {}: expected 非 string", c.name));
        assert_eq!(got, want, "case `{}`: to_posix_path 与 TS 不一致", c.name);
    }
}

#[test]
fn count_chapter_length_matches_ts() {
    use inkos_engine::models::length_governance::LengthCountingMode;
    for c in &load().count_chapter_length {
        let content = c.input["content"].as_str().unwrap_or_else(|| panic!("case {}: content 缺失", c.name));
        let mode = match c.input["mode"].as_str().unwrap_or_else(|| panic!("case {}: mode 缺失", c.name)) {
            "zh_chars" => LengthCountingMode::ZhChars,
            "en_words" => LengthCountingMode::EnWords,
            other => panic!("case {}: 未知 mode {other}", c.name),
        };
        let got = inkos_engine::utils::count_chapter_length(content, mode);
        let want = c.expected.as_u64().unwrap_or_else(|| panic!("case {}: expected 非 u64", c.name));
        assert_eq!(got as u64, want, "case `{}`: count_chapter_length 与 TS 不一致", c.name);
    }
}

#[test]
fn build_length_spec_matches_ts() {
    use inkos_engine::utils::WritingLanguage;
    for c in &load().build_length_spec {
        let target = c.input["target"].as_u64().unwrap_or_else(|| panic!("case {}: target 缺失", c.name)) as u32;
        let lang = match c.input["language"].as_str().unwrap_or_else(|| panic!("case {}: language 缺失", c.name)) {
            "zh" => WritingLanguage::Zh,
            "en" => WritingLanguage::En,
            other => panic!("case {}: 未知 language {other}", c.name),
        };
        let got = inkos_engine::utils::build_length_spec(target, lang);
        // 序列化为 JSON 后逐字段比对（验证 camelCase 字段名 + 数值都对齐 TS）
        let got_json = serde_json::to_value(&got).expect("LengthSpec 序列化失败");
        assert_eq!(got_json, c.expected, "case `{}`: build_length_spec 与 TS 不一致", c.name);
    }
}

#[test]
fn format_length_count_matches_ts() {
    use inkos_engine::models::length_governance::LengthCountingMode;
    for c in &load().format_length_count {
        let count = c.input["count"].as_u64().unwrap_or_else(|| panic!("case {}: count 缺失", c.name)) as u32;
        let mode = match c.input["mode"].as_str().unwrap_or_else(|| panic!("case {}: mode 缺失", c.name)) {
            "zh_chars" => LengthCountingMode::ZhChars,
            "en_words" => LengthCountingMode::EnWords,
            other => panic!("case {}: 未知 mode {other}", c.name),
        };
        let got = inkos_engine::utils::format_length_count(count, mode);
        let want = c.expected.as_str().unwrap_or_else(|| panic!("case {}: expected 非 string", c.name));
        assert_eq!(got, want, "case `{}`: format_length_count 与 TS 不一致", c.name);
    }
}

#[test]
fn resolve_length_counting_mode_matches_ts() {
    use inkos_engine::utils::WritingLanguage;
    for c in &load().resolve_length_counting_mode {
        let lang = match c.input.as_str().unwrap_or_else(|| panic!("case {}: input 非 string", c.name)) {
            "zh" => WritingLanguage::Zh,
            "en" => WritingLanguage::En,
            other => panic!("case {}: 未知 language {other}", c.name),
        };
        let got = inkos_engine::utils::resolve_length_counting_mode(lang);
        let got_str = match got {
            inkos_engine::models::length_governance::LengthCountingMode::ZhChars => "zh_chars",
            inkos_engine::models::length_governance::LengthCountingMode::EnWords => "en_words",
        };
        let want = c.expected.as_str().unwrap_or_else(|| panic!("case {}: expected 非 string", c.name));
        assert_eq!(got_str, want, "case `{}`: resolve_length_counting_mode 与 TS 不一致", c.name);
    }
}
