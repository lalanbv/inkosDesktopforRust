//! 528 号：写作链 prompt golden 差分（planner + settler）。
//!
//! 事实源 = `packages/core/src/__tests__/golden/writing-prompts.json`
//! （core `golden-writing-prompts.test.ts` 从 planner-prompts/settler-prompts
//! 纯函数生成）。本测试把手抄移植面与 TS 快照**码点级**比对——防措辞漂移
//! （漂移=双端同输入下给 LLM 的指令分叉，522 号工件差分的上游根源之一）。
//! writer 面（输入含深对象）备案后续轮扩展。

use inkos_engine::agents::planner_prompts::{
    build_planner_user_message, get_planner_memo_system_prompt, get_planner_memo_user_template,
    PlannerLengthBudget, PlannerUserMessageInput,
};
use inkos_engine::agents::settler_prompts::{build_settler_system_prompt, build_settler_user_prompt, SettlerUserPromptInput};
use inkos_engine::models::book::{BookConfig, BookStatus, Platform};
use inkos_engine::models::book_rules::BookRules;
use inkos_engine::models::genre_profile::GenreProfile;
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
}
