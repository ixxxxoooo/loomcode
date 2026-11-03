//! 平移 / 缩放的纯数学(与界面无关):全屏图表查看器使用。
//!
//! 约定:`scale` 是「内容 1 个单位 = 屏幕多少像素」,`off` 是内容左上角在视口里的坐标。
//! 这样以光标为中心缩放、适应窗口、限制平移范围都是直接的几何运算,窗口缩放时也不用换算相对量。
//!
//! @author ixxxxoooo

/// 缩放下限 / 上限(绝对倍数)。
pub const MIN_SCALE: f32 = 0.05;
pub const MAX_SCALE: f32 = 12.;
/// 平移时内容至少要留在视口里的像素数(拖丢了还能拽回来)。
const KEEP_VISIBLE: f32 = 64.;

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PanZoom {
    pub scale: f32,
    pub off: (f32, f32),
}

impl PanZoom {
    /// 适应视口:整张图居中放进 `viewport` 并留 `margin` 边距;`max_scale` 防止小图被放得过大。
    pub fn fit(content: (f32, f32), viewport: (f32, f32), margin: f32, max_scale: f32) -> Self {
        let avail = ((viewport.0 - margin * 2.).max(1.), (viewport.1 - margin * 2.).max(1.));
        let scale = (avail.0 / content.0.max(1.)).min(avail.1 / content.1.max(1.)).min(max_scale).clamp(MIN_SCALE, MAX_SCALE);
        Self { scale, off: ((viewport.0 - content.0 * scale) / 2., (viewport.1 - content.1 * scale) / 2.) }
    }

    /// 以视口坐标 `at` 为中心缩放到绝对倍数 `scale`:该点下面的内容位置保持不动。
    pub fn zoom_to_at(&mut self, scale: f32, at: (f32, f32)) {
        let s = scale.clamp(MIN_SCALE, MAX_SCALE);
        let r = s / self.scale;
        self.off = (at.0 - (at.0 - self.off.0) * r, at.1 - (at.1 - self.off.1) * r);
        self.scale = s;
    }

    /// 以 `at` 为中心按倍率 `factor` 缩放(滚轮 / 捏合 / 按钮)。
    pub fn zoom_by_at(&mut self, factor: f32, at: (f32, f32)) {
        self.zoom_to_at(self.scale * factor, at);
    }

    pub fn pan(&mut self, dx: f32, dy: f32) {
        self.off = (self.off.0 + dx, self.off.1 + dy);
    }

    /// 限制平移范围:内容比视口小的方向居中不让拖走,比视口大的方向边缘不能离开视口太远。
    pub fn clamp(&mut self, content: (f32, f32), viewport: (f32, f32)) {
        let size = (content.0 * self.scale, content.1 * self.scale);
        let axis = |off: f32, size: f32, view: f32| {
            let keep = KEEP_VISIBLE.min(size).min(view);
            off.clamp(keep - size, view - keep)
        };
        self.off = (axis(self.off.0, size.0, viewport.0), axis(self.off.1, size.1, viewport.1));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn near(a: f32, b: f32) -> bool {
        (a - b).abs() < 1e-3
    }

    #[test]
    fn 适应窗口居中且不放大超过上限() {
        // 内容 400×200 放进 1000×800(边距 40):宽度方向限制 → 920/400=2.3,高度 720/200=3.6,取 2.3;上限 2 时取 2
        let f = PanZoom::fit((400., 200.), (1000., 800.), 40., 10.);
        assert!(near(f.scale, 2.3));
        assert!(near(f.off.0, 40.) && near(f.off.1, (800. - 200. * 2.3) / 2.));
        assert!(near(PanZoom::fit((400., 200.), (1000., 800.), 40., 2.).scale, 2.));
        // 大图缩小到刚好放下
        let big = PanZoom::fit((4000., 1000.), (1000., 800.), 40., 2.);
        assert!(near(big.scale, 0.23));
    }

    #[test]
    fn 以光标为中心缩放时光标下的内容不动() {
        let mut p = PanZoom { scale: 1., off: (100., 50.) };
        let at = (400., 300.);
        // 光标下的内容坐标(缩放前)
        let content = ((at.0 - p.off.0) / p.scale, (at.1 - p.off.1) / p.scale);
        p.zoom_by_at(2.5, at);
        assert!(near(p.scale, 2.5));
        assert!(near(p.off.0 + content.0 * p.scale, at.0) && near(p.off.1 + content.1 * p.scale, at.1));
        // 再缩回去
        p.zoom_by_at(1. / 2.5, at);
        assert!(near(p.scale, 1.) && near(p.off.0, 100.) && near(p.off.1, 50.));
    }

    #[test]
    fn 缩放被限制在范围内() {
        let mut p = PanZoom { scale: 1., off: (0., 0.) };
        p.zoom_to_at(1000., (0., 0.));
        assert!(near(p.scale, MAX_SCALE));
        p.zoom_to_at(0.0001, (0., 0.));
        assert!(near(p.scale, MIN_SCALE));
    }

    #[test]
    fn 平移不会把内容拖出视口() {
        let content = (500., 400.);
        let view = (1000., 800.);
        let mut p = PanZoom { scale: 1., off: (9999., -9999.) };
        p.clamp(content, view);
        // 右边最多让内容留 64px 在视口内;上边同理
        assert!(near(p.off.0, 1000. - KEEP_VISIBLE) && near(p.off.1, KEEP_VISIBLE - 400.));
        // 在范围内的位置不动
        let mut q = PanZoom { scale: 1., off: (200., 100.) };
        q.clamp(content, view);
        assert_eq!(q.off, (200., 100.));
        // 内容很小(比 64px 还小)时,整块都要留在视口里
        let mut tiny = PanZoom { scale: 1., off: (-500., 0.) };
        tiny.clamp((20., 20.), view);
        assert!(near(tiny.off.0, 20. - 20.));
    }
}
