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
        let root = self.pinned.clone().or_else(|| self.wb.read(cx).active_project_path()).unwrap_or_default();
        let mut items: Vec<PaletteItem> = collect_files(std::path::Path::new(&root), 5000, &crate::prefs(cx).intel)
            .into_iter()
            .map(|f| PaletteItem { title: f.rsplit('/').next().unwrap_or(&f).to_string(), subtitle: f.clone(), group: Group::File, icon: IconName::FileText, action: PaletteAction::OpenFile(f) })
            .collect();
        // 最近打开的文件:无查询时单独成组列在最前(最多 8 个),有查询时文件列表也按最近优先
        let recent: Vec<String> = self.ide.read(cx).recent.iter().filter_map(|p| std::path::Path::new(p).strip_prefix(&root).ok().map(|r| r.to_string_lossy().to_string())).collect();
        items.sort_by_key(|i| recent.iter().position(|r| *r == i.subtitle).unwrap_or(usize::MAX));
        let top: Vec<PaletteItem> = recent
            .iter()
            .filter_map(|r| items.iter().find(|i| i.subtitle == *r))
            .take(8)
            .map(|i| PaletteItem { group: Group::Recent, ..i.clone() })
            .collect();
        items.splice(0..0, top);
        // 本项目的对话会话(IDE 固定在进入时的项目)
        {
            let wb = self.wb.read(cx);
            if let Some(p) = self.pinned_ix(wb).and_then(|ix| wb.projects.get(ix)) {
                for s in &p.sessions {
                    items.push(PaletteItem { title: s.title.clone(), subtitle: p.name.clone(), group: Group::Session, icon: IconName::MessageSquare, action: PaletteAction::OpenSession(0, s.id.clone()) });
                }
            }
        }
        let mut cmd = |title: &str, sub: &str, ic: IconName, action: PaletteAction| {
            items.push(PaletteItem { title: title.into(), subtitle: sub.into(), group: Group::Command, icon: ic, action });
        };
        cmd("新建临时记事本", "双击标签栏空白", IconName::FilePlus, PaletteAction::Ide("untitled"));
        cmd("保存文件", "⌘S", IconName::Save, PaletteAction::Ide("save"));
        cmd("全部保存", "", IconName::Save, PaletteAction::Ide("save-all"));
        cmd("关闭标签", "⌘W", IconName::X, PaletteAction::Ide("close-tab"));
        cmd("跳转到行…", "⌃G", IconName::ArrowDownToLine, PaletteAction::Ide("goto-line"));
        cmd("在项目中搜索", "⇧⌘F", IconName::Search, PaletteAction::Ide("search"));
        cmd("源代码管理", "⌃⇧G", IconName::GitBranch, PaletteAction::Ide("scm"));
        cmd("显示 / 隐藏终端", "⌘J", IconName::SquareTerminal, PaletteAction::Ide("terminal"));
        cmd("新建对话会话", "", IconName::Plus, PaletteAction::Ide("new-chat"));
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
                    PaletteAction::OpenSession(_, id) => this.activate_chat_tab(&id, window, cx),
                    PaletteAction::Ide(name) => this.run_ide_command(name, window, cx),
                    _ => {}
                }
                cx.notify();
            }
        }));
        self.quick = Some(palette);
        cx.notify();
    }

    /// 执行快速面板里的 IDE 内置命令。
    ///
    /// 参数:`name` 命令名(见 `PaletteAction::Ide`)。
    fn run_ide_command(&mut self, name: &str, window: &mut Window, cx: &mut Context<Self>) {
        match name {
            "untitled" => self.ide.update(cx, |v, cx| v.new_untitled(window, cx)),
            "save" => self.ide.update(cx, |v, cx| v.save(cx)),
            "save-all" => self.ide.update(cx, |v, cx| v.save_all(cx)),
            "close-tab" => self.ide.update(cx, |v, cx| v.close_active(window, cx)),
            "goto-line" => self.goto_line_dialog(window, cx),
            "search" => self.show_search(window, cx),
            "scm" => self.show_scm(window, cx),
            "terminal" => self.toggle_term(window, cx),
            "new-chat" => self.new_chat(window, cx),
            _ => {}
        }
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
                        crate::notice::warning(window, cx, "请输入行号,例如 42 或 42:7");
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
