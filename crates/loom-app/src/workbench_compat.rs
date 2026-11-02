//! claude 版本检查结论的界面文案(纯函数,便于测试)。
//!
//! @author ixxxxoooo

use loom_claude::compat::{MIN_VERSION, TESTED_MINOR, VersionCheck, check_version};

/// 把 `claude --version` 的输出翻成给用户看的一句提示。
/// 参数:`text` 命令输出。返回:需要提醒时的文案;版本正常或没装 claude(另有提示)时为 `None`。
pub fn version_hint(text: &str) -> Option<String> {
    let (a, b, c) = MIN_VERSION;
    let (ta, tb) = TESTED_MINOR;
    match check_version(text) {
        VersionCheck::TooOld => Some(format!("版本过低,最低需要 {a}.{b}.{c},请运行 claude update 升级")),
        VersionCheck::Untested => Some(format!("比已验证的 {ta}.{tb}.x 更新,个别功能可能不兼容,遇到问题请在设置 → 诊断里查看日志")),
        VersionCheck::Ok | VersionCheck::Unknown => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[::core::prelude::v1::test]
    fn 只在过旧或未验证时提醒() {
        assert!(version_hint("2.1.32 (Claude Code)").is_none());
        assert!(version_hint("未找到 claude 命令").is_none());
        assert!(version_hint("2.0.1").unwrap().contains("版本过低"));
        assert!(version_hint("2.3.0").unwrap().contains("更新"));
    }
}
