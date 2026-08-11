//! Hook 账校验器（hook-ledger-validator）。
//!
//! 移植自 `packages/core/src/utils/hook-ledger-validator.ts`（277 行）。Phase 9-3 hard gate：
//! 校验章节正文确实兑现了 memo 中「## 本章 hook 账」声明的 advance/resolve hook。
//! 另强制「揭 1 埋 1」硬下限（resolve 时须 open ≥ resolve 数量的新 hook）。

use regex::Regex;
use std::collections::HashSet;
use std::sync::OnceLock;

/// 校验违规。对齐 TS `HookLedgerViolation`。
#[derive(Debug, Clone, PartialEq)]
pub struct HookLedgerViolation {
    pub severity: ViolationSeverity,
    pub category: String,
    pub description: String,
    pub suggestion: String,
}

/// 违规严重度。对齐 TS `"critical" | "warning"`。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ViolationSeverity {
    Critical,
    Warning,
}

/// 单条 hook 账记录。对齐 TS `HookLedgerEntry`。
#[derive(Debug, Clone, PartialEq)]
pub struct HookLedgerEntry {
    pub id: String,
    pub descriptor: String,
    pub keywords: Vec<String>,
}

/// 解析后的 hook 账。对齐 TS `HookLedger`。
#[derive(Debug, Clone, Default, PartialEq)]
pub struct HookLedger {
    pub open: Vec<HookLedgerEntry>,
    pub advance: Vec<HookLedgerEntry>,
    pub resolve: Vec<HookLedgerEntry>,
    pub defer: Vec<HookLedgerEntry>,
    pub new_open_count: usize,
}

/// 解析 memo 中的 hook 账段。对齐 TS `parseHookLedger`。
pub fn parse_hook_ledger(memo_body: &str) -> HookLedger {
    let section = match extract_ledger_section(memo_body) {
        Some(s) => s,
        None => return HookLedger::default(),
    };

    let mut open: Vec<HookLedgerEntry> = Vec::new();
    let mut advance = Vec::new();
    let mut resolve = Vec::new();
    let mut defer = Vec::new();
    let mut new_open_count = 0usize;
    let mut current: Option<Subsection> = None;

    for raw in section.lines() {
        let line = raw.trim();
        if line.is_empty() {
            continue;
        }
        if let Some(c) = subsection_heading(line) {
            current = Some(c);
            continue;
        }
        let Some(cur) = current else { continue };
        if !line.starts_with('-') {
            continue;
        }
        let cleaned = line.trim_start_matches('-').trim();
        if cur == Subsection::Open && new_placeholder_re().is_match(cleaned) {
            new_open_count += 1;
            continue;
        }
        if let Some(entry) = extract_ledger_entry(line) {
            match cur {
                Subsection::Open => open.push(entry),
                Subsection::Advance => advance.push(entry),
                Subsection::Resolve => resolve.push(entry),
                Subsection::Defer => defer.push(entry),
            }
        }
    }

    HookLedger {
        open,
        advance,
        resolve,
        defer,
        new_open_count,
    }
}

/// 校验 memo 声明的 hook 在正文中是否有证据 + 揭1埋1。对齐 TS `validateHookLedger`。
pub fn validate_hook_ledger(memo_body: &str, draft_content: &str) -> Vec<HookLedgerViolation> {
    let ledger = parse_hook_ledger(memo_body);
    let mut violations = Vec::new();

    // advance + resolve 去重后，逐条查正文证据。
    let mut committed: Vec<&HookLedgerEntry> = Vec::new();
    let mut seen: HashSet<&str> = HashSet::new();
    for e in ledger.advance.iter().chain(ledger.resolve.iter()) {
        if seen.insert(e.id.as_str()) {
            committed.push(e);
        }
    }
    for entry in &committed {
        if !draft_echoes_entry(draft_content, entry) {
            violations.push(HookLedgerViolation {
                severity: ViolationSeverity::Warning,
                category: "hook 账需语义复核".to_string(),
                description: format!(
                    "memo 在 advance/resolve 里声明要处理 {}，但确定性关键词检查没有找到对应落点",
                    entry.id
                ),
                suggestion: format!(
                    "复核正文是否已经用动作、对话、物件或信息变化推进了 {}；若没有，请补具体场景，若已推进，可忽略这条确定性提示",
                    entry.id
                ),
            });
        }
    }

    // 揭 1 埋 1 硬下限：resolve > 0 时，open ≥ resolve。
    let resolved_count = ledger.resolve.len();
    let opened_count = ledger.open.len() + ledger.new_open_count;
    if resolved_count > 0 && opened_count < resolved_count {
        violations.push(HookLedgerViolation {
            severity: ViolationSeverity::Critical,
            category: "hook 账揭 1 埋 1 违规".to_string(),
            description: format!(
                "本章 resolve 了 {resolved_count} 个钩子，但 open 只有 {opened_count} 个新钩子。只揭不埋会让读者豁然开朗后索然无味，本书的前进拉力被削弱。"
            ),
            suggestion: format!(
                "在 memo 的 open 段下至少再埋 {} 个与本章已揭钩子相关的新钩子。新钩子最好与已揭钩子彼此关联，不要凭空冒出来。",
                resolved_count - opened_count
            ),
        });
    }

    violations
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Subsection {
    Open,
    Advance,
    Resolve,
    Defer,
}

fn subsection_heading(line: &str) -> Option<Subsection> {
    let caps = subsection_heading_re().captures(line)?;
    Some(match &caps["key"].to_lowercase()[..] {
        "open" => Subsection::Open,
        "advance" => Subsection::Advance,
        "resolve" => Subsection::Resolve,
        "defer" => Subsection::Defer,
        _ => return None,
    })
}

/// 提取 hook 账 section（从 heading 到下一个 ##/### heading）。对齐 TS `extractLedgerSection`。
fn extract_ledger_section(memo_body: &str) -> Option<String> {
    for pat in [zh_heading_re(), en_heading_re()] {
        if let Some(caps) = pat.captures(memo_body) {
            let m = caps.get(0)?;
            let start = m.end();
            let rest = &memo_body[start..];
            let end = next_heading_re().find(rest).map(|m| m.start()).unwrap_or(rest.len());
            return Some(rest[..end].to_string());
        }
    }
    None
}

/// 从单行提取 ledger entry（剥 `-`、过滤 placeholder、提取 id+descriptor+keywords）。
fn extract_ledger_entry(line: &str) -> Option<HookLedgerEntry> {
    let cleaned = line.trim_start_matches('-').trim();
    if cleaned.starts_with("[new]") || cleaned.starts_with("[NEW]") {
        return None;
    }
    let first_word = cleaned.split_whitespace().next().unwrap_or("");
    if placeholder_re().is_match(first_word) {
        return None;
    }
    let caps = id_re().captures(cleaned)?;
    let candidate = caps.get(1)?.as_str();
    if subsection_words_re().is_match(candidate) {
        return None;
    }
    if placeholder_re().is_match(candidate) {
        return None;
    }
    let descriptor = cleaned[candidate.len()..].trim().to_string();
    Some(HookLedgerEntry {
        id: candidate.to_string(),
        descriptor: descriptor.clone(),
        keywords: extract_keywords(&descriptor),
    })
}

/// 从 descriptor 提取匹配关键词（引号名优先，否则 →/-> 前；CJK runs+2gram+3gram+ASCII 词）。
fn extract_keywords(descriptor: &str) -> Vec<String> {
    if descriptor.is_empty() {
        return Vec::new();
    }
    // 引号名优先（"..." 或 "..."）。
    let source: String = quoted_name_re()
        .captures(descriptor)
        .and_then(|c| c.get(1).map(|m| m.as_str().to_string()))
        .unwrap_or_else(|| {
            // 否则取 → / -> 前的文本。
            arrow_split_re()
                .split(descriptor)
                .next()
                .unwrap_or("")
                .to_string()
        });

    let mut tokens: Vec<String> = Vec::new();

    // CJK 连续段（≥2 字符）：整段 + 2-gram（≥3 时）+ 3gram（≥4 时取首/尾 3）。
    for m in cjk_run_re().find_iter(&source) {
        let run: Vec<char> = m.as_str().chars().collect();
        let run_str: String = run.iter().collect();
        tokens.push(run_str.clone());
        if run.len() >= 3 {
            for window in run.windows(2) {
                tokens.push(window.iter().collect());
            }
        }
        if run.len() >= 4 {
            let head3: String = run[..3].iter().collect();
            let tail3: String = run[run.len() - 3..].iter().collect();
            tokens.push(head3);
            tokens.push(tail3);
        }
    }

    // ASCII 词（≥3 字母），lower。
    for m in ascii_word_re().find_iter(&source) {
        tokens.push(m.as_str().to_lowercase());
    }

    // 去停用词 + 去重，保留首次出现顺序。
    let mut seen: HashSet<String> = HashSet::new();
    let mut out: Vec<String> = Vec::new();
    for t in tokens {
        if ascii_stopwords().contains(t.as_str()) {
            continue;
        }
        if seen.insert(t.clone()) {
            out.push(t);
        }
    }
    out
}

/// 正文是否回响了 entry（关键词子串匹配，或裸 id 的词边界匹配）。
fn draft_echoes_entry(draft: &str, entry: &HookLedgerEntry) -> bool {
    if !entry.keywords.is_empty() {
        let draft_lower = draft.to_lowercase();
        return entry.keywords.iter().any(|kw| {
            if kw.starts_with(|c: char| c.is_ascii_lowercase()) {
                draft_lower.contains(kw)
            } else {
                draft.contains(kw)
            }
        });
    }
    // 裸 id 回退：ASCII id 用词边界，否则子串。
    if bare_id_re().is_match(&entry.id) {
        let pattern = format!(r"\b{}\b", regex::escape(&entry.id));
        Regex::new(&pattern).unwrap().is_match(draft)
    } else {
        draft.contains(&entry.id)
    }
}

// --- 正则 + 常量（OnceLock 编译一次）------------------------------------------

fn zh_heading_re() -> &'static Regex {
    static R: OnceLock<Regex> = OnceLock::new();
    R.get_or_init(|| Regex::new(r"(?im)^#{2,3}\s*本章\s*hook\s*账\s*$").expect("zh heading"))
}
fn en_heading_re() -> &'static Regex {
    static R: OnceLock<Regex> = OnceLock::new();
    R.get_or_init(|| Regex::new(r"(?im)^#{2,3}\s*Hook\s+ledger\s+for\s+this\s+chapter\s*$").expect("en heading"))
}
fn next_heading_re() -> &'static Regex {
    static R: OnceLock<Regex> = OnceLock::new();
    R.get_or_init(|| Regex::new(r"\n#{2,3}\s").expect("next heading"))
}
fn subsection_heading_re() -> &'static Regex {
    // 命名捕获 key：open/advance/resolve/defer，后接可选 : / ：。
    static R: OnceLock<Regex> = OnceLock::new();
    R.get_or_init(|| Regex::new(r"(?i)^(?P<key>open|advance|resolve|defer)\s*[:：]?\s*$").expect("subsection heading"))
}
fn new_placeholder_re() -> &'static Regex {
    static R: OnceLock<Regex> = OnceLock::new();
    R.get_or_init(|| Regex::new(r"(?i)^\[new\]").expect("new placeholder"))
}
fn placeholder_re() -> &'static Regex {
    static R: OnceLock<Regex> = OnceLock::new();
    R.get_or_init(|| Regex::new(r"(?i)^(无|空|none|nil|null|暂无|n/a|na|n-a|tbd|todo|待定)$").expect("placeholder"))
}
fn subsection_words_re() -> &'static Regex {
    static R: OnceLock<Regex> = OnceLock::new();
    R.get_or_init(|| Regex::new(r"(?i)^(open|advance|resolve|defer|new)$").expect("subsection words"))
}
fn id_re() -> &'static Regex {
    // TS: /^([A-Za-z一-鿿][A-Za-z0-9_\-一-鿿]{0,19})/
    static R: OnceLock<Regex> = OnceLock::new();
    R.get_or_init(|| Regex::new(r"^([A-Za-z\x{4e00}-\x{9fff}][A-Za-z0-9_\-\x{4e00}-\x{9fff}]{0,19})").expect("id"))
}
fn quoted_name_re() -> &'static Regex {
    // TS: /[""]([^""\n]+)[""]/
    static R: OnceLock<Regex> = OnceLock::new();
    R.get_or_init(|| Regex::new(r#"[“"]([^”"\n]+)[”"]"#).expect("quoted name"))
}
fn arrow_split_re() -> &'static Regex {
    // TS: split on /[→]|->/
    static R: OnceLock<Regex> = OnceLock::new();
    R.get_or_init(|| Regex::new(r"→|->").expect("arrow split"))
}
fn cjk_run_re() -> &'static Regex {
    static R: OnceLock<Regex> = OnceLock::new();
    R.get_or_init(|| Regex::new(r"[\x{4e00}-\x{9fff}]{2,}").expect("cjk run"))
}
fn ascii_word_re() -> &'static Regex {
    static R: OnceLock<Regex> = OnceLock::new();
    R.get_or_init(|| Regex::new(r"[A-Za-z]{3,}").expect("ascii word"))
}
fn bare_id_re() -> &'static Regex {
    static R: OnceLock<Regex> = OnceLock::new();
    R.get_or_init(|| Regex::new(r"^[A-Za-z0-9_-]+$").expect("bare id"))
}

fn ascii_stopwords() -> &'static HashSet<&'static str> {
    static S: OnceLock<HashSet<&'static str>> = OnceLock::new();
    S.get_or_init(|| {
        let mut s = HashSet::new();
        for w in [
            "and", "the", "for", "with", "from", "that", "into", "then", "open", "close",
            "advance", "resolve", "defer", "new", "planted", "pressured", "near", "payoff",
            "ready", "stale",
        ] {
            s.insert(w);
        }
        s
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    const MEMO: &str = "\
## 本章 hook 账

advance:
- h007 \"胖虎借条\" → 揭穿借条伪造

resolve:
- mentor-debt 师徒债清偿

open:
- [new] 圣物觉醒之谜
";

    #[test]
    fn parse_extracts_advance_resolve_open_new() {
        let ledger = parse_hook_ledger(MEMO);
        assert_eq!(ledger.advance.len(), 1);
        assert_eq!(ledger.advance[0].id, "h007");
        assert_eq!(ledger.resolve.len(), 1);
        assert_eq!(ledger.resolve[0].id, "mentor-debt");
        assert_eq!(ledger.new_open_count, 1, "[new] 行计数");
        assert!(ledger.open.is_empty(), "id-bearing open 行为空（只有 [new]）");
    }

    #[test]
    fn parse_returns_empty_when_no_section() {
        let ledger = parse_hook_ledger("## 其它段落\n无 hook 账");
        assert_eq!(ledger, HookLedger::default());
    }

    #[test]
    fn parse_en_heading() {
        let memo = "## Hook ledger for this chapter\n\nresolve:\n- h001 关闭线索\n";
        let ledger = parse_hook_ledger(memo);
        assert_eq!(ledger.resolve.len(), 1);
    }

    #[test]
    fn parse_filters_placeholder_lines() {
        let memo = "## 本章 hook 账\n\nadvance:\n- 无\n- h002 实际推进\n";
        let ledger = parse_hook_ledger(memo);
        assert_eq!(ledger.advance.len(), 1, "「无」placeholder 应过滤");
        assert_eq!(ledger.advance[0].id, "h002");
    }

    #[test]
    fn extract_keywords_prefers_quoted_name() {
        let kws = extract_keywords("\"胖虎借条\" → 揭穿");
        assert!(kws.contains(&"胖虎借条".to_string()));
        assert!(kws.contains(&"胖虎".to_string()), "≥3 字符 CJK 应拆 2-gram");
    }

    #[test]
    fn extract_keywords_strips_after_arrow_when_no_quote() {
        let kws = extract_keywords("师徒债清偿 → 关闭");
        assert!(kws.contains(&"师徒债清偿".to_string()));
        assert!(!kws.iter().any(|k| k == "关闭"), "→ 之后文本不应产关键词");
    }

    #[test]
    fn validate_flags_missing_evidence_as_warning() {
        // advance h007 关键词「胖虎借条」不在正文中 → warning。
        let draft = "本章什么都没推进，只是闲聊。";
        let v = validate_hook_ledger(MEMO, draft);
        assert!(v.iter().any(|x| x.severity == ViolationSeverity::Warning && x.description.contains("h007")));
    }

    #[test]
    fn validate_passes_when_draft_echoes_keywords() {
        // 正文含「胖虎借条」+「师徒债清偿」→ evidence 通过。
        let draft = "主角拿出了胖虎借条，师徒债清偿那一刻众人震惊。";
        let v = validate_hook_ledger(MEMO, draft);
        // resolve=1, open(new)=1 → 揭1埋1 不违规；evidence 都有 → 无 warning。
        assert!(v.iter().all(|x| x.severity != ViolationSeverity::Warning), "warnings: {v:?}");
        assert!(v.is_empty(), "应无违规: {v:?}");
    }

    #[test]
    fn validate_flags_resolve_without_open_as_critical() {
        // resolve 1 个但无 [new]/open → 揭1埋1 critical。
        let memo = "## 本章 hook 账\n\nresolve:\n- h001 关闭线索\n";
        let draft = "正文含 关闭线索 满足 evidence";
        let v = validate_hook_ledger(memo, draft);
        assert!(v.iter().any(|x| x.severity == ViolationSeverity::Critical && x.category.contains("揭 1 埋 1")));
    }

    #[test]
    fn validate_no_violation_when_no_resolve() {
        // 无 resolve → 揭1埋1 不触发；advance 有 evidence。
        let memo = "## 本章 hook 账\n\nadvance:\n- h001 线索推进\n";
        let draft = "线索推进在本章发生";
        let v = validate_hook_ledger(memo, draft);
        assert!(v.is_empty(), "{v:?}");
    }

    #[test]
    fn draft_echoes_bare_ascii_id_with_word_boundary() {
        let entry = HookLedgerEntry { id: "H007".into(), descriptor: "".into(), keywords: vec![] };
        assert!(draft_echoes_entry("see H007 here", &entry));
        assert!(!draft_echoes_entry("see H0077 here", &entry), "词边界：H0077 不匹配 H007");
    }
}
