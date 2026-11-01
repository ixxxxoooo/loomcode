//! 编辑器的悬停卡片(LSP hover):自己渲染,不用编辑器自带的气泡。
//!
//! 自带气泡的代码块带边框、零内边距,右侧还压着语言标签,和主题不搭。这里参考 Zed 的做法:
//! 卡片是弹出层底色 + 细边框 + 阴影,代码块是淡底圆角(无边框、有内边距、不显示语言标签),
//! 签名与文档之间用分隔线;卡片挂在符号上方(上方放不下才放下方),鼠标离开「符号 + 卡片」区域
//! 或点击外部即收起。语言服务器的悬停结果由 `lsp::LspProvider` 写进文档,再让编辑器自带气泡不出现(提供者返回 `None`)。
//!
//! @author ixxxxoooo

use crate::ide::{IdeDoc, IdeView};
use gpui_kit::component::ActiveTheme;
use gpui_kit::component::text::{TextView, TextViewStyle};
use gpui_kit::*;
use std::ops::Range;
use std::path::Path;

/// 卡片最大宽度。
const MAX_W: f32 = 520.;
/// 卡片最大高度。
const MAX_H: f32 = 320.;

/// 一张悬停卡片:触发的符号范围(字节偏移)与 Markdown 内容。
#[derive(Clone, Debug, PartialEq)]
pub struct HoverCard {
    pub range: Range<usize>,
    pub markdown: SharedString,
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
    (!text.trim().is_empty()).then_some(text)
}

impl IdeView {
    /// 写入 / 清除某文档的悬停卡片。
    pub(crate) fn set_hover_card(&mut self, path: &Path, card: Option<HoverCard>, cx: &mut Context<Self>) {
        if let Some(doc) = self.docs.get_mut(path).filter(|d| d.hover != card) {
            doc.hover = card;
            cx.notify();
        }
    }

    /// 当前卡片是否已经覆盖这个偏移(覆盖就不必重复向语言服务器请求)。
    pub(crate) fn hover_covers(&self, path: &Path, offset: usize) -> bool {
        self.docs.get(path).and_then(|d| d.hover.as_ref()).is_some_and(|c| c.range.contains(&offset))
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
    let above = at.origin.y > window.bounds().size.height * 0.35;
    let (pos, anchor) = if above {
        (point(at.origin.x, at.origin.y - px(4.)), Anchor::BottomLeft)
    } else {
        (point(at.origin.x, at.origin.y + at.size.height + px(4.)), Anchor::TopLeft)
    };
    let (view, path) = (view, path.to_path_buf());
    let guard = canvas(
        |b, _, _| b,
        move |card_bounds, _, window, _| {
            // 与触发符号合并成一块「保持展开」的区域(两者之间的缝隙也算在里面)
            let keep = card_bounds.union(&at);
            let (v1, p1) = (view.clone(), path.clone());
            window.on_mouse_event(move |e: &MouseMoveEvent, _, _, cx| {
                if !keep.contains(&e.position) {
                    let _ = v1.update(cx, |this, cx| this.set_hover_card(&p1, None, cx));
                }
            });
            let (v2, p2) = (view.clone(), path.clone());
            window.on_mouse_event(move |e: &MouseDownEvent, _, _, cx| {
                if !card_bounds.contains(&e.position) {
                    let _ = v2.update(cx, |this, cx| this.set_hover_card(&p2, None, cx));
                }
            });
        },
    )
    .absolute()
    .size_full();
    let style = TextViewStyle::default().paragraph_gap(rems(0.5)).code_block(
        StyleRefinement::default().bg(t.muted.opacity(0.55)).border_0().rounded(px(6.)).px_2().py_1p5().text_size(px(12.)),
    );
    // 给代码块一个空的「操作区」,库就不会在右上角画语言标签
    let body = TextView::markdown("ide-hover-md", card.markdown.clone()).style(style).selectable(true).code_block_actions(|_, _, _| div());
    Some(
        deferred(
            anchored().position(pos).anchor(anchor).snap_to_window_with_margin(px(8.)).child(
                div().id("ide-hover-card").relative().occlude().max_w(px(MAX_W)).max_h(px(MAX_H)).overflow_y_scroll()
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
    fn 数组用空行连接_全空白为空() {
        let arr = HoverContents::Array(vec![MarkedString::String("甲".into()), MarkedString::String("乙".into())]);
        assert_eq!(hover_markdown(&h(arr)).unwrap(), "甲\n\n乙");
        assert!(hover_markdown(&h(HoverContents::Scalar(MarkedString::String("  \n".into())))).is_none());
    }
}
