//! 「改动」分段里的「Git 更改」:`git status` 列表,点开看相对 HEAD 的 diff。
//! 所有 git 与文件读取都在后台线程里做。
//!
//! @author ixxxxoooo

use crate::diff::{self, DiffLine};
use crate::right_panel::{PanelEvent, RightPanel, split_path};
use crate::ui::{MONO, empty_state, icon};
use gpui_kit::assets::IconName;
use gpui_kit::component::button::{Button, ButtonVariants};
use gpui_kit::component::{ActiveTheme, Sizable, h_flex, v_flex};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;
use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use loom_core::TimelineItem;
use loom_core::git::{self, StatusEntry};

/// 超过这个大小的文件不算 diff(只显示在列表里)。
const MAX_DIFF_BYTES: u64 = 1024 * 1024;

/// 「改动」分段的两种内容。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ChangesMode {
    /// Claude 在本会话里改过的文件
    #[default]
    Claude,
    /// 工作区相对 HEAD 的全部未提交改动
    Git,
}

type FileDiff = (Vec<DiffLine>, usize, usize);

/// 「Git 更改」的状态。
#[derive(Default)]
pub struct GitPane {
    pub mode: ChangesMode,
    /// 仓库根目录
    top: Option<String>,
    entries: Vec<StatusEntry>,
    error: Option<String>,
    /// 至少成功或失败地读取过一次
    loaded: bool,
    loading: bool,
    /// 展开的文件 → 它的 diff(`None` 表示还在算)
    open: HashMap<String, Option<FileDiff>>,
    /// 上次刷新时时间线的条数:变了说明 Claude 又动了文件,需要重新读取
    seen_items: usize,
}

impl GitPane {
    /// 初始状态。调试变量 `LOOM_CHANGES=git` 直接进入 Git 模式,`git-open` 还会自动展开第一个文件。
    pub fn new() -> Self {
        let mode = if std::env::var("LOOM_CHANGES").is_ok_and(|v| v.starts_with("git")) { ChangesMode::Git } else { ChangesMode::Claude };
        Self { mode, ..Default::default() }
    }

    /// 切换项目 / 会话后丢掉旧数据,下次显示时重新读取。
    pub fn reset(&mut self) {
        self.top = None;
        self.entries.clear();
        self.error = None;
        self.loaded = false;
        self.open.clear();
    }
}

impl RightPanel {
    /// 「改动」分段:顶部切换条 + 对应内容。
    pub(crate) fn changes_tab(&self, items: &[TimelineItem], cx: &mut Context<Self>) -> AnyElement {
        let body = match self.git.mode {
            ChangesMode::Claude => self.changes_view(items, cx),
            ChangesMode::Git => self.git_view(cx),
        };
        v_flex().size_full().child(self.changes_switch(cx)).child(div().flex_1().min_h_0().child(body)).into_any_element()
    }

    fn changes_switch(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let t = cx.theme().clone();
        let btn = |id: &'static str, label: &'static str, mode: ChangesMode, cx: &mut Context<Self>| {
            let on = self.git.mode == mode;
            div().id(id).h(px(24.)).px_2().flex().items_center().rounded(px(6.)).cursor_pointer().text_size(px(12.))
                .when(on, |d| d.bg(t.list_active).font_weight(FontWeight::MEDIUM))
                .when(!on, |d| d.text_color(t.muted_foreground).hover(|d| d.bg(t.list_hover)))
                .on_click(cx.listener(move |this, _, _, cx| this.set_changes_mode(mode, cx)))
                .child(label)
        };
        h_flex().h(px(34.)).px_2().gap_1().items_center().border_b_1().border_color(t.border)
            .child(btn("chg-mode-claude", "Claude 改动", ChangesMode::Claude, cx))
            .child(btn("chg-mode-git", "Git 更改", ChangesMode::Git, cx))
    }

    /// 切换「Claude 改动 / Git 更改」;切到 Git 时刷新状态。
    pub(crate) fn set_changes_mode(&mut self, mode: ChangesMode, cx: &mut Context<Self>) {
        self.git.mode = mode;
        if mode == ChangesMode::Git {
            self.refresh_git(cx);
        }
        cx.notify();
    }

    /// 在显示「Git 更改」时由渲染调用:第一次显示、或 Claude 又产生了新的时间线条目(可能改了文件)就刷新。
    pub(crate) fn git_auto_refresh(&mut self, items: usize, cx: &mut Context<Self>) {
        if !self.git.loaded || items != self.git.seen_items {
            self.git.seen_items = items;
            self.refresh_git(cx);
        }
    }

    /// 后台重新读取 `git status`;已展开的文件会重新计算 diff。
    pub(crate) fn refresh_git(&mut self, cx: &mut Context<Self>) {
        if self.git.loading {
            return;
        }
        self.git.loading = true;
        let root = self.root.clone();
        cx.spawn(async move |this, cx| {
            let result = cx
                .background_spawn(async move {
                    let top = git::repo_root(&root).ok_or_else(|| "当前目录不在 Git 仓库里".to_string())?;
                    git::status(&top).map(|entries| (top, entries))
                })
                .await;
            let _ = this.update(cx, |this, cx| {
                this.git.loading = false;
                this.git.loaded = true;
                match result {
                    Ok((top, entries)) => {
                        let alive: HashSet<&str> = entries.iter().map(|e| e.path.as_str()).collect();
                        this.git.open.retain(|k, _| alive.contains(k.as_str()));
                        let reopen: Vec<String> = this.git.open.keys().cloned().collect();
                        this.git.top = Some(top);
                        this.git.entries = entries;
                        this.git.error = None;
                        for rel in reopen {
                            this.load_git_diff(rel, cx);
                        }
                        if std::env::var("LOOM_CHANGES").is_ok_and(|v| v == "git-open") && this.git.open.is_empty() {
                            if let Some(first) = this.git.entries.first().map(|e| e.path.clone()) {
                                this.load_git_diff(first, cx);
                            }
                        }
                    }
                    Err(e) => {
                        this.git.top = None;
                        this.git.entries.clear();
                        this.git.open.clear();
                        this.git.error = Some(e);
                    }
                }
                cx.notify();
            });
        })
        .detach();
    }

    /// 后台计算某个文件相对 HEAD 的 diff。
    fn load_git_diff(&mut self, rel: String, cx: &mut Context<Self>) {
        let Some(top) = self.git.top.clone() else { return };
        self.git.open.insert(rel.clone(), None);
        cx.spawn(async move |this, cx| {
            let r = rel.clone();
            let diff = cx
                .background_spawn(async move {
                    let abs = PathBuf::from(&top).join(&r);
                    if std::fs::metadata(&abs).is_ok_and(|m| m.len() > MAX_DIFF_BYTES) {
                        return (Vec::new(), 0, 0);
                    }
                    let old = git::file_at_head(&top, &r).unwrap_or_default();
                    let new = std::fs::read_to_string(&abs).unwrap_or_default();
                    diff::compute(&old, &new)
                })
                .await;
            let _ = this.update(cx, |this, cx| {
                // 期间用户可能已经收起了这个文件
                if this.git.open.contains_key(&rel) {
                    this.git.open.insert(rel, Some(diff));
                    cx.notify();
                }
            });
        })
        .detach();
    }

    fn toggle_git_diff(&mut self, rel: &str, cx: &mut Context<Self>) {
        if self.git.open.remove(rel).is_none() {
            self.load_git_diff(rel.to_string(), cx);
        }
        cx.notify();
    }

    fn git_view(&self, cx: &mut Context<Self>) -> AnyElement {
        let t = cx.theme().clone();
        if let Some(e) = &self.git.error {
            return empty_state(IconName::GitBranch, "无法读取 Git 状态", e, cx).into_any_element();
        }
        if !self.git.loaded {
            return div().p_4().text_size(px(12.)).text_color(t.muted_foreground).child("正在读取…").into_any_element();
        }
        let header = h_flex().h(px(34.)).px_3().gap_2().items_center().text_size(px(12.))
            .child(div().flex_1().text_color(t.muted_foreground).child(format!("{} 个更改", self.git.entries.len())))
            .child(Button::new("git-refresh").xsmall().ghost().icon(IconName::RefreshCw).tooltip("刷新")
                .on_click(cx.listener(|this, _, _, cx| this.refresh_git(cx))));
        if self.git.entries.is_empty() {
            return v_flex().size_full().child(header).child(div().flex_1().child(empty_state(IconName::GitBranch, "工作区很干净", "没有未提交的改动", cx))).into_any_element();
        }
        let top = self.git.top.clone().unwrap_or_default();
        v_flex().size_full().child(header).child(
            v_flex().id("git-scroll").flex_1().min_h_0().overflow_y_scroll().px_2().pb_3().gap_1p5()
                .children(self.git.entries.iter().enumerate().map(|(ix, e)| {
                    let open = self.git.open.get(&e.path);
                    let (dir, name) = split_path(&e.path);
                    let label = e.label();
                    let color = match label {
                        'M' => t.warning,
                        'D' | '!' => t.danger,
                        'A' | 'U' | 'R' | 'C' => t.success,
                        _ => t.muted_foreground,
                    };
                    let (rel, abs) = (e.path.clone(), PathBuf::from(&top).join(&e.path));
                    let can_open = label != 'D' && abs.is_file();
                    v_flex().w_full().rounded(px(8.)).border_1().border_color(t.border)
                        .child(
                            h_flex().id(SharedString::from(format!("git-{ix}"))).h(px(30.)).px_2().gap_1p5().items_center().cursor_pointer()
                                .hover(|d| d.bg(t.list_hover))
                                .on_click(cx.listener(move |this, _, _, cx| this.toggle_git_diff(&rel, cx)))
                                .child(icon(if open.is_some() { IconName::ChevronDown } else { IconName::ChevronRight }, 12., t.muted_foreground))
                                .child(div().w(px(14.)).text_size(px(11.)).font_family(MONO).font_weight(FontWeight::BOLD).text_color(color).child(label.to_string()))
                                .child(div().text_size(px(12.5)).font_weight(FontWeight::MEDIUM).child(name))
                                .child(div().flex_1().min_w_0().truncate().text_size(px(11.)).text_color(t.muted_foreground).child(dir))
                                .when(can_open, |d| d.child(
                                    Button::new(SharedString::from(format!("git-open-{ix}"))).xsmall().ghost().icon(IconName::ExternalLink).tooltip("打开文件")
                                        .on_click(cx.listener(move |_, _, _, cx| { cx.stop_propagation(); cx.emit(PanelEvent::OpenFile(abs.clone())); })),
                                )),
                        )
                        .when_some(open, |d, state| d.child(div().p_1().child(match state {
                            None => div().p_2().text_size(px(11.)).text_color(t.muted_foreground).child("正在计算差异…"),
                            Some((lines, _, _)) if lines.is_empty() => div().p_2().text_size(px(11.)).text_color(t.muted_foreground).child("没有可显示的文本差异(二进制、过大或空文件)"),
                            Some((lines, _, _)) => diff::render(lines, 400, cx),
                        })))
                }))
        ).into_any_element()
    }
}
