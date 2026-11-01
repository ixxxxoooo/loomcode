//! 右侧面板的「终端」标签:顶部是你自己的终端标签(+ 新建)与为单条 Claude 命令打开的只读标签,下面是内容。
//!
//! @author ixxxxoooo

use crate::right_panel::{RightPanel, TerminalRun};
use crate::term_store::{ProjectTerminals, TermSel};
use crate::ui::{MONO, empty_state, pill};
use crate::ansi;
use gpui_kit::assets::IconName;
use gpui_kit::component::button::{Button, ButtonVariants};
use gpui_kit::component::{ActiveTheme, Sizable, h_flex, v_flex};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;

/// 标签上最多显示的字符数,超出截断。
const MAX_TAB_CHARS: usize = 22;

/// 标签名超过上限时截断并加省略号。
pub(crate) fn clip_label(title: &str) -> String {
    if title.chars().count() <= MAX_TAB_CHARS {
        return title.to_string();
    }
    format!("{}…", title.chars().take(MAX_TAB_CHARS - 1).collect::<String>())
}

impl RightPanel {
    /// 新建一个终端并聚焦。
    pub(crate) fn new_terminal(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let hub = ProjectTerminals::hub(cx);
        let root = self.root.clone();
        let result = hub.update(cx, |h, cx| h.spawn(&root, None, cx));
        match result {
            Ok(id) => {
                let handle = hub.read(cx).tabs(&root).iter().find(|t| t.id == id).map(|t| t.view.read(cx).focus_handle());
                if let Some(h) = handle {
                    window.focus(&h, cx);
                }
            }
            Err(e) => loom_diag::error!("打开终端失败:{e}"),
        }
    }

    /// 在新终端里运行一条命令(调试变量 `LOOM_TERM_CMD` 用),命令结束后终端标签仍保留输出。
    pub fn run_command(&mut self, command: String, cx: &mut Context<Self>) {
        let hub = ProjectTerminals::hub(cx);
        let root = self.root.clone();
        let argv = vec!["/bin/sh".to_string(), "-c".to_string(), command];
        if let Err(e) = hub.update(cx, |h, cx| h.spawn(&root, Some(argv), cx)) {
            loom_diag::error!("运行命令失败:{e}");
        }
        cx.notify();
    }

    /// 为 Claude 的某条命令(工具调用 id)打开一个只读标签并选中,同时切到「终端」标签。
    pub fn open_run(&mut self, id: String, cx: &mut Context<Self>) {
        if !self.run_tabs.contains(&id) {
            self.run_tabs.push(id.clone());
        }
        self.active_run = Some(id);
        self.open_tab(crate::workbench::RightTab::Terminal, cx);
    }

    /// 关闭一条命令的只读标签。
    fn close_run(&mut self, id: &str, cx: &mut Context<Self>) {
        self.run_tabs.retain(|r| r != id);
        if self.active_run.as_deref() == Some(id) {
            self.active_run = None;
        }
        cx.notify();
    }

    /// 「终端」标签的整体视图。
    pub(crate) fn terminal_tab(&self, items: &[loom_core::TimelineItem], cx: &mut Context<Self>) -> AnyElement {
        let hub = ProjectTerminals::hub(cx);
        let root = self.root.clone();
        let folder = root.trim_end_matches('/').rsplit('/').next().unwrap_or("").to_string();
        let (active, tabs) = {
            let h = hub.read(cx);
            let tabs: Vec<(u64, String, Option<u32>, Entity<crate::terminal_view::TerminalView>)> = h.tabs(&root).iter().map(|t| {
                let v = t.view.read(cx);
                // 「前台进程 · 目录」,如 zsh · router-demo
                (t.id, v.label(&folder), v.exit_code(), t.view.clone())
            }).collect();
            (h.active(&root), tabs)
        };
        let runs = crate::right_panel::collect_terminal(items);
        let active_run = self.active_run.clone().filter(|id| runs.iter().any(|r| &r.id == id));
        let body = if let Some(run) = active_run.as_ref().and_then(|id| runs.iter().find(|r| &r.id == id)) {
            self.run_view(run, cx)
        } else {
            match tabs.iter().find(|t| TermSel::Term(t.0) == active).or(tabs.last()) {
                Some((_, _, _, view)) => div().size_full().child(view.clone()).into_any_element(),
                None => v_flex().size_full().items_center().justify_center().gap_3()
                    .child(empty_state(IconName::Terminal, "还没有终端", "点「新建终端」在项目目录里打开一个 shell", cx).h_auto().w_auto())
                    .child(Button::new("term-empty-new").small().primary().icon(IconName::Plus).label("新建终端")
                        .on_click(cx.listener(|this, _, window, cx| this.new_terminal(window, cx))))
                    .into_any_element(),
            }
        };
        v_flex().size_full()
            .child(self.term_strip(&root, if active_run.is_some() { TermSel::Nothing } else { active }, &tabs, &runs, active_run.as_deref(), &hub, cx))
            .child(div().flex_1().min_h_0().child(body))
            .into_any_element()
    }

    /// 标签条:终端 1 ✕ | 终端 2 ✕ | $ 命令 ✕(只读)| +
    fn term_strip(&self, root: &str, active: TermSel, tabs: &[(u64, String, Option<u32>, Entity<crate::terminal_view::TerminalView>)], runs: &[TerminalRun], active_run: Option<&str>, hub: &Entity<ProjectTerminals>, cx: &mut Context<Self>) -> impl IntoElement {
        let t = cx.theme().clone();
        let tab_btn = |id: SharedString, label: String, selected: bool| {
            div().id(id).h(px(26.)).px_2().flex().items_center().gap_1().rounded(px(6.)).cursor_pointer().text_size(px(12.))
                .when(selected, |d| d.bg(t.list_active).font_weight(FontWeight::MEDIUM))
                .when(!selected, |d| d.text_color(t.muted_foreground).hover(|d| d.bg(t.list_hover)))
                .child(label)
        };
        // 当前没有选中任何终端时(例如只开了命令标签),「选中」只看命令标签
        h_flex().h(px(34.)).px_2().gap_1().items_center().border_b_1().border_color(t.border)
            .children(tabs.iter().map(|(id, title, exit, _)| {
                let id = *id;
                let (hub_a, root_a) = (hub.clone(), root.to_string());
                let (hub_c, root_c) = (hub.clone(), root.to_string());
                let selected = active == TermSel::Term(id) || (active == TermSel::Nothing && active_run.is_none() && tabs.last().map(|x| x.0) == Some(id));
                let on_select = cx.listener(move |this, _, _, cx| {
                    this.active_run = None;
                    hub_a.update(cx, |h, cx| h.activate(&root_a, TermSel::Term(id), cx));
                });
                crate::term_tabs::term_tab("term", id, title, selected, exit.is_some(), 26., on_select, move |_, _, cx| hub_c.update(cx, |h, cx| h.close(&root_c, id, cx)), cx)
            }))
            .children(self.run_tabs.iter().filter_map(|rid| runs.iter().find(|r| &r.id == rid)).map(|run| {
                let rid = run.id.clone();
                let first = run.command.lines().next().unwrap_or("");
                let label = format!("$ {}", first.chars().take(MAX_TAB_CHARS).collect::<String>());
                let (sel, close) = (rid.clone(), rid.clone());
                tab_btn(SharedString::from(format!("run-tab-{rid}")), label, active_run == Some(rid.as_str()))
                    .on_click(cx.listener(move |this, _, _, cx| { this.active_run = Some(sel.clone()); cx.notify(); }))
                    .child(
                        Button::new(SharedString::from(format!("run-close-{rid}"))).xsmall().ghost().icon(IconName::X).tooltip("关闭")
                            .on_click(cx.listener(move |this, _, _, cx| { cx.stop_propagation(); this.close_run(&close, cx); })),
                    )
            }))
            .child(Button::new("term-new").xsmall().ghost().icon(IconName::Plus).tooltip("新建终端")
                .on_click(cx.listener(|this, _, window, cx| this.new_terminal(window, cx))))
    }

    /// 一条 Claude 命令的只读视图:命令、状态与输出。
    fn run_view(&self, run: &TerminalRun, cx: &mut Context<Self>) -> AnyElement {
        let t = cx.theme().clone();
        let shown: String = {
            let lines: Vec<&str> = run.output.lines().collect();
            let skip = lines.len().saturating_sub(2000);
            lines[skip..].join("\n")
        };
        let parsed = ansi::parse(&shown);
        v_flex().id("run-scroll").size_full().overflow_y_scroll().p_3().gap_2().font_family(MONO).text_size(px(12.))
            .child(
                h_flex().gap_2().items_center()
                    .child(div().text_color(t.primary).child("$"))
                    .child(div().flex_1().min_w_0().child(run.command.clone()))
                    .when(run.running, |d| d.child(pill("运行中", t.info, t.background)))
                    .when_some(run.exit_code, |d, code| d.child(pill(
                        if code == 0 { "成功".to_string() } else { format!("退出码 {code}") },
                        if code == 0 { t.success } else { t.danger },
                        t.background,
                    ))),
            )
            .when(!parsed.text.is_empty(), |d| d.child(StyledText::new(parsed.text).with_highlights(parsed.runs)))
            .into_any_element()
    }
}
