//! Agent 窗口的布局规则:栏宽范围、窄窗口自动收起侧栏的判断。纯函数,不依赖界面。
//!
//! @author ixxxxoooo

/// 左侧栏宽度范围(像素)。
pub const LEFT_MIN: f32 = 200.;
pub const LEFT_MAX: f32 = 520.;
/// 右侧面板宽度范围(像素)。
pub const RIGHT_MIN: f32 = 240.;
pub const RIGHT_MAX: f32 = 900.;
/// 中间对话区的最小宽度。
pub const CENTER_MIN: f32 = 400.;

/// 窗口窄于这个宽度时自动收起侧栏。
pub const NARROW_BELOW: f32 = 780.;
/// 窗口宽于这个宽度时恢复侧栏(比收起阈值大一截,避免在临界宽度来回闪)。
pub const WIDE_ABOVE: f32 = 820.;

/// 把保存的左栏宽度夹到合法范围;坏值(非有限数)回退默认。
pub fn clamp_left(w: f32) -> f32 {
    if w.is_finite() { w.clamp(LEFT_MIN, LEFT_MAX) } else { 264. }
}

/// 把保存的右栏宽度夹到合法范围;坏值(非有限数)回退默认。
pub fn clamp_right(w: f32) -> f32 {
    if w.is_finite() { w.clamp(RIGHT_MIN, RIGHT_MAX) } else { 400. }
}

/// 窗口当前是否处于「窄」状态,带滞回:已经窄时要宽过 [`WIDE_ABOVE`] 才算恢复,
/// 还没窄时要窄过 [`NARROW_BELOW`] 才算变窄。
///
/// 参数:`was_narrow` 上一次的状态;`width` 窗口宽度(像素)。
pub fn is_narrow(was_narrow: bool, width: f32) -> bool {
    if was_narrow { width < WIDE_ABOVE } else { width < NARROW_BELOW }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn 窄窗口判断带滞回() {
        assert!(!is_narrow(false, 1000.));
        assert!(is_narrow(false, 779.));
        assert!(!is_narrow(false, 780.));
        // 已经窄了:要到 820 才恢复,800 仍算窄
        assert!(is_narrow(true, 800.));
        assert!(is_narrow(true, 819.));
        assert!(!is_narrow(true, 820.));
        // 未变窄时 800 不算窄
        assert!(!is_narrow(false, 800.));
    }

    #[test]
    fn 栏宽夹到范围() {
        assert_eq!(clamp_left(10.), LEFT_MIN);
        assert_eq!(clamp_left(9999.), LEFT_MAX);
        assert_eq!(clamp_left(300.), 300.);
        assert_eq!(clamp_right(f32::NAN), 400.);
        assert_eq!(clamp_right(f32::INFINITY), 400.);
        assert_eq!(clamp_right(100.), RIGHT_MIN);
    }
}
