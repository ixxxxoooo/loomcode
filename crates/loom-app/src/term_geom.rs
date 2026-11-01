//! 终端的几何换算:鼠标位置 → 格子、选区 → 每行要画的区间。纯函数,不依赖界面。
//!
//! @author ixxxxoooo

/// 鼠标落在哪个格子。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CellPos {
    pub row: usize,
    pub col: usize,
    /// 落在格子的右半边(决定选区边界字符算不算选中)
    pub right: bool,
}

/// 把相对终端区域左上角的鼠标位置换算成格子,越界夹到边缘。
///
/// 参数:`x` `y` 鼠标相对区域左上角的坐标;`cell_w` `line_h` 格子宽高;`pad` 内容留白;
/// `cols` `rows` 当前网格大小。
pub fn cell_at(x: f32, y: f32, cell_w: f32, line_h: f32, pad: f32, cols: usize, rows: usize) -> CellPos {
    let axis = |v: f32, size: f32, n: usize| -> (usize, bool) {
        let n = n.max(1);
        let f = (v - pad) / size;
        if f < 0. {
            return (0, false);
        }
        let i = f.floor() as usize;
        if i >= n {
            return (n - 1, true);
        }
        (i, f - f.floor() >= 0.5)
    };
    let (col, right) = axis(x, cell_w, cols);
    let (row, _) = axis(y, line_h, rows);
    CellPos { row, col, right }
}

/// 把选区(起点、终点,含两端)拆成每一行要画的 `(行, 起始列, 结束列)`,结束列含在内。
/// 首行从起点列画到行尾,中间行整行,末行从行首画到终点列;单行选区只画起点到终点。
pub fn selection_spans(sel: ((usize, usize), (usize, usize)), cols: usize) -> Vec<(usize, usize, usize)> {
    let ((r0, c0), (r1, c1)) = sel;
    let last = cols.saturating_sub(1);
    (r0..=r1)
        .map(|r| {
            let from = if r == r0 { c0 } else { 0 };
            let to = if r == r1 { c1 } else { last };
            (r, from.min(last), to.min(last))
        })
        .filter(|(_, from, to)| from <= to)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn 鼠标位置换算成格子() {
        // 格宽 10、行高 16、留白 6:x=6 是第 0 列左边缘
        assert_eq!(cell_at(6., 6., 10., 16., 6., 80, 24), CellPos { row: 0, col: 0, right: false });
        assert_eq!(cell_at(12., 6., 10., 16., 6., 80, 24), CellPos { row: 0, col: 0, right: true });
        assert_eq!(cell_at(16., 22., 10., 16., 6., 80, 24), CellPos { row: 1, col: 1, right: false });
        assert_eq!(cell_at(36., 6., 10., 16., 6., 80, 24).col, 3);
    }

    #[test]
    fn 越界夹到边缘() {
        assert_eq!(cell_at(-50., -50., 10., 16., 6., 80, 24), CellPos { row: 0, col: 0, right: false });
        let far = cell_at(9999., 9999., 10., 16., 6., 80, 24);
        assert_eq!((far.row, far.col, far.right), (23, 79, true));
        // 网格为 0 时不下溢
        assert_eq!(cell_at(100., 100., 10., 16., 6., 0, 0).col, 0);
    }

    #[test]
    fn 单行与多行选区拆分() {
        assert_eq!(selection_spans(((2, 3), (2, 7)), 80), vec![(2, 3, 7)]);
        assert_eq!(selection_spans(((1, 70), (3, 5)), 80), vec![(1, 70, 79), (2, 0, 79), (3, 0, 5)]);
        // 列号超出行宽时夹到行尾
        assert_eq!(selection_spans(((0, 5), (0, 200)), 10), vec![(0, 5, 9)]);
    }
}
