//! 外部技能加载（external-loader）。
//!
//! 移植自 `packages/core/src/skills/external-loader.ts`（245 行）：
//! - [`parse_agent_skill_document`]：SKILL.md frontmatter（YAML）→ [`AgentSkill`]
//! - [`load_configured_agent_skills`]：env 显式目录 + user/project 四目录扫描
//!
//! ## 目录来源（configuredSkillDirs 逐字）
//! 1. `INKOS_SKILL_DIRS`（路径分隔符分割，external，显式）
//! 2. `{home}/.openclaw/skills`（user）
//! 3. `{home}/.agents/skills`（user）
//! 4. `{projectRoot}/.agents/skills`（project）
//! 5. `{projectRoot}/skills`（project）
//!
//! 显式目录缺失 → 计入 diagnostics；非显式目录缺失（ENOENT）→ 静默跳过。

use std::collections::HashSet;
use std::path::{Path, PathBuf};

use unicode_normalization::UnicodeNormalization;

use crate::skills::{AgentSkill, SkillSource};

pub const MAX_SKILL_MANIFEST_BYTES: u64 = 2 * 1024 * 1024;
pub const MAX_SKILL_NAME_CHARS: usize = 64;
pub const MAX_SKILL_DESCRIPTION_CHARS: usize = 1024;

/// 加载诊断（路径 + 错误消息）。对齐 TS `ExternalSkillDiagnostic`。
#[derive(Debug, Clone, PartialEq, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ExternalSkillDiagnostic {
    pub path: String,
    pub message: String,
}

/// `loadConfiguredAgentSkills` 结果。对齐 TS `LoadExternalAgentSkillsResult`。
#[derive(Debug, Clone, Default)]
pub struct LoadExternalAgentSkillsResult {
    pub skills: Vec<AgentSkill>,
    pub diagnostics: Vec<ExternalSkillDiagnostic>,
}

/// 单个候选目录的错误（TS 侧靠 `error.code === "ENOENT"` 区分缺失）。
#[derive(Debug)]
enum LoadCandidateError {
    /// 目录不存在（ENOENT）。
    Missing,
    /// 其他错误（消息对齐 TS Error.message）。
    Other(String),
}

impl LoadCandidateError {
    fn message(&self) -> String {
        match self {
            LoadCandidateError::Missing => {
                "no such file or directory".to_string()
            }
            LoadCandidateError::Other(message) => message.clone(),
        }
    }
}

fn io_candidate_error(error: &std::io::Error) -> LoadCandidateError {
    if error.kind() == std::io::ErrorKind::NotFound {
        LoadCandidateError::Missing
    } else {
        LoadCandidateError::Other(error.to_string())
    }
}

struct ConfiguredSkillDir {
    path: PathBuf,
    explicit: bool,
    source: SkillSource,
}

/// 解析 SKILL.md 文档。对齐 TS `parseAgentSkillDocument`（错误消息逐字）。
pub fn parse_agent_skill_document(
    raw: &str,
    skill_path: &Path,
    source: SkillSource,
) -> Result<AgentSkill, String> {
    let (data, body) = parse_frontmatter(raw)?;
    let data = data
        .ok_or_else(|| "SKILL.md frontmatter must be a YAML object.".to_string())?;
    if !data.is_mapping() {
        return Err("SKILL.md frontmatter must be a YAML object.".to_string());
    }

    let fallback_id = skill_path
        .parent()
        .and_then(|p| p.file_name())
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_default();
    let name = required_text(
        data.get("name"),
        "name",
        MAX_SKILL_NAME_CHARS,
    )?;
    let description = required_text(
        data.get("description"),
        "description",
        MAX_SKILL_DESCRIPTION_CHARS,
    )?;
    let id = normalize_external_skill_id(&name, &fallback_id)?;

    Ok(AgentSkill {
        id,
        name,
        description,
        body: body.trim().to_string(),
        source,
        base_dir: Some(
            skill_path
                .parent()
                .map(|p| p.to_path_buf())
                .unwrap_or_else(|| PathBuf::from("."))
                .to_string_lossy()
                .to_string(),
        ),
    })
}

/// frontmatter 切分 + YAML 解析。`Ok(None)` = YAML 值为 null（TS `!parsed.data` 分支）。
fn parse_frontmatter(raw: &str) -> Result<(Option<serde_yaml::Value>, String), String> {
    // TS：`raw.replace(/^\uFEFF/, "").replace(/\r\n?/g, "\n")`（单 BOM + CRLF/CR → LF）
    let no_bom = raw.strip_prefix('\u{FEFF}').unwrap_or(raw);
    let mut normalized = String::with_capacity(no_bom.len());
    let mut chars = no_bom.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '\r' {
            if chars.peek() == Some(&'\n') {
                chars.next();
            }
            normalized.push('\n');
        } else {
            normalized.push(c);
        }
    }
    if !normalized.starts_with("---\n") {
        return Err("SKILL.md must start with YAML frontmatter delimiters.".to_string());
    }
    // TS：`normalized.indexOf("\n---", 4)`（从索引 4 起的绝对位置）
    let Some(end) = normalized[4..].find("\n---").map(|i| i + 4) else {
        return Err("SKILL.md is missing closing YAML frontmatter delimiter.".to_string());
    };
    let frontmatter = normalized[4..end].trim().to_string();
    let after = &normalized[end + 4..];
    let body = after.strip_prefix('\n').unwrap_or(after).to_string();
    let data = serde_yaml::from_str::<serde_yaml::Value>(&frontmatter)
        .map_err(|e| e.to_string())?;
    let data = match data {
        serde_yaml::Value::Null => None,
        other => Some(other),
    };
    Ok((data, body))
}

/// `optionalText`：string 且 trim 非空 → trim，否则 None。
fn optional_text(value: Option<&serde_yaml::Value>) -> Option<String> {
    let text = value.and_then(serde_yaml::Value::as_str)?;
    let trimmed = text.trim();
    if trimmed.is_empty() {
        None
    } else {
        Some(trimmed.to_string())
    }
}

/// `requiredText`：必填文本 + UTF-16 码元长度上限（TS `text.length`）。
fn required_text(
    value: Option<&serde_yaml::Value>,
    field: &str,
    max_chars: usize,
) -> Result<String, String> {
    let Some(text) = optional_text(value) else {
        return Err(format!("SKILL.md frontmatter requires {field}."));
    };
    if text.encode_utf16().count() > max_chars {
        return Err(format!(
            "SKILL.md frontmatter {field} must be at most {max_chars} characters."
        ));
    }
    Ok(text)
}

/// `normalizeExternalSkillId`：NFKD + lower + 非 [a-z0-9] 段折叠为 `-`、
/// 去首尾 `-`、非字母开头补 `skill-` 前缀、≤64 UTF-16 码元。
fn normalize_external_skill_id(value: &str, fallback: &str) -> Result<String, String> {
    let normalize = |candidate: &str| -> String {
        let lowered: String = candidate.nfkd().flat_map(char::to_lowercase).collect();
        let mut out = String::with_capacity(lowered.len());
        let mut pending_dash = false;
        for c in lowered.chars() {
            if c.is_ascii_lowercase() || c.is_ascii_digit() {
                if pending_dash {
                    out.push('-');
                    pending_dash = false;
                }
                out.push(c);
            } else {
                pending_dash = true;
            }
        }
        if pending_dash {
            out.push('-');
        }
        out.trim_matches('-').to_string()
    };
    let primary = normalize(value);
    let id = if primary.is_empty() {
        normalize(fallback)
    } else {
        primary
    };
    if id.is_empty() {
        return Err("SKILL.md requires a name that can be used as a skill id.".to_string());
    }
    let normalized = if id.starts_with(|c: char| c.is_ascii_lowercase()) {
        id
    } else {
        format!("skill-{id}")
    };
    if normalized.encode_utf16().count() > MAX_SKILL_NAME_CHARS {
        return Err(format!(
            "SKILL.md skill id must be at most {MAX_SKILL_NAME_CHARS} characters."
        ));
    }
    Ok(normalized)
}

/// 候选目录列表（configuredSkillDirs 逐字；env 目录显式）。
fn configured_skill_dirs(project_root: &Path, env_dirs: &[String], home_dir: Option<&Path>) -> Vec<ConfiguredSkillDir> {
    let mut out: Vec<ConfiguredSkillDir> = env_dirs
        .iter()
        .map(|path| ConfiguredSkillDir {
            path: PathBuf::from(path),
            explicit: true,
            source: SkillSource::External,
        })
        .collect();
    if let Some(home) = home_dir {
        out.push(ConfiguredSkillDir {
            path: home.join(".openclaw").join("skills"),
            explicit: false,
            source: SkillSource::User,
        });
        out.push(ConfiguredSkillDir {
            path: home.join(".agents").join("skills"),
            explicit: false,
            source: SkillSource::User,
        });
    }
    out.push(ConfiguredSkillDir {
        path: project_root.join(".agents").join("skills"),
        explicit: false,
        source: SkillSource::Project,
    });
    out.push(ConfiguredSkillDir {
        path: project_root.join("skills"),
        explicit: false,
        source: SkillSource::Project,
    });
    out
}

/// 扫描配置目录集。对齐 TS `loadConfiguredAgentSkills`：
/// 显式目录缺失 → 诊断；非显式缺失（ENOENT）→ 跳过。
pub async fn load_configured_agent_skills(
    project_root: &Path,
    env_dirs: &[String],
    home_dir: Option<&Path>,
) -> LoadExternalAgentSkillsResult {
    let candidates = configured_skill_dirs(project_root, env_dirs, home_dir);
    let mut result = LoadExternalAgentSkillsResult::default();
    for candidate in candidates {
        match load_external_agent_skills(&candidate.path, candidate.source).await {
            Ok(mut loaded) => {
                result.skills.append(&mut loaded.skills);
                result.diagnostics.append(&mut loaded.diagnostics);
            }
            Err(LoadCandidateError::Missing) if !candidate.explicit => continue,
            Err(error) => result.diagnostics.push(ExternalSkillDiagnostic {
                path: candidate.path.to_string_lossy().to_string(),
                message: error.message(),
            }),
        }
    }
    result
}

/// 单目录加载。对齐 TS `loadExternalAgentSkills`（内部 per-skill 失败 → 诊断）。
async fn load_external_agent_skills(
    dir: &Path,
    source: SkillSource,
) -> Result<LoadExternalAgentSkillsResult, LoadCandidateError> {
    let skill_dirs = discover_skill_dirs(dir).await?;
    let mut result = LoadExternalAgentSkillsResult::default();
    for dir in skill_dirs {
        let skill_path = PathBuf::from(&dir).join("SKILL.md");
        match load_skill_manifest(&skill_path, source).await {
            Ok(skill) => result.skills.push(skill),
            Err(message) => result
                .diagnostics
                .push(ExternalSkillDiagnostic { path: skill_path.to_string_lossy().to_string(), message }),
        }
    }
    Ok(result)
}

/// 目录发现：自身含 SKILL.md 即技能目录，否则向下扫 2 层。对齐 TS `discoverSkillDirs`。
async fn discover_skill_dirs(dir: &Path) -> Result<Vec<String>, LoadCandidateError> {
    if !dir.is_absolute() {
        return Err(LoadCandidateError::Other(format!(
            "External skill directory must be absolute: {}",
            dir.display()
        )));
    }
    let info = tokio::fs::metadata(dir).await.map_err(|e| io_candidate_error(&e))?;
    if !info.is_dir() {
        return Err(LoadCandidateError::Other(format!(
            "External skill path is not a directory: {}",
            dir.display()
        )));
    }
    let mut dirs = Vec::new();
    if has_skill_manifest(dir).await {
        dirs.push(dir.to_string_lossy().to_string());
    } else {
        dirs.extend(discover_skill_dirs_below(dir, 2).await?);
    }
    dirs.sort();
    dirs.dedup();
    Ok(dirs)
}

async fn discover_skill_dirs_below(
    root: &Path,
    remaining_depth: u32,
) -> Result<Vec<String>, LoadCandidateError> {
    fn pinned<'a>(
        root: &'a Path,
        remaining_depth: u32,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<Vec<String>, LoadCandidateError>> + Send + 'a>> {
        Box::pin(discover_skill_dirs_below(root, remaining_depth))
    }
    if remaining_depth == 0 {
        return Ok(Vec::new());
    }
    let mut dirs = Vec::new();
    let mut entries = tokio::fs::read_dir(root)
        .await
        .map_err(|e| io_candidate_error(&e))?;
    while let Some(entry) = entries
        .next_entry()
        .await
        .map_err(|e| io_candidate_error(&e))?
    {
        if !entry
            .file_type()
            .await
            .map(|t| t.is_dir())
            .unwrap_or(false)
        {
            continue;
        }
        let child = root.join(entry.file_name());
        if has_skill_manifest(&child).await {
            dirs.push(child.to_string_lossy().to_string());
            continue;
        }
        dirs.extend(pinned(&child, remaining_depth - 1).await?);
    }
    Ok(dirs)
}

async fn has_skill_manifest(dir: &Path) -> bool {
    tokio::fs::metadata(dir.join("SKILL.md"))
        .await
        .map(|m| m.is_file())
        .unwrap_or(false)
}

/// 读取并解析 SKILL.md（2MB 上限）。对齐 TS `loadSkillManifest`。
async fn load_skill_manifest(skill_path: &Path, source: SkillSource) -> Result<AgentSkill, String> {
    let info = tokio::fs::metadata(skill_path)
        .await
        .map_err(|e| e.to_string())?;
    if info.len() > MAX_SKILL_MANIFEST_BYTES {
        return Err(format!("SKILL.md exceeds {MAX_SKILL_MANIFEST_BYTES} bytes."));
    }
    let raw = tokio::fs::read_to_string(skill_path)
        .await
        .map_err(|e| e.to_string())?;
    parse_agent_skill_document(&raw, skill_path, source)
}

/// 项目技能目录下的合规 id 集（GET /skills 的 editable 判定源）。
///
/// 对齐 TS `listProjectSkillIds`：目录名经 skill id 校验（不合规直接上抛 → 端点 500）、
/// SKILL.md 必须是普通文件。
pub async fn list_project_skill_ids(
    project_root: &Path,
) -> Result<HashSet<String>, crate::skills::SkillIdError> {
    let mut ids = HashSet::new();
    let skills_dir = project_root.join(".agents").join("skills");
    let mut entries = match tokio::fs::read_dir(&skills_dir).await {
        Ok(entries) => entries,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(ids),
        Err(e) => return Err(crate::skills::SkillIdError::Io(e.to_string())),
    };
    while let Some(entry) = entries
        .next_entry()
        .await
        .map_err(|e| crate::skills::SkillIdError::Io(e.to_string()))?
    {
        if !entry
            .file_type()
            .await
            .map(|t| t.is_dir())
            .unwrap_or(false)
        {
            continue;
        }
        let name = entry.file_name().to_string_lossy().to_string();
        let id = crate::skills::normalize_skill_id_strict(&name)?;
        let is_file = tokio::fs::metadata(skills_dir.join(&id).join("SKILL.md"))
            .await
            .map(|m| m.is_file())
            .unwrap_or(false);
        if is_file {
            ids.insert(id);
        }
    }
    Ok(ids)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(raw: &str, skill_path: &str) -> Result<AgentSkill, String> {
        parse_agent_skill_document(raw, Path::new(skill_path), SkillSource::Project)
    }

    #[test]
    fn parses_name_description_and_body() {
        let skill = parse(
            "---\nname: Combat Tactics\ndescription: 战斗策略\n---\n\n正文段落。\n",
            "/root/.agents/skills/combat/SKILL.md",
        )
        .unwrap();
        assert_eq!(skill.id, "combat-tactics");
        assert_eq!(skill.name, "Combat Tactics");
        assert_eq!(skill.description, "战斗策略");
        assert_eq!(skill.body, "正文段落。");
        assert_eq!(skill.source, SkillSource::Project);
        assert_eq!(skill.base_dir.as_deref(), Some("/root/.agents/skills/combat"));
    }

    #[test]
    fn id_falls_back_to_folder_name_when_name_unusable() {
        // name 全为非 ASCII → normalize 后空 → fallback（目录名）
        let skill = parse(
            "---\nname: 中文名\ndescription: d\n---\nbody",
            "/root/skills/Backup Folder/SKILL.md",
        )
        .unwrap();
        assert_eq!(skill.id, "backup-folder");
    }

    #[test]
    fn id_prefixed_when_not_letter_start() {
        let skill = parse(
            "---\nname: 42 Moves\ndescription: d\n---\nb",
            "/root/skills/x/SKILL.md",
        )
        .unwrap();
        assert_eq!(skill.id, "skill-42-moves");
    }

    #[test]
    fn id_nfkd_folds_diacritics() {
        // café → NFKD 分解（e + 组合符）→ lower → 组合符折叠为尾 - → trim → cafe
        let skill = parse(
            "---\nname: Café\ndescription: d\n---\nb",
            "/root/skills/x/SKILL.md",
        )
        .unwrap();
        assert_eq!(skill.id, "cafe");
    }

    #[test]
    fn missing_frontmatter_errors() {
        assert_eq!(
            parse("no frontmatter", "/x/y/SKILL.md").unwrap_err(),
            "SKILL.md must start with YAML frontmatter delimiters."
        );
        assert_eq!(
            parse("---\nname: x\ndescription: d\n", "/x/y/SKILL.md").unwrap_err(),
            "SKILL.md is missing closing YAML frontmatter delimiter."
        );
    }

    #[test]
    fn non_object_frontmatter_errors() {
        assert_eq!(
            parse("---\njust a scalar\n---\nb", "/x/y/SKILL.md").unwrap_err(),
            "SKILL.md frontmatter must be a YAML object."
        );
        // 空 frontmatter（空白 + null）同样是"非对象"；注意 "---\n---\n" 形态在 TS
        // 里 indexOf("\n---", 4) 找不到闭合符，走的是 missing delimiter 分支
        assert_eq!(
            parse("---\n \n---\nb", "/x/y/SKILL.md").unwrap_err(),
            "SKILL.md frontmatter must be a YAML object."
        );
        assert_eq!(
            parse("---\n---\nb", "/x/y/SKILL.md").unwrap_err(),
            "SKILL.md is missing closing YAML frontmatter delimiter."
        );
    }

    #[test]
    fn missing_required_fields_errors() {
        assert_eq!(
            parse("---\ndescription: d\n---\nb", "/x/y/SKILL.md").unwrap_err(),
            "SKILL.md frontmatter requires name."
        );
        assert_eq!(
            parse("---\nname: n\n---\nb", "/x/y/SKILL.md").unwrap_err(),
            "SKILL.md frontmatter requires description."
        );
        // 非字符串字段同样视为缺失（TS optionalText 只认 string）
        assert_eq!(
            parse("---\nname: 12\ndescription: d\n---\nb", "/x/y/SKILL.md").unwrap_err(),
            "SKILL.md frontmatter requires name."
        );
    }

    #[test]
    fn field_length_limits_use_utf16_units() {
        // 64 个 BMP 汉字 = 64 UTF-16 码元，通过；65 个 → 超限（UTF-8 字节数远超 64，
        // 用 UTF-16 计数保证中文 name 与 TS 行为一致）
        let name_64: String = "战".repeat(64);
        let raw = format!("---\nname: {name_64}\ndescription: d\n---\nb");
        // name 全非 ASCII → id 回退目录名
        assert!(parse(&raw, "/x/backup/SKILL.md").is_ok());
        let name_65: String = "战".repeat(65);
        let raw = format!("---\nname: {name_65}\ndescription: d\n---\nb");
        assert_eq!(
            parse(&raw, "/x/backup/SKILL.md").unwrap_err(),
            "SKILL.md frontmatter name must be at most 64 characters."
        );
    }

    #[tokio::test]
    async fn loads_project_skill_dirs_with_two_level_scan() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path();
        // 一级目录直接含 manifest
        std::fs::create_dir_all(root.join("skills").join("combat")).unwrap();
        std::fs::write(
            root.join("skills").join("combat").join("SKILL.md"),
            "---\nname: Combat\ndescription: d\n---\nb",
        )
        .unwrap();
        // 二级嵌套（skills 顶层无 SKILL.md → 扫 2 层）
        std::fs::create_dir_all(root.join(".agents").join("skills").join("pack").join("magic")).unwrap();
        std::fs::write(
            root.join(".agents").join("skills").join("pack").join("magic").join("SKILL.md"),
            "---\nname: Magic\ndescription: d\n---\nb",
        )
        .unwrap();

        let result = load_configured_agent_skills(root, &[], None).await;
        assert!(result.diagnostics.is_empty(), "{:?}", result.diagnostics);
        let ids: Vec<&str> = result.skills.iter().map(|s| s.id.as_str()).collect();
        assert!(ids.contains(&"combat"), "{ids:?}");
        assert!(ids.contains(&"magic"), "{ids:?}");
        // project 目录的 source 标记
        let combat = result.skills.iter().find(|s| s.id == "combat").unwrap();
        assert_eq!(combat.source, SkillSource::Project);
    }

    #[tokio::test]
    async fn missing_implicit_dirs_are_silent_explicit_dirs_diagnose() {
        let tmp = tempfile::tempdir().unwrap();
        let result = load_configured_agent_skills(
            tmp.path(),
            &["/nonexistent-absolute-path-xyz".to_string()],
            None,
        )
        .await;
        // 非显式（project .agents/skills、skills）缺失 → 静默；显式 env 目录缺失 → 诊断
        assert_eq!(result.diagnostics.len(), 1);
        assert!(result.diagnostics[0].path.contains("nonexistent-absolute-path-xyz"));
        assert!(result.skills.is_empty());
    }

    #[tokio::test]
    async fn malformed_manifest_goes_to_diagnostics() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path().join("skills").join("broken");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("SKILL.md"), "no frontmatter").unwrap();
        let result = load_configured_agent_skills(tmp.path(), &[], None).await;
        assert_eq!(result.diagnostics.len(), 1);
        assert_eq!(
            result.diagnostics[0].message,
            "SKILL.md must start with YAML frontmatter delimiters."
        );
    }

    #[tokio::test]
    async fn list_project_skill_ids_filters_incomplete_dirs() {
        let tmp = tempfile::tempdir().unwrap();
        let skills = tmp.path().join(".agents").join("skills");
        std::fs::create_dir_all(skills.join("good")).unwrap();
        std::fs::write(skills.join("good").join("SKILL.md"), "---\nname: G\ndescription: d\n---\nb").unwrap();
        std::fs::create_dir_all(skills.join("empty")).unwrap(); // 无 SKILL.md
        let ids = list_project_skill_ids(tmp.path()).await.unwrap();
        assert!(ids.contains("good"));
        assert!(!ids.contains("empty"));
    }
}
