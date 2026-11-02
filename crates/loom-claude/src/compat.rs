//! 与 claude 版本的兼容性:未识别协议事件的记录、版本号解析与检查。
//!
//! - 实时会话里遇到翻译器不认识的事件类型 / system 子类型时,每种只记一条日志(避免刷屏),
//!   并在诊断页的计数指标里累计次数,便于 claude 升级后发现「静默丢内容」;
//! - `check_version` 对照已验证的版本区间,给出过旧 / 未验证 / 正常三种结论。
//!
//! @author ixxxxoooo

use std::collections::BTreeSet;
use std::sync::Mutex;
use std::sync::atomic::{AtomicU64, Ordering};

/// 支持的最低版本(更旧的缺少本应用依赖的控制协议字段)。
pub const MIN_VERSION: (u32, u32, u32) = (2, 1, 0);
/// 已对其验证过协议的「主.次」版本。
pub const TESTED_MINOR: (u32, u32) = (2, 1);

/// 诊断页里的计数指标名。
const GAUGE: &str = "未识别的协议事件";

static SEEN: Mutex<BTreeSet<String>> = Mutex::new(BTreeSet::new());
static COUNT: AtomicU64 = AtomicU64::new(0);

/// 记录一次未识别的事件。
/// 参数:`kind` 位置(如 `type`、`system`),`name` 事件名。返回:无。
pub fn note_unknown(kind: &str, name: &str) {
    let n = COUNT.fetch_add(1, Ordering::Relaxed) + 1;
    loom_diag::stats::set_gauge(GAUGE, n);
    let key = format!("{kind}:{name}");
    let first = SEEN.lock().map(|mut s| s.insert(key)).unwrap_or(false);
    if first {
        loom_diag::info!("翻译器未识别的协议事件 {kind}={name:?}(已忽略,同类只记一次;claude 升级后可能新增了事件)");
    }
}

/// 至今记录到的未识别事件次数。
pub fn unknown_count() -> u64 {
    COUNT.load(Ordering::Relaxed)
}

/// 版本检查结论。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum VersionCheck {
    /// 在已验证区间内。
    Ok,
    /// 低于最低版本,需要升级。
    TooOld,
    /// 比已验证的更新,协议可能有变化。
    Untested,
    /// 输出里找不到版本号。
    Unknown,
}

/// 从 `claude --version` 的输出(如 `2.1.32 (Claude Code)`)里取出版本号。
/// 参数:`text` 命令输出。返回:(主, 次, 修订),找不到为 `None`。
pub fn parse_version(text: &str) -> Option<(u32, u32, u32)> {
    text.split(|c: char| c.is_whitespace() || c == '(' || c == 'v').find_map(|tok| {
        let mut it = tok.split('.');
        let major = it.next()?.parse().ok()?;
        let minor = it.next()?.parse().ok()?;
        let patch = it.next().map(|p| p.chars().take_while(char::is_ascii_digit).collect::<String>()).and_then(|p| p.parse().ok()).unwrap_or(0);
        Some((major, minor, patch))
    })
}

/// 对照已验证区间检查版本。
/// 参数:`text` `claude --version` 的输出。返回:检查结论。
pub fn check_version(text: &str) -> VersionCheck {
    match parse_version(text) {
        None => VersionCheck::Unknown,
        Some(v) if v < MIN_VERSION => VersionCheck::TooOld,
        Some((a, b, _)) if (a, b) > TESTED_MINOR => VersionCheck::Untested,
        Some(_) => VersionCheck::Ok,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn 解析常见版本输出() {
        assert_eq!(parse_version("2.1.32 (Claude Code)"), Some((2, 1, 32)));
        assert_eq!(parse_version("claude v2.0.5"), Some((2, 0, 5)));
        assert_eq!(parse_version("2.2.0-beta.1"), Some((2, 2, 0)));
        assert_eq!(parse_version("未找到 claude 命令"), None);
    }

    #[test]
    fn 按区间给出结论() {
        assert_eq!(check_version("2.1.32 (Claude Code)"), VersionCheck::Ok);
        assert_eq!(check_version("2.0.99"), VersionCheck::TooOld);
        assert_eq!(check_version("1.9.0"), VersionCheck::TooOld);
        assert_eq!(check_version("2.2.0"), VersionCheck::Untested);
        assert_eq!(check_version("3.0.0"), VersionCheck::Untested);
        assert_eq!(check_version("garbage"), VersionCheck::Unknown);
    }

    #[test]
    fn 未识别事件计数且同类只记一次() {
        let before = unknown_count();
        note_unknown("type", "测试用新事件");
        note_unknown("type", "测试用新事件");
        // 计数是全局的,并行运行的翻译测试也会累加,所以只断言至少增加了 2
        assert!(unknown_count() >= before + 2);
        assert!(SEEN.lock().unwrap().contains("type:测试用新事件"));
    }
}
