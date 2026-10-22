//! 脚手架验证:打开一个空窗口。
use gpui_kit::component::button::{Button, ButtonVariants};
use gpui_kit::*;

struct Hello;

impl Render for Hello {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        div()
            .flex()
            .flex_col()
            .size_full()
            .items_center()
            .justify_center()
            .gap_2()
            .child("UCode")
            .child(Button::new("hello").primary().label("OK").on_click(|_, _, _| println!("clicked")))
    }
}

fn main() {
    application().with_assets(assets::Assets).run(|cx| {
        init(cx);
        open_window(WindowOptions::default(), cx, |_, cx| cx.new(|_| Hello)).expect("打开窗口失败");
    });
}
