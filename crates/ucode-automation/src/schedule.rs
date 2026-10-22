//! 计划与下次运行时间。纯函数,全部以本地「墙上时间」计算。

use chrono::{Datelike, Duration, Local, NaiveDate, NaiveDateTime, TimeZone, Weekday};
use serde::{Deserialize, Serialize};

/// 运行计划。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Schedule {
    /// 每隔若干分钟
    Interval { minutes: u32 },
    /// 每天固定时刻
    Daily { hour: u32, minute: u32 },
    /// 每周某天固定时刻;`weekday` 0 = 周一 … 6 = 周日
    Weekly { weekday: u32, hour: u32, minute: u32 },
}

const WEEKDAYS: [&str; 7] = ["周一", "周二", "周三", "周四", "周五", "周六", "周日"];

impl Schedule {
    /// 严格晚于 `after` 的下一次运行时间。
    pub fn next_after(&self, after: NaiveDateTime) -> NaiveDateTime {
        match *self {
            Schedule::Interval { minutes } => after + Duration::minutes(minutes.max(1) as i64),
            Schedule::Daily { hour, minute } => {
                let today = at(after.date(), hour, minute);
                if today > after { today } else { today + Duration::days(1) }
            }
            Schedule::Weekly { weekday, hour, minute } => {
                let target = weekday_of(weekday);
                let delta = (target.num_days_from_monday() as i64 - after.weekday().num_days_from_monday() as i64).rem_euclid(7);
                let candidate = at(after.date() + Duration::days(delta), hour, minute);
                if candidate > after { candidate } else { candidate + Duration::days(7) }
            }
        }
    }

    /// 人类可读的描述。
    pub fn describe(&self) -> String {
        match *self {
            Schedule::Interval { minutes } if minutes % 60 == 0 && minutes >= 60 => format!("每 {} 小时", minutes / 60),
            Schedule::Interval { minutes } => format!("每 {minutes} 分钟"),
            Schedule::Daily { hour, minute } => format!("每天 {hour:02}:{minute:02}"),
            Schedule::Weekly { weekday, hour, minute } => {
                format!("每{} {hour:02}:{minute:02}", WEEKDAYS[(weekday as usize).min(6)])
            }
        }
    }

    /// 校验取值范围。
    pub fn is_valid(&self) -> bool {
        match *self {
            Schedule::Interval { minutes } => (1..=7 * 24 * 60).contains(&minutes),
            Schedule::Daily { hour, minute } => hour < 24 && minute < 60,
            Schedule::Weekly { weekday, hour, minute } => weekday < 7 && hour < 24 && minute < 60,
        }
    }
}

fn weekday_of(n: u32) -> Weekday {
    match n {
        0 => Weekday::Mon,
        1 => Weekday::Tue,
        2 => Weekday::Wed,
        3 => Weekday::Thu,
        4 => Weekday::Fri,
        5 => Weekday::Sat,
        _ => Weekday::Sun,
    }
}

fn at(date: NaiveDate, hour: u32, minute: u32) -> NaiveDateTime {
    date.and_hms_opt(hour.min(23), minute.min(59), 0).expect("时分已被限制在合法范围")
}

/// 构造本地墙上时间(测试与界面共用)。
pub fn naive(y: i32, m: u32, d: u32, h: u32, min: u32) -> NaiveDateTime {
    NaiveDate::from_ymd_opt(y, m, d).expect("合法日期").and_hms_opt(h, min, 0).expect("合法时间")
}

/// Unix 秒 → 本地墙上时间。
pub fn to_naive(ts: i64) -> NaiveDateTime {
    Local.timestamp_opt(ts, 0).single().map(|d| d.naive_local()).unwrap_or_default()
}

/// 本地墙上时间 → Unix 秒(夏令时间隙取较早的有效时刻)。
pub fn to_ts(t: NaiveDateTime) -> i64 {
    Local.from_local_datetime(&t).earliest().map(|d| d.timestamp()).unwrap_or_else(|| t.and_utc().timestamp())
}

/// 当前 Unix 秒。
pub fn now_ts() -> i64 {
    Local::now().timestamp()
}

/// 把 Unix 秒格式化成 `MM-DD HH:MM`。
pub fn format_ts(ts: i64) -> String {
    to_naive(ts).format("%m-%d %H:%M").to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn 每隔若干分钟() {
        let s = Schedule::Interval { minutes: 30 };
        assert_eq!(s.next_after(naive(2026, 10, 22, 8, 50)), naive(2026, 10, 22, 9, 20));
    }

    #[test]
    fn 每天固定时刻() {
        let s = Schedule::Daily { hour: 9, minute: 30 };
        assert_eq!(s.next_after(naive(2026, 10, 22, 8, 0)), naive(2026, 10, 22, 9, 30));
        assert_eq!(s.next_after(naive(2026, 10, 22, 9, 30)), naive(2026, 10, 23, 9, 30), "必须严格晚于");
        assert_eq!(s.next_after(naive(2026, 12, 31, 10, 0)), naive(2027, 1, 1, 9, 30), "跨年");
    }

    #[test]
    fn 每周固定时刻() {
        // 2026-10-22 是周四
        let fri = Schedule::Weekly { weekday: 4, hour: 18, minute: 0 };
        assert_eq!(fri.next_after(naive(2026, 10, 22, 12, 0)), naive(2026, 10, 23, 18, 0));
        let thu = Schedule::Weekly { weekday: 3, hour: 9, minute: 0 };
        assert_eq!(thu.next_after(naive(2026, 10, 22, 12, 0)), naive(2026, 10, 29, 9, 0), "今天已过,顺延一周");
        assert_eq!(thu.next_after(naive(2026, 10, 22, 8, 0)), naive(2026, 10, 22, 9, 0));
        let mon = Schedule::Weekly { weekday: 0, hour: 9, minute: 0 };
        assert_eq!(mon.next_after(naive(2026, 10, 22, 8, 0)), naive(2026, 10, 26, 9, 0));
    }

    #[test]
    fn 描述与校验() {
        assert_eq!(Schedule::Interval { minutes: 120 }.describe(), "每 2 小时");
        assert_eq!(Schedule::Interval { minutes: 45 }.describe(), "每 45 分钟");
        assert_eq!(Schedule::Daily { hour: 7, minute: 5 }.describe(), "每天 07:05");
        assert_eq!(Schedule::Weekly { weekday: 6, hour: 20, minute: 0 }.describe(), "每周日 20:00");
        assert!(!Schedule::Daily { hour: 24, minute: 0 }.is_valid());
        assert!(!Schedule::Interval { minutes: 0 }.is_valid());
        assert!(Schedule::Weekly { weekday: 6, hour: 23, minute: 59 }.is_valid());
    }

    #[test]
    fn 时间戳往返() {
        let t = naive(2026, 10, 22, 9, 5);
        assert_eq!(to_naive(to_ts(t)), t);
    }
}
