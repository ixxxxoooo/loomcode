//! 终端页:字号、光标、回看行数、shell 与启动目录。字号与光标即时生效,其余作用于之后新开的终端。
//!
//! @author ixxxxoooo

use super::widgets::{group, note, page_header, row, rows, segmented, stepper, switch_row};
use crate::workbench::Workbench;
use gpui_kit::component::input::Input;
use gpui_kit::component::{Sizable, v_flex};
use gpui_kit::*;
use loom_config::CursorShape;

/// 渲染终端页。
pub fn render(wb: &Workbench, cx: &mut Context<Workbench>) -> AnyElement {
    let p = wb.settings.terminal.clone();
    v_flex().w_full()
        .child(page_header("终端", "右侧面板与 IDE 底部终端的外观与 shell 设置", div(), cx))
        .child(group("外观", rows![
            row("字号", "终端文字大小,行高自动按比例换算", stepper("term-font", p.font_size as i64, (9, 24, 1), " px", cx,
                |this, v, cx| this.update_settings(cx, |s| s.terminal.font_size = v as u8)), cx),
            row("光标形状", "", segmented("term-cursor", vec![(CursorShape::Block, "方块"), (CursorShape::Bar, "竖线"), (CursorShape::Underline, "下划线")], p.cursor, cx,
                |this, v, cx| this.update_settings(cx, |s| s.terminal.cursor = v)), cx),
            row("回看行数", "滚轮可回看的历史输出行数;对之后新开的终端生效", stepper("term-scroll", p.scrollback as i64, (1000, 100_000, 1000), " 行", cx,
                |this, v, cx| this.update_settings(cx, |s| s.terminal.scrollback = v as u32)), cx),
        ], cx))
        .child(group("Shell", rows![
            row("Shell 程序", "留空使用环境变量 $SHELL;文本框失焦或回车应用", div().w(gpui_kit::px(220.)).child(Input::new(wb.field("term_shell")).small()), cx),
            switch_row("term-login", "登录 shell", "以 -l 启动,会读取 .zprofile 等登录配置(PATH、nvm 等通常放在这里)", p.login_shell, cx,
                |this, v, cx| this.update_settings(cx, |s| s.terminal.login_shell = v)),
            switch_row("term-cwd", "在项目目录启动", "关闭后新终端从家目录开始", p.start_in_project, cx,
                |this, v, cx| this.update_settings(cx, |s| s.terminal.start_in_project = v)),
        ], cx))
        .child(note("Shell、登录与启动目录只影响之后新开的终端,已经打开的终端不会重启", cx))
        .into_any_element()
}
