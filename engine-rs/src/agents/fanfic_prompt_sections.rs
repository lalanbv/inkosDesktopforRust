//! 同人 prompt 段（fanfic-prompt-sections）。
//!
//! 移植自 `packages/core/src/agents/fanfic-prompt-sections.ts`（110 行，纯逻辑）。
//! [`super::writer_prompts::build_writer_system_prompt`] 的同人三段构造依赖。

use std::sync::OnceLock;

use regex::Regex;

use crate::models::book::FanficMode;

/// 模式前言。逐字移植 TS `MODE_PREAMBLES`。
fn mode_preamble(mode: FanficMode) -> &'static str {
    match mode {
        FanficMode::Canon => {
            "你正在写**原作向同人**。严格遵守正典：\n- 角色的语癖、说话风格、行为模式必须与原作一致\n- 世界规则不可违反\n- 关键事件时间线不可矛盾\n- 可以填充原作空白、探索未详述的角度"
        }
        FanficMode::Au => {
            "你正在写**AU（平行世界）同人**：\n- 世界规则可以改变（已在 allowedDeviations 中声明的偏离）\n- 角色的核心性格和说话方式应保持辨识度——读者要能认出是谁\n- AU 设定偏离必须内部一致（改了一条规则，相关的都要跟着变）"
        }
        FanficMode::Ooc => {
            "你正在写**OOC 同人**：\n- 角色在极端情境下可以偏离性格底色\n- 但偏离必须有情境驱动，不能无缘无故变性格\n- 保留角色的语癖和说话特征——即使性格变了，说话方式也应有辨识度"
        }
        FanficMode::Cp => {
            "你正在写**CP 同人**，以角色互动和关系发展为核心：\n- 配对双方每章必须有有效互动\n- 互动风格要有化学反应——不是两个人在同一个场景各干各的\n- 关系发展应有节奏感：推进、试探、阻碍、突破"
        }
    }
}

/// 同人正典参照段。对齐 TS `buildFanficCanonSection`。
pub fn build_fanfic_canon_section(fanfic_canon: &str, mode: FanficMode) -> String {
    format!(
        "\n## 同人正典参照\n\n{}\n\n以下是原作正典信息，写作时必须参照：\n\n{fanfic_canon}",
        mode_preamble(mode)
    )
}

/// 从 fanfic_canon.md 提取角色表格生成语音参照段。对齐 TS `buildCharacterVoiceProfiles`。
///
/// 提取 `## 角色档案` 节下的 markdown 表格（表头 + 分隔行 + 数据行），
/// 每行取 [0]=姓名、[3]=口头禅、[4]=说话风格、[5]=典型行为；「（素材未提及）」跳过。
pub fn build_character_voice_profiles(fanfic_canon: &str) -> String {
    static TABLE: OnceLock<Regex> = OnceLock::new();
    let table = TABLE.get_or_init(|| {
        Regex::new(r"## 角色档案[\s\S]*?\n(\|[^\n]+\|\n\|[-|\s]+\|\n(?:\|[^\n]+\|\n)*)").unwrap()
    });

    let Some(m) = table.captures(fanfic_canon) else {
        return String::new();
    };
    let table_text = m.get(1).expect("组 1 必在").as_str();

    let rows: Vec<Vec<&str>> = table_text
        .split('\n')
        .filter(|line| {
            line.starts_with('|') && !line.starts_with("|--") && !line.starts_with("| 角色")
        })
        .map(|line| {
            line.split('|')
                .map(|cell| cell.trim())
                .filter(|cell| !cell.is_empty())
                .collect::<Vec<&str>>()
        })
        .filter(|cells| cells.len() >= 5)
        .collect();

    if rows.is_empty() {
        return String::new();
    }

    let profiles: Vec<String> = rows
        .iter()
        .map(|cells| {
            let name = cells[0];
            let catchphrases = cells.get(3).copied().unwrap_or("");
            let speaking_style = cells.get(4).copied().unwrap_or("");
            let behavior = cells.get(5).copied().unwrap_or("");
            let mut parts = vec![format!("### {name}")];
            if !catchphrases.is_empty() && catchphrases != "（素材未提及）" {
                parts.push(format!("- 口头禅/语癖：{catchphrases}"));
            }
            if !speaking_style.is_empty() && speaking_style != "（素材未提及）" {
                parts.push(format!("- 说话风格：{speaking_style}"));
            }
            if !behavior.is_empty() && behavior != "（素材未提及）" {
                parts.push(format!("- 典型行为：{behavior}"));
            }
            parts.join("\n")
        })
        .collect();

    format!(
        "\n## 角色语音参照（同人写作专用）\n\n以下角色的对话和行为必须参照原作特征。写对话时，先想\"这个角色在原作里会怎么说\"。\n\n{}",
        profiles.join("\n\n")
    )
}

/// 模式自检清单。逐字移植 TS `MODE_CHECKS`。
fn mode_checks(mode: FanficMode) -> &'static str {
    match mode {
        FanficMode::Canon => {
            "- 正典合规检查：本章是否违反原作设定？角色对话是否符合原作语癖？\n- 信息边界检查：角色是否引用了不该知道的信息？"
        }
        FanficMode::Au => {
            "- AU 偏离清单：本章改变了哪些世界规则？改变是否内部一致？\n- 角色辨识度检查：读者能否从对话中认出角色？"
        }
        FanficMode::Ooc => {
            "- OOC 偏离记录：角色在哪些方面偏离了性格底色？偏离驱动力是什么？\n- 语癖保留检查：即使 OOC，说话方式是否还有原作特征？"
        }
        FanficMode::Cp => {
            "- CP 互动检查：配对双方本章是否有有效互动？关系发展是否推进？\n- 互动质量检查：互动是否有化学反应（不是各干各的）？"
        }
    }
}

/// 同人写作自检段。对齐 TS `buildFanficModeInstructions`。
pub fn build_fanfic_mode_instructions(
    mode: FanficMode,
    allowed_deviations: &[String],
) -> String {
    let deviations_block = if !allowed_deviations.is_empty() {
        let lines: Vec<String> = allowed_deviations
            .iter()
            .map(|d| format!("- {d}"))
            .collect();
        format!("\n允许的偏离（不视为违规）：\n{}\n", lines.join("\n"))
    } else {
        String::new()
    };

    format!(
        "\n## 同人写作自检（在 PRE_WRITE_CHECK 中额外检查）\n\n{}{deviations_block}",
        mode_checks(mode)
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn canon_section_embeds_preamble_and_canon() {
        let out = build_fanfic_canon_section("原作设定 A", FanficMode::Canon);
        assert!(out.starts_with("\n## 同人正典参照\n\n你正在写**原作向同人**。严格遵守正典："));
        assert!(out.ends_with("以下是原作正典信息，写作时必须参照：\n\n原作设定 A"));
    }

    #[test]
    fn voice_profiles_extracts_character_table() {
        let canon = "## 世界规则\n...\n\n## 角色档案\n\n| 姓名 | 别名 | 定位 | 口头禅 | 说话风格 | 典型行为 | 备注 |\n|---|---|---|---|---|---|---|\n| 楚晚宁 | 晚宁 | 主角 | 蠢货 | 冷硬短句 | 抚琴 | aaa |\n| 谢怜 |  | 主角 | （素材未提及） | 温和 | （素材未提及） | bbb |\n";
        let out = build_character_voice_profiles(canon);
        // 表格 6 数据列（姓名..典型行为），两行都 ≥5 cells。
        assert!(out.contains("### 楚晚宁"));
        assert!(out.contains("- 口头禅/语癖：蠢货"));
        assert!(out.contains("- 说话风格：冷硬短句"));
        assert!(out.contains("- 典型行为：抚琴"));
        // 「（素材未提及）」条目跳过，但姓名段保留。
        assert!(out.contains("### 谢怜"));
        assert!(!out.contains("素材未提及"));
    }

    #[test]
    fn voice_profiles_no_table_returns_empty() {
        assert_eq!(build_character_voice_profiles("没有角色档案节"), "");
        // 分隔行不合规则（缺表头行结构）也返回空。
        assert_eq!(build_character_voice_profiles("## 角色档案\n普通文本"), "");
    }

    #[test]
    fn mode_instructions_with_and_without_deviations() {
        let out = build_fanfic_mode_instructions(FanficMode::Ooc, &[]);
        assert!(out.starts_with("\n## 同人写作自检（在 PRE_WRITE_CHECK 中额外检查）\n\n- OOC 偏离记录"));
        assert!(!out.contains("允许的偏离"));

        let deviations = vec!["口头禅".to_string(), "称呼".to_string()];
        let out = build_fanfic_mode_instructions(FanficMode::Au, &deviations);
        assert!(out.contains("\n允许的偏离（不视为违规）：\n- 口头禅\n- 称呼\n"));
    }
}
