//! R51/1653 号：写作目标纯函数族 golden 差分（TS 生产向量 → Rust 镜像
//! 对拍）。四组向量（shiftDayKey/dailyWordDelta/goalStreak/expectedFinish）
//! ——向量由 TS 生产函数导出（golden-writing-goal.test.ts 同文件锁定），
//! 双端一致 = 日历/进度面 duel 可比的前提。Rust 侧 dayKey 算术 = 零依赖
//! civil 日算法（Hinnant），与 TS Date.UTC/toISOString 等价性由跨月/跨年/
//! 闰年向量锁定。

use inkos_engine::utils::writing_goal::{
    daily_word_delta, expected_finish, goal_streak, shift_day_key, DayDelta, WritingLogRow,
};
use std::collections::HashMap;

const VECTORS: &str =
    include_str!("../../packages/core/src/__tests__/golden/writing-goal-vectors.json");

#[derive(serde::Deserialize)]
struct VectorFile {
    #[serde(rename = "shiftDayKey")]
    shift_day_key: Vec<ShiftVector>,
    #[serde(rename = "dailyWordDelta")]
    daily_word_delta: Vec<DeltaVector>,
    #[serde(rename = "goalStreak")]
    goal_streak: Vec<StreakVector>,
    #[serde(rename = "expectedFinish")]
    expected_finish: Vec<FinishVector>,
}

#[derive(serde::Deserialize)]
#[serde(rename_all = "camelCase")]
struct ShiftVector {
    name: String,
    day_key: String,
    days: i64,
    expected: String,
}

#[derive(serde::Deserialize)]
#[serde(rename_all = "camelCase")]
struct DeltaVector {
    name: String,
    rows: Vec<WritingLogRow>,
    /// 每行 dayKey（差分侧查表闭包——与 TS 锚面同喂法）。
    day_keys: Vec<String>,
    expected: Vec<DayDelta>,
}

#[derive(serde::Deserialize)]
#[serde(rename_all = "camelCase")]
struct StreakVector {
    name: String,
    deltas: Vec<DayDelta>,
    goal: i64,
    today_key: String,
    expected: i64,
}

#[derive(serde::Deserialize)]
#[serde(rename_all = "camelCase")]
struct FinishVector {
    name: String,
    current: i64,
    target: i64,
    pace: i64,
    today_key: String,
    /// 空串 = None 语义（已达成/零节奏）。
    expected: String,
}

#[test]
fn writing_goal_functions_match_ts_golden_vectors() {
    let file: VectorFile = serde_json::from_str(VECTORS).expect("向量文件可解析");
    assert!(
        !file.shift_day_key.is_empty(),
        "shiftDayKey golden 非空防空转"
    );
    assert!(
        !file.daily_word_delta.is_empty(),
        "dailyWordDelta golden 非空"
    );
    assert!(!file.goal_streak.is_empty(), "goalStreak golden 非空");
    assert!(
        !file.expected_finish.is_empty(),
        "expectedFinish golden 非空"
    );

    for vector in &file.shift_day_key {
        assert_eq!(
            shift_day_key(&vector.day_key, vector.days),
            vector.expected,
            "{} shiftDayKey 漂移",
            vector.name
        );
    }
    for vector in &file.daily_word_delta {
        let key_by_ts: HashMap<&str, &str> = vector
            .rows
            .iter()
            .enumerate()
            .map(|(index, row)| (row.ts.as_str(), vector.day_keys[index].as_str()))
            .collect();
        let actual = daily_word_delta(&vector.rows, |row| {
            key_by_ts
                .get(row.ts.as_str())
                .copied()
                .unwrap_or_default()
                .to_string()
        });
        assert_eq!(
            actual, vector.expected,
            "{} dailyWordDelta 漂移",
            vector.name
        );
    }
    for vector in &file.goal_streak {
        assert_eq!(
            goal_streak(&vector.deltas, vector.goal, &vector.today_key),
            vector.expected,
            "{} goalStreak 漂移",
            vector.name
        );
    }
    for vector in &file.expected_finish {
        let actual = expected_finish(
            vector.current,
            vector.target,
            vector.pace,
            &vector.today_key,
        );
        let expected = if vector.expected.is_empty() {
            None
        } else {
            Some(vector.expected.clone())
        };
        assert_eq!(actual, expected, "{} expectedFinish 漂移", vector.name);
    }
}
