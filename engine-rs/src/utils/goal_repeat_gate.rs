//! R28 章节目标防复读门（v5 五轮规划 P1，399 号）。
//!
//! TS 真源：`packages/core/src/utils/goal-repeat-gate.ts`。
//! planner 产物校验：memo.goal 与近 N 章摘要高度重合 → 判定复读；
//! 阈值保守（0.8 Dice）+ 只重规划一次 + 仍犯降级告警不阻断。
//! 共享向量 `packages/core/src/__tests__/golden/goal-repeat-vectors.json`
//! 差分：`tests/golden_goal_repeat_diff.rs`。

use std::collections::HashSet;

pub const GOAL_REPEAT_SIMILARITY_THRESHOLD: f64 = 0.8;
pub const GOAL_REPEAT_WINDOW: usize = 3;

/// 保留字母/数字/汉字（\u4e00–\u9fff），其余剔除；ASCII 小写。
/// 双端口径逐字对齐（禁 locale/全量 toLowerCase）。
pub fn normalize_goal_repeat_text(raw: &str) -> String {
    let mut out = String::new();
    for ch in raw.chars() {
        let code = ch as u32;
        let is_ascii_lower = (0x61..=0x7a).contains(&code);
        let is_ascii_upper = (0x41..=0x5a).contains(&code);
        let is_ascii_digit = (0x30..=0x39).contains(&code);
        let is_han = (0x4e00..=0x9fff).contains(&code);
        if is_ascii_lower || is_ascii_digit || is_han {
            out.push(ch);
        } else if is_ascii_upper {
            out.push((code as u8 + 0x20) as char);
        }
    }
    out
}

fn normalized_bigrams(normalized: &str) -> HashSet<String> {
    let chars: Vec<char> = normalized.chars().collect();
    // 对齐 TS `new Set(chars)`：len<2 时集合即字符本身（本函数仅在两侧
    // len≥2 的 Dice 路径被调用，此分支只为语义逐字对齐保留）。
    if chars.len() < 2 {
        return chars.into_iter().map(String::from).collect();
    }
    let mut set = HashSet::new();
    for i in 0..chars.len() - 1 {
        set.insert(format!("{}{}", chars[i], chars[i + 1]));
    }
    set
}

/// 字符 bigram 集合 Dice 系数；4 位小数向下截断（floor 口径，双端一致）。
pub fn goal_repeat_similarity(a: &str, b: &str) -> f64 {
    let na = normalize_goal_repeat_text(a);
    let nb = normalize_goal_repeat_text(b);
    // 退化短串：非空且相等即全同，否则无重叠（空 bigram 集合的 Dice 无定义）。
    if na.chars().count() < 2 || nb.chars().count() < 2 {
        return if !na.is_empty() && na == nb { 1.0 } else { 0.0 };
    }
    let ga = normalized_bigrams(&na);
    let gb = normalized_bigrams(&nb);
    let overlap = ga.intersection(&gb).count();
    let dice = (2.0 * overlap as f64) / (ga.len() as f64 + gb.len() as f64);
    (dice * 10000.0).floor() / 10000.0
}

/// 复读判定（TS GoalRepeatVerdict 同形；camelCase 序列化对齐消费端）。
#[derive(Debug, Clone, PartialEq, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GoalRepeatVerdict {
    pub repeat: bool,
    pub max_similarity: f64,
    pub exact_match: bool,
}

/// 复读判定：goal 与任一对照文本完全相等（归一化后）或相似度 ≥ 阈值 → repeat。
/// reference_texts 为空（开书首章）或 goal 归一化后为空 → 恒不判复读。
pub fn evaluate_goal_repeat(
    goal: &str,
    reference_texts: &[String],
    threshold: Option<f64>,
) -> GoalRepeatVerdict {
    let threshold = threshold.unwrap_or(GOAL_REPEAT_SIMILARITY_THRESHOLD);
    let normalized_goal = normalize_goal_repeat_text(goal);
    if normalized_goal.is_empty() || reference_texts.is_empty() {
        return GoalRepeatVerdict { repeat: false, max_similarity: 0.0, exact_match: false };
    }
    let mut max_similarity = 0.0f64;
    let mut exact_match = false;
    for reference in reference_texts {
        let normalized = normalize_goal_repeat_text(reference);
        if normalized.is_empty() {
            continue;
        }
        if normalized == normalized_goal {
            exact_match = true;
        }
        max_similarity = max_similarity.max(goal_repeat_similarity(goal, reference));
    }
    GoalRepeatVerdict {
        repeat: exact_match || max_similarity >= threshold,
        max_similarity,
        exact_match,
    }
}
