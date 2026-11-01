//! 编辑器里的「光标处消息」(对标 VS Code 的 `MessageController`):找不到定义 / 实现、光标不在符号上、没有可用的重构这类
//! 结果不弹全局通知,而是在光标上方冒一个小气泡(放不下才放下方),带一个指向光标的小三角。
//! 样式取自 VS Code:`editorHoverWidget` 底色与字色、1px `#007acc` 边框、圆角 3、内边距 2px 4px、三角 8px。
//! 关闭条件同 VS Code:光标移动 / 滚动、按 Esc、鼠标移到消息所在行上下 3 行之外。没有定时器。
//!
//! @author ixxxxoooo

use crate::ide::{IdeDoc, IdeView};
use crate::ide_hover::view_anchor;
use gpui_kit::component::{ActiveTheme, v_flex};
use gpui_kit::*;

/// 气泡与小三角的边框色(`inputValidation.infoBorder`)。
const BORDER: u32 = 0x007accff;
/// 小三角的边长。
const ARROW: f32 = 8.;

/// 光标处的一条消息。
pub struct CursorMsg {
    pub text: SharedString,
    /// 出现时的 (选区起, 选区止, 视口首行),之后任何一项变了就关闭
    pub anchor: (usize, usize, usize),
}

impl IdeView {
    /// 在光标处显示一条消息(替换已有的)。
    pub(crate) fn show_cursor_message(&mut self, text: impl Into<SharedString>, cx: &mut Context<Self>) {
        let Some(doc) = self.tabs.active().and_then(|p| self.docs.get(p)) else { return };
        let anchor = view_anchor(doc.editor.read(cx));
        self.cursor_msg = Some(CursorMsg { text: text.into(), anchor });
        cx.notify();
    }

    /// 光标移动 / 滚动后关闭消息(编辑器 notify 时调用)。
    pub(crate) fn cursor_msg_check_moved(&mut self, cx: &App) {
        let moved = match (&self.cursor_msg, self.tabs.active().and_then(|p| self.docs.get(p))) {
            (Some(m), Some(doc)) => m.anchor != view_anchor(doc.editor.read(cx)),
            (Some(_), None) => true,
            _ => false,
        };
        if moved {
            self.cursor_msg = None;
        }
    }

    /// 关闭光标处消息。
    pub(crate) fn close_cursor_message(&mut self, cx: &mut Context<Self>) {
        if self.cursor_msg.take().is_some() {
            cx.notify();
        }
    }
}

/// 光标处消息元素;没有消息或光标不在视口里返回 `None`。
pub fn message_element(doc: &IdeDoc, msg: &CursorMsg, view: WeakEntity<IdeView>, cx: &App) -> Option<AnyElement> {
    let ed = doc.editor.read(cx);
    let off = ed.selected_range().start;
    let at = ed.range_to_bounds(&(off..off))?;
    let t = cx.theme();
    let (bg, fg, border) = (if t.is_dark() { rgba(0x252526ff) } else { rgba(0xf3f3f3ff) }, if t.is_dark() { rgba(0xccccccff) } else { rgba(0x616161ff) }, rgba(BORDER));
    let lh = f32::from(at.size.height);
    // 优先放在光标上方;上方放不下(光标靠近窗口顶部)才放下方
    let below = f32::from(at.origin.y) < 80.;
    let boxed = div().px(px(4.)).py(px(2.)).rounded(px(3.)).border_1().border_color(border).bg(bg).text_color(fg).text_size(px(12.)).child(msg.text.clone());
    let arrow = canvas(|_, _, _| {}, move |b, _, window, _| {
        let (x, y) = (b.origin.x + px(2.), b.origin.y);
        let mut p = PathBuilder::fill();
        if below {
            p.move_to(point(x, y + px(ARROW)));
            p.line_to(point(x + px(ARROW * 2.), y + px(ARROW)));
            p.line_to(point(x + px(ARROW), y));
        } else {
            p.move_to(point(x, y));
            p.line_to(point(x + px(ARROW * 2.), y));
            p.line_to(point(x + px(ARROW), y + px(ARROW)));
        }
        p.close();
        if let Ok(path) = p.build() {
            window.paint_path(path, border);
        }
    }).w(px(ARROW * 2.)).h(px(ARROW)).ml(px(2.));
    // Esc 关闭;鼠标离开消息所在行上下 3 行就关闭
    let line_y = f32::from(at.origin.y);
    let guard = canvas(|b, _, _| b, move |_, _, window, _| {
        let (v1, v2) = (view.clone(), view.clone());
        window.on_key_event(move |e: &KeyDownEvent, _, _, cx| {
            if e.keystroke.key == "escape" {
                let _ = v1.update(cx, |this, cx| this.close_cursor_message(cx));
            }
        });
        window.on_mouse_event(move |e: &MouseMoveEvent, _, _, cx| {
            let y = f32::from(e.position.y);
            if y < line_y - 3. * lh || y > line_y + 4. * lh {
                let _ = v2.update(cx, |this, cx| this.close_cursor_message(cx));
            }
        });
    }).absolute().size_full();
    let (pos, anchor) = if below { (point(at.origin.x - px(6.), at.origin.y + at.size.height), Anchor::TopLeft) } else { (point(at.origin.x - px(6.), at.origin.y), Anchor::BottomLeft) };
    let col = if below { v_flex().child(arrow).child(boxed) } else { v_flex().child(boxed).child(arrow) };
    Some(deferred(anchored().position(pos).anchor(anchor).child(div().relative().child(guard).child(col))).into_any_element())
}
