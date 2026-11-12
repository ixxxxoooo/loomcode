//! IDE 窗口里的「详情」:会话流里点计划 / 子 Agent / 任务 / 命令输出 / 改动时的落点。
//!
//! Agent 窗口把这些内容放在右栏标签里;IDE 里右栏是编辑器与文件树,所以:
//! - 计划、子 Agent(对话型内容)→ 盖在对话栏上,顶部「返回对话」,Esc 返回;
//! - 任务、命令输出(终端型内容)→ 底部面板,顶部「返回终端」;
//! - 改动 → 左栏源代码管理。
//!
//! 内容复用 `RightPanel` 的各个视图(`detail` 是一个不带文件树的 `RightPanel`,只接会话)。
//!
//! @author ixxxxoooo

use crate::ide_window::IdeWindow;
use crate::model::UiRequest;
use gpui_kit::assets::IconName;
use gpui_kit::component::button::{Button, ButtonVariants};
use gpui_kit::component::{ActiveTheme, Sizable, h_flex, v_flex};
use gpui_kit::*;

/// 详情的类别。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Detail {
    Plan,
    Subagent,
    Tasks,
    Run,
}

impl Detail {
    /// 在对话栏里显示(否则在底部面板)。
    pub fn in_chat(self) -> bool {
        matches!(self, Self::Plan | Self::Subagent)
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::Plan => "计划",
            Self::Subagent => "子 Agent",
            Self::Tasks => "后台任务",
            Self::Run => "命令输出",
        }
    }
}

impl IdeWindow {
    /// 让详情面板跟随当前会话(渲染时调用,内容没变不重复设置)。
    pub(crate) fn sync_detail(&self, cx: &mut Context<Self>) {
        let (model, root) = {
            let wb = self.wb.read(cx);
            let foreign = self.active_is_foreign(wb);
            (wb.active_model().cloned().filter(|_| !foreign), self.panel.read(cx).root.clone())
        };
        self.detail.update(cx, |d, cx| d.set_model_only(model, root, cx));
    }

    /// 处理会话流发来的界面请求(工作台判定 IDE 正在显示时转进来)。
    pub(crate) fn on_ui_request(&mut self, ev: UiRequest, window: &mut Window, cx: &mut Context<Self>) {
        match ev {
            UiRequest::OpenPlan(id) => {
                self.detail.update(cx, |d, cx| d.open_plan(Some(id), cx));
                self.show_detail(Detail::Plan, window, cx);
            }
            UiRequest::OpenSubagent(id) => {
                self.detail.update(cx, |d, cx| d.open_subagent(id, cx));
                self.show_detail(Detail::Subagent, window, cx);
            }
            UiRequest::OpenTasks(id) => {
                self.detail.update(cx, |d, cx| d.open_tasks(id, cx));
                self.show_detail(Detail::Tasks, window, cx);
            }
            UiRequest::OpenChanges => {
                self.show_scm(window, cx);
                self.panel.update(cx, |p, cx| p.set_changes_mode(crate::git_changes::ChangesMode::Claude, cx));
            }
            UiRequest::OpenChangeFile(path) => {
                self.show_scm(window, cx);
                self.panel.update(cx, |p, cx| { p.set_changes_mode(crate::git_changes::ChangesMode::Claude, cx); p.focus_change_file(path, cx); });
            }
            // 文件、画布、回合结束由工作台自己处理
            UiRequest::OpenFile(_) | UiRequest::OpenCanvas(_) | UiRequest::SessionSettled => {}
        }
    }

    /// 工具卡「在终端面板查看」:在底部面板显示这条命令的输出。
    pub(crate) fn show_run(&mut self, id: String, window: &mut Window, cx: &mut Context<Self>) {
        self.detail.update(cx, |d, cx| d.open_run(id, cx));
        self.show_detail(Detail::Run, window, cx);
    }

    fn show_detail(&mut self, kind: Detail, window: &mut Window, cx: &mut Context<Self>) {
        self.sync_detail(cx);
        self.detail_view = Some(kind);
        if kind.in_chat() {
            self.chat_open = true;
        } else {
            self.term_open = true;
        }
        window.focus(&self.focus, cx);
        cx.notify();
    }

    /// 关闭详情(回到对话 / 终端)。
    pub(crate) fn close_detail(&mut self, kind: Detail, window: &mut Window, cx: &mut Context<Self>) {
        if self.detail_view == Some(kind) {
            self.detail_view = None;
            window.focus(&self.focus, cx);
            cx.notify();
        }
    }

    /// 对话栏的详情页(计划 / 子 Agent):顶部「返回对话」条 + 内容。
    pub(crate) fn chat_detail(&self, kind: Detail, cx: &mut Context<Self>) -> AnyElement {
        let t = cx.theme().clone();
        v_flex().size_full().bg(t.background)
            .child(
                h_flex().h(px(30.)).flex_none().px_2().gap_1p5().items_center().border_b_1().border_color(t.border)
                    .child(Button::new("detail-back").xsmall().ghost().icon(IconName::ArrowLeft).label("返回对话").tooltip("返回对话(Esc)")
                        .on_click(cx.listener(move |this, _, window, cx| this.close_detail(kind, window, cx))))
                    .child(div().text_size(px(12.)).text_color(t.muted_foreground).child(kind.label())),
            )
            .child(div().flex_1().min_h_0().child(self.detail.clone()))
            .into_any_element()
    }

    /// 底部面板的详情页(任务 / 命令输出):顶部「返回终端」条 + 内容。
    pub(crate) fn bottom_detail(&self, kind: Detail, cx: &mut Context<Self>) -> AnyElement {
        let t = cx.theme().clone();
        v_flex().size_full().bg(t.background)
            .child(
                h_flex().h(px(30.)).flex_none().px_2().gap_1p5().items_center().border_b_1().border_color(t.border).bg(t.sidebar)
                    .child(Button::new("bdetail-back").xsmall().ghost().icon(IconName::SquareTerminal).label("终端").tooltip("返回终端")
                        .on_click(cx.listener(move |this, _, window, cx| this.close_detail(kind, window, cx))))
                    .child(div().text_size(px(12.)).text_color(t.foreground).child(kind.label())),
            )
            .child(div().flex_1().min_h_0().child(self.detail.clone()))
            .into_any_element()
    }
}
