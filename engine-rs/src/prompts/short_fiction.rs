//! 短篇 web 小说 prompt 构造（short-fiction）。
//!
//! 移植自 `packages/core/src/prompts/short-fiction.ts`（568 行）。13 个 build 函数 + craft helper，
//! 中英双语模板字符串构造（system/user prompt、review、revision、continuation、package）。
//! 纯字符串构造，无 fs/LLM 依赖。

use crate::utils::language::WritingLanguage;

// --- 输入类型 ----------------------------------------------------------------

#[derive(Debug, Clone, Default)]
pub struct ShortFictionReferencePromptInput {
    pub text: Option<String>,
}

#[derive(Debug, Clone)]
pub struct ShortFictionOutlinePromptInput {
    pub direction: String,
    pub chapter_count: u32,
    pub chars_per_chapter: u32,
    pub reference: Option<ShortFictionReferencePromptInput>,
}

#[derive(Debug, Clone)]
pub struct ShortFictionOutlineReviewPromptInput {
    pub direction: String,
    pub outline_raw_content: String,
    pub reference: Option<ShortFictionReferencePromptInput>,
}

#[derive(Debug, Clone)]
pub struct ShortFictionOutlineRevisionPromptInput {
    pub direction: String,
    pub outline_raw_content: String,
    pub reference: Option<ShortFictionReferencePromptInput>,
    pub review: String,
    pub chapter_count: u32,
    pub chars_per_chapter: u32,
}

#[derive(Debug, Clone)]
pub struct ShortFictionDraftPromptInput {
    pub direction: String,
    pub outline_markdown: String,
    pub chapter_count: u32,
    pub chars_per_chapter: u32,
}

#[derive(Debug, Clone)]
pub struct ShortFictionDraftContinuationPromptInput {
    pub direction: String,
    pub outline_markdown: String,
    pub chapter_count: u32,
    pub chars_per_chapter: u32,
    pub existing_draft_markdown: String,
    pub missing_chapters: Vec<u32>,
}

#[derive(Debug, Clone)]
pub struct ShortFictionDraftReviewPromptInput {
    pub direction: String,
    pub outline_markdown: String,
    pub chapter_count: u32,
    pub chars_per_chapter: u32,
    pub draft_markdown: String,
}

#[derive(Debug, Clone)]
pub struct ShortFictionDraftRevisionPromptInput {
    pub direction: String,
    pub outline_markdown: String,
    pub chapter_count: u32,
    pub chars_per_chapter: u32,
    pub review: String,
}

#[derive(Debug, Clone)]
pub struct ShortFictionPackagePromptInput {
    pub direction: String,
    pub outline_markdown: String,
    pub draft_markdown: String,
    pub draft_title: String,
}

// --- 辅助：过滤空段后 join（对齐 TS .filter(Boolean).join("\n")）---------------

fn join_nonempty(parts: &[Option<String>]) -> String {
    parts
        .iter()
        .filter_map(|p| p.clone())
        .filter(|s| !s.is_empty())
        .collect::<Vec<_>>()
        .join("\n")
}

fn reference_block(ref_input: Option<&ShortFictionReferencePromptInput>, en: bool) -> Option<String> {
    let text = ref_input.and_then(|r| r.text.as_deref())?.trim();
    if text.is_empty() {
        return None;
    }
    Some(if en {
        format!("## Optional Reference Text\n{text}\n")
    } else {
        format!("## 可选参考文本\n{text}\n")
    })
}

/// 章节输出块：`=== CHAPTER N TITLE ===` + `=== CHAPTER N CONTENT ===`。
fn chapter_blocks(chapter: u32, en: bool) -> String {
    if en {
        format!(
            "=== CHAPTER {chapter} TITLE ===\nChapter title — plain text only, no #, no \"Chapter N\" prefix\n=== CHAPTER {chapter} CONTENT ===\nChapter {chapter} prose — full scenes, no synopsis, no author notes"
        )
    } else {
        format!(
            "=== CHAPTER {chapter} TITLE ===\n章节标题，只写纯文本，不要 #，不要第几章前缀\n=== CHAPTER {chapter} CONTENT ===\n第{chapter}章正文，写完整场面，不要梗概，不要作者备注"
        )
    }
}

// --- build 函数 --------------------------------------------------------------

pub fn build_outline_system_prompt(language: WritingLanguage) -> String {
    let en = language == WritingLanguage::En;
    if en {
        [
            "You are the managing editor for short web fiction. Your job is to turn one creative direction into a complete short-story plan.",
            "Work only from this direction and any reference text the user supplied; never claim to have read, quoted, or inherited material that was not provided.",
            "Content comes first: the title, the opening, the pressure on the protagonist, the evidence/relationship/identity leverage, the escalation chain, the reversal chain, and the payoff landing must be strong enough to carry a single-pass full draft.",
            "Do not over-structure and do not output JSON/YAML. Write human-readable Markdown, but the chapter plan must be dense enough that a writer can draft the whole story in one pass.",
            "A short defaults to 12-18 chapters at roughly 600-800 words per chapter. The story must be complete — not the first five chapters of a novel starter kit.",
        ].join("\n")
    } else {
        [
            "你是短篇小说总编，负责把一个创作方向做成完整短篇故事方案。",
            "只基于本次创作方向和用户提供的参考文本创作；没有提供的资料，不要声称读过、引用过或继承过。",
            "目标是内容优先：标题、开篇、人物压力、证据/关系/身份杠杆、升级链、反转链和回报落点必须能支撑一次写完整篇。",
            "不要过度结构化，不要输出 JSON/YAML。用人能读的 Markdown，但章节方案必须足够密，写手拿到后能直接一次写完。",
            "短篇默认 12-18 章，每章约 900-1200 字。故事要完整，不是长篇前 5 章启动包。",
        ].join("\n")
    }
}

pub fn build_outline_user_prompt(input: &ShortFictionOutlinePromptInput, language: WritingLanguage) -> String {
    let en = language == WritingLanguage::En;
    if en {
        join_nonempty(&[
            Some("## Creative Direction".into()),
            Some(input.direction.clone()),
            Some(String::new()),
            Some("## Target Spec".into()),
            Some(format!("A complete short story of {} chapters, about {} words per chapter.", input.chapter_count, input.chars_per_chapter)),
            Some(String::new()),
            reference_block(input.reference.as_ref(), true),
            Some("## Deliverable".into()),
            Some("Start with one platform-ready clickable title, then the full story plan. The plan must make clear why the protagonist is pinned down, what payoff the reader is waiting for, how the protagonist turns the tables, how evidence/relationships/identity/rules escalate step by step, why the antagonist strikes back, and how the ending lands.".into()),
            Some("The chapter plan must spell out, chapter by chapter: the direction of the chapter title, the key on-page scene, the characters' actions, the escalation or payoff, and the reason to keep reading at the chapter break.".into()),
            Some("Tags are allowed, but do not enumerate a tag table; tags serve premise selection and writing — they never replace the story.".into()),
            Some(String::new()),
            Some("## Output Format".into()),
            Some("=== SHORT_FICTION_PLAN_TITLE ===".into()),
            Some("Exactly one platform-ready title on a single line".into()),
            Some("=== SHORT_FICTION_PLAN ===".into()),
            Some("The full story plan in Markdown, covering: genre/audience, title direction, the opening hook, characters and relationships, the core pressure, how the protagonist wins, the escalation chain, the reversal chain, the ending payoff, and the chapter-by-chapter plan.".into()),
        ])
    } else {
        join_nonempty(&[
            Some("## 创作方向".into()),
            Some(input.direction.clone()),
            Some(String::new()),
            Some("## 目标规格".into()),
            Some(format!("完整短篇 {} 章，每章约 {} 字。", input.chapter_count, input.chars_per_chapter)),
            Some(String::new()),
            reference_block(input.reference.as_ref(), false),
            Some("## 产出要求".into()),
            Some("先给一个平台感标题，再给完整故事方案。大纲要讲清楚主角为什么被压住、读者想看什么回报、主角靠什么翻盘、证据/关系/身份/规则如何递进、反派为什么会反扑、结尾如何落地。".into()),
            Some("章节方案必须逐章写清：章节标题方向、当章发生的关键场面、角色动作、压力升级或回报、章尾继续读的理由。".into()),
            Some("可以给标签，但不要穷举标签表；标签服务选题和写作，不替代故事。".into()),
            Some(String::new()),
            Some("## 输出格式".into()),
            Some("=== SHORT_FICTION_PLAN_TITLE ===".into()),
            Some("只写一行平台感标题".into()),
            Some("=== SHORT_FICTION_PLAN ===".into()),
            Some("用 Markdown 写完整故事方案，包含：题材/受众、标题方向、开篇小钩子、人物与关系、核心压力、主角赢法、升级链、反转链、结尾回报、逐章方案。".into()),
        ])
    }
}

pub fn build_outline_review_system_prompt(language: WritingLanguage) -> String {
    let en = language == WritingLanguage::En;
    if en {
        [
            "You are a short-fiction outline reviewer. You do not assign scores and you do not police plagiarism.",
            "Your job is to judge whether this story plan can carry a single-pass full draft: is the genre engine clear, do character motivations hold, does the pressure chain escalate, is the antagonist's counterattack believable, is the ending payoff big enough.",
            "Review like a real reader and a real editor, not a checklist machine.",
            "Output Markdown. Name the flaws that would make the finished draft fall flat, and the strengths worth keeping.",
        ].join("\n")
    } else {
        [
            "你是短篇审纲编辑。你不负责打分，也不负责判抄。",
            "你的任务是判断这个故事方案能不能支撑一次写完整篇：题材发动机是否清楚、人物动机是否成立、压力链是否递进、反派反扑是否可信、结尾回报是否够。",
            "审稿要像真实读者和编辑，不要只列工程检查项。",
            "输出 Markdown，直接指出会导致成稿不好看的硬伤和可保留优点。",
        ].join("\n")
    }
}

pub fn build_outline_review_user_prompt(
    input: &ShortFictionOutlineReviewPromptInput,
    language: WritingLanguage,
) -> String {
    let en = language == WritingLanguage::En;
    if en {
        join_nonempty(&[
            Some("## Creative Direction".into()),
            Some(input.direction.clone()),
            Some(String::new()),
            reference_block(input.reference.as_ref(), true),
            Some("## Story Plan Under Review".into()),
            Some(input.outline_raw_content.clone()),
            Some(String::new()),
            Some("## Review Focus".into()),
            Some("- Is this a complete short story, rather than a partial tryout plan?\n- Do the title, the opening, and the first three chapters give readers a reason to click and keep reading?\n- Is the outline dense enough, or will the writer run out of material in the back half?\n- Do the key scenes contain character action, counterattack, and payoff, instead of bare result summaries?\n- Will readers be thrown out of the story by timeline, relationship, evidence-access, physical-state, or common-sense problems?".into()),
        ])
    } else {
        join_nonempty(&[
            Some("## 创作方向".into()),
            Some(input.direction.clone()),
            Some(String::new()),
            reference_block(input.reference.as_ref(), false),
            Some("## 待审故事方案".into()),
            Some(input.outline_raw_content.clone()),
            Some(String::new()),
            Some("## 审查重点".into()),
            Some("- 这是不是完整短篇故事，而不是局部试写方案。\n- 标题、开篇、前三章是否有点击和追读理由。\n- 大纲是否足够密，写手是否会在后半段泄气。\n- 关键场面有没有人物行动、反扑和回报，不是纯结果摘要。\n- 读者会不会因为时间线、人物关系、证据权限、身体状态、常识问题出戏。".into()),
        ])
    }
}

pub fn build_outline_revision_followup(
    input: &ShortFictionOutlineRevisionPromptInput,
    language: WritingLanguage,
) -> String {
    let en = language == WritingLanguage::En;
    if en {
        [
            "Based on the outline review above, produce the complete second version of the story plan.",
            "This is round two of the same project: do not start over from scratch, and do not output a list of edits instead of the plan.",
            &format!("Keep the structure at {} chapters of about {} words each.", input.chapter_count, input.chars_per_chapter),
            "Keep the genre engine and relationships that work; fix the flaws that would make the finished draft fall flat.",
            "",
            "## Outline Review",
            input.review.trim(),
            "",
            "## Output Format",
            "=== SHORT_FICTION_PLAN_TITLE ===",
            "Exactly one platform-ready title on a single line",
            "=== SHORT_FICTION_PLAN ===",
            "The complete second-version story plan in Markdown.",
        ].join("\n")
    } else {
        [
            "根据上面的审纲意见，继续给出第二版完整故事方案。",
            "这是同一次创作的第二轮，不要另起炉灶，不要只写修改说明。",
            &format!("仍然按 {} 章、每章约 {} 字来组织。", input.chapter_count, input.chars_per_chapter),
            "保留能打的题材发动机和人物关系，修掉会导致成稿不好看的硬伤。",
            "",
            "## 审纲意见",
            input.review.trim(),
            "",
            "## 输出格式",
            "=== SHORT_FICTION_PLAN_TITLE ===",
            "只写一行平台感标题",
            "=== SHORT_FICTION_PLAN ===",
            "用 Markdown 写完整第二版故事方案。",
        ].join("\n")
    }
}

pub fn build_writer_system_prompt(language: WritingLanguage) -> String {
    let en = language == WritingLanguage::En;
    if en {
        [
            "You are an English short-fiction BatchWriter. You write the complete short story in one API pass, following the story plan.",
            "Write natural, native English prose. Vary sentence length; mix short punchy sentences with longer flowing ones, and keep the narrative voice consistent throughout.",
            "This is not serialized-novel continuation and not chapter synopsis. Every chapter needs drama happening on the page: character action, dialogue or reaction, a shift in the situation, and a reason to keep reading at the chapter break.",
            "Keep the drama dialed up, web-fiction style: real-world pressure may be amplified as far as readers will still believe, but never so absurd that immersion breaks.",
            "The story title and chapter titles must read like platform content, not literary summaries. Keep the prose paced for mobile reading — short paragraphs, but never telegram-style fragments.",
            "The word count is a calibration, not an averaging exercise. Big scenes may run long and transitions short; a clearly short chapter usually means you wrote a synopsis and must add real scenes.",
            "Output must strictly use the specified blocks. No author notes, no word-count remarks, no review comments, no format explanations.",
        ].join("\n")
    } else {
        [
            "你是中文短篇 BatchWriter。你要根据故事方案一次 API 写完整短篇正文。",
            "这不是长篇连载续写，也不是章节梗概。每章都要有当场发生的戏：人物行动、对话或反应、局面变化、章尾继续读的理由。",
            "网文戏剧性要足：现实压力可以放大到读者愿意信的程度，但不能荒诞到失去代入。",
            "标题和章节标题要像平台内容，不要文艺化总结。正文保持移动端节奏，段落短但不要写成电报体。",
            "字数是校准，不是平均数学题。大场面可略长，过渡章可略短；明显偏短通常说明写成了梗概，必须补有效场面。",
            "输出必须严格使用指定 block，不要写作者说明、字数说明、审稿意见或格式解释。",
        ].join("\n")
    }
}

pub fn build_writer_user_prompt(input: &ShortFictionDraftPromptInput, language: WritingLanguage) -> String {
    let en = language == WritingLanguage::En;
    let craft = build_craft_prompt(language);
    let chapter_blocks_str: String = (1..=input.chapter_count)
        .map(|ch| chapter_blocks(ch, en))
        .collect::<Vec<_>>()
        .join("\n");
    if en {
        [
            "## Task",
            &format!("Write the complete {}-chapter story in one pass, about {} words per chapter.", input.chapter_count, input.chars_per_chapter),
            "Read the full story plan before writing. The prose must carry the plan's pressure chain, evidence chain, reversal chain, and emotional payoff — do not swerve into a different story midway.",
            "",
            &craft,
            "",
            "## Creative Direction",
            &input.direction,
            "",
            "## Story Plan",
            &input.outline_markdown,
            "",
            "## Output Format",
            "=== SHORT_FICTION_TITLE ===",
            "The story title — plain text, platform-ready, nothing else",
            "=== SHORT_FICTION_OPENING_HOOK ===",
            "An optional pre-story hook of about 130 words; if no standalone teaser is needed, still write the small first-screen scene that opens chapter 1",
            &chapter_blocks_str,
        ].join("\n")
    } else {
        [
            "## 任务",
            &format!("一次写完整 {} 章，每章约 {} 字。", input.chapter_count, input.chars_per_chapter),
            "先读完整故事方案，再写正文。正文要承接大纲的压力链、证据链、反转链和情绪回报，不要临时改成另一种故事。",
            "",
            &craft,
            "",
            "## 创作方向",
            &input.direction,
            "",
            "## 故事方案",
            &input.outline_markdown,
            "",
            "## 输出格式",
            "=== SHORT_FICTION_TITLE ===",
            "短篇标题，只写纯文本平台标题",
            "=== SHORT_FICTION_OPENING_HOOK ===",
            "可选正文前小钩子，约 200 字；如果不需要独立引子，也要写第 1 章第一屏的入局小场面",
            &chapter_blocks_str,
        ].join("\n")
    }
}

pub fn build_draft_continuation_user_prompt(
    input: &ShortFictionDraftContinuationPromptInput,
    language: WritingLanguage,
) -> String {
    let en = language == WritingLanguage::En;
    let craft = build_craft_prompt(language);
    let missing = input
        .missing_chapters
        .iter()
        .map(u32::to_string)
        .collect::<Vec<_>>()
        .join(", ");
    let chapter_blocks_str: String = input
        .missing_chapters
        .iter()
        .map(|&ch| chapter_blocks(ch, en))
        .collect::<Vec<_>>()
        .join("\n");
    if en {
        [
            "## Task",
            &format!("The previous draft was truncated or skipped chapters. Write ONLY the missing chapters: {missing}."),
            &format!("Stay calibrated to the complete {}-chapter short at about {} words per chapter.", input.chapter_count, input.chars_per_chapter),
            "Do not rewrite finished chapters, do not write summary notes, do not apologize, do not output review comments.",
            "",
            &craft,
            "",
            "## Creative Direction",
            &input.direction,
            "",
            "## Story Plan",
            &input.outline_markdown,
            "",
            "## Existing Draft (for continuity only — do not rewrite)",
            &input.existing_draft_markdown,
            "",
            "## Output Format",
            &chapter_blocks_str,
        ].join("\n")
    } else {
        [
            "## 任务",
            &format!("上一次正文被截断或漏章。现在只补写缺失章节：{missing}。"),
            &format!("仍然按完整短篇 {} 章、每章约 {} 字校准。", input.chapter_count, input.chars_per_chapter),
            "不要重写已完成章节，不要写总结说明，不要道歉，不要输出审稿意见。",
            "",
            &craft,
            "",
            "## 创作方向",
            &input.direction,
            "",
            "## 故事方案",
            &input.outline_markdown,
            "",
            "## 已有正文（只用于承接，不要重写）",
            &input.existing_draft_markdown,
            "",
            "## 输出格式",
            &chapter_blocks_str,
        ].join("\n")
    }
}

pub fn build_draft_review_system_prompt(language: WritingLanguage) -> String {
    let en = language == WritingLanguage::En;
    if en {
        [
            "You are a short-fiction draft reviewer.",
            "You judge only whether the content can sell, reads smoothly, and keeps pulling the reader forward; do not turn the review into deterministic scoring.",
            "Focus on: the title, chapter titles, the opening, character motivation, the timeline, relationships, evidence and access, escalating pressure, the antagonist's counterattack, whether the back half sags, and whether the ending payoff lands.",
            "Output Markdown. Separate the problems that would visibly stop readers from reading on from the small blemishes that are acceptable.",
        ].join("\n")
    } else {
        [
            "你是短篇成稿审稿编辑。",
            "你只看内容是否能卖、是否顺、是否有继续读的欲望；不要把审稿变成确定性打分。",
            "重点看标题、章节标题、开篇、人物动机、时间线、人物关系、证据/权限、压力递进、反派反扑、后半段是否泄气、结尾回报是否落地。",
            "输出 Markdown，写清哪些问题会明显影响读者读下去，哪些只是可接受的小瑕疵。",
        ].join("\n")
    }
}

pub fn build_draft_review_user_prompt(
    input: &ShortFictionDraftReviewPromptInput,
    language: WritingLanguage,
) -> String {
    let en = language == WritingLanguage::En;
    if en {
        [
            "## Creative Direction",
            &input.direction,
            "",
            "## Original Story Plan",
            &input.outline_markdown,
            "",
            "## Draft Under Review",
            &input.draft_markdown,
            "",
            "## Review Instructions",
            "Talk like a person: where does this story pull, where does it break immersion, where does it read like a synopsis, where does the back half sag, which title or chapter titles would nobody tap?",
            "Never condemn a chapter just for running slightly short or long; judge first whether the content is complete, dramatic, and paying off.",
        ].join("\n")
    } else {
        [
            "## 创作方向",
            &input.direction,
            "",
            "## 原故事方案",
            &input.outline_markdown,
            "",
            "## 待审正文",
            &input.draft_markdown,
            "",
            "## 审稿要求",
            "直接说人话：这本读起来哪里有欲望、哪里出戏、哪里像梗概、哪里后半段泄气、哪里标题或章节标题不想点。",
            "不要因为某章略短或略长就判死；先判断内容是否完整、有戏、有回报。",
        ].join("\n")
    }
}

pub fn build_draft_revision_followup(
    input: &ShortFictionDraftRevisionPromptInput,
    language: WritingLanguage,
) -> String {
    let en = language == WritingLanguage::En;
    let chapter_blocks_str: String = (1..=input.chapter_count)
        .map(|ch| chapter_blocks(ch, en))
        .collect::<Vec<_>>()
        .join("\n");
    if en {
        [
            "Based on the review notes, write the complete second-version draft.",
            "This is round two of the same story: keep what worked in the last version, fix what breaks immersion or kills the desire to keep reading.",
            "Do not output a list of suggested edits, and do not patch just a few chapters — output the complete draft.",
            "",
            "## Review Notes",
            input.review.trim(),
            "",
            "## Round-Two Priorities",
            "- Fix the immersion-breaking problems: timeline, logic, relationships, evidence access, physical state.",
            "- Add real scenes to the back half; never close on result summaries.",
            "- Keep the title, opening, chapter titles, and main title consistent with the prose, though the title may be re-sharpened from the final draft for platform click appeal.",
            "- Word count is calibration only: pad short chapters with real scenes; trim long ones by cutting explanation and repeated reactions.",
            "",
            "## Output Format",
            "=== SHORT_FICTION_TITLE ===",
            "The story title — plain text, platform-ready, nothing else",
            "=== SHORT_FICTION_OPENING_HOOK ===",
            "An optional pre-story hook of about 130 words; if no standalone teaser is needed, still write the small first-screen scene that opens chapter 1",
            &chapter_blocks_str,
        ].join("\n")
    } else {
        [
            "根据审稿意见，继续写第二版完整正文。",
            "这是同一篇的第二轮写作：保留上一版能打的地方，修掉会让读者出戏或不想读的问题。",
            "不要只列修改建议，不要只改几章片段，输出完整正文。",
            "",
            "## 审稿意见",
            input.review.trim(),
            "",
            "## 第二轮重点",
            "- 修时间线、逻辑、人物关系、证据权限、身体状态等会让读者出戏的问题。",
            "- 补后半段有效场面，不要用结果摘要收尾。",
            "- 保持标题、开篇、章节标题和正文主标题一致，但标题可以基于正文重新压得更有平台点击感。",
            "- 字数只做校准：偏短补有效场面，偏长删解释和重复反应。",
            "",
            "## 输出格式",
            "=== SHORT_FICTION_TITLE ===",
            "短篇标题，只写纯文本平台标题",
            "=== SHORT_FICTION_OPENING_HOOK ===",
            "可选正文前小钩子，约 200 字；如果不需要独立引子，也要写第 1 章第一屏的入局小场面",
            &chapter_blocks_str,
        ].join("\n")
    }
}

pub fn build_package_system_prompt(language: WritingLanguage) -> String {
    let en = language == WritingLanguage::En;
    if en {
        [
            "You are a short-fiction packaging editor. From the final draft you produce the synopsis, the selling points, and the cover-image prompt.",
            "Never invent a main title different from the draft's. All packaging must revolve around the draft's actual title and plot.",
            "Think of the cover prompt as a mobile portrait book cover: 3:4 vertical, a large title zone, strong character emotion, one or two instantly recognizable props, high-contrast colors — not a movie poster.",
        ].join("\n")
    } else {
        [
            "你是短篇小说包装编辑，负责根据最终正文生成简介、卖点和封面提示词。",
            "不要另起一个和正文不同的主标题。包装必须围绕正文实际标题和剧情。",
            "封面提示词按手机端竖版书封思考：3:4 竖图、大标题区、强人物情绪、少量一眼可识别道具、高对比色彩，不要影视海报感。",
        ].join("\n")
    }
}

pub fn build_package_user_prompt(input: &ShortFictionPackagePromptInput, language: WritingLanguage) -> String {
    let en = language == WritingLanguage::En;
    if en {
        [
            "## Creative Direction",
            &input.direction,
            "",
            "## Story Plan",
            input.outline_markdown.trim(),
            "",
            "## Final Draft",
            input.draft_markdown.trim(),
            "",
            "## Output Format",
            "=== SHORT_FICTION_PACKAGE_TITLE ===",
            &input.draft_title,
            "=== SHORT_FICTION_INTRO ===",
            "A 70-120 word platform synopsis that grabs the conflict, the pressure, and the payoff — never a spoiler-filled play-by-play.",
            "=== SHORT_FICTION_SELLING_POINTS ===",
            "- 3 to 6 selling points, one per line",
            "=== SHORT_FICTION_COVER_PROMPT ===",
            "An English cover-generation prompt: 3:4 portrait, main title zone, character emotion, props, color palette, typography style, and what to avoid.",
        ].join("\n")
    } else {
        [
            "## 创作方向",
            &input.direction,
            "",
            "## 故事方案",
            input.outline_markdown.trim(),
            "",
            "## 最终正文",
            input.draft_markdown.trim(),
            "",
            "## 输出格式",
            "=== SHORT_FICTION_PACKAGE_TITLE ===",
            &input.draft_title,
            "=== SHORT_FICTION_INTRO ===",
            "100-180字平台简介，直接抓冲突、压迫和回报，不要剧透成流水账。",
            "=== SHORT_FICTION_SELLING_POINTS ===",
            "- 3到6条卖点，每条一行",
            "=== SHORT_FICTION_COVER_PROMPT ===",
            "中文封面生成提示词：3:4竖图，主标题区，人物情绪，道具，配色，字体风格，避免事项。",
        ].join("\n")
    }
}

/// craft 提醒段（writer/continuation 内嵌）。原 TS `buildShortFictionCraftPrompt`。
fn build_craft_prompt(language: WritingLanguage) -> String {
    let en = language == WritingLanguage::En;
    if en {
        [
            "## Craft Reminders",
            "- Salt dissolves in the soup: values and ambition show through action, never through slogans.",
            "- Show, don't tell: let behavior, evidence, concrete detail, and staging make the reader feel a character's state.",
            "- Simile restraint: do not lean on \"like / as if / as though\" as default rhetoric — at most one simile per scene; prefer a precise verb and a concrete action over a figure of speech.",
            "- Anti-AI wording: ration AI-tell words (delve, tapestry, testament, intricate, pivotal); do not use the \"It wasn't X; it was Y\" construction as a crutch; keep analytical report language (\"core motivation\", \"strategic advantage\") out of the prose.",
            "- No padding: every scene must advance conflict, causality, emotion, evidence, pressure, payoff, or a relationship.",
            "- The climax is a scene, not a recap: eruptions of conflict, reversals, life-or-death beats, and reveals must play out beat by beat on the page (action, dialogue, the five senses). The heavier a chapter's information load, the more its key beat must be staged as a full scene — never compressed into one line like \"then he saved her and the rival was arrested.\"",
            "- Payoffs need setup: every reversal, comeuppance, reconciliation, revenge, or identity reveal must ride a chain of evidence and causality.",
            "- Side characters need motives: even the oppressor acts from interest, misjudgment, or fear — never a brainless plot device.",
            "- Everyday detail must become bait: each detail carries evidence, emotion, characterization, or a later reversal.",
            "- Mobile-first: short paragraphs, dense information, no vague lyricism or decorative filler.",
        ].join("\n")
    } else {
        [
            "## 写法提醒",
            "- 盐溶于汤：人物价值观和野心靠行动表现，不靠口号。",
            "- Show don't tell：用行为、证据、细节和场景让读者自己感到人物状态。",
            "- 明喻节制：别把「像/仿佛/如同」当默认修辞反复用，每个场景最多 1 处；优先用准确的动词和具体动作，而不是比喻。",
            "- 反注水：每个场景都推动冲突、因果、情绪、证据、压迫、回报或关系。",
            "- 高潮即场景，不是概述：冲突爆发、反转、生死、揭露这些关键拍必须当场一拍一拍演出（动作、对话、五感）。本章信息量越大越要把最关键那拍写成完整场面，绝不能用「然后他救了人、对手落网」这种一句话带过。",
            "- 回报要有铺垫：反转、打脸、和解、复仇、身份揭露都要有证据链和因果链。",
            "- 配角要有动机：压迫者也有利益、误判或恐惧，不要写成无脑工具人。",
            "- 日常细节要变成饵：细节承担证据、情绪、人物差异或后续反转功能。",
            "- 移动端优先：段落短，信息密，少写空泛抒情和装饰性废话。",
        ].join("\n")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn lang(en: bool) -> WritingLanguage {
        if en { WritingLanguage::En } else { WritingLanguage::Zh }
    }

    #[test]
    fn outline_system_prompt_has_language_specific_defaults() {
        let zh = build_outline_system_prompt(lang(false));
        let en = build_outline_system_prompt(lang(true));
        assert!(zh.contains("12-18 章"));
        assert!(en.contains("12-18 chapters"));
    }

    #[test]
    fn outline_user_prompt_includes_direction_and_spec() {
        let input = ShortFictionOutlinePromptInput {
            direction: "都市悬疑".into(),
            chapter_count: 15,
            chars_per_chapter: 1000,
            reference: None,
        };
        let zh = build_outline_user_prompt(&input, lang(false));
        assert!(zh.contains("都市悬疑"));
        assert!(zh.contains("完整短篇 15 章，每章约 1000 字"));
        assert!(zh.contains("=== SHORT_FICTION_PLAN_TITLE ==="));
    }

    #[test]
    fn outline_user_prompt_includes_reference_block_when_present() {
        let input = ShortFictionOutlinePromptInput {
            direction: "x".into(),
            chapter_count: 12,
            chars_per_chapter: 800,
            reference: Some(ShortFictionReferencePromptInput { text: Some("  ref text  ".into()) }),
        };
        let zh = build_outline_user_prompt(&input, lang(false));
        assert!(zh.contains("## 可选参考文本\nref text"));
        let en = build_outline_user_prompt(&input, lang(true));
        assert!(en.contains("## Optional Reference Text\nref text"));
    }

    #[test]
    fn outline_user_prompt_omits_reference_block_when_empty() {
        let input = ShortFictionOutlinePromptInput {
            direction: "x".into(),
            chapter_count: 12,
            chars_per_chapter: 800,
            reference: Some(ShortFictionReferencePromptInput { text: Some("   ".into()) }),
        };
        let zh = build_outline_user_prompt(&input, lang(false));
        assert!(!zh.contains("可选参考文本"));
    }

    #[test]
    fn writer_user_prompt_emits_chapter_blocks_per_count() {
        let input = ShortFictionDraftPromptInput {
            direction: "d".into(),
            outline_markdown: "plan".into(),
            chapter_count: 3,
            chars_per_chapter: 900,
        };
        let zh = build_writer_user_prompt(&input, lang(false));
        assert!(zh.contains("=== CHAPTER 1 TITLE ==="));
        assert!(zh.contains("=== CHAPTER 3 CONTENT ==="));
        assert!(!zh.contains("=== CHAPTER 4"));
        assert!(zh.contains("## 写法提醒"), "craft 段应内嵌");
    }

    #[test]
    fn draft_continuation_lists_only_missing_chapters() {
        let input = ShortFictionDraftContinuationPromptInput {
            direction: "d".into(),
            outline_markdown: "plan".into(),
            chapter_count: 5,
            chars_per_chapter: 900,
            existing_draft_markdown: "existing".into(),
            missing_chapters: vec![2, 4],
        };
        let en = build_draft_continuation_user_prompt(&input, lang(true));
        assert!(en.contains("missing chapters: 2, 4"));
        assert!(en.contains("=== CHAPTER 2 TITLE ==="));
        assert!(en.contains("=== CHAPTER 4 CONTENT ==="));
        assert!(!en.contains("=== CHAPTER 1 TITLE ==="));
    }

    #[test]
    fn draft_revision_followup_includes_review_and_chapter_blocks() {
        let input = ShortFictionDraftRevisionPromptInput {
            direction: "d".into(),
            outline_markdown: "plan".into(),
            chapter_count: 2,
            chars_per_chapter: 900,
            review: "  太平淡  ".into(),
        };
        let zh = build_draft_revision_followup(&input, lang(false));
        assert!(zh.contains("## 审稿意见\n太平淡"), "review 应 trim");
        assert!(zh.contains("=== CHAPTER 2 CONTENT ==="));
    }

    #[test]
    fn package_user_prompt_uses_draft_title() {
        let input = ShortFictionPackagePromptInput {
            direction: "d".into(),
            outline_markdown: "  plan  ".into(),
            draft_markdown: "  draft  ".into(),
            draft_title: "我的标题".into(),
        };
        let zh = build_package_user_prompt(&input, lang(false));
        assert!(zh.contains("=== SHORT_FICTION_PACKAGE_TITLE ===\n我的标题"));
        assert!(zh.contains("plan"), "outline 应 trim");
        assert!(zh.contains("draft"));
    }

    #[test]
    fn craft_prompt_distinct_per_language() {
        let zh = build_craft_prompt(lang(false));
        let en = build_craft_prompt(lang(true));
        assert!(zh.contains("盐溶于汤"));
        assert!(en.contains("Salt dissolves in the soup"));
    }

    #[test]
    fn review_system_prompts_non_empty_per_language() {
        assert!(!build_outline_review_system_prompt(lang(false)).is_empty());
        assert!(!build_outline_review_system_prompt(lang(true)).is_empty());
        assert!(!build_draft_review_system_prompt(lang(false)).is_empty());
        assert!(!build_package_system_prompt(lang(true)).is_empty());
    }
}
