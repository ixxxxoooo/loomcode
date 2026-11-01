//! IDE 窗口的快速打开(⌘P / ⌘K / 标题栏搜索框)与跳转到行(⌃G)。
//! 快速打开复用命令面板:文件按相对路径模糊搜索,外加几条 IDE 命令。
//!
//! @author ixxxxoooo

use crate::ide_window::IdeWindow;
use crate::palette::{Group, Palette, PaletteAction, PaletteEvent, PaletteItem};
use crate::palette_search::collect_files;
use gpui_kit::assets::IconName;
use gpui_kit::component::WindowExt as _;
use gpui_kit::component::input::{Input, InputState};
use gpui_kit::component::notification::Notification;
use gpui_kit::*;
use std::cell::Cell;
use std::path::PathBuf;

impl IdeWindow {
    /// 打开 / 关闭快速打开面板。
    pub fn toggle_quick(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.quick.take().is_some() {
            self._quick_sub = None;
            window.focus(&self.focus, cx);
            cx.notify();
            return;
        }
        let root = self.wb.read(cx).active_project_path().unwrap_or_default();
        let mut items: Vec<PaletteItem> = collect_files(std::path::Path::new(&root), 5000, &crate::prefs(cx).intel)
            .into_iter()
            .map(|f| PaletteItem { title: f.rsplit('/').next().unwrap_or(&f).to_string(), subtitle: f.clone(), group: Group::File, icon: IconName::FileText, action: PaletteAction::OpenFile(f) })
            .collect();
        // 最近打开的文件排在最前(查询为空时一眼能看到)
        let recent: Vec<String> = self.ide.read(cx).recent.iter().filter_map(|p| std::path::Path::new(p).strip_prefix(&root).ok().map(|r| r.to_string_lossy().to_string())).collect();
        items.sort_by_key(|i| recent.iter().position(|r| *r == i.subtitle).unwrap_or(usize::MAX));
        let mut cmd = |title: &str, sub: &str, ic: IconName, action: PaletteAction| {
            items.push(PaletteItem { title: title.into(), subtitle: sub.into(), group: Group::Command, icon: ic, action });
        };
        cmd("切换到 Agent", "⇧⌘E", IconName::MessageSquare, PaletteAction::ToggleIde);
        cmd("显示 / 隐藏侧栏", "⌘B", IconName::PanelLeft, PaletteAction::ToggleLeft);
        cmd("显示 / 隐藏对话栏", "⌥⌘B", IconName::PanelRight, PaletteAction::ToggleRight);
        let palette = cx.new(|cx| Palette::new(items, window, cx));
        self._quick_sub = Some(cx.subscribe_in(&palette, window, |this, _, ev: &PaletteEvent, window, cx| match ev {
            PaletteEvent::Close => this.toggle_quick(window, cx),
            PaletteEvent::Run(action) => {
                let action = action.clone();
                this.toggle_quick(window, cx);
                match action {
                    PaletteAction::OpenFile(f) => this.open_path(&f, window, cx),
                    PaletteAction::ToggleLeft => this.left_open = !this.left_open,
                    PaletteAction::ToggleRight => this.chat_open = !this.chat_open,
                    PaletteAction::ToggleIde => this.back_to_agent(cx),
                    _ => {}
                }
                cx.notify();
            }
        }));
        self.quick = Some(palette);
        cx.notify();
    }

    /// 跳转到行:输入 `行` 或 `行:列`。
    pub fn goto_line_dialog(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if !self.ide.read(cx).has_active() {
            return;
        }
        let input = cx.new(|cx| InputState::new(window, cx).placeholder("行号,或 行:列"));
        let (view, first) = (self.ide.downgrade(), Cell::new(true));
        window.open_dialog(cx, move |dialog, window, cx| {
            if first.replace(false) {
                input.focus_handle(cx).focus(window, cx);
            }
            let (view, input) = (view.clone(), input.clone());
            dialog.title("跳转到行").child(Input::new(&input.clone())).footer(crate::ui::dialog_footer("跳转")).on_ok(move |_, window, cx| {
                let text = input.read(cx).value().to_string();
                match parse_line_col(&text) {
                    Some((l, c)) => {
                        let _ = view.update(cx, |v, cx| v.goto(l, c, window, cx));
                        true
                    }
                    None => {
                        window.push_notification(Notification::warning("请输入行号,例如 42 或 42:7"), cx);
                        false
                    }
                }
            })
        });
    }

    /// 在编辑器里打开路径并跳到指定行(项目内搜索结果使用)。
    pub fn open_at(&mut self, path: PathBuf, line: u32, window: &mut Window, cx: &mut Context<Self>) {
        self.panel.update(cx, |p, _| p.files.reveal(&path));
        self.ide.update(cx, |v, cx| {
            v.open(path, false, window, cx);
            v.goto(line, 1, window, cx);
        });
    }
}

/// 解析 `行` 或 `行:列`(1 起始,行必须 ≥ 1)。
pub fn parse_line_col(text: &str) -> Option<(u32, u32)> {
    let mut it = text.trim().splitn(2, [':', ',']);
    let line: u32 = it.next()?.trim().parse().ok().filter(|l| *l >= 1)?;
    let col = match it.next() {
        Some(c) => c.trim().parse().ok().filter(|c| *c >= 1)?,
        None => 1,
    };
    Some((line, col))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[::core::prelude::v1::test]
    fn 解析行列() {
        assert_eq!(parse_line_col("42"), Some((42, 1)));
        assert_eq!(parse_line_col(" 3:7 "), Some((3, 7)));
        assert_eq!(parse_line_col("0"), None);
        assert_eq!(parse_line_col("abc"), None);
    }
}
