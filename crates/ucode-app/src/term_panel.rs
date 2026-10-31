//! 右侧面板的「终端」分段:顶部标签条(Agent 命令 + 若干终端 + 新建),下面是内容。
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
const MAX_TAB_CHARS: usize = 14;

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
            Err(e) => eprintln!("打开终端失败:{e}"),
        }
    }

    /// 在新终端里运行一条命令(调试变量 `UCODE_TERM_CMD` 用),命令结束后终端标签仍保留输出。
    pub fn run_command(&mut self, command: String, cx: &mut Context<Self>) {
        let hub = ProjectTerminals::hub(cx);
        let root = self.root.clone();
        let argv = vec!["/bin/sh".to_string(), "-c".to_string(), command];
        if let Err(e) = hub.update(cx, |h, cx| h.spawn(&root, Some(argv), cx)) {
            eprintln!("运行命令失败:{e}");
        }
        cx.notify();
    }

    /// 「终端」分段的整体视图。
    pub(crate) fn terminal_tab(&self, items: &[ucode_core::TimelineItem], cx: &mut Context<Self>) -> AnyElement {
        let hub = ProjectTerminals::hub(cx);
        let root = self.root.clone();
        let (active, tabs) = {
            let h = hub.read(cx);
            let tabs: Vec<(u64, String, Option<u32>, Entity<crate::terminal_view::TerminalView>)> = h.tabs(&root).iter().map(|t| {
                let v = t.view.read(cx);
                // 程序设置了标题就用标题,否则用「终端 N」
                let title = Some(v.title()).filter(|s| !s.trim().is_empty()).unwrap_or_else(|| t.name.clone());
                (t.id, title, v.exit_code(), t.view.clone())
            }).collect();
            (h.active(&root), tabs)
        };
        let body = match active {
            TermSel::Term(id) => match tabs.iter().find(|t| t.0 == id) {
                Some((_, _, _, view)) => div().size_full().child(view.clone()).into_any_element(),
                None => self.agent_runs_view(items, cx),
            },
            TermSel::Agent => self.agent_runs_view(items, cx),
        };
        v_flex().size_full()
            .child(self.term_strip(&root, active, &tabs, &hub, cx))
            .child(div().flex_1().min_h_0().child(body))
            .into_any_element()
    }

    /// 标签条:Agent 命令 | 终端 1 ✕ | 终端 2 ✕ | +
    fn term_strip(&self, root: &str, active: TermSel, tabs: &[(u64, String, Option<u32>, Entity<crate::terminal_view::TerminalView>)], hub: &Entity<ProjectTerminals>, cx: &mut Context<Self>) -> impl IntoElement {
        let t = cx.theme().clone();
        let tab_btn = |id: SharedString, label: String, selected: bool| {
            div().id(id).h(px(26.)).px_2().flex().items_center().gap_1().rounded(px(6.)).cursor_pointer().text_size(px(12.))
                .when(selected, |d| d.bg(t.list_active).font_weight(FontWeight::MEDIUM))
                .when(!selected, |d| d.text_color(t.muted_foreground).hover(|d| d.bg(t.list_hover)))
                .child(label)
        };
        let agent = {
            let (hub, root) = (hub.clone(), root.to_string());
            tab_btn("term-tab-agent".into(), "Agent 命令".into(), active == TermSel::Agent)
                .on_click(move |_, _, cx| hub.update(cx, |h, cx| h.activate(&root, TermSel::Agent, cx)))
        };
        h_flex().h(px(34.)).px_2().gap_1().items_center().border_b_1().border_color(t.border)
            .child(agent)
            .children(tabs.iter().map(|(id, title, exit, _)| {
                let id = *id;
                let label: String = title.chars().take(MAX_TAB_CHARS).collect();
                let (hub_a, root_a) = (hub.clone(), root.to_string());
                let (hub_c, root_c) = (hub.clone(), root.to_string());
                tab_btn(SharedString::from(format!("term-tab-{id}")), label, active == TermSel::Term(id))
                    .when(exit.is_some(), |d| d.text_color(t.muted_foreground))
                    .on_click(move |_, _, cx| hub_a.update(cx, |h, cx| h.activate(&root_a, TermSel::Term(id), cx)))
                    .child(
                        Button::new(SharedString::from(format!("term-close-{id}"))).xsmall().ghost().icon(IconName::X).tooltip("关闭终端")
                            .on_click(move |_, _, cx| { cx.stop_propagation(); hub_c.update(cx, |h, cx| h.close(&root_c, id, cx)); }),
                    )
            }))
            .child(Button::new("term-new").xsmall().ghost().icon(IconName::Plus).tooltip("新建终端")
                .on_click(cx.listener(|this, _, window, cx| this.new_terminal(window, cx))))
    }

    /// 「Agent 命令」:Claude 跑过的命令与输出(只读记录)。
    fn agent_runs_view(&self, items: &[ucode_core::TimelineItem], cx: &mut Context<Self>) -> AnyElement {
        let t = cx.theme().clone();
        let runs: Vec<TerminalRun> = crate::right_panel::collect_terminal(items);
        if runs.is_empty() {
            return empty_state(IconName::Terminal, "还没有命令记录", "Claude 执行的命令与输出会显示在这里;点右上角「+」新建一个终端", cx).into_any_element();
        }
        v_flex().id("terminal-scroll").size_full().overflow_y_scroll().p_2().gap_2()
            .children(runs.into_iter().map(|run| {
                let shown: String = {
                    let lines: Vec<&str> = run.output.lines().collect();
                    let skip = lines.len().saturating_sub(200);
                    lines[skip..].join("\n")
                };
                let parsed = ansi::parse(&shown);
                v_flex().w(relative(1.)).rounded(px(8.)).border_1().border_color(t.border).bg(t.muted).p_2().gap_1().font_family(MONO).text_size(px(12.))
                    .child(
                        h_flex().gap_2().items_center()
                            .child(div().text_color(t.primary).child("$"))
                            .child(div().flex_1().min_w_0().child(run.command.lines().next().unwrap_or("").to_string()))
                            .when(run.running, |d| d.child(pill("运行中", t.info, t.background)))
                            .when_some(run.exit_code, |d, code| d.child(pill(
                                if code == 0 { "成功".to_string() } else { format!("退出码 {code}") },
                                if code == 0 { t.success } else { t.danger },
                                t.background,
                            ))),
                    )
                    .when(!parsed.text.is_empty(), |d| d.child(StyledText::new(parsed.text).with_highlights(parsed.runs)))
            }))
            .into_any_element()
    }
}
