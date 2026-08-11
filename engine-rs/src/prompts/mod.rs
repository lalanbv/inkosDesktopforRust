//! 提示词域（builtin pack + prompt 数据）。
//!
//! 移植自 `packages/core/src/prompts/`：
//! - [`PromptPackManifest`] / [`PromptSource`]（types.ts）
//! - [`BuiltinPrompt`] + [`BUILTIN_PROMPT_PACKS`] / [`BUILTIN_PROMPTS`]（builtin-prompts.ts，3 pack + 12 prompt）
//! - [`get_builtin_prompt`] / [`list_builtin_prompts`] / [`list_builtin_prompt_packs`]（查询）
//!
//! ## 待移植（需文件 IO）
//! loadPromptPackPrompt / appendPromptPackGuidance（prompt-pack.ts，读项目/用户 prompt 覆盖）。
//! short-fiction.ts（568 行短篇模板数据）后续按需移植。

use serde::{Deserialize, Serialize};
#[cfg(feature = "export-bindings")]
use ts_rs::TS;

/// prompt 来源。对齐 TS `z.enum(["builtin","project","user","external"])`。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "export-bindings", derive(TS))]
#[cfg_attr(feature = "export-bindings", ts(export, type = "\"builtin\" | \"project\" | \"user\" | \"external\""))]
pub enum PromptSource {
    #[serde(rename = "builtin")] Builtin,
    #[serde(rename = "project")] Project,
    #[serde(rename = "user")] User,
    #[serde(rename = "external")] External,
}

/// prompt pack 清单。
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[cfg_attr(feature = "export-bindings", derive(TS))]
#[cfg_attr(feature = "export-bindings", ts(export))]
#[serde(rename_all = "camelCase")]
pub struct PromptPackManifest {
    pub id: String,
    pub title: String,
    pub description: String,
    #[serde(default)]
    pub prompts: Vec<String>,
    #[serde(default = "default_source")]
    pub source: PromptSource,
}

fn default_source() -> PromptSource {
    PromptSource::Builtin
}

/// 内建 prompt 条目。
#[derive(Debug, Clone, PartialEq)]
pub struct BuiltinPrompt {
    pub id: &'static str,
    pub pack_id: &'static str,
    pub title: &'static str,
    pub content: &'static str,
}

const LONGFORM_WRITER: &str = "You are InkOS's long-form chapter writer.\nWrite prose from the governed chapter intent and selected context package.\nProtected context is binding. Compressible context is supporting memory.\nDo not override author intent, current focus, hard facts, or active hook evidence with genre defaults.";
const LONGFORM_REVISER: &str = "You are InkOS's long-form reviser.\nFix the chapter according to audit issues while preserving established facts and the chapter goal.\nIf a repair requires changing higher-level state, surface that need instead of silently rewriting canon.";
const LONGFORM_AUDITOR: &str = "You are InkOS's continuity and quality auditor.\nCheck whether the chapter follows protected intent, hard facts, active hooks, proportions, and craft requirements.\nReport unresolved issues plainly; do not mark a failed chapter as fixed.";

const PLAY_START: &str = "You are InkOS Play's world-start guide.\nHelp confirm the playable premise, world contract, player persona, time semantics, and visual contract before starting.\nDo not force RPG levels or fixed stats unless the user asks for them.";
const PLAY_MUTATOR: &str = "You are InkOS Play's world mutation engine.\nTurn the player action into state changes: scene, entities, relationships, evidence, inventory, time, and consequences.\nRespect the world contract and preserve actor_player as the player entity id.";
const PLAY_RENDERER: &str = "You are InkOS Play's scene renderer.\nRender the applied world mutation as vivid interactive prose.\nDo not invent concrete objects, evidence, or characters that are absent from applied state unless the reconciler can record them.";
const PLAY_RECONCILER: &str = "You reconcile rendered scene prose back into the graph state.\nExtract newly mentioned concrete entities, evidence, relationships, and locations so state does not drift from narration.";
const PLAY_IMAGE: &str = "Create image prompts from the current play scene and visual contract.\nFollow user-defined visual semantics. Do not add watermarks, UI frames, text overlays, or default rarity borders unless requested.";

const IF_SCRIPT: &str = "You are an interactive-film script writer.\nConvert the confirmed premise/source into playable scenes, dialogue, choices, variables, and endings.\nLeave creative space to the user; ask or preserve format constraints instead of inventing production rules.";
const IF_STORYBOARD: &str = "You are an interactive-film storyboard designer.\nTurn script beats into shot-level visual plans with clear action, composition, and image prompts.\nDo not require video output; produce still-image/storyboard assets unless the user asks otherwise.";
const IF_STORY_GRAPH: &str = "You are an interactive-film story graph designer.\nCreate a playable graph: nodes, choices, variables/flags, and multiple endings.\nEvery branch must remain reachable and every path should resolve to an ending.";
const IF_IMAGE_PLAN: &str = "Create image plans for interactive-film nodes and assets.\nUse sceneKey/location continuity when available, but do not require full-screen game UI or video conversion.";

const PACK_IDS: &[&str] = &["longform", "play", "interactive-film"];

/// 3 个内建 pack（逐字移植 RAW_BUILTIN_PROMPT_PACKS）。函数返回，避免 const 分配 String。
pub fn builtin_prompt_packs() -> Vec<PromptPackManifest> {
    vec![
        PromptPackManifest {
            id: "longform".into(), title: "Longform Writing".into(),
            description: "Core long-form writing prompts used by chapter production and repair.".into(),
            prompts: vec!["longform.writer".into(), "longform.reviser".into(), "longform.auditor".into()],
            source: PromptSource::Builtin,
        },
        PromptPackManifest {
            id: "play".into(), title: "InkOS Play".into(),
            description: "Open-world / branching interaction prompts for world mutation, rendering, reconciliation, and images.".into(),
            prompts: vec!["play.start".into(), "play.mutator".into(), "play.renderer".into(), "play.reconciler".into(), "play.image".into()],
            source: PromptSource::Builtin,
        },
        PromptPackManifest {
            id: "interactive-film".into(), title: "Interactive Film Authoring".into(),
            description: "Script, storyboard, story graph, and image-planning prompts for interactive-film projects.".into(),
            prompts: vec!["interactive-film.script".into(), "interactive-film.storyboard".into(), "interactive-film.story-graph".into(), "interactive-film.image-plan".into()],
            source: PromptSource::Builtin,
        },
    ]
}

/// 12 个内建 prompt（逐字移植 RAW_BUILTIN_PROMPTS）。
pub const RAW_BUILTIN_PROMPTS: &[BuiltinPrompt] = &[
    BuiltinPrompt { id: "longform.writer", pack_id: "longform", title: "Longform Writer", content: LONGFORM_WRITER },
    BuiltinPrompt { id: "longform.reviser", pack_id: "longform", title: "Longform Reviser", content: LONGFORM_REVISER },
    BuiltinPrompt { id: "longform.auditor", pack_id: "longform", title: "Longform Auditor", content: LONGFORM_AUDITOR },
    BuiltinPrompt { id: "play.start", pack_id: "play", title: "Play Start", content: PLAY_START },
    BuiltinPrompt { id: "play.mutator", pack_id: "play", title: "Play World Mutator", content: PLAY_MUTATOR },
    BuiltinPrompt { id: "play.renderer", pack_id: "play", title: "Play Scene Renderer", content: PLAY_RENDERER },
    BuiltinPrompt { id: "play.reconciler", pack_id: "play", title: "Play Scene Reconciler", content: PLAY_RECONCILER },
    BuiltinPrompt { id: "play.image", pack_id: "play", title: "Play Image Prompt", content: PLAY_IMAGE },
    BuiltinPrompt { id: "interactive-film.script", pack_id: "interactive-film", title: "Interactive Film Script", content: IF_SCRIPT },
    BuiltinPrompt { id: "interactive-film.storyboard", pack_id: "interactive-film", title: "Interactive Film Storyboard", content: IF_STORYBOARD },
    BuiltinPrompt { id: "interactive-film.story-graph", pack_id: "interactive-film", title: "Interactive Film Story Graph", content: IF_STORY_GRAPH },
    BuiltinPrompt { id: "interactive-film.image-plan", pack_id: "interactive-film", title: "Interactive Film Image Plan", content: IF_IMAGE_PLAN },
];

/// BUILTIN_PROMPTS 的访问入口（与 TS 导出同名意图一致）。
pub fn builtin_prompts() -> &'static [BuiltinPrompt] {
    RAW_BUILTIN_PROMPTS
}

/// 按 id 查找内建 prompt。
pub fn get_builtin_prompt(id: &str) -> Option<&'static BuiltinPrompt> {
    RAW_BUILTIN_PROMPTS.iter().find(|p| p.id == id)
}

/// 列出所有内建 prompt 的 id。
pub fn list_builtin_prompts() -> Vec<&'static str> {
    RAW_BUILTIN_PROMPTS.iter().map(|p| p.id).collect()
}

/// 列出所有内建 pack 的 id。
pub fn list_builtin_prompt_packs() -> Vec<&'static str> {
    PACK_IDS.to_vec()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn twelve_prompts_three_packs() {
        assert_eq!(RAW_BUILTIN_PROMPTS.len(), 12);
        assert_eq!(builtin_prompt_packs().len(), 3);
    }

    #[test]
    fn get_by_id() {
        let p = get_builtin_prompt("longform.writer").unwrap();
        assert_eq!(p.pack_id, "longform");
        assert!(p.content.contains("long-form chapter writer"));
        assert!(get_builtin_prompt("nonexistent").is_none());
    }

    #[test]
    fn list_ids() {
        let prompts = list_builtin_prompts();
        assert!(prompts.contains(&"play.mutator"));
        assert_eq!(prompts.len(), 12);
        let packs = list_builtin_prompt_packs();
        assert_eq!(packs, vec!["longform", "play", "interactive-film"]);
    }

    #[test]
    fn pack_prompts_match_builtin_count() {
        // 每个 pack.prompts 的 id 都应能在 RAW_BUILTIN_PROMPTS 找到
        for pack in builtin_prompt_packs() {
            for id in &pack.prompts {
                assert!(get_builtin_prompt(id).is_some(), "pack {} 引用不存在的 prompt {}", pack.id, id);
            }
        }
    }

    #[test]
    fn manifest_serializes() {
        let json = serde_json::to_string(&builtin_prompt_packs()[0]).unwrap();
        assert!(json.contains(r#""id":"longform""#));
        assert!(json.contains(r#""source":"builtin""#));
    }
}
