//! 时间线上的各类卡片:待办、画布、改动汇总、提示(权限 / 提问 / 计划见 `dock_cards`、`ask_cards`,子 Agent 见 `subagent_card`)。

use crate::model::{SessionModel, UiRequest};
use crate::ui::{MONO, card, caption, icon};
use gpui_kit::assets::IconName;
use gpui_kit::component::button::{Button, ButtonVariants};
use gpui_kit::component::{ActiveTheme, Sizable, h_flex, v_flex};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;
use loom_core::*;

/// 待办清单卡。
pub fn render_todos(id: &str, items: &[TodoItem], cx: &App) -> AnyElement {
    let t = cx.theme();
    let done = items.iter().filter(|i| i.status == TodoStatus::Completed).count();
    card(cx)
        .p_3()
        .gap_1p5()
        .child(
            h_flex().gap_2().items_center()
                .child(icon(IconName::ListTodo, 14., t.muted_foreground))
                .child(div().text_size(px(13.)).font_weight(FontWeight::MEDIUM).child("待办"))
                .child(caption(format!("{done}/{}", items.len()), cx)),
        )
        .children(items.iter().enumerate().map(|(i, it)| {
            let (ic, color) = match it.status {
                TodoStatus::Completed => (IconName::CircleCheck, t.success),
                TodoStatus::InProgress => (IconName::CircleDot, t.primary),
                TodoStatus::Pending => (IconName::Circle, t.muted_foreground),
            };
            h_flex()
                .id(SharedString::from(format!("{id}-todo-{i}")))
                .gap_2()
                .items_center()
                .child(icon(ic, 14., color))
                .child(
                    div().text_size(px(13.))
                        .text_color(if it.status == TodoStatus::Completed { t.muted_foreground } else { t.foreground })
                        .when(it.status == TodoStatus::Completed, |d| d.line_through())
                        .child(it.content.clone()),
                )
        }))
        .into_any_element()
}

/// Canvas 卡片。
pub fn render_canvas(session: &Entity<SessionModel>, id: &str, title: &str, cx: &App) -> AnyElement {
    let t = cx.theme();
    let (s, cid) = (session.clone(), id.to_string());
    card(cx).id(SharedString::from(format!("canvas-{id}"))).p_3().cursor_pointer().hover(|d| d.bg(t.list_hover))
        .on_click(move |_, _, cx| { s.update(cx, |_, cx| cx.emit(UiRequest::OpenCanvas(cid.clone()))); })
        .child(h_flex().gap_2().items_center()
            .child(icon(IconName::LayoutDashboard, 16., t.primary))
            .child(v_flex().child(div().text_size(px(13.)).font_weight(FontWeight::MEDIUM).child(title.to_string())).child(caption("画布 · 点击打开", cx))))
        .into_any_element()
}

/// 本轮改动汇总。
pub fn render_changes(session: &Entity<SessionModel>, files: &[ChangedFile], cx: &App) -> AnyElement {
    let t = cx.theme();
    let (a, r): (u32, u32) = files.iter().fold((0, 0), |(a, r), f| (a + f.added, r + f.removed));
    let s = session.clone();
    card(cx).p_3().gap_1p5()
        .child(h_flex().gap_2().items_center()
            .child(icon(IconName::FileDiff, 15., t.muted_foreground))
            .child(div().text_size(px(13.)).font_weight(FontWeight::MEDIUM).child(format!("本轮改动 · {} 个文件", files.len())))
            .child(div().text_size(px(12.)).font_family(MONO).text_color(t.success).child(format!("+{a}")))
            .child(div().text_size(px(12.)).font_family(MONO).text_color(t.danger).child(format!("−{r}")))
            .child(div().flex_1())
            .child(Button::new("changes-open").small().ghost().label("查看改动")
                .on_click(move |_, _, cx| { s.update(cx, |_, cx| cx.emit(UiRequest::OpenChanges)); })))
        .children(files.iter().map(|f| {
            h_flex().gap_2().text_size(px(12.)).font_family(MONO)
                .child(div().flex_1().truncate().child(f.path.clone()))
                .child(div().text_color(t.success).child(format!("+{}", f.added)))
                .child(div().text_color(t.danger).child(format!("−{}", f.removed)))
        }))
        .into_any_element()
}

/// 提示行。
pub fn render_notice(level: NoticeLevel, text: &str, cx: &App) -> AnyElement {
    let t = cx.theme();
    let (ic, color) = match level {
        NoticeLevel::Info => (IconName::Info, t.info),
        NoticeLevel::Warning => (IconName::TriangleAlert, t.warning),
        NoticeLevel::Error => (IconName::CircleX, t.danger),
    };
    h_flex().w_full().gap_2().px_3().py_1p5().rounded(px(8.)).bg(t.muted).items_start()
        .child(div().mt(px(2.)).child(icon(ic, 14., color)))
        // min_w_0:长英文报错在卡片内折行,不再溢出右侧
        .child(div().flex_1().min_w_0().text_size(px(13.)).text_color(color).child(text.to_string()))
        .into_any_element()
}
