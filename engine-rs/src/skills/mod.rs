//! 技能域（AgentSkill 类型 + 注册表）。
//!
//! 移植自 `packages/core/src/skills/`：
//! - [`AgentSkill`] / [`SkillSource`]（types.ts）
//! - [`SkillRegistry`] trait + [`create_skill_registry`]（registry.ts）
//! - [`external_loader`]：SKILL.md 解析 + env/user/project 目录扫描（external-loader.ts）
//! - [`normalize_skill_id_strict`]：SkillIdSchema 校验（action-envelope.ts）

pub mod external_loader;
pub mod production_bindings;

use serde::{Deserialize, Serialize};
#[cfg(feature = "export-bindings")]
use ts_rs::TS;
use std::collections::{HashMap, HashSet};

/// SkillIdSchema 校验错误（消息为 zod 默认文本）。
#[derive(Debug, Clone, PartialEq)]
pub enum SkillIdError {
    /// `.min(1)` 失败。
    Empty,
    /// `.regex(/^[a-z][a-z0-9-]*$/i)` 失败。
    Pattern,
    /// 文件系统错误（list_project_skill_ids 读取目录）。
    Io(String),
}

impl SkillIdError {
    pub fn message(&self) -> String {
        match self {
            SkillIdError::Empty => "String must contain at least 1 character(s)".to_string(),
            SkillIdError::Pattern => {
                "Skill id must use letters, numbers, and hyphens.".to_string()
            }
            SkillIdError::Io(message) => message.clone(),
        }
    }
}

/// SkillIdSchema 校验 + 小写化：`z.string().trim().min(1).regex(/^[a-z][a-z0-9-]*$/i)` → toLowerCase。
pub fn normalize_skill_id_strict(value: &str) -> Result<String, SkillIdError> {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        return Err(SkillIdError::Empty);
    }
    let bytes = trimmed.as_bytes();
    let valid = bytes[0].is_ascii_alphabetic()
        && bytes
            .iter()
            .all(|c| c.is_ascii_alphanumeric() || *c == b'-');
    if !valid {
        return Err(SkillIdError::Pattern);
    }
    Ok(trimmed.to_lowercase())
}

/// 技能来源。对齐 TS `z.enum(["builtin","project","user","external"])`
/// （"builtin" 为 130 号合并同步：packages/core/skills 专业技能包）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "export-bindings", derive(TS))]
#[cfg_attr(feature = "export-bindings", ts(export, type = "\"builtin\" | \"project\" | \"user\" | \"external\""))]
pub enum SkillSource {
    #[serde(rename = "builtin")] Builtin,
    #[serde(rename = "project")] Project,
    #[serde(rename = "user")] User,
    #[serde(rename = "external")] External,
}

/// 技能定义。
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[cfg_attr(feature = "export-bindings", derive(TS))]
#[cfg_attr(feature = "export-bindings", ts(export))]
#[serde(rename_all = "camelCase")]
pub struct AgentSkill {
    pub id: String,
    pub name: String,
    pub description: String,
    #[serde(default)]
    pub body: String,
    #[serde(default = "default_source")]
    pub source: SkillSource,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub base_dir: Option<String>,
}

fn default_source() -> SkillSource {
    SkillSource::External
}

/// 技能解析输入。
#[derive(Debug, Clone, Default)]
pub struct SkillResolutionInput {
    pub requested_skills: Vec<String>,
    pub disabled_skills: Vec<String>,
}

/// 技能解析结果。
#[derive(Debug, Clone)]
pub struct SkillResolutionResult {
    pub used_skills: Vec<AgentSkill>,
    pub forced_skill_ids: Vec<String>,
    pub missing_skill_ids: Vec<String>,
    pub disabled_skill_ids: Vec<String>,
    pub available_skills: Vec<AgentSkill>,
    pub available_skill_ids: Vec<String>,
}

/// 技能注册表（trait，对齐 TS interface）。
pub trait SkillRegistry: Send + Sync {
    fn list_skills(&self) -> Vec<AgentSkill>;
    fn get_skill(&self, id: &str) -> Option<AgentSkill>;
    fn resolve_skills(&self, input: &SkillResolutionInput) -> SkillResolutionResult;
}

/// 规范化技能 id：trim + 小写。
fn normalize_skill_id(value: &str) -> String {
    value.trim().to_lowercase()
}

fn dedupe_strings(values: &[String]) -> Vec<String> {
    let mut out = Vec::new();
    let mut seen = HashSet::new();
    for v in values {
        if v.is_empty() || !seen.insert(v.clone()) {
            continue;
        }
        out.push(v.clone());
    }
    out
}

/// 内建技能注册表实现。
pub struct BuiltinSkillRegistry {
    skills: Vec<AgentSkill>,
    by_id: HashMap<String, AgentSkill>,
}

impl BuiltinSkillRegistry {
    pub fn new(skills: Vec<AgentSkill>) -> Self {
        // 去重（按规范化 id，last-write-wins——TS Map.set 覆盖语义；130 号
        // 修正原 or_insert 的 first-wins：builtin+configured 合并后项目/用户
        // 需能同名覆盖内置默认）+ 规范化 id + 按 id 排序
        let mut by_id: HashMap<String, AgentSkill> = HashMap::new();
        for mut s in skills {
            let nid = normalize_skill_id(&s.id);
            s.id = nid.clone();
            by_id.insert(nid, s);
        }
        let mut skills: Vec<AgentSkill> = by_id.values().cloned().collect();
        skills.sort_by(|a, b| a.id.cmp(&b.id));
        BuiltinSkillRegistry { skills, by_id }
    }
}

impl SkillRegistry for BuiltinSkillRegistry {
    fn list_skills(&self) -> Vec<AgentSkill> {
        self.skills.clone()
    }
    fn get_skill(&self, id: &str) -> Option<AgentSkill> {
        self.by_id.get(&normalize_skill_id(id)).cloned()
    }
    fn resolve_skills(&self, input: &SkillResolutionInput) -> SkillResolutionResult {
        let disabled: HashSet<String> = input.disabled_skills.iter().map(|s| normalize_skill_id(s)).collect();
        let requested: Vec<String> = dedupe_strings(
            &input.requested_skills.iter().map(|s| normalize_skill_id(s)).filter(|s| !s.is_empty()).collect::<Vec<String>>(),
        );
        let mut missing = Vec::new();
        // 535 号：disabled_skill_ids 对齐 TS Set 插入序（registry.ts resolveSkills：
        // 规范化+去重后按输入序过滤在册）——此前 HashSet 迭代随机序。
        let disabled_skill_ids: Vec<String> = dedupe_strings(
            &input.disabled_skills.iter().map(|s| normalize_skill_id(s)).filter(|s| !s.is_empty()).collect::<Vec<String>>(),
        )
        .into_iter()
        .filter(|id| self.by_id.contains_key(id))
        .collect();
        // 535 号：usedSkills 对齐 TS Map 插入序（请求序即产出序）——此前
        // HashMap 迭代随机序，多技能时 system prompt 指导段顺序双端漂移
        // 且自身逐轮不稳定（requested 已去重，push 即 Map.set 语义）。
        let mut used: Vec<AgentSkill> = Vec::new();
        let mut forced = Vec::new();
        for id in &requested {
            match self.by_id.get(id) {
                None => missing.push(id.clone()),
                Some(skill) => {
                    if disabled.contains(id) {
                        continue;
                    }
                    used.push(skill.clone());
                    forced.push(id.clone());
                }
            }
        }
        let available_skills: Vec<AgentSkill> = self.skills.iter().filter(|s| !disabled.contains(&s.id)).cloned().collect();
        let available_skill_ids: Vec<String> = available_skills.iter().map(|s| s.id.clone()).collect();
        SkillResolutionResult {
            used_skills: used,
            forced_skill_ids: forced,
            missing_skill_ids: dedupe_strings(&missing),
            disabled_skill_ids,
            available_skills,
            available_skill_ids,
        }
    }
}

/// 创建技能注册表（对齐 TS createSkillRegistry）。
pub fn create_skill_registry(skills: Vec<AgentSkill>) -> BuiltinSkillRegistry {
    BuiltinSkillRegistry::new(skills)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn skill(id: &str) -> AgentSkill {
        AgentSkill { id: id.into(), name: id.into(), description: "d".into(), body: "".into(), source: SkillSource::External, base_dir: None }
    }

    #[test]
    fn dedupes_and_normalizes_ids() {
        let reg = create_skill_registry(vec![skill("Combat"), skill("combat"), skill("Magic")]);
        // "Combat" 与 "combat" 规范化后同 id，去重
        let list = reg.list_skills();
        assert_eq!(list.len(), 2);
        assert!(list.iter().any(|s| s.id == "combat"));
        assert!(list.iter().any(|s| s.id == "magic"));
    }

    #[test]
    fn get_skill_normalizes_query() {
        let reg = create_skill_registry(vec![skill("Combat")]);
        assert!(reg.get_skill("COMBAT").is_some());
        assert!(reg.get_skill(" combat ").is_some());
        assert!(reg.get_skill("nonexistent").is_none());
    }

    #[test]
    fn resolve_skills_handles_requested_missing_disabled() {
        let reg = create_skill_registry(vec![skill("combat"), skill("magic"), skill("stealth")]);
        let input = SkillResolutionInput {
            requested_skills: vec!["Combat".into(), "UnknownSkill".into()],
            disabled_skills: vec!["stealth".into()],
        };
        let r = reg.resolve_skills(&input);
        // requested: combat 命中（强制），UnknownSkill 缺失
        assert_eq!(r.forced_skill_ids, vec!["combat"]);
        assert_eq!(r.used_skills.len(), 1);
        assert_eq!(r.missing_skill_ids, vec!["unknownskill"]);
        // disabled: stealth 在册 → 列入 disabled；combat 不在 disabled
        assert!(r.disabled_skill_ids.contains(&"stealth".to_string()));
        // available = 全部非 disabled
        assert_eq!(r.available_skill_ids.len(), 2);
        assert!(!r.available_skill_ids.contains(&"stealth".to_string()));
    }

    #[test]
    fn resolve_requested_disabled_is_skipped() {
        let reg = create_skill_registry(vec![skill("combat")]);
        let input = SkillResolutionInput {
            requested_skills: vec!["combat".into()],
            disabled_skills: vec!["combat".into()],
        };
        let r = reg.resolve_skills(&input);
        assert!(r.used_skills.is_empty());
        assert!(r.forced_skill_ids.is_empty());
    }

    /// 535 号：usedSkills/disabledSkillIds 序确定性——请求序/输入序（TS
    /// Map/Set 插入序镜像，registry.ts resolveSkills）；此前 HashMap/HashSet
    /// 迭代随机序，多技能时指导段顺序双端漂移且自身不稳定。
    #[test]
    fn resolve_skills_preserves_request_and_disabled_order() {
        let reg = create_skill_registry(vec![skill("a"), skill("b"), skill("c"), skill("d")]);
        let input = SkillResolutionInput {
            requested_skills: vec!["c".into(), "a".into(), "b".into()],
            disabled_skills: vec!["d".into(), "d".into(), "".into()],
        };
        let r = reg.resolve_skills(&input);
        assert_eq!(
            r.used_skills.iter().map(|s| s.id.as_str()).collect::<Vec<_>>(),
            vec!["c", "a", "b"]
        );
        assert_eq!(r.forced_skill_ids, vec!["c", "a", "b"]);
        // disabled：输入序 + 去重 + 滤空 + 仅在册。
        assert_eq!(r.disabled_skill_ids, vec!["d"]);
    }

    #[test]
    fn agent_skill_serializes() {
        let s = skill("x");
        let json = serde_json::to_string(&s).unwrap();
        assert!(json.contains(r#""source":"external""#));
    }
}
