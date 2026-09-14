//! 防呆写出方言（G7c/332 号）。
//!
//! TS 真源：`packages/core/src/utils/truth-dialect.ts`；共享向量：
//! `packages/core/src/__tests__/golden/truth-dialect-vectors.json`
//! （差分测试 `tests/golden_truth_dialect_diff.rs`）。
//!
//! 平铺 front matter / 块列表 / 危险值双引号转义 / 写出前剥前导 BOM——
//! 规则详见 TS 模块 doc。

use serde::Serialize;
use std::sync::OnceLock;

/// 剥除一个前导 BOM（\u{FEFF}）；只剥第一个，中间出现的 BOM 是内容不动。
pub fn strip_utf8_bom(content: &str) -> String {
    let mut chars = content.chars();
    match chars.next() {
        Some('\u{feff}') => chars.as_str().to_string(),
        _ => content.to_string(),
    }
}

/// 平铺为单行标量：换行（CRLF/CR/LF）与制表符折叠为单空格，去首尾空白。
pub fn flatten_truth_scalar(value: &str) -> String {
    value
        .replace("\r\n", "\n")
        .replace('\r', "\n")
        .replace(['\n', '\t'], " ")
        .trim()
        .to_string()
}

/// 判定平铺后的标量是否必须加引号（YAML 指示符/歧义形/转义字符等）。
pub fn is_dangerous_truth_value(flat: &str) -> bool {
    if flat.is_empty() {
        return true;
    }
    const INDICATORS: [char; 19] = [
        '-', '?', ':', '!', '&', '*', '[', ']', '{', '}', '>', '|', '%', '#', '`', '"', '\'', '@',
        ',',
    ];
    let first = flat.chars().next().unwrap();
    if INDICATORS.contains(&first) {
        return true;
    }
    if flat.contains(": ") {
        return true;
    }
    if flat.ends_with(':') {
        return true;
    }
    if flat.contains(" #") {
        return true;
    }
    if flat.contains('"') || flat.contains('\\') || flat.contains('|') {
        return true;
    }
    if matches!(
        flat.to_ascii_lowercase().as_str(),
        "true" | "false" | "null" | "yes" | "no" | "on" | "off" | "~"
    ) {
        return true;
    }
    numeric_shape_re().is_match(flat)
}

/// TS `/^[-+]?(\d+\.?\d*|\.\d+)(e[-+]?\d+)?$/i`。
fn numeric_shape_re() -> &'static regex::Regex {
    static RE: OnceLock<regex::Regex> = OnceLock::new();
    RE.get_or_init(|| {
        regex::Regex::new(r"(?i)^[-+]?(\d+\.?\d*|\.\d+)(e[-+]?\d+)?$").unwrap()
    })
}

/// 双引号包裹 + 反斜杠/双引号转义。入参须已平铺。
pub fn quote_truth_value(flat: &str) -> String {
    format!("\"{}\"", flat.replace('\\', "\\\\").replace('"', "\\\""))
}

/// 方言标量渲染：平铺 → 危险判定 →（按需）转义加引号。
pub fn render_truth_value(value: &str) -> String {
    let flat = flatten_truth_scalar(value);
    if is_dangerous_truth_value(&flat) {
        quote_truth_value(&flat)
    } else {
        flat
    }
}

/// 平铺 meta 块：`---` 开头、顶层 `key: value` 每条一行。
/// key 是代码控制的标识符（仅平铺不判危）；value 走完整方言渲染。
pub fn render_flat_meta_block(entries: &[(&str, &str)]) -> String {
    let mut lines = vec!["---".to_string()];
    for (key, value) in entries {
        lines.push(format!(
            "{}: {}",
            flatten_truth_scalar(key),
            render_truth_value(value)
        ));
    }
    lines.join("\n")
}

/// 块列表：`- ` 前缀每条一行，逐条过方言标量渲染；空列表产出 empty_marker。
pub fn render_truth_block_list(items: &[&str], empty_marker: &str) -> String {
    if items.is_empty() {
        return empty_marker.to_string();
    }
    items
        .iter()
        .map(|item| format!("- {}", render_truth_value(item)))
        .collect::<Vec<_>>()
        .join("\n")
}

/// 机器可读方言契约（双端 golden 锁形状）。
#[derive(Debug, Clone, Serialize)]
pub struct TruthDialectContract {
    pub version: u32,
    pub dialect: &'static str,
    pub rules: Vec<&'static str>,
}

pub fn truth_dialect_contract() -> TruthDialectContract {
    TruthDialectContract {
        version: 1,
        dialect: "inkos-truth-dialect",
        rules: vec![
            "flat-single-line-scalars-only",
            "dangerous-values-double-quoted-and-escaped",
            "block-list-dash-prefix-per-line",
            "strip-leading-utf8-bom-before-write",
        ],
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn quotes_yaml_indicators_and_escapes() {
        assert_eq!(render_truth_value("他说\"走\"便走"), "\"他说\\\"走\\\"便走\"");
        assert_eq!(render_truth_value("@某人"), "\"@某人\"");
        assert_eq!(render_truth_value("42"), "\"42\"");
        assert_eq!(render_truth_value("2026-09-12T00:00:00.000Z"), "2026-09-12T00:00:00.000Z");
        assert_eq!(render_truth_value("第一行\n第二行"), "第一行 第二行");
    }

    #[test]
    fn bom_strip_is_leading_only() {
        assert_eq!(strip_utf8_bom("\u{feff}正文"), "正文");
        assert_eq!(strip_utf8_bom("\u{feff}\u{feff}正文"), "\u{feff}正文");
        assert_eq!(strip_utf8_bom("正文"), "正文");
    }
}
