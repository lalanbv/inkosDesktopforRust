//! 书籍规则（book_rules.md 契约）。
//!
//! 移植自 `packages/core/src/models/book-rules.ts`（313 行）：
//! [`parse_book_rules`]（frontmatter YAML 优先 + shim 检测 + markdown 回退）、
//! [`try_parse_book_rules_frontmatter`]（严格变体，失败返回 Err 供调用方回退）。

use regex::Regex;
use serde::{Deserialize, Serialize};
use std::sync::OnceLock;
#[cfg(feature = "export-bindings")]
use ts_rs::TS;

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
#[cfg_attr(feature = "export-bindings", derive(TS))]
#[cfg_attr(feature = "export-bindings", ts(export))]
#[serde(rename_all = "camelCase", default)]
pub struct Protagonist {
    pub name: String,
    #[serde(default)]
    pub personality_lock: Vec<String>,
    #[serde(default)]
    pub behavioral_constraints: Vec<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
#[cfg_attr(feature = "export-bindings", derive(TS))]
#[cfg_attr(feature = "export-bindings", ts(export))]
#[serde(rename_all = "camelCase", default)]
pub struct GenreLock {
    pub primary: String,
    #[serde(default)]
    pub forbidden: Vec<String>,
}

/// 数值上限：number | string（对齐 TS `z.union([z.number(), z.string()])`）。
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(untagged)]
#[cfg_attr(feature = "export-bindings", derive(TS))]
#[cfg_attr(feature = "export-bindings", ts(export, untagged))]
pub enum HardCap {
    Number(f64),
    Text(String),
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
#[cfg_attr(feature = "export-bindings", derive(TS))]
#[cfg_attr(feature = "export-bindings", ts(export))]
#[serde(rename_all = "camelCase", default)]
pub struct NumericalOverrides {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub hard_cap: Option<HardCap>,
    #[serde(default)]
    pub resource_types: Vec<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
#[cfg_attr(feature = "export-bindings", derive(TS))]
#[cfg_attr(feature = "export-bindings", ts(export))]
#[serde(rename_all = "camelCase", default)]
pub struct EraConstraints {
    #[serde(default)]
    pub enabled: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub period: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub region: Option<String>,
}

/// 叙事人称。对齐 TS `z.enum(["first","third"])`。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "export-bindings", derive(TS))]
#[cfg_attr(
    feature = "export-bindings",
    ts(export, type = "\"first\" | \"third\"")
)]
pub enum NarrativePerson {
    #[serde(rename = "first")]
    First,
    #[serde(rename = "third")]
    Third,
}

/// 审计维度值：number | string（对齐 TS `z.union([z.number(), z.string()])`）。
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(untagged)]
#[cfg_attr(feature = "export-bindings", derive(TS))]
#[cfg_attr(feature = "export-bindings", ts(export, untagged))]
pub enum AuditDimension {
    Number(f64),
    Text(String),
}

/// 书籍规则（book_rules.md 的结构化规则面）。
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
#[cfg_attr(feature = "export-bindings", derive(TS))]
#[cfg_attr(feature = "export-bindings", ts(export))]
#[serde(rename_all = "camelCase", default)]
pub struct BookRules {
    #[serde(default = "default_version")]
    pub version: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub protagonist: Option<Protagonist>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub genre_lock: Option<GenreLock>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub narrative_person: Option<NarrativePerson>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub numerical_system_overrides: Option<NumericalOverrides>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub era_constraints: Option<EraConstraints>,
    #[serde(default)]
    pub prohibitions: Vec<String>,
    #[serde(default)]
    pub chapter_types_override: Vec<String>,
    #[serde(default)]
    pub fatigue_words_override: Vec<String>,
    #[serde(default)]
    pub additional_audit_dimensions: Vec<AuditDimension>,
    #[serde(default)]
    pub enable_full_cast_tracking: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub fanfic_mode: Option<super::book::FanficMode>,
    #[serde(default)]
    pub allowed_deviations: Vec<String>,
}

fn default_version() -> String {
    "1.0".into()
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[cfg_attr(feature = "export-bindings", derive(TS))]
#[cfg_attr(feature = "export-bindings", ts(export))]
pub struct ParsedBookRules {
    pub rules: BookRules,
    pub body: String,
}

// 4 个 shim 标记正则（逐字移植 TS isBookRulesShim）
fn shim_re() -> &'static Regex {
    static R: OnceLock<Regex> = OnceLock::new();
    R.get_or_init(|| {
        Regex::new(
            r"本书规则（兼容指针——已废弃）|Book Rules \(compat pointer — deprecated\)|本文件仅为外部读取保留|This file is kept for external readers only",
        ).unwrap()
    })
}

/// 检测 Phase 5 兼容指针 shim（无真实规则，调用方应回退 story_frame）。
pub fn is_book_rules_shim(raw: &str) -> bool {
    shim_re().is_match(raw)
}

// --- frontmatter 解析（serde_yaml_ng，对齐 TS js-yaml + zod）--------------------

/// frontmatter 解析失败原因。区分「没有 frontmatter」与「有但坏」（供调用方决定回退路径）。
#[derive(Debug, thiserror::Error, PartialEq)]
pub enum BookRulesFrontmatterError {
    #[error("no YAML frontmatter found")]
    NoFrontmatter,
    #[error("frontmatter parse failed: {0}")]
    Invalid(String),
}

/// zod 语义中间层：必填字段缺失即报错（如 `Protagonist.name`），可选字段显式 default。
/// 已知与 TS 的一处偏差：serde `Option` 把 YAML 显式 `null` 当缺失，zod 会 throw；
/// 受控 frontmatter 无此写法，接受该差异。
#[derive(Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase", default)]
struct BookRulesRaw {
    #[serde(default = "default_version")]
    version: String,
    protagonist: Option<ProtagonistRaw>,
    genre_lock: Option<GenreLockRaw>,
    /// z.enum(["first","third"]).optional().catch(undefined)：任意非法值静默降级为 None。
    /// 用 Value 承载以复刻 catch（类型错也不报错）。
    narrative_person: Option<serde_yaml_ng::Value>,
    numerical_system_overrides: Option<NumericalOverridesRaw>,
    era_constraints: Option<EraConstraintsRaw>,
    prohibitions: Vec<String>,
    chapter_types_override: Vec<String>,
    fatigue_words_override: Vec<String>,
    additional_audit_dimensions: Vec<AuditDimension>,
    enable_full_cast_tracking: bool,
    /// z.enum(["canon","au","ooc","cp"]).optional()：非法值 throw（无 catch）。
    fanfic_mode: Option<serde_yaml_ng::Value>,
    allowed_deviations: Vec<String>,
}

/// zod 语义：`name` 必填（容器级无 default，缺失即报错），列表字段 default。
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ProtagonistRaw {
    name: String,
    #[serde(default)]
    personality_lock: Vec<String>,
    #[serde(default)]
    behavioral_constraints: Vec<String>,
}

/// zod 语义：`primary` 必填（容器级无 default）。
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct GenreLockRaw {
    primary: String,
    #[serde(default)]
    forbidden: Vec<String>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase", default)]
struct NumericalOverridesRaw {
    hard_cap: Option<HardCap>,
    resource_types: Vec<String>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase", default)]
struct EraConstraintsRaw {
    enabled: bool,
    period: Option<String>,
    region: Option<String>,
}

impl TryFrom<BookRulesRaw> for BookRules {
    type Error = String;

    fn try_from(raw: BookRulesRaw) -> Result<Self, Self::Error> {
        // fanficMode：缺失 → None；字符串但非法 → Err（对齐 zod enum throw）；非字符串 → Err。
        let fanfic_mode = match raw.fanfic_mode {
            None => None,
            Some(v) => match v.as_str() {
                Some("canon") => Some(super::book::FanficMode::Canon),
                Some("au") => Some(super::book::FanficMode::Au),
                Some("ooc") => Some(super::book::FanficMode::Ooc),
                Some("cp") => Some(super::book::FanficMode::Cp),
                other => {
                    return Err(format!("fanficMode 非法: {other:?}"));
                }
            },
        };
        // narrativePerson：仅接受 "first"/"third"，其余（含类型错）→ None（对齐 catch(undefined)）。
        let narrative_person = match raw.narrative_person.as_ref().and_then(|v| v.as_str()) {
            Some("first") => Some(NarrativePerson::First),
            Some("third") => Some(NarrativePerson::Third),
            _ => None,
        };
        Ok(BookRules {
            version: raw.version,
            protagonist: raw.protagonist.map(|p| Protagonist {
                name: p.name,
                personality_lock: p.personality_lock,
                behavioral_constraints: p.behavioral_constraints,
            }),
            genre_lock: raw.genre_lock.map(|g| GenreLock {
                primary: g.primary,
                forbidden: g.forbidden,
            }),
            narrative_person,
            numerical_system_overrides: raw.numerical_system_overrides.map(|n| {
                NumericalOverrides {
                    hard_cap: n.hard_cap,
                    resource_types: n.resource_types,
                }
            }),
            era_constraints: raw.era_constraints.map(|e| EraConstraints {
                enabled: e.enabled,
                period: e.period,
                region: e.region,
            }),
            prohibitions: raw.prohibitions,
            chapter_types_override: raw.chapter_types_override,
            fatigue_words_override: raw.fatigue_words_override,
            additional_audit_dimensions: raw.additional_audit_dimensions,
            enable_full_cast_tracking: raw.enable_full_cast_tracking,
            fanfic_mode,
            allowed_deviations: raw.allowed_deviations,
        })
    }
}

/// 代码栅栏剥离。逐字移植 TS `^```(?:md|markdown|yaml)?\s*\n` + `\n```\s*$`。
fn strip_code_fence(raw: &str) -> String {
    let leading = leading_fence_re().replace(raw, "");
    let stripped = trailing_fence_re().replace(&leading, "");
    stripped.into_owned()
}

fn leading_fence_re() -> &'static Regex {
    static R: OnceLock<Regex> = OnceLock::new();
    R.get_or_init(|| Regex::new(r"^```(?:md|markdown|yaml)?\s*\n").unwrap())
}

fn trailing_fence_re() -> &'static Regex {
    static R: OnceLock<Regex> = OnceLock::new();
    R.get_or_init(|| Regex::new(r"\n```\s*$").unwrap())
}

/// frontmatter 提取。逐字移植 TS `---\s*\n([\s\S]*?)\n---\s*\n?([\s\S]*)$`（非锚定，
/// 懒惰匹配 → 文中第一处 `--- ... ---` 块）。
fn book_rules_frontmatter_re() -> &'static Regex {
    static R: OnceLock<Regex> = OnceLock::new();
    R.get_or_init(|| Regex::new(r"---\s*\n([\s\S]*?)\n---\s*\n?([\s\S]*)$").unwrap())
}

/// 严格解析 frontmatter：无 frontmatter → [`BookRulesFrontmatterError::NoFrontmatter`]；
/// YAML / 字段校验失败 → `Invalid`。对齐 TS `tryParseBookRulesFrontmatter` 的两类 null。
pub fn try_parse_book_rules_frontmatter(
    raw: &str,
) -> Result<ParsedBookRules, BookRulesFrontmatterError> {
    let stripped = strip_code_fence(raw);
    let Some(caps) = book_rules_frontmatter_re().captures(&stripped) else {
        return Err(BookRulesFrontmatterError::NoFrontmatter);
    };
    let fm = caps.get(1).expect("组 1 必在").as_str();
    let body = caps.get(2).expect("组 2 必在").as_str();
    // 对齐 TS：yaml.load("") → undefined / yaml.load("null") → null → zod parse throw
    // （golden frontmatter-empty 向量验证）。serde_yaml_ng 会把空输入吞成全默认结构，
    // 须显式拦下交给调用方回退。
    let value: serde_yaml_ng::Value =
        serde_yaml_ng::from_str(fm).map_err(|e| BookRulesFrontmatterError::Invalid(e.to_string()))?;
    if value.is_null() {
        return Err(BookRulesFrontmatterError::Invalid(
            "frontmatter 为空（yaml.load → undefined/null）".to_string(),
        ));
    }
    let raw_rules: BookRulesRaw = serde_yaml_ng::from_value(value)
        .map_err(|e| BookRulesFrontmatterError::Invalid(e.to_string()))?;
    let rules = BookRules::try_from(raw_rules).map_err(BookRulesFrontmatterError::Invalid)?;
    Ok(ParsedBookRules {
        rules,
        body: body.trim().to_string(),
    })
}

/// 解析 book_rules.md。对齐 TS `parseBookRules`：
/// 1. 剥代码栅栏 → 找 frontmatter → YAML+zod 解析（失败则**静默**落入下一步）
/// 2. shim 检测（Phase 5 兼容指针）→ None（调用方应回退 story_frame）
/// 3. markdown 回退：从普通 markdown 提取规则面，body = 全文
pub fn parse_book_rules(raw: &str) -> Option<ParsedBookRules> {
    let stripped = strip_code_fence(raw);
    if let Ok(parsed) = try_parse_book_rules_frontmatter(&stripped) {
        return Some(parsed);
    }
    if is_book_rules_shim(&stripped) {
        return None;
    }
    let rules = parse_markdown_book_rules(&stripped);
    Some(ParsedBookRules {
        rules,
        body: stripped.trim().to_string(),
    })
}

// --- markdown 回退解析器（逐字移植 TS parseMarkdownBookRules 及其助手）----------

/// 从普通 markdown 提取规则面。对齐 TS `parseMarkdownBookRules`（条件构造，对齐 zod default 语义）。
fn parse_markdown_book_rules(raw: &str) -> BookRules {
    let protagonist_section = extract_markdown_section(raw, &["主角", "Protagonist"]);
    let protagonist_name = read_labeled_value(
        &protagonist_section,
        &["名字", "姓名", "name", "protagonist"],
    )
    .or_else(|| read_labeled_value(raw, &["主角", "protagonist"]));
    let personality_lock = read_labeled_list(
        &protagonist_section,
        &[
            "性格锁",
            "性格关键词",
            "personalityLock",
            "personality lock",
            "core tags",
        ],
    );
    let behavioral_constraints = read_labeled_list(
        &protagonist_section,
        &[
            "行为约束",
            "behavioralConstraints",
            "behavioral constraints",
        ],
    );

    let genre_section = extract_markdown_section(raw, &["题材锁", "Genre Lock", "Genre"]);
    let primary = read_labeled_value(&genre_section, &["主类型", "题材", "primary", "genre"]);
    let mut forbidden = read_labeled_list(&genre_section, &["禁止混入", "禁混", "forbidden"]);
    forbidden.extend(read_markdown_list(&extract_markdown_section(
        raw,
        &["禁止混入", "Forbidden Style Intrusions", "Forbidden"],
    )));

    let prohibitions = read_markdown_list(&extract_markdown_section(
        raw,
        &["禁止事项", "禁忌", "本书禁忌", "Prohibitions", "Do Not"],
    ));
    let fanfic_section = extract_markdown_section(raw, &["同人模式", "Fanfic Mode", "Fanfic"]);
    let fanfic_mode = normalize_fanfic_mode(
        read_labeled_value(
            &fanfic_section,
            &["模式", "同人模式", "fanficMode", "fanfic mode", "mode"],
        )
        .as_deref(),
    );
    let allowed_deviations = read_labeled_list(
        &fanfic_section,
        &[
            "允许偏离",
            "允许的偏离",
            "allowedDeviations",
            "allowed deviations",
        ],
    );

    let numerical_section = extract_markdown_section(
        raw,
        &[
            "数值/资源规则",
            "数值规则",
            "资源规则",
            "Numerical / Resource Rules",
            "Numerical Rules",
            "Resource Rules",
        ],
    );
    let resource_types = read_labeled_list(
        &numerical_section,
        &[
            "核心资源",
            "资源类型",
            "resourceTypes",
            "core resources",
            "resources",
        ],
    );
    let hard_cap = read_labeled_value(&numerical_section, &["硬上限", "hardCap", "hard cap"]);

    let era_section = extract_markdown_section(raw, &["年代限制", "时代限制", "Era Constraints"]);
    let period = read_labeled_value(&era_section, &["时期", "年代", "period", "era"]);
    let region = read_labeled_value(&era_section, &["地域", "地区", "region"]);

    BookRules {
        protagonist: protagonist_name.map(|name| Protagonist {
            name,
            personality_lock,
            behavioral_constraints,
        }),
        genre_lock: (primary.is_some() || !forbidden.is_empty()).then(|| GenreLock {
            primary: primary.unwrap_or_default(),
            forbidden,
        }),
        narrative_person: detect_narrative_person(raw),
        numerical_system_overrides: (hard_cap.is_some() || !resource_types.is_empty()).then(|| {
            NumericalOverrides {
                hard_cap: hard_cap.map(HardCap::Text),
                resource_types,
            }
        }),
        era_constraints: (!era_section.is_empty()).then_some(EraConstraints {
            enabled: true,
            period,
            region,
        }),
        prohibitions,
        fanfic_mode,
        allowed_deviations,
        // zod default：version "1.0"（BookRules::default() 的 derive default 是空串，不适用）。
        version: default_version(),
        ..BookRules::default()
    }
}

/// 提取 markdown 小节。逐字移植 TS `extractMarkdownSection`（行级状态机）。
fn extract_markdown_section(raw: &str, headings: &[&str]) -> String {
    let wanted: Vec<String> = headings.iter().map(|h| normalize_heading(h)).collect();
    let mut collecting = false;
    let mut out: Vec<&str> = Vec::new();

    for line in split_lines(raw) {
        if let Some(heading) = heading_text(line) {
            if collecting {
                break;
            }
            collecting = wanted.contains(&normalize_heading(&heading));
            continue;
        }
        if collecting {
            out.push(line);
        }
    }

    out.join("\n").trim().to_string()
}

/// 标题行文本。逐字移植 TS `^\s{0,3}#{1,6}\s+(.+?)\s*#*\s*$`。
fn heading_text(line: &str) -> Option<String> {
    heading_line_re()
        .captures(line)
        .map(|c| c.get(1).expect("组 1 必在").as_str().to_string())
}

fn heading_line_re() -> &'static Regex {
    static R: OnceLock<Regex> = OnceLock::new();
    R.get_or_init(|| Regex::new(r"^\s{0,3}#{1,6}\s+(.+?)\s*#*\s*$").unwrap())
}

/// 读标签值。逐字移植 TS `readLabeledValue`（多行 + 大小写不敏感 + 首个匹配）。
fn read_labeled_value(raw: &str, labels: &[&str]) -> Option<String> {
    if raw.trim().is_empty() {
        return None;
    }
    let pattern = labels
        .iter()
        .map(|l| regex::escape(l))
        .collect::<Vec<_>>()
        .join("|");
    let re = Regex::new(&format!(
        r"(?im)^\s*(?:[-*]\s*)?(?:{pattern})\s*[:：]\s*(.+?)\s*$"
    ))
    .expect("标签模式应合法");
    let value = re
        .captures(raw)
        .map(|c| c.get(1).expect("组 1 必在").as_str());
    let cleaned = clean_scalar(value.unwrap_or_default());
    (!cleaned.is_empty()).then_some(cleaned)
}

/// 读标签列表。逐字移植 TS `readLabeledList`。
fn read_labeled_list(raw: &str, labels: &[&str]) -> Vec<String> {
    read_labeled_value(raw, labels)
        .map(|v| split_list(&v))
        .unwrap_or_default()
}

/// 读 markdown 列表。逐字移植 TS `readMarkdownList`（`- `/`* ` 开头行）。
fn read_markdown_list(raw: &str) -> Vec<String> {
    if raw.trim().is_empty() {
        return Vec::new();
    }
    let bullet_re = bullet_line_re();
    split_lines(raw)
        .into_iter()
        .map(|l| l.trim().to_string())
        .filter(|l| bullet_re.is_match(l))
        .map(|l| clean_scalar(bullet_re.replace(&l, "").as_ref()))
        .filter(|v| !v.is_empty())
        .collect()
}

fn bullet_line_re() -> &'static Regex {
    static R: OnceLock<Regex> = OnceLock::new();
    R.get_or_init(|| Regex::new(r"^[-*]\s+").unwrap())
}

/// 拆列表。逐字移植 TS `splitList`（剥一层包裹括号 → `[、,，;；|]` 分隔 → cleanScalar）。
fn split_list(value: &str) -> Vec<String> {
    let cleaned = clean_scalar(value);
    let stripped = list_wrap_open_re().replace(&cleaned, "");
    let stripped = list_wrap_close_re().replace(&stripped, "");
    list_split_re()
        .split(stripped.as_ref())
        .map(clean_scalar)
        .filter(|item| !item.is_empty())
        .collect()
}

fn list_wrap_open_re() -> &'static Regex {
    static R: OnceLock<Regex> = OnceLock::new();
    R.get_or_init(|| Regex::new(r"^[\[(（【]\s*").unwrap())
}

fn list_wrap_close_re() -> &'static Regex {
    static R: OnceLock<Regex> = OnceLock::new();
    R.get_or_init(|| Regex::new(r"\s*[\])）】]$").unwrap())
}

fn list_split_re() -> &'static Regex {
    static R: OnceLock<Regex> = OnceLock::new();
    R.get_or_init(|| Regex::new(r"[、,，;；|]").unwrap())
}

/// 叙事人称探测。逐字移植 TS `detectNarrativePerson`。
/// `(?-u:\b)` 用 ASCII 词边界对齐 JS `\b`（JS 的 \w 仅 ASCII）。
fn detect_narrative_person(raw: &str) -> Option<NarrativePerson> {
    if narrative_first_re().is_match(raw) {
        return Some(NarrativePerson::First);
    }
    if narrative_third_re().is_match(raw) {
        return Some(NarrativePerson::Third);
    }
    None
}

fn narrative_first_re() -> &'static Regex {
    static R: OnceLock<Regex> = OnceLock::new();
    R.get_or_init(|| Regex::new(r"(?i)第一人称|first[-\s]?person|(?-u:\b)first(?-u:\b)").unwrap())
}

fn narrative_third_re() -> &'static Regex {
    static R: OnceLock<Regex> = OnceLock::new();
    R.get_or_init(|| Regex::new(r"(?i)第三人称|third[-\s]?person|(?-u:\b)third(?-u:\b)").unwrap())
}

/// 同人模式归一化。逐字移植 TS `normalizeFanficMode`。
fn normalize_fanfic_mode(value: Option<&str>) -> Option<super::book::FanficMode> {
    let value = value?;
    let normalized = value.trim().to_lowercase();
    if normalized == "canon" || fanfic_canon_re().is_match(value) {
        return Some(super::book::FanficMode::Canon);
    }
    if normalized == "au" || fanfic_au_re().is_match(value) {
        return Some(super::book::FanficMode::Au);
    }
    if normalized == "ooc" || fanfic_ooc_re().is_match(value) {
        return Some(super::book::FanficMode::Ooc);
    }
    if normalized == "cp" || fanfic_cp_re().is_match(value) {
        return Some(super::book::FanficMode::Cp);
    }
    None
}

fn fanfic_canon_re() -> &'static Regex {
    static R: OnceLock<Regex> = OnceLock::new();
    R.get_or_init(|| Regex::new(r"正典|原作空白|原作视角").unwrap())
}

fn fanfic_au_re() -> &'static Regex {
    static R: OnceLock<Regex> = OnceLock::new();
    R.get_or_init(|| Regex::new(r"(?i)平行|分歧|if线").unwrap())
}

fn fanfic_ooc_re() -> &'static Regex {
    static R: OnceLock<Regex> = OnceLock::new();
    R.get_or_init(|| Regex::new(r"(?i)性格偏离").unwrap())
}

fn fanfic_cp_re() -> &'static Regex {
    static R: OnceLock<Regex> = OnceLock::new();
    R.get_or_init(|| Regex::new(r"(?i)配对|感情线").unwrap())
}

/// 标量清洗。逐字移植 TS `cleanScalar`（剥包裹引号 + 无值模式归空）。
fn clean_scalar(value: &str) -> String {
    let stripped = quotes_re().replace_all(value.trim(), "");
    let trimmed = stripped.trim();
    if none_value_re().is_match(trimmed) {
        String::new()
    } else {
        trimmed.to_string()
    }
}

fn quotes_re() -> &'static Regex {
    static R: OnceLock<Regex> = OnceLock::new();
    R.get_or_init(|| Regex::new(r#"^["'`“”‘’]+|["'`“”‘’]+$"#).unwrap())
}

fn none_value_re() -> &'static Regex {
    static R: OnceLock<Regex> = OnceLock::new();
    R.get_or_init(|| Regex::new(r"(?i)^(?:无|none|n/a|na|\(none\)|（无）|-|—)$").unwrap())
}

/// 标题归一化。逐字移植 TS `normalizeHeading`。
fn normalize_heading(value: &str) -> String {
    trailing_colon_re().replace(value, "").trim().to_lowercase()
}

fn trailing_colon_re() -> &'static Regex {
    static R: OnceLock<Regex> = OnceLock::new();
    R.get_or_init(|| Regex::new(r"[：:]\s*$").unwrap())
}

/// 按 `\r?\n` 拆行（对齐 TS `raw.split(/\r?\n/)`）。
fn split_lines(raw: &str) -> Vec<&str> {
    raw.split('\n')
        .map(|l| l.strip_suffix('\r').unwrap_or(l))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shim_detection() {
        assert!(is_book_rules_shim("本书规则（兼容指针——已废弃）\n其余内容"));
        assert!(is_book_rules_shim(
            "Book Rules (compat pointer — deprecated)"
        ));
        assert!(is_book_rules_shim("本文件仅为外部读取保留"));
        assert!(is_book_rules_shim(
            "This file is kept for external readers only"
        ));
        assert!(!is_book_rules_shim("# 真实规则\n主角: 林动"));
    }

    #[test]
    fn book_rules_default_deserialize() {
        let r: BookRules = serde_json::from_str("{}").unwrap();
        assert_eq!(r.version, "1.0");
        assert!(r.prohibitions.is_empty());
        assert!(!r.enable_full_cast_tracking);
    }

    #[test]
    fn hard_cap_and_audit_dim_untagged() {
        let hc: HardCap = serde_json::from_str("100").unwrap();
        assert_eq!(hc, HardCap::Number(100.0));
        let hc2: HardCap = serde_json::from_str(r#""unlimited""#).unwrap();
        assert_eq!(hc2, HardCap::Text("unlimited".into()));
    }

    // --- parse_book_rules / try_parse_book_rules_frontmatter ------------------

    const FM_RULES: &str = "---\nversion: \"2.0\"\nprotagonist:\n  name: 林动\n  personalityLock: [冷静, 果决]\n  behavioralConstraints: [不滥杀]\ngenreLock:\n  primary: 仙侠\n  forbidden: [科幻]\nprohibitions: [无逻辑巧合]\nfatigueWordsOverride: [震惊]\nadditionalAuditDimensions: [5, \"战力崩坏\"]\neraConstraints:\n  enabled: true\n  period: 宋代\nnumericalSystemOverrides:\n  hardCap: 100\nfanficMode: canon\n---\n\n正文规则说明\n";

    #[test]
    fn parse_frontmatter_full() {
        let parsed = parse_book_rules(FM_RULES).expect("应解析成功");
        let r = &parsed.rules;
        assert_eq!(r.version, "2.0");
        let p = r.protagonist.as_ref().expect("protagonist 应在");
        assert_eq!(p.name, "林动");
        assert_eq!(
            p.personality_lock,
            vec!["冷静".to_string(), "果决".to_string()]
        );
        assert_eq!(p.behavioral_constraints, vec!["不滥杀".to_string()]);
        let g = r.genre_lock.as_ref().expect("genreLock 应在");
        assert_eq!(g.primary, "仙侠");
        assert_eq!(g.forbidden, vec!["科幻".to_string()]);
        assert_eq!(r.prohibitions, vec!["无逻辑巧合".to_string()]);
        assert_eq!(r.fatigue_words_override, vec!["震惊".to_string()]);
        assert_eq!(
            r.additional_audit_dimensions,
            vec![
                AuditDimension::Number(5.0),
                AuditDimension::Text("战力崩坏".into())
            ]
        );
        let era = r.era_constraints.as_ref().expect("eraConstraints 应在");
        assert!(era.enabled);
        assert_eq!(era.period.as_deref(), Some("宋代"));
        let num = r
            .numerical_system_overrides
            .as_ref()
            .expect("numericalSystemOverrides 应在");
        assert_eq!(num.hard_cap, Some(HardCap::Number(100.0)));
        assert_eq!(r.fanfic_mode, Some(super::super::book::FanficMode::Canon));
        assert_eq!(parsed.body, "正文规则说明");
    }

    #[test]
    fn parse_frontmatter_defaults() {
        // 有实质 YAML 的最小 frontmatter → 全默认规则 + body。
        let parsed = parse_book_rules("---\nprohibitions: []\n---\nbody").expect("应解析成功");
        assert_eq!(parsed.rules.version, "1.0");
        assert!(parsed.rules.protagonist.is_none());
        assert!(parsed.rules.prohibitions.is_empty());
        assert!(!parsed.rules.enable_full_cast_tracking);
        assert_eq!(parsed.body, "body");
    }

    #[test]
    fn empty_frontmatter_falls_back_to_markdown() {
        // 对齐 TS：yaml.load("") → undefined → zod throw → markdown 回退，body 保留全文。
        // （golden frontmatter-empty 向量守门。）
        let parsed = parse_book_rules("---\n\n---\nbody").expect("应回退 markdown 解析");
        assert!(parsed.rules.protagonist.is_none());
        assert_eq!(parsed.body, "---\n\n---\nbody");
    }

    #[test]
    fn degenerate_dashes_do_not_form_frontmatter() {
        // `---\n---\nbody` 中闭合 --- 前无空行 → TS 正则不匹配 → markdown 回退，body 保留全文。
        let parsed = parse_book_rules("---\n---\nbody").expect("应回退 markdown 解析");
        assert_eq!(parsed.body, "---\n---\nbody");
    }

    #[test]
    fn parse_fenced_output() {
        // LLM 常把输出裹 ```md ... ``` → 剥栅栏后再解析（对齐 TS）。
        let fenced = "```md\n---\nprohibitions: [a]\n---\n正文\n```";
        let parsed = parse_book_rules(fenced).expect("栅栏包裹应解析成功");
        assert_eq!(parsed.rules.prohibitions, vec!["a".to_string()]);
        assert_eq!(parsed.body, "正文");
    }

    #[test]
    fn narrative_person_catch_degrades_to_none() {
        // zod .catch(undefined)：非法值静默降级，不 fail 整个解析。
        let raw = "---\nnarrativePerson: sideways\n---\nbody";
        let parsed = parse_book_rules(raw).expect("narrativePerson catch 应降级");
        assert_eq!(parsed.rules.narrative_person, None);
    }

    #[test]
    fn invalid_fanfic_mode_fails_frontmatter_but_falls_back() {
        // zod enum 无 catch：非法 fanficMode → frontmatter 失败 → 静默落 markdown 回退。
        let raw = "---\nfanficMode: bogus\n---\n\n## 主角\n\n名字: 林动\n";
        let parsed = parse_book_rules(raw).expect("应回退 markdown 解析");
        assert_eq!(parsed.rules.fanfic_mode, None);
        // markdown 回退提得 protagonist，body 为全文。
        assert_eq!(parsed.rules.protagonist.as_ref().unwrap().name, "林动");
        assert!(parsed.body.contains("## 主角"));
    }

    #[test]
    fn try_parse_frontmatter_distinguishes_errors() {
        assert_eq!(
            try_parse_book_rules_frontmatter("# 无 frontmatter"),
            Err(BookRulesFrontmatterError::NoFrontmatter)
        );
        assert!(matches!(
            try_parse_book_rules_frontmatter("---\nfanficMode: bogus\n---\nbody"),
            Err(BookRulesFrontmatterError::Invalid(_))
        ));
        assert!(try_parse_book_rules_frontmatter("---\nprohibitions: [x]\n---\nbody").is_ok());
    }

    #[test]
    fn shim_returns_none() {
        let shim = "# 本书规则（兼容指针——已废弃）\n本文件仅为外部读取保留";
        assert_eq!(parse_book_rules(shim), None);
    }

    // --- markdown 回退解析器 ----------------------------------------------------

    const MARKDOWN_RULES: &str = "# 主角\n\n名字：林动\n性格锁：冷静、果决\n行为约束：不滥杀；不弃队友\n\n## 题材锁\n\n主类型：仙侠\n禁止混入：科幻、悬疑\n\n## 禁止事项\n\n- 无逻辑的巧合推进剧情\n- 配角降智配合主角\n\n## 同人模式\n\n模式：原作向（正典）\n允许偏离：口头禅\n\n## 数值/资源规则\n\n核心资源：灵石、贡献点\n硬上限：100\n\n## 年代限制\n\n时期：宋代\n地域：江南\n\n全文第一人称叙述。\n";

    #[test]
    fn parse_markdown_full() {
        let parsed = parse_book_rules(MARKDOWN_RULES).expect("markdown 应解析成功");
        let r = &parsed.rules;
        let p = r.protagonist.as_ref().expect("protagonist 应在");
        assert_eq!(p.name, "林动");
        assert_eq!(
            p.personality_lock,
            vec!["冷静".to_string(), "果决".to_string()]
        );
        assert_eq!(
            p.behavioral_constraints,
            vec!["不滥杀".to_string(), "不弃队友".to_string()]
        );
        let g = r.genre_lock.as_ref().expect("genreLock 应在");
        assert_eq!(g.primary, "仙侠");
        assert_eq!(g.forbidden, vec!["科幻".to_string(), "悬疑".to_string()]);
        assert_eq!(
            r.prohibitions,
            vec![
                "无逻辑的巧合推进剧情".to_string(),
                "配角降智配合主角".to_string()
            ]
        );
        assert_eq!(r.fanfic_mode, Some(super::super::book::FanficMode::Canon));
        assert_eq!(r.allowed_deviations, vec!["口头禅".to_string()]);
        let num = r.numerical_system_overrides.as_ref().expect("数值规则应在");
        assert_eq!(num.hard_cap, Some(HardCap::Text("100".into())));
        assert_eq!(
            num.resource_types,
            vec!["灵石".to_string(), "贡献点".to_string()]
        );
        let era = r.era_constraints.as_ref().expect("年代限制应在");
        assert!(era.enabled);
        assert_eq!(era.period.as_deref(), Some("宋代"));
        assert_eq!(era.region.as_deref(), Some("江南"));
        assert_eq!(r.narrative_person, Some(NarrativePerson::First));
        // body = 全文（strip 后 trim）。
        assert!(parsed.body.starts_with("# 主角"));
    }

    #[test]
    fn parse_markdown_empty_yields_defaults() {
        let parsed = parse_book_rules("只是普通正文，无任何规则段。").expect("应有默认规则");
        assert!(parsed.rules.protagonist.is_none());
        assert!(parsed.rules.genre_lock.is_none());
        assert!(parsed.rules.era_constraints.is_none());
        assert_eq!(parsed.rules.narrative_person, None);
    }

    #[test]
    fn clean_scalar_strips_quotes_and_none_values() {
        assert_eq!(clean_scalar(" \"林动\" "), "林动");
        assert_eq!(clean_scalar("“林动”"), "林动");
        assert_eq!(clean_scalar("无"), "");
        assert_eq!(clean_scalar("N/A"), "");
        assert_eq!(clean_scalar("（无）"), "");
        assert_eq!(clean_scalar("—"), "");
        assert_eq!(clean_scalar("正常值"), "正常值");
    }

    #[test]
    fn split_list_handles_wrappers_and_separators() {
        // TS splitList 不去重：全部分隔符（、,，;；|）逐一切分。
        assert_eq!(
            split_list("[冷静、果决，坚毅；沉稳|专注]"),
            vec![
                "冷静".to_string(),
                "果决".to_string(),
                "坚毅".to_string(),
                "沉稳".to_string(),
                "专注".to_string()
            ]
        );
        assert_eq!(
            split_list("（灵石，贡献点）"),
            vec!["灵石".to_string(), "贡献点".to_string()]
        );
        assert!(split_list("无").is_empty());
    }

    #[test]
    fn normalize_fanfic_mode_matches_chinese_and_codes() {
        use super::super::book::FanficMode;
        assert_eq!(
            normalize_fanfic_mode(Some("canon")),
            Some(FanficMode::Canon)
        );
        assert_eq!(
            normalize_fanfic_mode(Some("原作空白")),
            Some(FanficMode::Canon)
        );
        assert_eq!(normalize_fanfic_mode(Some("AU")), Some(FanficMode::Au));
        assert_eq!(
            normalize_fanfic_mode(Some("平行世界")),
            Some(FanficMode::Au)
        );
        assert_eq!(normalize_fanfic_mode(Some("OOC")), Some(FanficMode::Ooc));
        assert_eq!(
            normalize_fanfic_mode(Some("性格偏离")),
            Some(FanficMode::Ooc)
        );
        assert_eq!(normalize_fanfic_mode(Some("cp")), Some(FanficMode::Cp));
        assert_eq!(normalize_fanfic_mode(Some("感情线")), Some(FanficMode::Cp));
        assert_eq!(normalize_fanfic_mode(Some("未知")), None);
        assert_eq!(normalize_fanfic_mode(None), None);
    }

    #[test]
    fn detect_narrative_person_third() {
        assert_eq!(
            detect_narrative_person("第三人称限知视角"),
            Some(NarrativePerson::Third)
        );
        assert_eq!(
            detect_narrative_person("First person POV"),
            Some(NarrativePerson::First)
        );
        assert_eq!(detect_narrative_person("没有称谓信息"), None);
    }

    #[test]
    fn extract_markdown_section_stops_at_next_heading() {
        let md = "## 主角\n\n名字：林动\n\n## 其他\n\n别的";
        assert_eq!(extract_markdown_section(md, &["主角"]), "名字：林动");
        assert_eq!(extract_markdown_section(md, &["不存在"]), "");
        // 标题带冒号也归一化匹配（对齐 normalizeHeading）。
        assert_eq!(extract_markdown_section("## 主角：\n行", &["主角"]), "行");
    }
}
