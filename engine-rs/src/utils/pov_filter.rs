//! POV（视角）感知的上下文过滤。
//!
//! 移植自 `packages/core/src/utils/pov-filter.ts`（149 行）。
//!
//! 依据当前 POV 角色的信息边界过滤真相文件：角色只能"看到"其亲历或被告知的信息。
//!
//! ## 移植要点
//! - 动态正则（含 chapterNumber）按调用编译（非热路径）
//! - TS `split(/(?=^###)/m)` 的前瞻分割在 Rust regex 不支持，改手写「按 ^### 行首切点分割」
//! - `\b` 默认 Unicode；对纯数字边界与 ASCII 行为一致

use regex::Regex;
use std::collections::HashSet;
use std::sync::OnceLock;

fn pov_decl_re() -> &'static Regex {
    static R: OnceLock<Regex> = OnceLock::new();
    R.get_or_init(|| Regex::new(r"(?i)(?:POV|视角|pov)[：:\s]+([^\s，,。.、]+)").unwrap())
}
fn h3_start_re() -> &'static Regex {
    static R: OnceLock<Regex> = OnceLock::new();
    R.get_or_init(|| Regex::new(r"(?m)^###").unwrap())
}
fn info_boundary_re() -> &'static Regex {
    static R: OnceLock<Regex> = OnceLock::new();
    R.get_or_init(|| Regex::new(r"(?i)信息边界|Information\s+Boundar").unwrap())
}
fn table_chapter_num_re() -> &'static Regex {
    static R: OnceLock<Regex> = OnceLock::new();
    R.get_or_init(|| Regex::new(r"\|\s*(\d+)\s*\|").unwrap())
}

/// 从卷大纲提取指定章节的 POV 角色。
///
/// 在章节段落内查找 `POV: 角色名` / `视角: 角色名` / `POV: Name` 声明。找不到返回 None。
pub fn extract_pov_from_outline(volume_outline: &str, chapter_number: u32) -> Option<String> {
    let lines: Vec<&str> = volume_outline.split('\n').collect();
    // 动态模式：按 chapterNumber 编译
    let p1 = Regex::new(&format!("第{chapter_number}章")).ok()?;
    let p2 = Regex::new(&format!(r"Chapter\s+{chapter_number}\b")).ok()?;
    let p3 = Regex::new(&format!(r"\b{chapter_number}\b.*章")).ok()?;

    let mut in_chapter_section = false;
    for line in &lines {
        if p1.is_match(line) || p2.is_match(line) || p3.is_match(line) {
            in_chapter_section = true;
        } else if in_chapter_section && line.starts_with(['#', '-']) && !line.contains(&chapter_number.to_string()) {
            break;
        }
        if in_chapter_section {
            if let Some(caps) = pov_decl_re().captures(line) {
                return Some(caps.get(1)?.as_str().to_string());
            }
        }
    }
    None
}

/// 按段落是否在 `^###` 行首之前切分（等价 TS `split(/(?=^###)/m)`，前瞻在 Rust regex 不支持）。
fn split_before_h3(s: &str) -> Vec<String> {
    let points: Vec<usize> = h3_start_re().find_iter(s).map(|m| m.start()).collect();
    let mut parts: Vec<String> = Vec::with_capacity(points.len() + 1);
    let mut prev = 0usize;
    for p in points {
        if p > prev {
            parts.push(s[prev..p].to_string());
            prev = p;
        }
    }
    parts.push(s[prev..].to_string());
    parts
}

/// 按信息边界段落是否命中关键词判定。
fn is_info_boundary(section: &str) -> bool {
    info_boundary_re().is_match(section)
}

/// 过滤角色矩阵：信息边界段只保留 POV 角色行，其余角色行隐藏并加注释。
pub fn filter_matrix_by_pov(character_matrix: &str, pov_character: &str) -> String {
    if character_matrix.is_empty() || character_matrix == "(文件尚未创建)" {
        return character_matrix.to_string();
    }
    if pov_character.is_empty() {
        return character_matrix.to_string();
    }

    let sections = split_before_h3(character_matrix);
    let filtered: Vec<String> = sections
        .iter()
        .map(|section| {
            if !is_info_boundary(section) {
                return section.clone();
            }
            let lines: Vec<&str> = section.split('\n').collect();
            let header_lines: Vec<&&str> = lines
                .iter()
                .filter(|l| {
                    l.starts_with('|')
                        && (l.contains("---") || l.contains("角色") || l.contains("Character") || l.contains("已知") || l.contains("Known"))
                })
                .collect();
            let data_lines: Vec<&&str> = lines
                .iter()
                .filter(|l| {
                    l.starts_with('|')
                        && !l.contains("---")
                        && !l.contains("角色")
                        && !l.contains("Character")
                        && !l.contains("已知")
                        && !l.contains("Known")
                })
                .collect();
            let pov_rows: Vec<&&str> = data_lines.iter().copied().filter(|l| l.contains(pov_character)).collect();
            let other_char_count = data_lines.len().saturating_sub(pov_rows.len());

            let section_header = lines.iter().find(|l| l.starts_with("###")).copied().unwrap_or("### 信息边界");
            let mut result = String::new();
            result.push_str(section_header);
            result.push('\n');
            result.push_str(&format!("（当前视角：{pov_character}，其他 {other_char_count} 个角色的信息边界已隐藏）"));
            result.push('\n');
            for h in &header_lines {
                result.push_str(h);
                result.push('\n');
            }
            for r in &pov_rows {
                result.push_str(r);
                result.push('\n');
            }
            // 末尾换行处理：TS 用 join("\n")，这里多了一个尾换行 → 去掉
            if result.ends_with('\n') {
                result.pop();
            }
            result
        })
        .collect();
    filtered.join("\n")
}

/// 按 POV 角色认知过滤 pending_hooks：POV 不在场章节埋的伏笔被隐藏（启发式）。
pub fn filter_hooks_by_pov(hooks: &str, pov_character: &str, chapter_summaries: &str) -> String {
    if hooks.is_empty() || hooks == "(文件尚未创建)" {
        return hooks.to_string();
    }
    if pov_character.is_empty() {
        return hooks.to_string();
    }

    let lines: Vec<&str> = hooks.split('\n').collect();
    let header_lines: Vec<&&str> = lines
        .iter()
        .filter(|l| l.starts_with('|') && (l.contains("hook_id") || l.contains("---")))
        .collect();
    let data_lines: Vec<&&str> = lines
        .iter()
        .filter(|l| l.starts_with('|') && !l.contains("hook_id") && !l.contains("---"))
        .collect();

    // 解析 summary 表，收集 POV 角色出场的章节号
    let mut pov_chapters: HashSet<u32> = HashSet::new();
    if !chapter_summaries.is_empty() {
        for line in chapter_summaries.split('\n') {
            if line.contains(pov_character) {
                if let Some(caps) = table_chapter_num_re().captures(line) {
                    if let Ok(n) = caps[1].parse::<u32>() {
                        pov_chapters.insert(n);
                    }
                }
            }
        }
    }

    let filtered: Vec<&&str> = data_lines
        .iter()
        .filter(|row| {
            if row.contains(pov_character) {
                return true;
            }
            let source_chapter = match table_chapter_num_re().captures(row) {
                Some(caps) => caps[1].parse::<u32>().ok(),
                None => return true, // 无法判定 → 保留
            };
            match source_chapter {
                Some(ch) => pov_chapters.contains(&ch),
                None => true,
            }
        })
        .copied()
        .collect();

    // 回退：过滤后全空则返回原 hooks
    if filtered.is_empty() && !data_lines.is_empty() {
        return hooks.to_string();
    }

    let non_table_lines: Vec<&&str> = lines.iter().filter(|l| !l.starts_with('|')).collect();
    let mut out: Vec<&str> = non_table_lines.into_iter().copied().collect();
    out.extend(header_lines.into_iter().copied());
    out.extend(filtered);
    out.join("\n")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extract_pov_finds_zh_declaration() {
        let outline = "第3章 觉醒\nPOV: 林动\n内容...\n第4章 离开";
        assert_eq!(extract_pov_from_outline(outline, 3), Some("林动".to_string()));
    }

    #[test]
    fn extract_pov_finds_en_chapter() {
        let outline = "Chapter 5 Confrontation\nPOV: Alice\n...";
        assert_eq!(extract_pov_from_outline(outline, 5), Some("Alice".to_string()));
    }

    #[test]
    fn extract_pov_returns_none_when_absent() {
        // 注意：输入不得含 "POV"/"视角"/"pov" 字样（否则会被声明正则匹配，与 TS 一致）
        let outline = "第3章 觉醒\n本章没有视角相关声明";
        assert_eq!(extract_pov_from_outline(outline, 3), None);
    }

    #[test]
    fn filter_matrix_passes_through_uncreated() {
        assert_eq!(filter_matrix_by_pov("(文件尚未创建)", "林动"), "(文件尚未创建)");
        assert_eq!(filter_matrix_by_pov("matrix", ""), "matrix");
    }

    #[test]
    fn filter_matrix_keeps_only_pov_row_in_info_boundary() {
        let matrix = "### 角色矩阵\n| 角色 | 已知 |\n| --- | --- |\n| 林动 | 系统秘密 |\n| 王胖 | 其他 |\n\n### 信息边界\n| 角色 | 已知 |\n| --- | --- |\n| 林动 | 知道A |\n| 王胖 | 知道B |\n";
        let out = filter_matrix_by_pov(matrix, "林动");
        // 信息边界段只保留林动行
        assert!(out.contains("知道A"));
        assert!(!out.contains("知道B"));
        // 含隐藏注释
        assert!(out.contains("其他 1 个角色的信息边界已隐藏"));
        // 非信息边界段保留原样（含 王胖 | 其他）
        assert!(out.contains("其他") && out.contains("系统秘密"));
    }

    #[test]
    fn filter_hooks_passes_through_uncreated() {
        assert_eq!(filter_hooks_by_pov("(文件尚未创建)", "林动", ""), "(文件尚未创建)");
    }

    #[test]
    fn filter_hooks_keeps_pov_present_chapters() {
        let hooks = "| hook_id | 章节 | 描述 |\n| --- | --- | --- |\n| H1 | 1 | 伏笔一 |\n| H2 | 2 | 伏笔二 |\n";
        // POV 在第 1 章出场 → H1 保留；H2 在第 2 章 POV 不在场 → 隐藏
        let summaries = "| 1 | 林动登场 |\n";
        let out = filter_hooks_by_pov(hooks, "林动", summaries);
        assert!(out.contains("H1"));
        assert!(!out.contains("H2"));
    }

    #[test]
    fn filter_hooks_keeps_directly_mentioned() {
        let hooks = "| hook_id | 章节 | 描述 |\n| --- | --- | --- |\n| H9 | 5 | 林动的秘密 |\n";
        let out = filter_hooks_by_pov(hooks, "林动", "");
        assert!(out.contains("H9")); // 直接提及 POV → 保留
    }
}
