//! 编辑器的悬停卡片(LSP hover):自己渲染,不用编辑器自带的气泡。
//!
//! 自带气泡的代码块带边框、零内边距,右侧还压着语言标签,和主题不搭。这里参考 Zed 的做法:
//! 卡片是弹出层底色 + 细边框 + 阴影,代码块是淡底圆角(无边框、有内边距、不显示语言标签),
//! 签名与文档之间用分隔线;卡片挂在符号上方(上方放不下才放下方),鼠标离开「符号 + 卡片」区域
//! 或点击外部即收起。语言服务器的悬停结果由 `lsp::LspProvider` 写进文档,再让编辑器自带气泡不出现(提供者返回 `None`)。
//!
//! @author ixxxxoooo

use crate::ide::{IdeDoc, IdeView};
use gpui_kit::component::input::EditorState;
use gpui_kit::component::ActiveTheme;
use gpui_kit::component::text::{TextView, TextViewStyle};
use gpui_kit::*;
use std::ops::Range;
use std::path::Path;

/// 卡片最大宽度。
const MAX_W: f32 = 520.;
/// 卡片最大高度。
const MAX_H: f32 = 320.;
/// 悬停内容字节上限(与 Zed 的 `MAX_HOVER_BYTES` 一致),超出截断。
pub const MAX_HOVER_BYTES: usize = 100_000;
/// 卡片与符号之间的间距(Zed 是 10,这里贴近鼠标取 4)。
const GAP: f32 = 4.;

/// 悬停卡片的界面状态:收起计时、鼠标到卡片的最近距离(判断是否在靠近卡片)。
#[derive(Default)]
pub struct HoverUi {
    hide_task: Option<Task<()>>,
    closest: Option<Pixels>,
    /// 鼠标正朝卡片靠近(途经别的符号时不替换 / 不清掉当前卡片)
    approaching: bool,
}

/// 一张悬停卡片:触发的符号范围(字节偏移)与 Markdown 内容。
#[derive(Clone, Debug, PartialEq)]
pub struct HoverCard {
    pub range: Range<usize>,
    pub markdown: SharedString,
    /// 卡片出现时的 (选区起, 选区止, 视口首行):之后任何一项变了(光标移动 / 滚动)就收起
    pub anchor: (usize, usize, usize),
    /// 请求悬停时的鼠标位置:卡片横向贴着鼠标出现
    pub mouse: Point<Pixels>,
}

/// 编辑器当前的 (选区起, 选区止, 视口首行)。
pub fn view_anchor(ed: &EditorState, ) -> (usize, usize, usize) {
    let sel = ed.selected_range();
    (sel.start, sel.end, ed.visible_row_range().map_or(0, |r| r.start))
}

/// 把 LSP 悬停内容拼成 Markdown。带语言的片段包成代码块,方便着色;全空白返回 `None`。
///
/// 参数:`hover` LSP 悬停结果。返回:Markdown 文本。
pub fn hover_markdown(hover: &lsp_types::Hover) -> Option<String> {
    use lsp_types::{HoverContents, MarkedString};
    let one = |m: &MarkedString| match m {
        MarkedString::String(s) => s.clone(),
        MarkedString::LanguageString(l) => format!("```{}\n{}\n```", l.language, l.value),
    };
    let text = match &hover.contents {
        HoverContents::Scalar(m) => one(m),
        HoverContents::Array(items) => items.iter().map(one).collect::<Vec<_>>().join("\n\n"),
        HoverContents::Markup(m) => m.value.clone(),
    };
    (!text.trim().is_empty()).then(|| truncate(text))
}

/// 超过 `MAX_HOVER_BYTES` 时在字符边界截断并补「…」;截断后代码围栏不成对就补一个,免得吞掉后面的排版。
fn truncate(mut s: String) -> String {
    if s.len() <= MAX_HOVER_BYTES {
        return s;
    }
    let mut end = MAX_HOVER_BYTES - '…'.len_utf8();
    while !s.is_char_boundary(end) {
        end -= 1;
    }
    s.truncate(end);
    s.push('…');
    if s.matches("```").count() % 2 == 1 {
        s.push_str("\n```");
    }
    s
}

/// 点到矩形的距离(在矩形内为 0)。
fn distance_to(b: Bounds<Pixels>, p: Point<Pixels>) -> Pixels {
    let dx = (b.left() - p.x).max(p.x - b.right()).max(px(0.));
    let dy = (b.top() - p.y).max(p.y - b.bottom()).max(px(0.));
    px(f32::from(dx).hypot(f32::from(dy)))
}

impl IdeView {
    /// 写入 / 清除某文档的悬停卡片。
    pub(crate) fn set_hover_card(&mut self, path: &Path, mut card: Option<HoverCard>, cx: &mut Context<Self>) {
        if let (Some(c), Some(doc)) = (card.as_mut(), self.docs.get(path)) {
            c.anchor = view_anchor(doc.editor.read(cx));
        }
        if card.is_some() {
            self.hover_ui.hide_task = None;
            self.hover_ui.closest = None;
        }
        if let Some(doc) = self.docs.get_mut(path).filter(|d| d.hover != card) {
            doc.hover = card;
            cx.notify();
        }
    }

    /// 鼠标移动时的粘性收起判断(照 Zed):在符号或卡片上就取消计时;离开后等 300ms 再收起,
    /// 但鼠标在朝卡片靠近时不收起(重新计时),远离且计时已在走就让它走完。
    pub(crate) fn hover_mouse_moved(&mut self, path: &Path, pos: Point<Pixels>, symbol: Bounds<Pixels>, card: Bounds<Pixels>, cx: &mut Context<Self>) {
        let ui = &mut self.hover_ui;
        if card.contains(&pos) || symbol.contains(&pos) {
            ui.hide_task = None;
            ui.closest = None;
            ui.approaching = false;
            return;
        }
        let d = distance_to(card, pos);
        // 非粘性:离开符号与卡片就收起
        let (sticky, hide_ms) = { let e = crate::prefs(cx).editor; (e.hover_sticky, e.hover_hiding_delay_ms) };
        if !sticky {
            self.set_hover_card(path, None, cx);
            return;
        }
        let ui = &mut self.hover_ui;
        let closer = !ui.closest.is_some_and(|c| d > c + px(4.));
        ui.approaching = closer;
        if closer {
            ui.closest = Some(d.min(ui.closest.unwrap_or(d)));
        }
        if !closer && ui.hide_task.is_some() {
            return;
        }
        let path = path.to_path_buf();
        ui.hide_task = Some(cx.spawn(async move |this, cx| {
            cx.background_executor().timer(std::time::Duration::from_millis(u64::from(hide_ms))).await;
            let _ = this.update(cx, |this, cx| {
                this.hover_ui.hide_task.take().map(Task::detach);
                this.set_hover_card(&path, None, cx);
            });
        }));
    }

    /// 光标移动或滚动后(选区 / 视口首行与卡片出现时不同)收起卡片,与 Zed 一致。
    pub(crate) fn hover_check_moved(&mut self, path: &Path, cx: &App) -> bool {
        let Some(doc) = self.docs.get_mut(path) else { return false };
        let moved = doc.hover.as_ref().is_some_and(|c| c.anchor != view_anchor(doc.editor.read(cx)));
        if moved {
            doc.hover = None;
        }
        moved
    }

    /// 这次悬停请求是否可以跳过:当前卡片已覆盖该位置(不重复请求),或鼠标正朝卡片移动(途经别的符号不换卡片)。
    pub(crate) fn hover_blocked(&self, path: &Path, offset: usize) -> bool {
        let Some(card) = self.docs.get(path).and_then(|d| d.hover.as_ref()) else { return false };
        card.range.contains(&offset) || self.hover_ui.approaching
    }
}

/// 悬停卡片元素。没有卡片、符号不在视口里、内容已过期(编辑后范围越界)时返回 `None`。
///
/// 参数:`doc` 文档;`path` 文档路径;`view` 所在视图(收起时回写)。返回:定位好的弹层。
pub fn hover_card(doc: &IdeDoc, path: &Path, view: WeakEntity<IdeView>, window: &Window, cx: &App) -> Option<AnyElement> {
    let card = doc.hover.as_ref()?;
    let ed = doc.editor.read(cx);
    let at = ed.range_to_bounds(&card.range)?;
    let t = cx.theme();
    // 横向贴着鼠标(略偏左一点),而不是符号起点
    let left = (card.mouse.x - px(14.)).max(px(8.));
    let above = at.origin.y > window.bounds().size.height * 0.35;
    let (pos, anchor) = if above {
        (point(left, at.origin.y - px(GAP)), Anchor::BottomLeft)
    } else {
        (point(left, at.origin.y + at.size.height + px(GAP)), Anchor::TopLeft)
    };
    let (view, path) = (view, path.to_path_buf());
    let guard = canvas(
        |b, _, _| b,
        move |card_bounds, _, window, _| {
            let (v1, p1) = (view.clone(), path.clone());
            window.on_mouse_event(move |e: &MouseMoveEvent, _, _, cx| {
                let pos = e.position;
                let _ = v1.update(cx, |this, cx| this.hover_mouse_moved(&p1, pos, at, card_bounds, cx));
            });
            // 点击卡片外、在卡片外滚动、按 Esc:立即收起(Zed 在选区变化 / 滚动时同样收起)
            let (v2, p2) = (view.clone(), path.clone());
            window.on_mouse_event(move |e: &MouseDownEvent, _, _, cx| {
                if !card_bounds.contains(&e.position) {
                    let _ = v2.update(cx, |this, cx| this.set_hover_card(&p2, None, cx));
                }
            });
            let (v3, p3) = (view.clone(), path.clone());
            window.on_mouse_event(move |e: &ScrollWheelEvent, _, _, cx| {
                if !card_bounds.contains(&e.position) {
                    let _ = v3.update(cx, |this, cx| this.set_hover_card(&p3, None, cx));
                }
            });
            let (v4, p4) = (view.clone(), path.clone());
            window.on_key_event(move |e: &KeyDownEvent, _, _, cx| {
                if e.keystroke.key == "escape" {
                    let _ = v4.update(cx, |this, cx| this.set_hover_card(&p4, None, cx));
                }
            });
        },
    )
    .absolute()
    .size_full();
    let style = TextViewStyle::default().paragraph_gap(rems(0.5)).inline_code(crate::md_theme::inline_code(cx)).code_block(
        StyleRefinement::default().bg(t.muted.opacity(0.55)).border_0().rounded(px(6.)).px_2().py_1p5().text_size(px(12.)),
    );
    // 给代码块一个空的「操作区」,库就不会在右上角画语言标签
    let body = TextView::markdown(SharedString::from(format!("ide-hover-md-{}", card.range.start)), card.markdown.clone()).style(style).selectable(true).code_block_actions(|_, _, _| div());
    Some(
        deferred(
            anchored().position(pos).anchor(anchor).snap_to_window_with_margin(px(8.)).child(
                // 卡片里滚到头就停,不把滚轮事件传给下面的编辑器(内置的卡片滚动先处理,这里再截断冒泡)
                div().id("ide-hover-card").relative().occlude().on_scroll_wheel(|_, _, cx| cx.stop_propagation()).max_w(px(MAX_W)).max_h(px(MAX_H)).overflow_y_scroll()
                    .px_3().py_2().rounded(t.radius).border_1().border_color(t.border).bg(t.popover).text_color(t.popover_foreground)
                    .text_size(px(12.5)).shadow_md()
                    .child(guard)
                    .child(body),
            ),
        )
        .into_any_element(),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use lsp_types::{Hover, HoverContents, LanguageString, MarkedString, MarkupContent, MarkupKind};

    fn h(contents: HoverContents) -> Hover {
        Hover { contents, range: None }
    }

    #[::core::prelude::v1::test]
    fn markup_原样返回() {
        let m = HoverContents::Markup(MarkupContent { kind: MarkupKind::Markdown, value: "```rust\nfn a()\n```".into() });
        assert_eq!(hover_markdown(&h(m)).unwrap(), "```rust\nfn a()\n```");
    }

    #[::core::prelude::v1::test]
    fn 语言片段包成代码块() {
        let m = HoverContents::Scalar(MarkedString::LanguageString(LanguageString { language: "rust".into(), value: "fn a()".into() }));
        assert_eq!(hover_markdown(&h(m)).unwrap(), "```rust\nfn a()\n```");
    }

    #[::core::prelude::v1::test]
    fn 超长内容在字符边界截断并补齐围栏() {
        let s = format!("```rust\n{}", "汉".repeat(MAX_HOVER_BYTES));
        let out = truncate(s);
        assert!(out.len() <= MAX_HOVER_BYTES + 5 && out.contains('…') && out.ends_with("\n```"));
        assert_eq!(truncate("短".into()), "短");
    }

    #[::core::prelude::v1::test]
    fn 距离在内为零_在外按直线() {
        let b = Bounds::new(point(px(10.), px(10.)), size(px(10.), px(10.)));
        assert_eq!(distance_to(b, point(px(15.), px(15.))), px(0.));
        assert_eq!(distance_to(b, point(px(23.), px(24.))), px(5.));
    }

    #[::core::prelude::v1::test]
    fn 数组用空行连接_全空白为空() {
        let arr = HoverContents::Array(vec![MarkedString::String("甲".into()), MarkedString::String("乙".into())]);
        assert_eq!(hover_markdown(&h(arr)).unwrap(), "甲\n\n乙");
        assert!(hover_markdown(&h(HoverContents::Scalar(MarkedString::String("  \n".into())))).is_none());
    }
}
