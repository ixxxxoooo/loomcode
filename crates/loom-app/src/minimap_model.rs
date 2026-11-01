//! 迷你地图的纯数据与几何:按语法着色的行色块、滑块位置与滚动比例换算。与界面无关,可单独测试。
//!
//! @author ixxxxoooo

use std::ops::Range;

/// 每行最多画多少列(再长的行没有缩略意义)。
pub const MAX_COLS: usize = 120;

/// 字符的「墨量」档位:缩略图里按字形的疏密画出深浅,标点淡、小写与数字适中、大写与符号浓,
/// 代码才有「纹理」而不是一条条实心色带(Monaco 用逐字符的字形密度,这里分三档近似)。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Weight {
    Light,
    Normal,
    Heavy,
}

impl Weight {
    /// 绘制时叠加到透明度上的系数。
    pub fn alpha(self) -> f32 {
        match self {
            Weight::Light => 0.5,
            Weight::Normal => 0.8,
            Weight::Heavy => 1.0,
        }
    }
}

/// 某个字符的墨量档位。
pub fn weight_of(ch: char) -> Weight {
    match ch {
        '.' | ',' | ';' | ':' | '\'' | '"' | '`' | '!' | '|' | '(' | ')' | '[' | ']' | '{' | '}' | '-' | '_' | '/' | '\\' => Weight::Light,
        'A'..='Z' | '@' | '#' | '$' | '%' | '&' | '*' | '+' | '=' | '<' | '>' | '~' | '^' | '?' => Weight::Heavy,
        // 中日韩等宽字符笔画密
        c if !c.is_ascii() => Weight::Heavy,
        _ => Weight::Normal,
    }
}

/// 一行里同色、同墨量的连续非空白字符。
#[derive(Debug, Clone, PartialEq)]
pub struct MiniRun<C> {
    /// 起始列(含缩进)
    pub start: u16,
    pub len: u16,
    /// 语法颜色;`None` 用默认灰
    pub color: Option<C>,
    /// 墨量档位
    pub weight: Weight,
}

/// 整个文档的缩略数据:每行一组色块。
#[derive(Debug, Clone, PartialEq)]
pub struct MiniMap<C> {
    pub rows: Vec<Vec<MiniRun<C>>>,
}

impl<C> Default for MiniMap<C> {
    fn default() -> Self {
        Self { rows: Vec::new() }
    }
}

/// 由文本与语法着色区间生成缩略数据。
///
/// 参数:`text` 全文;`styles` 按字节偏移排序的 (区间, 颜色),可以有空隙(空隙用默认色),为空则整体单色。
/// 制表符按 4 列算;空白打断色块;相邻同色且同墨量的字符合并成一个色块,绘制时一个色块只需一次填充。
pub fn build<C: Copy + PartialEq>(text: &str, styles: &[(Range<usize>, Option<C>)]) -> MiniMap<C> {
    let mut rows = Vec::new();
    let (mut offset, mut si) = (0usize, 0usize);
    for line in text.split('\n') {
        let mut runs: Vec<MiniRun<C>> = Vec::new();
        let mut col = 0usize;
        for (i, ch) in line.char_indices() {
            if col >= MAX_COLS {
                break;
            }
            if ch.is_whitespace() {
                col += if ch == '\t' { 4 } else { 1 };
                continue;
            }
            let b = offset + i;
            while si < styles.len() && styles[si].0.end <= b {
                si += 1;
            }
            let color = styles.get(si).filter(|(r, _)| r.start <= b).and_then(|(_, c)| *c);
            let weight = weight_of(ch);
            match runs.last_mut() {
                Some(r) if r.color == color && r.weight == weight && usize::from(r.start) + usize::from(r.len) == col => r.len += 1,
                _ => runs.push(MiniRun { start: col as u16, len: 1, color, weight }),
            }
            col += 1;
        }
        rows.push(runs);
        offset += line.len() + 1;
    }
    MiniMap { rows }
}

/// 滑块与滚动的几何:统一「内容比面板短」与「内容比面板长(缩略图按比例滚动)」两种情形。
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Geo {
    /// 每行像素高度
    pub row_h: f32,
    /// 面板高度
    pub panel_h: f32,
    /// 总行数
    pub rows: usize,
    /// 编辑器视口的首行与行数
    pub vis_start: usize,
    pub vis_len: usize,
}

impl Geo {
    fn content_h(&self) -> f32 {
        self.rows as f32 * self.row_h
    }

    /// 编辑器滚动比例 0..=1。
    fn frac(&self) -> f32 {
        let span = self.rows.saturating_sub(self.vis_len);
        if span == 0 { 0. } else { (self.vis_start as f32 / span as f32).clamp(0., 1.) }
    }

    /// 缩略内容整体上移多少像素(内容不够长时为 0)。
    pub fn offset(&self) -> f32 {
        let over = self.content_h() - self.panel_h;
        if over <= 0. { 0. } else { self.frac() * over }
    }

    /// 视口滑块在面板里的 (顶部, 高度)。
    pub fn slider(&self) -> (f32, f32) {
        let h = (self.row_h * self.vis_len as f32).max(8.).min(self.panel_h);
        (self.row_h * self.vis_start as f32 - self.offset(), h)
    }

    /// 缩略图里某一行(含小数)的面板纵坐标。
    pub fn row_y(&self, row: f32) -> f32 {
        row * self.row_h - self.offset()
    }

    /// 反算:滑块顶部在面板里的位置 → 编辑器视口应当从第几行开始(含小数)。
    pub fn start_row_for_slider_top(&self, top: f32) -> f32 {
        let span = self.rows.saturating_sub(self.vis_len) as f32;
        if self.content_h() <= self.panel_h {
            (top / self.row_h).clamp(0., span.max(0.))
        } else {
            let travel = (self.panel_h - self.slider().1).max(1.);
            (top / travel).clamp(0., 1.) * span
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[::core::prelude::v1::test]
    fn 同色相邻字符合并_空白打断() {
        let text = "fn ab {\n  let x;\n}";
        // 前两个字符(fn)着色 1,其余默认
        let m = build(text, &[(0..2, Some(1u8))]);
        assert_eq!(m.rows.len(), 3);
        assert_eq!(m.rows[0][0], MiniRun { start: 0, len: 2, color: Some(1), weight: Weight::Normal });
        // 空格后 ab 合并成一个默认色块,再空格后 { 单独一块(标点淡)
        assert_eq!(m.rows[0][1], MiniRun { start: 3, len: 2, color: None, weight: Weight::Normal });
        assert_eq!(m.rows[0][2], MiniRun { start: 6, len: 1, color: None, weight: Weight::Light });
        // 第二行缩进 2 列
        assert_eq!(m.rows[1][0].start, 2);
    }

    #[::core::prelude::v1::test]
    fn 空行没有色块且制表符算四列() {
        let m = build::<u8>("\n\tx\r\n", &[]);
        assert!(m.rows[0].is_empty());
        assert_eq!(m.rows[1], vec![MiniRun { start: 4, len: 1, color: None, weight: Weight::Normal }]);
    }

    #[::core::prelude::v1::test]
    fn 墨量档位分三档且打断合并() {
        assert_eq!(weight_of('('), Weight::Light);
        assert_eq!(weight_of('a'), Weight::Normal);
        assert_eq!(weight_of('7'), Weight::Normal);
        assert_eq!(weight_of('W'), Weight::Heavy);
        assert_eq!(weight_of('中'), Weight::Heavy);
        assert!(Weight::Light.alpha() < Weight::Normal.alpha() && Weight::Normal.alpha() < Weight::Heavy.alpha());
        // 「fooBar()」:foo 一块、B 一块(浓)、ar 一块、() 一块(淡)
        let m = build::<u8>("fooBar()", &[]);
        let w: Vec<_> = m.rows[0].iter().map(|r| (r.start, r.len, r.weight)).collect();
        assert_eq!(w, [(0, 3, Weight::Normal), (3, 1, Weight::Heavy), (4, 2, Weight::Normal), (6, 2, Weight::Light)]);
    }

    #[::core::prelude::v1::test]
    fn 超长行按最大列数截断() {
        let m = build::<u8>(&"a".repeat(500), &[]);
        assert_eq!(usize::from(m.rows[0][0].len), MAX_COLS);
    }

    #[::core::prelude::v1::test]
    fn 着色区间有空隙时回退默认色() {
        let m = build("abcd", &[(1..2, Some(7u8)), (3..4, Some(9u8))]);
        let colors: Vec<_> = m.rows[0].iter().map(|r| (r.start, r.len, r.color)).collect();
        assert_eq!(colors, [(0, 1, None), (1, 1, Some(7)), (2, 1, None), (3, 1, Some(9))]);
    }

    #[::core::prelude::v1::test]
    fn 短文件滑块按行高定位且可反算() {
        let g = Geo { row_h: 2., panel_h: 600., rows: 100, vis_start: 10, vis_len: 30 };
        assert_eq!(g.offset(), 0.);
        assert_eq!(g.slider(), (20., 60.));
        assert!((g.start_row_for_slider_top(20.) - 10.).abs() < 1e-4);
    }

    #[::core::prelude::v1::test]
    fn 长文件缩略图按比例滚动且滑块像滚动条() {
        let rows = 2000;
        let panel = 600.;
        for start in [0usize, 500, 1970] {
            let g = Geo { row_h: 2., panel_h: panel, rows, vis_start: start, vis_len: 30 };
            let (top, h) = g.slider();
            // 滑块始终在面板内
            assert!(top >= -1e-3 && top + h <= panel + 1e-3, "start={start} top={top} h={h}");
            // 反算回到同一个起始行
            assert!((g.start_row_for_slider_top(top) - start as f32).abs() < 0.5, "start={start}");
        }
        // 滚到底时滑块贴着面板底部
        let g = Geo { row_h: 2., panel_h: panel, rows, vis_start: rows - 30, vis_len: 30 };
        let (top, h) = g.slider();
        assert!((top + h - panel).abs() < 1e-3);
    }
}
