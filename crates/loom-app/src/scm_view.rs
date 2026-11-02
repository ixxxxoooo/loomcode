//! 源代码管理视图的界面(对标 VS Code 的 Source Control):顶部分支与工具栏、提交消息框与主按钮、
//! 「合并更改 / 暂存的更改 / 更改」分组,文件行悬停出现 打开 / 放弃 / 暂存 按钮,点文件行展开 diff,
//! 支持列表与目录树两种视图。状态与操作在 `scm.rs`。
//!
//! @author ixxxxoooo

use crate::diff;
use crate::file_icons::file_icon_img;
use crate::scm::{After, ScmView};
use crate::scm_model::{self, Group, Item, Row, kind_title};
use crate::ui::{MONO, empty_state, icon};
use gpui_kit::assets::IconName;
use gpui_kit::component::button::{Button, ButtonVariants};
use gpui_kit::component::input::Textarea;
use gpui_kit::component::menu::{DropdownMenu as _, PopupMenu, PopupMenuItem};
use gpui_kit::component::{ActiveTheme, Disableable as _, Sizable, h_flex, v_flex};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;
use loom_core::git::StatusEntry;
use std::path::{Path, PathBuf};

/// 状态字母的颜色(对标 VS Code:修改偏黄、新增 / 未跟踪偏绿、删除红、重命名偏蓝、冲突红)。
fn kind_color(kind: char, cx: &App) -> Hsla {
    let t = cx.theme();
    match kind {
        'M' => t.warning,
        'A' | 'U' => t.success,
        'D' | '!' => t.danger,
        'R' | 'C' => t.info,
        _ => t.muted_foreground,
    }
}

/// 一个小的图标按钮(悬停操作区用):固定 20×20,点击不冒泡到整行。
fn act_btn<F>(id: SharedString, ic: IconName, tip: &'static str, cx: &mut Context<ScmView>, f: F) -> Button
where
    F: Fn(&mut ScmView, &mut Window, &mut Context<ScmView>) + 'static,
{
    Button::new(id).xsmall().ghost().icon(ic).tooltip(tip).on_click(cx.listener(move |this, _, window, cx| {
        cx.stop_propagation();
        f(this, window, cx);
    }))
}

impl ScmView {
    /// 顶部:分支与同步计数 + 视图 / 刷新 / 更多。
    fn toolbar(&self, cx: &mut Context<Self>) -> AnyElement {
        let t = cx.theme().clone();
        let branch = self.sync.branch.clone().unwrap_or_else(|| "游离 HEAD".into());
        let (ahead, behind, has_up) = (self.sync.ahead, self.sync.behind, self.sync.upstream.is_some());
        let weak = cx.entity().downgrade();
        let tree = self.tree;
        let stashes: Vec<(usize, String)> = self.graph.stashes.iter().map(|s| (s.index, s.message.clone())).collect();
        let more = move |menu: PopupMenu, _: &mut Window, _: &mut Context<PopupMenu>| {
            let item = |label: &'static str, w: WeakEntity<ScmView>, f: fn(&mut ScmView, &mut Window, &mut Context<ScmView>)| {
                PopupMenuItem::new(label).on_click(move |_, window, cx| {
                    let _ = w.update(cx, |this, cx| f(this, window, cx));
                })
            };
            let mut menu = menu.item(item("拉取", weak.clone(), |t, w, cx| t.pull(w, cx)))
                .item(item("推送", weak.clone(), |t, w, cx| t.push(w, cx)))
                .item(item("同步", weak.clone(), |t, w, cx| t.sync_changes(w, cx)))
                .item(item("提取", weak.clone(), |t, w, cx| t.fetch(w, cx)))
                .separator()
                .item(item("暂存所有更改", weak.clone(), |t, w, cx| t.stage_group(Group::Changes, w, cx)))
                .item(item("取消全部暂存", weak.clone(), |t, w, cx| t.unstage_all(w, cx)))
                .item(item("放弃所有更改", weak.clone(), |t, w, cx| {
                    let all: Vec<StatusEntry> = t.groups.changes.iter().map(|i| i.entry.clone()).collect();
                    t.confirm_discard(all, w, cx)
                }))
                .separator()
                .item(item("储藏所有更改", weak.clone(), |t, w, cx| t.stash_push(w, cx)))
                .separator()
                .item(PopupMenuItem::new("以树形视图查看").checked(tree).on_click({
                    let w = weak.clone();
                    move |_, _, cx| {
                        let _ = w.update(cx, |this, cx| this.toggle_tree(cx));
                    }
                }))
                .item(item("刷新", weak.clone(), |t, _, cx| t.refresh(cx)));
            // 储藏列表(最多 5 条):应用 / 弹出 / 删除
            for (i, msg) in stashes.iter().take(5).cloned() {
                let (w1, w2, w3) = (weak.clone(), weak.clone(), weak.clone());
                let name = |s: &str| format!("{s} stash@{{{i}}}:{}", msg.chars().take(24).collect::<String>());
                menu = menu
                    .item(PopupMenuItem::new(name("应用")).on_click(move |_, window, cx| { let _ = w1.update(cx, |t, cx| t.stash_apply(i, window, cx)); }))
                    .item(PopupMenuItem::new(name("弹出")).on_click(move |_, window, cx| { let _ = w2.update(cx, |t, cx| t.stash_pop(i, window, cx)); }))
                    .item(PopupMenuItem::new(name("删除")).on_click(move |_, window, cx| { let _ = w3.update(cx, |t, cx| t.stash_drop(i, window, cx)); }));
            }
            menu
        };
        h_flex().h(px(32.)).w_full().min_w_0().px_3().gap_1().items_center().flex_none()
            .child(
                h_flex().id("scm-branch").min_w_0().h(px(24.)).px_1().gap_1().items_center().rounded(px(5.))
                    .when(self.pick_branch, |d| d.cursor_pointer().hover(|d| d.bg(t.list_hover)).on_click(cx.listener(|_, _, _, cx| cx.emit(crate::scm::ScmEvent::PickBranch))))
                    .child(icon(IconName::GitBranch, 13., t.muted_foreground))
                    .child(div().min_w_0().truncate().text_size(px(12.5)).font_weight(FontWeight::MEDIUM).child(branch))
                    .when(self.pick_branch, |d| d.child(icon(IconName::ChevronDown, 11., t.muted_foreground))),
            )
            .when(has_up && (ahead > 0 || behind > 0), |d| {
                d.child(div().ml_1().text_size(px(11.)).font_family(MONO).text_color(t.muted_foreground).child(format!("{}{}", if behind > 0 { format!("↓{behind} ") } else { String::new() }, if ahead > 0 { format!("↑{ahead}") } else { String::new() })))
            })
            .child(div().flex_1())
            .child(act_btn("scm-view".into(), if tree { IconName::List } else { IconName::ListTree }, if tree { "以列表查看" } else { "以树形视图查看" }, cx, |this, _, cx| this.toggle_tree(cx)))
            .child(act_btn("scm-refresh".into(), IconName::RefreshCw, "刷新", cx, |this, _, cx| this.refresh(cx)))
            .child(Button::new("scm-more").xsmall().ghost().icon(IconName::Ellipsis).tooltip("更多操作").dropdown_menu(more))
            .into_any_element()
    }

    /// 提交消息框 + 主按钮(提交 / 同步更改 / 发布分支)与旁边的下拉。
    fn commit_box(&self, cx: &mut Context<Self>) -> AnyElement {
        let t = cx.theme().clone();
        let (staged, changes) = (self.groups.staged.len(), self.groups.changes.len() + self.groups.conflicts.len());
        let nothing = staged == 0 && changes == 0 && !self.amend;
        let s = &self.sync;
        let (label, _) = scm_model::commit_labels(staged, changes, self.amend, s.branch.as_deref());
        // 没有可提交的内容时,主按钮变成同步 / 发布(与 VS Code 一致)
        enum Primary {
            Commit,
            Sync,
            Publish,
        }
        let primary = if !nothing {
            Primary::Commit
        } else if s.upstream.is_some() && (s.ahead > 0 || s.behind > 0) {
            Primary::Sync
        } else if s.upstream.is_none() && s.has_remote && s.has_commits && s.branch.is_some() {
            Primary::Publish
        } else {
            Primary::Commit
        };
        let (text, ic, disabled) = match (&primary, self.busy) {
            (_, Some(b)) => (format!("{b}中…"), IconName::LoaderCircle, true),
            (Primary::Commit, _) => (label, IconName::Check, nothing),
            (Primary::Sync, _) => (format!("同步更改 {}{}", if s.behind > 0 { format!("↓{} ", s.behind) } else { String::new() }, if s.ahead > 0 { format!("↑{}", s.ahead) } else { String::new() }).trim().to_string(), IconName::RefreshCw, false),
            (Primary::Publish, _) => ("发布分支".to_string(), IconName::CloudUpload, false),
        };
        let weak = cx.entity().downgrade();
        let amend = self.amend;
        let menu = move |menu: PopupMenu, _: &mut Window, _: &mut Context<PopupMenu>| {
            let item = |label: &'static str, w: WeakEntity<ScmView>, f: fn(&mut ScmView, &mut Window, &mut Context<ScmView>)| {
                PopupMenuItem::new(label).on_click(move |_, window, cx| {
                    let _ = w.update(cx, |this, cx| f(this, window, cx));
                })
            };
            menu.item(item("提交", weak.clone(), |t, w, cx| t.commit(After::Nothing, w, cx)))
                .item(item("提交并推送", weak.clone(), |t, w, cx| t.commit(After::Push, w, cx)))
                .item(item("提交并同步", weak.clone(), |t, w, cx| t.commit(After::Sync, w, cx)))
                .separator()
                .item(PopupMenuItem::new("修改上次提交").checked(amend).on_click({
                    let w = weak.clone();
                    move |_, window, cx| {
                        let _ = w.update(cx, |this, cx| this.toggle_amend(window, cx));
                    }
                }))
        };
        let main = Button::new("scm-commit").small().primary().w_full().icon(ic).label(text).disabled(disabled)
            .on_click(cx.listener(move |this, _, window, cx| match primary {
                Primary::Commit => this.commit(After::Nothing, window, cx),
                Primary::Sync => this.sync_changes(window, cx),
                Primary::Publish => this.push(window, cx),
            }));
        v_flex().w_full().min_w_0().px_3().pb_2().gap_2().flex_none()
            .when(self.amend, |d| d.child(div().text_size(px(11.)).text_color(t.warning).child("正在修改上次提交(再次选择「修改上次提交」可取消)")))
            .child(
                h_flex().w_full().rounded(px(6.)).border_1().border_color(t.border).pl_2().pr_1().py_1p5().gap_1().items_start()
                    .child(div().flex_1().min_w_0().child(Textarea::new(&self.message).appearance(false).bordered(false)))
                    .child(
                        Button::new("scm-generate").xsmall().ghost().icon(if self.generating { IconName::LoaderCircle } else { IconName::Sparkles })
                            .tooltip(if self.generating { "正在生成提交说明…" } else { "根据改动与最近提交风格生成提交说明" })
                            .disabled(self.generating || self.busy.is_some() || self.groups.distinct() == 0)
                            .on_click(cx.listener(|this, _, window, cx| this.generate_message(window, cx))),
                    ),
            )
            .child(
                h_flex().w_full().gap_px().items_center()
                    .child(div().flex_1().min_w_0().child(main))
                    .child(Button::new("scm-commit-more").small().primary().icon(IconName::ChevronDown).disabled(self.busy.is_some()).tooltip("更多提交操作").dropdown_menu(menu)),
            )
            .into_any_element()
    }

    /// 一个分组:标题行(折叠箭头、组名、悬停操作、数量)+ 文件行。
    fn group_view(&self, g: Group, cx: &mut Context<Self>) -> Option<AnyElement> {
        let items = self.groups.of(g);
        if items.is_empty() {
            return None;
        }
        let t = cx.theme().clone();
        let gkey = format!("g:{}", g.key());
        let open = !self.collapsed.contains(&gkey);
        let group_name = SharedString::from(format!("scm-gh-{}", g.key()));
        let actions = match g {
            Group::Staged => h_flex().gap_0p5()
                .child(act_btn(format!("scm-un-all-{}", g.key()).into(), IconName::Minus, "取消全部暂存", cx, |this, w, cx| this.unstage_all(w, cx))),
            Group::Changes => h_flex().gap_0p5()
                .child(act_btn(format!("scm-di-all-{}", g.key()).into(), IconName::Undo, "放弃所有更改", cx, |this, w, cx| {
                    let all: Vec<StatusEntry> = this.groups.changes.iter().map(|i| i.entry.clone()).collect();
                    this.confirm_discard(all, w, cx)
                }))
                .child(act_btn(format!("scm-st-all-{}", g.key()).into(), IconName::Plus, "暂存所有更改", cx, move |this, w, cx| this.stage_group(Group::Changes, w, cx))),
            Group::Conflicts => h_flex().gap_0p5()
                .child(act_btn(format!("scm-st-all-{}", g.key()).into(), IconName::Plus, "暂存所有(标记为已解决)", cx, move |this, w, cx| this.stage_group(Group::Conflicts, w, cx))),
        };
        let header = h_flex().id(SharedString::from(format!("scm-gh-row-{}", g.key()))).group(group_name.clone()).h(px(26.)).px_2().gap_1().items_center().cursor_pointer()
            .hover(|d| d.bg(t.list_hover))
            .on_click(cx.listener(move |this, _, _, cx| this.toggle_collapsed(gkey.clone(), cx)))
            .child(icon(if open { IconName::ChevronDown } else { IconName::ChevronRight }, 12., t.muted_foreground))
            .child(div().flex_1().min_w_0().truncate().text_size(px(11.5)).font_weight(FontWeight::SEMIBOLD).child(g.title()))
            .child(div().invisible().group_hover(group_name, |d| d.visible()).child(actions))
            .child(div().min_w(px(18.)).h(px(16.)).px_1().flex().items_center().justify_center().rounded(px(8.)).bg(t.muted).text_size(px(10.5)).text_color(t.muted_foreground).child(items.len().to_string()));
        let rows = if open { scm_model::rows(items, self.tree, g, &self.collapsed) } else { Vec::new() };
        let tree = self.tree;
        Some(
            v_flex().w_full()
                .child(header)
                .children(rows.into_iter().map(|row| match row {
                    Row::Dir { path, name, depth, open, files } => self.dir_row(g, &path, &name, depth, open, files, cx),
                    Row::File { ix, depth } => self.file_row(&items[ix], depth, !tree, cx),
                }))
                .into_any_element(),
        )
    }

    /// 目录行(树模式):折叠箭头、文件夹图标、名称;悬停可对整个目录暂存 / 取消暂存 / 放弃。
    fn dir_row(&self, g: Group, path: &str, name: &str, depth: usize, open: bool, files: usize, cx: &mut Context<Self>) -> AnyElement {
        let t = cx.theme().clone();
        let key = format!("{}:{path}", g.key());
        let group_name = SharedString::from(format!("scm-dr-{key}"));
        let entries = scm_model::files_under(self.groups.of(g), path);
        let actions = {
            let e = entries.clone();
            match g {
                Group::Staged => h_flex().gap_0p5().child(act_btn(format!("scm-du-{key}").into(), IconName::Minus, "取消暂存此文件夹", cx, move |this, w, cx| this.unstage(e.clone(), w, cx))),
                Group::Changes => {
                    let (e1, e2) = (e.clone(), e);
                    h_flex().gap_0p5()
                        .child(act_btn(format!("scm-dd-{key}").into(), IconName::Undo, "放弃此文件夹的更改", cx, move |this, w, cx| this.confirm_discard(e1.clone(), w, cx)))
                        .child(act_btn(format!("scm-ds-{key}").into(), IconName::Plus, "暂存此文件夹", cx, move |this, w, cx| this.stage(e2.clone(), w, cx)))
                }
                Group::Conflicts => h_flex(),
            }
        };
        let toggle = key.clone();
        h_flex().id(SharedString::from(format!("scm-dir-{key}"))).group(group_name.clone()).h(px(24.)).pl(px(8. + depth as f32 * 12.)).pr_2().gap_1p5().items_center().cursor_pointer()
            .hover(|d| d.bg(t.list_hover))
            .on_click(cx.listener(move |this, _, _, cx| this.toggle_collapsed(toggle.clone(), cx)))
            .child(icon(if open { IconName::ChevronDown } else { IconName::ChevronRight }, 12., t.muted_foreground))
            .child(file_icon_img(name, true, open, 14.))
            .child(div().flex_1().min_w_0().truncate().text_size(px(12.5)).child(name.to_string()))
            .child(div().invisible().group_hover(group_name, |d| d.visible()).child(actions))
            .child(div().text_size(px(10.5)).text_color(t.muted_foreground).child(files.to_string()))
            .into_any_element()
    }

    /// 文件行:图标、文件名(按状态着色)、目录(列表模式)、悬停操作、状态字母;点击展开 diff,双击打开文件。
    fn file_row(&self, it: &Item, depth: usize, show_dir: bool, cx: &mut Context<Self>) -> AnyElement {
        let t = cx.theme().clone();
        let key = it.key();
        let (dir, name) = crate::right_panel::split_path(it.path());
        let color = kind_color(it.kind, cx);
        let selected = self.selected.as_deref() == Some(key.as_str());
        let group_name = SharedString::from(format!("scm-fr-{key}"));
        let entry = it.entry.clone();
        let exists = self.top.as_ref().is_some_and(|top| Path::new(top).join(it.path()).is_file());
        let (path_open, rel) = (exists, it.path().to_string());
        let mut actions = h_flex().gap_0p5();
        if path_open && it.kind != 'D' {
            let rel = rel.clone();
            actions = actions.child(act_btn(format!("scm-open-{key}").into(), IconName::ExternalLink, "打开文件", cx, move |this, _, cx| this.open_file(&rel, true, cx)));
        }
        match it.group {
            Group::Staged => {
                let e = entry.clone();
                actions = actions.child(act_btn(format!("scm-unstage-{key}").into(), IconName::Minus, "取消暂存更改", cx, move |this, w, cx| this.unstage(vec![e.clone()], w, cx)));
            }
            Group::Changes => {
                let (e1, e2) = (entry.clone(), entry.clone());
                let tip = if entry.untracked() { "删除此未跟踪文件" } else { "放弃更改" };
                actions = actions
                    .child(act_btn(format!("scm-discard-{key}").into(), IconName::Undo, tip, cx, move |this, w, cx| this.confirm_discard(vec![e1.clone()], w, cx)))
                    .child(act_btn(format!("scm-stage-{key}").into(), IconName::Plus, "暂存更改", cx, move |this, w, cx| this.stage(vec![e2.clone()], w, cx)));
            }
            Group::Conflicts => {
                let e = entry.clone();
                actions = actions.child(act_btn(format!("scm-resolve-{key}").into(), IconName::Plus, "暂存更改(标记为已解决)", cx, move |this, w, cx| this.stage(vec![e.clone()], w, cx)));
            }
        }
        let open_diff = self.open.get(&key);
        let lang_path = PathBuf::from(it.path());
        let click_key = key.clone();
        let double_rel = rel.clone();
        let dim = it.kind == 'D';
        let kind = it.kind;
        let has_dir = show_dir && !dir.is_empty();
        v_flex().w_full()
            .child(
                h_flex().id(SharedString::from(format!("scm-file-{key}"))).group(group_name.clone()).h(px(24.)).pl(px(8. + depth as f32 * 12. + if self.tree { 16. } else { 0. })).pr_2().gap_1p5().items_center().cursor_pointer()
                    .when(selected, |d| d.bg(t.list_active))
                    .when(!selected, |d| d.hover(|d| d.bg(t.list_hover)))
                    .on_click(cx.listener(move |this, ev: &ClickEvent, _, cx| {
                        if this.open_on_click {
                            // IDE:单击在编辑区直接看左右对比(预览标签),双击固定该标签
                            this.selected = Some(click_key.clone());
                            this.open_diff_tab(&click_key, ev.click_count() >= 2, cx);
                            cx.notify();
                        } else if ev.click_count() >= 2 && exists {
                            this.open_file(&double_rel, true, cx);
                        } else {
                            this.toggle_diff(&click_key, cx);
                        }
                    }))
                    .child(file_icon_img(&name, false, false, 14.))
                    .child(div().flex_none().max_w(px(180.)).truncate().text_size(px(12.5)).text_color(color).when(dim, |d| d.line_through()).child(name))
                    .child(if has_dir { div().flex_1().min_w_0().truncate().text_size(px(11.)).text_color(t.muted_foreground).child(dir) } else { div().flex_1() })
                    .child(div().invisible().group_hover(group_name, |d| d.visible()).child(actions))
                    .child(
                        div().id(SharedString::from(format!("scm-kind-{key}"))).w(px(14.)).flex_none().text_size(px(11.)).font_family(MONO).font_weight(FontWeight::BOLD).text_color(color)
                            .tooltip(move |window, cx| gpui_kit::component::tooltip::Tooltip::new(kind_title(kind)).build(window, cx))
                            .child(it.kind.to_string()),
                    ),
            )
            .when_some(open_diff, |d, state| {
                d.child(div().mx_2().mb_1().rounded(px(6.)).border_1().border_color(t.border).overflow_hidden().child(match state {
                    None => div().p_2().text_size(px(11.)).text_color(t.muted_foreground).child("正在计算差异…"),
                    Some((lines, _, _)) if lines.is_empty() => div().p_2().text_size(px(11.)).text_color(t.muted_foreground).child("没有可显示的文本差异(二进制、过大或空文件)"),
                    Some((lines, _, _)) => diff::render_flat(lines, 400, crate::file_tree::language_for(&lang_path), cx),
                }))
            })
            .into_any_element()
    }
}

impl Render for ScmView {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        self.touch();
        let t = cx.theme().clone();
        if let Some(e) = &self.error {
            return empty_state(IconName::GitBranch, "无法读取源代码管理状态", e, cx).into_any_element();
        }
        if !self.loaded {
            return div().p_4().text_size(px(12.)).text_color(t.muted_foreground).child("正在读取…").into_any_element();
        }
        let groups: Vec<AnyElement> = [Group::Conflicts, Group::Staged, Group::Changes].into_iter().filter_map(|g| self.group_view(g, cx)).collect();
        let clean = groups.is_empty();
        v_flex().size_full().min_w_0().overflow_hidden().bg(t.background)
            .child(self.toolbar(cx))
            .child(self.commit_box(cx))
            .child(
                v_flex().id("scm-scroll").flex_1().min_h_0().overflow_y_scroll().pb_3()
                    .children(groups)
                    .when(clean, |d| d.child(div().px_3().py_4().text_size(px(12.)).text_color(t.muted_foreground).child("没有检测到更改")))
                    .children(self.graph_section(cx)),
            )
            .into_any_element()
    }
}
