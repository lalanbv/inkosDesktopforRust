//! 528 号：写作链 prompt golden 差分（planner + settler + writer）。
//!
//! 事实源 = `packages/core/src/__tests__/golden/writing-prompts.json`
//! （core `golden-writing-prompts.test.ts` 从 planner-prompts/settler-prompts/
//! writer-prompts 纯函数生成）。本测试把手抄移植面与 TS 快照**码点级**比对——
//! 防措辞漂移（漂移=双端同输入下给 LLM 的指令分叉，522 号工件差分的上游根源之一）。
//! 529 号：writer 面 6 案例扩展（对齐 c56586ec 后的 TS 形态）。

use inkos_engine::agents::planner_prompts::{
    build_planner_user_message, get_planner_memo_system_prompt, get_planner_memo_user_template,
    PlannerLengthBudget, PlannerUserMessageInput,
};
use inkos_engine::agents::settler_prompts::{build_settler_system_prompt, build_settler_user_prompt, SettlerUserPromptInput};
use inkos_engine::agents::writer_prompts::{
    build_writer_system_prompt, FanficContext, InputProfile, WriterPromptMode, WriterSystemPromptInput,
};
use inkos_engine::agents::append_activated_skill_guidance;
use inkos_engine::llm::provider::{LLMMessage, LLMRole};
use inkos_engine::models::book::{BookConfig, BookStatus, FanficMode, Platform};
use inkos_engine::models::book_rules::{BookRules, GenreLock, NarrativePerson, Protagonist};
use inkos_engine::models::genre_profile::GenreProfile;
use inkos_engine::skills::{AgentSkill, SkillSource};
use inkos_engine::utils::language::WritingLanguage;
use serde_json::Value;

const GOLDEN: &str = include_str!("../../packages/core/src/__tests__/golden/writing-prompts.json");

fn golden() -> Value {
    serde_json::from_str(GOLDEN).expect("golden json")
}

fn assert_match(key: &str, got: String, golden: &Value) {
    let expected = golden
        .get(key)
        .unwrap_or_else(|| panic!("golden 缺 key: {key}"))
        .as_str()
        .unwrap_or_else(|| panic!("golden key 非字符串: {key}"));
    assert_eq!(got, expected, "writing prompt '{key}' 漂移");
}

fn zh() -> WritingLanguage {
    WritingLanguage::Zh
}

fn en() -> WritingLanguage {
    WritingLanguage::En
}

fn settler_book() -> BookConfig {
    BookConfig {
        series_id: None,
        id: "b1".into(),
        title: "镜花水月".into(),
        platform: Platform::Qidian,
        genre: "东方玄幻".into(),
        status: BookStatus::Active,
        target_chapters: 200,
        chapter_word_count: 3000,
        language: None,
        created_at: "2026-01-01T00:00:00.000Z".into(),
        updated_at: "2026-01-01T00:00:00.000Z".into(),
        parent_book_id: None,
        fanfic_mode: None,
        series: None,
        writing: None,
        governance: None,
    }
}

fn settler_genre(numerical: bool) -> GenreProfile {
    GenreProfile {
        name: "东方玄幻".into(),
        id: "xuanhuan".into(),
        language: "zh".into(),
        chapter_types: vec!["日常推进".into()],
        fatigue_words: vec![],
        numerical_system: numerical,
        power_scaling: false,
        era_research: false,
        pacing_rule: String::new(),
        satisfaction_types: vec![],
        audit_dimensions: vec![],
    }
}

#[test]
fn writing_prompts_match_ts_snapshot() {
    let golden = golden();

    // planner：system / user 模板直出。
    assert_match("planner.system.zh", get_planner_memo_system_prompt(zh()).to_string(), &golden);
    assert_match("planner.system.en", get_planner_memo_system_prompt(en()).to_string(), &golden);
    assert_match("planner.template.zh", get_planner_memo_user_template(zh()).to_string(), &golden);
    assert_match("planner.template.en", get_planner_memo_user_template(en()).to_string(), &golden);

    // planner user：黄金三章案例（ch2 + brief + 本章指令）。
    let user_golden = PlannerUserMessageInput {
        chapter_number: 2,
        previous_chapter_ending_excerpt: "……门缝里的光熄了。",
        recent_summaries: "- 第1章：林秋入府为杂役，捡到残缺腰牌。",
        current_arc_prose: "主线：腰牌来历牵出府邸旧案。",
        protagonist_matrix_row: "- 林秋｜杂役｜隐忍、观察力强",
        opponent_rows: "- 王管事｜克扣月钱、试探新人",
        collaborator_rows: "- 师兄｜指点规矩、来路不明",
        relevant_threads: "- H003 杂役腰牌（pressured）",
        recyclable_hooks: "- H003：距上次推进 1 章",
        is_golden_opening: true,
        length_budget: PlannerLengthBudget {
            target: 2000,
            soft_min: 1600,
            soft_max: 2400,
            hard_min: 1200,
            hard_max: 3000,
            unit: "字",
        },
        book_rules_relevant: "- 禁止主角突然圣母\n- 反派不降智",
        brief: Some("双主线：权谋 60% + 感情 40%；开场三章节奏要快。"),
        chapter_context: Some("本章要出现第一次系统提示。"),
        language: zh(),
    };
    assert_match("planner.user.zh.golden", build_planner_user_message(&user_golden), &golden);

    // planner user：普通章节案例（ch5，无 brief/指令，英文书）。
    let user_plain = PlannerUserMessageInput {
        chapter_number: 5,
        previous_chapter_ending_excerpt: "The light behind the door went out.",
        recent_summaries: "- Ch1: Lin Qiu enters the manor as a servant.",
        current_arc_prose: "Main line: the token traces back to an old case.",
        protagonist_matrix_row: "- Lin Qiu | servant | observant",
        opponent_rows: "- Steward Wang | petty tyranny",
        collaborator_rows: "- Senior brother | unclear motives",
        relevant_threads: "- H003 servant token (pressured)",
        recyclable_hooks: "- H003: advanced 1 chapter ago",
        is_golden_opening: false,
        length_budget: PlannerLengthBudget {
            target: 2000,
            soft_min: 1600,
            soft_max: 2400,
            hard_min: 1200,
            hard_max: 3000,
            unit: "words",
        },
        book_rules_relevant: "- No sudden saintliness",
        brief: None,
        chapter_context: None,
        language: en(),
    };
    assert_match("planner.user.en.plain", build_planner_user_message(&user_plain), &golden);

    // settler system：无数值体系基线 + 数值/全员分支。
    let book = settler_book();
    let genre_plain = settler_genre(false);
    let genre_numerical = settler_genre(true);
    let full_cast_rules = BookRules {
        version: "1.0".into(),
        protagonist: None,
        genre_lock: None,
        narrative_person: None,
        numerical_system_overrides: None,
        era_constraints: None,
        prohibitions: vec![],
        chapter_types_override: vec![],
        fatigue_words_override: vec![],
        additional_audit_dimensions: vec![],
        enable_full_cast_tracking: true,
        fanfic_mode: None,
        allowed_deviations: vec![],
    };
    assert_match(
        "settler.system.zh.plain",
        build_settler_system_prompt(&book, &genre_plain, None, None),
        &golden,
    );
    assert_match(
        "settler.system.zh.numerical",
        build_settler_system_prompt(&book, &genre_numerical, Some(&full_cast_rules), None),
        &golden,
    );

    // settler user：全量案例（可选块全给 + governed 控制块让卷纲让位）。
    let user_full = SettlerUserPromptInput {
        chapter_number: 12,
        title: "镜中裂痕",
        content: "林秋推开门，看见了不该看见的东西。",
        current_state: "位置：柴房；目标：查清腰牌来历",
        ledger: "灵石：120→80",
        hooks: "- H003 杂役腰牌（pressured，最近推进 11）",
        chapter_summaries: "## 第 11 章\n- 林秋被罚抄规矩。",
        subplot_board: "- 支线A：师兄的身世（推进中）",
        emotional_arcs: "- 林秋：隐忍→将爆发",
        character_matrix: "- 林秋×王管事：提防",
        volume_outline: "第 12 章：腰牌第一次发光。",
        observations: Some("1) 林秋进入正房 2) 腰牌发热"),
        selected_evidence_block: Some("- E009 门口血迹（第 9 章）"),
        governed_control_block: Some("【GOVERNED CONTROL】按治理方案压缩支线。"),
        validation_feedback: Some("上一轮 H003 状态与正文矛盾，请修正。"),
    };
    assert_match("settler.user.zh.full", build_settler_user_prompt(&user_full), &golden);

    // settler user：最小案例（truth 文件未创建 + 卷纲块兜底）。
    let user_min = SettlerUserPromptInput {
        chapter_number: 1,
        title: "入府",
        content: "林秋背着一卷铺盖走进角门。",
        current_state: "(文件尚未创建)",
        ledger: "",
        hooks: "(文件尚未创建)",
        chapter_summaries: "(文件尚未创建)",
        subplot_board: "(文件尚未创建)",
        emotional_arcs: "(文件尚未创建)",
        character_matrix: "(文件尚未创建)",
        volume_outline: "第 1 章：入府，捡到腰牌。",
        observations: None,
        selected_evidence_block: None,
        governed_control_block: None,
        validation_feedback: None,
    };
    assert_match("settler.user.zh.min", build_settler_user_prompt(&user_min), &golden);

    // ---------------------------------------------------------------------------
    // writer system：6 案例面（529 号扩展）。生产唯一调用形态 = creative + governed；
    // 其余面锁分支矩阵（en / golden-open / legacy+full / numerical+fullCast+主角铁律 /
    // fanfic 三段）。length_spec 一律 None → build_length_spec(3000) 默认推导。
    // ---------------------------------------------------------------------------

    let writer_genre_en = GenreProfile {
        name: "Eastern Xuanhuan".into(),
        language: "en".into(),
        ..settler_genre(false)
    };
    let writer_rules_full = BookRules {
        version: "1.0".into(),
        protagonist: Some(Protagonist {
            name: "林秋".into(),
            personality_lock: vec!["隐忍".into(), "观察力强".into()],
            behavioral_constraints: vec!["不滥杀".into(), "不透露腰牌来历".into()],
        }),
        genre_lock: Some(GenreLock {
            primary: "东方玄幻".into(),
            forbidden: vec!["科幻".into(), "西幻".into()],
        }),
        narrative_person: Some(NarrativePerson::First),
        numerical_system_overrides: None,
        era_constraints: None,
        prohibitions: vec!["禁止主角突然圣母".into(), "反派不降智".into()],
        chapter_types_override: vec![],
        fatigue_words_override: vec![],
        additional_audit_dimensions: vec![],
        enable_full_cast_tracking: true,
        fanfic_mode: None,
        allowed_deviations: vec![],
    };

    // 生产主形态：zh + creative + governed，ch6（无黄金开篇段）。
    let writer_governed_creative = WriterSystemPromptInput {
        book: Some(&book),
        genre_profile: Some(&genre_plain),
        book_rules: None,
        book_rules_body: "",
        genre_body: "题材正文：灵气复苏下的都市修行。",
        style_guide: "## 文风\n短句为主，动作外化情绪。",
        style_fingerprint: None,
        chapter_number: Some(6),
        mode: Some(WriterPromptMode::Creative),
        fanfic_context: None,
        language_override: None,
        input_profile: Some(InputProfile::Governed),
        length_spec: None,
    };
    assert_match(
        "writer.system.zh.governed-creative",
        build_writer_system_prompt(&writer_governed_creative),
        &golden,
    );

    // 黄金开篇：ch2 追加黄金三章纪律段。
    let writer_golden_open = WriterSystemPromptInput {
        book: Some(&book),
        genre_profile: Some(&genre_plain),
        book_rules: None,
        book_rules_body: "",
        genre_body: "",
        style_guide: "",
        style_fingerprint: None,
        chapter_number: Some(2),
        mode: Some(WriterPromptMode::Creative),
        fanfic_context: None,
        language_override: None,
        input_profile: Some(InputProfile::Governed),
        length_spec: None,
    };
    assert_match(
        "writer.system.zh.governed-creative-golden-open",
        build_writer_system_prompt(&writer_golden_open),
        &golden,
    );

    // 英文书：en 序列 + en 字数单位（words）。
    let writer_en = WriterSystemPromptInput {
        book: Some(&book),
        genre_profile: Some(&writer_genre_en),
        book_rules: None,
        book_rules_body: "",
        genre_body: "Genre guidance: qi revival in a modern city.",
        style_guide: "Style: short sentences, show don't tell.",
        style_fingerprint: Some("风格指纹样本"),
        chapter_number: Some(12),
        mode: Some(WriterPromptMode::Creative),
        fanfic_context: None,
        language_override: Some(WritingLanguage::En),
        input_profile: Some(InputProfile::Governed),
        length_spec: None,
    };
    assert_match(
        "writer.system.en.governed-creative",
        build_writer_system_prompt(&writer_en),
        &golden,
    );

    // 库兼容面：legacy + full（旧输出格式）；style_guide 缺失标记 → 文风指南段缺位。
    let writer_legacy_full = WriterSystemPromptInput {
        book: Some(&book),
        genre_profile: Some(&genre_plain),
        book_rules: None,
        book_rules_body: "本书专属规则正文：禁止圣母。",
        genre_body: "",
        style_guide: "(文件尚未创建)",
        style_fingerprint: None,
        chapter_number: Some(9),
        mode: Some(WriterPromptMode::Full),
        fanfic_context: None,
        language_override: None,
        input_profile: None,
        length_spec: None,
    };
    assert_match(
        "writer.system.zh.legacy-full",
        build_writer_system_prompt(&writer_legacy_full),
        &golden,
    );

    // 分支全家桶：numerical + fullCast + 主角铁律 + 人称硬约束 + 禁忌/风格禁区。
    let writer_full_cast = WriterSystemPromptInput {
        book: Some(&book),
        genre_profile: Some(&genre_numerical),
        book_rules: Some(&writer_rules_full),
        book_rules_body: "",
        genre_body: "",
        style_guide: "文风正文",
        style_fingerprint: None,
        chapter_number: Some(7),
        mode: Some(WriterPromptMode::Full),
        fanfic_context: None,
        language_override: None,
        input_profile: None,
        length_spec: None,
    };
    assert_match(
        "writer.system.zh.full-cast-numerical",
        build_writer_system_prompt(&writer_full_cast),
        &golden,
    );

    // 同人三段：canon 模式 + 允许偏离清单。
    let writer_fanfic_ctx = FanficContext {
        fanfic_canon: "原作设定：林秋为杂役，腰牌来历不明。".into(),
        fanfic_mode: FanficMode::Canon,
        allowed_deviations: vec!["口头禅可保留".into()],
    };
    let writer_fanfic = WriterSystemPromptInput {
        book: Some(&book),
        genre_profile: Some(&genre_plain),
        book_rules: None,
        book_rules_body: "",
        genre_body: "",
        style_guide: "",
        style_fingerprint: None,
        chapter_number: Some(5),
        mode: Some(WriterPromptMode::Creative),
        fanfic_context: Some(&writer_fanfic_ctx),
        language_override: None,
        input_profile: None,
        length_spec: None,
    };
    assert_match(
        "writer.system.zh.fanfic",
        build_writer_system_prompt(&writer_fanfic),
        &golden,
    );

    // ---------------------------------------------------------------------------
    // skill 激活指导段（531 号）：append_activated_skill_guidance 拼接格式
    // 与 TS golden 码点级比对（530 链级注入的 system 追加段）。
    // ---------------------------------------------------------------------------

    let craft_skill = AgentSkill {
        id: "inkos-long-writing".into(),
        name: "Long-form narrative craft".into(),
        description: "长篇小说的场景构造、人物因果、信息释放与连载节奏。".into(),
        body: "Turn the chapter goal into scenes with an immediate objective, resistance, a meaningful turn.".into(),
        source: SkillSource::Builtin,
        base_dir: None,
    };

    // 单技能、无参考资源、无既有 system → 指导段前置为首条 system。
    let mut plain = vec![LLMMessage {
        role: LLMRole::User,
        content: "写下一章。".into(),
        tool_calls: None,
        tool_call_id: None,
    }];
    append_activated_skill_guidance(
        &mut plain,
        &[inkos_engine::skills::production_bindings::ActivatedSkillGuidance {
            skill: craft_skill.clone(),
            resources: vec![],
        }],
    );
    assert_match("skill.guidance.plain", plain[0].content.clone(), &golden);

    // 双技能 + 参考资源 + 空 body 回退 description + 既有 system 追加形态。
    let fallback_skill = AgentSkill {
        id: "inkos-story-review".into(),
        name: "Story review".into(),
        body: "   ".into(),
        ..craft_skill.clone()
    };
    let mut full = vec![
        LLMMessage { role: LLMRole::System, content: "你是写手。".into(), tool_calls: None, tool_call_id: None },
        LLMMessage { role: LLMRole::User, content: "继续。".into(), tool_calls: None, tool_call_id: None },
    ];
    append_activated_skill_guidance(
        &mut full,
        &[
            inkos_engine::skills::production_bindings::ActivatedSkillGuidance {
                skill: craft_skill,
                resources: vec![
                    inkos_engine::skills::production_bindings::ActivatedSkillResource {
                        path: "references/craft.md".into(),
                        heading: Some("节奏".into()),
                        body: "控制信息释放密度。".into(),
                        char_start: 12,
                        char_end: 88,
                    },
                    inkos_engine::skills::production_bindings::ActivatedSkillResource {
                        path: "references/review.md".into(),
                        heading: None,
                        body: "复审清单。".into(),
                        char_start: 0,
                        char_end: 40,
                    },
                ],
            },
            inkos_engine::skills::production_bindings::ActivatedSkillGuidance {
                skill: fallback_skill,
                resources: vec![],
            },
        ],
    );
    assert_match("skill.guidance.full", full[0].content.clone(), &golden);
}
