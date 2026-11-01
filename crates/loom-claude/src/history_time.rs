//! 历史回放用的时间戳工具:从 jsonl 记录的 `timestamp` 算出思考与回合的用时。
//!
//! @author ixxxxoooo

/// 解析 `2026-10-09T16:47:26.753Z` 形式的 UTC 时间,返回自 1970 年起的毫秒数;格式不对返回 `None`。
pub fn parse_iso_ms(s: &str) -> Option<i64> {
    let (date, time) = s.trim().trim_end_matches('Z').split_once('T')?;
    let mut d = date.split('-').map(|x| x.parse::<i64>().ok());
    let (y, m, day) = (d.next()??, d.next()??, d.next()??);
    let (hms, frac) = time.split_once('.').unwrap_or((time, "0"));
    let mut t = hms.split(':').map(|x| x.parse::<i64>().ok());
    let (h, mi, sec) = (t.next()??, t.next()??, t.next()??);
    if !(1..=12).contains(&m) || !(1..=31).contains(&day) {
        return None;
    }
    // 公历日期 → 自 1970-01-01 的天数(Howard Hinnant 的 days_from_civil)
    let y2 = if m <= 2 { y - 1 } else { y };
    let era = y2.div_euclid(400);
    let yoe = y2 - era * 400;
    let doy = (153 * (if m > 2 { m - 3 } else { m + 9 }) + 2) / 5 + day - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    let days = era * 146_097 + doe - 719_468;
    // 毫秒:取小数的前三位
    let ms: i64 = format!("{frac:0<3}")[..3].parse().ok()?;
    Some(((days * 24 + h) * 60 + mi) * 60_000 + sec * 1000 + ms)
}

/// 两个时间点之间的用时(秒),限制在 1 秒到 `max_secs` 之间;`end` 不晚于 `start` 返回 `None`。
pub fn secs_between(start: i64, end: i64, max_secs: u64) -> Option<u64> {
    (end > start).then(|| (((end - start) as u64 + 500) / 1000).clamp(1, max_secs))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn 解析时间戳() {
        assert_eq!(parse_iso_ms("1970-01-01T00:00:00Z"), Some(0));
        assert_eq!(parse_iso_ms("1970-01-01T00:00:01.5Z"), Some(1500));
        assert_eq!(parse_iso_ms("2000-03-01T00:00:00.000Z"), Some(951_868_800_000));
        let a = parse_iso_ms("2026-10-09T16:47:26.753Z").unwrap();
        let b = parse_iso_ms("2026-10-09T16:47:41.753Z").unwrap();
        assert_eq!(b - a, 15_000);
        assert_eq!(parse_iso_ms("not a time"), None);
        assert_eq!(parse_iso_ms("2026-13-01T00:00:00Z"), None);
    }

    #[test]
    fn 用时取整并限幅() {
        assert_eq!(secs_between(0, 400, 3600), Some(1));
        assert_eq!(secs_between(0, 12_400, 3600), Some(12));
        assert_eq!(secs_between(0, 99_999_999, 3600), Some(3600));
        assert_eq!(secs_between(5, 5, 3600), None);
        assert_eq!(secs_between(9, 5, 3600), None);
    }
}
