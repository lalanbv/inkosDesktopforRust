//! 写作语言推断。
//!
//! 移植自 `packages/core/src/utils/language.ts`（17 行，纯函数）。
//!
//! ## 保守策略（对齐 TS）
//! 默认 `Zh`（保留中文用户既有行为）；仅当文本明显拉丁主导时返回 `En`：
//! - CJK=0 且 Latin>0 → `En`
//! - Latin>0 且 `CJK*4 < Latin` → `En`
//! - 否则 → `Zh`
//!
//! 故：含英文专名的中文简介仍判 `Zh`；以英文为主、夹杂少量 CJK 的仍判 `En`。

use serde::{Deserialize, Serialize};
#[cfg(feature = "export-bindings")]
use ts_rs::TS;

/// 写作语言。序列化为小写字符串（`"zh"` / `"en"`），对齐 TS `"zh" | "en"` 字面量联合。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "export-bindings", derive(TS))]
#[cfg_attr(feature = "export-bindings", ts(export, type = "\"zh\" | \"en\""))]
pub enum WritingLanguage {
    #[serde(rename = "zh")]
    Zh,
    #[serde(rename = "en")]
    En,
}

/// CJK 统一表意文字范围（U+4E00..U+9FFF），对齐 TS `[一-鿿]`。
#[inline]
fn is_cjk_ideograph(c: char) -> bool {
    ('\u{4E00}'..='\u{9FFF}').contains(&c)
}

/// 字符串的 UTF-16 码元长度。与 JS `String.prototype.length` 同源
/// （BMP 外字符计 2），是所有「字符数阈值」分支点的 JS parity 基准。
#[inline]
pub fn utf16_len(s: &str) -> usize {
    s.encode_utf16().count()
}

/// 从自由文本（简介/premise）推断写作语言。None/空文本 → `Zh`（保守默认）。
pub fn infer_language(text: Option<&str>) -> WritingLanguage {
    let t = text.unwrap_or("");
    let mut cjk = 0usize;
    let mut latin = 0usize;
    for c in t.chars() {
        if is_cjk_ideograph(c) {
            cjk += 1;
        } else if c.is_ascii_alphabetic() {
            latin += 1;
        }
    }
    // TS 原文有两条独立判断（cjk==0&&latin>0 与 latin>0&&cjk*4<latin），但前者被后者
    // 完全覆盖：cjk==0 时 cjk*4=0 < latin ⟺ latin>0。合并为单条件，语义等价（由 golden 差分守门）。
    if latin > 0 && cjk * 4 < latin {
        WritingLanguage::En
    } else {
        WritingLanguage::Zh
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn none_or_empty_defaults_zh() {
        assert_eq!(infer_language(None), WritingLanguage::Zh);
        assert_eq!(infer_language(Some("")), WritingLanguage::Zh);
    }

    #[test]
    fn latin_only_is_en() {
        assert_eq!(infer_language(Some("A dark fantasy novel")), WritingLanguage::En);
        assert_eq!(infer_language(Some("hello world")), WritingLanguage::En);
    }

    #[test]
    fn cjk_only_is_zh() {
        assert_eq!(infer_language(Some("夜港账本")), WritingLanguage::Zh);
        assert_eq!(infer_language(Some("天机破诡")), WritingLanguage::Zh);
    }

    #[test]
    fn cjk_with_incidental_english_stays_zh() {
        // 含英文专名/术语的中文简介仍判 Zh
        assert_eq!(infer_language(Some("主角觉醒了 System 面板")), WritingLanguage::Zh);
    }

    #[test]
    fn english_with_incidental_cjk_becomes_en() {
        // 英文主导、夹杂少量 CJK → En（cjk*4 < latin）
        assert_eq!(
            infer_language(Some("这是一段中文 but mostly English content here")),
            WritingLanguage::En
        );
    }

    #[test]
    fn utf16_len_counts_surrogate_pairs_as_two() {
        assert_eq!(utf16_len("abc"), 3);
        assert_eq!(utf16_len("中文"), 2);
        // U+1F600（emoji）在 UTF-16 中是代理对，占 2 码元。
        assert_eq!(utf16_len("a😀b"), 4);
        assert_eq!(utf16_len(""), 0);
    }

    #[test]
    fn serializes_to_lowercase_string() {
        assert_eq!(serde_json::to_string(&WritingLanguage::Zh).unwrap(), "\"zh\"");
        assert_eq!(serde_json::to_string(&WritingLanguage::En).unwrap(), "\"en\"");
        // 反序列化往返
        let zh: WritingLanguage = serde_json::from_str("\"zh\"").unwrap();
        assert_eq!(zh, WritingLanguage::Zh);
    }
}
