//! work 创作配置注册表（884 号 M1b）：`packages/core/src/harness/profile-registry.ts`
//! 与 `builtin-profiles.ts` 移植。内置 9 个 profile 静态数据 + `.inkos/profiles/*.json`
//! 自定义归并（replace 语义：同名覆盖内置/异名新增；注册顺序不影响 list 排序）。
//!
//! 契约面：`GET /api/v1/profiles` 返回 zod parse 后的全字段对象（default 已应用，
//! 如 description=""、requiredSkillIds=[] 等），serde 序列化字段声明序与 TS 一致。

use std::path::Path;

use serde_json::Map;

use super::contracts::{
    ConfirmationPolicy, ConfirmationValue, ContextRecipe, ProfileProduction, WorkProfile,
    HARNESS_VERSION,
};

pub const PROFILES_DIRECTORY: &str = ".inkos/profiles";

#[derive(Debug, Clone, thiserror::Error)]
pub enum ProfileRegistryError {
    #[error("Unknown work profile: {0}")]
    Unknown(String),
    #[error("Work profile already registered: {0}")]
    AlreadyRegistered(String),
    #[error("{0}")]
    Invalid(String),
    #[error("{0}")]
    Io(String),
}

const SHORT_FICTION_SCHEMAS: &[(&str, &str)] = &[
    ("source/final/short-story.json", "short-manuscript"),
    ("source/final/sales-package.json", "short-package"),
];
const TRANSLATION_SCHEMAS: &[(&str, &str)] = &[
    ("source/manifest.json", "translation-manifest"),
    ("source/glossary.json", "translation-glossary"),
];

fn schema_map(entries: &[(&str, &str)]) -> Map<String, serde_json::Value> {
    entries
        .iter()
        .map(|(path, schema)| ((*path).to_string(), (*schema).to_string().into()))
        .collect()
}

fn confirmation_default() -> ConfirmationPolicy {
    ConfirmationPolicy {
        inferred_mutation: ConfirmationValue::Execute,
        explicit_recoverable_mutation: ConfirmationValue::Execute,
        destructive_mutation: ConfirmationValue::Confirm,
    }
}

fn context_recipe(profile_id: &str) -> ContextRecipe {
    ContextRecipe {
        id: profile_id.to_string(),
        // 与 TS 一致：task/skills/work 三源配方（builtin-profiles.ts L80）。
        source_ids: vec!["task".into(), "skills".into(), "work".into()],
    }
}

struct ProfileInput {
    id: &'static str,
    title: &'static str,
    description: &'static str,
    capability_ids: &'static [&'static str],
    required_skill_ids: &'static [&'static str],
    recommended_skill_ids: &'static [&'static str],
    artifact_kinds: &'static [&'static str],
    schemas: &'static [(&'static str, &'static str)],
}

/// TS `PROFILE_INPUTS`（builtin-profiles.ts L6-71）逐行对照；description/技能空缺
/// 项 = TS default 值（"" / []）。
const PROFILE_INPUTS: &[ProfileInput] = &[
    ProfileInput {
        id: "workspace-default",
        title: "Creative workspace",
        description: "Discuss goals, inspect works, and create derived works.",
        capability_ids: &["workspace", "adaptation", "translation", "visual"],
        required_skill_ids: &[],
        recommended_skill_ids: &[],
        artifact_kinds: &["brief", "source", "export"],
        schemas: &[],
    },
    ProfileInput {
        id: "longform-novel",
        title: "Long-form novel",
        description: "",
        capability_ids: &["workspace", "longform", "adaptation", "visual"],
        required_skill_ids: &["inkos-long-writing"],
        recommended_skill_ids: &["inkos-story-review"],
        artifact_kinds: &["foundation", "chapter-plan", "chapter", "review", "cover"],
        schemas: &[],
    },
    ProfileInput {
        id: "short-fiction",
        title: "Short fiction",
        description: "",
        capability_ids: &["workspace", "short-fiction", "adaptation", "visual"],
        required_skill_ids: &["inkos-short-writing"],
        recommended_skill_ids: &[],
        artifact_kinds: &["outline", "manuscript", "sales-package", "cover"],
        schemas: SHORT_FICTION_SCHEMAS,
    },
    ProfileInput {
        id: "script",
        title: "Script",
        description: "",
        capability_ids: &["workspace", "script", "adaptation", "visual"],
        required_skill_ids: &["inkos-script-writing"],
        recommended_skill_ids: &[],
        artifact_kinds: &["script-spec", "script"],
        schemas: &[],
    },
    ProfileInput {
        id: "storyboard",
        title: "Storyboard",
        description: "",
        capability_ids: &["workspace", "storyboard", "adaptation", "visual"],
        required_skill_ids: &["inkos-storyboard"],
        recommended_skill_ids: &[],
        artifact_kinds: &["storyboard-spec", "storyboard", "image-prompt", "image"],
        schemas: &[],
    },
    ProfileInput {
        id: "interactive-film",
        title: "Interactive film",
        description: "",
        capability_ids: &["workspace", "interactive-film", "adaptation", "visual"],
        required_skill_ids: &["inkos-interactive-film"],
        recommended_skill_ids: &[],
        artifact_kinds: &["story-graph", "flags", "script", "storyboard", "image"],
        schemas: &[],
    },
    ProfileInput {
        id: "interactive-world",
        title: "Interactive world",
        description: "",
        capability_ids: &["workspace", "interactive-world", "visual"],
        required_skill_ids: &["inkos-play-world"],
        recommended_skill_ids: &[],
        artifact_kinds: &["world-contract", "world-state", "scene", "event", "image"],
        schemas: &[],
    },
    ProfileInput {
        id: "translation",
        title: "Translation",
        description: "",
        capability_ids: &["workspace", "translation"],
        required_skill_ids: &["inkos-translation"],
        recommended_skill_ids: &[],
        artifact_kinds: &["source", "glossary", "translation", "review", "export"],
        schemas: TRANSLATION_SCHEMAS,
    },
    ProfileInput {
        id: "visual-asset",
        title: "Visual asset",
        description: "",
        capability_ids: &["workspace", "visual"],
        required_skill_ids: &["inkos-story-cover"],
        recommended_skill_ids: &[],
        artifact_kinds: &["image-prompt", "image"],
        schemas: &[],
    },
];

impl From<&ProfileInput> for WorkProfile {
    fn from(input: &ProfileInput) -> Self {
        WorkProfile {
            version: HARNESS_VERSION,
            id: input.id.to_string(),
            title: input.title.to_string(),
            description: input.description.to_string(),
            capability_ids: input.capability_ids.iter().map(|s| (*s).to_string()).collect(),
            required_skill_ids: input.required_skill_ids.iter().map(|s| (*s).to_string()).collect(),
            recommended_skill_ids: input
                .recommended_skill_ids
                .iter()
                .map(|s| (*s).to_string())
                .collect(),
            artifact_kinds: input.artifact_kinds.iter().map(|s| (*s).to_string()).collect(),
            confirmation: confirmation_default(),
            context_recipe: Some(context_recipe(input.id)),
            artifact_schemas: schema_map(input.schemas),
            quality_criteria: Vec::new(),
            production: ProfileProduction::default(),
        }
    }
}

/// TS `builtInWorkProfiles()`。
pub fn built_in_work_profiles() -> Vec<WorkProfile> {
    PROFILE_INPUTS.iter().map(WorkProfile::from).collect()
}

/// TS `WorkProfileRegistry`：register（重复即错）/replace（覆盖）/get/require/list。
/// list 按 id 升序（TS localeCompare——内置与自定义 id 均为 ASCII kebab-case）。
#[derive(Default)]
pub struct WorkProfileRegistry {
    profiles: Vec<WorkProfile>,
}

impl WorkProfileRegistry {
    pub fn register(&mut self, profile: WorkProfile) -> Result<(), ProfileRegistryError> {
        profile.validate().map_err(ProfileRegistryError::Invalid)?;
        if self.profiles.iter().any(|item| item.id == profile.id) {
            return Err(ProfileRegistryError::AlreadyRegistered(profile.id));
        }
        self.profiles.push(profile);
        Ok(())
    }

    pub fn replace(&mut self, profile: WorkProfile) -> Result<(), ProfileRegistryError> {
        profile.validate().map_err(ProfileRegistryError::Invalid)?;
        match self.profiles.iter_mut().find(|item| item.id == profile.id) {
            Some(slot) => *slot = profile,
            None => self.profiles.push(profile),
        }
        Ok(())
    }

    pub fn get(&self, id: &str) -> Option<&WorkProfile> {
        self.profiles.iter().find(|item| item.id == id)
    }

    pub fn require(&self, id: &str) -> Result<&WorkProfile, ProfileRegistryError> {
        self.get(id).ok_or_else(|| ProfileRegistryError::Unknown(id.to_string()))
    }

    pub fn list(&self) -> Vec<&WorkProfile> {
        let mut profiles: Vec<&WorkProfile> = self.profiles.iter().collect();
        profiles.sort_by(|left, right| left.id.cmp(&right.id));
        profiles
    }
}

/// TS `createBuiltInWorkProfileRegistry(projectRoot)`：内置 9 个 + `.inkos/profiles/*.json`
/// 归并。目录缺失 = 仅内置；文件按名排序逐个 replace；符号链接/非常规文件 = 错
///（TS "Profile must be a regular file"）。
pub fn create_built_in_work_profile_registry(
    project_root: &Path,
) -> Result<WorkProfileRegistry, ProfileRegistryError> {
    let mut registry = WorkProfileRegistry::default();
    for profile in built_in_work_profiles() {
        registry.register(profile)?;
    }
    let directory = project_root.join(PROFILES_DIRECTORY);
    let mut files: Vec<std::path::PathBuf> = match std::fs::read_dir(&directory) {
        Ok(entries) => entries
            .filter_map(|entry| entry.ok())
            .map(|entry| entry.path())
            .collect(),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Vec::new(),
        Err(error) => return Err(ProfileRegistryError::Io(error.to_string())),
    };
    files.sort();
    for path in files {
        let meta = std::fs::symlink_metadata(&path)
            .map_err(|error| ProfileRegistryError::Io(error.to_string()))?;
        if !meta.is_file() || meta.file_type().is_symlink() {
            return Err(ProfileRegistryError::Invalid(
                "Profile must be a regular file".into(),
            ));
        }
        let raw = std::fs::read_to_string(&path)
            .map_err(|error| ProfileRegistryError::Io(error.to_string()))?;
        let profile: WorkProfile = serde_json::from_str(&raw)
            .map_err(|error| ProfileRegistryError::Invalid(error.to_string()))?;
        // TS WorkProfileSchema.parse：version literal(2) 由 zod 字面量强制；
        // Rust contracts.validate 未含字面量检查，此处补齐（自定义 profile 加载面）。
        if profile.version != HARNESS_VERSION {
            return Err(ProfileRegistryError::Invalid(format!(
                "Profile version must be {HARNESS_VERSION}"
            )));
        }
        registry.replace(profile)?;
    }
    Ok(registry)
}
