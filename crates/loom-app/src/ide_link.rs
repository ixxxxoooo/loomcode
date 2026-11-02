//! ⌘ 点击跳转到定义的补强(对标 VS Code / Zed)。
//!
//! 编辑器库只在「⌘ 按着时鼠标移动」才请求定义并画下划线,点击时也只认那份已就绪的结果,于是有两个缺口:
//! 1. 鼠标已经停在符号上再按 ⌘:没有鼠标移动事件,不出下划线,此时点击什么也不发生。
//!    VS Code 的 `LinkDetector` 与 Zed 的 `modifiers_changed → update_hovered_link` 都会在修饰键变化时用鼠标当前位置重新检测。
//!    这里在 ⌘ 按下 / 松开时,用鼠标当前位置合成一次鼠标移动,让编辑器库照常走它自己的悬停定义流程。
//! 2. 结果还没回来(服务器首次分析较慢)就点击:Zed 的做法是点击(鼠标抬起)时,若没有现成的链接,直接对点击位置发
//!    一次「转到定义」(`mouse_up → go_to_definition`)。这里同样:⌘ 点击落在一个空选区上、且库没有处理它(没有跳转,光标仍在点击处),
//!    就派发 `GoToDefinition`,由库请求定义并在结果回来时(光标没动才)跳转。
//!
//! @author ixxxxoooo

use crate::ide::IdeView;
use gpui_kit::component::input::GoToDefinition;
use gpui_kit::*;
use std::cell::Cell;
use std::ops::Range;
use std::path::Path;

thread_local! {
    /// 上一次观察到的「⌘ 是否按着」,只在变化时合成鼠标移动
    static CMD_DOWN: Cell<bool> = const { Cell::new(false) };
}

/// ⌘ 点击是否可能是「转到定义」:主修饰键、没有 Alt / Shift、单击。
pub fn is_cmd_click(m: Modifiers, click_count: usize) -> bool {
    m.secondary() && !m.alt && !m.shift && click_count == 1
}

/// 盖住编辑区的透明画布:⌘ 按下 / 松开且鼠标在编辑区内时,合成一次鼠标移动(见模块说明第 1 点)。
pub fn modifier_probe() -> impl IntoElement {
    canvas(
        |b, _, _| b,
        move |bounds, _, window, _| {
            window.on_key_event(move |e: &ModifiersChangedEvent, phase, window, cx| {
                if !phase.bubble() {
                    return;
                }
                let now = e.modifiers.secondary() && !e.modifiers.alt;
                if CMD_DOWN.with(|c| c.replace(now)) == now {
                    return;
                }
                let pos = window.mouse_position();
                if !bounds.contains(&pos) {
                    return;
                }
                let modifiers = e.modifiers;
                // 在本次按键事件之后再派发,避免在事件分发中重入
                window.defer(cx, move |window, cx| {
                    window.dispatch_event(PlatformInput::MouseMove(MouseMoveEvent { position: pos, pressed_button: None, modifiers }), cx);
                });
            });
        },
    )
    .absolute()
    .size_full()
}

impl IdeView {
    /// 鼠标按下(冒泡到编辑区容器时,编辑器自己已处理完):记下 ⌘ 单击后的选区。
    pub(crate) fn link_mouse_down(&mut self, path: &Path, e: &MouseDownEvent, sel: Range<usize>) {
        self.cmd_click = is_cmd_click(e.modifiers, e.click_count).then(|| (path.to_path_buf(), sel));
    }

    /// 鼠标抬起:若是 ⌘ 单击、落在空选区、光标仍在点击处(库没有处理成跳转),就请求「转到定义」。
    pub(crate) fn link_mouse_up(&mut self, path: &Path, still: Range<usize>, window: &mut Window, cx: &mut Context<Self>) {
        let Some((p0, r0)) = self.cmd_click.take() else { return };
        if p0 != path || !r0.is_empty() || still != r0 || self.tabs.active() != Some(path) {
            return;
        }
        window.defer(cx, |window, cx| window.dispatch_action(Box::new(GoToDefinition), cx));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[::core::prelude::v1::test]
    fn 只有单击主修饰键才算() {
        let cmd = Modifiers { platform: true, ..Default::default() };
        assert!(is_cmd_click(cmd, 1));
        assert!(!is_cmd_click(cmd, 2));
        assert!(!is_cmd_click(Modifiers::default(), 1));
        assert!(!is_cmd_click(Modifiers { shift: true, ..cmd }, 1));
        assert!(!is_cmd_click(Modifiers { alt: true, ..cmd }, 1));
    }
}
