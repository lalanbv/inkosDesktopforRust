//! 精修补丁（spot-fix）解析与应用。
//!
//! 移植自 `packages/core/src/utils/spot-fix-patches.ts`（189 行，纯函数）。
//! 解析结构化补丁块（TARGET_TEXT / REPLACEMENT_TEXT），对原文按补丁逐个替换：
//! 先精确匹配（需唯一），失败则模糊匹配（空白归一化），仍失败则跳过该补丁。

use regex::Regex;
use serde::{Deserialize, Serialize};
#[cfg(feature = "export-bindings")]
use ts_rs::TS;
use std::sync::OnceLock;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[cfg_attr(feature = "export-bindings", derive(TS))]
#[cfg_attr(feature = "export-bindings", ts(export))]
#[serde(rename_all = "camelCase")]
pub struct SpotFixPatch {
    pub target_text: String,
    pub replacement_text: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[cfg_attr(feature = "export-bindings", derive(TS))]
#[cfg_attr(feature = "export-bindings", ts(export))]
#[serde(rename_all = "camelCase")]
pub struct SpotFixPatchApplyResult {
    pub applied: bool,
    pub revised_content: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub rejected_reason: Option<String>,
    pub applied_patch_count: u32,
    pub skipped_patch_count: u32,
    pub touched_chars: u32,
}

fn patch_re() -> &'static Regex {
    static R: OnceLock<Regex> = OnceLock::new();
    R.get_or_init(|| {
        Regex::new(r"(?s)--- PATCH(?:\s+\d+)? ---\s*TARGET_TEXT:\s*(.*?)\s*REPLACEMENT_TEXT:\s*(.*?)\s*--- END PATCH ---").unwrap()
    })
}

/// 从原始输出解析补丁列表。可带 `=== PATCHES ===` 前缀。
pub fn parse_spot_fix_patches(raw: &str) -> Vec<SpotFixPatch> {
    let normalized = if let Some(idx) = raw.find("=== PATCHES ===") {
        &raw[idx + "=== PATCHES ===".len()..]
    } else {
        raw
    };
    let mut patches = Vec::new();
    for caps in patch_re().captures_iter(normalized) {
        patches.push(SpotFixPatch {
            target_text: trim_field(&caps[1]),
            replacement_text: trim_field(&caps[2]),
        });
    }
    patches.into_iter().filter(|p| !p.target_text.is_empty()).collect()
}

/// 对原文应用补丁（每补丁独立精确/模糊匹配，失败跳过）。
pub fn apply_spot_fix_patches(original: &str, patches: &[SpotFixPatch]) -> SpotFixPatchApplyResult {
    if patches.is_empty() {
        return SpotFixPatchApplyResult {
            applied: false,
            revised_content: original.to_string(),
            rejected_reason: Some("No valid patches returned.".into()),
            applied_patch_count: 0,
            skipped_patch_count: 0,
            touched_chars: 0,
        };
    }
    let mut current = original.to_string();
    let mut applied = 0u32;
    let mut skipped = 0u32;
    let mut touched = 0u32;
    for patch in patches {
        if let Some(new_content) = try_apply_patch(&current, patch) {
            touched += patch.target_text.chars().count() as u32;
            current = new_content;
            applied += 1;
        } else {
            skipped += 1;
        }
    }
    SpotFixPatchApplyResult {
        applied: applied > 0 && current != original,
        revised_content: current,
        rejected_reason: if applied == 0 { Some("No patches could be matched to the chapter content.".into()) } else { None },
        applied_patch_count: applied,
        skipped_patch_count: skipped,
        touched_chars: touched,
    }
}

fn try_apply_patch(content: &str, patch: &SpotFixPatch) -> Option<String> {
    // 1. 精确匹配（需唯一）
    if let Some(byte_start) = find_unique(content, &patch.target_text) {
        let mut out = String::with_capacity(content.len());
        out.push_str(&content[..byte_start]);
        out.push_str(&patch.replacement_text);
        out.push_str(&content[byte_start + patch.target_text.len()..]);
        return Some(out);
    }
    // 2. 模糊匹配（空白归一化）
    if let Some((start, end)) = try_fuzzy_match(content, &patch.target_text) {
        let mut out = String::with_capacity(content.len());
        out.push_str(&content[..start]);
        out.push_str(&patch.replacement_text);
        out.push_str(&content[end..]);
        return Some(out);
    }
    None
}

/// 唯一子串查找：返回第一个出现的字节起点；若出现多次返回 None。
fn find_unique(content: &str, target: &str) -> Option<usize> {
    let start = content.find(target)?;
    if content[start + target.len()..].contains(target) {
        None
    } else {
        Some(start)
    }
}

fn try_fuzzy_match(content: &str, target: &str) -> Option<(usize, usize)> {
    // 归一化（空白折叠 + trim），按 char 工作
    let normalized_target: Vec<char> = normalize_whitespace_chars(target);
    if normalized_target.len() < 10 {
        return None;
    }
    let content_chars: Vec<char> = content.chars().collect();
    let normalized_content: Vec<char> = normalize_whitespace_chars(content);

    // 在归一化内容中找唯一 target（char 索引）
    let match_start = find_subsequence(&normalized_content, &normalized_target)?;
    let after = &normalized_content[match_start + normalized_target.len()..];
    if find_subsequence(after, &normalized_target).is_some() {
        return None;
    }
    let match_end_norm = match_start + normalized_target.len();

    // 映射归一化 char 索引回原始 char 索引，再回字节偏移
    let map = build_norm_to_orig_map(&content_chars);
    let orig_start_char = *map.get(match_start)?;
    let orig_end_char = *map.get(match_end_norm)?;
    // char 索引 → 字节偏移
    let mut byte_start = 0usize;
    for (i, (b, _len)) in content.char_indices().enumerate() {
        if i == orig_start_char {
            byte_start = b;
            break;
        }
    }
    let mut byte_end = content.len();
    for (i, (b, _len)) in content.char_indices().enumerate() {
        if i == orig_end_char {
            byte_end = b;
            break;
        }
    }
    Some((byte_start, byte_end))
}

/// 归一化为 char 序列：连续空白折叠为单空格 + trim。
fn normalize_whitespace_chars(text: &str) -> Vec<char> {
    let mut out: Vec<char> = Vec::new();
    let chars: Vec<char> = text.chars().collect();
    let mut in_ws = false;
    let mut started = false;
    for &c in &chars {
        if c.is_whitespace() {
            if started {
                in_ws = true;
            }
        } else {
            if in_ws {
                out.push(' ');
            }
            out.push(c);
            in_ws = false;
            started = true;
        }
    }
    out
}

/// 在 haystack 中找 needle 子序列的首个起始索引。
fn find_subsequence(haystack: &[char], needle: &[char]) -> Option<usize> {
    if needle.is_empty() || needle.len() > haystack.len() {
        return None;
    }
    (0..=haystack.len() - needle.len()).find(|&i| haystack[i..i + needle.len()] == *needle)
}

/// 构建归一化 char 索引 → 原始 char 索引 的映射（按相同折叠规则）。
fn build_norm_to_orig_map(orig_chars: &[char]) -> Vec<usize> {
    let mut map = Vec::new();
    let mut in_ws = false;
    let mut started = false;
    for (oi, &c) in orig_chars.iter().enumerate() {
        if c.is_whitespace() {
            if started {
                in_ws = true;
            }
        } else {
            if in_ws {
                map.push(oi); // 归一化的空格对应原始的「下一非空白字符起点」前一个位置近似
                // 注：归一化空格是逻辑插入，映射到当前非空白字符起点（用于边界）
            }
            map.push(oi);
            in_ws = false;
            started = true;
        }
    }
    map
}

fn trim_field(value: &str) -> String {
    let s = value.strip_prefix('\n').unwrap_or(value);
    let s = s.strip_suffix('\n').unwrap_or(s);
    s.trim().to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_extracts_patches() {
        let raw = "=== PATCHES ===\n--- PATCH 1 ---\nTARGET_TEXT:\n旧文本\nREPLACEMENT_TEXT:\n新文本\n--- END PATCH ---\n--- PATCH 2 ---\nTARGET_TEXT:\n另一处\nREPLACEMENT_TEXT:\n改\n--- END PATCH ---";
        let patches = parse_spot_fix_patches(raw);
        assert_eq!(patches.len(), 2);
        assert_eq!(patches[0].target_text, "旧文本");
        assert_eq!(patches[0].replacement_text, "新文本");
    }

    #[test]
    fn parse_filters_empty_target() {
        let raw = "--- PATCH ---\nTARGET_TEXT:\n\nREPLACEMENT_TEXT:\n非空\n--- END PATCH ---";
        assert!(parse_spot_fix_patches(raw).is_empty());
    }

    #[test]
    fn apply_exact_unique_match() {
        let original = "开头 中间文本 结尾";
        let patches = vec![SpotFixPatch { target_text: "中间文本".into(), replacement_text: "替换".into() }];
        let r = apply_spot_fix_patches(original, &patches);
        assert!(r.applied);
        assert_eq!(r.revised_content, "开头 替换 结尾");
        assert_eq!(r.applied_patch_count, 1);
        assert_eq!(r.touched_chars, 4);
    }

    #[test]
    fn apply_skips_non_unique_match() {
        let original = "重复 重复 其他";
        let patches = vec![SpotFixPatch { target_text: "重复".into(), replacement_text: "X".into() }];
        let r = apply_spot_fix_patches(original, &patches);
        assert!(!r.applied);
        assert_eq!(r.skipped_patch_count, 1);
    }

    #[test]
    fn apply_fuzzy_whitespace_normalized() {
        // 归一化后 ≥10 字符（fuzzy 阈值）
        let original = "他\n慢慢地  走了过来。";
        let patches = vec![SpotFixPatch { target_text: "他 慢慢地 走了过来".into(), replacement_text: "他来了".into() }];
        let r = apply_spot_fix_patches(original, &patches);
        assert!(r.applied, "应通过模糊匹配应用");
        assert!(r.revised_content.contains("他来了"));
    }

    #[test]
    fn apply_empty_patches_rejects() {
        let r = apply_spot_fix_patches("内容", &[]);
        assert!(!r.applied);
        assert!(r.rejected_reason.is_some());
    }
}
