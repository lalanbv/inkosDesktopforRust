//! UTC 时间戳工具（`new Date().toISOString()` 等价，无 chrono 依赖）。

/// 当前时刻的 UTC ISO 毫秒精度时间戳（`...Z` 结尾，24 字符）。
pub fn utc_now_iso() -> String {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default();
    unix_to_utc_iso(now.as_secs() as i64, now.subsec_millis())
}

/// 当前 Unix 毫秒时间戳（13 位，对齐 JS `Date.now()`）。
pub fn utc_now_millis() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}

/// Unix 秒 + 毫秒 → UTC ISO 时间戳。
pub fn unix_to_utc_iso(secs: i64, millis: u32) -> String {
    let (year, month, day, hour, minute, second) = civil_from_unix(secs);
    format!("{year:04}-{month:02}-{day:02}T{hour:02}:{minute:02}:{second:02}.{millis:03}Z")
}

/// Unix 秒 → 公历 civil 时刻（Howard Hinnant 算法）。
pub fn civil_from_unix(secs: i64) -> (i64, u32, u32, u32, u32, u32) {
    let days = secs.div_euclid(86_400);
    let rem = secs.rem_euclid(86_400);
    let hour = (rem / 3600) as u32;
    let minute = ((rem % 3600) / 60) as u32;
    let second = (rem % 60) as u32;

    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let m = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    let year = if m <= 2 { y + 1 } else { y };
    (year, m, d, hour, minute, second)
}

/// Unix 毫秒 → UTC ISO 毫秒时间戳（`utc_now_iso` 同形）。
pub fn unix_millis_to_utc_iso(millis: i64) -> String {
    // 余数恒在 0..1000，收窄无损。
    unix_to_utc_iso(millis.div_euclid(1000), millis.rem_euclid(1000) as u32)
}

/// 公历 civil 日期 → 自 1970-01-01 的天数（`civil_from_unix` 的逆；Howard
/// Hinnant 算法）。
pub fn days_from_civil(year: i64, month: i64, day: i64) -> i64 {
    let year = if month <= 2 { year - 1 } else { year };
    let era = year.div_euclid(400);
    let year_of_era = year - era * 400;
    let day_of_year = (153 * (if month > 2 { month - 3 } else { month + 9 }) + 2) / 5 + day - 1;
    let day_of_era = year_of_era * 365 + year_of_era / 4 - year_of_era / 100 + day_of_year;
    era * 146_097 + day_of_era - 719_468
}

/// `unix_to_utc_iso` 的逆：UTC ISO 毫秒时间戳 → Unix 毫秒。只认
/// `YYYY-MM-DDTHH:MM:SS.sssZ` 这一种 24 字符形态（分隔符与各数字段逐一校验），
/// 其余 None——调用方据此区分「可顺推的时间戳」与「任意字符串」。
pub fn utc_iso_to_unix_millis(ts: &str) -> Option<i64> {
    let bytes = ts.as_bytes();
    if bytes.len() != 24
        || bytes[4] != b'-'
        || bytes[7] != b'-'
        || bytes[10] != b'T'
        || bytes[13] != b':'
        || bytes[16] != b':'
        || bytes[19] != b'.'
        || bytes[23] != b'Z'
    {
        return None;
    }
    let field = |from: usize, to: usize| {
        let text = ts.get(from..to)?;
        text.bytes()
            .all(|b| b.is_ascii_digit())
            .then(|| text.parse::<i64>().ok())?
    };
    let year = field(0, 4)?;
    let month = field(5, 7)?;
    let day = field(8, 10)?;
    let hour = field(11, 13)?;
    let minute = field(14, 16)?;
    let second = field(17, 19)?;
    let millis = field(20, 23)?;
    if !(1..=12).contains(&month) || !(1..=31).contains(&day) {
        return None;
    }
    Some(
        (days_from_civil(year, month, day) * 86_400 + hour * 3600 + minute * 60 + second) * 1000
            + millis,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn known_timepoints() {
        assert_eq!(civil_from_unix(1_786_752_000), (2026, 8, 15, 0, 0, 0));
        assert_eq!(civil_from_unix(0), (1970, 1, 1, 0, 0, 0));
        assert_eq!(civil_from_unix(951_827_696), (2000, 2, 29, 12, 34, 56));
        assert_eq!(
            unix_to_utc_iso(1_786_752_000, 123),
            "2026-08-15T00:00:00.123Z"
        );
        let now = utc_now_iso();
        assert!(now.ends_with('Z') && now.len() == 24);
    }

    /// 1666 号：逆运算往返——跨月/跨年/闰日/纪元前各取一点，正逆互推一致。
    #[test]
    fn iso_millis_round_trips_with_civil_conversion() {
        for secs in [
            0_i64,
            86_399,
            86_400,
            951_827_696,   // 2000-02-29（闰日）
            1_709_164_800, // 2024-03-01T00:00:00Z（闰年二月末次日）
            1_735_689_599, // 2024-12-31T23:59:59Z（跨年前一秒）
            1_735_689_600, // 2025-01-01T00:00:00Z
            1_786_752_000,
            -1, // 1969-12-31T23:59:59Z（纪元前）
        ] {
            let (year, month, day, ..) = civil_from_unix(secs);
            assert_eq!(
                days_from_civil(year, month.into(), day.into()),
                secs.div_euclid(86_400)
            );
            let millis = secs * 1000 + 7;
            let iso = unix_millis_to_utc_iso(millis);
            assert_eq!(utc_iso_to_unix_millis(&iso), Some(millis), "{iso}");
            assert_eq!(unix_to_utc_iso(secs, 7), iso);
        }
        assert_eq!(
            unix_to_utc_iso(1_786_752_000, 123),
            "2026-08-15T00:00:00.123Z"
        );
        // 只认 24 字符 ISO 毫秒形态。
        for rejected in [
            "",
            "1",
            "2026-08-15",
            "2026-08-15T00:00:00Z",
            "2026-08-15T00:00:00.123",
            "2026-08-15 00:00:00.123Z",
            "2026-13-15T00:00:00.123Z",
            "2026-08-15T00:00:00.12Z",
            "2026-08-15T00:00:00.12aZ",
            "2026-08-15T00:00:00.123+00:00",
        ] {
            assert_eq!(utc_iso_to_unix_millis(rejected), None, "{rejected}");
        }
    }
}
