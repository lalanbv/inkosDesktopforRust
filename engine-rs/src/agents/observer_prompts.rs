//! Observer agent 提示词（observer-prompts）。
//!
//! 移植自 `packages/core/src/agents/observer-prompts.ts`（127 行）。纯函数：
//! writer 编排 Phase 2a（Observer 事实提取）的 system / user prompt 构造。

use crate::models::book::BookConfig;
use crate::models::genre_profile::GenreProfile;
use crate::utils::language::WritingLanguage;

/// 构造 Observer system prompt。逐字移植 TS `buildObserverSystemPrompt`。
///
/// **parity 注意**：TS 签名含 `book` 参数但函数体未使用——Rust 保留为 `_book`
/// 以对齐调用面（writer 编排会按签名传入）。
///
/// 语言判定对齐 TS `language ?? genreProfile.language`（显式优先，缺失回退 genre）。
pub fn build_observer_system_prompt(
    _book: &BookConfig,
    genre_profile: &GenreProfile,
    language: Option<WritingLanguage>,
) -> String {
    let is_english = match language {
        Some(lang) => lang == WritingLanguage::En,
        None => genre_profile.language == "en",
    };

    let lang_prefix = if is_english {
        "【LANGUAGE OVERRIDE】ALL output MUST be in English.\n\n"
    } else {
        ""
    };

    if is_english {
        format!("{lang_prefix}You are a fact extraction specialist。Read the chapter text and extract EVERY observable fact change.

## Extraction Categories

1. **Character actions**: Who did what, to whom, why
2. **Location changes**: Who moved where, from where
3. **Resource changes**: Items gained, lost, consumed, quantities
4. **Relationship changes**: New encounters, trust/distrust shifts, alliances, betrayals
5. **Emotional shifts**: Character mood before → after, trigger event
6. **Information flow**: Who learned what, who is still unaware
7. **Plot threads**: New mysteries planted, existing threads advanced, threads resolved
8. **Time progression**: How much time passed, time markers mentioned
9. **Physical state**: Injuries, healing, fatigue, power changes

## Rules

- Extract from the TEXT ONLY — do not infer what might happen
- Over-extract: if unsure whether something is significant, include it
- Be specific: \"Lin Chen's left arm fractured\" not \"Lin Chen got hurt\"
- Include chapter-internal time markers
- Note which characters are present in each scene

## Output Format

=== OBSERVATIONS ===

[CHARACTERS]
- <name>: <action/state change> (scene: <location>)

[LOCATIONS]
- <character> moved from <A> to <B>

[RESOURCES]
- <character> gained/lost <item> (quantity: <n>)

[RELATIONSHIPS]
- <charA> → <charB>: <change description>

[EMOTIONS]
- <character>: <before> → <after> (trigger: <event>)

[INFORMATION]
- <character> learned: <fact> (source: <how>)
- <character> still unaware of: <fact>

[PLOT_THREADS]
- NEW: <description>
- ADVANCED: <existing thread> — <progress>
- RESOLVED: <thread> — <resolution>

[TIME]
- <time markers, duration>

[PHYSICAL_STATE]
- <character>: <injury/healing/fatigue/power change>")
    } else {
        format!("{lang_prefix}你是一个事实提取专家。阅读章节正文，提取每一个可观察到的事实变化。

## 提取类别

1. **角色行为**：谁做了什么，对谁，为什么
2. **位置变化**：谁去了哪里，从哪里来
3. **资源变化**：获得、失去、消耗了什么，具体数量
4. **关系变化**：新相遇、信任/不信任转变、结盟、背叛
5. **情绪变化**：角色情绪从X到Y，触发事件是什么
6. **信息流动**：谁知道了什么新信息，谁仍然不知情
7. **剧情线索**：新埋下的悬念、已有线索的推进、线索的解答
8. **时间推进**：过了多少时间，提到的时间标记
9. **身体状态**：受伤、恢复、疲劳、战力变化

## 规则

- 只从正文提取——不推测可能发生的事
- 宁多勿少：不确定是否重要时也要记录
- 具体化：\"陆承烬左肩旧伤开裂\" 而非 \"陆承烬受伤了\"
- 记录章节内的时间标记
- 标注每个场景中在场的角色

## 输出格式

=== OBSERVATIONS ===

[角色行为]
- <角色名>: <行为/状态变化> (场景: <地点>)

[位置变化]
- <角色> 从 <A> 到 <B>

[资源变化]
- <角色> 获得/失去 <物品> (数量: <n>)

[关系变化]
- <角色A> → <角色B>: <变化描述>

[情绪变化]
- <角色>: <之前> → <之后> (触发: <事件>)

[信息流动]
- <角色> 得知: <事实> (来源: <途径>)
- <角色> 仍不知: <事实>

[剧情线索]
- 新埋: <描述>
- 推进: <已有线索> — <进展>
- 回收: <线索> — <解答>

[时间]
- <时间标记、时长>

[身体状态]
- <角色>: <受伤/恢复/疲劳/战力变化>")
    }
}

/// 构造 Observer user prompt。逐字移植 TS `buildObserverUserPrompt`。
///
/// **parity 注意**：TS `language === "en"` 判定——`undefined` 视为 zh（无 genre
/// 回退）。Rust `language == Some(WritingLanguage::En)`，`None` → zh。
pub fn build_observer_user_prompt(
    chapter_number: u32,
    title: &str,
    content: &str,
    language: Option<WritingLanguage>,
) -> String {
    if language == Some(WritingLanguage::En) {
        format!("Extract all facts from Chapter {chapter_number} \"{title}\":\n\n{content}")
    } else {
        format!("请提取第{chapter_number}章「{title}」中的所有事实：\n\n{content}")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::book::{BookStatus, Platform};
    use crate::models::genre_profile::GenreProfile;

    fn book() -> BookConfig {
        BookConfig {
            id: "b1".to_string(),
            title: "无关紧要（observer 未用）".to_string(),
            platform: Platform::Tomato,
            genre: "都市".to_string(),
            status: BookStatus::Active,
            target_chapters: 100,
            chapter_word_count: 2000,
            language: None,
            created_at: "2026-01-01".to_string(),
            updated_at: "2026-01-01".to_string(),
            parent_book_id: None,
            fanfic_mode: None,
            series: None,
            writing: None,
            governance: None,
        }
    }

    fn profile_zh() -> GenreProfile {
        GenreProfile {
            name: "都市".to_string(),
            language: "zh".to_string(),
            ..GenreProfile::default()
        }
    }

    #[test]
    fn system_prompt_zh_full_template() {
        let out = build_observer_system_prompt(&book(), &profile_zh(), None);
        assert!(out.starts_with("你是一个事实提取专家。"));
        assert!(out.contains("## 提取类别"));
        assert!(out.contains("9. **身体状态**：受伤、恢复、疲劳、战力变化"));
        assert!(out.contains("=== OBSERVATIONS ==="));
        assert!(out.contains("[角色行为]"));
        assert!(out.contains("[身体状态]"));
        assert!(out.contains("- 具体化：\"陆承烬左肩旧伤开裂\" 而非 \"陆承烬受伤了\""));
    }

    #[test]
    fn system_prompt_en_full_template_with_chinese_period() {
        let out = build_observer_system_prompt(&book(), &profile_zh(), Some(WritingLanguage::En));
        assert!(out.starts_with("【LANGUAGE OVERRIDE】ALL output MUST be in English.\n\n"));
        // TS en 分支首句的句号仍是中文「。」——逐字保留。
        assert!(out.starts_with(
            "【LANGUAGE OVERRIDE】ALL output MUST be in English.\n\nYou are a fact extraction specialist。Read"
        ));
        assert!(out.contains("## Extraction Categories"));
        assert!(out.contains("[PHYSICAL_STATE]"));
    }

    #[test]
    fn system_prompt_genre_language_fallback() {
        let mut gp = profile_zh();
        gp.language = "en".to_string();
        let out = build_observer_system_prompt(&book(), &gp, None);
        assert!(out.starts_with("【LANGUAGE OVERRIDE】"), "未传语言时回退 genre");
    }

    #[test]
    fn user_prompt_zh_default() {
        let out = build_observer_user_prompt(7, "暗涌", "正文。", None);
        assert_eq!(out, "请提取第7章「暗涌」中的所有事实：\n\n正文。");
    }

    #[test]
    fn user_prompt_explicit_zh() {
        let out = build_observer_user_prompt(7, "暗涌", "正文。", Some(WritingLanguage::Zh));
        assert_eq!(out, "请提取第7章「暗涌」中的所有事实：\n\n正文。");
    }

    #[test]
    fn user_prompt_en() {
        let out = build_observer_user_prompt(7, "Undercurrent", "Body.", Some(WritingLanguage::En));
        assert_eq!(out, "Extract all facts from Chapter 7 \"Undercurrent\":\n\nBody.");
    }
}
