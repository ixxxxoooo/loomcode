//! 设置导航:选中分类并记住、搜索回车跳转、上下方向键切换、Esc 清搜索 / 返回。
//!
//! @author ixxxxoooo

use super::SettingsTab;
use crate::workbench::Workbench;
use gpui_kit::*;

impl Workbench {
    /// 选中一个设置分类,并把它记为「上次停留」。
    pub fn select_settings_tab(&mut self, tab: SettingsTab, cx: &mut Context<Self>) {
        if self.settings_tab != tab {
            self.settings_tab = tab;
            self.update_settings(cx, |s| s.last_settings_tab = tab.key().to_string());
        }
        cx.notify();
    }

    /// 当前搜索词命中的分类(按导航顺序)。
    fn visible_settings_tabs(&self, cx: &App) -> Vec<SettingsTab> {
        let q = self.field_value("settings_search", cx);
        SettingsTab::all().filter(|t| t.matches(&q)).collect()
    }

    /// 清空设置搜索框。
    fn clear_settings_search(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.set_field("settings_search", String::new(), window, cx);
    }

    /// 设置页的键盘处理(挂在工作台根上,仅 `Page::Settings` 时调用)。
    ///
    /// 返回:事件是否已处理。上下键只在没有其它输入框聚焦时切换分类,免得抢走输入框的光标移动。
    pub fn settings_key(&mut self, ev: &KeyDownEvent, window: &mut Window, cx: &mut Context<Self>) -> bool {
        let key = ev.keystroke.key.as_str();
        let search_focused = self.settings_forms.is_some() && self.field("settings_search").read(cx).focus_handle(cx).is_focused(window);
        let nothing_focused = self.focus.is_focused(window);
        match key {
            "escape" => {
                if !self.field_value("settings_search", cx).is_empty() {
                    self.clear_settings_search(window, cx);
                } else {
                    self.close_settings(window, cx);
                }
                window.focus(&self.focus, cx);
                true
            }
            "enter" if search_focused => {
                if let Some(first) = self.visible_settings_tabs(cx).first().copied() {
                    self.select_settings_tab(first, cx);
                    self.clear_settings_search(window, cx);
                    window.focus(&self.focus, cx);
                }
                true
            }
            "up" | "down" if nothing_focused || search_focused => {
                let tabs = self.visible_settings_tabs(cx);
                if tabs.is_empty() {
                    return true;
                }
                let at = tabs.iter().position(|t| *t == self.settings_tab);
                let next = match (at, key) {
                    (Some(i), "down") => tabs[(i + 1) % tabs.len()],
                    (Some(i), _) => tabs[(i + tabs.len() - 1) % tabs.len()],
                    (None, _) => tabs[0],
                };
                self.select_settings_tab(next, cx);
                true
            }
            _ => false,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[::core::prelude::v1::test]
    fn 搜索命中分类名与关键词() {
        assert!(SettingsTab::Terminal.matches("终端"));
        assert!(SettingsTab::Terminal.matches("SHELL"));
        assert!(SettingsTab::Network.matches("代理"));
        assert!(SettingsTab::Editor.matches("minimap"));
        assert!(!SettingsTab::Editor.matches("代理"));
        assert!(SettingsTab::About.matches(""));
    }

    #[::core::prelude::v1::test]
    fn 键名唯一且可反查() {
        let all: Vec<_> = SettingsTab::all().collect();
        for t in &all {
            assert_eq!(SettingsTab::from_name(t.key()), Some(*t));
        }
        let mut keys: Vec<_> = all.iter().map(|t| t.key()).collect();
        keys.sort_unstable();
        keys.dedup();
        assert_eq!(keys.len(), all.len());
    }
}
