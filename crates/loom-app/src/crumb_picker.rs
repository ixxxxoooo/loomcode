//! 面包屑下拉(对标 VS Code 面包屑 / OhMyAgent `BreadcrumbMenu`):点路径里的某一段,弹出该段所在目录的内容,
//! 可搜索、点目录进入、点文件在编辑器里打开、顶部「返回上一级」,当前项打勾。
//!
//! @author ixxxxoooo

use crate::file_icons::file_icon_img;
use crate::file_lang::{Node, read_dir_sorted};
use crate::ui::icon;
use gpui_kit::assets::IconName;
use gpui_kit::component::input::{Input, InputEvent, InputState, MoveDown, MoveUp};
use gpui_kit::component::{ActiveTheme, h_flex, v_flex};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;
use std::path::{Path, PathBuf};

/// 下拉发给宿主的事件。
pub enum CrumbEvent {
    /// 点了一个文件
    Open(PathBuf),
    Close,
}

impl EventEmitter<CrumbEvent> for CrumbPicker {}

/// 一行:「返回上一级」或目录 / 文件。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CrumbRow {
    pub name: String,
    pub path: PathBuf,
    pub is_dir: bool,
    pub back: bool,
}

/// 按查询过滤并加上「返回上一级」。纯函数:不分大小写做子串匹配;进入了子目录(`dir != start`)才有「返回」。
pub fn build_rows(entries: &[Node], dir: &Path, start: &Path, query: &str) -> Vec<CrumbRow> {
    let q = query.trim().to_lowercase();
    let mut rows = Vec::new();
    if dir != start && let Some(parent) = dir.parent() {
        rows.push(CrumbRow { name: "返回上一级".into(), path: parent.to_path_buf(), is_dir: true, back: true });
    }
    rows.extend(
        entries
            .iter()
            .filter(|n| q.is_empty() || n.name.to_lowercase().contains(&q))
            .map(|n| CrumbRow { name: n.name.clone(), path: n.path.clone(), is_dir: n.is_dir, back: false }),
    );
    rows
}

pub struct CrumbPicker {
    input: Entity<InputState>,
    start: PathBuf,
    dir: PathBuf,
    /// 当前打开的文件(打勾)
    current: PathBuf,
    entries: Vec<Node>,
    rows: Vec<CrumbRow>,
    sel: usize,
    /// 弹出位置(窗口坐标)
    pub pos: Point<Pixels>,
    _sub: Subscription,
}

impl CrumbPicker {
    /// `dir` 为要列出的目录,`current` 为当前文件,`pos` 为弹出位置。
    pub fn new(dir: PathBuf, current: PathBuf, pos: Point<Pixels>, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let input = cx.new(|cx| InputState::new(window, cx).placeholder("搜索当前目录"));
        let sub = cx.subscribe_in(&input, window, |this: &mut Self, _, ev: &InputEvent, window, cx| match ev {
            InputEvent::Change => this.refilter(cx),
            InputEvent::PressEnter { .. } => this.accept(window, cx),
            _ => {}
        });
        input.update(cx, |i, cx| i.focus(window, cx));
        let entries = read_dir_sorted(&dir);
        let mut me = Self { input, start: dir.clone(), dir, current, entries, rows: Vec::new(), sel: 0, pos, _sub: sub };
        me.rows = build_rows(&me.entries, &me.dir, &me.start, "");
        // 默认选中当前文件所在行
        me.sel = me.rows.iter().position(|r| r.path == me.current).unwrap_or(0);
        me
    }

    fn refilter(&mut self, cx: &mut Context<Self>) {
        let q = self.input.read(cx).value().to_string();
        self.rows = build_rows(&self.entries, &self.dir, &self.start, &q);
        self.sel = 0;
        cx.notify();
    }

    fn choose(&mut self, row: &CrumbRow, window: &mut Window, cx: &mut Context<Self>) {
        if row.is_dir {
            self.dir = row.path.clone();
            self.entries = read_dir_sorted(&self.dir);
            self.input.update(cx, |i, cx| i.set_value(String::new(), window, cx));
            self.rows = build_rows(&self.entries, &self.dir, &self.start, "");
            self.sel = 0;
            cx.notify();
        } else {
            cx.emit(CrumbEvent::Open(row.path.clone()));
        }
    }

    fn accept(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(row) = self.rows.get(self.sel).cloned() {
            self.choose(&row, window, cx);
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

impl Render for CrumbPicker {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let t = cx.theme().clone();
        let this = cx.entity();
        let mut list = v_flex().id("crumb-list").max_h(px(300.)).overflow_y_scroll().p_1();
        if self.rows.is_empty() {
            list = list.child(div().p_3().text_size(px(12.)).text_color(t.muted_foreground).child(if self.entries.is_empty() { "空文件夹" } else { "没有匹配项" }));
        }
        for (i, row) in self.rows.iter().enumerate() {
            let (this, r) = (this.clone(), row.clone());
            let is_current = row.path == self.current;
            list = list.child(
                h_flex().id(SharedString::from(format!("crumb-{i}"))).h(px(26.)).px_2().gap_2().items_center().rounded(px(6.)).cursor_pointer()
                    .when(i == self.sel, |d| d.bg(t.list_active))
                    .hover(|d| d.bg(t.list_hover))
                    .on_click(move |_, window, cx| { let r = r.clone(); this.update(cx, |p, cx| p.choose(&r, window, cx)); })
                    .child(if row.back { icon(IconName::ChevronLeft, 13., t.muted_foreground).into_any_element() } else { file_icon_img(&row.name, row.is_dir, false, 15.).into_any_element() })
                    .child(div().flex_1().min_w_0().truncate().text_size(px(12.5)).child(row.name.clone()))
                    .when(row.is_dir && !row.back, |d| d.child(icon(IconName::ChevronRight, 11., t.muted_foreground)))
                    .when(is_current, |d| d.child(icon(IconName::Check, 13., t.primary))),
            );
        }
        deferred(
            anchored().position(self.pos).anchor(Anchor::TopLeft).snap_to_window_with_margin(px(8.)).child(
                v_flex().id("crumb-card").occlude().w(px(280.)).rounded(px(10.)).border_1().border_color(t.border).bg(t.popover).shadow_lg().overflow_hidden()
                    .on_mouse_down_out(cx.listener(|_, _, _, cx| cx.emit(CrumbEvent::Close)))
                    .capture_action(cx.listener(|this, _: &MoveUp, _, cx| this.move_sel(-1, cx)))
                    .capture_action(cx.listener(|this, _: &MoveDown, _, cx| this.move_sel(1, cx)))
                    .capture_action(cx.listener(|_, _: &crate::workbench::Escape, _, cx| cx.emit(CrumbEvent::Close)))
                    .child(
                        h_flex().h(px(34.)).px_3().gap_2().items_center().border_b_1().border_color(t.border)
                            .child(icon(IconName::Search, 13., t.muted_foreground))
                            .child(div().flex_1().child(Input::new(&self.input).appearance(false).bordered(false))),
                    )
                    .child(list),
            ),
        )
        .with_priority(6)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn n(name: &str, is_dir: bool) -> Node {
        Node { name: name.into(), path: PathBuf::from("/p").join(name), is_dir }
    }

    #[::core::prelude::v1::test]
    fn 过滤不分大小写_子目录里才有返回() {
        let entries = vec![n("src", true), n("Main.go", false), n("go.mod", false)];
        let start = Path::new("/p");
        let rows = build_rows(&entries, start, start, "GO");
        assert_eq!(rows.iter().map(|r| r.name.as_str()).collect::<Vec<_>>(), ["Main.go", "go.mod"]);
        let rows = build_rows(&entries, Path::new("/p/src"), start, "");
        assert!(rows[0].back);
        assert_eq!(rows[0].path, Path::new("/p"));
        assert_eq!(rows.len(), 4);
    }
}
