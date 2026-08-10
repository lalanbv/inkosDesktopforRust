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
