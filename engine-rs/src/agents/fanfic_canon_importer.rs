//! 同人正典导入器（fanfic-canon-importer，59 号）。
//!
//! 移植自 `packages/core/src/agents/fanfic-canon-importer.ts`（201 行）：
//! 源文本（>50k UTF-16 码元先分块编译为语义资料包）→ LLM 五段 SECTION 提取
//! → `fanfic_canon.md` 全文档拼装（世界规则/角色档案/关键事件/力量体系/写作
//! 风格 + meta 块）。

use crate::agents::continuity::ChatOutcome;
use crate::llm::provider::{LLMMessage, LLMRole};
use crate::models::book::FanficMode;
use crate::utils::truth_dialect::{render_flat_meta_block, strip_utf8_bom};
use crate::utils::utc_time::utc_now_iso;

const SOURCE_CHUNK_CHARS: usize = 50_000;

/// 导入器 chat 端口。
#[async_trait::async_trait]
pub trait FanficCanonImporterChat: Send + Sync {
    async fn chat(&self, messages: Vec<LLMMessage>, temperature: f64) -> Result<ChatOutcome, String>;
}

/// 导入产出。对齐 TS `FanficCanonOutput`。
#[derive(Debug, Clone, Default, PartialEq)]
pub struct FanficCanonOutput {
    pub world_rules: String,
    pub character_profiles: String,
    pub key_events: String,
    pub power_system: String,
    pub writing_style: String,
    pub full_document: String,
}

fn mode_label(mode: FanficMode) -> &'static str {
    match mode {
        FanficMode::Canon => "原作向（严格遵守原作设定）",
        FanficMode::Au => "AU/平行世界（世界规则可改，角色保留）",
        FanficMode::Ooc => "OOC（角色性格可偏离原作）",
        FanficMode::Cp => "CP（以配对关系为核心）",
    }
}

/// 主入口（temp 0.3）。对齐 TS `importFromText`。
pub async fn import_from_text(
    chat: &dyn FanficCanonImporterChat,
    source_text: &str,
    source_name: &str,
    fanfic_mode: FanficMode,
) -> Result<FanficCanonOutput, String> {
    let source = prepare_source_text(chat, source_text, source_name).await?;
    let mode = fanfic_mode_str(fanfic_mode);

    let system_prompt = format!(
        "你是一个专业的同人创作素材分析师。你的任务是从用户提供的原作素材中提取结构化正典信息，供同人写作系统使用。\n\n同人模式：{}\n\n你需要从原作素材中提取以下内容，每个部分用 === SECTION: <name> === 分隔：\n\n=== SECTION: world_rules ===\n世界规则（地理、物理法则、魔法/力量体系、阵营组织、社会结构）。\n如果原作素材不包含明确的世界规则，从已有信息合理推断。\n\n=== SECTION: character_profiles ===\n角色档案表格，每个重要角色一行：\n\n| 角色 | 身份 | 性格底色 | 语癖/口头禅 | 说话风格 | 行为模式 | 关键关系 | 信息边界 |\n|------|------|----------|-------------|----------|----------|----------|----------|\n\n要求：\n- 语癖/口头禅必须从原文中精确提取，如有的话\n- 说话风格描述该角色的语气、用词偏好、句式特征\n- 行为模式描述该角色在特定情境下的典型反应\n- 信息边界标注该角色知道什么、不知道什么\n- 至少提取 3 个角色，不超过 15 个\n\n=== SECTION: key_events ===\n关键事件时间线：\n\n| 序号 | 事件 | 涉及角色 | 对同人写作的约束 |\n|------|------|----------|------------------|\n\n按时间/出现顺序排列，标注每个事件对同人创作的约束程度。\n\n=== SECTION: power_system ===\n力量/能力体系（如果适用）。包括等级划分、核心规则、已知限制。\n如果原作没有明确的力量体系，输出\"（原作无明确力量体系）\"。\n\n=== SECTION: writing_style ===\n原作写作风格特征（供同人写作模仿）：\n\n1. 叙事人称与视角（第一人称/第三人称有限/全知，是否频繁切换）\n2. 句式节奏（长短句交替模式、段落平均长度感受、对话占比）\n3. 场景描写手法（五感偏好、意象选择、环境描写密度）\n4. 对话标记习惯（说/道/笑道 等用法，对话前后是否有动作/表情补充）\n5. 情绪表达方式（直白内心独白 vs 动作外化 vs 环境映射）\n6. 比喻/修辞倾向（常用比喻类型、修辞频率）\n7. 节奏转换（紧张→舒缓的过渡方式、章节结尾习惯）\n\n每项用1-2个原文例句佐证。只提取原文实际存在的特征，不要泛泛描述。\n\n提取原则：\n- 忠实于原作素材，不捏造原作中没有的信息\n- 信息不足时标注\"（素材未提及）\"而非编造\n- 角色语癖是最重要的字段——同人读者最在意角色\"像不像\"\n- 写作风格提取必须基于实际文本特征，附原文例句{}",
        mode_label(fanfic_mode),
        if source.compiled {
            "\n注意：原作素材较长。下面输入是逐段读取完整素材后生成的语义资料包，不是截断文本；请以资料包中的片段编号和证据为准。"
        } else {
            ""
        }
    );
    let response = chat
        .chat(
            vec![
                LLMMessage { role: LLMRole::System, content: system_prompt, tool_calls: None, tool_call_id: None },
                LLMMessage {
                    role: LLMRole::User,
                    content: format!("以下是原作《{source_name}》的素材：\n\n{}", source.text),
                    tool_calls: None, tool_call_id: None,
                },
            ],
            0.3,
        )
        .await?;
    // G7c/332 号：正文剥前导 BOM 后再进 SECTION 提取。
    let content = strip_utf8_bom(&response.content);

    let extract = |tag: &str| -> String {
        section_extract_re(tag)
            .captures(&content)
            .map(|caps| caps[1].trim().to_string())
            .unwrap_or_default()
    };
    let world_rules = extract("world_rules");
    let character_profiles = extract("character_profiles");
    let key_events = extract("key_events");
    let power_system = extract("power_system");
    let writing_style = extract("writing_style");

    // G7c/332 号防呆方言：平铺 meta 块 + 危险值转义加引号。
    let meta = render_flat_meta_block(&[
        ("sourceFile", &source_name),
        ("fanficMode", mode),
        ("generatedAt", &utc_now_iso()),
    ]);

    let full_document = [
        format!("# 同人正典（《{source_name}》）"),
        String::new(),
        "## 世界规则".to_string(),
        if world_rules.is_empty() { "（素材中未提取到明确世界规则）".to_string() } else { world_rules.clone() },
        String::new(),
        "## 角色档案".to_string(),
        if character_profiles.is_empty() { "（素材中未提取到角色信息）".to_string() } else { character_profiles.clone() },
        String::new(),
        "## 关键事件时间线".to_string(),
        if key_events.is_empty() { "（素材中未提取到关键事件）".to_string() } else { key_events.clone() },
        String::new(),
        "## 力量体系".to_string(),
        if power_system.is_empty() { "（原作无明确力量体系）".to_string() } else { power_system.clone() },
        String::new(),
        "## 原作写作风格".to_string(),
        if writing_style.is_empty() { "（素材不足以提取风格特征）".to_string() } else { writing_style.clone() },
        String::new(),
        meta,
    ]
    .join("\n");

    Ok(FanficCanonOutput {
        world_rules,
        character_profiles,
        key_events,
        power_system,
        writing_style,
        full_document,
    })
}

fn section_extract_re(tag: &str) -> regex::Regex {
    // TS: `=== SECTION: {tag} ===\s*([\s\S]*?)(?==== SECTION:|$)`
    // ——lookahead 改消费式（消费终止符不影响各段独立提取）。
    regex::Regex::new(&format!(
        r"=== SECTION: {tag} ===\s*([\s\S]*?)\s*(?:=== SECTION:|$)"
    ))
    .expect("fanfic section regex")
}

fn fanfic_mode_str(mode: FanficMode) -> &'static str {
    match mode {
        FanficMode::Canon => "canon",
        FanficMode::Au => "au",
        FanficMode::Ooc => "ooc",
        FanficMode::Cp => "cp",
    }
}

struct PreparedSource {
    text: String,
    compiled: bool,
}

/// ≤50k 原样；超长先逐段编译（temp 0.2）为语义资料包。
async fn prepare_source_text(
    chat: &dyn FanficCanonImporterChat,
    source_text: &str,
    source_name: &str,
) -> Result<PreparedSource, String> {
    if source_text.encode_utf16().count() <= SOURCE_CHUNK_CHARS {
        return Ok(PreparedSource { text: source_text.to_string(), compiled: false });
    }
    let chunks = split_into_chunks(source_text, SOURCE_CHUNK_CHARS);
    let total = chunks.len();
    let mut notes: Vec<String> = Vec::new();
    for (index, chunk) in chunks.iter().enumerate() {
        let response = chat
            .chat(
                vec![
                    LLMMessage {
                        role: LLMRole::System,
                        content: [
                            "你是同人正典资料编译器。任务是把一个原作片段压成后续抽取可用的 Markdown 资料包。",
                            "不要续写、不要创作、不要补不存在的信息。只保留片段里实际出现的世界规则、人物、关系、关键事件、能力体系、口头禅、说话风格和原文证据。",
                            "如果片段没有某类信息，直接省略该类。保留片段编号，方便后续追溯。",
                        ]
                        .join("\n"),
                        tool_calls: None, tool_call_id: None,
                    },
                    LLMMessage {
                        role: LLMRole::User,
                        content: format!("原作：《{source_name}》\n片段：{}/{}\n\n{chunk}", index + 1, total),
                        tool_calls: None, tool_call_id: None,
                    },
                ],
                0.2,
            )
            .await?;
        let content = response.content.trim();
        if !content.is_empty() {
            notes.push(format!("## 片段 {}/{}\n\n{content}", index + 1, total));
        }
    }
    let mut text = format!(
        "# 《{source_name}》语义资料包\n\n以下内容由 InkOS 逐段读取完整原作素材后压缩生成，用于后续正典抽取。它不是原文截断。\n"
    );
    for note in notes {
        text.push('\n');
        text.push_str(&note);
    }
    Ok(PreparedSource { text, compiled: true })
}

/// UTF-16 码元分块（TS slice 语义）。
fn split_into_chunks(text: &str, chunk_chars: usize) -> Vec<String> {
    let units: Vec<u16> = text.encode_utf16().collect();
    units
        .chunks(chunk_chars)
        .map(String::from_utf16_lossy)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn full_document_assembly_with_missing_sections() {
        let content = "\
=== SECTION: world_rules ===
斗气大陆，等级森严。

=== SECTION: character_profiles ===
| 角色 | 身份 |
|---|---|
| 林动 | 主角 |";
        let out = test_parse(content);
        assert_eq!(out.world_rules, "斗气大陆，等级森严。");
        assert!(out.character_profiles.contains("林动"));
        // 缺失段为空串（full_document 拼装时填充占位文案 + meta 块）。
        assert!(out.key_events.is_empty());
        assert!(out.power_system.is_empty());
    }

    fn test_parse(content: &str) -> FanficCanonOutput {
        let extract = |tag: &str| -> String {
            section_extract_re(tag)
                .captures(content)
                .map(|caps| caps[1].trim().to_string())
                .unwrap_or_default()
        };
        FanficCanonOutput {
            world_rules: extract("world_rules"),
            character_profiles: extract("character_profiles"),
            key_events: extract("key_events"),
            power_system: extract("power_system"),
            writing_style: extract("writing_style"),
            full_document: String::new(),
        }
    }

    #[test]
    fn chunks_split_by_utf16_units() {
        let text = "少年".repeat(60);
        let chunks = split_into_chunks(&text, 100);
        assert_eq!(chunks.len(), 2);
        assert!(chunks[0].chars().count() == 100);
    }
}
