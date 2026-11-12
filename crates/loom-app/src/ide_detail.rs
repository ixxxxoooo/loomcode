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
    /// 问题(错误 / 警告)列表,不走详情面板,自己画
    Problems,
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
            Self::Problems => "问题",
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

    /// 打开底部的「问题」列表(状态栏的错误 / 警告数点击)。
    pub(crate) fn show_problems(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.show_detail(Detail::Problems, window, cx);
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
        let body = if kind == Detail::Problems { self.problems_view(cx) } else { self.detail.clone().into_any_element() };
        v_flex().size_full().bg(t.background)
            .child(
                h_flex().h(px(30.)).flex_none().px_2().gap_1p5().items_center().border_b_1().border_color(t.border).bg(t.sidebar)
                    .child(Button::new("bdetail-back").xsmall().ghost().icon(IconName::SquareTerminal).label("终端").tooltip("返回终端")
                        .on_click(cx.listener(move |this, _, window, cx| this.close_detail(kind, window, cx))))
                    .child(div().text_size(px(12.)).text_color(t.foreground).child(kind.label())),
            )
            .child(div().flex_1().min_h_0().child(body))
            .into_any_element()
    }

    /// 「问题」列表:按文件分组,错误 / 警告图标 + 消息 + 来源 + 行:列,点击跳转。
    fn problems_view(&self, cx: &mut Context<Self>) -> AnyElement {
        const MAX_ROWS: usize = 500;
        let t = cx.theme().clone();
        let root = self.panel.read(cx).root.clone();
        let groups = self.ide.read(cx).all_problems();
        if groups.is_empty() {
            return crate::ui::empty_state(IconName::CircleCheck, "没有问题", "已打开的文件里没有错误或警告(需要语言服务器已就绪)", cx).into_any_element();
        }
        let mut shown = 0usize;
        let mut col = v_flex().id("problems-scroll").size_full().overflow_y_scroll().py_1();
        for (gi, (path, items)) in groups.into_iter().enumerate() {
            if shown >= MAX_ROWS {
                break;
            }
            let rel = path.strip_prefix(&root).unwrap_or(&path).display().to_string();
            let name = path.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default();
            col = col.child(
                h_flex().h(px(24.)).px_3().gap_1p5().items_center().text_size(px(12.))
                    .child(crate::file_icons::file_icon_img(&name, false, false, 14.))
                    .child(div().text_color(t.foreground).child(name))
                    .child(div().min_w_0().truncate().text_color(t.muted_foreground).child(rel))
                    .child(div().flex_none().text_size(px(11.)).text_color(t.muted_foreground).child(items.len().to_string())),
            );
            for (ii, p) in items.iter().enumerate() {
                if shown >= MAX_ROWS {
                    break;
                }
                shown += 1;
                let (target, line, hover) = (path.clone(), p.line, t.list_hover);
                let (ic, color) = if p.error { (IconName::CircleX, t.danger) } else { (IconName::TriangleAlert, t.warning) };
                col = col.child(
                    h_flex().id(SharedString::from(format!("prob-{gi}-{ii}"))).min_h(px(22.)).pl(px(34.)).pr_3().gap_2().items_start().cursor_pointer().text_size(px(12.))
                        .hover(move |d| d.bg(hover))
                        .on_click(cx.listener(move |this, _, window, cx| this.open_at(target.clone(), line, window, cx)))
                        .child(div().mt(px(3.)).flex_none().child(crate::ui::icon(ic, 12., color)))
                        .child(div().flex_1().min_w_0().text_color(t.foreground).child(p.message.lines().next().unwrap_or("").to_string()))
                        .child(div().flex_none().text_color(t.muted_foreground).child(if p.source.is_empty() { String::new() } else { p.source.clone() }))
                        .child(div().flex_none().text_color(t.muted_foreground).child(format!("[{}:{}]", p.line, p.col))),
                );
            }
        }
        col.into_any_element()
    }
}
