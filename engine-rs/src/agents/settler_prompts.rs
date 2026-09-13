//! 结算 agent 提示词（settler-prompts）。
//!
//! 移植自 `packages/core/src/agents/settler-prompts.ts`（230 行）。纯函数：
//! writer 编排 Phase 2b（Reflector）的 system / user prompt 构造。

use crate::models::book::BookConfig;
use crate::models::book_rules::BookRules;
use crate::models::genre_profile::GenreProfile;
use crate::utils::language::WritingLanguage;

/// 构造结算 system prompt。逐字移植 TS `buildSettlerSystemPrompt`。
///
/// 语言合并对齐 TS `language ?? genreProfile.language`（显式传入优先，缺失才回退
/// genre 画像语言）。数值块 / 全员追踪块按条件拼接；hook 规则段固定中文原文
/// （TS 源即如此，en 仅前置 LANGUAGE OVERRIDE 头）。
pub fn build_settler_system_prompt(
    book: &BookConfig,
    genre_profile: &GenreProfile,
    book_rules: Option<&BookRules>,
    language: Option<WritingLanguage>,
) -> String {
    // TS: const resolvedLang = language ?? genreProfile.language; isEnglish = resolvedLang === "en"
    let is_english = match language {
        Some(lang) => lang == WritingLanguage::En,
        None => genre_profile.language == "en",
    };

    let numerical_block = if genre_profile.numerical_system {
        "\n- 本题材有数值/资源体系，你必须在 UPDATED_LEDGER 中追踪正文中出现的所有资源变动\n- 数值验算铁律：期初 + 增量 = 期末，三项必须可验算"
    } else {
        "\n- 本题材无数值系统，UPDATED_LEDGER 留空"
    };

    let hook_rules = "
## 伏笔追踪规则（严格执行）

- 新伏笔：只有当正文中出现一个会延续到后续章节、且有具体回收方向的未解问题时，才新增 hook_id。不要为旧 hook 的换说法、重述、抽象总结再开新 hook
- 提及伏笔：已有伏笔在本章被提到，但没有新增信息、没有改变读者或角色对该问题的理解 → 放入 mention 数组，不要更新最近推进
- 推进伏笔：已有伏笔在本章出现了新的事实、证据、关系变化、风险升级或范围收缩 → **必须**更新\"最近推进\"列为当前章节号，更新状态和备注
- 回收伏笔：伏笔在本章被明确揭示、解决、或不再成立 → 状态改为\"已回收\"，备注回收方式
- 延后伏笔：只有当正文明确显示该线被主动搁置、转入后台、或被剧情压后时，才标注\"延后\"；不要因为\u{201c}已经过了几章\u{201d}就机械延后
- 当前伏笔池会同时提供活跃伏笔和与本章语义相关的休眠种子。休眠不等于无关：本章如果启动、改写或具体化了它，必须复用它已有的 hookId，并在 hookOps.upsert 中更新状态、回收方向和备注
- 判断“正文的新表述是否仍是既有叙事承诺”是你的语义职责。即使人物、数字、证据形式或措辞发生变化，只要它承接的是同一悬念/冲突/回收承诺，就更新既有 hookId，不要另开候选
- newHookCandidates 只用于当前伏笔池中没有任何一条能代表的全新叙事承诺。宿主只校验结构，不会再用关键词替你猜语义归属
- payoffTiming 使用语义节奏，不用硬写章节号：只允许 immediate / near-term / mid-arc / slow-burn / endgame
- **铁律**：不要把\u{201c}再次提到\u{201d}\u{201c}换个说法重述\u{201d}\u{201c}抽象复盘\u{201d}当成推进。只有状态真的变了，才更新最近推进。只是出现过的旧 hook，放进 mention 数组。";

    let full_cast_block = match book_rules.map(|rules| rules.enable_full_cast_tracking) {
        Some(true) => "\n## 全员追踪\nPOST_SETTLEMENT 必须额外包含：本章出场角色清单、角色间关系变动、未出场但被提及的角色。",
        _ => "",
    };

    let lang_prefix = if is_english {
        "【LANGUAGE OVERRIDE】ALL output (state card, hooks, summaries, subplots, emotional arcs, character matrix) MUST be in English. The === TAG === markers remain unchanged.\n\n"
    } else {
        ""
    };

    format!(
        "{lang_prefix}你是状态追踪分析师。给定新章节正文和当前 truth 文件，你的任务是产出更新后的 truth 文件。

## 工作模式

你不是在写作。你的任务是：
1. 仔细阅读正文，提取所有状态变化
2. 基于\"当前追踪文件\"做增量更新
3. 严格按照 === TAG === 格式输出

## 分析维度

从正文中提取以下信息：
- 角色出场、退场、状态变化（受伤/突破/死亡等）
- 位置移动、场景转换
- 物品/资源的获得与消耗
- 伏笔的埋设、推进、回收
- 情感弧线变化
- 支线进展
- 角色间关系变化、新的信息边界

## 书籍信息

- 标题：{title}
- 题材：{genre_name}（{genre}）
- 平台：{platform}
{numerical_block}
{hook_rules}{full_cast_block}

## 输出格式（必须严格遵循）

{output_format}

## 关键规则

1. 状态卡和伏笔池必须基于\"当前追踪文件\"做增量更新，不是从零开始
2. 正文中的每一个事实性变化都必须反映在对应的追踪文件中
3. 不要遗漏细节：数值变化、位置变化、关系变化、信息变化都要记录
4. 角色交互矩阵中的\"信息边界\"要准确——角色只知道他在场时发生的事

## 铁律：只记录正文中实际发生的事（严格执行）

- **只提取正文中明确描写的事件和状态变化**。不要推断、预测、或补充正文没有写到的内容
- 如果正文只写到角色走到门口还没进去，状态卡就不能写\"角色已进入房间\"
- 如果正文只暗示了某种可能性但没有确认，不要把它当作已发生的事实记录
- 不要从卷纲或大纲中补充正文尚未到达的剧情到状态卡
- 不要删除或修改已有 hooks 中与本章无关的内容——只更新本章正文涉及的 hooks
- 第 1 章尤其注意：初始追踪文件可能包含从大纲预生成的内容，只保留正文实际支持的部分，不要保留正文未涉及的预设
- **伏笔例外**：正文中出现的未解疑问、悬念、伏笔线索必须在 hooks 中记录。这不是\"推断\"，而是\"提取正文中的叙事承诺\"。如果正文暗示了一个谜题/冲突/秘密但没有解答，那就是一个 hook，必须记录",
        title = book.title,
        genre_name = genre_profile.name,
        genre = book.genre,
        platform = book.platform.as_str(),
        numerical_block = numerical_block,
        hook_rules = hook_rules,
        full_cast_block = full_cast_block,
        output_format = build_settler_output_format(genre_profile),
    )
}

/// 结算输出格式段。逐字移植 TS `buildSettlerOutputFormat`：
/// `chapterTypes[0]` 缺省回退 `"主线推进"`。
fn build_settler_output_format(gp: &GenreProfile) -> String {
    let chapter_type_example = gp.chapter_types.first().map(String::as_str).unwrap_or("主线推进");

    format!(
        "=== POST_SETTLEMENT ===
（简要说明本章有哪些状态变动、伏笔推进、结算注意事项；允许 Markdown 表格或要点）

=== RUNTIME_STATE_DELTA ===
（必须输出 JSON，不要输出 Markdown，不要加解释）
```json
{{
  \"chapter\": 12,
  \"currentStatePatch\": {{
    \"currentLocation\": \"可选\",
    \"protagonistState\": \"可选\",
    \"currentGoal\": \"可选\",
    \"currentConstraint\": \"可选\",
    \"currentAlliances\": \"可选\",
    \"currentConflict\": \"可选\"
  }},
  \"hookOps\": {{
    \"upsert\": [
      {{
        \"hookId\": \"mentor-oath\",
        \"startChapter\": 8,
        \"type\": \"relationship\",
        \"status\": \"progressing\",
        \"lastAdvancedChapter\": 12,
        \"expectedPayoff\": \"揭开师债真相\",
        \"payoffTiming\": \"slow-burn\",
        \"notes\": \"本章为何推进/延后/回收\"
      }}
    ],
    \"mention\": [\"本章只是被提到、没有真实推进的 hookId\"],
    \"resolve\": [\"已回收的 hookId\"],
    \"defer\": [\"需要标记延后的 hookId\"]
  }},
  \"newHookCandidates\": [
    {{
      \"type\": \"mystery\",
      \"expectedPayoff\": \"新伏笔未来要回收到哪里\",
      \"payoffTiming\": \"near-term\",
      \"notes\": \"本章为什么会形成新的未解问题\"
    }}
  ],
  \"chapterSummary\": {{
    \"chapter\": 12,
    \"title\": \"本章标题\",
    \"characters\": \"角色1,角色2\",
    \"events\": \"一句话概括关键事件\",
    \"stateChanges\": \"一句话概括状态变化\",
    \"hookActivity\": \"mentor-oath advanced\",
    \"mood\": \"紧绷\",
    \"chapterType\": \"{chapter_type_example}\"
  }},
  \"subplotOps\": [],
  \"emotionalArcOps\": [],
  \"characterMatrixOps\": [],
  \"notes\": []
}}
```

=== TENSION_METRICS ===
（R2 张力曲线：给本章打两个整数分，范围 1–10。conflictLevel=冲突强度（本章正文的冲突/对抗激烈程度），revealLevel=揭示强度（章末钩子/新信息揭示的力度）。只输出下面两行，不要加解释）
conflictLevel: 7
revealLevel: 6

规则：
1. 只输出增量，不要重写完整 truth files
2. 所有章节号字段都必须是整数，不能写自然语言
3. hookOps.upsert 里只能写\u{201c}当前伏笔池里已经存在\u{201d}的 hookId，不允许发明新的 hookId；语义上承接既有伏笔时必须复用该 id
4. 只有确认当前伏笔池没有同一叙事承诺时，brand-new unresolved thread 才写进 newHookCandidates
5. 如果旧 hook 只是被提到、没有真实状态变化，把它放进 mention，不要更新 lastAdvancedChapter
6. 如果本章推进了旧 hook，lastAdvancedChapter 必须等于当前章号
7. 如果回收或延后 hook，必须放在 resolve / defer 数组里
8. chapterSummary.chapter 必须等于当前章节号"
    )
}

/// 结算 user prompt 入参（对齐 TS `buildSettlerUserPrompt` 的参数对象）。
#[derive(Debug, Clone, Copy)]
pub struct SettlerUserPromptInput<'a> {
    pub chapter_number: u32,
    pub title: &'a str,
    pub content: &'a str,
    pub current_state: &'a str,
    pub ledger: &'a str,
    pub hooks: &'a str,
    pub chapter_summaries: &'a str,
    pub subplot_board: &'a str,
    pub emotional_arcs: &'a str,
    pub character_matrix: &'a str,
    pub volume_outline: &'a str,
    pub observations: Option<&'a str>,
    pub selected_evidence_block: Option<&'a str>,
    pub governed_control_block: Option<&'a str>,
    pub validation_feedback: Option<&'a str>,
}

/// 文件占位文案（对齐 TS `"(文件尚未创建)"` 判定）。
const FILE_NOT_CREATED: &str = "(文件尚未创建)";

/// 构造结算 user prompt。逐字移植 TS `buildSettlerUserPrompt`。
///
/// 条件块语义：ledger / observations / evidence / feedback 为非空字符串才输出
/// （TS truthy）；四类 truth 文件以 `!== "(文件尚未创建)"` 判定；
/// `governed_control_block` 存在时卷纲块让位（TS `controlBlock.length === 0` 互斥）。
pub fn build_settler_user_prompt(params: &SettlerUserPromptInput<'_>) -> String {
    let p = params;
    let ledger_block = if !p.ledger.is_empty() {
        format!("\n## 当前资源账本\n{}\n", p.ledger)
    } else {
        String::new()
    };

    let summaries_block = if p.chapter_summaries != FILE_NOT_CREATED {
        format!("\n## 已有章节摘要\n{}\n", p.chapter_summaries)
    } else {
        String::new()
    };

    let subplot_block = if p.subplot_board != FILE_NOT_CREATED {
        format!("\n## 当前支线进度板\n{}\n", p.subplot_board)
    } else {
        String::new()
    };

    let emotional_block = if p.emotional_arcs != FILE_NOT_CREATED {
        format!("\n## 当前情感弧线\n{}\n", p.emotional_arcs)
    } else {
        String::new()
    };

    let matrix_block = if p.character_matrix != FILE_NOT_CREATED {
        format!("\n## 当前角色交互矩阵\n{}\n", p.character_matrix)
    } else {
        String::new()
    };

    let observations_block = match p.observations.filter(|s| !s.is_empty()) {
        Some(observations) => format!(
            "\n## 观察日志（由 Observer 提取，包含本章所有事实变化）\n{observations}\n\n基于以上观察日志和正文，更新所有追踪文件。确保观察日志中的每一项变化都反映在对应的文件中。\n"
        ),
        None => String::new(),
    };
    let selected_evidence_block = match p.selected_evidence_block.filter(|s| !s.is_empty()) {
        Some(block) => format!("\n## 已选长程证据\n{block}\n"),
        None => String::new(),
    };
    let control_block = p.governed_control_block.unwrap_or("");
    let outline_block = if control_block.is_empty() {
        format!("\n## 卷纲\n{}\n", p.volume_outline)
    } else {
        String::new()
    };
    let validation_feedback_block = match p.validation_feedback.filter(|s| !s.is_empty()) {
        Some(feedback) => format!(
            "\n## 状态校验反馈\n{feedback}\n\n请严格纠正这些矛盾，只修正 truth files，不要改写正文，不要引入正文中不存在的新事实。\n"
        ),
        None => String::new(),
    };

    format!(
        "请分析第{chapter}章「{title}」的正文，更新所有追踪文件。
{observations_block}
{validation_feedback_block}
## 本章正文

{content}
{control_block}

## 当前状态卡
{current_state}
{ledger_block}
## 当前伏笔池（含活跃伏笔与本章语义相关的休眠种子）
{hooks}
{selected_evidence_block}{summaries_block}{subplot_block}{emotional_block}{matrix_block}
{outline_block}

请严格按照 === TAG === 格式输出结算结果。",
        chapter = p.chapter_number,
        title = p.title,
        content = p.content,
        control_block = control_block,
        current_state = p.current_state,
        ledger_block = ledger_block,
        hooks = p.hooks,
        selected_evidence_block = selected_evidence_block,
        summaries_block = summaries_block,
        subplot_block = subplot_block,
        emotional_block = emotional_block,
        matrix_block = matrix_block,
        outline_block = outline_block,
        observations_block = observations_block,
        validation_feedback_block = validation_feedback_block,
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::book::{BookStatus, Platform};
    use crate::models::genre_profile::GenreProfile;

    fn book() -> BookConfig {
        BookConfig {
            series_id: None,
            id: "b1".to_string(),
            title: "测试之书".to_string(),
            platform: Platform::Tomato,
            genre: "都市脑洞".to_string(),
            status: BookStatus::Active,
            target_chapters: 300,
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

    fn profile(numerical: bool, chapter_types: &[&str]) -> GenreProfile {
        GenreProfile {
            name: "都市脑洞".to_string(),
            id: "urban".to_string(),
            language: "zh".to_string(),
            chapter_types: chapter_types.iter().map(|s| s.to_string()).collect(),
            numerical_system: numerical,
            ..GenreProfile::default()
        }
    }

    #[test]
    fn system_prompt_zh_numerical_with_chapter_types() {
        let gp = profile(true, &["主线推进", "情感过渡"]);
        let out = build_settler_system_prompt(&book(), &gp, None, None);
        assert!(out.starts_with("你是状态追踪分析师。"));
        assert!(out.contains("- 标题：测试之书"));
        assert!(out.contains("- 题材：都市脑洞（都市脑洞）"));
        assert!(out.contains("- 平台：tomato"));
        assert!(out.contains("- 本题材有数值/资源体系"));
        assert!(out.contains("## 伏笔追踪规则（严格执行）"));
        assert!(!out.contains("## 全员追踪"), "无 book_rules 时无全员追踪块");
        // chapterTypes[0] 插值
        assert!(out.contains("\"chapterType\": \"主线推进\""));
        assert!(out.contains("=== RUNTIME_STATE_DELTA ==="));
        assert!(out.contains("8. chapterSummary.chapter 必须等于当前章节号"));
    }

    #[test]
    fn system_prompt_numerical_false_and_no_chapter_types() {
        let gp = profile(false, &[]);
        let out = build_settler_system_prompt(&book(), &gp, None, None);
        assert!(out.contains("- 本题材无数值系统，UPDATED_LEDGER 留空"));
        // chapterTypes 空 → 回退 "主线推进"
        assert!(out.contains("\"chapterType\": \"主线推进\""));
    }

    #[test]
    fn system_prompt_english_prefix_but_chinese_body() {
        let gp = profile(true, &["伏笔回收"]);
        let out = build_settler_system_prompt(&book(), &gp, None, Some(WritingLanguage::En));
        assert!(out.starts_with("【LANGUAGE OVERRIDE】ALL output"));
        assert!(
            out.contains("The === TAG === markers remain unchanged.\n\n你是状态追踪分析师。"),
            "en 仅加前缀，正文仍是中文（TS 原样）"
        );
    }

    #[test]
    fn explicit_zh_overrides_en_genre_profile() {
        // TS ?? 语义：显式传入优先于 genre 画像语言。
        let mut gp = profile(false, &[]);
        gp.language = "en".to_string();
        let out = build_settler_system_prompt(&book(), &gp, None, Some(WritingLanguage::Zh));
        assert!(out.starts_with("你是状态追踪分析师。"), "显式 zh 压过 genre en");
    }

    #[test]
    fn genre_profile_language_fallback() {
        let mut gp = profile(false, &[]);
        gp.language = "en".to_string();
        let out = build_settler_system_prompt(&book(), &gp, None, None);
        assert!(out.starts_with("【LANGUAGE OVERRIDE】"), "未传语言时回退 genre 画像");
    }

    #[test]
    fn system_prompt_full_cast_block_only_when_enabled() {
        let gp = profile(false, &[]);
        let mut rules = BookRules::default();
        assert!(!rules.enable_full_cast_tracking, "默认关闭");
        let off = build_settler_system_prompt(&book(), &gp, Some(&rules), None);
        assert!(!off.contains("## 全员追踪"));

        rules.enable_full_cast_tracking = true;
        let on = build_settler_system_prompt(&book(), &gp, Some(&rules), None);
        assert!(on.contains("## 全员追踪\nPOST_SETTLEMENT 必须额外包含"));
    }

    #[test]
    fn user_prompt_minimal_shape() {
        let out = build_settler_user_prompt(&SettlerUserPromptInput {
            chapter_number: 12,
            title: "试炼",
            content: "正文内容。",
            current_state: "状态卡内容",
            ledger: "",
            hooks: "伏笔池内容",
            chapter_summaries: FILE_NOT_CREATED,
            subplot_board: FILE_NOT_CREATED,
            emotional_arcs: FILE_NOT_CREATED,
            character_matrix: FILE_NOT_CREATED,
            volume_outline: "第一卷：开局",
            observations: None,
            selected_evidence_block: None,
            governed_control_block: None,
            validation_feedback: None,
        });
        assert!(out.starts_with("请分析第12章「试炼」的正文，更新所有追踪文件。\n"));
        assert!(out.contains("## 本章正文\n\n正文内容。\n\n"));
        assert!(out.contains("## 当前状态卡\n状态卡内容\n"));
        assert!(out.contains("## 当前伏笔池（含活跃伏笔与本章语义相关的休眠种子）\n伏笔池内容\n"));
        assert!(out.contains("## 卷纲\n第一卷：开局\n"), "无 control block 时输出卷纲");
        assert!(!out.contains("## 当前资源账本"), "空 ledger 无账本块");
        assert!(!out.contains("## 已有章节摘要"), "占位文案不输出摘要块");
        assert!(out.ends_with("请严格按照 === TAG === 格式输出结算结果。"));
    }

    #[test]
    fn user_prompt_all_blocks_present() {
        let out = build_settler_user_prompt(&SettlerUserPromptInput {
            chapter_number: 3,
            title: "转折",
            content: "内容",
            current_state: "状态",
            ledger: "灵石 120",
            hooks: "H01",
            chapter_summaries: "| 章节 |",
            subplot_board: "支线A",
            emotional_arcs: "弧线",
            character_matrix: "矩阵",
            volume_outline: "不该出现的卷纲",
            observations: Some("观察1"),
            selected_evidence_block: Some("证据块"),
            governed_control_block: Some("\n## 本章控制输入\nintent"),
            validation_feedback: Some("状态矛盾：X"),
        });
        assert!(out.contains("## 观察日志（由 Observer 提取，包含本章所有事实变化）\n观察1"));
        assert!(out.contains("基于以上观察日志和正文，更新所有追踪文件。"));
        assert!(out.contains("## 状态校验反馈\n状态矛盾：X"));
        assert!(out.contains("## 已选长程证据\n证据块"));
        assert!(out.contains("## 当前资源账本\n灵石 120"));
        assert!(out.contains("## 已有章节摘要\n| 章节 |"));
        assert!(out.contains("## 当前支线进度板\n支线A"));
        assert!(out.contains("## 当前情感弧线\n弧线"));
        assert!(out.contains("## 当前角色交互矩阵\n矩阵"));
        assert!(out.contains("## 本章控制输入\nintent"));
        assert!(!out.contains("不该出现的卷纲"), "有 control block 时卷纲块让位");
    }

    #[test]
    fn user_prompt_empty_optional_strings_are_skipped() {
        // TS truthy：空字符串视同缺失。
        let out = build_settler_user_prompt(&SettlerUserPromptInput {
            chapter_number: 1,
            title: "t",
            content: "c",
            current_state: "s",
            ledger: "",
            hooks: "h",
            chapter_summaries: "",
            subplot_board: "",
            emotional_arcs: "",
            character_matrix: "",
            volume_outline: "vol",
            observations: Some(""),
            selected_evidence_block: Some(""),
            governed_control_block: Some(""),
            validation_feedback: Some(""),
        });
        assert!(!out.contains("## 观察日志"));
        assert!(!out.contains("## 已选长程证据"));
        assert!(!out.contains("## 状态校验反馈"));
        // governedControlBlock 空串 → controlBlock.length === 0 → 卷纲照常输出。
        assert!(out.contains("## 卷纲"));
        // 空串 !== "(文件尚未创建)" → 各 truth 块照常输出（TS 语义）。
        assert!(out.contains("## 已有章节摘要\n\n"));
    }
}
