//! 「图形」区的界面:标题行(折叠、所有分支开关、刷新)、提交行(泳道图 + 标题 + 引用徽标 + 作者与时间),
//! 点提交展开详情与改动的文件,点文件在行内看 diff。泳道线用 `canvas` + 路径绘制。
//!
//! @author ixxxxoooo

use crate::diff;
use crate::file_icons::file_icon_img;
use crate::scm::ScmView;
use crate::ui::{MONO, icon};
use gpui_kit::assets::IconName;
use gpui_kit::component::button::{Button, ButtonVariants};
use gpui_kit::component::{ActiveTheme, Selectable as _, Sizable, h_flex, v_flex};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;
use loom_core::git_log::{self, CommitFile, GraphRow};
use std::path::PathBuf;

/// 提交行高。
const ROW_H: f32 = 24.;
/// 泳道列宽。
const LANE_W: f32 = 12.;
/// 泳道最多画多少列(再多的分支挤在一起没有意义,超出的列贴着最右列)。
const MAX_LANES: usize = 8;

/// 一行里泳道线的形状(只含整数,渲染时每帧重建很便宜)。
struct Shape {
    column: usize,
    color: usize,
    merge: bool,
    head: bool,
    /// 穿过本行的泳道:(列, 颜色)
    pass: Vec<(usize, usize)>,
    /// 汇入提交点的泳道:(列, 颜色)
    incoming: Vec<(usize, usize)>,
    /// 从提交点分出的泳道:(列, 颜色)
    outgoing: Vec<(usize, usize)>,
}

fn shape(row: &GraphRow) -> Shape {
    let pass = row.top.iter().enumerate().filter_map(|(i, l)| l.as_ref().filter(|_| !row.incoming.contains(&i)).map(|l| (i, l.color))).collect();
    let incoming = row.incoming.iter().filter_map(|&i| row.top.get(i).and_then(|l| l.as_ref()).map(|l| (i, l.color))).collect();
    let outgoing = row.outgoing.iter().filter_map(|&i| row.bottom.get(i).and_then(|l| l.as_ref()).map(|l| (i, l.color))).collect();
    Shape { column: row.column, color: row.color, merge: row.commit.parents.len() > 1, head: row.commit.refs.iter().any(|r| r.starts_with("HEAD")), pass, incoming, outgoing }
}

/// 画一条从 (x0,y0) 到 (x1,y1) 的线:同列画直线,换列画一段平滑的曲线。
fn stroke(window: &mut Window, from: Point<Pixels>, to: Point<Pixels>, color: Hsla) {
    let mut b = PathBuilder::stroke(px(1.6));
    b.move_to(from);
    if (from.x - to.x).abs() < px(0.5) {
        b.line_to(to);
    } else {
        let mid = (from.y + to.y) / 2.;
        b.cubic_bezier_to(to, point(from.x, mid), point(to.x, mid));
    }
    if let Ok(path) = b.build() {
        window.paint_path(path, color);
    }
}

/// 泳道图元素。
fn lanes_canvas(s: Shape, width: f32, palette: [Hsla; 6], bg: Hsla) -> impl IntoElement {
    canvas(
        |_, _, _| (),
        move |bounds, _, window, _| {
            let x_of = |col: usize| bounds.origin.x + px(LANE_W / 2. + col.min(MAX_LANES - 1) as f32 * LANE_W);
            let (top, mid, bottom) = (bounds.origin.y, bounds.origin.y + px(ROW_H / 2.), bounds.origin.y + px(ROW_H));
            let color = |c: usize| palette[c % palette.len()];
            for &(col, c) in &s.pass {
                stroke(window, point(x_of(col), top), point(x_of(col), bottom), color(c));
            }
            for &(col, c) in &s.incoming {
                stroke(window, point(x_of(col), top), point(x_of(s.column), mid), color(c));
            }
            for &(col, c) in &s.outgoing {
                stroke(window, point(x_of(s.column), mid), point(x_of(col), bottom), color(c));
            }
            // 提交点:普通提交实心圆,合并提交空心圆,HEAD 再加一圈
            let (cx, r) = (x_of(s.column), if s.head { 5. } else { 4. });
            let dot = Bounds::new(point(cx - px(r), mid - px(r)), size(px(r * 2.), px(r * 2.)));
            let col = color(s.color);
            if s.merge {
                window.paint_quad(quad(dot, px(r), bg, 1.8, col, BorderStyle::default()));
            } else if s.head {
                window.paint_quad(quad(dot, px(r), col, 2., bg, BorderStyle::default()));
                let ring = Bounds::new(point(cx - px(r + 1.5), mid - px(r + 1.5)), size(px((r + 1.5) * 2.), px((r + 1.5) * 2.)));
                window.paint_quad(quad(ring, px(r + 1.5), transparent_black(), 1.4, col, BorderStyle::default()));
            } else {
                window.paint_quad(quad(dot, px(r), col, 0., transparent_black(), BorderStyle::default()));
            }
        },
    )
    .w(px(width))
    .h(px(ROW_H))
    .flex_none()
}

/// 引用徽标的颜色:HEAD / 本地分支主题色,远程分支蓝,tag 黄。
fn ref_color(r: &str, cx: &App) -> Hsla {
    let t = cx.theme();
    if r.starts_with("tag: ") {
        t.warning
    } else if r.starts_with("HEAD") {
        t.primary
    } else if r.contains('/') {
        t.info
    } else {
        t.success
    }
}

/// 状态字母颜色(与文件行一致)。
fn status_color(c: char, cx: &App) -> Hsla {
    let t = cx.theme();
    match c {
        'M' => t.warning,
        'A' => t.success,
        'D' => t.danger,
        'R' | 'C' => t.info,
        _ => t.muted_foreground,
    }
}

impl ScmView {
    /// 整个「图形」区。没有提交历史时返回 `None`。
    pub(crate) fn graph_section(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        let g = &self.graph;
        let t = cx.theme().clone();
        let open = g.open;
        let header = h_flex().id("scm-graph-head").group("scm-graph-head").h(px(26.)).px_2().gap_1().items_center().cursor_pointer().flex_none()
            .hover(|d| d.bg(t.list_hover))
            .on_click(cx.listener(|this, _, _, cx| this.toggle_graph(cx)))
            .child(icon(if open { IconName::ChevronDown } else { IconName::ChevronRight }, 12., t.muted_foreground))
            .child(div().flex_1().min_w_0().truncate().text_size(px(11.5)).font_weight(FontWeight::SEMIBOLD).child(match &g.file { Some(f) => format!("图形 · 文件历史:{}", f.rsplit('/').next().unwrap_or(f)), None => "图形".to_string() }))
            .when(g.file.is_some(), |d| d.child(Button::new("scm-graph-unfile").xsmall().ghost().icon(IconName::X).tooltip("取消文件过滤,显示整个仓库的历史").on_click(cx.listener(|this, _, _, cx| { cx.stop_propagation(); this.graph.file = None; this.graph.sig = None; this.graph.loaded = false; this.reload_graph(cx); }))))
            .child(
                h_flex().invisible().group_hover("scm-graph-head", |d| d.visible()).gap_0p5()
                    .child(Button::new("scm-graph-all").xsmall().ghost().icon(IconName::GitBranch).tooltip(if g.all { "显示所有分支(点击只看当前分支)" } else { "仅当前分支(点击显示所有分支)" }).selected(g.all)
                        .on_click(cx.listener(|this, _, _, cx| { cx.stop_propagation(); this.toggle_graph_all(cx) })))
                    .child(Button::new("scm-graph-refresh").xsmall().ghost().icon(IconName::RefreshCw).tooltip("刷新图形")
                        .on_click(cx.listener(|this, _, _, cx| { cx.stop_propagation(); this.graph.sig = None; this.reload_graph(cx) }))),
            );
        let mut body = v_flex().w_full();
        if open {
            if let Some(e) = &g.error {
                body = body.child(div().px_3().py_2().text_size(px(11.5)).text_color(t.danger).child(e.clone()));
            } else if !g.loaded {
                body = body.child(div().px_3().py_2().text_size(px(11.5)).text_color(t.muted_foreground).child("正在读取提交历史…"));
            } else if g.rows.is_empty() {
                body = body.child(div().px_3().py_2().text_size(px(11.5)).text_color(t.muted_foreground).child("还没有提交"));
            } else {
                let lanes = g.rows.iter().map(GraphRow::lane_count).max().unwrap_or(1).clamp(1, MAX_LANES);
                let width = lanes as f32 * LANE_W + 6.;
                let palette = [t.info, t.success, t.warning, t.primary, t.danger, t.cyan];
                let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_secs() as i64);
                for row in &g.rows {
                    body = body.child(self.commit_row(row, width, palette, now, cx));
                }
                if g.has_more {
                    body = body.child(
                        div().px_3().py_1p5().child(Button::new("scm-graph-more").xsmall().ghost().label(if g.loading { "加载中…" } else { "加载更多提交" }).on_click(cx.listener(|this, _, _, cx| this.graph_more(cx)))),
                    );
                }
            }
        }
        Some(v_flex().w_full().border_t_1().border_color(t.border.opacity(0.5)).child(header).child(body).into_any_element())
    }

    /// 一个提交行 + (展开时的)详情。
    fn commit_row(&self, row: &GraphRow, lane_w: f32, palette: [Hsla; 6], now: i64, cx: &mut Context<Self>) -> AnyElement {
        let t = cx.theme().clone();
        let c = &row.commit;
        let expanded = self.graph.expanded.as_deref() == Some(c.hash.as_str());
        let hash = c.hash.clone();
        let refs: Vec<String> = c.refs.iter().filter(|r| r.as_str() != "HEAD" || c.refs.len() == 1).cloned().collect();
        let tip = format!("{}\n{} <{}>\n{}", c.subject, c.author, c.email, git_log::format_datetime(c.time, loom_diag::local_tz_offset()));
        let mut col = v_flex().w_full();
        col = col.child(
            h_flex().id(SharedString::from(format!("scm-commit-{}", c.hash))).h(px(ROW_H)).w_full().min_w_0().pl_1().pr_2().gap_1p5().items_center().cursor_pointer()
                .when(expanded, |d| d.bg(t.list_active))
                .when(!expanded, |d| d.hover(|d| d.bg(t.list_hover)))
                .tooltip(move |window, cx| gpui_kit::component::tooltip::Tooltip::new(tip.clone()).build(window, cx))
                .on_click(cx.listener({
                    let h = hash.clone();
                    move |this, _, _, cx| this.toggle_commit(&h, cx)
                }))
                .child(lanes_canvas(shape(row), lane_w, palette, t.background))
                .child(div().flex_1().min_w_0().truncate().text_size(px(12.5)).child(c.subject.clone()))
                .children(refs.iter().take(3).map(|r| {
                    let color = ref_color(r, cx);
                    let label = r.strip_prefix("tag: ").unwrap_or(r).replace("HEAD -> ", "");
                    div().flex_none().max_w(px(90.)).truncate().px_1().rounded(px(3.)).bg(color.opacity(0.16)).text_size(px(10.)).text_color(color).child(label)
                }))
                .when(refs.len() > 3, |d| d.child(div().flex_none().text_size(px(10.)).text_color(t.muted_foreground).child(format!("+{}", refs.len() - 3))))
                .child(div().flex_none().text_size(px(10.5)).text_color(t.muted_foreground).child(git_log::relative_time(c.time, now))),
        );
        if expanded {
            col = col.child(self.commit_detail(row, cx));
        }
        col.into_any_element()
    }

    /// 展开的提交详情:作者与时间、完整消息、复制哈希、改动的文件。
    fn commit_detail(&self, row: &GraphRow, cx: &mut Context<Self>) -> AnyElement {
        let t = cx.theme().clone();
        let c = &row.commit;
        let files = self.graph.files.get(&c.hash);
        let hash = c.hash.clone();
        let mut d = v_flex().w_full().pl(px(12.)).pr_2().pb_2().gap_1()
            .child(
                h_flex().gap_2().items_center().flex_wrap()
                    .child(div().text_size(px(11.5)).font_weight(FontWeight::MEDIUM).child(c.author.clone()))
                    .child(div().text_size(px(11.)).text_color(t.muted_foreground).child(git_log::format_datetime(c.time, loom_diag::local_tz_offset())))
                    .child(
                        Button::new(SharedString::from(format!("scm-copy-{}", c.hash))).xsmall().ghost().icon(IconName::Copy).label(c.short().to_string()).tooltip("复制完整哈希")
                            .on_click(cx.listener(move |this, _, _, cx| this.copy_hash(&hash, cx))),
                    ),
            );
        d = d.child(div().text_size(px(12.)).child(c.subject.clone()));
        if !c.body.is_empty() {
            d = d.child(div().text_size(px(11.5)).text_color(t.muted_foreground).child(c.body.clone()));
        }
        d = match files {
            None | Some(None) => d.child(div().text_size(px(11.)).text_color(t.muted_foreground).child("正在读取改动的文件…")),
            Some(Some(list)) if list.is_empty() => d.child(div().text_size(px(11.)).text_color(t.muted_foreground).child("这次提交没有文件改动")),
            Some(Some(list)) => d.children(list.iter().map(|f| self.commit_file_row(&c.hash, f, cx))),
        };
        d.into_any_element()
    }

    /// 提交里的一个文件:状态字母、文件名、目录;点击展开该次提交里的 diff。
    fn commit_file_row(&self, hash: &str, f: &CommitFile, cx: &mut Context<Self>) -> AnyElement {
        let t = cx.theme().clone();
        let (dir, name) = crate::right_panel::split_path(&f.path);
        let color = status_color(f.status, cx);
        let key = format!("{hash}:{}", f.path);
        let open = self.graph.diffs.get(&key);
        let file = f.clone();
        let h = hash.to_string();
        let lang_path = PathBuf::from(&f.path);
        // 工作区里还在的文件:点击在编辑器打开(单击预览、双击固定);已删除的文件没法打开,点击改为展开差异
        let exists = f.status != 'D' && self.top.as_ref().is_some_and(|top| PathBuf::from(top).join(&f.path).is_file());
        let (rel, diff_h, diff_file) = (f.path.clone(), hash.to_string(), f.clone());
        let diff_open = open.is_some();
        v_flex().w_full()
            .child(
                h_flex().id(SharedString::from(format!("scm-cf-{key}"))).h(px(22.)).w_full().min_w_0().gap_1p5().items_center().cursor_pointer().px_1().rounded(px(4.))
                    .hover(|d| d.bg(t.list_hover))
                    .on_click(cx.listener(move |this, ev: &ClickEvent, _, cx| {
                        if exists {
                            this.open_file(&rel, ev.click_count() >= 2, cx);
                        } else {
                            this.toggle_commit_diff(&h, &file, cx);
                        }
                    }))
                    .child(file_icon_img(&name, false, false, 14.))
                    .child(div().flex_none().max_w(px(170.)).truncate().text_size(px(12.)).text_color(color).when(f.status == 'D', |d| d.line_through()).child(name))
                    .child(div().flex_1().min_w_0().truncate().text_size(px(10.5)).text_color(t.muted_foreground).child(dir))
                    .child(
                        Button::new(SharedString::from(format!("scm-cf-diff-{key}"))).xsmall().ghost()
                            .icon(if diff_open { IconName::ChevronDown } else { IconName::ChevronRight }).tooltip(if diff_open { "收起差异" } else { "展开这次提交的差异" })
                            .on_click(cx.listener(move |this, _, _, cx| { cx.stop_propagation(); this.toggle_commit_diff(&diff_h, &diff_file, cx); })),
                    )
                    .child(div().w(px(14.)).flex_none().text_size(px(11.)).font_family(MONO).font_weight(FontWeight::BOLD).text_color(color).child(f.status.to_string())),
            )
            .when_some(open, |d, state| {
                d.child(div().mb_1().rounded(px(6.)).border_1().border_color(t.border).overflow_hidden().child(match state {
                    None => div().p_2().text_size(px(11.)).text_color(t.muted_foreground).child("正在计算差异…"),
                    Some((lines, _, _)) if lines.is_empty() => div().p_2().text_size(px(11.)).text_color(t.muted_foreground).child("没有可显示的文本差异(二进制、过大或空文件)"),
                    Some((lines, _, _)) => diff::render_flat(lines, 300, crate::file_tree::language_for(&lang_path), cx),
                }))
            })
            .into_any_element()
    }
}
