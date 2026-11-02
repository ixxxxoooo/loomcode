//! 分支选择器(对标 VS Code 的「切换分支」快速选择):搜索本地 / 远程分支,回车或点击切换,
//! 输入不存在的名字可直接新建并切换。数据在后台读取,选中后由宿主执行实际的 git 操作。
//!
//! @author ixxxxoooo

use crate::ui::icon;
use gpui_kit::assets::IconName;
use gpui_kit::component::input::{Input, InputEvent, InputState, MoveDown, MoveUp};
use gpui_kit::component::{ActiveTheme, h_flex, v_flex};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;
use loom_core::git::{self, Branch};

/// 选择器发给宿主的事件。
pub enum BranchEvent {
    /// 切换到已有分支;`remote` 为真表示远程分支(`origin/x`,需建立跟踪分支)
    Switch { name: String, remote: bool },
    /// 新建并切换
    Create(String),
    Close,
}

impl EventEmitter<BranchEvent> for BranchPicker {}

/// 列表行的种类。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RowKind {
    Local,
    Remote,
    Create,
}

/// 一行。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Row {
    pub kind: RowKind,
    pub name: String,
    pub current: bool,
}

/// 按查询构造显示行:本地(当前分支在前)→ 远程 → 「新建」。纯函数,不分大小写做子串匹配。
pub fn build_rows(local: &[Branch], remote: &[String], query: &str) -> Vec<Row> {
    let q = query.trim().to_lowercase();
    let hit = |name: &str| q.is_empty() || name.to_lowercase().contains(&q);
    let mut rows: Vec<Row> = local.iter().filter(|b| hit(&b.name)).map(|b| Row { kind: RowKind::Local, name: b.name.clone(), current: b.current }).collect();
    rows.sort_by_key(|r| !r.current);
    rows.extend(remote.iter().filter(|n| hit(n)).map(|n| Row { kind: RowKind::Remote, name: n.clone(), current: false }));
    let name = query.trim();
    let exists = local.iter().any(|b| b.name == name) || remote.iter().any(|r| r == name);
    if !name.is_empty() && !exists && git::validate_branch_name(name).is_ok() {
        rows.push(Row { kind: RowKind::Create, name: name.to_string(), current: false });
    }
    rows
}

pub struct BranchPicker {
    input: Entity<InputState>,
    local: Vec<Branch>,
    remote: Vec<String>,
    loading: bool,
    rows: Vec<Row>,
    sel: usize,
    _sub: Subscription,
}

impl BranchPicker {
    /// 创建并开始后台读取分支;`dir` 为仓库内任意目录。
    pub fn new(dir: String, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let input = cx.new(|cx| InputState::new(window, cx).placeholder("搜索分支,或输入新分支名后回车创建"));
        let sub = cx.subscribe_in(&input, window, |this: &mut Self, _, ev: &InputEvent, _, cx| match ev {
            InputEvent::Change => this.refilter(cx),
            InputEvent::PressEnter { .. } => this.accept(cx),
            _ => {}
        });
        input.update(cx, |i, cx| i.focus(window, cx));
        cx.spawn(async move |this, cx| {
            let (local, remote) = cx
                .background_spawn(async move {
                    let local = git::branches(&dir).unwrap_or_default();
                    let remote = git::remote_branches(&dir, &local);
                    (local, remote)
                })
                .await;
            let _ = this.update(cx, |this, cx| {
                this.local = local;
                this.remote = remote;
                this.loading = false;
                this.refilter(cx);
            });
        })
        .detach();
        Self { input, local: Vec::new(), remote: Vec::new(), loading: true, rows: Vec::new(), sel: 0, _sub: sub }
    }

    /// 预填查询(调试用)。
    pub fn set_query(&mut self, q: &str, window: &mut Window, cx: &mut Context<Self>) {
        self.input.update(cx, |i, cx| i.set_value(q.to_string(), window, cx));
        self.refilter(cx);
    }

    fn refilter(&mut self, cx: &mut Context<Self>) {
        let q = self.input.read(cx).value().to_string();
        self.rows = build_rows(&self.local, &self.remote, &q);
        self.sel = 0;
        cx.notify();
    }

    fn accept(&mut self, cx: &mut Context<Self>) {
        if let Some(row) = self.rows.get(self.sel).cloned() {
            self.choose(&row, cx);
        }
    }

    fn choose(&mut self, row: &Row, cx: &mut Context<Self>) {
        match row.kind {
            // 点当前分支:什么都不用做,直接关闭
            RowKind::Local if row.current => cx.emit(BranchEvent::Close),
            RowKind::Local => cx.emit(BranchEvent::Switch { name: row.name.clone(), remote: false }),
            RowKind::Remote => cx.emit(BranchEvent::Switch { name: row.name.clone(), remote: true }),
            RowKind::Create => cx.emit(BranchEvent::Create(row.name.clone())),
        }
    }

    fn move_sel(&mut self, delta: isize, cx: &mut Context<Self>) {
        if self.rows.is_empty() {
            return;
        }
        let n = self.rows.len() as isize;
        self.sel = ((self.sel as isize + delta).rem_euclid(n)) as usize;
        cx.notify();
    }
}

impl Render for BranchPicker {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let t = cx.theme().clone();
        let this = cx.entity();
        let section = |label: &'static str| div().px_2().pt_1p5().pb_0p5().text_size(px(11.)).text_color(t.muted_foreground).child(label);
        let mut list = v_flex().id("branch-list").flex_1().min_h_0().overflow_y_scroll().p_1();
        if self.loading {
            list = list.child(div().p_3().text_size(px(12.)).text_color(t.muted_foreground).child("正在读取分支…"));
        } else if self.rows.is_empty() {
            list = list.child(div().p_3().text_size(px(12.)).text_color(t.muted_foreground).child("没有匹配的分支"));
        }
        let mut prev: Option<RowKind> = None;
        for (i, row) in self.rows.iter().enumerate() {
            if prev != Some(row.kind) {
                list = list.child(section(match row.kind {
                    RowKind::Local => "本地分支",
                    RowKind::Remote => "远程分支",
                    RowKind::Create => "新建",
                }));
                prev = Some(row.kind);
            }
            let (this, r) = (this.clone(), row.clone());
            let (ic, label) = match row.kind {
                RowKind::Create => (IconName::Plus, format!("创建并切换到「{}」", row.name)),
                RowKind::Remote => (IconName::Cloud, row.name.clone()),
                RowKind::Local => (IconName::GitBranch, row.name.clone()),
            };
            list = list.child(
                h_flex().id(SharedString::from(format!("branch-{i}"))).h(px(28.)).px_2().gap_2().items_center().rounded(px(6.)).cursor_pointer()
                    .when(i == self.sel, |d| d.bg(t.list_active))
                    .hover(|d| d.bg(t.list_hover))
                    .on_click(move |_, _, cx| { let r = r.clone(); this.update(cx, |p, cx| p.choose(&r, cx)); })
                    .child(icon(ic, 13., t.muted_foreground))
                    .child(div().flex_1().min_w_0().truncate().text_size(px(12.5)).child(label))
                    .when(row.current, |d| d.child(icon(IconName::Check, 13., t.primary))),
            );
        }
        // 遮罩 + 靠上居中的面板(与命令面板同一套交互:点遮罩 / Esc 关闭,上下键选择)
        div().absolute().inset_0().flex().justify_center().items_start().pt(px(60.))
            .child(div().absolute().inset_0().id("branch-backdrop").on_click(cx.listener(|_, _, _, cx| cx.emit(BranchEvent::Close))))
            .child(
                v_flex().relative().w(px(420.)).max_h(px(400.)).rounded(px(12.)).border_1().border_color(t.border).bg(t.popover).shadow_lg().overflow_hidden()
                    .capture_action(cx.listener(|this, _: &MoveUp, _, cx| this.move_sel(-1, cx)))
                    .capture_action(cx.listener(|this, _: &MoveDown, _, cx| this.move_sel(1, cx)))
                    .capture_action(cx.listener(|_, _: &crate::workbench::Escape, _, cx| cx.emit(BranchEvent::Close)))
                    .child(
                        h_flex().h(px(40.)).px_3().gap_2().items_center().border_b_1().border_color(t.border)
                            .child(icon(IconName::GitBranch, 14., t.muted_foreground))
                            .child(div().flex_1().child(Input::new(&self.input).appearance(false).bordered(false))),
                    )
                    .child(list),
            )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn local() -> Vec<Branch> {
        vec![Branch { name: "dev".into(), current: false }, Branch { name: "main".into(), current: true }]
    }

    #[::core::prelude::v1::test]
    fn 当前分支排在最前_远程在后() {
        let rows = build_rows(&local(), &["origin/feature/x".into()], "");
        let names: Vec<_> = rows.iter().map(|r| r.name.as_str()).collect();
        assert_eq!(names, ["main", "dev", "origin/feature/x"]);
        assert_eq!(rows[2].kind, RowKind::Remote);
    }

    #[::core::prelude::v1::test]
    fn 搜索不分大小写_无同名时给出新建() {
        let rows = build_rows(&local(), &[], "DE");
        assert_eq!(rows.iter().map(|r| r.name.as_str()).collect::<Vec<_>>(), ["dev", "DE"]);
        assert_eq!(rows[1].kind, RowKind::Create);
    }

    #[::core::prelude::v1::test]
    fn 已存在同名或名字非法不给新建() {
        assert!(build_rows(&local(), &[], "dev").iter().all(|r| r.kind != RowKind::Create));
        assert!(build_rows(&local(), &[], "bad name").iter().all(|r| r.kind != RowKind::Create));
    }
}
