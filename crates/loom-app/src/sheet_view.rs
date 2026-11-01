//! 表格文件视图(csv / tsv / xlsx / xls / ods):在编辑器区域里以网格显示,对标 OhMyAgent 的表格预览。
//! 本文件放状态、后台读取与整体布局;网格见 `sheet_grid.rs`,编辑栏与底栏见 `sheet_bar.rs`,
//! 列筛选面板见 `sheet_filter_panel.rs`,查找条见 `sheet_find.rs`。
//!
//! @author ixxxxoooo

use gpui_kit::component::input::InputState;
use gpui_kit::component::v_flex;
use gpui_kit::*;
use loom_sheet::{FilterState, Kind, Match, Sheet};
use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::Arc;

/// 估算列宽的最小 / 最大值与采样行数。
const MIN_COL_W: f32 = 64.;
const MAX_COL_W: f32 = 360.;
const SAMPLE_ROWS: usize = 120;

actions!(sheet, [ToggleFind, FindNext, FindPrev, CloseFind]);

/// 表格视图向宿主发出的事件。
#[derive(Debug, Clone)]
pub enum SheetEvent {
    /// 请改用文本编辑器打开这个文件(分隔文本才有)
    OpenAsText,
}

impl EventEmitter<SheetEvent> for SheetView {}

/// 读取阶段。
#[derive(Debug, Clone, PartialEq)]
pub enum Phase {
    Loading,
    Ready,
    Failed(String),
}

/// 表格视图。
pub struct SheetView {
    pub(crate) path: PathBuf,
    pub(crate) kind: Kind,
    pub(crate) phase: Phase,
    /// 工作表名(csv 只有一个)
    pub(crate) names: Vec<String>,
    pub(crate) active: usize,
    pub(crate) sheet: Arc<Sheet>,
    pub(crate) filter: FilterState,
    /// 筛选模式:列头显示筛选按钮
    pub(crate) filter_mode: bool,
    /// 筛选 / 排序后的可见行(原始行号)
    pub(crate) visible: Arc<Vec<usize>>,
    pub(crate) widths: Arc<Vec<f32>>,
    /// 选中的单元格(原始行号, 列)
    pub(crate) selected: Option<(usize, usize)>,
    pub(crate) scroll: UniformListScrollHandle,
    pub(crate) focus: FocusHandle,
    /// 打开着的列筛选面板:(列, 点击位置)
    pub(crate) panel: Option<(usize, Point<Pixels>)>,
    pub(crate) find_open: bool,
    pub(crate) find_input: Entity<InputState>,
    pub(crate) matches: Arc<Vec<Match>>,
    pub(crate) match_set: Arc<HashSet<(usize, usize)>>,
    pub(crate) current_match: usize,
    pub(crate) _subs: Vec<Subscription>,
}

/// 路径是不是表格文件。
pub fn is_sheet(path: &Path) -> bool {
    loom_sheet::kind_of(path).is_some()
}

impl SheetView {
    /// 打开一个表格文件(后台读取,读完刷新)。
    pub fn new(path: PathBuf, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let kind = loom_sheet::kind_of(&path).unwrap_or(Kind::Delimited);
        let find_input = cx.new(|cx| InputState::new(window, cx).placeholder("在表格中查找(回车下一处)"));
        let sub = cx.subscribe_in(&find_input, window, |this: &mut Self, _, ev: &gpui_kit::component::input::InputEvent, _, cx| match ev {
            gpui_kit::component::input::InputEvent::Change => this.refresh_matches(cx),
            gpui_kit::component::input::InputEvent::PressEnter { .. } => this.step_match(1, cx),
            _ => {}
        });
        let mut me = Self {
            path, kind, phase: Phase::Loading, names: Vec::new(), active: 0, sheet: Arc::default(), filter: FilterState::default(), filter_mode: false,
            visible: Arc::default(), widths: Arc::default(), selected: None, scroll: UniformListScrollHandle::new(), focus: cx.focus_handle(), panel: None,
            find_open: false, find_input, matches: Arc::default(), match_set: Arc::default(), current_match: 0, _subs: vec![sub],
        };
        me.load(0, true, cx);
        me
    }

    /// 后台读取第 `index` 张工作表;`first` 为真时同时取工作表名列表。
    pub(crate) fn load(&mut self, index: usize, first: bool, cx: &mut Context<Self>) {
        self.phase = Phase::Loading;
        let (path, kind) = (self.path.clone(), self.kind);
        cx.spawn(async move |this, cx| {
            let res = cx
                .background_spawn(async move {
                    match kind {
                        Kind::Delimited => {
                            let bytes = std::fs::read(&path).map_err(|e| format!("读取失败:{e}"))?;
                            let name = path.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default();
                            let tsv = path.extension().is_some_and(|e| e.eq_ignore_ascii_case("tsv"));
                            let sheet = loom_sheet::csv::parse(&bytes, &name, tsv);
                            Ok((vec![name], sheet))
                        }
                        Kind::Workbook => {
                            let names = if first { loom_sheet::xlsx::sheet_names(&path)? } else { Vec::new() };
                            let sheet = loom_sheet::xlsx::read_sheet(&path, index)?;
                            Ok((names, sheet))
                        }
                    }
                })
                .await;
            let _ = this.update(cx, |this, cx| {
                match res {
                    Ok((names, sheet)) => {
                        if !names.is_empty() {
                            this.names = names;
                        }
                        this.active = index;
                        this.set_sheet(sheet, cx);
                    }
                    Err(e) => this.phase = Phase::Failed(e),
                }
                cx.notify();
            });
        })
        .detach();
    }

    /// 换上新读到的工作表:重置筛选、选中与列宽。
    fn set_sheet(&mut self, sheet: Sheet, cx: &mut Context<Self>) {
        self.widths = Arc::new(estimate_widths(&sheet));
        self.sheet = Arc::new(sheet);
        self.filter = FilterState::default();
        self.selected = None;
        self.panel = None;
        self.phase = Phase::Ready;
        self.recompute(cx);
    }

    /// 筛选 / 排序变了:重算可见行与查找结果。
    pub(crate) fn recompute(&mut self, cx: &mut Context<Self>) {
        self.visible = Arc::new(self.filter.visible_rows(&self.sheet));
        self.refresh_matches(cx);
        cx.notify();
    }

    /// 切换工作表。
    pub(crate) fn select_sheet(&mut self, index: usize, cx: &mut Context<Self>) {
        if index != self.active && index < self.names.len() {
            self.load(index, false, cx);
        }
    }
}

/// 按前若干行内容估算各列宽度(中日韩字符按 2 个字符宽)。
fn estimate_widths(sheet: &Sheet) -> Vec<f32> {
    (0..sheet.column_count)
        .map(|c| {
            let chars = sheet.rows.iter().take(SAMPLE_ROWS).map(|r| r.get(c).map_or(0., |cell| display_width(&cell.text))).fold(0., f32::max);
            (chars * 7.5 + 20.).clamp(MIN_COL_W, MAX_COL_W)
        })
        .collect()
}

/// 文本的显示宽度(字符数,全角字符算 2)。
fn display_width(s: &str) -> f32 {
    s.chars().take(60).map(|c| if c.is_ascii() { 1. } else { 2. }).sum()
}

impl Render for SheetView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        use gpui_kit::component::ActiveTheme;
        let t = cx.theme().clone();
        let body = match self.phase.clone() {
            Phase::Loading => crate::ui::empty_state(gpui_kit::assets::IconName::LoaderCircle, "正在读取表格…", "", cx).into_any_element(),
            Phase::Failed(e) => crate::ui::empty_state(gpui_kit::assets::IconName::TriangleAlert, "无法打开表格", &e, cx).into_any_element(),
            Phase::Ready => self.ready_view(window, cx),
        };
        v_flex().id("sheet-view").key_context("SheetView").track_focus(&self.focus).size_full().min_h_0().bg(t.background)
            .on_action(cx.listener(|this, _: &ToggleFind, window, cx| this.toggle_find(window, cx)))
            .on_action(cx.listener(|this, _: &FindNext, _, cx| this.step_match(1, cx)))
            .on_action(cx.listener(|this, _: &FindPrev, _, cx| this.step_match(-1, cx)))
            .on_action(cx.listener(|this, _: &CloseFind, window, cx| this.close_find(window, cx)))
            .child(body)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[::core::prelude::v1::test]
    fn 列宽按内容估算并限制范围() {
        use loom_sheet::Cell;
        let sheet = Sheet::new("s", vec![vec![Cell::text("a"), Cell::text("很长很长很长很长很长很长很长很长很长很长很长很长很长很长很长很长"), Cell::text("x".repeat(200))]], false);
        let w = estimate_widths(&sheet);
        assert_eq!(w[0], MIN_COL_W);
        assert!(w[1] > MIN_COL_W && w[1] <= MAX_COL_W);
        assert_eq!(w[2], MAX_COL_W);
    }
}
