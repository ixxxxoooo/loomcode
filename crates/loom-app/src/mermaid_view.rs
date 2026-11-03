//! Mermaid 图表的界面渲染:`loom-mermaid` 排版出图元场景,这里按主题上色并用 gpui 绘制
//! (形状与连线走画布路径,文字用绝对定位的文本元素,字体与全局一致)。
//! 排版结果按源码缓存;不支持的类型 / 语法错误回退为带原因的源码卡片。
//!
//! @author ixxxxoooo

use crate::ui::{MONO, card, icon};
use gpui_kit::assets::IconName;
use gpui_kit::component::button::{Button, ButtonVariants};
use gpui_kit::component::{ActiveTheme, Sizable, h_flex, v_flex};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;
use loom_mermaid::{Anchor, Item, Paint, Role, Scene};
use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;

/// 缓存里最多保留的图数。
const CACHE_MAX: usize = 64;

type Rendered = Rc<Result<Scene, loom_mermaid::RenderError>>;

thread_local! {
    static CACHE: RefCell<HashMap<String, Rendered>> = RefCell::new(HashMap::new());
}

/// 取(或排版并缓存)源码对应的场景。
pub fn scene_of(src: &str) -> Rendered {
    CACHE.with(|c| {
        let mut c = c.borrow_mut();
        if let Some(s) = c.get(src) {
            return s.clone();
        }
        if c.len() >= CACHE_MAX {
            c.clear();
        }
        let r = Rc::new(loom_mermaid::render(src));
        c.insert(src.to_string(), r.clone());
        r
    })
}

/// 强调色板(饼图扇区等)。
fn accent(i: u8, dark: bool) -> Hsla {
    const HUES: [f32; 8] = [0.60, 0.08, 0.40, 0.76, 0.95, 0.50, 0.13, 0.0];
    hsla(HUES[i as usize % 8], 0.62, if dark { 0.58 } else { 0.52 }, 1.)
}

/// 语义角色 → 当前主题颜色。
fn color(p: Paint, cx: &App) -> Hsla {
    let t = cx.theme();
    let dark = t.is_dark();
    match p {
        Paint::Rgb(v) => rgb(v).into(),
        Paint::Role(r) => match r {
            Role::Surface => t.background,
            Role::Node => if dark { t.muted } else { t.muted.opacity(0.7) },
            Role::Border => t.muted_foreground.opacity(0.55),
            Role::Line => t.muted_foreground,
            Role::Text => t.foreground,
            Role::Muted => t.muted_foreground,
            Role::Note => if dark { hsla(0.12, 0.45, 0.22, 1.) } else { hsla(0.13, 0.9, 0.88, 1.) },
            Role::Group => t.muted.opacity(0.35),
            Role::Accent(i) => accent(i, dark),
        },
    }
}

/// 一个待画的形状:图元 + 已解析好的填充 / 描边色。
type Shape = (Item, Option<Hsla>, Option<Hsla>);

/// 画布闭包:按顺序画出全部矩形与折线;`s` 是缩放倍数(全屏查看时矢量重绘,放大不糊)。
fn paint_shapes(shapes: &[Shape], origin: Point<Pixels>, s: f32, window: &mut Window) {
    for (item, fill_c, stroke_c) in shapes {
        match item {
            Item::Rect { x, y, w, h, r, width, .. } => {
                let rect = Bounds::new(point(origin.x + px(*x * s), origin.y + px(*y * s)), size(px(*w * s), px(*h * s)));
                let (bw, bc) = stroke_c.map_or((0., transparent_black()), |c| (*width * s, c));
                window.paint_quad(quad(rect, px(*r * s), fill_c.unwrap_or_else(transparent_black), bw, bc, BorderStyle::default()));
            }
            Item::Poly { pts, closed, width, .. } if pts.len() >= 2 => {
                let at = |p: &(f32, f32)| point(origin.x + px(p.0 * s), origin.y + px(p.1 * s));
                if let (true, Some(c)) = (*closed, fill_c) {
                    let mut b = PathBuilder::fill();
                    b.move_to(at(&pts[0]));
                    pts[1..].iter().for_each(|p| b.line_to(at(p)));
                    b.close();
                    if let Ok(path) = b.build() {
                        window.paint_path(path, *c);
                    }
                }
                if let Some(c) = stroke_c {
                    let mut b = PathBuilder::stroke(px(*width * s));
                    b.move_to(at(&pts[0]));
                    pts[1..].iter().for_each(|p| b.line_to(at(p)));
                    if *closed {
                        b.close();
                    }
                    if let Ok(path) = b.build() {
                        window.paint_path(path, *c);
                    }
                }
            }
            _ => {}
        }
    }
}

/// 把场景按 `scale` 倍画成元素(文字字号、线宽、坐标全部按倍数换算,保持矢量清晰)。
pub fn scene_element(scene: &Scene, scale: f32, cx: &App) -> impl IntoElement {
    let shapes: Vec<Shape> = scene
        .items
        .iter()
        .filter_map(|i| match i {
            Item::Rect { fill, stroke, .. } | Item::Poly { fill, stroke, .. } => Some((i.clone(), fill.map(|p| color(p, cx)), stroke.map(|p| color(p, cx)))),
            Item::Text { .. } => None,
        })
        .collect();
    let paint = canvas(|_, _, _| (), move |bounds, _, window, _| paint_shapes(&shapes, bounds.origin, scale, window)).absolute().size_full();
    let texts = scene.items.iter().filter_map(|i| {
        let Item::Text { x, y, text, size: fs, bold, anchor, paint } = i else { return None };
        let (fs, lh) = (fs * scale, fs * 1.35 * scale);
        let (x, y, box_w) = (x * scale, y * scale, 800. * scale);
        let row = div().absolute().top(px(y - lh / 2.)).w(px(box_w)).h(px(lh)).flex().items_center().whitespace_nowrap().text_size(px(fs)).line_height(px(lh)).text_color(color(*paint, cx));
        let row = match anchor {
            Anchor::Start => row.left(px(x)).justify_start(),
            Anchor::Middle => row.left(px(x - box_w / 2.)).justify_center(),
            Anchor::End => row.left(px(x - box_w)).justify_end(),
        };
        Some(if *bold { row.font_weight(FontWeight::SEMIBOLD) } else { row }.child(text.clone()))
    });
    div().relative().w(px(scene.width * scale)).h(px(scene.height * scale)).flex_none().child(paint).children(texts)
}

/// 渲染一张 Mermaid 图。
///
/// 参数:`key` 元素 id 前缀(同页内唯一);`src` 源码;`show_src` 是否显示源码;
/// `on_toggle` 点「查看图 / 查看源码」时调用。
/// 返回:卡片元素;无法排版时卡片里显示原因与源码。
pub fn render(key: &str, src: &str, show_src: bool, on_toggle: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static, cx: &App) -> AnyElement {
    let t = cx.theme();
    let result = scene_of(src);
    let title = loom_mermaid::kind_title(src).map_or("Mermaid".to_string(), |k| format!("Mermaid · {k}"));
    let failed = result.as_ref().is_err();
    let src_text = src.trim_end().to_string();
    let copy_src = src_text.clone();
    let title_open = title.clone();
    let head = h_flex().gap_2().items_center()
        .child(icon(IconName::Workflow, 14., t.muted_foreground))
        .child(div().text_size(px(12.)).text_color(t.muted_foreground).child(title))
        .child(div().flex_1())
        .child(Button::new(SharedString::from(format!("mm-copy-{key}"))).ghost().xsmall().icon(IconName::Copy).tooltip("复制源码").on_click(move |_, _, cx| cx.write_to_clipboard(ClipboardItem::new_string(copy_src.clone()))))
        .when(!failed, |d| d.child({
            let (title, src) = (title_open.clone(), src_text.clone());
            Button::new(SharedString::from(format!("mm-full-{key}"))).ghost().xsmall().icon(IconName::Maximize).tooltip("全屏查看(可缩放、拖动)")
                .on_click(move |_, window, cx| crate::diagram_viewer::open_mermaid(&title, &src, window, cx))
        }))
        .when(!failed, |d| d.child(Button::new(SharedString::from(format!("mm-toggle-{key}"))).ghost().xsmall().label(if show_src { "查看图" } else { "查看源码" }).on_click(on_toggle)));
    let body: AnyElement = match result.as_ref() {
        Ok(scene) if !show_src => {
            // 双击图也能全屏查看
            let (title, src) = (title_open.clone(), src_text.clone());
            div().id(SharedString::from(format!("mm-{key}"))).w_full().overflow_x_scroll()
                .on_click(move |ev, window, cx| {
                    if matches!(ev, ClickEvent::Mouse(m) if m.down.click_count >= 2) {
                        crate::diagram_viewer::open_mermaid(&title, &src, window, cx);
                    }
                })
                .child(h_flex().min_w(px(scene.width)).justify_center().child(scene_element(scene, 1., cx)))
                .into_any_element()
        }
        other => {
            let note = other.as_ref().err().map(|e| e.to_string());
            v_flex().w_full().gap_1p5()
                .when_some(note, |d, n| d.child(div().text_size(px(12.)).text_color(t.warning).child(format!("{n},以下为源码"))))
                .child(div().w_full().p_2().rounded(px(6.)).bg(t.muted).font_family(MONO).text_size(px(12.)).child(StyledText::new(src_text.clone()).with_highlights(crate::code_hl::styles(&src_text, "mermaid", cx))))
                .into_any_element()
        }
    };
    card(cx).p_3().gap_2().child(head).child(body).into_any_element()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 演示画布里的 Mermaid 块:除了故意不支持的 gantt,其余都能排版成图。
    #[::core::prelude::v1::test]
    fn 演示画布的图表都能渲染() {
        let doc = loom_canvas::demo_document("/w", 1);
        fn walk(blocks: &[loom_canvas::Block], out: &mut Vec<String>) {
            for b in blocks {
                match b {
                    loom_canvas::Block::Mermaid { text } => out.push(text.clone()),
                    loom_canvas::Block::Section { blocks, .. } => walk(blocks, out),
                    _ => {}
                }
            }
        }
        let mut all = Vec::new();
        walk(&doc.blocks, &mut all);
        assert!(all.len() >= 6);
        for src in all {
            match loom_mermaid::render(&src) {
                Ok(s) => assert!(s.width > 50. && s.height > 50., "{src}"),
                Err(loom_mermaid::RenderError::Unsupported(k)) => assert_eq!(k, "gantt"),
                Err(e) => panic!("{src}: {e}"),
            }
        }
    }

    /// 缓存按源码命中,同一源码返回同一份排版。
    #[::core::prelude::v1::test]
    fn 排版结果被缓存() {
        let a = scene_of("pie\n \"a\" : 1");
        let b = scene_of("pie\n \"a\" : 1");
        assert!(Rc::ptr_eq(&a, &b));
    }
}
