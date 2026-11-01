//! 终端调色板:16 色按明暗主题各一套,16..255 按 xterm 标准公式计算。纯函数。
//!
//! @author ixxxxoooo

/// 深色背景下的 16 色(参考 VS Code 默认终端配色)。
const DARK16: [u32; 16] = [
    0x000000, 0xCD3131, 0x0DBC79, 0xE5E510, 0x2472C8, 0xBC3FBC, 0x11A8CD, 0xE5E5E5,
    0x666666, 0xF14C4C, 0x23D18B, 0xF5F543, 0x3B8EEA, 0xD670D6, 0x29B8DB, 0xFFFFFF,
];

/// 浅色背景下的 16 色(白色 / 亮白压暗,避免在白底上看不见)。
const LIGHT16: [u32; 16] = [
    0x000000, 0xCD3131, 0x00A268, 0x949800, 0x0451A5, 0xBC05BC, 0x0598BC, 0x555555,
    0x666666, 0xCD3131, 0x14CE14, 0xB5BA00, 0x0451A5, 0xBC05BC, 0x0598BC, 0xA5A5A5,
];

/// 256 色表里第 `i` 项的 RGB。
///
/// 参数:`i` 颜色序号;`dark` 当前是否深色主题(只影响前 16 色)。
pub fn indexed_rgb(i: u8, dark: bool) -> (u8, u8, u8) {
    match i {
        0..=15 => {
            let v = if dark { DARK16 } else { LIGHT16 }[i as usize];
            ((v >> 16) as u8, (v >> 8) as u8, v as u8)
        }
        16..=231 => {
            // 6×6×6 色立方,每个分量取值 0 / 95 / 135 / 175 / 215 / 255
            let n = i - 16;
            let level = |x: u8| if x == 0 { 0 } else { 55 + 40 * x };
            (level(n / 36), level((n / 6) % 6), level(n % 6))
        }
        232..=255 => {
            // 24 级灰阶 8, 18, ..., 238
            let g = 8 + 10 * (i - 232);
            (g, g, g)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn 前_16_色分明暗() {
        assert_eq!(indexed_rgb(1, true), (0xCD, 0x31, 0x31));
        // 白色在浅色主题下被压暗
        assert_eq!(indexed_rgb(7, true), (0xE5, 0xE5, 0xE5));
        assert_eq!(indexed_rgb(7, false), (0x55, 0x55, 0x55));
    }

    #[test]
    fn 色立方与灰阶符合_xterm() {
        assert_eq!(indexed_rgb(16, true), (0, 0, 0));
        assert_eq!(indexed_rgb(21, true), (0, 0, 255));
        assert_eq!(indexed_rgb(196, true), (255, 0, 0));
        assert_eq!(indexed_rgb(231, true), (255, 255, 255));
        assert_eq!(indexed_rgb(232, true), (8, 8, 8));
        assert_eq!(indexed_rgb(255, true), (238, 238, 238));
        // 立方中间值:索引 16 + 36*1 + 6*2 + 3 = 67 → (95, 135, 175)
        assert_eq!(indexed_rgb(67, true), (95, 135, 175));
    }
}
