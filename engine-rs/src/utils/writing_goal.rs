//! R51/1653 号：写作目标与日产出账本纯函数（TS 镜像 =
//! packages/core/src/utils/writing-goal.ts，golden
//! writing-goal-vectors.json 锁定）。
//!
//! 账本行 = `{ts, chapterNum, before, after}` 四键（ts=UTC ISO——写入端
//! 不做时区换算）；日净增 = Σ 该日本地日记录 (after-before)。dayKey 全部
//! 由消费端计算后入参（纯函数不吃时区环境——与 TS 同语义）。

use std::collections::HashMap;

/// 账本行（serde camelCase 对齐 TS `WritingLogRow`）。
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WritingLogRow {
    pub ts: String,
    pub chapter_num: u32,
    pub before: i64,
    pub after: i64,
}

/// 日净增条目（dayKey → 净增，可为负）。
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DayDelta {
    pub day_key: String,
    pub delta: i64,
}

/// 记录流 + dayKey 映射 → 日净增表（dayKey 升序；无记录日不出现）。
/// TS 镜像 = `dailyWordDelta`。
pub fn daily_word_delta(
    rows: &[WritingLogRow],
    day_key_of: impl Fn(&WritingLogRow) -> String,
) -> Vec<DayDelta> {
    let mut buckets: HashMap<String, i64> = HashMap::new();
    for row in rows {
        let day_key = day_key_of(row);
        *buckets.entry(day_key).or_insert(0) += row.after - row.before;
    }
    let mut out: Vec<DayDelta> = buckets
        .into_iter()
        .map(|(day_key, delta)| DayDelta { day_key, delta })
        .collect();
    out.sort_by(|a, b| a.day_key.cmp(&b.day_key));
    out
}

/// 连续达成天数（达成=当日净增 ≥ 日目标）。
/// - 端点 = 净增表最大 dayKey；从端点日逐日回溯（**缺日=0=断链**）。
/// - 今日已达成 → 计入（正向激励）；今日未达成 → 跳过不毁（进行中，从
///   昨日起回溯）；「今日未写不计」= 今日无记录同样跳过不毁。
/// - 非今日的未达标/缺日 → 断链重置（停）。TS 镜像 = `goalStreak`。
pub fn goal_streak(deltas: &[DayDelta], daily_goal_words: i64, today_key: &str) -> i64 {
    if deltas.is_empty() {
        return 0;
    }
    let by_day: HashMap<&str, i64> = deltas
        .iter()
        .map(|entry| (entry.day_key.as_str(), entry.delta))
        .collect();
    let endpoint = deltas
        .iter()
        .map(|entry| entry.day_key.as_str())
        .max()
        .unwrap_or_default()
        .to_string();
    // 今日未达成（或未写）跳过：进行中不判定，从昨日起回溯。
    let mut day_key = if endpoint == today_key
        && by_day.get(today_key).copied().unwrap_or(0) < daily_goal_words
    {
        shift_day_key(today_key, -1)
    } else {
        endpoint
    };
    let mut streak = 0i64;
    while by_day.get(day_key.as_str()).copied().unwrap_or(0) >= daily_goal_words {
        streak += 1;
        day_key = shift_day_key(&day_key, -1);
    }
    streak
}

/// 预计完成日（dayKey 算术）：剩余 = ceil((目标-当前)/日节奏) 天，完成日 =
/// todayKey + 剩余天数。已达成或日节奏 ≤ 0 → None。TS 镜像 =
/// `expectedFinish`。
pub fn expected_finish(
    current_words: i64,
    target_words: i64,
    daily_pace_words: i64,
    today_key: &str,
) -> Option<String> {
    if current_words >= target_words || daily_pace_words <= 0 {
        return None;
    }
    let remaining = (target_words - current_words + daily_pace_words - 1) / daily_pace_words;
    Some(shift_day_key(today_key, remaining))
}

/// dayKey（YYYY-MM-DD）± 天数——零依赖 civil 日算术（Howard Hinnant
/// days_from_civil/civil_from_days；与 TS `shiftDayKey` 的 Date.UTC/
/// toISOString 等价，golden 跨月/跨年向量锁定）。
pub fn shift_day_key(day_key: &str, days: i64) -> String {
    let parts: Vec<&str> = day_key.split('-').collect();
    if parts.len() != 3 {
        panic!("Invalid dayKey format, expected YYYY-MM-DD");
    }
    let year: i64 = parts[0].parse().expect("dayKey year");
    let month: i64 = parts[1].parse().expect("dayKey month");
    let day: i64 = parts[2].parse().expect("dayKey day");
    let shifted = days_from_civil(year, month, day) + days;
    let (y, m, d) = civil_from_days(shifted);
    format!("{y:04}-{m:02}-{d:02}")
}

/// 自 epoch 的天数（公历 civil 日序；Hinnant 算法）。
fn days_from_civil(y: i64, m: i64, d: i64) -> i64 {
    let y = if m <= 2 { y - 1 } else { y };
    let era = if y >= 0 { y } else { y - 399 } / 400;
    let yoe = y - era * 400;
    let mp = if m > 2 { m - 3 } else { m + 9 };
    let doy = (153 * mp + 2) / 5 + d - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146097 + doe - 719468
}

/// epoch 天数 → (年, 月, 日)。
fn civil_from_days(z: i64) -> (i64, i64, i64) {
    let z = z + 719468;
    let era = if z >= 0 { z } else { z - 146096 } / 146097;
    let doe = z - era * 146097;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    (if m <= 2 { y + 1 } else { y }, m, d)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shift_day_key_cross_month_and_year() {
        assert_eq!(shift_day_key("2026-11-30", 1), "2026-12-01");
        assert_eq!(shift_day_key("2026-12-31", 1), "2027-01-01");
        assert_eq!(shift_day_key("2028-02-28", 1), "2028-02-29");
        assert_eq!(shift_day_key("2026-03-01", -1), "2026-02-28");
        assert_eq!(shift_day_key("2026-11-22", 0), "2026-11-22");
    }

    #[test]
    fn streak_today_not_yet_met_does_not_break() {
        let deltas = vec![
            DayDelta {
                day_key: "2026-11-20".into(),
                delta: 500,
            },
            DayDelta {
                day_key: "2026-11-21".into(),
                delta: 600,
            },
            DayDelta {
                day_key: "2026-11-22".into(),
                delta: 100,
            },
        ];
        // 今日 100 < 500：跳过今日，从 21 日回溯 → 21(600✓) 20(500✓) → 2。
        assert_eq!(goal_streak(&deltas, 500, "2026-11-22"), 2);
    }

    #[test]
    fn streak_today_met_counts() {
        let deltas = vec![DayDelta {
            day_key: "2026-11-22".into(),
            delta: 700,
        }];
        assert_eq!(goal_streak(&deltas, 500, "2026-11-22"), 1);
    }

    #[test]
    fn streak_gap_resets() {
        let deltas = vec![
            DayDelta {
                day_key: "2026-11-18".into(),
                delta: 600,
            },
            DayDelta {
                day_key: "2026-11-20".into(),
                delta: 600,
            },
            DayDelta {
                day_key: "2026-11-21".into(),
                delta: 600,
            },
        ];
        // 22 日无记录（非今日表端点=21）：21✓ 20✓ 19 缺=断 → 2。
        assert_eq!(goal_streak(&deltas, 500, "2026-11-22"), 2);
    }

    #[test]
    fn expected_finish_branches() {
        assert_eq!(
            expected_finish(5000, 10000, 1667, "2026-11-22"),
            Some("2026-11-25".into())
        );
        assert_eq!(expected_finish(10000, 10000, 100, "2026-11-22"), None);
        assert_eq!(expected_finish(0, 100, 0, "2026-11-22"), None);
        // ceil：剩余 1.1 天 → 2 天。
        assert_eq!(
            expected_finish(0, 110, 100, "2026-11-22"),
            Some("2026-11-24".into())
        );
    }
}
