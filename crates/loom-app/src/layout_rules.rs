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

/// 输入框(`Composer`)的最小宽度:IDE 对话栏等放输入框的栏,下限就是它。
///
/// 由各部分的固定宽度加起来(见 [`composer_floor`]),工具栏里的模式 / 模型 chip 在窄时收缩并截断文字,
/// 其余控件固定宽度,所以不会出现按钮被挤出框外。
pub const COMPOSER_MIN_W: f32 = 370.;
/// 输入框外层左右内边距(各 16)。
#[cfg(test)]
pub const COMPOSER_OUTER_PAD: f32 = 32.;
/// 工具栏内边距(各 8)与框线(各 1)。
#[cfg(test)]
pub const COMPOSER_BAR_PAD: f32 = 18.;
/// 模式 chip 收缩后的最小宽度(图标 + 下拉箭头 + 内边距)。
pub const MODE_CHIP_MIN: f32 = 44.;
/// 模型 chip 收缩后的最小宽度(品牌图标 + 一小段文字 + 下拉箭头)。
pub const MODEL_CHIP_MIN: f32 = 80.;
/// 思考强度 chip 收缩后的最小宽度(文字 + 下拉箭头 + 内边距)。
pub const EFFORT_CHIP_MIN: f32 = 40.;
/// MCP chip 收缩后的最小宽度(图标 + 数字 + 下拉箭头 + 内边距)。
pub const MCP_CHIP_MIN: f32 = 44.;
/// 上下文环、提示词优化、发送三个固定控件的宽度。
#[cfg(test)]
pub const COMPOSER_FIXED: f32 = 26. + 28. + 28.;
/// 工具栏里 7 个控件之间的间距(gap_1 = 4,共 6 处)。
#[cfg(test)]
pub const COMPOSER_GAPS: f32 = 24.;

/// 输入框各部分固定宽度之和:`COMPOSER_MIN_W` 不得小于它(只在测试里校验)。
#[cfg(test)]
pub fn composer_floor() -> f32 {
    COMPOSER_OUTER_PAD + COMPOSER_BAR_PAD + MODE_CHIP_MIN + MODEL_CHIP_MIN + EFFORT_CHIP_MIN + MCP_CHIP_MIN + COMPOSER_FIXED + COMPOSER_GAPS
}

/// 窗口窄于这个宽度时自动收起侧栏。
pub const NARROW_BELOW: f32 = 780.;
/// 窗口宽于这个宽度时恢复侧栏(比收起阈值大一截,避免在临界宽度来回闪)。
pub const WIDE_ABOVE: f32 = 820.;

/// 窗口窄于这个宽度时自动收起右侧面板(只剩侧栏 + 对话)。
pub const RIGHT_HIDE_BELOW: f32 = 1100.;
/// 窗口宽于这个宽度时恢复右侧面板(带滞回)。
pub const RIGHT_SHOW_ABOVE: f32 = 1140.;

/// 通用的带滞回判断:`was` 为上一次是否处于「收起」状态。
///
/// 参数:`was` 上次状态;`width` 窗口宽度;`below` 变窄阈值;`above` 恢复阈值。
/// 返回:本次是否应处于收起状态。
pub fn collapsed(was: bool, width: f32, below: f32, above: f32) -> bool {
    if was { width < above } else { width < below }
}

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
    fn 输入框最小宽度容得下所有控件() {
        assert!(COMPOSER_MIN_W >= composer_floor(), "工具栏控件的固定宽度之和 {} 超过了最小宽度 {COMPOSER_MIN_W}", composer_floor());
        // 对话区的下限也不能比输入框还窄
        assert!(CENTER_MIN >= COMPOSER_MIN_W);
    }

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
    fn 通用滞回判断() {
        assert!(!collapsed(false, 1100., RIGHT_HIDE_BELOW, RIGHT_SHOW_ABOVE));
        assert!(collapsed(false, 1099., RIGHT_HIDE_BELOW, RIGHT_SHOW_ABOVE));
        assert!(collapsed(true, 1139., RIGHT_HIDE_BELOW, RIGHT_SHOW_ABOVE));
        assert!(!collapsed(true, 1140., RIGHT_HIDE_BELOW, RIGHT_SHOW_ABOVE));
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
