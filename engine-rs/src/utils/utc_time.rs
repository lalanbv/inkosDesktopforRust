//! UTC 时间戳工具（`new Date().toISOString()` 等价，无 chrono 依赖）。

/// 当前时刻的 UTC ISO 毫秒精度时间戳（`...Z` 结尾，24 字符）。
pub fn utc_now_iso() -> String {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default();
    unix_to_utc_iso(now.as_secs() as i64, now.subsec_millis())
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn known_timepoints() {
        assert_eq!(civil_from_unix(1_786_752_000), (2026, 8, 15, 0, 0, 0));
        assert_eq!(civil_from_unix(0), (1970, 1, 1, 0, 0, 0));
        assert_eq!(civil_from_unix(951_827_696), (2000, 2, 29, 12, 34, 56));
        assert_eq!(unix_to_utc_iso(1_786_752_000, 123), "2026-08-15T00:00:00.123Z");
        let now = utc_now_iso();
        assert!(now.ends_with('Z') && now.len() == 24);
    }
}
